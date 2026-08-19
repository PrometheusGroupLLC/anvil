use crate::{carry_retained_temp_dir, retained_temp_dir, RetainedTempDir};
use anvil_core::domain::playbook::composite_registry::CompositePlaybookRegistry;
use anvil_core::domain::playbook::hearth_registry::HearthPlaybookRegistry;
use anvil_core::domain::playbook::registry::{driven_candidates, SeedPlaybookRegistry};
use anvil_core::domain::telemetry_salt::{ACTOR_HASH_HEX_LEN, UNKNOWN_CONVERSATION_HASH};
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{async_step_def, check_def, step_def, StepDef};
use prost::Message;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

fn parse_csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn parse_bool_param(raw: &str) -> Result<bool, String> {
    match raw {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("Expected 'true' or 'false', got '{}'", raw)),
    }
}

/// Grab a free loopback port (TOCTOU, covered by the readiness gate below).
fn free_port() -> Result<u16, String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("Failed to find free port: {}", e))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("Failed to get port: {}", e))?
        .port();
    drop(listener);
    Ok(port)
}

/// Throwaway append target for anvil's UNCONDITIONAL fleet telemetry
/// (`~/.anvil/anvil-telemetry.jsonl`). Anvil has NO opt-in gate — consent lives in
/// Foundry's emitter — so any Brine scenario driving the real engine/hooks records
/// a row. Redirecting `ANVIL_TELEMETRY_DIR` to a temp dir keeps those writes out of
/// the developer's real `~/.anvil`. Mirrors Lore's `LORE_TELEMETRY_DIR` /
/// Forge's `FORGE_TELEMETRY_DIR`.
fn brine_fleet_telemetry_dir() -> PathBuf {
    std::env::temp_dir().join("anvil-brine-telemetry")
}

fn anvil_engine_command(binary: &Path) -> Command {
    let mut command = Command::new(binary);
    command.env("ANVIL_RENDEZVOUS_DISABLE", "1");
    command.env("ANVIL_TELEMETRY_DIR", brine_fleet_telemetry_dir());
    // P4 exercises the default-off posture through each request hearth's
    // engine-flags.env. Do not let a developer's process-wide rollout override
    // leak into Brine subprocesses and make the default scenario non-hermetic.
    command.env_remove("ANVIL_ENFORCE_CLAIMED_EVIDENCE");
    // The FREE P4 control deliberately carries an obligation to prove this
    // transition gate never applies to FREE machines. Keep the separate T-EEC-1
    // authoring gates from rejecting that fixture during registry loading.
    command.env_remove("ANVIL_ENFORCE_EVIDENCE_OBLIGATION");
    command.env_remove("ANVIL_ENFORCE_MEASUREMENT_DEFINITION");
    // K5 supervision-bind hardening (R10 bindability precondition + R9
    // terminal-resolve no-op) is dark-by-default. Strip the flag from every
    // default engine subprocess so the existing suites stay byte-identical
    // (A7); only the K5 features opt in via a scenario-scoped engine-flags.env.
    command.env_remove("ANVIL_K5_BIND");
    // Change-record shadow recording and the authority flip are per-hearth, read
    // from each request hearth's engine-flags.env. A process-env value for
    // either wins over every hearth's file (the documented all-lanes override),
    // so a developer's shell would make the per-hearth scenarios pass or fail
    // for a reason that has nothing to do with the flip.
    command.env_remove("ANVIL_CHANGE_RECORD_SHADOW");
    command.env_remove("ANVIL_CHANGE_RECORD_AUTHORITY");
    command
}

/// How long a spawned engine gets to answer HealthCheck before the harness
/// gives up. The old bound was `for _ in 0..100` with a 50ms sleep — nominally
/// 5s, but ACTUALLY less, because each failed `connect()` also consumes wall
/// clock and the loop counted iterations rather than time. Solo that is ample
/// (measured: a debug engine answers in well under a second). Under a full
/// workspace run, with dozens of debug-build engines starting concurrently, it
/// is marginal — which is exactly the shape of a suite that passes two runs in
/// three.
///
/// So the budget is TIME, generous, and tunable for slower machines.
const GRPC_READY_BUDGET: std::time::Duration = std::time::Duration::from_secs(30);

fn grpc_ready_budget() -> std::time::Duration {
    std::env::var("ANVIL_TEST_ENGINE_READY_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(std::time::Duration::from_millis)
        .unwrap_or(GRPC_READY_BUDGET)
}

/// Poll a real gRPC HealthCheck until it succeeds or the budget expires. Runs
/// the poll on a SEPARATE thread that owns its own runtime — this helper is
/// called from steps already running on the brine tokio runtime, where a nested
/// block_on would panic.
///
/// Returns the elapsed wait on success so callers can report it. A wait that is
/// merely SLOW is the early warning for a wait that will later be FATAL, and
/// under the old code that signal did not exist: a run at 4.9s and a run at 5.1s
/// were reported identically as pass and fail, with nothing in between to show
/// the margin closing.
fn wait_until_grpc_ready_detailed(port: u16) -> Result<std::time::Duration, String> {
    let budget = grpc_ready_budget();
    let started = std::time::Instant::now();
    let outcome = std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => return Err(format!("could not build readiness runtime: {e}")),
        };
        rt.block_on(async {
            let addr = format!("http://127.0.0.1:{}", port);
            let deadline = std::time::Instant::now() + budget;
            let mut last: Option<String> = None;
            while std::time::Instant::now() < deadline {
                match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(
                    addr.clone(),
                )
                .await
                {
                    Ok(mut client) => {
                        let req = crate::surfaced(anvil_engine::proto::HealthCheckRequest {});
                        match client.health_check(req).await {
                            Ok(_) => return Ok(()),
                            // Connected but unhealthy is a DIFFERENT failure from
                            // never connecting, and conflating them is how a real
                            // defect hides behind "flaky".
                            Err(status) => {
                                last = Some(format!("connected, HealthCheck failed: {status}"))
                            }
                        }
                    }
                    Err(e) => last = Some(format!("connect failed: {e}")),
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            Err(last.unwrap_or_else(|| "no connection attempt completed".to_string()))
        })
    })
    .join()
    .unwrap_or_else(|_| Err("readiness thread panicked".to_string()));

    let elapsed = started.elapsed();
    match outcome {
        Ok(()) => Ok(elapsed),
        Err(why) => Err(format!(
            "anvil-engine did not answer gRPC HealthCheck on 127.0.0.1:{port} within {:?} \
             (last: {why}). Raise ANVIL_TEST_ENGINE_READY_TIMEOUT_MS if this machine is slow.",
            budget
        )),
    }
}

pub fn wait_until_grpc_ready(port: u16) -> bool {
    match wait_until_grpc_ready_detailed(port) {
        Ok(elapsed) => {
            // Alarm on a closing margin instead of waiting for it to close.
            // Standing rule: a ceiling reports, it does not silently truncate.
            if elapsed > grpc_ready_budget() / 2 {
                eprintln!(
                    "anvil-test-support: engine on 127.0.0.1:{port} took {elapsed:?} to answer \
                     HealthCheck — over half the {:?} budget",
                    grpc_ready_budget()
                );
            }
            true
        }
        Err(why) => {
            eprintln!("anvil-test-support: {why}");
            false
        }
    }
}

pub struct EngineProcess {
    child: Child,
    pub port: u16,
    /// Shared buffer accumulating every line the engine writes to stderr.
    /// Filled by a background drain thread (N1/N2): draining the pipe as it
    /// fills prevents the undrained-pipe buffer-fill deadlock, and the
    /// `Arc<Mutex<_>>` lets RPC steps read captured logs mid-run even across
    /// the harness's `take`/`set` context hops (the buffer lives INSIDE
    /// EngineProcess, so it travels with it).
    stderr: Arc<Mutex<Vec<String>>>,
    /// Handle to the drain thread; the thread ends when the stderr pipe hits
    /// EOF (which happens once the child is killed/exits).
    drain: Option<JoinHandle<()>>,
    temp_dir_handles: Vec<RetainedTempDir>,
}

impl EngineProcess {
    /// Construct an EngineProcess, taking ownership of the spawned child and
    /// spinning up the background stderr drain thread. Pass `None` for
    /// `stderr_pipe` at the `Stdio::null()` spawn site (no pipe to drain).
    fn new(mut child: Child, port: u16) -> Self {
        let stderr = Arc::new(Mutex::new(Vec::<String>::new()));
        let drain = child.stderr.take().map(|pipe| {
            let buf = Arc::clone(&stderr);
            std::thread::spawn(move || {
                let reader = BufReader::new(pipe);
                for line in reader.lines() {
                    match line {
                        Ok(l) => {
                            if let Ok(mut guard) = buf.lock() {
                                guard.push(l);
                            }
                        }
                        Err(_) => break,
                    }
                }
            })
        });
        EngineProcess {
            child,
            port,
            stderr,
            drain,
            temp_dir_handles: Vec::new(),
        }
    }

    /// Construct an EngineProcess whose stderr is not piped (the
    /// `Stdio::null()` minimal-hearth site). No drain thread is spawned and the
    /// captured-stderr buffer stays empty.
    fn without_capture(child: Child, port: u16) -> Self {
        EngineProcess {
            child,
            port,
            stderr: Arc::new(Mutex::new(Vec::new())),
            drain: None,
            temp_dir_handles: Vec::new(),
        }
    }

    /// Snapshot the captured stderr lines accumulated so far.
    pub fn stderr_lines(&self) -> Vec<String> {
        self.stderr.lock().map(|g| g.clone()).unwrap_or_default()
    }

    fn retain_temp_dir(&mut self, handle: &RetainedTempDir) {
        self.temp_dir_handles.push(Arc::clone(handle));
    }
}

fn retain_ctx_temp_dir(process: &mut EngineProcess, ctx: &Context, key: &str) {
    if let Some(handle) = ctx.get::<RetainedTempDir>(key) {
        process.retain_temp_dir(handle);
    }
}

/// Spawn the real engine for a test hearth with no extra environment seam,
/// blocking until it actually answers a real gRPC HealthCheck. Same shape as
/// [`spawn_engine_with_test_env`] minus the child-local override.
pub fn spawn_engine_for_hearth(hearth_path: &Path) -> Result<EngineProcess, String> {
    let port = free_port()?;
    let binary = crate::harness::binary_path("anvil-engine");
    let temper_home = hearth_path.join("__temper_home__");
    let child = anvil_engine_command(&binary)
        .arg("--hearth")
        .arg(hearth_path)
        .arg("--port")
        .arg(port.to_string())
        .env("ANVIL_TEMPER_HOME", &temper_home)
        .env("ANVIL_TELEMETRY_SALT", "brine-test-salt")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            format!(
                "Failed to start anvil-engine at {}: {}",
                binary.display(),
                error
            )
        })?;
    if !wait_until_grpc_ready(port) {
        let mut child = child;
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!(
            "anvil-engine did not answer gRPC HealthCheck on 127.0.0.1:{} within timeout",
            port
        ));
    }
    Ok(EngineProcess::new(child, port))
}

/// Spawn the real engine for a test hearth with one child-local environment
/// seam. This intentionally does not mutate the Brine runner's process-wide
/// environment, so parallel scenarios cannot inherit another scenario's test
/// writer controls.
pub fn spawn_engine_with_test_env(
    hearth_path: &Path,
    env_key: &str,
    env_value: &Path,
) -> Result<EngineProcess, String> {
    let port = free_port()?;
    let binary = crate::harness::binary_path("anvil-engine");
    let temper_home = hearth_path.join("__temper_home__");
    let child = anvil_engine_command(&binary)
        .arg("--hearth")
        .arg(hearth_path)
        .arg("--port")
        .arg(port.to_string())
        .env("ANVIL_TEMPER_HOME", &temper_home)
        .env("ANVIL_TELEMETRY_SALT", "brine-test-salt")
        .env(env_key, env_value)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            format!(
                "Failed to start anvil-engine at {} with {}: {}",
                binary.display(),
                env_key,
                error
            )
        })?;
    if !wait_until_grpc_ready(port) {
        let mut child = child;
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!(
            "anvil-engine did not answer gRPC HealthCheck on 127.0.0.1:{} within timeout",
            port
        ));
    }
    Ok(EngineProcess::new(child, port))
}

impl Drop for EngineProcess {
    fn drop(&mut self) {
        // Kill the child FIRST so its stderr pipe reaches EOF, then the drain
        // thread's `lines()` loop ends on its own — joining can never hang.
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(handle) = self.drain.take() {
            let _ = handle.join();
        }
    }
}

/// A simple catalog response structure for test verification.
#[derive(Debug, Clone)]
struct CatalogResponse {
    active_artifacts: Vec<CatalogArtifact>,
    available_types: Vec<CatalogAvailableType>,
    available_artifact_kinds: Vec<CatalogAvailableArtifactKind>,
    invalid_artifacts: Vec<InvalidArtifact>,
    resolved_hearth: String,
}

/// An invalid artifact entry from catalog validation.
#[derive(Debug, Clone)]
struct InvalidArtifact {
    id: String,
    code: String,
}

#[derive(Debug, Clone)]
struct CatalogArtifact {
    id: String,
    artifact_type: String,
    state: String,
    summary: String,
    execution_route: String,
}

#[derive(Debug, Clone)]
struct CatalogAvailableType {
    name: String,
    description: String,
    requires_parent: String,
    /// "engine" when begin() creates this kind today, else "fallback:<skill>".
    execution_route: String,
}

#[derive(Debug, Clone)]
struct CatalogAvailableArtifactKind {
    kind: String,
    source_tier: String,
    is_described: bool,
    has_triggers: bool,
}

fn catalog_response_from_proto(resp: anvil_engine::proto::CatalogResponse) -> CatalogResponse {
    CatalogResponse {
        active_artifacts: resp
            .active_artifacts
            .into_iter()
            .map(|a| CatalogArtifact {
                id: a.id,
                artifact_type: a.artifact_type,
                state: a.state,
                summary: a.summary,
                execution_route: a.execution_route,
            })
            .collect(),
        available_types: resp
            .available_types
            .into_iter()
            .map(|t| CatalogAvailableType {
                name: t.name,
                description: t.description,
                requires_parent: t.requires_parent,
                execution_route: t.execution_route,
            })
            .collect(),
        available_artifact_kinds: resp
            .available_artifact_kinds
            .into_iter()
            .map(|k| CatalogAvailableArtifactKind {
                kind: k.kind,
                source_tier: k.source_tier,
                is_described: k.is_described,
                has_triggers: k.has_triggers,
            })
            .collect(),
        invalid_artifacts: resp
            .invalid_artifacts
            .into_iter()
            .map(|iv| InvalidArtifact {
                id: iv.id,
                code: iv.code,
            })
            .collect(),
        resolved_hearth: resp.resolved_hearth,
    }
}

/// Result from a catalog RPC call — either success, a structured gRPC status
/// (code + message, for the Foundry-mode refusal assertions), or a transport
/// error.
#[derive(Debug, Clone)]
enum CatalogRpcResult {
    Success(CatalogResponse),
    /// A gRPC status returned by the server (e.g. UNAUTHENTICATED with a
    /// `not_authenticated` message under Foundry mode, spec Req 5). Kept
    /// distinct from `Error` (transport/connection failure) so the refusal
    /// assertions can match on the gRPC code precisely.
    Status {
        code: String,
        message: String,
    },
    Error(String),
}

/// A simple checkin response structure for test verification.
#[derive(Debug, Clone)]
struct CheckinRpcResponse {
    actor_name: String,
    filtered_artifacts: Vec<CatalogArtifact>,
    available_types: Vec<CatalogAvailableType>,
    next_step: String,
    /// T4 — re-served open-begin hook content (CheckinResponse.context).
    context: String,
}

/// Result from a checkin RPC call — either success or gRPC error with status code.
#[derive(Debug, Clone)]
enum CheckinRpcResult {
    Success(CheckinRpcResponse),
    Error { code: String, message: String },
}

/// Result from a describe RPC call.
#[derive(Debug, Clone)]
enum DescribeRpcResult {
    Success(anvil_engine::proto::DescribeResponse),
    Error { code: String, message: String },
}

/// Result from a begin RPC call.
///
/// PUBLIC so a step in another crate can thread it forward. A brine Map step
/// retains only the keys it declares, so a step that sends a surfaceless call in
/// the MIDDLE of an open run has to re-declare and re-emit the run it is
/// interrupting — otherwise the scenario cannot assert what happened to that run
/// afterwards, and the only expressible version of "a refused change wrote
/// nothing" is one where there was nothing to write about.
#[derive(Debug, Clone)]
pub enum BeginRpcResult {
    Success(anvil_engine::proto::BeginResponse),
    Error { code: String, message: String },
}

/// Result from a route RPC call.
#[derive(Debug, Clone)]
enum RouteRpcResult {
    Success(anvil_engine::proto::RouteResponse),
    Error { code: String, message: String },
}

/// Result from a snapshot RPC call.
///
/// `pub(crate)` so the `kind_resolution` step module can produce the SAME
/// context value this module's assertion steps consume — the address-form
/// matrix (bare id / `<dir>/<id>` / absolute) needs its own driving step
/// because the artifact's absolute path is only known at run time, but it
/// must remain assertable by the existing `the snapshot RPC response …` steps
/// rather than growing a parallel set of them.
#[derive(Debug, Clone)]
pub enum SnapshotRpcResult {
    Success(anvil_engine::proto::SnapshotResponse),
    Error { code: String, message: String },
}

/// Result from a complete RPC call.
#[derive(Debug, Clone)]
enum CompleteRpcResult {
    Success(anvil_engine::proto::CompleteResponse),
    Error { code: String, message: String },
}

/// Result from a begin_adoption_status RPC call (BP3).
#[derive(Debug, Clone)]
enum BeginAdoptionStatusRpcResult {
    Success(anvil_engine::proto::BeginAdoptionStatusResponse),
    Error { code: String, message: String },
}

/// Result from an Amend RPC call (B5b BP3).
#[derive(Debug, Clone)]
enum AmendRpcResult {
    Success(anvil_engine::proto::AmendResponse),
    Error { code: String, message: String },
}

/// Result from a PersistPlaybook RPC call (track 1a BP3).
#[derive(Debug, Clone)]
enum PersistPlaybookRpcResult {
    Success(anvil_engine::proto::PersistPlaybookResponse),
    Error { code: String, message: String },
}

#[derive(Debug, Clone)]
struct HealthCheckRpcResponse {
    wire_proto_version: u32,
    build_version: String,
}

/// Convert tonic status code to canonical gRPC name.
pub fn grpc_code_name(code: tonic::Code) -> String {
    match code {
        tonic::Code::Ok => "OK",
        tonic::Code::Cancelled => "CANCELLED",
        tonic::Code::Unknown => "UNKNOWN",
        tonic::Code::InvalidArgument => "INVALID_ARGUMENT",
        tonic::Code::DeadlineExceeded => "DEADLINE_EXCEEDED",
        tonic::Code::NotFound => "NOT_FOUND",
        tonic::Code::AlreadyExists => "ALREADY_EXISTS",
        tonic::Code::PermissionDenied => "PERMISSION_DENIED",
        tonic::Code::ResourceExhausted => "RESOURCE_EXHAUSTED",
        tonic::Code::FailedPrecondition => "FAILED_PRECONDITION",
        tonic::Code::Aborted => "ABORTED",
        tonic::Code::OutOfRange => "OUT_OF_RANGE",
        tonic::Code::Unimplemented => "UNIMPLEMENTED",
        tonic::Code::Internal => "INTERNAL",
        tonic::Code::Unavailable => "UNAVAILABLE",
        tonic::Code::DataLoss => "DATA_LOSS",
        tonic::Code::Unauthenticated => "UNAUTHENTICATED",
    }
    .to_string()
}

/// Count the immediate entries in a directory; 0 if the directory is absent.
/// Used by the PersistPlaybook boundary scenario to prove the engine hearth's
/// workflows/ entry count is unchanged after a persist to a separate owner-home.
fn count_dir_entries(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir)
        .map(|it| it.flatten().count())
        .unwrap_or(0)
}

/// Run the REAL `anvil-hooks begin` binary against the engine already in `ctx`,
/// appending the caller's mode-specific flags (`--artifact-type …`, `--identifier
/// …`, or neither) to the actor/port/hearth envelope every begin scenario shares.
/// Success prints to stdout and a rejection prints to stderr, so both streams are
/// combined into `begin_output` — the `output contains` check then works for
/// either outcome. The `EngineProcess` owns the hearth's temp-dir handles, so
/// carrying it forward is what keeps the hearth alive for later hearth checks.
fn run_anvil_hooks_begin(mut ctx: Context, mode_args: &[String]) -> Result<Context, String> {
    let engine = ctx
        .take::<EngineProcess>("engine_process")
        .ok_or("No engine_process")?;
    let port = engine.port;
    let hearth = ctx.get::<PathBuf>("hearth_path").cloned();
    crate::harness::ensure_binary("anvil-hooks");
    let bin = crate::harness::binary_path("anvil-hooks");
    let mut cmd = Command::new(&bin);
    cmd.arg("begin");
    for arg in mode_args {
        cmd.arg(arg);
    }
    cmd.arg("--actor-name")
        .arg("BddActor-1")
        .arg("--actor-type")
        .arg("agent")
        .arg("--actor-model")
        .arg("test-model")
        .arg("--actor-provider")
        .arg("test-provider")
        .arg("--port")
        .arg(port.to_string());
    if let Some(h) = &hearth {
        cmd.arg("--hearth").arg(h.to_str().unwrap());
    }
    let output = cmd.output().map_err(|e| format!("run begin: {}", e))?;
    let code = output.status.code().unwrap_or(-1);
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let mut out = Context::new();
    out.set("engine_process", engine);
    if let Some(h) = hearth {
        out.set("hearth_path", h);
    }
    out.set("begin_exit", code as i64);
    out.set("begin_output", combined);
    Ok(out)
}

fn column_index(table: &DataTable, name: &str) -> Result<usize, String> {
    table
        .headers
        .iter()
        .position(|h| h == name)
        .ok_or_else(|| format!("Missing '{}' column", name))
}

fn read_jsonl(path: &Path) -> Result<Vec<serde_json::Value>, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("read JSONL sink {}: {}", path.display(), e))?;
    contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .map_err(|e| format!("parse JSONL line in {}: {} ({})", path.display(), e, line))
        })
        .collect()
}

fn read_jsonl_text(contents: &str) -> Result<Vec<serde_json::Value>, String> {
    contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(|error| error.to_string()))
        .collect()
}

const STEP_MEASUREMENT_POLL_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(2);
const STEP_MEASUREMENT_POLL_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(20);

fn step_measurement_record_count(contents: &str) -> usize {
    contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count()
}

fn complete_step_measurement_record_count(contents: &str) -> Option<usize> {
    let lines = contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    if lines
        .iter()
        .all(|line| serde_json::from_str::<serde_json::Value>(line).is_ok())
    {
        Some(lines.len())
    } else {
        None
    }
}

/// Wait for an asynchronously dispatched step-measurement sink observation.
///
/// P2b deliberately lets the lifecycle RPC return before the durable writer,
/// so behavior checks must establish that the relevant row arrived before
/// making positive or negative assertions about its bytes.
fn wait_for_step_measurement_contents(
    path: &Path,
    description: &str,
    predicate: impl Fn(&str) -> bool,
) -> Result<String, String> {
    let deadline = std::time::Instant::now() + STEP_MEASUREMENT_POLL_TIMEOUT;
    let mut last_observation: String;

    loop {
        match std::fs::read_to_string(path) {
            Ok(contents) => {
                if predicate(&contents) {
                    return Ok(contents);
                }
                last_observation = format!(
                    "{} complete record(s): {}",
                    step_measurement_record_count(&contents),
                    contents
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                last_observation = "sink was absent".to_string();
            }
            Err(error) => {
                last_observation = format!("read failed: {}", error);
            }
        }

        let now = std::time::Instant::now();
        if now >= deadline {
            return Err(format!(
                "step-measurement sink {} did not reach {} within {:?}: {}",
                path.display(),
                description,
                STEP_MEASUREMENT_POLL_TIMEOUT,
                last_observation
            ));
        }
        std::thread::sleep(STEP_MEASUREMENT_POLL_INTERVAL.min(deadline - now));
    }
}

fn wait_for_step_measurement_record(
    path: &Path,
    field: &str,
    value: &str,
) -> Result<serde_json::Value, String> {
    let deadline = std::time::Instant::now() + STEP_MEASUREMENT_POLL_TIMEOUT;
    let mut last_observation: String;

    loop {
        match read_jsonl(path) {
            Ok(records) => {
                if let Some(record) = jsonl_record_matching(&records, field, value) {
                    return Ok(record.clone());
                }
                last_observation = format!("{} parsed record(s): {:?}", records.len(), records);
            }
            Err(error) => last_observation = error,
        }

        let now = std::time::Instant::now();
        if now >= deadline {
            return Err(format!(
                "step-measurement sink {} did not contain {}='{}' within {:?}: {}",
                path.display(),
                field,
                value,
                STEP_MEASUREMENT_POLL_TIMEOUT,
                last_observation
            ));
        }
        std::thread::sleep(STEP_MEASUREMENT_POLL_INTERVAL.min(deadline - now));
    }
}

fn string_field<'a>(record: &'a serde_json::Value, field: &str) -> Option<&'a str> {
    record.get(field).and_then(serde_json::Value::as_str)
}

fn jsonl_record_matching<'a>(
    records: &'a [serde_json::Value],
    field: &str,
    value: &str,
) -> Option<&'a serde_json::Value> {
    records
        .iter()
        .find(|record| string_field(record, field) == Some(value))
}

/// Poll `<hearth>/delivery-log.jsonl` until every line parses and `predicate`
/// accepts the whole row vector, or the deadline expires.
///
/// The two delivery-log steps that shipped before this one read the sink
/// exactly ONCE. That is a load-dependent flake waiting to happen: the sink is
/// appended by a SEPARATE process (`anvil-hooks`), and although the step
/// observed that process exit, a read-once assertion has no slack at all for a
/// loaded box. This sink exists to catch an outage that already read as silence
/// for days; an assertion over it that fails for a timing reason teaches the
/// reader to distrust exactly the instrument they need. So: poll with a
/// deadline, and report the last observation on failure.
fn wait_for_delivery_rows(
    hearth: &Path,
    description: &str,
    predicate: impl Fn(&[serde_json::Value]) -> bool,
) -> Result<Vec<serde_json::Value>, String> {
    let log = hearth.join("delivery-log.jsonl");
    let deadline = std::time::Instant::now() + STEP_MEASUREMENT_POLL_TIMEOUT;
    let mut last_observation: String;
    loop {
        match std::fs::read_to_string(&log) {
            Ok(contents) => match read_jsonl_text(&contents) {
                Ok(rows) => {
                    if predicate(&rows) {
                        return Ok(rows);
                    }
                    last_observation = format!("{} row(s): {}", rows.len(), contents.trim());
                }
                // A line that does not parse is the GLUED-JSON defect, not a
                // slow write — but it is reported through the same deadline so
                // a half-written final line still gets its chance to land.
                Err(error) => last_observation = format!("unparseable row: {}", error),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                last_observation = "sink was absent".to_string();
            }
            Err(error) => last_observation = format!("read failed: {}", error),
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            return Err(format!(
                "delivery log {} did not reach {} within {:?}: {}",
                log.display(),
                description,
                STEP_MEASUREMENT_POLL_TIMEOUT,
                last_observation
            ));
        }
        std::thread::sleep(STEP_MEASUREMENT_POLL_INTERVAL.min(deadline - now));
    }
}

/// The LAST delivery row's string `field`. `<missing>` when the key is absent —
/// which is a DIFFERENT fact from an empty value and must not read as one.
fn delivery_string<'a>(rows: &'a [serde_json::Value], field: &str) -> &'a str {
    rows.last()
        .and_then(|row| row.get(field))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<missing>")
}

/// Poll `<hearth>/activity-log.jsonl` for the first record whose `command`
/// matches. Same reason as [`wait_for_delivery_rows`]: a durable append by
/// another process is not ordered against this read.
fn wait_for_activity_record(
    hearth: &Path,
    command: &str,
) -> Result<serde_json::Value, String> {
    let log = hearth.join("activity-log.jsonl");
    let deadline = std::time::Instant::now() + STEP_MEASUREMENT_POLL_TIMEOUT;
    let mut last_observation: String;
    loop {
        match std::fs::read_to_string(&log) {
            Ok(contents) => match read_jsonl_text(&contents) {
                Ok(records) => match jsonl_record_matching(&records, "command", command) {
                    Some(record) => return Ok(record.clone()),
                    None => {
                        last_observation = format!(
                            "{} record(s), commands {:?}",
                            records.len(),
                            records
                                .iter()
                                .map(|r| string_field(r, "command").unwrap_or("<missing>"))
                                .collect::<Vec<_>>()
                        )
                    }
                },
                Err(error) => last_observation = format!("unparseable row: {}", error),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                last_observation = "sink was absent".to_string();
            }
            Err(error) => last_observation = format!("read failed: {}", error),
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            return Err(format!(
                "activity log {} never held a {:?} record within {:?}: {}",
                log.display(),
                command,
                STEP_MEASUREMENT_POLL_TIMEOUT,
                last_observation
            ));
        }
        std::thread::sleep(STEP_MEASUREMENT_POLL_INTERVAL.min(deadline - now));
    }
}

fn project_label_from_root(project_root: &str) -> Result<String, String> {
    Path::new(project_root)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| format!("project root '{}' has no basename", project_root))
}

/// Call the checkin RPC with the given role. Returns the
/// CheckinRpcResult plus the captured next_step text. Used by the
/// literal-role step defs that expose a per-role typed next_step key.
async fn call_checkin(
    engine: &EngineProcess,
    role: &str,
) -> Result<(CheckinRpcResult, String), String> {
    let addr = format!("http://127.0.0.1:{}", engine.port);
    match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
        Ok(mut client) => {
            let request = crate::surfaced(anvil_engine::proto::CheckinRequest {
                hearth_path: String::new(),
                role: role.to_string(),
                actor_type: "agent".to_string(),
                actor_model: "test-model".to_string(),
                actor_provider: "test".to_string(),
                actor_name: String::new(),
            });
            match client.checkin(request).await {
                Ok(response) => {
                    let resp = response.into_inner();
                    let captured = resp.next_step.clone();
                    let r = CheckinRpcResult::Success(CheckinRpcResponse {
                        actor_name: resp.actor_name,
                        filtered_artifacts: resp
                            .filtered_artifacts
                            .into_iter()
                            .map(|a| CatalogArtifact {
                                id: a.id,
                                artifact_type: a.artifact_type,
                                state: a.state,
                                summary: a.summary,
                                execution_route: a.execution_route,
                            })
                            .collect(),
                        available_types: resp
                            .available_types
                            .into_iter()
                            .map(|t| CatalogAvailableType {
                                name: t.name,
                                description: t.description,
                                requires_parent: t.requires_parent,
                                execution_route: t.execution_route,
                            })
                            .collect(),
                        next_step: resp.next_step,
                        context: resp.context,
                    });
                    Ok((r, captured))
                }
                Err(status) => Err(format!(
                    "Checkin RPC failed: gRPC {}: {}",
                    grpc_code_name(status.code()),
                    status.message()
                )),
            }
        }
        Err(e) => Err(format!("Connection failed: {}", e)),
    }
}

async fn call_checkin_with_hearth(
    engine: &EngineProcess,
    role: &str,
    hearth_path: String,
) -> CheckinRpcResult {
    let addr = format!("http://127.0.0.1:{}", engine.port);
    match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
        Ok(mut client) => {
            let request = crate::surfaced(anvil_engine::proto::CheckinRequest {
                hearth_path,
                role: role.to_string(),
                actor_type: "agent".to_string(),
                actor_model: "test-model".to_string(),
                actor_provider: "test".to_string(),
                actor_name: String::new(),
            });
            match client.checkin(request).await {
                Ok(response) => {
                    let resp = response.into_inner();
                    CheckinRpcResult::Success(CheckinRpcResponse {
                        actor_name: resp.actor_name,
                        filtered_artifacts: resp
                            .filtered_artifacts
                            .into_iter()
                            .map(|a| CatalogArtifact {
                                id: a.id,
                                artifact_type: a.artifact_type,
                                state: a.state,
                                summary: a.summary,
                                execution_route: a.execution_route,
                            })
                            .collect(),
                        available_types: resp
                            .available_types
                            .into_iter()
                            .map(|t| CatalogAvailableType {
                                name: t.name,
                                description: t.description,
                                requires_parent: t.requires_parent,
                                execution_route: t.execution_route,
                            })
                            .collect(),
                        next_step: resp.next_step,
                        context: resp.context,
                    })
                }
                Err(status) => CheckinRpcResult::Error {
                    code: grpc_code_name(status.code()),
                    message: status.message().to_string(),
                },
            }
        }
        Err(e) => CheckinRpcResult::Error {
            code: "UNAVAILABLE".to_string(),
            message: format!("Connection failed: {}", e),
        },
    }
}

/// Call the describe RPC with the given identifier (type name or
/// instance id). Returns the DescribeRpcResult plus the captured
/// next_step text. Used by the subject-specific step defs that expose
/// per-subject typed next_step keys.
async fn call_describe(
    engine: &EngineProcess,
    identifier: &str,
) -> Result<(DescribeRpcResult, String), String> {
    let addr = format!("http://127.0.0.1:{}", engine.port);
    match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
        Ok(mut client) => {
            let request = crate::surfaced(anvil_engine::proto::DescribeRequest {
                hearth_path: String::new(),
                identifier: identifier.to_string(),
            });
            match client.describe(request).await {
                Ok(response) => {
                    let resp = response.into_inner();
                    let captured = resp.next_step.clone();
                    Ok((DescribeRpcResult::Success(resp), captured))
                }
                Err(status) => Err(format!(
                    "Describe RPC failed: gRPC {}: {}",
                    grpc_code_name(status.code()),
                    status.message()
                )),
            }
        }
        Err(e) => Err(format!("Connection failed: {}", e)),
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "the engine is started with that hearth",
            &[("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("engine_port", "u16"),
                ("kiln_router_port", "i64"),
            ],
            |ctx, _params| {
                let hearth_path = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();

                // Use a free port to avoid conflicts
                let listener = std::net::TcpListener::bind("127.0.0.1:0")
                    .map_err(|e| format!("Failed to find free port: {}", e))?;
                let port = listener.local_addr()
                    .map_err(|e| format!("Failed to get port: {}", e))?.port();
                drop(listener);

                let binary = crate::harness::binary_path("anvil-engine");

                // Redirect the §0 temper stream into the test temp tree so the
                // engine never writes the real `~/.temper`, and the events file
                // can be read back by the temper-stream assertions. The writer
                // prefers ANVIL_TEMPER_HOME and roots the stream at
                // `<ANVIL_TEMPER_HOME>/.temper/step-measurements/<kind>/events.jsonl`.
                let temper_home = hearth_path.join("__temper_home__");

                let child = anvil_engine_command(&binary)
                    .arg("--hearth")
                    .arg(hearth_path.to_str().unwrap())
                    .arg("--port")
                    .arg(port.to_string())
                    .env("ANVIL_TEMPER_HOME", temper_home.to_str().unwrap())
                    .env("ANVIL_TELEMETRY_SALT", "brine-test-salt")
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap_or_else(|e| {
                        panic!(
                            "Failed to start anvil-engine at {}: {}",
                            binary.display(),
                            e
                        )
                    });

                // Wait until the engine is actually SERVING gRPC before
                // proceeding — not just TCP-listening. A blind sleep (or a
                // TCP-only check) races: the port can accept a connection while
                // the tonic/HTTP-2 server isn't ready, so a consumer's gRPC
                // HealthCheck (the shim's dial shape) returns nothing and it
                // spawns its own engine. Poll a real gRPC HealthCheck until it
                // succeeds, then fail loudly if it never comes up. (The free-port
                // grab above is itself TOCTOU; this readiness gate also covers a
                // lost-port respawn.)
                {
                    // This step runs ON the brine tokio runtime, so a nested
                    // block_on would panic ("runtime within a runtime"). Drive
                    // the readiness poll on a SEPARATE thread that owns its own
                    // runtime.
                    let ready = wait_until_grpc_ready(port);
                    if !ready {
                        return Err(format!(
                            "anvil-engine did not answer gRPC HealthCheck on 127.0.0.1:{} within timeout",
                            port
                        ));
                    }
                }

                let mut process = EngineProcess::new(child, port);
                retain_ctx_temp_dir(&mut process, &ctx, "hearth_path_handle");
                let mut out = Context::new();
                out.set("engine_process", process);
                out.set("hearth_path", hearth_path);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                out.set("engine_port", port);
                if let Some(kiln_port) = ctx.get::<i64>("kiln_router_port") {
                    out.set("kiln_router_port", *kiln_port);
                }
                Ok(out)
            },
        ),
        // Measurement-enforcement spawn: identical to "started with that hearth"
        // but flips ANVIL_ENFORCE_MEASUREMENT_DEFINITION ON so the enforcing
        // generator/loader dark-gate is active in the engine process.
        step_def(
            "the engine is started with that hearth and measurement enforcement on",
            &[("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("engine_port", "u16"),
                ("kiln_router_port", "i64"),
            ],
            |ctx, _params| {
                let hearth_path = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();

                let listener = std::net::TcpListener::bind("127.0.0.1:0")
                    .map_err(|e| format!("Failed to find free port: {}", e))?;
                let port = listener
                    .local_addr()
                    .map_err(|e| format!("Failed to get port: {}", e))?
                    .port();
                drop(listener);

                let binary = crate::harness::binary_path("anvil-engine");
                let temper_home = hearth_path.join("__temper_home__");

                let child = anvil_engine_command(&binary)
                    .arg("--hearth")
                    .arg(hearth_path.to_str().unwrap())
                    .arg("--port")
                    .arg(port.to_string())
                    .env("ANVIL_TEMPER_HOME", temper_home.to_str().unwrap())
                    .env("ANVIL_TELEMETRY_SALT", "brine-test-salt")
                    .env("ANVIL_ENFORCE_MEASUREMENT_DEFINITION", "1")
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap_or_else(|e| {
                        panic!(
                            "Failed to start anvil-engine at {}: {}",
                            binary.display(),
                            e
                        )
                    });

                {
                    let ready = wait_until_grpc_ready(port);
                    if !ready {
                        return Err(format!(
                            "anvil-engine did not answer gRPC HealthCheck on 127.0.0.1:{} within timeout",
                            port
                        ));
                    }
                }

                let mut process = EngineProcess::new(child, port);
                retain_ctx_temp_dir(&mut process, &ctx, "hearth_path_handle");
                let mut out = Context::new();
                out.set("engine_process", process);
                out.set("hearth_path", hearth_path);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                out.set("engine_port", port);
                if let Some(kiln_port) = ctx.get::<i64>("kiln_router_port") {
                    out.set("kiln_router_port", *kiln_port);
                }
                Ok(out)
            },
        ),
        // Evidence-obligation-enforcement spawn (T-EEC-1 P4): identical to
        // "started with that hearth" but flips ANVIL_ENFORCE_EVIDENCE_OBLIGATION
        // ON (and leaves the measurement gate OFF) so ONLY the obligation leg is
        // active at the loader/registry, persist, and intake seams. Also sets
        // ANVIL_SKIP_HOOK_INSTALL=1 so a test engine never rewrites the live
        // ~/.claude/settings.json hook command (memory: test engines clobber live
        // hooks).
        step_def(
            "the engine is started with that hearth and evidence obligation enforcement on",
            &[("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("engine_port", "u16"),
                ("kiln_router_port", "i64"),
            ],
            |ctx, _params| {
                let hearth_path = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();

                let listener = std::net::TcpListener::bind("127.0.0.1:0")
                    .map_err(|e| format!("Failed to find free port: {}", e))?;
                let port = listener
                    .local_addr()
                    .map_err(|e| format!("Failed to get port: {}", e))?
                    .port();
                drop(listener);

                let binary = crate::harness::binary_path("anvil-engine");
                let temper_home = hearth_path.join("__temper_home__");

                let child = anvil_engine_command(&binary)
                    .arg("--hearth")
                    .arg(hearth_path.to_str().unwrap())
                    .arg("--port")
                    .arg(port.to_string())
                    .env("ANVIL_TEMPER_HOME", temper_home.to_str().unwrap())
                    .env("ANVIL_TELEMETRY_SALT", "brine-test-salt")
                    .env("ANVIL_ENFORCE_EVIDENCE_OBLIGATION", "1")
                    .env("ANVIL_SKIP_HOOK_INSTALL", "1")
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap_or_else(|e| {
                        panic!(
                            "Failed to start anvil-engine at {}: {}",
                            binary.display(),
                            e
                        )
                    });

                if !wait_until_grpc_ready(port) {
                    return Err(format!(
                        "anvil-engine did not answer gRPC HealthCheck on 127.0.0.1:{} within timeout",
                        port
                    ));
                }

                let mut process = EngineProcess::new(child, port);
                retain_ctx_temp_dir(&mut process, &ctx, "hearth_path_handle");
                let mut out = Context::new();
                out.set("engine_process", process);
                out.set("hearth_path", hearth_path);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                out.set("engine_port", port);
                if let Some(kiln_port) = ctx.get::<i64>("kiln_router_port") {
                    out.set("kiln_router_port", *kiln_port);
                }
                Ok(out)
            },
        ),
        // Semantic Route RPC dark-launch spawn: identical to "started with that
        // hearth" but flips the dark flag ON (ANVIL_SEMANTIC_ROUTE_RPC=on) and
        // points the Kiln gateway at a CLOSED loopback port so the single Kiln
        // call transport-errors → RouterVerdict::Fallback → apply_semantic_verdict
        // keeps the lexical resolution UNCHANGED (fail-open). Fully hermetic: the
        // config-file + token-file overrides point at nonexistent paths so the
        // developer's real ~/.anvil/router.json and ~/.foundry token never bleed
        // in, and the closed port means NO real gateway is ever dialed.
        step_def(
            "the engine is started with that hearth and the semantic route RPC flag on but Kiln unreachable",
            &[("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("engine_port", "u16"),
            ],
            |ctx, _params| {
                let hearth_path = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();

                let port = free_port()?;
                // A just-released loopback port: nothing listens, so the Kiln
                // connect fails immediately (transport_err → Fallback).
                let closed_kiln_port = free_port()?;

                let binary = crate::harness::binary_path("anvil-engine");
                let temper_home = hearth_path.join("__temper_home__");
                let no_router_config = hearth_path.join("__no_router_config__.json");
                let no_token_file = hearth_path.join("__no_kiln_token__");

                let child = anvil_engine_command(&binary)
                    .arg("--hearth")
                    .arg(hearth_path.to_str().unwrap())
                    .arg("--port")
                    .arg(port.to_string())
                    .env("ANVIL_TEMPER_HOME", temper_home.to_str().unwrap())
                    .env("ANVIL_TELEMETRY_SALT", "brine-test-salt")
                    .env("ANVIL_SEMANTIC_ROUTE_RPC", "on")
                    .env("ANVIL_ROUTER_ENABLED", "on")
                    .env("ANVIL_KILN_PORT", closed_kiln_port.to_string())
                    .env("ANVIL_KILN_TIMEOUT_MS", "1500")
                    .env("ANVIL_ROUTER_CONFIG_FILE", no_router_config.to_str().unwrap())
                    .env("ANVIL_KILN_TOKEN_FILE", no_token_file.to_str().unwrap())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap_or_else(|e| {
                        panic!(
                            "Failed to start anvil-engine at {}: {}",
                            binary.display(),
                            e
                        )
                    });

                if !wait_until_grpc_ready(port) {
                    return Err(format!(
                        "anvil-engine did not answer gRPC HealthCheck on 127.0.0.1:{} within timeout",
                        port
                    ));
                }

                let mut process = EngineProcess::new(child, port);
                retain_ctx_temp_dir(&mut process, &ctx, "hearth_path_handle");
                let mut out = Context::new();
                out.set("engine_process", process);
                out.set("hearth_path", hearth_path);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                out.set("engine_port", port);
                Ok(out)
            },
        ),
        step_def(
            "the engine is started with that hearth, semantic routing on, V1 {string}, and V2 cap {string}",
            &[("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("engine_port", "u16"),
                ("semantic_kiln_request", "String"),
            ],
            |ctx, params| {
                let hearth_path = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let v1 = params.get_string(0).ok_or("Expected V1 state")?;
                let v2_cap = params.get_string(1).ok_or("Expected V2 cap")?;
                let request_file = hearth_path.join("semantic-kiln-request.txt");
                let kiln_port = start_kiln_router_capturing_stub(
                    &request_file,
                    serde_json::json!({ "kind": "daily_recap" }),
                )?;
                let port = free_port()?;
                let binary = crate::harness::binary_path("anvil-engine");
                let mut command = anvil_engine_command(&binary);
                command
                    .arg("--hearth")
                    .arg(&hearth_path)
                    .arg("--port")
                    .arg(port.to_string())
                    .env("ANVIL_TEMPER_HOME", hearth_path.join("__temper_home__"))
                    .env("ANVIL_TELEMETRY_SALT", "brine-test-salt")
                    .env("ANVIL_SEMANTIC_ROUTE_RPC", "on")
                    .env("ANVIL_ROUTER_ENABLED", "on")
                    .env("ANVIL_KILN_PORT", kiln_port.to_string())
                    .env("ANVIL_KILN_TIMEOUT_MS", "500")
                    .env(
                        "ANVIL_ROUTER_CONFIG_FILE",
                        hearth_path.join("__no_router_config__.json"),
                    )
                    .env(
                        "ANVIL_KILN_TOKEN_FILE",
                        hearth_path.join("__no_kiln_token__"),
                    )
                    .stdout(Stdio::null())
                    .stderr(Stdio::piped());
                if v1 == "on" {
                    command.env("ANVIL_ROUTER_V1_GATE_BREADTH", "on");
                } else {
                    command.env_remove("ANVIL_ROUTER_V1_GATE_BREADTH");
                }
                if v2_cap == "unset" {
                    command.env_remove("ANVIL_ROUTER_V2_BRIEF_CAP");
                } else {
                    command.env("ANVIL_ROUTER_V2_BRIEF_CAP", v2_cap.to_string());
                }
                let child = command
                    .spawn()
                    .map_err(|e| format!("Failed to start semantic experiment engine: {}", e))?;
                if !wait_until_grpc_ready(port) {
                    return Err(format!(
                        "semantic experiment engine did not answer HealthCheck on port {}",
                        port
                    ));
                }
                let mut process = EngineProcess::new(child, port);
                retain_ctx_temp_dir(&mut process, &ctx, "hearth_path_handle");
                let mut out = Context::new();
                out.set("engine_process", process);
                out.set("hearth_path", hearth_path);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                out.set("engine_port", port);
                out.set(
                    "semantic_kiln_request",
                    request_file.to_string_lossy().to_string(),
                );
                Ok(out)
            },
        ),
        check_def(
            "the live semantic router was not called",
            &[("semantic_kiln_request", "String")],
            |ctx, _params| {
                let path = ctx
                    .get::<String>("semantic_kiln_request")
                    .ok_or("No semantic Kiln request path")?;
                if std::fs::metadata(path).is_err() {
                    Ok(())
                } else {
                    Err(format!("Semantic router unexpectedly received a request at {}", path))
                }
            },
        ),
        check_def(
            "the engine warning records invalid brief cap {string}",
            &[("engine_process", "EngineProcess")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected invalid cap")?;
                let engine = ctx
                    .get::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                loop {
                    for line in engine.stderr_lines() {
                        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
                            continue;
                        };
                        if value["level"] == "WARN"
                            && value["brief_cap"].as_str() == Some(expected.as_ref())
                            && value["message"]
                                == "invalid ANVIL_ROUTER_V2_BRIEF_CAP; falling back to all briefs"
                        {
                            return Ok(());
                        }
                    }
                    if std::time::Instant::now() >= deadline {
                        return Err(format!(
                            "No structured invalid-cap warning found for {:?}",
                            expected
                        ));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
            },
        ),
        // Liveness-under-load spawn: identical to "started with that hearth" but
        // pins the engine's tokio runtime to a SINGLE worker thread
        // (`TOKIO_WORKER_THREADS=1`, honored by `#[tokio::main]`'s multi-thread
        // runtime). One worker makes runtime starvation DETERMINISTIC and
        // hardware-independent: any synchronous whole-hearth fold left inline on the
        // async worker blocks EVERYTHING on that runtime — including the `/health`
        // accept loop — for the fold's full duration. It is the tightest possible
        // proxy for the production crash (few workers, heavy folds). With the folds
        // offloaded to the blocking pool, the lone async worker stays free and
        // `/health` answers immediately no matter how heavy/concurrent the folds are.
        step_def(
            "the engine is started with that hearth on a single worker thread",
            &[("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("engine_port", "u16"),
            ],
            |ctx, _params| {
                let hearth_path = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();

                let listener = std::net::TcpListener::bind("127.0.0.1:0")
                    .map_err(|e| format!("Failed to find free port: {}", e))?;
                let port = listener
                    .local_addr()
                    .map_err(|e| format!("Failed to get port: {}", e))?
                    .port();
                drop(listener);

                let binary = crate::harness::binary_path("anvil-engine");
                let temper_home = hearth_path.join("__temper_home__");

                let child = anvil_engine_command(&binary)
                    .arg("--hearth")
                    .arg(hearth_path.to_str().unwrap())
                    .arg("--port")
                    .arg(port.to_string())
                    .env("ANVIL_TEMPER_HOME", temper_home.to_str().unwrap())
                    .env("ANVIL_TELEMETRY_SALT", "brine-test-salt")
                    .env("TOKIO_WORKER_THREADS", "1")
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap_or_else(|e| {
                        panic!(
                            "Failed to start anvil-engine at {}: {}",
                            binary.display(),
                            e
                        )
                    });

                if !wait_until_grpc_ready(port) {
                    return Err(format!(
                        "anvil-engine (single-worker) did not answer gRPC HealthCheck on 127.0.0.1:{} within timeout",
                        port
                    ));
                }

                let mut process = EngineProcess::new(child, port);
                retain_ctx_temp_dir(&mut process, &ctx, "hearth_path_handle");
                let mut out = Context::new();
                out.set("engine_process", process);
                out.set("hearth_path", hearth_path);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                out.set("engine_port", port);
                Ok(out)
            },
        ),
        // Crucible publication-log: start the engine with
        // `$FOUNDRY_PUBLICATION_LOG_DIR` pointed at a temp dir inside the
        // (retained) hearth tree, so the best-effort mirror activates and we can
        // read the canonical envelope back. Without this env the Brine harness
        // (which sets ANVIL_TEMPER_HOME) keeps the publication log inert.
        step_def(
            "the engine is started with that hearth and a publication log",
            &[("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("engine_port", "u16"),
                ("publication_log_dir", "PathBuf"),
            ],
            |ctx, _params| {
                let hearth_path = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();

                let port = free_port()?;
                let binary = crate::harness::binary_path("anvil-engine");
                let temper_home = hearth_path.join("__temper_home__");
                // Lives inside the retained hearth temp tree, so it survives for
                // the engine's lifetime alongside `hearth_path_handle`.
                let publication_log_dir = hearth_path.join("__publication_log__");

                let child = anvil_engine_command(&binary)
                    .arg("--hearth")
                    .arg(hearth_path.to_str().unwrap())
                    .arg("--port")
                    .arg(port.to_string())
                    .env("ANVIL_TEMPER_HOME", temper_home.to_str().unwrap())
                    .env(
                        "FOUNDRY_PUBLICATION_LOG_DIR",
                        publication_log_dir.to_str().unwrap(),
                    )
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap_or_else(|e| {
                        panic!("Failed to start anvil-engine at {}: {}", binary.display(), e)
                    });

                if !wait_until_grpc_ready(port) {
                    return Err(format!(
                        "anvil-engine did not answer gRPC HealthCheck on 127.0.0.1:{} within timeout",
                        port
                    ));
                }

                let mut process = EngineProcess::new(child, port);
                retain_ctx_temp_dir(&mut process, &ctx, "hearth_path_handle");
                let mut out = Context::new();
                out.set("engine_process", process);
                out.set("hearth_path", hearth_path);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                out.set("engine_port", port);
                out.set("publication_log_dir", publication_log_dir);
                Ok(out)
            },
        ),
        // Spawn the engine with hearth A as the default AND hearth B as a second
        // permitted root, so an `all_hearths` query folds BOTH. Both hearth temp
        // dirs are retained on the EngineProcess so they survive the run.
        step_def(
            "the engine is started with both permitted hearths",
            &[
                ("hearth_path", "PathBuf"),
                ("uts_rpc_hearth_b", "PathBuf"),
            ],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("uts_rpc_hearth_b", "PathBuf"),
                ("uts_rpc_hearth_b_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("engine_port", "u16"),
            ],
            |ctx, _params| {
                let hearth_a = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path (A)")?
                    .clone();
                let hearth_b = ctx
                    .get::<PathBuf>("uts_rpc_hearth_b")
                    .ok_or("No uts_rpc_hearth_b")?
                    .clone();
                let port = free_port()?;
                let binary = crate::harness::binary_path("anvil-engine");
                let child = anvil_engine_command(&binary)
                    .arg("--hearth")
                    .arg(hearth_a.to_str().unwrap())
                    .arg("--permitted-root")
                    .arg(hearth_b.to_str().unwrap())
                    .arg("--port")
                    .arg(port.to_string())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap_or_else(|e| {
                        panic!("Failed to start anvil-engine at {}: {}", binary.display(), e)
                    });
                if !wait_until_grpc_ready(port) {
                    return Err(format!(
                        "anvil-engine did not answer gRPC HealthCheck on 127.0.0.1:{} within timeout",
                        port
                    ));
                }
                let mut process = EngineProcess::new(child, port);
                retain_ctx_temp_dir(&mut process, &ctx, "hearth_path_handle");
                retain_ctx_temp_dir(&mut process, &ctx, "uts_rpc_hearth_b_handle");
                let mut out = Context::new();
                out.set("engine_process", process);
                out.set("hearth_path", hearth_a);
                out.set("uts_rpc_hearth_b", hearth_b);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                carry_retained_temp_dir(&ctx, &mut out, "uts_rpc_hearth_b_handle");
                out.set("engine_port", port);
                Ok(out)
            },
        ),
        // Spawn the engine with sub-hearth A as the explicit --hearth default AND
        // the PARENT directory as --permitted-root. The parent is not itself a
        // hearth; an `all_hearths` query must DISCOVER both sub-hearths beneath
        // it. Both temp-dir handles travel on the EngineProcess so they survive.
        step_def(
            "the engine is started with the first sub-hearth and the parent as a permitted root",
            &[
                ("hearth_path", "PathBuf"),
                ("wa_parent_root", "PathBuf"),
            ],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("wa_parent_root", "PathBuf"),
                ("engine_port", "u16"),
            ],
            |ctx, _params| {
                let hearth_a = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path (sub-hearth A)")?
                    .clone();
                let parent = ctx
                    .get::<PathBuf>("wa_parent_root")
                    .ok_or("No wa_parent_root")?
                    .clone();
                let port = free_port()?;
                let binary = crate::harness::binary_path("anvil-engine");
                let child = anvil_engine_command(&binary)
                    .arg("--hearth")
                    .arg(hearth_a.to_str().unwrap())
                    .arg("--permitted-root")
                    .arg(parent.to_str().unwrap())
                    .arg("--port")
                    .arg(port.to_string())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap_or_else(|e| {
                        panic!("Failed to start anvil-engine at {}: {}", binary.display(), e)
                    });
                if !wait_until_grpc_ready(port) {
                    return Err(format!(
                        "anvil-engine did not answer gRPC HealthCheck on 127.0.0.1:{} within timeout",
                        port
                    ));
                }
                let mut process = EngineProcess::new(child, port);
                retain_ctx_temp_dir(&mut process, &ctx, "hearth_path_handle");
                let mut out = Context::new();
                out.set("engine_process", process);
                out.set("hearth_path", hearth_a);
                out.set("wa_parent_root", parent);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                out.set("engine_port", port);
                Ok(out)
            },
        ),
        // Spawn the engine with a chosen ANVIL_LOG EnvFilter value, so AC4
        // (verbosity control) can assert quiet-default vs verbose behavior.
        step_def(
            "the engine is started with that hearth and ANVIL_LOG {string}",
            &[("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("engine_port", "u16"),
            ],
            |ctx, params| {
                let anvil_log = params.get_string(0).ok_or("Expected ANVIL_LOG value")?.to_string();
                let hearth_path = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();

                let listener = std::net::TcpListener::bind("127.0.0.1:0")
                    .map_err(|e| format!("Failed to find free port: {}", e))?;
                let port = listener.local_addr()
                    .map_err(|e| format!("Failed to get port: {}", e))?.port();
                drop(listener);

                let binary = crate::harness::binary_path("anvil-engine");

                let child = anvil_engine_command(&binary)
                    .arg("--hearth")
                    .arg(hearth_path.to_str().unwrap())
                    .arg("--port")
                    .arg(port.to_string())
                    .env("ANVIL_LOG", anvil_log)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap_or_else(|e| {
                        panic!(
                            "Failed to start anvil-engine at {}: {}",
                            binary.display(),
                            e
                        )
                    });

                wait_for_engine_ready(port);

                let mut process = EngineProcess::new(child, port);
                retain_ctx_temp_dir(&mut process, &ctx, "hearth_path_handle");
                let mut out = Context::new();
                out.set("engine_process", process);
                out.set("hearth_path", hearth_path);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                out.set("engine_port", port);
                Ok(out)
            },
        ),
        step_def(
            "the engine is started with hearth path {string}",
            &[],
            &[("engine_process", "EngineProcess")],
            |_ctx, params| {
                let hearth_path = params.get_string(0).ok_or("Expected hearth path")?.to_string();
                // Use a free port (not a fixed pid-derived one) so this engine
                // never collides with the random-free-port engines spawned by
                // other scenarios in the same process run — a fixed port made
                // the catalog RPC occasionally dial the wrong engine.
                let listener = std::net::TcpListener::bind("127.0.0.1:0")
                    .map_err(|e| format!("Failed to find free port: {}", e))?;
                let port = listener
                    .local_addr()
                    .map_err(|e| format!("Failed to get port: {}", e))?
                    .port();
                drop(listener);

                let binary = crate::harness::binary_path("anvil-engine");

                let child = anvil_engine_command(&binary)
                    .arg("--hearth")
                    .arg(&hearth_path)
                    .arg("--port")
                    .arg(port.to_string())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap_or_else(|e| {
                        panic!(
                            "Failed to start anvil-engine at {}: {}",
                            binary.display(),
                            e
                        )
                    });

                std::thread::sleep(std::time::Duration::from_millis(500));

                let process = EngineProcess::new(child, port);
                let mut out = Context::new();
                out.set("engine_process", process);
                Ok(out)
            },
        ),
        // ===== B2: two-hearth fixture + one engine serving both =====
        // Creates two complete hearths X and Y, each with the standard
        // structure (tracks/, tracks.md, a track in `spec`, projections/
        // execution.md) so each satisfies the Req-4 hearth predicate. Spawns ONE
        // hearth-less engine with the common temp parent as the permitted root;
        // X and Y are both reached via per-request hearth_path, matching the
        // production daemon topology.
        // Stores hearth_x_path / hearth_y_path; the `hearth_path` context value
        // remains X, preserving existing scenario placeholders and assertions.
        step_def(
            "two hearth directories X and Y each with the standard structure",
            &[],
            &[
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("hearth_x_path", "PathBuf"),
                ("hearth_y_path", "PathBuf"),
            ],
            |_ctx, _params| {
                let (handle, base) = retained_temp_dir("anvil-multi-hearth-")?;
                let hearth_x = base.join("hearth-X");
                let hearth_y = base.join("hearth-Y");
                seed_standard_hearth(&hearth_x, "X")?;
                seed_standard_hearth(&hearth_y, "Y")?;

                let mut process = start_engine_for_a1(None, &[base])?;
                process.retain_temp_dir(&handle);
                let mut out = Context::new();
                out.set("engine_port", process.port);
                out.set("engine_process", process);
                out.set("hearth_path", hearth_x.clone());
                out.set("hearth_path_handle", handle);
                out.set("hearth_x_path", hearth_x);
                out.set("hearth_y_path", hearth_y);
                Ok(out)
            },
        ),
        step_def(
            "two hearth directories X and Y each with the standard structure and a hearth-less engine",
            &[],
            &[
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("hearth_x_path", "PathBuf"),
                ("hearth_y_path", "PathBuf"),
            ],
            |_ctx, _params| {
                let (handle, base) = retained_temp_dir("anvil-hearthless-multi-")?;
                let permitted_root = base.join("permitted");
                let hearth_x = permitted_root.join("hearth-X");
                let hearth_y = permitted_root.join("hearth-Y");
                seed_standard_hearth(&hearth_x, "X")?;
                seed_standard_hearth(&hearth_y, "Y")?;

                let mut process = start_engine_for_a1(None, &[permitted_root])?;
                process.retain_temp_dir(&handle);
                let mut out = Context::new();
                out.set("engine_port", process.port);
                out.set("engine_process", process);
                out.set("hearth_path", hearth_x.clone());
                out.set("hearth_path_handle", handle);
                out.set("hearth_x_path", hearth_x);
                out.set("hearth_y_path", hearth_y);
                Ok(out)
            },
        ),
        step_def(
            "a standard hearth directory X and a hearth-less engine",
            &[],
            &[
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("hearth_x_path", "PathBuf"),
            ],
            |_ctx, _params| {
                let (handle, base) = retained_temp_dir("anvil-hearthless-empty-")?;
                let hearth_x = base.join("hearth-X");
                seed_standard_hearth(&hearth_x, "X")?;

                let mut process = start_engine_for_a1(None, &[])?;
                process.retain_temp_dir(&handle);
                let mut out = Context::new();
                out.set("engine_port", process.port);
                out.set("engine_process", process);
                out.set("hearth_path", hearth_x.clone());
                out.set("hearth_path_handle", handle);
                out.set("hearth_x_path", hearth_x);
                Ok(out)
            },
        ),
        step_def(
            "a standard hearth directory X and an engine started with it as the default hearth",
            &[],
            &[
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("hearth_x_path", "PathBuf"),
            ],
            |_ctx, _params| {
                let (handle, base) = retained_temp_dir("anvil-default-hearth-")?;
                let hearth_x = base.join("hearth-X");
                seed_standard_hearth(&hearth_x, "X")?;

                let mut process = start_engine_for_a1(Some(&hearth_x), &[])?;
                process.retain_temp_dir(&handle);
                let mut out = Context::new();
                out.set("engine_port", process.port);
                out.set("engine_process", process);
                out.set("hearth_path", hearth_x.clone());
                out.set("hearth_path_handle", handle);
                out.set("hearth_x_path", hearth_x);
                Ok(out)
            },
        ),
        step_def(
            "a request hearth without knowledge_lifecycle and a global playbooks hearth with knowledge_lifecycle body {string}",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("global_playbooks_hearth_path", "PathBuf"),
            ],
            |_ctx, params| {
                let body = params.get_string(0).ok_or("Expected global body")?.to_string();
                let (handle, base) = retained_temp_dir("anvil-global-playbooks-")?;
                let request_hearth = base.join("request-hearth");
                let global_hearth = base.join("global-hearth");
                seed_knowledge_request_hearth(&request_hearth, false, "", false)?;
                seed_knowledge_request_hearth(&global_hearth, true, &body, false)?;
                let mut out = Context::new();
                out.set("hearth_path", request_hearth);
                out.set("hearth_path_handle", handle);
                out.set("global_playbooks_hearth_path", global_hearth);
                Ok(out)
            },
        ),
        step_def(
            "a request hearth without knowledge_lifecycle and a global playbooks hearth with measured knowledge_lifecycle intent {string} expected_output {string} body {string}",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("global_playbooks_hearth_path", "PathBuf"),
            ],
            |_ctx, params| {
                let intent = params.get_string(0).ok_or("Expected global intent")?.to_string();
                let expected_output = params
                    .get_string(1)
                    .ok_or("Expected global expected_output")?
                    .to_string();
                let body = params.get_string(2).ok_or("Expected global body")?.to_string();
                let (handle, base) = retained_temp_dir("anvil-global-measured-")?;
                let request_hearth = base.join("request-hearth");
                let global_hearth = base.join("global-hearth");
                seed_knowledge_request_hearth(&request_hearth, false, "", false)?;
                seed_knowledge_request_hearth_measured(
                    &global_hearth,
                    true,
                    &body,
                    &intent,
                    &expected_output,
                )?;
                let mut out = Context::new();
                out.set("hearth_path", request_hearth);
                out.set("hearth_path_handle", handle);
                out.set("global_playbooks_hearth_path", global_hearth);
                Ok(out)
            },
        ),
        step_def(
            "a request hearth with knowledge_lifecycle body {string} and a global playbooks hearth with knowledge_lifecycle body {string}",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("global_playbooks_hearth_path", "PathBuf"),
            ],
            |_ctx, params| {
                let request_body = params.get_string(0).ok_or("Expected request body")?.to_string();
                let global_body = params.get_string(1).ok_or("Expected global body")?.to_string();
                let (handle, base) = retained_temp_dir("anvil-global-shadow-")?;
                let request_hearth = base.join("request-hearth");
                let global_hearth = base.join("global-hearth");
                seed_knowledge_request_hearth(&request_hearth, true, &request_body, false)?;
                seed_knowledge_request_hearth(&global_hearth, true, &global_body, false)?;
                let mut out = Context::new();
                out.set("hearth_path", request_hearth);
                out.set("hearth_path_handle", handle);
                out.set("global_playbooks_hearth_path", global_hearth);
                Ok(out)
            },
        ),
        step_def(
            "a request hearth with measured knowledge_lifecycle intent {string} expected_output {string} body {string} and a global playbooks hearth with measured knowledge_lifecycle intent {string} expected_output {string} body {string}",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("global_playbooks_hearth_path", "PathBuf"),
            ],
            |_ctx, params| {
                let request_intent = params
                    .get_string(0)
                    .ok_or("Expected request intent")?
                    .to_string();
                let request_expected_output = params
                    .get_string(1)
                    .ok_or("Expected request expected_output")?
                    .to_string();
                let request_body = params
                    .get_string(2)
                    .ok_or("Expected request body")?
                    .to_string();
                let global_intent = params
                    .get_string(3)
                    .ok_or("Expected global intent")?
                    .to_string();
                let global_expected_output = params
                    .get_string(4)
                    .ok_or("Expected global expected_output")?
                    .to_string();
                let global_body = params
                    .get_string(5)
                    .ok_or("Expected global body")?
                    .to_string();
                let (handle, base) = retained_temp_dir("anvil-global-shadow-measured-")?;
                let request_hearth = base.join("request-hearth");
                let global_hearth = base.join("global-hearth");
                seed_knowledge_request_hearth_measured(
                    &request_hearth,
                    true,
                    &request_body,
                    &request_intent,
                    &request_expected_output,
                )?;
                seed_knowledge_request_hearth_measured(
                    &global_hearth,
                    true,
                    &global_body,
                    &global_intent,
                    &global_expected_output,
                )?;
                let mut out = Context::new();
                out.set("hearth_path", request_hearth);
                out.set("hearth_path_handle", handle);
                out.set("global_playbooks_hearth_path", global_hearth);
                Ok(out)
            },
        ),
        step_def(
            "a request hearth with malformed knowledge_lifecycle and a global playbooks hearth with measured knowledge_lifecycle intent {string} expected_output {string} body {string}",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("global_playbooks_hearth_path", "PathBuf"),
            ],
            |_ctx, params| {
                let global_intent = params
                    .get_string(0)
                    .ok_or("Expected global intent")?
                    .to_string();
                let global_expected_output = params
                    .get_string(1)
                    .ok_or("Expected global expected_output")?
                    .to_string();
                let global_body = params
                    .get_string(2)
                    .ok_or("Expected global body")?
                    .to_string();
                let (handle, base) = retained_temp_dir("anvil-global-malformed-selected-")?;
                let request_hearth = base.join("request-hearth");
                let global_hearth = base.join("global-hearth");
                seed_knowledge_request_hearth(&request_hearth, false, "", false)?;
                seed_malformed_knowledge_playbook(&request_hearth)?;
                seed_knowledge_request_hearth_measured(
                    &global_hearth,
                    true,
                    &global_body,
                    &global_intent,
                    &global_expected_output,
                )?;
                let mut out = Context::new();
                out.set("hearth_path", request_hearth);
                out.set("hearth_path_handle", handle);
                out.set("global_playbooks_hearth_path", global_hearth);
                Ok(out)
            },
        ),
        step_def(
            "a request hearth without knowledge_lifecycle and a global playbooks hearth with knowledge_lifecycle body {string} and a {string} to amend edge",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("global_playbooks_hearth_path", "PathBuf"),
            ],
            |_ctx, params| {
                let body = params.get_string(0).ok_or("Expected global body")?.to_string();
                let from_state = params.get_string(1).ok_or("Expected from state")?.to_string();
                if from_state != "published" {
                    return Err(format!(
                        "This fixture only declares the published -> amend edge, got '{}'",
                        from_state
                    ));
                }
                let (handle, base) = retained_temp_dir("anvil-global-amend-")?;
                let request_hearth = base.join("request-hearth");
                let global_hearth = base.join("global-hearth");
                seed_knowledge_request_hearth(&request_hearth, false, "", false)?;
                seed_knowledge_request_hearth(&global_hearth, true, &body, true)?;
                let mut out = Context::new();
                out.set("hearth_path", request_hearth);
                out.set("hearth_path_handle", handle);
                out.set("global_playbooks_hearth_path", global_hearth);
                Ok(out)
            },
        ),
        step_def(
            "the request hearth has a knowledge_lifecycle artifact {string} in state {string}",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("global_playbooks_hearth_path", "PathBuf"),
            ],
            |ctx, params| {
                let id = params.get_string(0).ok_or("Expected artifact id")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let global = ctx.get::<PathBuf>("global_playbooks_hearth_path").cloned();
                seed_knowledge_artifact(&hearth, &id, &state)?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                if let Some(global) = global {
                    out.set("global_playbooks_hearth_path", global);
                }
                Ok(out)
            },
        ),
        step_def(
            "the hearth-less engine is started with the global playbooks hearth",
            &[("hearth_path", "PathBuf"), ("global_playbooks_hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("global_playbooks_hearth_path", "PathBuf"),
            ],
            |ctx, _params| {
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let global = ctx
                    .get::<PathBuf>("global_playbooks_hearth_path")
                    .ok_or("No global_playbooks_hearth_path")?
                    .clone();
                let permitted_root = hearth
                    .parent()
                    .ok_or("Request hearth has no parent for permitted root")?
                    .to_path_buf();
                let mut process = start_engine_for_a2(None, &[permitted_root], Some(&global))?;
                retain_ctx_temp_dir(&mut process, &ctx, "hearth_path_handle");
                let mut out = Context::new();
                out.set("engine_port", process.port);
                out.set("engine_process", process);
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                out.set("global_playbooks_hearth_path", global);
                Ok(out)
            },
        ),
        step_def(
            "a hearth-less engine is started without the global playbooks hearth",
            &[("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, _params| {
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let permitted_root = hearth
                    .parent()
                    .ok_or("Request hearth has no parent for permitted root")?
                    .to_path_buf();
                let mut process = start_engine_for_a2(None, &[permitted_root], None)?;
                retain_ctx_temp_dir(&mut process, &ctx, "hearth_path_handle");
                let mut out = Context::new();
                out.set("engine_port", process.port);
                out.set("engine_process", process);
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        step_def(
            "a permitted root containing standard hearth X and outside standard hearth Y",
            &[],
            &[
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("hearth_x_path", "PathBuf"),
                ("hearth_y_path", "PathBuf"),
                ("hearth_y_parent_escape_path", "PathBuf"),
            ],
            |_ctx, _params| {
                let (handle, base) = retained_temp_dir("anvil-permitted-root-")?;
                let permitted_root = base.join("permitted");
                let hearth_x = permitted_root.join("hearth-X");
                let hearth_y = base.join("outside-hearth-Y");
                seed_standard_hearth(&hearth_x, "X")?;
                seed_standard_hearth(&hearth_y, "Y")?;
                let parent_escape = permitted_root.join("..").join("outside-hearth-Y");

                let mut process = start_engine_for_a1(None, &[permitted_root])?;
                process.retain_temp_dir(&handle);
                let mut out = Context::new();
                out.set("engine_port", process.port);
                out.set("engine_process", process);
                out.set("hearth_path", hearth_x.clone());
                out.set("hearth_path_handle", handle);
                out.set("hearth_x_path", hearth_x);
                out.set("hearth_y_path", hearth_y);
                out.set("hearth_y_parent_escape_path", parent_escape);
                Ok(out)
            },
        ),
        step_def(
            "a permitted root containing standard hearth X and a symlink inside it to outside hearth Y",
            &[],
            &[
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("hearth_x_path", "PathBuf"),
                ("hearth_y_path", "PathBuf"),
                ("hearth_y_symlink_path", "PathBuf"),
            ],
            |_ctx, _params| {
                let (handle, base) = retained_temp_dir("anvil-symlink-root-")?;
                let permitted_root = base.join("permitted");
                let hearth_x = permitted_root.join("hearth-X");
                let hearth_y = base.join("outside-hearth-Y");
                let symlink_path = permitted_root.join("link-to-hearth-Y");
                seed_standard_hearth(&hearth_x, "X")?;
                seed_standard_hearth(&hearth_y, "Y")?;
                create_dir_symlink(&hearth_y, &symlink_path)?;

                let mut process = start_engine_for_a1(None, &[permitted_root])?;
                process.retain_temp_dir(&handle);
                let mut out = Context::new();
                out.set("engine_port", process.port);
                out.set("engine_process", process);
                out.set("hearth_path", hearth_x.clone());
                out.set("hearth_path_handle", handle);
                out.set("hearth_x_path", hearth_x);
                out.set("hearth_y_path", hearth_y);
                out.set("hearth_y_symlink_path", symlink_path);
                Ok(out)
            },
        ),
        step_def(
            "standard hearth X is the default and outside any configured permitted root",
            &[],
            &[
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("hearth_x_path", "PathBuf"),
            ],
            |_ctx, _params| {
                let (handle, base) = retained_temp_dir("anvil-default-implicit-root-")?;
                let hearth_x = base.join("outside-default-hearth-X");
                let unrelated_root = base.join("unrelated-permitted-root");
                std::fs::create_dir_all(&unrelated_root)
                    .map_err(|e| format!("Failed to create unrelated permitted root: {}", e))?;
                seed_standard_hearth(&hearth_x, "X")?;

                let mut process = start_engine_for_a1(Some(&hearth_x), &[unrelated_root])?;
                process.retain_temp_dir(&handle);
                let mut out = Context::new();
                out.set("engine_port", process.port);
                out.set("engine_process", process);
                out.set("hearth_path", hearth_x.clone());
                out.set("hearth_path_handle", handle);
                out.set("hearth_x_path", hearth_x);
                Ok(out)
            },
        ),
        // Snapshot the byte contents of every file under hearth Y, for the
        // isolation assertion (a transition on X must leave Y byte-unchanged).
        step_def(
            "hearth Y's file contents are recorded",
            &[("hearth_y_path", "PathBuf"), ("engine_process", "EngineProcess")],
            &[
                ("hearth_y_path", "PathBuf"),
                ("hearth_y_snapshot", "HearthSnapshot"),
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_x_path", "PathBuf"),
            ],
            |mut ctx, _params| {
                let hearth_y = ctx.get::<PathBuf>("hearth_y_path").ok_or("No hearth_y_path")?.clone();
                let snapshot = snapshot_tree(&hearth_y);
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").cloned();
                let hearth_x = ctx.get::<PathBuf>("hearth_x_path").cloned();
                let mut out = Context::new();
                out.set("hearth_y_path", hearth_y);
                out.set("hearth_y_snapshot", HearthSnapshot { files: snapshot });
                out.set("engine_process", engine);
                if let Some(p) = hearth_path { out.set("hearth_path", p); }
                if let Some(p) = hearth_x { out.set("hearth_x_path", p); }
                Ok(out)
            },
        ),
        check_def(
            "hearth Y's files are byte-unchanged",
            &[("hearth_y_path", "PathBuf"), ("hearth_y_snapshot", "HearthSnapshot")],
            |ctx, _params| {
                let hearth_y = ctx.get::<PathBuf>("hearth_y_path").ok_or("No hearth_y_path")?;
                let recorded = ctx.get::<HearthSnapshot>("hearth_y_snapshot").ok_or("No hearth_y_snapshot")?;
                let current = snapshot_tree(hearth_y);
                if current == recorded.files {
                    Ok(())
                } else {
                    Err(format!(
                        "Hearth Y changed: recorded {} files, now {} files (or contents differ)",
                        recorded.files.len(),
                        current.len()
                    ))
                }
            },
        ),
        // ===== B3 / N3: cross-RPC same-hearth concurrency =====
        // Issues a snapshot (projection-only spark) and a complete to the SAME
        // hearth concurrently against ONE engine, then to DIFFERENT hearths.
        // Both routes obtain their guard from the one HearthLocks instance, so
        // same-hearth writes serialize (no deadlock, no torn registry — backed
        // by atomic_write) and different-hearth writes do not block. Asserts
        // both outcomes succeeded; the deterministic serialization proof lives
        // at the anvil-core HearthLocks seam (AC5).
        async_step_def(
            "concurrent snapshot and complete are issued to the same hearth",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                ("snapshot_rpc_result", "SnapshotRpcResult"),
                ("complete_rpc_result", "CompleteRpcResult"),
                ("hearth_path", "PathBuf"),
                ("hearth_x_path", "PathBuf"),
                ("hearth_y_path", "PathBuf"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let port = engine.port;
                let hearth_x = ctx.get::<PathBuf>("hearth_x_path").ok_or("No hearth_x_path")?.to_string_lossy().into_owned();
                let hearth_path = ctx.get::<PathBuf>("hearth_path").cloned();
                let hearth_x_pb = ctx.get::<PathBuf>("hearth_x_path").cloned();
                let hearth_y_pb = ctx.get::<PathBuf>("hearth_y_path").cloned();

                let addr = format!("http://127.0.0.1:{}", port);
                let snap_hearth = hearth_x.clone();
                let comp_hearth = hearth_x.clone();
                let snap_addr = addr.clone();
                let comp_addr = addr.clone();

                let snap_fut = async move {
                    let mut client = anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(snap_addr).await
                        .map_err(|e| format!("snap connect: {}", e))?;
                    let req = anvil_engine::proto::SnapshotRequest {
                        hearth_path: snap_hearth,
                        artifact_path: "sparks/sparks.md".to_string(),
                        projection_only: true,
                        event_type: "spark".to_string(),
                        ..Default::default()
                    };
                    match client.snapshot(crate::surfaced(req)).await {
                        Ok(r) => Ok::<SnapshotRpcResult, String>(SnapshotRpcResult::Success(r.into_inner())),
                        Err(s) => Ok(SnapshotRpcResult::Error { code: grpc_code_name(s.code()), message: s.message().to_string() }),
                    }
                };
                let comp_fut = async move {
                    let mut client = anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(comp_addr).await
                        .map_err(|e| format!("comp connect: {}", e))?;
                    let req = anvil_engine::proto::CompleteRequest {
                        artifact_path: "tracks/20260419T1100_track_x".to_string(),
                        actor_name: "Concurrent-Doer-555555".to_string(),
                        actor_type: "agent".to_string(),
                        actor_model: "claude-opus-4-7".to_string(),
                        actor_provider: "anthropic".to_string(),
                        actor_context_window: 200000,
                        actor_entrypoint: "claude-code".to_string(),
                        hearth_path: comp_hearth,
                        ..Default::default()
                    };
                    match client.complete(crate::surfaced(req)).await {
                        Ok(r) => Ok::<CompleteRpcResult, String>(CompleteRpcResult::Success(r.into_inner())),
                        Err(s) => Ok(CompleteRpcResult::Error { code: grpc_code_name(s.code()), message: s.message().to_string() }),
                    }
                };

                let (snap_res, comp_res) = tokio::join!(snap_fut, comp_fut);
                let snapshot_rpc_result = snap_res?;
                let complete_rpc_result = comp_res?;

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("snapshot_rpc_result", snapshot_rpc_result);
                out.set("complete_rpc_result", complete_rpc_result);
                if let Some(p) = hearth_path { out.set("hearth_path", p); }
                if let Some(p) = hearth_x_pb { out.set("hearth_x_path", p); }
                if let Some(p) = hearth_y_pb { out.set("hearth_y_path", p); }
                Ok(out)
            },
        ),
        async_step_def(
            "the catalog RPC is called",
            &[("engine_process", "EngineProcess")],
            &[
                ("catalog_result", "CatalogRpcResult"),
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_x_path", "PathBuf"),
                ("hearth_y_path", "PathBuf"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let port = engine.port;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").cloned();
                let hearth_x = ctx.get::<PathBuf>("hearth_x_path").cloned();
                let hearth_y = ctx.get::<PathBuf>("hearth_y_path").cloned();

                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::CatalogRequest {
                            hearth_path: String::new(),
                        });

                        match client.catalog(request).await {
                            Ok(response) => CatalogRpcResult::Success(catalog_response_from_proto(response.into_inner())),
                            Err(status) => CatalogRpcResult::Error(format!("gRPC error: {}", status)),
                        }
                    }
                    Err(e) => CatalogRpcResult::Error(format!("Connection failed: {}", e)),
                };

                let mut out = Context::new();
                out.set("catalog_result", result);
                out.set("engine_process", engine);
                if let Some(p) = hearth_path { out.set("hearth_path", p); }
                if let Some(p) = hearth_x { out.set("hearth_x_path", p); }
                if let Some(p) = hearth_y { out.set("hearth_y_path", p); }
                Ok(out)
            },
        ),
        // B2: catalog targeting a specific hearth by per-request hearth_path.
        // "X" / "Y" select the two-hearth fixture's hearths; any other value is
        // sent verbatim (used by confinement/binding scenarios).
        async_step_def(
            "the catalog RPC is called for hearth {string}",
            &[("engine_process", "EngineProcess")],
            &[
                ("catalog_result", "CatalogRpcResult"),
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_x_path", "PathBuf"),
                ("hearth_y_path", "PathBuf"),
            ],
            |mut ctx, params| async move {
                let selector = params.get_string(0).ok_or("Expected hearth selector")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let port = engine.port;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").cloned();
                let hearth_x = ctx.get::<PathBuf>("hearth_x_path").cloned();
                let hearth_y = ctx.get::<PathBuf>("hearth_y_path").cloned();
                let req_hearth = match selector.as_str() {
                    "X" => hearth_x.as_ref().ok_or("No hearth_x_path")?.to_string_lossy().into_owned(),
                    "Y" => hearth_y.as_ref().ok_or("No hearth_y_path")?.to_string_lossy().into_owned(),
                    other => other.to_string(),
                };

                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::CatalogRequest {
                            hearth_path: req_hearth,
                        });
                        match client.catalog(request).await {
                            Ok(response) => CatalogRpcResult::Success(catalog_response_from_proto(response.into_inner())),
                            Err(status) => CatalogRpcResult::Error(format!("gRPC error: {}: {}", grpc_code_name(status.code()), status.message())),
                        }
                    }
                    Err(e) => CatalogRpcResult::Error(format!("Connection failed: {}", e)),
                };

                let mut out = Context::new();
                out.set("catalog_result", result);
                out.set("engine_process", engine);
                if let Some(p) = hearth_path { out.set("hearth_path", p); }
                if let Some(p) = hearth_x { out.set("hearth_x_path", p); }
                if let Some(p) = hearth_y { out.set("hearth_y_path", p); }
                Ok(out)
            },
        ),
        async_step_def(
            "the catalog RPC is called for the request hearth",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("catalog_result", "CatalogRpcResult"),
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("global_playbooks_hearth_path", "PathBuf"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let global = ctx.get::<PathBuf>("global_playbooks_hearth_path").cloned();
                let result = call_catalog_with_hearth(&engine, hearth.to_string_lossy().into_owned()).await;
                let mut out = Context::new();
                out.set("catalog_result", result);
                out.set("engine_process", engine);
                out.set("hearth_path", hearth);
                if let Some(global) = global {
                    out.set("global_playbooks_hearth_path", global);
                }
                Ok(out)
            },
        ),
        async_step_def(
            "the catalog RPC is called for the parent escape to hearth Y",
            &[("engine_process", "EngineProcess"), ("hearth_y_parent_escape_path", "PathBuf")],
            &[
                ("catalog_result", "CatalogRpcResult"),
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_x_path", "PathBuf"),
                ("hearth_y_path", "PathBuf"),
                ("hearth_y_parent_escape_path", "PathBuf"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let req_hearth = ctx
                    .get::<PathBuf>("hearth_y_parent_escape_path")
                    .ok_or("No hearth_y_parent_escape_path")?
                    .to_string_lossy()
                    .into_owned();
                let result = call_catalog_with_hearth(&engine, req_hearth).await;
                let mut out = Context::new();
                out.set("catalog_result", result);
                out.set("engine_process", engine);
                if let Some(p) = ctx.get::<PathBuf>("hearth_path") { out.set("hearth_path", p.clone()); }
                if let Some(p) = ctx.get::<PathBuf>("hearth_x_path") { out.set("hearth_x_path", p.clone()); }
                if let Some(p) = ctx.get::<PathBuf>("hearth_y_path") { out.set("hearth_y_path", p.clone()); }
                if let Some(p) = ctx.get::<PathBuf>("hearth_y_parent_escape_path") { out.set("hearth_y_parent_escape_path", p.clone()); }
                Ok(out)
            },
        ),
        async_step_def(
            "the catalog RPC is called for hearth symlink {string}",
            &[("engine_process", "EngineProcess"), ("hearth_y_symlink_path", "PathBuf")],
            &[
                ("catalog_result", "CatalogRpcResult"),
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_x_path", "PathBuf"),
                ("hearth_y_path", "PathBuf"),
                ("hearth_y_symlink_path", "PathBuf"),
            ],
            |mut ctx, params| async move {
                let selector = params.get_string(0).ok_or("Expected hearth selector")?.to_string();
                if selector != "Y" {
                    return Err(format!("Unsupported symlink hearth selector '{}'", selector));
                }
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let req_hearth = ctx
                    .get::<PathBuf>("hearth_y_symlink_path")
                    .ok_or("No hearth_y_symlink_path")?
                    .to_string_lossy()
                    .into_owned();
                let result = call_catalog_with_hearth(&engine, req_hearth).await;
                let mut out = Context::new();
                out.set("catalog_result", result);
                out.set("engine_process", engine);
                if let Some(p) = ctx.get::<PathBuf>("hearth_path") { out.set("hearth_path", p.clone()); }
                if let Some(p) = ctx.get::<PathBuf>("hearth_x_path") { out.set("hearth_x_path", p.clone()); }
                if let Some(p) = ctx.get::<PathBuf>("hearth_y_path") { out.set("hearth_y_path", p.clone()); }
                if let Some(p) = ctx.get::<PathBuf>("hearth_y_symlink_path") { out.set("hearth_y_symlink_path", p.clone()); }
                Ok(out)
            },
        ),
        check_def(
            "the catalog RPC returns gRPC error containing {string}",
            &[("catalog_result", "CatalogRpcResult")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?.to_string();
                let result = ctx.get::<CatalogRpcResult>("catalog_result").ok_or("No catalog_result")?;
                match result {
                    CatalogRpcResult::Error(e) => {
                        if e.contains(&needle) {
                            Ok(())
                        } else {
                            Err(format!("Expected catalog error containing '{}', got '{}'", needle, e))
                        }
                    }
                    CatalogRpcResult::Status { message, .. } => {
                        if message.contains(&needle) {
                            Ok(())
                        } else {
                            Err(format!("Expected catalog error containing '{}', got '{}'", needle, message))
                        }
                    }
                    CatalogRpcResult::Success(_) => Err(format!("Expected a gRPC error containing '{}', got success", needle)),
                }
            },
        ),
        check_def(
            "the catalog response contains {int} active artifacts",
            &[("catalog_result", "CatalogRpcResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let result = ctx.get::<CatalogRpcResult>("catalog_result").ok_or("No catalog_result")?;
                match result {
                    CatalogRpcResult::Success(resp) => {
                        if resp.active_artifacts.len() != expected {
                            Err(format!(
                                "Expected {} active artifacts, got {} ({:?})",
                                expected,
                                resp.active_artifacts.len(),
                                resp.active_artifacts.iter().map(|a| &a.id).collect::<Vec<_>>()
                            ))
                        } else {
                            Ok(())
                        }
                    }
                    CatalogRpcResult::Error(e) => Err(format!("Expected success, got error: {}", e)),
                    CatalogRpcResult::Status { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the catalog response includes artifact {string} with type {string}",
            &[("catalog_result", "CatalogRpcResult")],
            |ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?;
                let artifact_type = params.get_string(1).ok_or("Expected type")?;
                let result = ctx.get::<CatalogRpcResult>("catalog_result").ok_or("No catalog_result")?;
                match result {
                    CatalogRpcResult::Success(resp) => {
                        let found = resp.active_artifacts.iter().find(|a| a.id == id);
                        match found {
                            None => Err(format!("Artifact '{}' not found", id)),
                            Some(a) => {
                                if a.artifact_type != artifact_type {
                                    Err(format!("Artifact '{}' type: expected '{}', got '{}'", id, artifact_type, a.artifact_type))
                                } else {
                                    Ok(())
                                }
                            }
                        }
                    }
                    CatalogRpcResult::Error(e) => Err(format!("Expected success, got error: {}", e)),
                    CatalogRpcResult::Status { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the active artifact {string} has execution_route {string}",
            &[("catalog_result", "CatalogRpcResult")],
            |ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?;
                let expected = params.get_string(1).ok_or("Expected execution_route")?;
                let result = ctx.get::<CatalogRpcResult>("catalog_result").ok_or("No catalog_result")?;
                match result {
                    CatalogRpcResult::Success(resp) => {
                        let found = resp.active_artifacts.iter().find(|a| a.id == id);
                        match found {
                            None => Err(format!("Artifact '{}' not found in active_artifacts", id)),
                            Some(a) => {
                                if a.execution_route != expected {
                                    Err(format!(
                                        "Artifact '{}' execution_route: expected '{}', got '{}'",
                                        id, expected, a.execution_route
                                    ))
                                } else {
                                    Ok(())
                                }
                            }
                        }
                    }
                    CatalogRpcResult::Error(e) => Err(format!("Expected success, got error: {}", e)),
                    CatalogRpcResult::Status { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the catalog response does not include {string}",
            &[("catalog_result", "CatalogRpcResult")],
            |ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?;
                let result = ctx.get::<CatalogRpcResult>("catalog_result").ok_or("No catalog_result")?;
                match result {
                    CatalogRpcResult::Success(resp) => {
                        if resp.active_artifacts.iter().any(|a| a.id == id) {
                            Err(format!("Artifact '{}' should not be in response", id))
                        } else {
                            Ok(())
                        }
                    }
                    CatalogRpcResult::Error(e) => Err(format!("Expected success, got error: {}", e)),
                    CatalogRpcResult::Status { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the catalog response contains {int} available artifact types",
            &[("catalog_result", "CatalogRpcResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let result = ctx.get::<CatalogRpcResult>("catalog_result").ok_or("No catalog_result")?;
                match result {
                    CatalogRpcResult::Success(resp) => {
                        if resp.available_types.len() != expected {
                            Err(format!(
                                "Expected {} available types, got {}",
                                expected, resp.available_types.len()
                            ))
                        } else {
                            Ok(())
                        }
                    }
                    CatalogRpcResult::Error(e) => Err(format!("Expected success, got error: {}", e)),
                    CatalogRpcResult::Status { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the catalog available playbook kinds include {string}",
            &[("catalog_result", "CatalogRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected playbook kind")?;
                let result = ctx.get::<CatalogRpcResult>("catalog_result").ok_or("No catalog_result")?;
                match result {
                    CatalogRpcResult::Success(resp) => {
                        if resp
                            .available_artifact_kinds
                            .iter()
                            .any(|k| k.kind == expected)
                        {
                            Ok(())
                        } else {
                            Err(format!(
                                "Playbook kind '{}' not found in available_artifact_kinds {:?}",
                                expected,
                                resp.available_artifact_kinds
                                    .iter()
                                    .map(|k| format!("{}:{}", k.kind, k.source_tier))
                                    .collect::<Vec<_>>()
                            ))
                        }
                    }
                    CatalogRpcResult::Error(e) => Err(format!("Expected success, got error: {}", e)),
                    CatalogRpcResult::Status { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the catalog available playbook kinds do not include {string}",
            &[("catalog_result", "CatalogRpcResult")],
            |ctx, params| {
                let unexpected = params.get_string(0).ok_or("Expected playbook kind")?;
                let result = ctx.get::<CatalogRpcResult>("catalog_result").ok_or("No catalog_result")?;
                match result {
                    CatalogRpcResult::Success(resp) => {
                        if resp
                            .available_artifact_kinds
                            .iter()
                            .any(|k| k.kind == unexpected)
                        {
                            Err(format!(
                                "Playbook kind '{}' should have been dropped from available_artifact_kinds but is present {:?}",
                                unexpected,
                                resp.available_artifact_kinds
                                    .iter()
                                    .map(|k| format!("{}:{}", k.kind, k.source_tier))
                                    .collect::<Vec<_>>()
                            ))
                        } else {
                            Ok(())
                        }
                    }
                    CatalogRpcResult::Error(e) => Err(format!("Expected success, got error: {}", e)),
                    CatalogRpcResult::Status { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the catalog available playbook kind {string} reports described {string} and triggers {string}",
            &[("catalog_result", "CatalogRpcResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected playbook kind")?;
                let expected_is_described = parse_bool_param(
                    params.get_string(1).ok_or("Expected described flag")?,
                )?;
                let expected_has_triggers = parse_bool_param(
                    params.get_string(2).ok_or("Expected triggers flag")?,
                )?;
                let result = ctx.get::<CatalogRpcResult>("catalog_result").ok_or("No catalog_result")?;
                match result {
                    CatalogRpcResult::Success(resp) => {
                        let artifact_kind = resp
                            .available_artifact_kinds
                            .iter()
                            .find(|k| k.kind == kind)
                            .ok_or_else(|| {
                                format!(
                                    "Playbook kind '{}' not found in available_artifact_kinds {:?}",
                                    kind,
                                    resp.available_artifact_kinds
                                        .iter()
                                        .map(|k| k.kind.as_str())
                                        .collect::<Vec<_>>()
                                )
                            })?;
                        if artifact_kind.is_described != expected_is_described
                            || artifact_kind.has_triggers != expected_has_triggers
                        {
                            Err(format!(
                                "Playbook kind '{}' expected is_described={} has_triggers={}, got is_described={} has_triggers={}",
                                kind,
                                expected_is_described,
                                expected_has_triggers,
                                artifact_kind.is_described,
                                artifact_kind.has_triggers
                            ))
                        } else {
                            Ok(())
                        }
                    }
                    CatalogRpcResult::Error(e) => Err(format!("Expected success, got error: {}", e)),
                    CatalogRpcResult::Status { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the available types include {string} with description containing {string}",
            &[("catalog_result", "CatalogRpcResult")],
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected name")?;
                let desc_fragment = params.get_string(1).ok_or("Expected description")?;
                let result = ctx.get::<CatalogRpcResult>("catalog_result").ok_or("No catalog_result")?;
                match result {
                    CatalogRpcResult::Success(resp) => {
                        let found = resp.available_types.iter().find(|t| t.name == name);
                        match found {
                            None => Err(format!("Available type '{}' not found", name)),
                            Some(t) => {
                                if !t.description.contains(desc_fragment) {
                                    Err(format!(
                                        "Type '{}' description '{}' does not contain '{}'",
                                        name, t.description, desc_fragment
                                    ))
                                } else {
                                    Ok(())
                                }
                            }
                        }
                    }
                    CatalogRpcResult::Error(e) => Err(format!("Expected success, got error: {}", e)),
                    CatalogRpcResult::Status { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the available types include {string} requiring parent {string}",
            &[("catalog_result", "CatalogRpcResult")],
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected name")?;
                let parent = params.get_string(1).ok_or("Expected parent")?;
                let result = ctx.get::<CatalogRpcResult>("catalog_result").ok_or("No catalog_result")?;
                match result {
                    CatalogRpcResult::Success(resp) => {
                        let found = resp.available_types.iter().find(|t| t.name == name);
                        match found {
                            None => Err(format!("Available type '{}' not found", name)),
                            Some(t) => {
                                if t.requires_parent != parent {
                                    Err(format!(
                                        "Type '{}' requires_parent: expected '{}', got '{}'",
                                        name, parent, t.requires_parent
                                    ))
                                } else {
                                    Ok(())
                                }
                            }
                        }
                    }
                    CatalogRpcResult::Error(e) => Err(format!("Expected success, got error: {}", e)),
                    CatalogRpcResult::Status { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the available types include {string} with no parent required",
            &[("catalog_result", "CatalogRpcResult")],
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected name")?;
                let result = ctx.get::<CatalogRpcResult>("catalog_result").ok_or("No catalog_result")?;
                match result {
                    CatalogRpcResult::Success(resp) => {
                        let found = resp.available_types.iter().find(|t| t.name == name);
                        match found {
                            None => Err(format!("Available type '{}' not found", name)),
                            Some(t) => {
                                if !t.requires_parent.is_empty() {
                                    Err(format!(
                                        "Type '{}' should have no parent, but requires '{}'",
                                        name, t.requires_parent
                                    ))
                                } else {
                                    Ok(())
                                }
                            }
                        }
                    }
                    CatalogRpcResult::Error(e) => Err(format!("Expected success, got error: {}", e)),
                    CatalogRpcResult::Status { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the catalog RPC returns a gRPC error",
            &[("catalog_result", "CatalogRpcResult")],
            |ctx, _params| {
                let result = ctx.get::<CatalogRpcResult>("catalog_result").ok_or("No catalog_result")?;
                match result {
                    CatalogRpcResult::Error(_) => Ok(()),
                    CatalogRpcResult::Status { .. } => Ok(()),
                    CatalogRpcResult::Success(resp) => Err(format!(
                        "Expected gRPC error, got success with {} artifacts",
                        resp.active_artifacts.len()
                    )),
                }
            },
        ),
        // ===== CQRS logging: stderr scrape steps (N3) =====
        // Field-parsed JSON matcher. Each captured stderr line is parsed as a
        // serde_json::Value; we match requested keys as NAMED top-level fields
        // (the subscriber uses `.flatten_event(true)`, so event fields are at
        // the top level — confirmed empirically in T2.2). Never substring/regex
        // the whole line: the timestamp + level keys vary across runs.
        check_def(
            "the engine stderr contains a JSON log record with fields:",
            &[("engine_process", "EngineProcess")],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                // Collect (key, expected) pairs. The header row is the first
                // pair (mirrors the snapshot-RPC step's table convention).
                let mut wanted: Vec<(String, String)> = Vec::new();
                if table.headers.len() >= 2 {
                    wanted.push((
                        table.headers[0].trim().to_string(),
                        table.headers[1].trim().to_string(),
                    ));
                }
                for row in &table.rows {
                    if row.len() >= 2 {
                        wanted.push((row[0].trim().to_string(), row[1].trim().to_string()));
                    }
                }
                let engine = ctx
                    .get::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;

                // Poll-until-match with bounded retry so the assertion never
                // races the log flush (Req 7 deterministic sync).
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                let mut last_seen: Vec<String> = Vec::new();
                loop {
                    let lines = engine.stderr_lines();
                    for line in &lines {
                        let value: serde_json::Value = match serde_json::from_str(line) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };
                        let obj = match value.as_object() {
                            Some(o) => o,
                            None => continue,
                        };
                        let all_match = wanted.iter().all(|(k, expected)| {
                            match obj.get(k) {
                                Some(field) => json_field_matches(field, expected),
                                None => false,
                            }
                        });
                        if all_match {
                            return Ok(());
                        }
                    }
                    last_seen = lines;
                    if std::time::Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
                Err(format!(
                    "No JSON log record matched all fields {:?}.\nCaptured stderr lines ({}):\n{}",
                    wanted,
                    last_seen.len(),
                    last_seen.join("\n")
                ))
            },
        ),
        // Crucible publication-log: assert a canonical envelope line landed in
        // the standard `events-YYYY-MM-DD.jsonl` file under the configured
        // publication-log dir, that it parses as JSON, and that it carries the
        // expected `kind`. Poll-until-present so we never race the best-effort
        // mirror's append.
        check_def(
            "the publication log contains an event with kind {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                // The publication-log dir is rooted at a stable path inside the
                // (retained) hearth temp tree — derive it from `hearth_path`,
                // which threads through every step, rather than a dedicated key
                // that intermediate RPC steps drop.
                let dir = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .join("__publication_log__");

                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                let mut last_lines: Vec<String> = Vec::new();
                loop {
                    // Scan every events-*.jsonl in the dir (one per UTC day).
                    let mut lines: Vec<String> = Vec::new();
                    if let Ok(entries) = std::fs::read_dir(&dir) {
                        for entry in entries.flatten() {
                            let name = entry.file_name();
                            let name = name.to_string_lossy();
                            if name.starts_with("events-") && name.ends_with(".jsonl") {
                                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                                    lines.extend(content.lines().map(str::to_string));
                                }
                            }
                        }
                    }
                    for line in &lines {
                        let value: serde_json::Value = match serde_json::from_str(line) {
                            Ok(v) => v,
                            Err(_) => continue, // not the canonical envelope shape
                        };
                        // Canonical envelope: kind present + valid JSON.
                        if value.get("kind").and_then(|k| k.as_str()) == Some(kind.as_str())
                            && value.get("eventId").and_then(|e| e.as_str()).is_some()
                            && value.get("timestamp").is_some()
                            && value.get("data").is_some()
                        {
                            return Ok(());
                        }
                    }
                    last_lines = lines;
                    if std::time::Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
                Err(format!(
                    "No publication-log envelope with kind {:?} in {}.\nFound {} line(s):\n{}",
                    kind,
                    dir.display(),
                    last_lines.len(),
                    last_lines.join("\n")
                ))
            },
        ),
        // M7: identity-strict sibling of the field-match step. Proves a JSON log
        // record matches every requested literal field AND its `playbook_id`
        // equals the begin RPC response's run instance id (the last path segment
        // of `track_path`). This is what makes a begin step_measurement record
        // provably carry the per-RUN instance id (not the playbook definition id,
        // not the kind) — and, combined with a `track_id` literal in the same
        // table, proves `playbook_id != track_id` without a cross-field step.
        check_def(
            "the engine stderr contains a JSON log record with playbook_id equal to the begin RPC response run id and fields:",
            &[
                ("engine_process", "EngineProcess"),
                ("begin_result", "BeginRpcResult"),
            ],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let mut wanted: Vec<(String, String)> = Vec::new();
                if table.headers.len() >= 2 {
                    wanted.push((
                        table.headers[0].trim().to_string(),
                        table.headers[1].trim().to_string(),
                    ));
                }
                for row in &table.rows {
                    if row.len() >= 2 {
                        wanted.push((row[0].trim().to_string(), row[1].trim().to_string()));
                    }
                }

                // Resolve the begin-created run instance id = last path segment of
                // the begin response track_path (mirrors the engine's
                // `last_segment(track_path)` used to build playbook_id).
                let begin = ctx
                    .get::<BeginRpcResult>("begin_result")
                    .ok_or("No begin_result")?;
                let track_path = match begin {
                    BeginRpcResult::Success(r) => r.track_path.clone(),
                    BeginRpcResult::Error { code, message } => {
                        return Err(format!("Expected begin success, got gRPC {}: {}", code, message))
                    }
                };
                let run_id = track_path
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .to_string();
                if run_id.is_empty() {
                    return Err(format!("begin track_path '{}' has no run id segment", track_path));
                }

                let engine = ctx
                    .get::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                let mut last_seen: Vec<String> = Vec::new();
                loop {
                    let lines = engine.stderr_lines();
                    for line in &lines {
                        let value: serde_json::Value = match serde_json::from_str(line) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };
                        let obj = match value.as_object() {
                            Some(o) => o,
                            None => continue,
                        };
                        let literals_match = wanted.iter().all(|(k, expected)| match obj.get(k) {
                            Some(field) => json_field_matches(field, expected),
                            None => false,
                        });
                        let playbook_id_match = obj
                            .get("playbook_id")
                            .and_then(|v| v.as_str())
                            .map(|s| s == run_id)
                            .unwrap_or(false);
                        if literals_match && playbook_id_match {
                            return Ok(());
                        }
                    }
                    last_seen = lines;
                    if std::time::Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
                Err(format!(
                    "No JSON log record matched fields {:?} AND playbook_id == begin run id '{}'.\nCaptured stderr lines ({}):\n{}",
                    wanted,
                    run_id,
                    last_seen.len(),
                    last_seen.join("\n")
                ))
            },
        ),
        // ===== Step-measurement: count-aware + event_kind-scoped steps (M-P3) =====
        // Count-aware sibling of the field-match step. Counts every JSON record
        // whose `event_kind` equals the given value AND matches every requested
        // field, then asserts the count equals exactly N. Unlike the at-least-one
        // field-match step, this proves "exactly one emit per call" (AC-7). The
        // count is read after the 2s flush deadline so it never races the log
        // flush nor over-counts an in-flight record.
        check_def(
            "the engine stderr contains exactly {int} JSON log records with event_kind {string}",
            &[("engine_process", "EngineProcess")],
            |ctx, params| {
                let expected_count = params.get_int(0).ok_or("Expected count")? as usize;
                let event_kind = params.get_string(1).ok_or("Expected event_kind")?.to_string();
                let engine = ctx
                    .get::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                // Settle: let the log flush before counting (mirrors the
                // field-match step's 2s deadline, but counts at the end).
                std::thread::sleep(std::time::Duration::from_secs(2));
                let lines = engine.stderr_lines();
                let count = lines
                    .iter()
                    .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                    .filter_map(|v| v.as_object().cloned())
                    .filter(|obj| {
                        obj.get("event_kind")
                            .and_then(|v| v.as_str())
                            .map(|s| s == event_kind)
                            .unwrap_or(false)
                    })
                    .count();
                if count == expected_count {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected exactly {} records with event_kind '{}', found {}.\nCaptured stderr lines ({}):\n{}",
                        expected_count,
                        event_kind,
                        count,
                        lines.len(),
                        lines.join("\n")
                    ))
                }
            },
        ),
        check_def(
            "the engine stderr contains exactly {int} JSON log records with event_kind {string}, playbook_id {string}, and to_state in:",
            &[("engine_process", "EngineProcess")],
            |ctx, params| {
                let expected_count = params.get_int(0).ok_or("Expected count")? as usize;
                let event_kind = params.get_string(1).ok_or("Expected event_kind")?.to_string();
                let playbook_id = params.get_string(2).ok_or("Expected playbook_id")?.to_string();
                let table = params.data_table().ok_or("Expected to_state data table")?;

                let mut allowed_to_states = std::collections::BTreeSet::new();
                if let Some(header) = table.headers.first() {
                    allowed_to_states.insert(header.trim().to_string());
                }
                for row in &table.rows {
                    if let Some(cell) = row.first() {
                        allowed_to_states.insert(cell.trim().to_string());
                    }
                }
                allowed_to_states.retain(|state| !state.is_empty());
                if allowed_to_states.is_empty() {
                    return Err("Expected at least one to_state value".to_string());
                }

                let engine = ctx
                    .get::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                std::thread::sleep(std::time::Duration::from_secs(2));
                let lines = engine.stderr_lines();
                let count = lines
                    .iter()
                    .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                    .filter_map(|v| v.as_object().cloned())
                    .filter(|obj| {
                        obj.get("event_kind")
                            .and_then(|v| v.as_str())
                            .map(|s| s == event_kind)
                            .unwrap_or(false)
                            && obj
                                .get("playbook_id")
                                .and_then(|v| v.as_str())
                                .map(|s| s == playbook_id)
                                .unwrap_or(false)
                            && obj
                                .get("to_state")
                                .and_then(|v| v.as_str())
                                .map(|s| allowed_to_states.contains(s))
                                .unwrap_or(false)
                    })
                    .count();
                if count == expected_count {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected exactly {} records with event_kind '{}', playbook_id '{}', and to_state in {:?}, found {}.\nCaptured stderr lines ({}):\n{}",
                        expected_count,
                        event_kind,
                        playbook_id,
                        allowed_to_states,
                        count,
                        lines.len(),
                        lines.join("\n")
                    ))
                }
            },
        ),
        // track_id-keyed sibling of the playbook_id step above. After H1,
        // step_measurement records carry track_id = the playbook KIND (the
        // cross-instance aggregation key) and playbook_id = the per-run instance
        // id; per-kind step coverage filters on track_id.
        check_def(
            "the engine stderr contains exactly {int} JSON log records with event_kind {string}, track_id {string}, and to_state in:",
            &[("engine_process", "EngineProcess")],
            |ctx, params| {
                let expected_count = params.get_int(0).ok_or("Expected count")? as usize;
                let event_kind = params.get_string(1).ok_or("Expected event_kind")?.to_string();
                let track_id = params.get_string(2).ok_or("Expected track_id")?.to_string();
                let table = params.data_table().ok_or("Expected to_state data table")?;

                let mut allowed_to_states = std::collections::BTreeSet::new();
                if let Some(header) = table.headers.first() {
                    allowed_to_states.insert(header.trim().to_string());
                }
                for row in &table.rows {
                    if let Some(cell) = row.first() {
                        allowed_to_states.insert(cell.trim().to_string());
                    }
                }
                allowed_to_states.retain(|state| !state.is_empty());
                if allowed_to_states.is_empty() {
                    return Err("Expected at least one to_state value".to_string());
                }

                let engine = ctx
                    .get::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                std::thread::sleep(std::time::Duration::from_secs(2));
                let lines = engine.stderr_lines();
                let count = lines
                    .iter()
                    .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                    .filter_map(|v| v.as_object().cloned())
                    .filter(|obj| {
                        obj.get("event_kind")
                            .and_then(|v| v.as_str())
                            .map(|s| s == event_kind)
                            .unwrap_or(false)
                            && obj
                                .get("track_id")
                                .and_then(|v| v.as_str())
                                .map(|s| s == track_id)
                                .unwrap_or(false)
                            && obj
                                .get("to_state")
                                .and_then(|v| v.as_str())
                                .map(|s| allowed_to_states.contains(s))
                                .unwrap_or(false)
                    })
                    .count();
                if count == expected_count {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected exactly {} records with event_kind '{}', track_id '{}', and to_state in {:?}, found {}.\nCaptured stderr lines ({}):\n{}",
                        expected_count,
                        event_kind,
                        track_id,
                        allowed_to_states,
                        count,
                        lines.len(),
                        lines.join("\n")
                    ))
                }
            },
        ),
        // event_kind-scoped absent-key assertion. Finds the (first) record whose
        // `event_kind` equals the given value and asserts it does NOT carry the
        // given key (e.g. `tokens`/`duration_ms` are ABSENT, D-4). This is the
        // event_kind-keyed sibling of the command-keyed no-field step below; the
        // step_measurement emit is keyed on `event_kind`, not `command`.
        check_def(
            "the engine stderr event_kind {string} log record has no {string} field",
            &[("engine_process", "EngineProcess")],
            |ctx, params| {
                let event_kind = params.get_string(0).ok_or("Expected event_kind")?.to_string();
                let field = params.get_string(1).ok_or("Expected field name")?.to_string();
                let engine = ctx
                    .get::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                loop {
                    let lines = engine.stderr_lines();
                    for line in &lines {
                        let value: serde_json::Value = match serde_json::from_str(line) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };
                        let obj = match value.as_object() {
                            Some(o) => o,
                            None => continue,
                        };
                        let is_match = obj
                            .get("event_kind")
                            .and_then(|v| v.as_str())
                            .map(|s| s == event_kind)
                            .unwrap_or(false);
                        if !is_match {
                            continue;
                        }
                        if obj.contains_key(&field) {
                            return Err(format!(
                                "Record for event_kind '{}' unexpectedly carries field '{}': {}",
                                event_kind, field, line
                            ));
                        }
                        return Ok(());
                    }
                    if std::time::Instant::now() >= deadline {
                        return Err(format!(
                            "No log record found for event_kind '{}'",
                            event_kind
                        ));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
            },
        ),
        // Query RPCs carry no actor/events (Req 3: not fabricated). Asserts the
        // record whose `command` equals the given value omits the given field.
        check_def(
            "the engine stderr {string} log record has no {string} field",
            &[("engine_process", "EngineProcess")],
            |ctx, params| {
                let command = params.get_string(0).ok_or("Expected command")?.to_string();
                let field = params.get_string(1).ok_or("Expected field name")?.to_string();
                let engine = ctx
                    .get::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                loop {
                    let lines = engine.stderr_lines();
                    for line in &lines {
                        let value: serde_json::Value = match serde_json::from_str(line) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };
                        let obj = match value.as_object() {
                            Some(o) => o,
                            None => continue,
                        };
                        let is_match = obj
                            .get("command")
                            .and_then(|v| v.as_str())
                            .map(|s| s == command)
                            .unwrap_or(false);
                        if !is_match {
                            continue;
                        }
                        if obj.contains_key(&field) {
                            return Err(format!(
                                "Record for command '{}' unexpectedly carries field '{}': {}",
                                command, field, line
                            ));
                        }
                        return Ok(());
                    }
                    if std::time::Instant::now() >= deadline {
                        return Err(format!(
                            "No log record found for command '{}'",
                            command
                        ));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
            },
        ),
        // Negative hygiene scan (strict whole-buffer substring): the sentinel
        // must appear in NO captured stderr line at all.
        check_def(
            "the engine stderr contains no log record with value {string}",
            &[("engine_process", "EngineProcess")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected value")?.to_string();
                let engine = ctx
                    .get::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let lines = engine.stderr_lines();
                if let Some(offender) = lines.iter().find(|l| l.contains(&needle)) {
                    Err(format!(
                        "Sentinel '{}' leaked into a log line: {}",
                        needle, offender
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        // Two catalog records with DIFFERENT resolved-hearth fields (AC3): parse
        // all JSON lines, keep `command=catalog`, assert >=2 distinct `hearth`s.
        check_def(
            "the engine stderr contains two catalog log records with different hearth fields",
            &[("engine_process", "EngineProcess")],
            |ctx, _params| {
                let engine = ctx
                    .get::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                loop {
                    let lines = engine.stderr_lines();
                    let mut hearths: std::collections::BTreeSet<String> =
                        std::collections::BTreeSet::new();
                    for line in &lines {
                        let value: serde_json::Value = match serde_json::from_str(line) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };
                        let obj = match value.as_object() {
                            Some(o) => o,
                            None => continue,
                        };
                        let is_catalog = obj
                            .get("command")
                            .and_then(|v| v.as_str())
                            .map(|s| s == "catalog")
                            .unwrap_or(false);
                        if !is_catalog {
                            continue;
                        }
                        if let Some(h) = obj.get("hearth").and_then(|v| v.as_str()) {
                            hearths.insert(h.to_string());
                        }
                    }
                    if hearths.len() >= 2 {
                        return Ok(());
                    }
                    if std::time::Instant::now() >= deadline {
                        return Err(format!(
                            "Expected >=2 distinct catalog hearth fields, saw {}: {:?}",
                            hearths.len(),
                            hearths
                        ));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
            },
        ),
        // ===== Checkin RPC steps =====
        step_def(
            "a context file {string} in the hearth with content {string}",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let filename = params.get_string(0).ok_or("Missing filename")?.to_string();
                let content = params.get_string(1).ok_or("Missing content")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let context_dir = hearth.join("context");
                std::fs::create_dir_all(&context_dir).map_err(|e| e.to_string())?;
                std::fs::write(context_dir.join(&filename), &content).map_err(|e| e.to_string())?;
                // Also create tracks.md and projections for the adapter
                let tracks_md = hearth.join("tracks.md");
                if !tracks_md.exists() {
                    std::fs::write(&tracks_md, "# Tracks\n\n## spec\n\n## completed\n").map_err(|e| e.to_string())?;
                }
                let proj_dir = hearth.join("projections");
                if !proj_dir.exists() {
                    std::fs::create_dir_all(&proj_dir).map_err(|e| e.to_string())?;
                    std::fs::write(
                        proj_dir.join("execution.md"),
                        "---\nincremental_count: 0\nbase_snapshot: 2026-04-12T22:00:00Z\nlast_updated: 2026-04-12T22:00:00Z\nafter_event: \"test\"\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Shelved (0)\n",
                    ).map_err(|e| e.to_string())?;
                }
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        step_def(
            "a playbook hook body for the spec doer hook {string} with content {string}",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let filename = params.get_string(0).ok_or("Missing filename")?.to_string();
                let content = params.get_string(1).ok_or("Missing content")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                // Seed an on-disk track playbook that declares the (spec, doer)
                // hook and carries its body under hooks/. The HearthPlaybookRegistry
                // scans this on construction; begin resolves the declaration
                // (hearth-first) and reads the body via read_playbook_hook_body.
                let playbook_id = "20260422T0000_track_lifecycle";
                let playbook_dir = hearth.join("playbooks").join(playbook_id);
                let hooks_dir = playbook_dir.join("hooks");
                std::fs::create_dir_all(&hooks_dir).map_err(|e| e.to_string())?;
                std::fs::write(hooks_dir.join(&filename), &content).map_err(|e| e.to_string())?;

                // A minimal-but-valid track machine.yaml declaring the doer hook
                // on the spec state. Mirrors the seed's shape; the loader validates
                // the hook filename against the hooks/ listing seeded above.
                let machine_yaml = format!(
                    concat!(
                        "kind: track\n",
                        "directory: tracks\n",
                        "registry: tracks.md\n",
                        "parent_kind: proposal\n",
                        "description: \"Track lifecycle (test fixture).\"\n",
                        "required_fields:\n",
                        "  - name: name\n",
                        "    field_type: string\n",
                        "    description: Track name\n",
                        "  - name: parent_id\n",
                        "    field_type: artifact_id\n",
                        "    description: Parent proposal id\n",
                        "  - name: approver\n",
                        "    field_type: actor_name\n",
                        "    description: Human or reviewer authorizing creation\n",
                        "roles:\n",
                        "  - doer\n",
                        "  - spec\n",
                        "  - reviewer\n",
                        "states:\n",
                        "  - name: spec\n",
                        "    role_filters: []\n",
                        "    registry_section: \"\"\n",
                        "    projection_targets: []\n",
                        "    is_review_gate: false\n",
                        "    is_terminal: false\n",
                        "    hooks_by_role:\n",
                        "      doer: {filename}\n",
                        "    measurement_by_role:\n",
                        "      doer:\n",
                        "        intent: \"Translate the approved proposal into a concrete, testable spec.\"\n",
                        "        expected_output: \"A spec.md with brine-checkable acceptance criteria.\"\n",
                        // The track encoding keeps every review state
                        // is_review_gate: false (discriminated by role, not
                        // satisfaction) — mirror the real seed so the
                        // review-gate invariant + selector behave identically.
                        "  - name: spec_review\n",
                        "    role_filters: []\n",
                        "    registry_section: \"spec_review\"\n",
                        "    projection_targets:\n",
                        "      - \"Spec Review\"\n",
                        "    is_review_gate: false\n",
                        "    is_terminal: false\n",
                        "  - name: spec_revision\n",
                        "    role_filters: []\n",
                        "    registry_section: \"\"\n",
                        "    projection_targets: []\n",
                        "    is_review_gate: false\n",
                        "    is_terminal: false\n",
                        "  - name: plan\n",
                        "    role_filters: []\n",
                        "    registry_section: \"plan\"\n",
                        "    projection_targets:\n",
                        "      - \"Planned\"\n",
                        "    is_review_gate: false\n",
                        // `plan` is this minimal fixture's terminal endpoint so
                        // the machine satisfies the registration-time contiguity
                        // gate (no non-terminal dead-end; a terminal is
                        // reachable from every state).
                        "    is_terminal: true\n",
                        // The complete handler is now machine-driven: it selects
                        // the spec→spec_review (doer) and spec_review→plan
                        // (reviewer) edges from the resolved machine, so this
                        // fixture must declare them (it previously had
                        // `transitions: []`, which the literal-driven complete
                        // ignored). spec_revision→spec_review closes the
                        // revision loop so spec_revision is not a dead-end.
                        "transitions:\n",
                        "  - from_state: spec\n",
                        "    to_state: spec_review\n",
                        "    required_role: spec\n",
                        "    required_satisfaction: ~\n",
                        "    requires_approver: false\n",
                        "  - from_state: spec_review\n",
                        "    to_state: spec_revision\n",
                        "    required_role: spec\n",
                        "    required_satisfaction: ~\n",
                        "    requires_approver: false\n",
                        "  - from_state: spec_revision\n",
                        "    to_state: spec_review\n",
                        "    required_role: spec\n",
                        "    required_satisfaction: ~\n",
                        "    requires_approver: false\n",
                        "  - from_state: spec_review\n",
                        "    to_state: plan\n",
                        "    required_role: reviewer\n",
                        "    required_satisfaction: ~\n",
                        "    requires_approver: false\n",
                    ),
                    filename = filename
                );
                std::fs::write(playbook_dir.join("machine.yaml"), machine_yaml)
                    .map_err(|e| e.to_string())?;

                // Ensure tracks.md + projections exist so the engine hearth
                // predicate is satisfied (mirrors the context-file fixture step).
                let tracks_md = hearth.join("tracks.md");
                if !tracks_md.exists() {
                    std::fs::write(&tracks_md, "# Tracks\n\n## spec\n\n## completed\n")
                        .map_err(|e| e.to_string())?;
                }
                let proj_dir = hearth.join("projections");
                if !proj_dir.exists() {
                    std::fs::create_dir_all(&proj_dir).map_err(|e| e.to_string())?;
                    std::fs::write(
                        proj_dir.join("execution.md"),
                        "---\nincremental_count: 0\nbase_snapshot: 2026-04-12T22:00:00Z\nlast_updated: 2026-04-12T22:00:00Z\nafter_event: \"test\"\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Shelved (0)\n",
                    ).map_err(|e| e.to_string())?;
                }
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        step_def(
            "a playbook hook body for the track hook {string} with content {string}",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let filename = params.get_string(0).ok_or("Missing filename")?.to_string();
                let content = params.get_string(1).ok_or("Missing content")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let hooks_dir = hearth
                    .join("playbooks")
                    .join("20260422T0000_track_lifecycle")
                    .join("hooks");
                std::fs::create_dir_all(&hooks_dir).map_err(|e| e.to_string())?;
                std::fs::write(hooks_dir.join(&filename), &content).map_err(|e| e.to_string())?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        step_def(
            "a playbook hook body for the spec_review reviewer hook {string} with content {string}",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let filename = params.get_string(0).ok_or("Missing filename")?.to_string();
                let content = params.get_string(1).ok_or("Missing content")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                // Seed an on-disk track playbook that declares the (spec_review, reviewer)
                // hook and carries its body under hooks/. The HearthPlaybookRegistry
                // scans this on construction; begin resolves the declaration
                // (hearth-first) and reads the body via read_playbook_hook_body.
                let playbook_id = "20260422T0000_track_lifecycle";
                let playbook_dir = hearth.join("playbooks").join(playbook_id);
                let hooks_dir = playbook_dir.join("hooks");
                std::fs::create_dir_all(&hooks_dir).map_err(|e| e.to_string())?;
                std::fs::write(hooks_dir.join(&filename), &content).map_err(|e| e.to_string())?;

                // A minimal-but-valid track machine.yaml declaring the reviewer hook
                // on the spec_review state. The loader validates the hook filename
                // against the hooks/ listing seeded above.
                let machine_yaml = format!(
                    concat!(
                        "kind: track\n",
                        "directory: tracks\n",
                        "registry: tracks.md\n",
                        "parent_kind: proposal\n",
                        "description: \"Track lifecycle (test fixture).\"\n",
                        "required_fields:\n",
                        "  - name: name\n",
                        "    field_type: string\n",
                        "    description: Track name\n",
                        "  - name: parent_id\n",
                        "    field_type: artifact_id\n",
                        "    description: Parent proposal id\n",
                        "  - name: approver\n",
                        "    field_type: actor_name\n",
                        "    description: Human or reviewer authorizing creation\n",
                        "roles:\n",
                        "  - doer\n",
                        "  - reviewer\n",
                        "  - spec\n",
                        "states:\n",
                        // Track encoding: review states stay is_review_gate:false
                        // (role-discriminated, not satisfaction). Mirror the real
                        // seed so the now-machine-driven complete selects the
                        // spec→spec_review (doer) and spec_review→plan (reviewer,
                        // satisfied-compat) edges. The spec state + edge are
                        // declared because some e2e flows do a doer-complete from
                        // spec before the reviewer-complete this fixture targets.
                        "  - name: spec\n",
                        "    role_filters: []\n",
                        "    registry_section: \"\"\n",
                        "    projection_targets: []\n",
                        "    is_review_gate: false\n",
                        "    is_terminal: false\n",
                        "  - name: spec_review\n",
                        "    role_filters: []\n",
                        "    registry_section: \"spec_review\"\n",
                        "    projection_targets:\n",
                        "      - \"Spec Review\"\n",
                        "    is_review_gate: false\n",
                        "    is_terminal: false\n",
                        "    hooks_by_role:\n",
                        "      reviewer: {filename}\n",
                        "    measurement_by_role:\n",
                        "      reviewer:\n",
                        "        intent: \"Judge whether the spec's acceptance criteria are complete and faithful.\"\n",
                        "        expected_output: \"A spec.review.md verdict citing each acceptance criterion.\"\n",
                        "  - name: spec_revision\n",
                        "    role_filters: []\n",
                        "    registry_section: \"\"\n",
                        "    projection_targets: []\n",
                        "    is_review_gate: false\n",
                        "    is_terminal: false\n",
                        "  - name: plan\n",
                        "    role_filters: []\n",
                        "    registry_section: \"plan\"\n",
                        "    projection_targets:\n",
                        "      - \"Planned\"\n",
                        "    is_review_gate: false\n",
                        // Terminal endpoint so the minimal fixture flows (see
                        // the doer-hook fixture above for the same rationale).
                        "    is_terminal: true\n",
                        "transitions:\n",
                        "  - from_state: spec\n",
                        "    to_state: spec_review\n",
                        "    required_role: spec\n",
                        "    required_satisfaction: ~\n",
                        "    requires_approver: false\n",
                        "  - from_state: spec_review\n",
                        "    to_state: spec_revision\n",
                        "    required_role: spec\n",
                        "    required_satisfaction: ~\n",
                        "    requires_approver: false\n",
                        "  - from_state: spec_revision\n",
                        "    to_state: spec_review\n",
                        "    required_role: spec\n",
                        "    required_satisfaction: ~\n",
                        "    requires_approver: false\n",
                        "  - from_state: spec_review\n",
                        "    to_state: plan\n",
                        "    required_role: reviewer\n",
                        "    required_satisfaction: ~\n",
                        "    requires_approver: false\n",
                    ),
                    filename = filename
                );
                std::fs::write(playbook_dir.join("machine.yaml"), machine_yaml)
                    .map_err(|e| e.to_string())?;

                // Ensure tracks.md + projections exist so the engine hearth
                // predicate is satisfied (mirrors the context-file fixture step).
                let tracks_md = hearth.join("tracks.md");
                if !tracks_md.exists() {
                    std::fs::write(&tracks_md, "# Tracks\n\n## spec\n\n## completed\n")
                        .map_err(|e| e.to_string())?;
                }
                let proj_dir = hearth.join("projections");
                if !proj_dir.exists() {
                    std::fs::create_dir_all(&proj_dir).map_err(|e| e.to_string())?;
                    std::fs::write(
                        proj_dir.join("execution.md"),
                        "---\nincremental_count: 0\nbase_snapshot: 2026-04-12T22:00:00Z\nlast_updated: 2026-04-12T22:00:00Z\nafter_event: \"test\"\n---\n\n# Anvil — State of Execution\n\n## Spec Review (0)\n\n## Shelved (0)\n",
                    ).map_err(|e| e.to_string())?;
                }
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        async_step_def(
            "the checkin RPC is called with role {string}",
            &[("engine_process", "EngineProcess")],
            &[("checkin_result", "CheckinRpcResult"), ("engine_process", "EngineProcess")],
            |mut ctx, params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let port = engine.port;
                let role = params.get_string(0).ok_or("Missing role")?.to_string();

                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::CheckinRequest {
                            hearth_path: String::new(),
                            role,
                            actor_type: "agent".to_string(),
                            actor_model: "test-model".to_string(),
                            actor_provider: "test".to_string(),
                            actor_name: String::new(),
                        });
                        match client.checkin(request).await {
                            Ok(response) => {
                                let resp = response.into_inner();
                                CheckinRpcResult::Success(CheckinRpcResponse {
                                    actor_name: resp.actor_name,
                                    filtered_artifacts: resp.filtered_artifacts.into_iter().map(|a| CatalogArtifact {
                                        id: a.id, artifact_type: a.artifact_type, state: a.state, summary: a.summary, execution_route: a.execution_route,
                                    }).collect(),
                                    available_types: resp.available_types.into_iter().map(|t| CatalogAvailableType {
                                        name: t.name,
                                        description: t.description,
                                        requires_parent: t.requires_parent,
                                        execution_route: t.execution_route,
                                    }).collect(),
                                    next_step: resp.next_step,
                                    context: resp.context,
                                })
                            }
                            Err(status) => CheckinRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => CheckinRpcResult::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };

                let mut out = Context::new();
                out.set("checkin_result", result);
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        async_step_def(
            "the checkin RPC is called for hearth {string} with role {string}",
            &[("engine_process", "EngineProcess")],
            &[
                ("checkin_result", "CheckinRpcResult"),
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_x_path", "PathBuf"),
                ("hearth_y_path", "PathBuf"),
            ],
            |mut ctx, params| async move {
                let selector = params.get_string(0).ok_or("Expected hearth selector")?.to_string();
                let role = params.get_string(1).ok_or("Missing role")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").cloned();
                let hearth_x = ctx.get::<PathBuf>("hearth_x_path").cloned();
                let hearth_y = ctx.get::<PathBuf>("hearth_y_path").cloned();
                let req_hearth = match selector.as_str() {
                    "X" => hearth_x.as_ref().ok_or("No hearth_x_path")?.to_string_lossy().into_owned(),
                    "Y" => hearth_y.as_ref().ok_or("No hearth_y_path")?.to_string_lossy().into_owned(),
                    other => other.to_string(),
                };
                let result = call_checkin_with_hearth(&engine, &role, req_hearth).await;

                let mut out = Context::new();
                out.set("checkin_result", result);
                out.set("engine_process", engine);
                if let Some(p) = hearth_path { out.set("hearth_path", p); }
                if let Some(p) = hearth_x { out.set("hearth_x_path", p); }
                if let Some(p) = hearth_y { out.set("hearth_y_path", p); }
                Ok(out)
            },
        ),
        async_step_def(
            "the checkin RPC is called with role {string} and actor_name {string}",
            &[("engine_process", "EngineProcess")],
            &[("checkin_result", "CheckinRpcResult"), ("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            |mut ctx, params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                // Carry hearth_path forward when present so a checkin in the
                // middle of a multi-call sequence (begin -> checkin -> complete)
                // does not strip it from the context the next RPC step needs.
                let hearth_path = ctx.get::<PathBuf>("hearth_path").cloned();
                let port = engine.port;
                let role = params.get_string(0).ok_or("Missing role")?.to_string();
                let actor_name = params.get_string(1).ok_or("Missing actor_name")?.to_string();

                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::CheckinRequest {
                            hearth_path: String::new(),
                            role,
                            actor_type: "agent".to_string(),
                            actor_model: "test-model".to_string(),
                            actor_provider: "test".to_string(),
                            actor_name,
                        });
                        match client.checkin(request).await {
                            Ok(response) => {
                                let resp = response.into_inner();
                                CheckinRpcResult::Success(CheckinRpcResponse {
                                    actor_name: resp.actor_name,
                                    filtered_artifacts: resp.filtered_artifacts.into_iter().map(|a| CatalogArtifact {
                                        id: a.id, artifact_type: a.artifact_type, state: a.state, summary: a.summary, execution_route: a.execution_route,
                                    }).collect(),
                                    available_types: resp.available_types.into_iter().map(|t| CatalogAvailableType {
                                        name: t.name,
                                        description: t.description,
                                        requires_parent: t.requires_parent,
                                        execution_route: t.execution_route,
                                    }).collect(),
                                    next_step: resp.next_step,
                                    context: resp.context,
                                })
                            }
                            Err(status) => CheckinRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => CheckinRpcResult::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };

                let mut out = Context::new();
                out.set("checkin_result", result);
                out.set("engine_process", engine);
                if let Some(hp) = hearth_path {
                    out.set("hearth_path", hp);
                }
                Ok(out)
            },
        ),
        // --- Checkin RPC response checks ---
        check_def(
            "the checkin RPC response actor_name is {string}",
            &[("checkin_result", "CheckinRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected name")?;
                let result = ctx.get::<CheckinRpcResult>("checkin_result").ok_or("No checkin_result")?;
                match result {
                    CheckinRpcResult::Success(r) if r.actor_name == expected => Ok(()),
                    CheckinRpcResult::Success(r) => Err(format!("Expected actor_name '{}', got '{}'", expected, r.actor_name)),
                    CheckinRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the checkin RPC response context contains {string}",
            &[("checkin_result", "CheckinRpcResult")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let result = ctx.get::<CheckinRpcResult>("checkin_result").ok_or("No checkin_result")?;
                match result {
                    CheckinRpcResult::Success(r) if r.context.contains(needle) => Ok(()),
                    CheckinRpcResult::Success(r) => Err(format!("Expected context to contain '{}', got '{}'", needle, r.context)),
                    CheckinRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the checkin RPC response context is empty",
            &[("checkin_result", "CheckinRpcResult")],
            |ctx, _params| {
                let result = ctx.get::<CheckinRpcResult>("checkin_result").ok_or("No checkin_result")?;
                match result {
                    CheckinRpcResult::Success(r) if r.context.is_empty() => Ok(()),
                    CheckinRpcResult::Success(r) => Err(format!("Expected empty context, got '{}'", r.context)),
                    CheckinRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the checkin RPC response has a non-empty actor name",
            &[("checkin_result", "CheckinRpcResult")],
            |ctx, _params| {
                let result = ctx.get::<CheckinRpcResult>("checkin_result").ok_or("No checkin_result")?;
                match result {
                    CheckinRpcResult::Success(r) if !r.actor_name.is_empty() => Ok(()),
                    CheckinRpcResult::Success(r) => Err(format!("Actor name is empty: '{}'", r.actor_name)),
                    CheckinRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the checkin RPC filtered artifacts include {string}",
            &[("checkin_result", "CheckinRpcResult")],
            |ctx, params| {
                let expected_id = params.get_string(0).ok_or("Expected id")?.to_string();
                let result = ctx.get::<CheckinRpcResult>("checkin_result").ok_or("No checkin_result")?;
                match result {
                    CheckinRpcResult::Success(r) => {
                        if r.filtered_artifacts.iter().any(|a| a.id == expected_id) {
                            Ok(())
                        } else {
                            Err(format!("Artifact '{}' not in filtered list: {:?}",
                                expected_id, r.filtered_artifacts.iter().map(|a| &a.id).collect::<Vec<_>>()))
                        }
                    }
                    CheckinRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the checkin RPC filtered artifacts do not include {string}",
            &[("checkin_result", "CheckinRpcResult")],
            |ctx, params| {
                let excluded_id = params.get_string(0).ok_or("Expected id")?.to_string();
                let result = ctx.get::<CheckinRpcResult>("checkin_result").ok_or("No checkin_result")?;
                match result {
                    CheckinRpcResult::Success(r) => {
                        if r.filtered_artifacts.iter().any(|a| a.id == excluded_id) {
                            Err(format!("Artifact '{}' should not be in filtered list", excluded_id))
                        } else {
                            Ok(())
                        }
                    }
                    CheckinRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the checkin RPC available types include {string}",
            &[("checkin_result", "CheckinRpcResult")],
            |ctx, params| {
                let type_name = params.get_string(0).ok_or("Expected type")?.to_string();
                let result = ctx.get::<CheckinRpcResult>("checkin_result").ok_or("No checkin_result")?;
                match result {
                    CheckinRpcResult::Success(r) => {
                        if r.available_types.iter().any(|t| t.name == type_name) {
                            Ok(())
                        } else {
                            Err(format!("Type '{}' not in available types", type_name))
                        }
                    }
                    CheckinRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the checkin RPC available type {string} has execution_route {string}",
            &[("checkin_result", "CheckinRpcResult")],
            |ctx, params| {
                let type_name = params.get_string(0).ok_or("Expected type")?.to_string();
                let expected = params.get_string(1).ok_or("Expected workflow")?.to_string();
                let result = ctx.get::<CheckinRpcResult>("checkin_result").ok_or("No checkin_result")?;
                match result {
                    CheckinRpcResult::Success(r) => {
                        match r.available_types.iter().find(|t| t.name == type_name) {
                            Some(t) if t.execution_route == expected => Ok(()),
                            Some(t) => Err(format!(
                                "Type '{}' has execution_route '{}', expected '{}'",
                                type_name, t.execution_route, expected
                            )),
                            None => Err(format!("Type '{}' not in available types", type_name)),
                        }
                    }
                    CheckinRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the checkin RPC returns gRPC status {string}",
            &[("checkin_result", "CheckinRpcResult")],
            |ctx, params| {
                let expected_code = params.get_string(0).ok_or("Missing status code")?.to_string();
                let result = ctx.get::<CheckinRpcResult>("checkin_result").ok_or("No checkin_result")?;
                match result {
                    CheckinRpcResult::Error { code, .. } if *code == expected_code => Ok(()),
                    CheckinRpcResult::Error { code, message } => {
                        if code.contains(&expected_code) {
                            Ok(())
                        } else {
                            Err(format!("Expected gRPC {}, got {} ({})", expected_code, code, message))
                        }
                    },
                    CheckinRpcResult::Success(_) => Err(format!("Expected gRPC {} error, got success", expected_code)),
                }
            },
        ),
        // ===== Describe RPC steps =====
        step_def(
            "the engine is started with a minimal hearth",
            &[],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("engine_port", "u16"),
            ],
            |_ctx, _params| {
                let (handle, tmp) = retained_temp_dir("anvil-test-hearth-minimal-")?;
                std::fs::create_dir_all(&tmp).map_err(|e| format!("Failed to create dir: {}", e))?;

                let listener = std::net::TcpListener::bind("127.0.0.1:0")
                    .map_err(|e| format!("Failed to find free port: {}", e))?;
                let port = listener.local_addr()
                    .map_err(|e| format!("Failed to get port: {}", e))?.port();
                drop(listener);

                let binary = crate::harness::binary_path("anvil-engine");

                let child = anvil_engine_command(&binary)
                    .arg("--hearth").arg(tmp.to_str().unwrap())
                    .arg("--port").arg(port.to_string())
                    .stdout(Stdio::null()).stderr(Stdio::null())
                    .spawn()
                    .unwrap_or_else(|e| panic!("Failed to start engine: {}", e));

                wait_for_engine_ready(port);

                let mut process = EngineProcess::without_capture(child, port);
                process.retain_temp_dir(&handle);
                let mut out = Context::new();
                out.set("engine_process", process);
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                out.set("engine_port", port);
                Ok(out)
            },
        ),
        async_step_def(
            "the describe RPC is called with identifier {string}",
            &[("engine_process", "EngineProcess")],
            &[
                ("describe_result", "DescribeRpcResult"),
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |mut ctx, params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").cloned();
                let hearth_path_handle = ctx.get::<RetainedTempDir>("hearth_path_handle").cloned();
                let port = engine.port;
                let identifier = params.get_string(0).ok_or("Missing identifier")?.to_string();

                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::DescribeRequest {
                            hearth_path: String::new(),
                            identifier,
                        });
                        match client.describe(request).await {
                            Ok(response) => DescribeRpcResult::Success(response.into_inner()),
                            Err(status) => DescribeRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => DescribeRpcResult::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };

                let mut out = Context::new();
                out.set("describe_result", result);
                out.set("engine_process", engine);
                if let Some(path) = hearth_path {
                    out.set("hearth_path", path);
                }
                if let Some(handle) = hearth_path_handle {
                    out.set("hearth_path_handle", handle);
                }
                Ok(out)
            },
        ),
        check_def(
            "the describe RPC returns type info with name {string}",
            &[("describe_result", "DescribeRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected name")?.to_string();
                let result = ctx.get::<DescribeRpcResult>("describe_result").ok_or("No describe_result")?;
                match result {
                    DescribeRpcResult::Success(resp) => {
                        match &resp.info {
                            Some(anvil_engine::proto::describe_response::Info::TypeInfo(t)) if t.name == expected => Ok(()),
                            Some(anvil_engine::proto::describe_response::Info::TypeInfo(t)) => Err(format!("Expected name '{}', got '{}'", expected, t.name)),
                            _ => Err("Expected TypeInfo response".to_string()),
                        }
                    }
                    DescribeRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the describe RPC type info has parent type {string}",
            &[("describe_result", "DescribeRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected parent type")?.to_string();
                let result = ctx.get::<DescribeRpcResult>("describe_result").ok_or("No describe_result")?;
                match result {
                    DescribeRpcResult::Success(resp) => {
                        match &resp.info {
                            Some(anvil_engine::proto::describe_response::Info::TypeInfo(t)) if t.parent_type == expected => Ok(()),
                            Some(anvil_engine::proto::describe_response::Info::TypeInfo(t)) => Err(format!("Expected parent '{}', got '{}'", expected, t.parent_type)),
                            _ => Err("Expected TypeInfo response".to_string()),
                        }
                    }
                    DescribeRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the describe RPC returns gRPC status {string}",
            &[("describe_result", "DescribeRpcResult")],
            |ctx, params| {
                let expected_code = params.get_string(0).ok_or("Missing status code")?.to_string();
                let result = ctx.get::<DescribeRpcResult>("describe_result").ok_or("No describe_result")?;
                match result {
                    DescribeRpcResult::Error { code, .. } if *code == expected_code => Ok(()),
                    DescribeRpcResult::Error { code, message } => {
                        if code.contains(&expected_code) { Ok(()) }
                        else { Err(format!("Expected gRPC {}, got {} ({})", expected_code, code, message)) }
                    },
                    DescribeRpcResult::Success(_) => Err(format!("Expected gRPC {} error, got success", expected_code)),
                }
            },
        ),
        // ===== next_step capture: per-label typed context keys =====
        //
        // Role and subject are closed enums (creator/reviewer/resumer;
        // type-level/instance-level), so we register a literal step_def
        // per label that captures its next_step into a distinct typed
        // context key. Assertion steps require the specific typed keys,
        // so brine's pipeline statically verifies upstream production
        // before downstream read — no runtime-keyed map smuggling.
        //
        // If a new role is added to CheckinRole, add a literal step_def
        // here for it. The combinatorial ceiling (N) is the domain enum
        // size, not the feature-file vocabulary size.
        async_step_def(
            "the checkin RPC for role \"creator\" is called",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                ("checkin_result", "CheckinRpcResult"),
                ("checkin_creator_next_step", "String"),
                ("checkin_reviewer_next_step", "String"),
                ("checkin_resumer_next_step", "String"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let passthrough_reviewer = ctx.get::<String>("checkin_reviewer_next_step").cloned();
                let passthrough_resumer = ctx.get::<String>("checkin_resumer_next_step").cloned();
                let (result, next_step) = call_checkin(&engine, "creator").await?;
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("checkin_result", result);
                out.set("checkin_creator_next_step", next_step);
                if let Some(v) = passthrough_reviewer { out.set("checkin_reviewer_next_step", v); }
                if let Some(v) = passthrough_resumer { out.set("checkin_resumer_next_step", v); }
                Ok(out)
            },
        ),
        async_step_def(
            "the checkin RPC for role \"reviewer\" is called",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                ("checkin_result", "CheckinRpcResult"),
                ("checkin_creator_next_step", "String"),
                ("checkin_reviewer_next_step", "String"),
                ("checkin_resumer_next_step", "String"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let passthrough_creator = ctx.get::<String>("checkin_creator_next_step").cloned();
                let passthrough_resumer = ctx.get::<String>("checkin_resumer_next_step").cloned();
                let (result, next_step) = call_checkin(&engine, "reviewer").await?;
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("checkin_result", result);
                out.set("checkin_reviewer_next_step", next_step);
                if let Some(v) = passthrough_creator { out.set("checkin_creator_next_step", v); }
                if let Some(v) = passthrough_resumer { out.set("checkin_resumer_next_step", v); }
                Ok(out)
            },
        ),
        async_step_def(
            "the checkin RPC for role \"resumer\" is called",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                ("checkin_result", "CheckinRpcResult"),
                ("checkin_creator_next_step", "String"),
                ("checkin_reviewer_next_step", "String"),
                ("checkin_resumer_next_step", "String"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let passthrough_creator = ctx.get::<String>("checkin_creator_next_step").cloned();
                let passthrough_reviewer = ctx.get::<String>("checkin_reviewer_next_step").cloned();
                let (result, next_step) = call_checkin(&engine, "resumer").await?;
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("checkin_result", result);
                out.set("checkin_resumer_next_step", next_step);
                if let Some(v) = passthrough_creator { out.set("checkin_creator_next_step", v); }
                if let Some(v) = passthrough_reviewer { out.set("checkin_reviewer_next_step", v); }
                Ok(out)
            },
        ),
        async_step_def(
            "the describe RPC for type {string} is called",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                ("describe_result", "DescribeRpcResult"),
                ("describe_type_level_next_step", "String"),
                ("describe_instance_level_next_step", "String"),
            ],
            |mut ctx, params| async move {
                let type_name = params.get_string(0).ok_or("Expected type name")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let passthrough_instance = ctx.get::<String>("describe_instance_level_next_step").cloned();
                let (result, next_step) = call_describe(&engine, &type_name).await?;
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("describe_result", result);
                out.set("describe_type_level_next_step", next_step);
                if let Some(v) = passthrough_instance { out.set("describe_instance_level_next_step", v); }
                Ok(out)
            },
        ),
        async_step_def(
            "the describe RPC for instance id {string} is called",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                ("describe_result", "DescribeRpcResult"),
                ("describe_type_level_next_step", "String"),
                ("describe_instance_level_next_step", "String"),
            ],
            |mut ctx, params| async move {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let passthrough_type = ctx.get::<String>("describe_type_level_next_step").cloned();
                let (result, next_step) = call_describe(&engine, &id).await?;
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("describe_result", result);
                out.set("describe_instance_level_next_step", next_step);
                if let Some(v) = passthrough_type { out.set("describe_type_level_next_step", v); }
                Ok(out)
            },
        ),
        // --- Assertion steps over typed per-label next_step keys ---
        check_def(
            "the checkin next_step for creator differs from reviewer",
            &[("checkin_creator_next_step", "String"), ("checkin_reviewer_next_step", "String")],
            |ctx, _params| {
                let a = ctx.get::<String>("checkin_creator_next_step").ok_or("No creator next_step")?;
                let b = ctx.get::<String>("checkin_reviewer_next_step").ok_or("No reviewer next_step")?;
                if a.is_empty() { return Err("creator next_step is empty".to_string()); }
                if b.is_empty() { return Err("reviewer next_step is empty".to_string()); }
                if a == b {
                    Err(format!("creator next_step is identical to reviewer: both = '{}'", a))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the checkin next_step for creator references the execution_route field",
            &[("checkin_creator_next_step", "String")],
            |ctx, _params| {
                let value = ctx.get::<String>("checkin_creator_next_step").ok_or("No creator next_step")?;
                if value.contains("execution_route") { Ok(()) }
                else { Err(format!("creator next_step does not reference 'execution_route'. Got: '{}'", value)) }
            },
        ),
        check_def(
            "the checkin next_step for reviewer references the execution_route field",
            &[("checkin_reviewer_next_step", "String")],
            |ctx, _params| {
                let value = ctx.get::<String>("checkin_reviewer_next_step").ok_or("No reviewer next_step")?;
                if value.contains("execution_route") { Ok(()) }
                else { Err(format!("reviewer next_step does not reference 'execution_route'. Got: '{}'", value)) }
            },
        ),
        check_def(
            "the describe next_step differs between type-level and instance-level",
            &[("describe_type_level_next_step", "String"), ("describe_instance_level_next_step", "String")],
            |ctx, _params| {
                let a = ctx.get::<String>("describe_type_level_next_step").ok_or("No type-level next_step")?;
                let b = ctx.get::<String>("describe_instance_level_next_step").ok_or("No instance-level next_step")?;
                if a.is_empty() { return Err("type-level next_step is empty".to_string()); }
                if b.is_empty() { return Err("instance-level next_step is empty".to_string()); }
                if a == b {
                    Err(format!("type-level next_step is identical to instance-level: both = '{}'", a))
                } else {
                    Ok(())
                }
            },
        ),
        // ===== Begin RPC steps =====
        async_step_def(
            "the begin RPC is called with identifier {string} and session_role {string}",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                ("begin_result", "BeginRpcResult"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |mut ctx, params| async move {
                let identifier = params.get_string(0).ok_or("Expected identifier")?.to_string();
                let session_role = params.get_string(1).ok_or("Expected session_role")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let port = engine.port;

                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::BeginRequest {
                            hearth_path: String::new(),
                            artifact_type: String::new(),
                            parent_id: String::new(),
                            track_name: String::new(),
                            playbook_name: String::new(),
                            target_owner: String::new(),
                            approver: String::new(),
                            actor_name: "Rpc-Test-000000".to_string(),
                            actor_type: "agent".to_string(),
                            actor_model: "test".to_string(),
                            actor_provider: "test".to_string(),
                            actor_context_window: 0,
                            actor_sdk_version: String::new(),
                            actor_entrypoint: String::new(),
                            identifier,
                            session_role,
                            ctx_org: "Foundation".to_string(),
                            ctx_space: String::new(),
                            ctx_role: "read".to_string(),
                            ctx_clearance: "internal".to_string(),
                        ..Default::default()
                        });
                        match client.begin(request).await {
                            Ok(response) => BeginRpcResult::Success(response.into_inner()),
                            Err(status) => BeginRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => BeginRpcResult::Error { code: "UNAVAILABLE".to_string(), message: format!("Connection failed: {}", e) },
                };

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("begin_result", result);
                if let Some(hearth_path) = ctx.take::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hearth_path);
                }
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        async_step_def(
            "the begin RPC is called with identifier {string} and session_role {string} and actor_name {string}",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("begin_result", "BeginRpcResult"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |mut ctx, params| async move {
                let identifier = params.get_string(0).ok_or("Expected identifier")?.to_string();
                let session_role = params.get_string(1).ok_or("Expected session_role")?.to_string();
                let actor_name = params.get_string(2).ok_or("Expected actor_name")?.to_string();
                let hearth = ctx.take::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let port = engine.port;

                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::BeginRequest {
                            hearth_path: String::new(),
                            artifact_type: String::new(),
                            parent_id: String::new(),
                            track_name: String::new(),
                            playbook_name: String::new(),
                            target_owner: String::new(),
                            approver: String::new(),
                            actor_name,
                            actor_type: "agent".to_string(),
                            actor_model: "test".to_string(),
                            actor_provider: "test".to_string(),
                            actor_context_window: 0,
                            actor_sdk_version: String::new(),
                            actor_entrypoint: String::new(),
                            identifier,
                            session_role,
                            ctx_org: "Foundation".to_string(),
                            ctx_space: String::new(),
                            ctx_role: "read".to_string(),
                            ctx_clearance: "internal".to_string(),
                        ..Default::default()
                        });
                        match client.begin(request).await {
                            Ok(response) => BeginRpcResult::Success(response.into_inner()),
                            Err(status) => BeginRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => BeginRpcResult::Error { code: "UNAVAILABLE".to_string(), message: format!("Connection failed: {}", e) },
                };

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("begin_result", result);
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        // Adopt an out-of-engine artifact: begin(identifier, adopt: true).
        async_step_def(
            "the begin RPC is called with identifier {string}, session_role {string}, and adopt {string}",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("begin_result", "BeginRpcResult"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |mut ctx, params| async move {
                let identifier = params.get_string(0).ok_or("Expected identifier")?.to_string();
                let session_role = params.get_string(1).ok_or("Expected session_role")?.to_string();
                let adopt = params.get_string(2).map(|s| s == "true").unwrap_or(false);
                let hearth = ctx.take::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let port = engine.port;

                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::BeginRequest {
                            actor_name: "Adopter-000000".to_string(),
                            actor_type: "agent".to_string(),
                            actor_model: "test".to_string(),
                            actor_provider: "test".to_string(),
                            identifier,
                            session_role,
                            adopt,
                            ctx_org: "Foundation".to_string(),
                            ctx_space: String::new(),
                            ctx_role: "read".to_string(),
                            ctx_clearance: "internal".to_string(),
                            ..Default::default()
                        });
                        match client.begin(request).await {
                            Ok(response) => BeginRpcResult::Success(response.into_inner()),
                            Err(status) => BeginRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => BeginRpcResult::Error { code: "UNAVAILABLE".to_string(), message: format!("Connection failed: {}", e) },
                };

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("begin_result", result);
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        // resume-aware routing: a begin RPC carrying a conversation_id so the
        // durable open-begin marker records it (Phase 1 read-back at the seam).
        async_step_def(
            "the begin RPC is called with identifier {string} and session_role {string} and actor_name {string} and conversation_id {string}",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("begin_result", "BeginRpcResult"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |mut ctx, params| async move {
                let identifier = params.get_string(0).ok_or("Expected identifier")?.to_string();
                let session_role = params.get_string(1).ok_or("Expected session_role")?.to_string();
                let actor_name = params.get_string(2).ok_or("Expected actor_name")?.to_string();
                let conversation_id = params.get_string(3).ok_or("Expected conversation_id")?.to_string();
                let hearth = ctx.take::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let port = engine.port;

                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::BeginRequest {
                            hearth_path: String::new(),
                            actor_name,
                            actor_type: "agent".to_string(),
                            actor_model: "test".to_string(),
                            actor_provider: "test".to_string(),
                            identifier,
                            session_role,
                            conversation_id,
                            ctx_org: "Foundation".to_string(),
                            ctx_space: String::new(),
                            ctx_role: "read".to_string(),
                            ctx_clearance: "internal".to_string(),
                            ..Default::default()
                        });
                        match client.begin(request).await {
                            Ok(response) => BeginRpcResult::Success(response.into_inner()),
                            Err(status) => BeginRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => BeginRpcResult::Error { code: "UNAVAILABLE".to_string(), message: format!("Connection failed: {}", e) },
                };

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("begin_result", result);
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        // Slice C: overwrite a track file mid-scenario while keeping the running
        // engine alive in context (supports the R4.5 fresh-read test, which
        // begins, re-writes carry-forward.md, then begins again against the same
        // engine). `\n` in content is unescaped to real newlines.
        step_def(
            "the carry-forward file for {string} is overwritten with {string}",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |mut ctx, params| {
                let track_id = params.get_string(0).ok_or("Expected track id")?.to_string();
                let content = params.get_string(1).ok_or("Expected content")?.to_string();
                let hearth = ctx.take::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let track_dir = hearth.join("tracks").join(&track_id);
                std::fs::create_dir_all(&track_dir)
                    .map_err(|e| format!("Failed to create track dir: {}", e))?;
                std::fs::write(track_dir.join("carry-forward.md"), content.replace("\\n", "\n"))
                    .map_err(|e| format!("Failed to write carry-forward.md: {}", e))?;
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        async_step_def(
            "the begin RPC is called with identifier {string}, session_role {string}, and empty {string}",
            &[("engine_process", "EngineProcess")],
            &[("engine_process", "EngineProcess"), ("begin_result", "BeginRpcResult")],
            |mut ctx, params| async move {
                let identifier = params.get_string(0).ok_or("Expected identifier")?.to_string();
                let session_role = params.get_string(1).ok_or("Expected session_role")?.to_string();
                let empty_field = params.get_string(2).ok_or("Expected empty field name")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let port = engine.port;

                let mut actor_name = "Rpc-Test-000000".to_string();
                let mut actor_type = "agent".to_string();
                let mut actor_model = "test".to_string();
                let mut actor_provider = "test".to_string();
                match empty_field.as_str() {
                    "actor_name" => actor_name.clear(),
                    "actor_type" => actor_type.clear(),
                    "actor_model" => actor_model.clear(),
                    "actor_provider" => actor_provider.clear(),
                    other => return Err(format!("Unknown identity field to clear: '{}'", other)),
                }

                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::BeginRequest {
                            hearth_path: String::new(),
                            artifact_type: String::new(),
                            parent_id: String::new(),
                            track_name: String::new(),
                            playbook_name: String::new(),
                            target_owner: String::new(),
                            approver: String::new(),
                            actor_name,
                            actor_type,
                            actor_model,
                            actor_provider,
                            actor_context_window: 0,
                            actor_sdk_version: String::new(),
                            actor_entrypoint: String::new(),
                            identifier,
                            session_role,
                            ctx_org: "Foundation".to_string(),
                            ctx_space: String::new(),
                            ctx_role: "read".to_string(),
                            ctx_clearance: "internal".to_string(),
                        ..Default::default()
                        });
                        match client.begin(request).await {
                            Ok(response) => BeginRpcResult::Success(response.into_inner()),
                            Err(status) => BeginRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => BeginRpcResult::Error { code: "UNAVAILABLE".to_string(), message: format!("Connection failed: {}", e) },
                };

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("begin_result", result);
                Ok(out)
            },
        ),
        check_def(
            "the begin RPC response state is {string}",
            &[("begin_result", "BeginRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?.to_string();
                let result = ctx.get::<BeginRpcResult>("begin_result").ok_or("No begin_result")?;
                match result {
                    BeginRpcResult::Success(r) if r.state == expected => Ok(()),
                    BeginRpcResult::Success(r) => Err(format!("Expected state '{}', got '{}'", expected, r.state)),
                    BeginRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the begin RPC returns gRPC status {string}",
            &[("begin_result", "BeginRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected status code")?.to_string();
                let result = ctx.get::<BeginRpcResult>("begin_result").ok_or("No begin_result")?;
                match result {
                    BeginRpcResult::Error { code, .. } if *code == expected => Ok(()),
                    BeginRpcResult::Error { code, message } => Err(format!("Expected gRPC {}, got {} ({})", expected, code, message)),
                    BeginRpcResult::Success(_) => Err(format!("Expected gRPC {} error, got success", expected)),
                }
            },
        ),
        check_def(
            "the begin RPC error message contains {string}",
            &[("begin_result", "BeginRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected text")?.to_string();
                let result = ctx.get::<BeginRpcResult>("begin_result").ok_or("No begin_result")?;
                match result {
                    BeginRpcResult::Error { message, .. } => {
                        if message.contains(&expected) {
                            Ok(())
                        } else {
                            Err(format!("Error message '{}' doesn't contain '{}'", message, expected))
                        }
                    }
                    BeginRpcResult::Success(_) => Err(format!("Expected error, got success")),
                }
            },
        ),
        check_def(
            "the hearth contains no artifact directories under {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let dir = params.get_string(0).ok_or("Expected directory")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let path = hearth.join(&dir);
                if !path.exists() {
                    return Ok(());
                }
                let mut dirs = Vec::new();
                for entry in std::fs::read_dir(&path)
                    .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?
                {
                    let entry = entry.map_err(|e| e.to_string())?;
                    if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
                        dirs.push(entry.file_name().to_string_lossy().to_string());
                    }
                }
                if dirs.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected no artifact directories under {}, found {:?}",
                        dir, dirs
                    ))
                }
            },
        ),
        check_def(
            "the hearth step-measurement sink contains {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let path = hearth.join("step-measurement.jsonl");
                wait_for_step_measurement_contents(
                    &path,
                    &format!("a record containing '{}'", needle),
                    |contents| {
                        complete_step_measurement_record_count(contents)
                            .is_some_and(|count| count >= 1)
                            && contents.contains(&needle)
                    },
                )?;
                Ok(())
            },
        ),
        check_def(
            "the hearth step-measurement sink does not contain {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let path = hearth.join("step-measurement.jsonl");
                let contents = wait_for_step_measurement_contents(
                    &path,
                    "at least one complete record before the negative assertion",
                    |contents| {
                        complete_step_measurement_record_count(contents)
                            .is_some_and(|count| count >= 1)
                    },
                )?;
                if contents.contains(&needle) {
                    Err(format!(
                        "step-measurement sink unexpectedly contains '{}'. Contents:\n{}",
                        needle, contents
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the hearth step-measurement sink has exactly {int} records",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let path = hearth.join("step-measurement.jsonl");
                let contents = wait_for_step_measurement_contents(
                    &path,
                    &format!("at least {} complete record(s)", expected),
                    |contents| {
                        complete_step_measurement_record_count(contents)
                            .is_some_and(|count| count >= expected)
                    },
                )?;
                let count = complete_step_measurement_record_count(&contents)
                    .ok_or("step-measurement sink contained an incomplete JSONL record")?;
                if count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} step-measurement records, got {}. Contents:\n{}",
                        expected, count, contents
                    ))
                }
            },
        ),
        // The `{int} structural records for playbook {string}` variant: the
        // scenario's subject is the JOIN across the three granularities, so it
        // must count the rows belonging to ONE playbook kind rather than every
        // row in the sink. Registered here because the feature has always named
        // it and no module ever defined it — an undefined step is a scenario
        // that never ran.
        check_def(
            "the hearth step-measurement sink has exactly {int} structural records for playbook {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let kind = params.get_string(1).ok_or("Expected playbook kind")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let path = hearth.join("step-measurement.jsonl");
                let needle = format!("\"artifact_kind\":\"{}\"", kind);
                let contents = wait_for_step_measurement_contents(
                    &path,
                    &format!("at least {} record(s) for playbook {}", expected, kind),
                    |contents| {
                        contents
                            .lines()
                            .filter(|l| l.contains(&needle))
                            .count()
                            >= expected
                    },
                )?;
                let count = contents
                    .lines()
                    .filter(|l| !l.trim().is_empty() && l.contains(&needle))
                    .count();
                if count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} step-measurement records for playbook {}, got {}. Contents:\n{}",
                        expected, kind, count, contents
                    ))
                }
            },
        ),
        // The JOIN across the three granularities. The three sinks are only
        // "joinable" if the same run carries the SAME correlation triple in all
        // three, which is the claim the scenario's name makes and which no step
        // definition ever checked — it was undefined, so the scenario never ran.
        check_def(
            "the three measurement sinks join for the completed playbook run on conversation hash, project label, and playbook run id",
            &[("hearth_path", "PathBuf")],
            |ctx, _params| {
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let sinks = [
                    "step-measurement.jsonl",
                    "transition-measurement.jsonl",
                    "playbook-measurement.jsonl",
                ];
                let mut triples: Vec<(String, String)> = Vec::new();
                for sink in sinks {
                    let path = hearth.join(sink);
                    let contents = std::fs::read_to_string(&path)
                        .map_err(|e| format!("read {}: {}", path.display(), e))?;
                    let mut seen: Vec<String> = Vec::new();
                    for line in contents.lines().filter(|l| !l.trim().is_empty()) {
                        let row: serde_json::Value = serde_json::from_str(line)
                            .map_err(|e| format!("{} is not JSON: {} ({})", sink, line, e))?;
                        let triple = format!(
                            "{}|{}|{}",
                            string_field(&row, "conversation_hash").unwrap_or_default(),
                            string_field(&row, "project_label").unwrap_or_default(),
                            string_field(&row, "playbook_run_id").unwrap_or_default(),
                        );
                        if triple.split('|').any(|part| part.is_empty()) {
                            return Err(format!(
                                "{} row cannot join — a correlation key is missing: {}",
                                sink, line
                            ));
                        }
                        if !seen.contains(&triple) {
                            seen.push(triple.clone());
                        }
                    }
                    if seen.len() != 1 {
                        return Err(format!(
                            "{} carries {} distinct correlation triples, expected exactly 1: {:?}",
                            sink,
                            seen.len(),
                            seen
                        ));
                    }
                    triples.push((sink.to_string(), seen.remove(0)));
                }
                let first = &triples[0].1;
                let divergent: Vec<&(String, String)> =
                    triples.iter().filter(|(_, t)| t != first).collect();
                if !divergent.is_empty() {
                    return Err(format!(
                        "the three sinks do NOT join: {:?}",
                        triples
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the hearth step-measurement sink has exactly {int} structural records for workflow {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let workflow_kind = params.get_string(1).ok_or("Expected workflow kind")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let path = hearth.join("step-measurement.jsonl");
                let contents = wait_for_step_measurement_contents(
                    &path,
                    &format!("{} structural records", expected),
                    |contents| {
                        read_jsonl_text(contents).is_ok_and(|records| {
                            records
                                .iter()
                                .filter(|record| {
                                    string_field(record, "artifact_kind") == Some(workflow_kind)
                                        && record["intent_present"].as_bool() == Some(false)
                                        && record["expected_output_present"].as_bool() == Some(false)
                                })
                                .count()
                                >= expected
                        })
                    },
                )?;
                let records = read_jsonl_text(&contents)?;
                let count = records
                    .iter()
                    .filter(|record| {
                        string_field(record, "artifact_kind") == Some(workflow_kind)
                            && record["intent_present"].as_bool() == Some(false)
                            && record["expected_output_present"].as_bool() == Some(false)
                    })
                    .count();
                (count == expected).then_some(()).ok_or_else(|| {
                    format!(
                        "Expected {} structural step records for {}, got {}: {:?}",
                        expected, workflow_kind, count, records
                    )
                })
            },
        ),
        check_def(
            "the hearth step-measurement sink has a transition to {string} carrying correlation keys for project root {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let to_state = params.get_string(0).ok_or("Expected to_state")?.to_string();
                let project_root = params.get_string(1).ok_or("Expected project_root")?.to_string();
                let expected_label = project_label_from_root(&project_root)?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let record = wait_for_step_measurement_record(
                    &hearth.join("step-measurement.jsonl"),
                    "to_state",
                    &to_state,
                )?;
                let hash = string_field(&record, "conversation_hash").unwrap_or_default();
                if hash.is_empty() {
                    return Err(format!("step-measurement record lacks conversation_hash: {}", record));
                }
                if string_field(&record, "project_label") != Some(expected_label.as_str()) {
                    return Err(format!(
                        "expected project_label {:?}, got {:?} in {}",
                        expected_label,
                        record.get("project_label"),
                        record
                    ));
                }
                let playbook_run_id = string_field(&record, "playbook_run_id").unwrap_or_default();
                if playbook_run_id.is_empty() {
                    return Err(format!("step-measurement record lacks playbook_run_id: {}", record));
                }
                if record.to_string().contains(&project_root) {
                    return Err(format!("step-measurement record leaks project root {}: {}", project_root, record));
                }
                Ok(())
            },
        ),
        // transition_carries_step_evidence_status: assert the two fields ON THE
        // SINK, not on the function that computes them. The property is that the
        // values reach the durable record — a unit assertion would pass with the
        // emit site never wired at all, which is exactly the half-chain this
        // track keeps finding elsewhere.
        // Phase 5: the Rust/Python boundary, exercised as the REAL BINARY. A unit
        // call would prove the function; only running the binary proves the
        // subcommand the sweep will actually shell out to.
        step_def(
            "anvil-hooks artifact-of-record runs for kind {string} state {string} name {string}",
            &[],
            &[("aor_stdout", "String")],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let name = params.get_string(2).unwrap_or_default().to_string();
                let binary = crate::harness::binary_path("anvil-hooks");
                let out = std::process::Command::new(&binary)
                    .arg("artifact-of-record")
                    .args(["--kind", &kind, "--state", &state, "--display-name", &name])
                    .output()
                    .map_err(|e| format!("spawn {}: {e}", binary.display()))?;
                let mut ctx_out = Context::new();
                ctx_out.set(
                    "aor_stdout",
                    String::from_utf8_lossy(&out.stdout).trim().to_string(),
                );
                Ok(ctx_out)
            },
        ),
        check_def(
            "the artifact-of-record output contains {string}",
            &[("aor_stdout", "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected fragment")?.to_string();
                let got = ctx.get::<String>("aor_stdout").ok_or("No output")?;
                if got.contains(&want) {
                    Ok(())
                } else {
                    Err(format!("output {got:?} does not contain {want:?}"))
                }
            },
        ),
        check_def(
            "the artifact-of-record hash differs for name {string} versus {string}",
            &[],
            |_ctx, params| {
                let a = params.get_string(0).ok_or("Expected name a")?.to_string();
                let b = params.get_string(1).ok_or("Expected name b")?.to_string();
                let binary = crate::harness::binary_path("anvil-hooks");
                let run = |n: &str| -> Result<String, String> {
                    let out = std::process::Command::new(&binary)
                        .arg("artifact-of-record")
                        .args(["--kind", "track", "--state", "spec", "--display-name", n])
                        .output()
                        .map_err(|e| format!("spawn: {e}"))?;
                    Ok(String::from_utf8_lossy(&out.stdout).to_string())
                };
                let (x, y) = (run(&a)?, run(&b)?);
                if x != y {
                    Ok(())
                } else {
                    Err(format!(
                        "hash is identical for {a:?} and {b:?} — the placeholder does not track \
                         the display name, so the sweep cannot tell an untouched scaffold from a \
                         different track's"
                    ))
                }
            },
        ),
        check_def(
            "the transition to {string} records claim {string} and artifact {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let to_state = params.get_string(0).ok_or("Expected to_state")?.to_string();
                let want_claim = params.get_string(1).ok_or("Expected claim")?.to_string();
                let want_artifact = params.get_string(2).ok_or("Expected artifact")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let path = hearth.join("transition-measurement.jsonl");
                let contents = std::fs::read_to_string(&path).unwrap_or_default();
                let mut seen = Vec::new();
                for line in contents.lines().filter(|l| !l.trim().is_empty()) {
                    let v: serde_json::Value = match serde_json::from_str(line) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    if v["to_state"].as_str().unwrap_or_default() != to_state {
                        continue;
                    }
                    let claim = v["claimed_evidence_status"].as_str().unwrap_or("<absent>");
                    let artifact = v["artifact_assessment"].as_str().unwrap_or("<absent>");
                    if claim == want_claim && artifact == want_artifact {
                        return Ok(());
                    }
                    seen.push(format!("claim={claim} artifact={artifact}"));
                }
                Err(format!(
                    "no transition to {to_state:?} with claim {want_claim:?} artifact \
                     {want_artifact:?}; saw: {seen:?}"
                ))
            },
        ),
        check_def(
            "the hearth transition-measurement sink has exactly {int} records",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let path = hearth.join("transition-measurement.jsonl");
                let contents = std::fs::read_to_string(&path).unwrap_or_default();
                let count = contents.lines().filter(|l| !l.trim().is_empty()).count();
                if count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} transition-measurement records, got {}. Contents:\n{}",
                        expected, count, contents
                    ))
                }
            },
        ),
        check_def(
            "the hearth transition-measurement sink has exactly {int} record from {string} to {string} role {string} outcome {string} success {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let from_state = params.get_string(1).unwrap_or_default().to_string();
                let to_state = params.get_string(2).ok_or("Expected to_state")?.to_string();
                let role = params.get_string(3).ok_or("Expected role")?.to_string();
                let outcome = params.get_string(4).ok_or("Expected outcome")?.to_string();
                let success = params.get_string(5).ok_or("Expected success")? == "true";
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = read_jsonl(&hearth.join("transition-measurement.jsonl"))?;
                let count = records
                    .iter()
                    .filter(|record| {
                        string_field(record, "kind") == Some("transition_measurement")
                            && string_field(record, "from_state") == Some(from_state.as_str())
                            && string_field(record, "to_state") == Some(to_state.as_str())
                            && string_field(record, "role") == Some(role.as_str())
                            && string_field(record, "outcome") == Some(outcome.as_str())
                            && record.get("success").and_then(serde_json::Value::as_bool)
                                == Some(success)
                    })
                    .count();
                if count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} transition-measurement records from '{}' to '{}' role '{}' outcome '{}' success {}, got {}: {:?}",
                        expected, from_state, to_state, role, outcome, success, count, records
                    ))
                }
            },
        ),
        check_def(
            "the hearth transition-measurement sink has a transition to {string} carrying correlation keys for project root {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let to_state = params.get_string(0).ok_or("Expected to_state")?.to_string();
                let project_root = params.get_string(1).ok_or("Expected project_root")?.to_string();
                let expected_label = project_label_from_root(&project_root)?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = read_jsonl(&hearth.join("transition-measurement.jsonl"))?;
                let record = jsonl_record_matching(&records, "to_state", &to_state)
                    .ok_or_else(|| format!("No transition-measurement record for to_state '{}': {:?}", to_state, records))?;
                let hash = string_field(record, "conversation_hash").unwrap_or_default();
                if hash.is_empty() {
                    return Err(format!("transition-measurement record lacks conversation_hash: {}", record));
                }
                if string_field(record, "project_label") != Some(expected_label.as_str()) {
                    return Err(format!(
                        "expected project_label {:?}, got {:?} in {}",
                        expected_label,
                        record.get("project_label"),
                        record
                    ));
                }
                let playbook_run_id = string_field(record, "playbook_run_id").unwrap_or_default();
                if playbook_run_id.is_empty() {
                    return Err(format!("transition-measurement record lacks playbook_run_id: {}", record));
                }
                if record.to_string().contains(&project_root) {
                    return Err(format!("transition-measurement record leaks project root {}: {}", project_root, record));
                }
                Ok(())
            },
        ),
        check_def(
            "the hearth playbook-measurement sink has exactly {int} records",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let path = hearth.join("playbook-measurement.jsonl");
                let contents = std::fs::read_to_string(&path).unwrap_or_default();
                let count = contents.lines().filter(|l| !l.trim().is_empty()).count();
                if count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} playbook-measurement records, got {}. Contents:\n{}",
                        expected, count, contents
                    ))
                }
            },
        ),
        check_def(
            "the hearth playbook-measurement sink has exactly {int} record for artifact_kind {string} terminal_state {string} outcome {string} success {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let artifact_kind = params.get_string(1).ok_or("Expected artifact_kind")?.to_string();
                let terminal_state = params.get_string(2).ok_or("Expected terminal_state")?.to_string();
                let outcome = params.get_string(3).ok_or("Expected outcome")?.to_string();
                let success = parse_bool_param(params.get_string(4).ok_or("Expected success")?)?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = read_jsonl(&hearth.join("playbook-measurement.jsonl"))?;
                let count = records
                    .iter()
                    .filter(|record| {
                        string_field(record, "kind") == Some("playbook_measurement")
                            && string_field(record, "artifact_kind") == Some(artifact_kind.as_str())
                            && string_field(record, "terminal_state") == Some(terminal_state.as_str())
                            && record.get("terminal_reached").and_then(serde_json::Value::as_bool)
                                == Some(true)
                            && string_field(record, "outcome") == Some(outcome.as_str())
                            && record.get("success").and_then(serde_json::Value::as_bool)
                                == Some(success)
                    })
                    .count();
                if count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} playbook-measurement records for kind '{}' terminal '{}' outcome '{}' success {}, got {}: {:?}",
                        expected, artifact_kind, terminal_state, outcome, success, count, records
                    ))
                }
            },
        ),
        check_def(
            "the hearth playbook-measurement sink has a terminal_state {string} carrying correlation keys for project root {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let terminal_state = params.get_string(0).ok_or("Expected terminal_state")?.to_string();
                let project_root = params.get_string(1).ok_or("Expected project_root")?.to_string();
                let expected_label = project_label_from_root(&project_root)?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = read_jsonl(&hearth.join("playbook-measurement.jsonl"))?;
                let record = jsonl_record_matching(&records, "terminal_state", &terminal_state)
                    .ok_or_else(|| format!("No playbook-measurement record for terminal_state '{}': {:?}", terminal_state, records))?;
                let hash = string_field(record, "conversation_hash").unwrap_or_default();
                if hash.is_empty() {
                    return Err(format!("playbook-measurement record lacks conversation_hash: {}", record));
                }
                if string_field(record, "project_label") != Some(expected_label.as_str()) {
                    return Err(format!(
                        "expected project_label {:?}, got {:?} in {}",
                        expected_label,
                        record.get("project_label"),
                        record
                    ));
                }
                let playbook_run_id = string_field(record, "playbook_run_id").unwrap_or_default();
                if playbook_run_id.is_empty() {
                    return Err(format!("playbook-measurement record lacks playbook_run_id: {}", record));
                }
                if record.to_string().contains(&project_root) {
                    return Err(format!("playbook-measurement record leaks project root {}: {}", project_root, record));
                }
                Ok(())
            },
        ),
        check_def(
            "the three measurement sinks join for the completed workflow instance on conversation hash, project label, and workflow instance id",
            &[("hearth_path", "PathBuf")],
            |ctx, _params| {
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let mut keys_by_sink = Vec::new();
                for filename in [
                    "step-measurement.jsonl",
                    "transition-measurement.jsonl",
                    "workflow-measurement.jsonl",
                ] {
                    let records = read_jsonl(&hearth.join(filename))?;
                    let keys = records
                        .iter()
                        .map(|record| {
                            (
                                string_field(record, "conversation_hash")
                                    .unwrap_or_default()
                                    .to_string(),
                                string_field(record, "project_label")
                                    .unwrap_or_default()
                                    .to_string(),
                                string_field(record, "workflow_instance_id")
                                    .unwrap_or_default()
                                    .to_string(),
                            )
                        })
                        .collect::<std::collections::HashSet<_>>();
                    keys_by_sink.push(keys);
                }
                let common = keys_by_sink[0]
                    .intersection(&keys_by_sink[1])
                    .filter(|keys| keys_by_sink[2].contains(*keys))
                    .filter(|(conversation, project, instance)| {
                        !conversation.is_empty() && !project.is_empty() && !instance.is_empty()
                    })
                    .count();
                (common == 1)
                    .then_some(())
                    .ok_or_else(|| format!("Expected one joinable workflow run, got {}", common))
            },
        ),
        check_def(
            "the hearth review-verdict sink has exactly {int} records",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let path = hearth.join("review-verdict.jsonl");
                let contents = std::fs::read_to_string(&path).unwrap_or_default();
                let count = contents.lines().filter(|l| !l.trim().is_empty()).count();
                if count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} review-verdict records, got {}. Contents:\n{}",
                        expected, count, contents
                    ))
                }
            },
        ),
        check_def(
            "the hearth review-verdict sink has exactly {int} record for artifact_kind {string} gate_state {string} satisfaction {string} final_gate {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let artifact_kind = params.get_string(1).ok_or("Expected artifact_kind")?.to_string();
                let gate_state = params.get_string(2).ok_or("Expected gate_state")?.to_string();
                let satisfaction = params.get_string(3).ok_or("Expected satisfaction")?.to_string();
                let final_gate = parse_bool_param(params.get_string(4).ok_or("Expected final_gate")?)?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = read_jsonl(&hearth.join("review-verdict.jsonl"))?;
                let count = records
                    .iter()
                    .filter(|record| {
                        string_field(record, "kind") == Some("review_verdict")
                            && string_field(record, "artifact_kind") == Some(artifact_kind.as_str())
                            && string_field(record, "gate_state") == Some(gate_state.as_str())
                            && string_field(record, "satisfaction") == Some(satisfaction.as_str())
                            && record.get("is_final_gate").and_then(serde_json::Value::as_bool)
                                == Some(final_gate)
                    })
                    .count();
                if count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} review-verdict records for kind '{}' gate '{}' satisfaction '{}' final_gate {}, got {}: {:?}",
                        expected, artifact_kind, gate_state, satisfaction, final_gate, count, records
                    ))
                }
            },
        ),
        check_def(
            "the hearth review-verdict sink record for gate_state {string} carries the intent confidence field",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let gate_state = params.get_string(0).ok_or("Expected gate_state")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = read_jsonl(&hearth.join("review-verdict.jsonl"))?;
                let record = jsonl_record_matching(&records, "gate_state", &gate_state)
                    .ok_or_else(|| format!("No review-verdict record for gate_state '{}': {:?}", gate_state, records))?;
                // At the final gate the holistic confidence field is present (a
                // string; empty in this phase) rather than null.
                match record.get("intent_confidence") {
                    Some(v) if v.is_string() => Ok(()),
                    other => Err(format!(
                        "expected intent_confidence string at final gate, got {:?} in {}",
                        other, record
                    )),
                }
            },
        ),
        check_def(
            "the hearth review-verdict sink record for gate_state {string} carries correlation keys for project root {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let gate_state = params.get_string(0).ok_or("Expected gate_state")?.to_string();
                let project_root = params.get_string(1).ok_or("Expected project_root")?.to_string();
                let expected_label = project_label_from_root(&project_root)?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = read_jsonl(&hearth.join("review-verdict.jsonl"))?;
                let record = jsonl_record_matching(&records, "gate_state", &gate_state)
                    .ok_or_else(|| format!("No review-verdict record for gate_state '{}': {:?}", gate_state, records))?;
                let hash = string_field(record, "conversation_hash").unwrap_or_default();
                if hash.is_empty() {
                    return Err(format!("review-verdict record lacks conversation_hash: {}", record));
                }
                if string_field(record, "project_label") != Some(expected_label.as_str()) {
                    return Err(format!(
                        "expected project_label {:?}, got {:?} in {}",
                        expected_label,
                        record.get("project_label"),
                        record
                    ));
                }
                let playbook_run_id = string_field(record, "playbook_run_id").unwrap_or_default();
                if playbook_run_id.is_empty() {
                    return Err(format!("review-verdict record lacks playbook_run_id: {}", record));
                }
                if record.to_string().contains(&project_root) {
                    return Err(format!("review-verdict record leaks project root {}: {}", project_root, record));
                }
                Ok(())
            },
        ),
        check_def(
            "the hearth playbook-measurement sink has a terminal_state {string} carrying quality-vector fields with success {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let terminal_state = params.get_string(0).ok_or("Expected terminal_state")?.to_string();
                let success = parse_bool_param(params.get_string(1).ok_or("Expected success")?)?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = read_jsonl(&hearth.join("playbook-measurement.jsonl"))?;
                let record = jsonl_record_matching(&records, "terminal_state", &terminal_state)
                    .ok_or_else(|| format!("No playbook-measurement record for terminal_state '{}': {:?}", terminal_state, records))?;
                // The completion floor is still present and distinct from quality.
                if record.get("success").and_then(serde_json::Value::as_bool) != Some(success) {
                    return Err(format!(
                        "expected success {} but got {:?} in {}",
                        success,
                        record.get("success"),
                        record
                    ));
                }
                // Quality vector fields are present (shape emitted; empty/None now).
                if string_field(record, "quality_signal") != Some("leading") {
                    return Err(format!(
                        "expected quality_signal 'leading', got {:?} in {}",
                        record.get("quality_signal"),
                        record
                    ));
                }
                match record.get("quality_dimension_scores") {
                    Some(v) if v.is_array() => {}
                    other => {
                        return Err(format!(
                            "expected quality_dimension_scores array, got {:?} in {}",
                            other, record
                        ))
                    }
                }
                if !record.as_object().map(|o| o.contains_key("quality_overall")).unwrap_or(false) {
                    return Err(format!("record lacks quality_overall field: {}", record));
                }
                if !record.as_object().map(|o| o.contains_key("quality_grader")).unwrap_or(false) {
                    return Err(format!("record lacks quality_grader field: {}", record));
                }
                Ok(())
            },
        ),
        check_def(
            "the hearth playbook-measurement sink record for terminal_state {string} is graded by {string} scoring {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let terminal_state = params.get_string(0).ok_or("Expected terminal_state")?.to_string();
                let grader = params.get_string(1).ok_or("Expected a grader name")?.to_string();
                let expected: f64 = params
                    .get_string(2)
                    .ok_or("Expected a score")?
                    .parse()
                    .map_err(|e| format!("Expected a numeric score: {e}"))?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = read_jsonl(&hearth.join("playbook-measurement.jsonl"))?;
                let record = jsonl_record_matching(&records, "terminal_state", &terminal_state)
                    .ok_or_else(|| format!("No playbook-measurement record for terminal_state '{}': {:?}", terminal_state, records))?;
                // THE GRADER'S NAME IS THE DISCRIMINATOR, NOT THE SCORE. An
                // unclean run scores a real 0.0, so a check keyed on "non-zero"
                // could not tell it from a record nothing ever graded.
                if string_field(record, "quality_grader") != Some(grader.as_str()) {
                    return Err(format!(
                        "expected quality_grader '{}', got {:?} in {}",
                        grader,
                        record.get("quality_grader"),
                        record
                    ));
                }
                let overall = record
                    .get("quality_overall")
                    .and_then(serde_json::Value::as_f64)
                    .ok_or_else(|| format!("record carries no numeric quality_overall: {}", record))?;
                if (overall - expected).abs() >= f64::EPSILON {
                    return Err(format!(
                        "expected quality_overall {}, got {} in {}",
                        expected, overall, record
                    ));
                }
                let scores = record
                    .get("quality_dimension_scores")
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(|| format!("record carries no quality_dimension_scores array: {}", record))?;
                let dimension = scores
                    .iter()
                    .find(|d| string_field(d, "dimension") == Some("run_cleanliness"))
                    .ok_or_else(|| format!("record carries no run_cleanliness dimension: {}", record))?;
                let score = dimension
                    .get("score")
                    .and_then(serde_json::Value::as_f64)
                    .ok_or_else(|| format!("run_cleanliness dimension carries no numeric score: {}", record))?;
                if (score - expected).abs() >= f64::EPSILON {
                    return Err(format!(
                        "expected run_cleanliness {}, got {} in {}",
                        expected, score, record
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the hearth playbook-measurement sink record for terminal_state {string} carries no grade at all",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let terminal_state = params.get_string(0).ok_or("Expected terminal_state")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = read_jsonl(&hearth.join("playbook-measurement.jsonl"))?;
                let record = jsonl_record_matching(&records, "terminal_state", &terminal_state)
                    .ok_or_else(|| format!("No playbook-measurement record for terminal_state '{}': {:?}", terminal_state, records))?;
                // ABSENT, never a zero wearing a grade's clothes. The field must
                // be present and null; a record whose grader is missing entirely
                // would also pass a check that only asks "is it not a name".
                if !record.as_object().map(|o| o.contains_key("quality_grader")).unwrap_or(false) {
                    return Err(format!("record lacks the quality_grader field entirely: {}", record));
                }
                if let Some(name) = string_field(record, "quality_grader") {
                    return Err(format!(
                        "record was graded by '{}'; expected no grade at all: {}",
                        name, record
                    ));
                }
                if record.get("quality_overall").and_then(serde_json::Value::as_f64).is_some() {
                    return Err(format!(
                        "record carries a quality_overall score with no grader — a score nobody produced: {}",
                        record
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the hearth playbook-measurement sink record for terminal_state {string} carries a non-empty playbook_version",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let terminal_state = params.get_string(0).ok_or("Expected terminal_state")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = read_jsonl(&hearth.join("playbook-measurement.jsonl"))?;
                let record = jsonl_record_matching(&records, "terminal_state", &terminal_state)
                    .ok_or_else(|| format!("No playbook-measurement record for terminal_state '{}': {:?}", terminal_state, records))?;
                match string_field(record, "playbook_version") {
                    Some(v) if !v.is_empty() => Ok(()),
                    other => Err(format!(
                        "expected a non-empty playbook_version (content hash), got {:?} in {}",
                        other, record
                    )),
                }
            },
        ),
        check_def(
            "the hearth review-verdict sink record for gate_state {string} carries a non-empty playbook_version",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let gate_state = params.get_string(0).ok_or("Expected gate_state")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = read_jsonl(&hearth.join("review-verdict.jsonl"))?;
                let record = jsonl_record_matching(&records, "gate_state", &gate_state)
                    .ok_or_else(|| format!("No review-verdict record for gate_state '{}': {:?}", gate_state, records))?;
                match string_field(record, "playbook_version") {
                    Some(v) if !v.is_empty() => Ok(()),
                    other => Err(format!(
                        "expected a non-empty playbook_version (content hash), got {:?} in {}",
                        other, record
                    )),
                }
            },
        ),
        check_def(
            "the activity log command {string} carries correlation keys for project root {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let command = params.get_string(0).ok_or("Expected command")?.to_string();
                let project_root = params.get_string(1).ok_or("Expected project_root")?.to_string();
                let expected_label = project_label_from_root(&project_root)?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = read_jsonl(&hearth.join("activity-log.jsonl"))?;
                let record = jsonl_record_matching(&records, "command", &command)
                    .ok_or_else(|| format!("No activity-log command '{}': {:?}", command, records))?;
                let hash = string_field(record, "conversation_hash").unwrap_or_default();
                if hash.is_empty() {
                    return Err(format!("activity-log record lacks conversation_hash: {}", record));
                }
                if string_field(record, "project_label") != Some(expected_label.as_str()) {
                    return Err(format!(
                        "expected project_label {:?}, got {:?} in {}",
                        expected_label,
                        record.get("project_label"),
                        record
                    ));
                }
                let playbook_run_id = string_field(record, "playbook_run_id").unwrap_or_default();
                if playbook_run_id.is_empty() {
                    return Err(format!("activity-log record lacks playbook_run_id: {}", record));
                }
                if record.to_string().contains(&project_root) {
                    return Err(format!("activity-log record leaks project root {}: {}", project_root, record));
                }
                Ok(())
            },
        ),
        check_def(
            "the activity log command {string} has no conversation_hash field",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let command = params.get_string(0).ok_or("Expected command")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = read_jsonl(&hearth.join("activity-log.jsonl"))?;
                let record = jsonl_record_matching(&records, "command", &command)
                    .ok_or_else(|| format!("No activity-log command '{}': {:?}", command, records))?;
                if record.get("conversation_hash").is_some() {
                    Err(format!("activity-log command '{}' unexpectedly has conversation_hash: {}", command, record))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the activity log command {string} has the same conversation_hash as command {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let left = params.get_string(0).ok_or("Expected left command")?.to_string();
                let right = params.get_string(1).ok_or("Expected right command")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = read_jsonl(&hearth.join("activity-log.jsonl"))?;
                let left_record = jsonl_record_matching(&records, "command", &left)
                    .ok_or_else(|| format!("No activity-log command '{}': {:?}", left, records))?;
                let right_record = jsonl_record_matching(&records, "command", &right)
                    .ok_or_else(|| format!("No activity-log command '{}': {:?}", right, records))?;
                let left_hash = string_field(left_record, "conversation_hash").unwrap_or_default();
                let right_hash = string_field(right_record, "conversation_hash").unwrap_or_default();
                if !left_hash.is_empty() && left_hash == right_hash {
                    Ok(())
                } else {
                    Err(format!(
                        "conversation_hash mismatch: {}={:?}, {}={:?}; records: {:?}",
                        left, left_hash, right, right_hash, records
                    ))
                }
            },
        ),
        check_def(
            "the routing activity sink carries correlation keys for project root {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let project_root = params.get_string(0).ok_or("Expected project_root")?.to_string();
                let expected_label = project_label_from_root(&project_root)?;
                let project_root_path = PathBuf::from(&project_root);
                let fallback_hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let sink_root = if project_root_path.join("routing-activity.jsonl").exists() {
                    project_root_path.as_path()
                } else {
                    fallback_hearth.as_path()
                };
                let records = read_jsonl(&sink_root.join("routing-activity.jsonl"))?;
                let record = records.first().ok_or("routing-activity sink is empty")?;
                let hash = string_field(record, "conversation_hash").unwrap_or_default();
                if hash.is_empty() {
                    return Err(format!("routing-activity record lacks conversation_hash: {}", record));
                }
                if string_field(record, "project_label") != Some(expected_label.as_str()) {
                    return Err(format!(
                        "expected project_label {:?}, got {:?} in {}",
                        expected_label,
                        record.get("project_label"),
                        record
                    ));
                }
                if record.to_string().contains(&project_root) {
                    return Err(format!("routing-activity record leaks project root {}: {}", project_root, record));
                }
                Ok(())
            },
        ),
        check_def(
            "the {string} sink does not contain raw text {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let sink_name = params.get_string(0).ok_or("Expected sink name")?.to_string();
                let raw = params.get_string(1).ok_or("Expected raw text")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let hearth_sink = hearth.join(&sink_name);
                let sink_path = if hearth_sink.exists() {
                    hearth_sink
                } else if let Some(project_root) = ctx.get::<PathBuf>("route_project_root") {
                    project_root.join(&sink_name)
                } else {
                    hearth_sink
                };
                let contents = std::fs::read_to_string(&sink_path)
                    .map_err(|e| format!("read {}: {}", sink_name, e))?;
                if contents.contains(&raw) {
                    Err(format!("{} unexpectedly contains raw text {:?}: {}", sink_name, raw, contents))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the begin RPC response has non-empty {string}",
            &[("begin_result", "BeginRpcResult")],
            |ctx, params| {
                let field = params.get_string(0).ok_or("Expected field name")?.to_string();
                let result = ctx.get::<BeginRpcResult>("begin_result").ok_or("No begin_result")?;
                match result {
                    BeginRpcResult::Success(r) => {
                        let value = match field.as_str() {
                            "artifact_text" => &r.artifact_text,
                            "review_context_text" => &r.review_context_text,
                            "review_doc_path" => &r.review_doc_path,
                            "track_path" => &r.track_path,
                            "context_text" => &r.context_text,
                            "intent" => &r.intent,
                            "expected_output" => &r.expected_output,
                            "playbook_id" => &r.playbook_id,
                            _ => return Err(format!("Unknown BeginResponse field '{}'", field)),
                        };
                        if value.is_empty() {
                            Err(format!("BeginResponse field '{}' is empty", field))
                        } else {
                            Ok(())
                        }
                    }
                    BeginRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the begin RPC response {string} contains {string}",
            &[("begin_result", "BeginRpcResult")],
            |ctx, params| {
                let field = params.get_string(0).ok_or("Expected field name")?.to_string();
                let needle = params.get_string(1).ok_or("Expected needle")?.to_string();
                let result = ctx.get::<BeginRpcResult>("begin_result").ok_or("No begin_result")?;
                match result {
                    BeginRpcResult::Success(r) => {
                        let value = match field.as_str() {
                            "artifact_text" => &r.artifact_text,
                            "review_context_text" => &r.review_context_text,
                            "review_doc_path" => &r.review_doc_path,
                            "track_path" => &r.track_path,
                            "context_text" => &r.context_text,
                            "intent" => &r.intent,
                            "expected_output" => &r.expected_output,
                            "playbook_id" => &r.playbook_id,
                            _ => return Err(format!("Unknown BeginResponse field '{}'", field)),
                        };
                        if value.contains(&needle) {
                            Ok(())
                        } else {
                            Err(format!(
                                "Expected BeginResponse field '{}' to contain '{}', got '{}'",
                                field, needle, value
                            ))
                        }
                    }
                    BeginRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the begin RPC response {string} does not contain {string}",
            &[("begin_result", "BeginRpcResult")],
            |ctx, params| {
                let field = params.get_string(0).ok_or("Expected field name")?.to_string();
                let needle = params.get_string(1).ok_or("Expected needle")?.to_string();
                let result = ctx.get::<BeginRpcResult>("begin_result").ok_or("No begin_result")?;
                match result {
                    BeginRpcResult::Success(r) => {
                        let value = match field.as_str() {
                            "artifact_text" => &r.artifact_text,
                            "review_context_text" => &r.review_context_text,
                            "review_doc_path" => &r.review_doc_path,
                            "track_path" => &r.track_path,
                            "context_text" => &r.context_text,
                            "intent" => &r.intent,
                            "expected_output" => &r.expected_output,
                            "playbook_id" => &r.playbook_id,
                            _ => return Err(format!("Unknown BeginResponse field '{}'", field)),
                        };
                        if !value.contains(&needle) {
                            Ok(())
                        } else {
                            Err(format!(
                                "Expected BeginResponse field '{}' to NOT contain '{}', got '{}'",
                                field, needle, value
                            ))
                        }
                    }
                    BeginRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the begin RPC response {string} is exactly {string}",
            &[("begin_result", "BeginRpcResult")],
            |ctx, params| {
                let field = params.get_string(0).ok_or("Expected field name")?.to_string();
                // Gherkin string literals can't carry raw newlines; authors write \n / \t
                // for multi-line bodies (e.g. served hook context_text). Unescape before
                // the exact compare. No-op for single-line expectations.
                let expected = params
                    .get_string(1)
                    .ok_or("Expected value")?
                    .replace("\\n", "\n")
                    .replace("\\t", "\t");
                let result = ctx.get::<BeginRpcResult>("begin_result").ok_or("No begin_result")?;
                match result {
                    BeginRpcResult::Success(r) => {
                        let value = match field.as_str() {
                            "artifact_text" => &r.artifact_text,
                            "review_context_text" => &r.review_context_text,
                            "review_doc_path" => &r.review_doc_path,
                            "track_path" => &r.track_path,
                            "context_text" => &r.context_text,
                            "intent" => &r.intent,
                            "expected_output" => &r.expected_output,
                            "playbook_id" => &r.playbook_id,
                            _ => return Err(format!("Unknown BeginResponse field '{}'", field)),
                        };
                        if value == &expected {
                            Ok(())
                        } else {
                            Err(format!(
                                "Expected BeginResponse field '{}' to equal '{}', got '{}'",
                                field, expected, value
                            ))
                        }
                    }
                    BeginRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the checkin RPC next_step text contains {string}",
            &[("checkin_result", "CheckinRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected text")?.to_string();
                let result = ctx.get::<CheckinRpcResult>("checkin_result").ok_or("No checkin_result")?;
                match result {
                    CheckinRpcResult::Success(r) => {
                        if r.next_step.contains(&expected) {
                            Ok(())
                        } else {
                            Err(format!("next_step '{}' doesn't contain '{}'", r.next_step, expected))
                        }
                    }
                    CheckinRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),

        // ===== complete next_step assertions =====
        check_def(
            "the complete RPC response next_step contains {string}",
            &[("complete_rpc_result", "CompleteRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected text")?.to_string();
                let result = ctx.get::<CompleteRpcResult>("complete_rpc_result").ok_or("No complete_rpc_result")?;
                match result {
                    CompleteRpcResult::Success(r) => {
                        if r.next_step.contains(&expected) {
                            Ok(())
                        } else {
                            Err(format!("complete next_step '{}' doesn't contain '{}'", r.next_step, expected))
                        }
                    }
                    CompleteRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the complete RPC response next_step does not contain {string}",
            &[("complete_rpc_result", "CompleteRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected text")?.to_string();
                let result = ctx.get::<CompleteRpcResult>("complete_rpc_result").ok_or("No complete_rpc_result")?;
                match result {
                    CompleteRpcResult::Success(r) => {
                        if r.next_step.contains(&expected) {
                            Err(format!("complete next_step '{}' should not contain '{}'", r.next_step, expected))
                        } else {
                            Ok(())
                        }
                    }
                    CompleteRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        // ===== begin next_step assertions =====
        check_def(
            "the begin RPC response next_step contains {string}",
            &[("begin_result", "BeginRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected text")?.to_string();
                let result = ctx.get::<BeginRpcResult>("begin_result").ok_or("No begin_result")?;
                match result {
                    BeginRpcResult::Success(r) => {
                        if r.next_step.contains(&expected) {
                            Ok(())
                        } else {
                            Err(format!("begin next_step '{}' doesn't contain '{}'", r.next_step, expected))
                        }
                    }
                    BeginRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the begin RPC response next_step does not contain {string}",
            &[("begin_result", "BeginRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected text")?.to_string();
                let result = ctx.get::<BeginRpcResult>("begin_result").ok_or("No begin_result")?;
                match result {
                    BeginRpcResult::Success(r) => {
                        if r.next_step.contains(&expected) {
                            Err(format!("begin next_step '{}' should not contain '{}'", r.next_step, expected))
                        } else {
                            Ok(())
                        }
                    }
                    BeginRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the begin RPC response has non-empty next_step",
            &[("begin_result", "BeginRpcResult")],
            |ctx, _params| {
                let result = ctx.get::<BeginRpcResult>("begin_result").ok_or("No begin_result")?;
                match result {
                    BeginRpcResult::Success(r) => {
                        if r.next_step.trim().is_empty() {
                            Err("begin next_step is empty".to_string())
                        } else {
                            Ok(())
                        }
                    }
                    BeginRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),

        // ===== Snapshot RPC steps =====
        async_step_def(
            "the snapshot RPC is called with:",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                ("snapshot_rpc_result", "SnapshotRpcResult"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |mut ctx, params| async move {
                let table = params.data_table().ok_or("Expected data table")?;
                let mut pairs: Vec<(String, String)> = Vec::new();
                if table.headers.len() >= 2 {
                    pairs.push((
                        table.headers[0].trim().to_string(),
                        table.headers[1].trim().to_string(),
                    ));
                }
                for row in &table.rows {
                    if row.len() >= 2 {
                        pairs.push((row[0].trim().to_string(), row[1].trim().to_string()));
                    }
                }
                let mut req = anvil_engine::proto::SnapshotRequest {
                    hearth_path: String::new(),
                    artifact_path: String::new(),
                    to_state: String::new(),
                    actor_name: String::new(),
                    actor_role: String::new(),
                    approver: String::new(),
                    note: String::new(),
                    actor_type: String::new(),
                    actor_model: String::new(),
                    actor_provider: String::new(),
                    actor_context_window: 0,
                    actor_sdk_version: String::new(),
                    actor_entrypoint: String::new(),
                    projection_only: false,
                    event_type: String::new(),
                    conversation_id: String::new(),
                    project_root: String::new(),
                    claimed_evidence: Vec::new(),
                };
                for (k, v) in pairs {
                    match k.as_str() {
                        "hearth_path" => req.hearth_path = v,
                        "artifact_path" => req.artifact_path = v,
                        "to_state" => req.to_state = v,
                        "actor_name" => req.actor_name = v,
                        "actor_role" => req.actor_role = v,
                        "approver" => req.approver = v,
                        "note" => req.note = v,
                        "actor_type" => req.actor_type = v,
                        "actor_model" => req.actor_model = v,
                        "actor_provider" => req.actor_provider = v,
                        "actor_context_window" => {
                            req.actor_context_window = v.parse().unwrap_or(0)
                        }
                        "actor_sdk_version" => req.actor_sdk_version = v,
                        "actor_entrypoint" => req.actor_entrypoint = v,
                        "projection_only" => req.projection_only = v == "true",
                        "event_type" => req.event_type = v,
                        "conversation_id" => req.conversation_id = v,
                        "project_root" => req.project_root = v,
                        other => return Err(format!("Unknown key: '{}'", other)),
                    }
                }
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let port = engine.port;
                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        match client.snapshot(crate::surfaced(req)).await {
                            Ok(response) => SnapshotRpcResult::Success(response.into_inner()),
                            Err(status) => SnapshotRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => SnapshotRpcResult::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("snapshot_rpc_result", result);
                // Thread hearth_path through (when present) so on-disk file
                // assertions can run after the snapshot RPC, mirroring the
                // complete RPC step.
                if let Some(hearth_path) = ctx.take::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hearth_path);
                }
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        check_def(
            "the snapshot RPC response success is {string}",
            &[("snapshot_rpc_result", "SnapshotRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected bool")?;
                let want = expected == "true";
                let r = ctx.get::<SnapshotRpcResult>("snapshot_rpc_result").ok_or("No snapshot_rpc_result")?;
                match r {
                    SnapshotRpcResult::Success(resp) if resp.success == want => Ok(()),
                    SnapshotRpcResult::Success(resp) => Err(format!("Expected success={}, got {}", want, resp.success)),
                    SnapshotRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the snapshot RPC response timestamp matches {string}",
            &[("snapshot_rpc_result", "SnapshotRpcResult")],
            |ctx, params| {
                let pat = params.get_string(0).ok_or("Expected pattern")?;
                let r = ctx.get::<SnapshotRpcResult>("snapshot_rpc_result").ok_or("No snapshot_rpc_result")?;
                match r {
                    SnapshotRpcResult::Success(resp) => {
                        if crate::snapshot::SimpleRegex::matches(pat, &resp.timestamp) {
                            Ok(())
                        } else {
                            Err(format!("timestamp '{}' does not match '{}'", resp.timestamp, pat))
                        }
                    }
                    SnapshotRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the snapshot RPC response actor_name is {string}",
            &[("snapshot_rpc_result", "SnapshotRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected actor_name")?;
                let r = ctx.get::<SnapshotRpcResult>("snapshot_rpc_result").ok_or("No snapshot_rpc_result")?;
                match r {
                    SnapshotRpcResult::Success(resp) if resp.actor_name == *expected => Ok(()),
                    SnapshotRpcResult::Success(resp) => Err(format!("Expected actor_name '{}', got '{}'", expected, resp.actor_name)),
                    SnapshotRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the snapshot RPC response actor_name matches {string}",
            &[("snapshot_rpc_result", "SnapshotRpcResult")],
            |ctx, params| {
                let pat = params.get_string(0).ok_or("Expected pattern")?;
                let r = ctx.get::<SnapshotRpcResult>("snapshot_rpc_result").ok_or("No snapshot_rpc_result")?;
                match r {
                    SnapshotRpcResult::Success(resp) => {
                        if crate::snapshot::SimpleRegex::matches(pat, &resp.actor_name) {
                            Ok(())
                        } else {
                            Err(format!("actor_name '{}' does not match '{}'", resp.actor_name, pat))
                        }
                    }
                    SnapshotRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the snapshot RPC response projections_updated contains {string}",
            &[("snapshot_rpc_result", "SnapshotRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected file")?;
                let r = ctx.get::<SnapshotRpcResult>("snapshot_rpc_result").ok_or("No snapshot_rpc_result")?;
                match r {
                    SnapshotRpcResult::Success(resp) => {
                        if resp.projections_updated.iter().any(|s| s == expected) {
                            Ok(())
                        } else {
                            Err(format!("Expected '{}' in projections_updated, got {:?}", expected, resp.projections_updated))
                        }
                    }
                    SnapshotRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the snapshot RPC response projections_updated is empty",
            &[("snapshot_rpc_result", "SnapshotRpcResult")],
            |ctx, _params| {
                let r = ctx.get::<SnapshotRpcResult>("snapshot_rpc_result").ok_or("No snapshot_rpc_result")?;
                match r {
                    SnapshotRpcResult::Success(resp) if resp.projections_updated.is_empty() => Ok(()),
                    SnapshotRpcResult::Success(resp) => Err(format!("Expected empty projections_updated, got {:?}", resp.projections_updated)),
                    SnapshotRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the snapshot RPC response status_updated is {string}",
            &[("snapshot_rpc_result", "SnapshotRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected bool")?;
                let want = expected == "true";
                let r = ctx.get::<SnapshotRpcResult>("snapshot_rpc_result").ok_or("No snapshot_rpc_result")?;
                match r {
                    SnapshotRpcResult::Success(resp) if resp.status_updated == want => Ok(()),
                    SnapshotRpcResult::Success(resp) => Err(format!("Expected status_updated={}, got {}", want, resp.status_updated)),
                    SnapshotRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the snapshot RPC response registry_updated is {string}",
            &[("snapshot_rpc_result", "SnapshotRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected bool")?;
                let want = expected == "true";
                let r = ctx.get::<SnapshotRpcResult>("snapshot_rpc_result").ok_or("No snapshot_rpc_result")?;
                match r {
                    SnapshotRpcResult::Success(resp) if resp.registry_updated == want => Ok(()),
                    SnapshotRpcResult::Success(resp) => Err(format!("Expected registry_updated={}, got {}", want, resp.registry_updated)),
                    SnapshotRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the snapshot RPC response warnings contain {string}",
            &[("snapshot_rpc_result", "SnapshotRpcResult")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?.to_string();
                let r = ctx.get::<SnapshotRpcResult>("snapshot_rpc_result").ok_or("No snapshot_rpc_result")?;
                match r {
                    SnapshotRpcResult::Success(resp) => {
                        if resp.warnings.iter().any(|w| w.contains(&needle)) {
                            Ok(())
                        } else {
                            Err(format!("Expected a warning containing '{}', got {:?}", needle, resp.warnings))
                        }
                    }
                    SnapshotRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the snapshot RPC returns gRPC status {string}",
            &[("snapshot_rpc_result", "SnapshotRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected status code")?.to_string();
                let r = ctx.get::<SnapshotRpcResult>("snapshot_rpc_result").ok_or("No snapshot_rpc_result")?;
                match r {
                    SnapshotRpcResult::Error { code, .. } if *code == expected => Ok(()),
                    SnapshotRpcResult::Error { code, message } => Err(format!("Expected gRPC {}, got {} ({})", expected, code, message)),
                    SnapshotRpcResult::Success(_) => Err(format!("Expected gRPC {} error, got success", expected)),
                }
            },
        ),
        check_def(
            "the snapshot RPC error message contains {string}",
            &[("snapshot_rpc_result", "SnapshotRpcResult")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected text")?;
                let r = ctx.get::<SnapshotRpcResult>("snapshot_rpc_result").ok_or("No snapshot_rpc_result")?;
                match r {
                    SnapshotRpcResult::Error { message, .. } if message.contains(needle) => Ok(()),
                    SnapshotRpcResult::Error { message, .. } => {
                        Err(format!("Error message '{}' doesn't contain '{}'", message, needle))
                    }
                    SnapshotRpcResult::Success(_) => Err(format!("Expected error, got success")),
                }
            },
        ),

        // ===== Complete RPC steps =====
        async_step_def(
            "the complete RPC is called with:",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("complete_rpc_result", "CompleteRpcResult"),
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("hearth_x_path", "PathBuf"),
                ("hearth_y_path", "PathBuf"),
                ("hearth_y_snapshot", "HearthSnapshot"),
            ],
            |mut ctx, params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let hearth_x_path = ctx.get::<PathBuf>("hearth_x_path").cloned();
                let hearth_y_path = ctx.get::<PathBuf>("hearth_y_path").cloned();
                let hearth_y_snapshot = ctx.take::<HearthSnapshot>("hearth_y_snapshot");
                let port = engine.port;
                let table = params.data_table().ok_or("Expected data table")?;

                let mut artifact_path = String::new();
                let mut actor_name = String::new();
                let mut actor_type = String::new();
                let mut actor_model = String::new();
                let mut actor_provider = String::new();
                let mut actor_context_window: i64 = 0;
                let mut actor_sdk_version = String::new();
                let mut actor_entrypoint = String::new();
                let mut satisfaction = String::new();
                let mut approver = String::new();
                let mut note = String::new();
                let mut reflection_notes = String::new();
                let mut findings = String::new();
                // Per-request hearth (spec Req 1). Defaults to empty → engine
                // falls back to its --hearth spawn default (no-regression path).
                // The literal "<hearth>" expands to the spawned hearth fixture's
                // absolute path so multi-hearth scenarios can target it.
                let mut complete_hearth_path = String::new();

                let mut pairs: Vec<(String, String)> = Vec::new();
                if table.headers.len() >= 2 {
                    pairs.push((table.headers[0].trim().to_string(), table.headers[1].trim().to_string()));
                }
                for row in &table.rows {
                    if row.len() >= 2 {
                        pairs.push((row[0].trim().to_string(), row[1].trim().to_string()));
                    }
                }
                for (key, value) in pairs {
                    match key.as_str() {
                        "artifact_path" => artifact_path = value,
                        "actor_name" => actor_name = value,
                        "actor_type" => actor_type = value,
                        "actor_model" => actor_model = value,
                        "actor_provider" => actor_provider = value,
                        "actor_context_window" => actor_context_window = value.parse().unwrap_or(0),
                        "actor_sdk_version" => actor_sdk_version = value,
                        "actor_entrypoint" => actor_entrypoint = value,
                        "satisfaction" => satisfaction = value,
                        "approver" => approver = value,
                        "note" => note = value,
                        "reflection_notes" => reflection_notes = value,
                        "findings" => findings = value.replace("\\n", "\n"),
                        "hearth_path" => {
                            complete_hearth_path = match value.as_str() {
                                "<hearth>" | "<hearth_x>" => {
                                    hearth_path.to_string_lossy().into_owned()
                                }
                                "<hearth_y>" => hearth_y_path
                                    .as_ref()
                                    .map(|p: &PathBuf| p.to_string_lossy().into_owned())
                                    .ok_or("No hearth_y_path in context for <hearth_y>")?,
                                _ => value,
                            };
                        }
                        other => return Err(format!("Unknown complete RPC field: '{}'", other)),
                    }
                }

                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::CompleteRequest {
                            artifact_path,
                            actor_name,
                            actor_type,
                            actor_model,
                            actor_provider,
                            actor_context_window,
                            actor_sdk_version,
                            actor_entrypoint,
                            satisfaction,
                            approver,
                            note,
                            reflection_notes,
                            hearth_path: complete_hearth_path,
                            findings,
                            conversation_id: String::new(),
                            project_root: String::new(),
                            claimed_evidence: Vec::new(),
                        });
                        match client.complete(request).await {
                            Ok(response) => CompleteRpcResult::Success(response.into_inner()),
                            Err(status) => CompleteRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => CompleteRpcResult::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };

                let mut out = Context::new();
                out.set("complete_rpc_result", result);
                out.set("engine_process", engine);
                out.set("hearth_path", hearth_path);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                if let Some(p) = hearth_x_path { out.set("hearth_x_path", p); }
                if let Some(p) = hearth_y_path { out.set("hearth_y_path", p); }
                if let Some(s) = hearth_y_snapshot { out.set("hearth_y_snapshot", s); }
                Ok(out)
            },
        ),
        check_def(
            "the complete RPC response new_state is {string}",
            &[("complete_rpc_result", "CompleteRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected new_state")?.to_string();
                let r = ctx.get::<CompleteRpcResult>("complete_rpc_result").ok_or("No complete_rpc_result")?;
                match r {
                    CompleteRpcResult::Success(resp) if resp.new_state == expected => Ok(()),
                    CompleteRpcResult::Success(resp) => Err(format!("Expected new_state '{}', got '{}'", expected, resp.new_state)),
                    CompleteRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the complete RPC response transition_at matches {string}",
            &[("complete_rpc_result", "CompleteRpcResult")],
            |ctx, params| {
                let pattern = params.get_string(0).ok_or("Expected pattern")?.to_string();
                let r = ctx.get::<CompleteRpcResult>("complete_rpc_result").ok_or("No complete_rpc_result")?;
                match r {
                    CompleteRpcResult::Success(resp) => {
                        use crate::snapshot::SimpleRegex;
                        if SimpleRegex::matches(&pattern, &resp.transition_at) {
                            Ok(())
                        } else {
                            Err(format!("transition_at '{}' does not match '{}'", resp.transition_at, pattern))
                        }
                    }
                    CompleteRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the complete RPC response artifact_path is {string}",
            &[("complete_rpc_result", "CompleteRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected artifact_path")?.to_string();
                let r = ctx.get::<CompleteRpcResult>("complete_rpc_result").ok_or("No complete_rpc_result")?;
                match r {
                    CompleteRpcResult::Success(resp) if resp.artifact_path == expected => Ok(()),
                    CompleteRpcResult::Success(resp) => Err(format!("Expected artifact_path '{}', got '{}'", expected, resp.artifact_path)),
                    CompleteRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the complete RPC response warnings contain {string}",
            &[("complete_rpc_result", "CompleteRpcResult")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?.to_string();
                let r = ctx.get::<CompleteRpcResult>("complete_rpc_result").ok_or("No complete_rpc_result")?;
                match r {
                    CompleteRpcResult::Success(resp) => {
                        if resp.warnings.iter().any(|w| w.contains(&needle)) {
                            Ok(())
                        } else {
                            Err(format!("Expected a warning containing '{}', got {:?}", needle, resp.warnings))
                        }
                    }
                    CompleteRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the complete RPC response has {int} warnings",
            &[("complete_rpc_result", "CompleteRpcResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let r = ctx.get::<CompleteRpcResult>("complete_rpc_result").ok_or("No complete_rpc_result")?;
                match r {
                    CompleteRpcResult::Success(resp) if resp.warnings.len() == expected => Ok(()),
                    CompleteRpcResult::Success(resp) => Err(format!("Expected {} warnings, got {}: {:?}", expected, resp.warnings.len(), resp.warnings)),
                    CompleteRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the complete RPC returns gRPC status {string}",
            &[("complete_rpc_result", "CompleteRpcResult")],
            |ctx, params| {
                let expected_code = params.get_string(0).ok_or("Missing status code")?.to_string();
                let r = ctx.get::<CompleteRpcResult>("complete_rpc_result").ok_or("No complete_rpc_result")?;
                match r {
                    CompleteRpcResult::Error { code, .. } if *code == expected_code => Ok(()),
                    CompleteRpcResult::Error { code, message } => {
                        if code.contains(&expected_code) { Ok(()) }
                        else { Err(format!("Expected gRPC {}, got {} ({})", expected_code, code, message)) }
                    },
                    CompleteRpcResult::Success(_) => Err(format!("Expected gRPC {} error, got success", expected_code)),
                }
            },
        ),
        check_def(
            "the complete RPC error message contains {string}",
            &[("complete_rpc_result", "CompleteRpcResult")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected text")?;
                let r = ctx.get::<CompleteRpcResult>("complete_rpc_result").ok_or("No complete_rpc_result")?;
                match r {
                    CompleteRpcResult::Error { message, .. } if message.contains(needle) => Ok(()),
                    CompleteRpcResult::Error { message, .. } => Err(format!("Error message '{}' doesn't contain '{}'", message, needle)),
                    CompleteRpcResult::Success(_) => Err("Expected error, got success".to_string()),
                }
            },
        ),
        check_def(
            "the complete RPC response reflection_path ends with {string}",
            &[("complete_rpc_result", "CompleteRpcResult")],
            |ctx, params| {
                let suffix = params.get_string(0).ok_or("Expected suffix")?.to_string();
                let r = ctx.get::<CompleteRpcResult>("complete_rpc_result").ok_or("No complete_rpc_result")?;
                match r {
                    CompleteRpcResult::Success(resp) => {
                        if resp.reflection_path.ends_with(&suffix) {
                            Ok(())
                        } else {
                            Err(format!(
                                "Expected reflection_path to end with '{}', got '{}'",
                                suffix, resp.reflection_path
                            ))
                        }
                    }
                    CompleteRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the complete RPC response reflection_path contains {string}",
            &[("complete_rpc_result", "CompleteRpcResult")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?.to_string();
                let r = ctx.get::<CompleteRpcResult>("complete_rpc_result").ok_or("No complete_rpc_result")?;
                match r {
                    CompleteRpcResult::Success(resp) => {
                        if resp.reflection_path.contains(&needle) {
                            Ok(())
                        } else {
                            Err(format!(
                                "Expected reflection_path to contain '{}', got '{}'",
                                needle, resp.reflection_path
                            ))
                        }
                    }
                    CompleteRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        // Slice C: carry_forward_path assertions on the complete RPC response.
        check_def(
            "the complete RPC response carry_forward_path ends with {string}",
            &[("complete_rpc_result", "CompleteRpcResult")],
            |ctx, params| {
                let suffix = params.get_string(0).ok_or("Expected suffix")?.to_string();
                let r = ctx.get::<CompleteRpcResult>("complete_rpc_result").ok_or("No complete_rpc_result")?;
                match r {
                    CompleteRpcResult::Success(resp) => {
                        if resp.carry_forward_path.ends_with(&suffix) {
                            Ok(())
                        } else {
                            Err(format!(
                                "Expected carry_forward_path to end with '{}', got '{}'",
                                suffix, resp.carry_forward_path
                            ))
                        }
                    }
                    CompleteRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the complete RPC response carry_forward_path is non-empty",
            &[("complete_rpc_result", "CompleteRpcResult")],
            |ctx, _params| {
                let r = ctx.get::<CompleteRpcResult>("complete_rpc_result").ok_or("No complete_rpc_result")?;
                match r {
                    CompleteRpcResult::Success(resp) => {
                        if !resp.carry_forward_path.is_empty() {
                            Ok(())
                        } else {
                            Err("Expected non-empty carry_forward_path, got empty".to_string())
                        }
                    }
                    CompleteRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the hearth directory {string} exists",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected path")?.to_string();
                let path = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .join(&rel);
                if path.exists() && path.is_dir() {
                    Ok(())
                } else {
                    Err(format!("Expected directory '{}' to exist", path.display()))
                }
            },
        ),
        check_def(
            "the hearth directory {string} does not exist",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected path")?.to_string();
                let path = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .join(&rel);
                if path.exists() {
                    Err(format!("Expected directory '{}' to NOT exist", path.display()))
                } else {
                    Ok(())
                }
            },
        ),

        // ===== Catalog invalid_artifacts step defs (Phase 3) =====

        check_def(
            "the catalog response contains {int} invalid artifacts",
            &[("catalog_result", "CatalogRpcResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let result = ctx.get::<CatalogRpcResult>("catalog_result").ok_or("No catalog_result")?;
                match result {
                    CatalogRpcResult::Success(resp) => {
                        if resp.invalid_artifacts.len() != expected {
                            Err(format!(
                                "Expected {} invalid artifacts, got {} ({:?})",
                                expected,
                                resp.invalid_artifacts.len(),
                                resp.invalid_artifacts.iter().map(|iv| format!("{}:{}", iv.id, iv.code)).collect::<Vec<_>>()
                            ))
                        } else {
                            Ok(())
                        }
                    }
                    CatalogRpcResult::Error(e) => Err(format!("Expected success, got error: {}", e)),
                    CatalogRpcResult::Status { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the catalog invalid artifacts include an entry with id {string} and code {string}",
            &[("catalog_result", "CatalogRpcResult")],
            |ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let code = params.get_string(1).ok_or("Expected code")?.to_string();
                let result = ctx.get::<CatalogRpcResult>("catalog_result").ok_or("No catalog_result")?;
                match result {
                    CatalogRpcResult::Success(resp) => {
                        let found = resp.invalid_artifacts.iter()
                            .any(|iv| iv.id == id && iv.code == code);
                        if found {
                            Ok(())
                        } else {
                            Err(format!(
                                "No invalid artifact with id='{}' and code='{}'. Got: {:?}",
                                id, code,
                                resp.invalid_artifacts.iter().map(|iv| format!("{}:{}", iv.id, iv.code)).collect::<Vec<_>>()
                            ))
                        }
                    }
                    CatalogRpcResult::Error(e) => Err(format!("Expected success, got error: {}", e)),
                    CatalogRpcResult::Status { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the catalog response does not have invalid artifacts",
            &[("catalog_result", "CatalogRpcResult")],
            |ctx, _params| {
                let result = ctx.get::<CatalogRpcResult>("catalog_result").ok_or("No catalog_result")?;
                match result {
                    CatalogRpcResult::Success(resp) => {
                        if resp.invalid_artifacts.is_empty() {
                            Ok(())
                        } else {
                            Err(format!(
                                "Expected no invalid artifacts but found: {:?}",
                                resp.invalid_artifacts.iter().map(|iv| format!("{}:{}", iv.id, iv.code)).collect::<Vec<_>>()
                            ))
                        }
                    }
                    CatalogRpcResult::Error(e) => Err(format!("Expected success, got error: {}", e)),
                    CatalogRpcResult::Status { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        step_def(
            "a hearth directory with playbook files:",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, params| {
                // Creates a hearth directory from a table of (path, content) pairs.
                // Content may use \n for embedded newlines (same as snapshot fs hearth).
                // Used for engine-level tests that need playbook artifacts with machine.yaml.
                let table = params.data_table().ok_or("Expected a data table")?;
                let path_col = column_index(table, "path")?;
                let content_col = column_index(table, "content")?;

                let (handle, tmp) = retained_temp_dir("anvil-test-engine-hearth-")?;

                for row in &table.rows {
                    let file_path = row[path_col].trim();
                    let content = row[content_col].trim().replace("\\n", "\n");

                    let full = tmp.join(file_path);
                    if let Some(parent) = full.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| format!("Failed to create dir: {}", e))?;
                    }
                    std::fs::write(&full, content)
                        .map_err(|e| format!("Failed to write {}: {}", full.display(), e))?;
                }

                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),

        // ===== Begin RPC create-flow steps (Phase 4 — event routing coverage) =====

        // Calls the begin RPC with artifact_type="track", session_role="creator",
        // parent_id set to the given proposal id, and track_name set. Threads
        // hearth_path through so post-call filesystem assertions can use it.
        async_step_def(
            "the begin RPC is called to create a track named {string} under parent {string}",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[("engine_process", "EngineProcess"), ("begin_result", "BeginRpcResult"), ("hearth_path", "PathBuf")],
            |mut ctx, params| async move {
                let track_name = params.get_string(0).ok_or("Expected track_name")?.to_string();
                let parent_id = params.get_string(1).ok_or("Expected parent_id")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let port = engine.port;

                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::BeginRequest {
                            hearth_path: String::new(),
                            artifact_type: "track".to_string(),
                            parent_id,
                            track_name,
                            playbook_name: String::new(),
                            target_owner: String::new(),
                            approver: "Approver-E2E".to_string(),
                            actor_name: "Rpc-Test-000000".to_string(),
                            actor_type: "agent".to_string(),
                            actor_model: "test".to_string(),
                            actor_provider: "test".to_string(),
                            actor_context_window: 0,
                            actor_sdk_version: String::new(),
                            actor_entrypoint: String::new(),
                            identifier: String::new(),
                            session_role: "creator".to_string(),
                            ctx_org: "Foundation".to_string(),
                            ctx_space: String::new(),
                            ctx_role: "read".to_string(),
                            ctx_clearance: "internal".to_string(),
                        ..Default::default()
                        });
                        match client.begin(request).await {
                            Ok(response) => BeginRpcResult::Success(response.into_inner()),
                            Err(status) => BeginRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => BeginRpcResult::Error { code: "UNAVAILABLE".to_string(), message: format!("Connection failed: {}", e) },
                };

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("begin_result", result);
                out.set("hearth_path", hearth_path);
                Ok(out)
            },
        ),
        // M3 — a create begin carrying a conversation_id must reliably write the
        // resumable open-begin marker onto the freshly-scaffolded artifact.
        async_step_def(
            "the begin RPC is called to create a track named {string} under parent {string} for conversation {string}",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[("engine_process", "EngineProcess"), ("begin_result", "BeginRpcResult"), ("hearth_path", "PathBuf")],
            |mut ctx, params| async move {
                let track_name = params.get_string(0).ok_or("Expected track_name")?.to_string();
                let parent_id = params.get_string(1).ok_or("Expected parent_id")?.to_string();
                let conversation_id = params.get_string(2).ok_or("Expected conversation_id")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let port = engine.port;

                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::BeginRequest {
                            hearth_path: String::new(),
                            artifact_type: "track".to_string(),
                            parent_id,
                            track_name,
                            approver: "Approver-E2E".to_string(),
                            actor_name: "Rpc-Test-000000".to_string(),
                            actor_type: "agent".to_string(),
                            actor_model: "test".to_string(),
                            actor_provider: "test".to_string(),
                            session_role: "creator".to_string(),
                            ctx_org: "Foundation".to_string(),
                            ctx_role: "read".to_string(),
                            ctx_clearance: "internal".to_string(),
                            conversation_id,
                            ..Default::default()
                        });
                        match client.begin(request).await {
                            Ok(response) => BeginRpcResult::Success(response.into_inner()),
                            Err(status) => BeginRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => BeginRpcResult::Error { code: "UNAVAILABLE".to_string(), message: format!("Connection failed: {}", e) },
                };

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("begin_result", result);
                out.set("hearth_path", hearth_path);
                Ok(out)
            },
        ),
        async_step_def(
            "the begin RPC is called to create a track with track_name {string} parent {string} approver {string}",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[("engine_process", "EngineProcess"), ("begin_result", "BeginRpcResult"), ("hearth_path", "PathBuf")],
            |mut ctx, params| async move {
                let track_name = params.get_string(0).ok_or("Expected track_name")?.to_string();
                let parent_id = params.get_string(1).ok_or("Expected parent_id")?.to_string();
                let approver = params.get_string(2).ok_or("Expected approver")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let port = engine.port;

                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::BeginRequest {
                            hearth_path: String::new(),
                            artifact_type: "track".to_string(),
                            parent_id,
                            track_name,
                            playbook_name: String::new(),
                            target_owner: String::new(),
                            approver,
                            actor_name: "Rpc-Test-000000".to_string(),
                            actor_type: "agent".to_string(),
                            actor_model: "test".to_string(),
                            actor_provider: "test".to_string(),
                            actor_context_window: 0,
                            actor_sdk_version: String::new(),
                            actor_entrypoint: String::new(),
                            identifier: String::new(),
                            session_role: "creator".to_string(),
                            ctx_org: "Foundation".to_string(),
                            ctx_space: String::new(),
                            ctx_role: "read".to_string(),
                            ctx_clearance: "internal".to_string(),
                        ..Default::default()
                        });
                        match client.begin(request).await {
                            Ok(response) => BeginRpcResult::Success(response.into_inner()),
                            Err(status) => BeginRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => BeginRpcResult::Error { code: "UNAVAILABLE".to_string(), message: format!("Connection failed: {}", e) },
                };

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("begin_result", result);
                out.set("hearth_path", hearth_path);
                Ok(out)
            },
        ),

        // Reads a file at <hearth_path>/<begin_result.track_path>/<sub_path> and checks it
        // contains the given needle. Used to assert TrackCreation event routing produced the
        // correct on-disk state (e.g. status.yaml).
        check_def(
            "the begin RPC response track_path file {string} contains {string}",
            &[("begin_result", "BeginRpcResult"), ("hearth_path", "PathBuf")],
            |ctx, params| {
                let sub_path = params.get_string(0).ok_or("Expected sub_path")?.to_string();
                let needle = params.get_string(1).ok_or("Expected needle")?.to_string();
                let result = ctx.get::<BeginRpcResult>("begin_result").ok_or("No begin_result")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let track_path = match result {
                    BeginRpcResult::Success(r) => &r.track_path,
                    BeginRpcResult::Error { code, message } => {
                        return Err(format!("Expected success, got gRPC {}: {}", code, message));
                    }
                };
                if track_path.is_empty() {
                    return Err("begin_result track_path is empty".to_string());
                }
                let full_path = hearth.join(track_path).join(&sub_path);
                let content = std::fs::read_to_string(&full_path)
                    .map_err(|e| format!("Failed to read {}: {}", full_path.display(), e))?;
                if content.contains(&needle) {
                    Ok(())
                } else {
                    Err(format!("Expected '{}' in {}, content:\n{}", needle, full_path.display(), content))
                }
            },
        ),
        // Behavior-preservation (A3): a kind whose machine does NOT declare a
        // target_owner descriptor writes NO target_owner line to status.yaml.
        check_def(
            "the begin RPC response track_path file {string} does not contain {string}",
            &[("begin_result", "BeginRpcResult"), ("hearth_path", "PathBuf")],
            |ctx, params| {
                let sub_path = params.get_string(0).ok_or("Expected sub_path")?.to_string();
                let needle = params.get_string(1).ok_or("Expected needle")?.to_string();
                let result = ctx.get::<BeginRpcResult>("begin_result").ok_or("No begin_result")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let track_path = match result {
                    BeginRpcResult::Success(r) => &r.track_path,
                    BeginRpcResult::Error { code, message } => {
                        return Err(format!("Expected success, got gRPC {}: {}", code, message));
                    }
                };
                if track_path.is_empty() {
                    return Err("begin_result track_path is empty".to_string());
                }
                let full_path = hearth.join(track_path).join(&sub_path);
                let content = std::fs::read_to_string(&full_path)
                    .map_err(|e| format!("Failed to read {}: {}", full_path.display(), e))?;
                if content.contains(&needle) {
                    Err(format!("Expected NO '{}' in {}, but found it. Content:\n{}", needle, full_path.display(), content))
                } else {
                    Ok(())
                }
            },
        ),

        // Reads the file at the absolute path stored in begin_result.review_doc_path and
        // checks it contains the given needle. Used to assert ReviewDocCreated event routing
        // produced the correct spec.review.md on disk.
        check_def(
            "the begin RPC response review_doc_path file contains {string}",
            &[("begin_result", "BeginRpcResult")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?.to_string();
                let result = ctx.get::<BeginRpcResult>("begin_result").ok_or("No begin_result")?;
                let review_doc_path = match result {
                    BeginRpcResult::Success(r) => &r.review_doc_path,
                    BeginRpcResult::Error { code, message } => {
                        return Err(format!("Expected success, got gRPC {}: {}", code, message));
                    }
                };
                if review_doc_path.is_empty() {
                    return Err("begin_result review_doc_path is empty".to_string());
                }
                let content = std::fs::read_to_string(review_doc_path)
                    .map_err(|e| format!("Failed to read {}: {}", review_doc_path, e))?;
                if content.contains(&needle) {
                    Ok(())
                } else {
                    Err(format!("Expected '{}' in {}, content:\n{}", needle, review_doc_path, content))
                }
            },
        ),

        // ===== resolved_hearth attribution (N1, spec Req 7) =====
        // The expected value "<hearth>" expands to the spawned hearth fixture's
        // canonical absolute path. Any other value is canonicalized and
        // compared. Proves the engine echoes the hearth it actually operated on.
        check_def(
            "the catalog RPC response resolved_hearth is the hearth {string}",
            &[("catalog_result", "CatalogRpcResult"), ("hearth_path", "PathBuf")],
            |ctx, params| {
                let expected_raw = params.get_string(0).ok_or("Expected value")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let hearth_y = ctx.get::<PathBuf>("hearth_y_path");
                let expected = expected_resolved_hearth(&expected_raw, hearth, hearth_y)?;
                let result = ctx.get::<CatalogRpcResult>("catalog_result").ok_or("No catalog_result")?;
                match result {
                    CatalogRpcResult::Success(resp) => {
                        if canonical_eq(&resp.resolved_hearth, &expected) {
                            Ok(())
                        } else {
                            Err(format!(
                                "Expected resolved_hearth '{}', got '{}'",
                                expected, resp.resolved_hearth
                            ))
                        }
                    }
                    CatalogRpcResult::Error(e) => Err(format!("Expected success, got error: {}", e)),
                    CatalogRpcResult::Status { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the complete RPC response resolved_hearth is the hearth {string}",
            &[("complete_rpc_result", "CompleteRpcResult"), ("hearth_path", "PathBuf")],
            |ctx, params| {
                let expected_raw = params.get_string(0).ok_or("Expected value")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let hearth_y = ctx.get::<PathBuf>("hearth_y_path");
                let expected = expected_resolved_hearth(&expected_raw, hearth, hearth_y)?;
                let r = ctx.get::<CompleteRpcResult>("complete_rpc_result").ok_or("No complete_rpc_result")?;
                match r {
                    CompleteRpcResult::Success(resp) => {
                        if canonical_eq(&resp.resolved_hearth, &expected) {
                            Ok(())
                        } else {
                            Err(format!(
                                "Expected resolved_hearth '{}', got '{}'",
                                expected, resp.resolved_hearth
                            ))
                        }
                    }
                    CompleteRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        // ===== Single-instance bind arbitration (Phase 4, T4.1) =====
        step_def(
            "a second engine is started on the same port",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf"), ("engine_port", "u16")],
            &[("engine_process", "EngineProcess"), ("engine_port", "u16"), ("second_engine_error", "String")],
            |mut ctx, _params| {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let hearth_path = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let port = *ctx.get::<u16>("engine_port").ok_or("No engine_port")?;
                let binary = crate::harness::binary_path("anvil-engine");

                let mut child = anvil_engine_command(&binary)
                    .arg("--hearth")
                    .arg(hearth_path.to_str().unwrap())
                    .arg("--port")
                    .arg(port.to_string())
                    .stdout(Stdio::null())
                    .stderr(Stdio::piped())
                    .spawn()
                    .map_err(|e| {
                        format!(
                            "Failed to start second anvil-engine at {}: {}",
                            binary.display(),
                            e
                        )
                    })?;

                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                loop {
                    match child.try_wait() {
                        Ok(Some(_)) => {
                            let output = child
                                .wait_with_output()
                                .map_err(|e| format!("Failed to collect second engine output: {}", e))?;
                            if output.status.success() {
                                return Err(
                                    "Second engine exited successfully instead of failing bind"
                                        .to_string(),
                                );
                            }
                            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                            let mut out = Context::new();
                            out.set("engine_process", engine);
                            out.set("engine_port", port);
                            out.set("second_engine_error", stderr);
                            return Ok(out);
                        }
                        Ok(None) if std::time::Instant::now() < deadline => {
                            std::thread::sleep(std::time::Duration::from_millis(50));
                        }
                        Ok(None) => {
                            let _ = child.kill();
                            let _ = child.wait();
                            return Err(format!(
                                "Second engine on port {} did not fail within 5 seconds",
                                port
                            ));
                        }
                        Err(e) => {
                            return Err(format!("Failed to poll second engine exit: {}", e));
                        }
                    }
                }
            },
        ),
        check_def(
            "the second engine fails fast because the port is already in use",
            &[("second_engine_error", "String"), ("engine_port", "u16")],
            |ctx, _params| {
                let error = ctx
                    .get::<String>("second_engine_error")
                    .ok_or("No second_engine_error")?;
                let port = *ctx.get::<u16>("engine_port").ok_or("No engine_port")?;
                let lower = error.to_ascii_lowercase();
                if lower.contains("address already in use")
                    || lower.contains("addrinuse")
                    || lower.contains("eaddrinuse")
                    || lower.contains("os error 48")
                    || lower.contains("os error 98")
                {
                    Ok(())
                } else {
                    Err(format!(
                        "Second engine failed, but stderr did not show port {} was already in use: {}",
                        port, error
                    ))
                }
            },
        ),
        // ===== Graceful shutdown (Phase 1, T1.1) =====
        // Send SIGTERM to the running engine's pid. We do NOT wait here; the
        // `exits within N seconds` check polls for exit so the assertion owns
        // the timing bound. The EngineProcess (with its captured exit status)
        // travels forward in context.
        step_def(
            "the engine is sent SIGTERM",
            &[("engine_process", "EngineProcess")],
            &[("engine_process", "EngineProcess"), ("engine_port", "u16")],
            |mut ctx, _params| {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let port = engine.port;
                let pid = engine.child.id() as libc::pid_t;
                let rc = unsafe { libc::kill(pid, libc::SIGTERM) };
                if rc != 0 {
                    return Err(format!("kill(SIGTERM) failed for pid {}", pid));
                }
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("engine_port", port);
                Ok(out)
            },
        ),
        step_def(
            "the engine is sent SIGINT",
            &[("engine_process", "EngineProcess")],
            &[("engine_process", "EngineProcess"), ("engine_port", "u16")],
            |mut ctx, _params| {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let port = engine.port;
                let pid = engine.child.id() as libc::pid_t;
                let rc = unsafe { libc::kill(pid, libc::SIGINT) };
                if rc != 0 {
                    return Err(format!("kill(SIGINT) failed for pid {}", pid));
                }
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("engine_port", port);
                Ok(out)
            },
        ),
        // Poll the child for exit within the bound and assert a CLEAN exit
        // (exit code 0). This is the discriminating assertion: the default
        // disposition of SIGTERM/SIGINT is to *terminate* the process, which
        // yields a signal-killed status (code() == None), NOT a zero exit. Only
        // a `serve_with_shutdown` handler that lets `main` return `Ok(())`
        // produces exit code 0 — so a no-handler engine fails this for the
        // right reason (killed-by-signal, not graceful).
        step_def(
            "the engine exits within {int} seconds",
            &[("engine_process", "EngineProcess")],
            &[("engine_process", "EngineProcess"), ("engine_port", "u16")],
            |mut ctx, params| {
                let secs = params.get_int(0).ok_or("Expected seconds")? as u64;
                let mut engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let port = engine.port;
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
                loop {
                    let status = engine
                        .child
                        .try_wait()
                        .map_err(|e| format!("try_wait failed: {}", e))?;
                    if let Some(st) = status {
                        if st.success() {
                            let mut out = Context::new();
                            out.set("engine_process", engine);
                            out.set("engine_port", port);
                            return Ok(out);
                        }
                        return Err(format!(
                            "Engine exited but NOT cleanly (code: {:?}) — a signal-killed \
                             process is not a graceful shutdown",
                            st.code()
                        ));
                    }
                    if std::time::Instant::now() >= deadline {
                        return Err(format!(
                            "Engine did not exit within {} seconds of the signal",
                            secs
                        ));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            },
        ),
        // After a clean shutdown the listening socket must be released so a
        // fresh engine (or anything) can bind the same port. Poll-bind with a
        // short retry to absorb kernel TIME_WAIT/close latency.
        check_def(
            "the engine port can be re-bound",
            &[("engine_port", "u16")],
            |ctx, _params| {
                let port = *ctx.get::<u16>("engine_port").ok_or("No engine_port")?;
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                loop {
                    match std::net::TcpListener::bind(("127.0.0.1", port)) {
                        Ok(l) => {
                            drop(l);
                            return Ok(());
                        }
                        Err(e) => {
                            if std::time::Instant::now() >= deadline {
                                return Err(format!(
                                    "Port {} could not be re-bound after shutdown: {}",
                                    port, e
                                ));
                            }
                            std::thread::sleep(std::time::Duration::from_millis(50));
                        }
                    }
                }
            },
        ),
        // ===== Fixed-port endpoint (Phase 1, T1.2) =====
        // The single existing listener must answer a gRPC HealthCheck (the
        // shim's dial shape).
        async_step_def(
            "a gRPC HealthCheck on the engine port succeeds",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
                ("health_check_response", "HealthCheckRpcResponse"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let port = engine.port;
                let addr = format!("http://127.0.0.1:{}", port);
                let response = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => client
                        .health_check(crate::surfaced(anvil_engine::proto::HealthCheckRequest {}))
                        .await
                        .map(|r| r.into_inner())
                        .map_err(|e| format!("gRPC HealthCheck on port {} failed: {}", port, e))?,
                    Err(e) => return Err(format!("gRPC HealthCheck connect on port {} failed: {}", port, e)),
                };
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("engine_port", port);
                out.set("health_check_response", HealthCheckRpcResponse {
                    wire_proto_version: response.wire_proto_version,
                    build_version: response.build_version,
                });
                Ok(out)
            },
        ),
        async_step_def(
            "the gRPC HealthCheck is requested on the engine port",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
                ("health_check_response", "HealthCheckRpcResponse"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let port = engine.port;
                let addr = format!("http://127.0.0.1:{}", port);
                let response = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => client
                        .health_check(crate::surfaced(anvil_engine::proto::HealthCheckRequest {}))
                        .await
                        .map(|r| r.into_inner())
                        .map_err(|e| format!("gRPC HealthCheck on port {} failed: {}", port, e))?,
                    Err(e) => return Err(format!("gRPC HealthCheck connect on port {} failed: {}", port, e)),
                };
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("engine_port", port);
                out.set("health_check_response", HealthCheckRpcResponse {
                    wire_proto_version: response.wire_proto_version,
                    build_version: response.build_version,
                });
                Ok(out)
            },
        ),
        check_def(
            "the HealthCheck response reports the current wire protocol version",
            &[("health_check_response", "HealthCheckRpcResponse")],
            |ctx, _params| {
                let response = ctx
                    .get::<HealthCheckRpcResponse>("health_check_response")
                    .ok_or("No health_check_response")?;
                if response.wire_proto_version != anvil_engine::WIRE_PROTO_VERSION {
                    return Err(format!(
                        "Expected HealthCheck wire_proto_version {}, got {}",
                        anvil_engine::WIRE_PROTO_VERSION,
                        response.wire_proto_version
                    ));
                }
                if response.wire_proto_version == 0 {
                    return Err("HealthCheck wire_proto_version must be >= 1".to_string());
                }
                Ok(())
            },
        ),
        check_def(
            "the HealthCheck response build_version is non-empty",
            &[("health_check_response", "HealthCheckRpcResponse")],
            |ctx, _params| {
                let response = ctx
                    .get::<HealthCheckRpcResponse>("health_check_response")
                    .ok_or("No health_check_response")?;
                if response.build_version.trim().is_empty() {
                    return Err("HealthCheck build_version was empty".to_string());
                }
                Ok(())
            },
        ),
        // The SAME listener must also accept a raw TCP connection (the shape
        // Foundry's health probe uses — TCP-connect, not HTTP GET).
        check_def(
            "a raw TCP connection to the engine port succeeds",
            &[("engine_port", "u16")],
            |ctx, _params| {
                let port = *ctx.get::<u16>("engine_port").ok_or("No engine_port")?;
                let addr: std::net::SocketAddr = ([127, 0, 0, 1], port).into();
                std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(2))
                    .map(|_| ())
                    .map_err(|e| format!("Raw TCP connect to port {} failed: {}", port, e))
            },
        ),

        // ===== R1 — Foundry-mode refusal gatekeeper (spec Req 5/6/1) =====
        //
        // These steps spawn the REAL anvil-engine binary with the Foundry
        // environment set (FOUNDRY_SESSION_TOKEN + FOUNDRY_BROKER_SOCKET), so
        // the engine's startup mode-detection (D1) selects Foundry mode from
        // its own env. The token verifier is selected via the test-only,
        // debug-assertions-gated env switch ANVIL_TEST_SESSION_VERIFIER so the
        // reject paths are hermetic — no live broker is required (D4). The RPC
        // call steps attach `authorization: Bearer <jwt>` gRPC metadata so the
        // engine's `authorize()` actually sees a per-request credential to
        // verify. No existing engine step sets Foundry env or attaches auth
        // metadata, so these are required for the refusal scenarios to
        // genuinely exercise the gatekeeper (no false-green).

        // Spawn the engine in Foundry mode with a stub verifier that REJECTS
        // every presented token (hermetic reject path). `FOUNDRY_BROKER_SOCKET`
        // is set to a bogus path; the stub means it is never dialed.
        step_def(
            "the engine is started in Foundry mode with a rejecting verifier",
            &[],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("engine_port", "u16"),
            ],
            |_ctx, _params| {
                let (process, hearth, handle, port) = spawn_foundry_engine(
                    "test-session-token",
                    Some("stub_reject"),
                )?;
                let mut out = Context::new();
                out.set("engine_process", process);
                out.set("hearth_path", hearth);
                out.set("hearth_path_handle", handle);
                out.set("engine_port", port);
                Ok(out)
            },
        ),

        // Spawn the engine in Foundry mode with a stub verifier that simulates
        // the broker being UNREACHABLE on every lookup (returns KeyFetch). Per
        // spec Req 6, a broker-unreachable cache miss is a refusal, not a
        // bypass — fail-closed, driven against the real spawned engine.
        step_def(
            "the engine is started in Foundry mode with an unreachable broker",
            &[],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("engine_port", "u16"),
            ],
            |_ctx, _params| {
                let (process, hearth, handle, port) = spawn_foundry_engine(
                    "test-session-token",
                    Some("stub_unreachable"),
                )?;
                let mut out = Context::new();
                out.set("engine_process", process);
                out.set("hearth_path", hearth);
                out.set("hearth_path_handle", handle);
                out.set("engine_port", port);
                Ok(out)
            },
        ),

        // Spawn the engine in Foundry mode with a WHITESPACE-ONLY session
        // token. Per spec Req 1, the engine's mode detection trims the token
        // and treats whitespace-only as absent ⇒ Standalone ⇒ calls served
        // with no bearer required. No verifier is consulted.
        step_def(
            "the engine is started in Foundry mode with a whitespace-only session token",
            &[],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("engine_port", "u16"),
            ],
            |_ctx, _params| {
                // A whitespace-only at-spawn token. The stub is set to REJECT so
                // that, IF the engine were to (wrongly) treat whitespace as a
                // present token and verify it, the call would be refused — making
                // the "served" assertion a genuine proof of the trim rule.
                let (process, hearth, handle, port) = spawn_foundry_engine(
                    "   ",
                    Some("stub_reject"),
                )?;
                let mut out = Context::new();
                out.set("engine_process", process);
                out.set("hearth_path", hearth);
                out.set("hearth_path_handle", handle);
                out.set("engine_port", port);
                Ok(out)
            },
        ),

        // Call the catalog RPC attaching `authorization: Bearer <jwt>` metadata.
        async_step_def(
            "the catalog RPC is called with bearer token {string}",
            &[("engine_process", "EngineProcess")],
            &[("catalog_result", "CatalogRpcResult"), ("engine_process", "EngineProcess")],
            |mut ctx, params| async move {
                let token = params.get_string(0).ok_or("Expected bearer token")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let result = call_catalog_with_bearer(&engine, Some(&token)).await;
                let mut out = Context::new();
                out.set("catalog_result", result);
                out.set("engine_process", engine);
                Ok(out)
            },
        ),

        // Call the catalog RPC with NO authorization metadata at all.
        async_step_def(
            "the catalog RPC is called with no bearer token",
            &[("engine_process", "EngineProcess")],
            &[("catalog_result", "CatalogRpcResult"), ("engine_process", "EngineProcess")],
            |mut ctx, _params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let result = call_catalog_with_bearer(&engine, None).await;
                let mut out = Context::new();
                out.set("catalog_result", result);
                out.set("engine_process", engine);
                Ok(out)
            },
        ),

        // Call the catalog RPC with an EMPTY hearth_path (the self-heal path):
        // against a hearth-less engine this yields a typed precondition error
        // carrying the guidance, instead of silently defaulting attribution.
        async_step_def(
            "the catalog RPC is called with no hearth_path",
            &[("engine_process", "EngineProcess")],
            &[("catalog_result", "CatalogRpcResult"), ("engine_process", "EngineProcess")],
            |mut ctx, _params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let result = call_catalog_with_hearth(&engine, String::new()).await;
                let mut out = Context::new();
                out.set("catalog_result", result);
                out.set("engine_process", engine);
                Ok(out)
            },
        ),

        // Assert the catalog RPC was refused with the given gRPC status code
        // (e.g. UNAUTHENTICATED, spec Req 5).
        check_def(
            "the catalog RPC returns gRPC status {string}",
            &[("catalog_result", "CatalogRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected status code")?.to_string();
                let result = ctx.get::<CatalogRpcResult>("catalog_result").ok_or("No catalog_result")?;
                match result {
                    CatalogRpcResult::Status { code, .. } if *code == expected => Ok(()),
                    CatalogRpcResult::Status { code, message } => {
                        Err(format!("Expected gRPC {}, got {} ({})", expected, code, message))
                    }
                    CatalogRpcResult::Success(_) => {
                        Err(format!("Expected gRPC {} error, got success", expected))
                    }
                    CatalogRpcResult::Error(e) => {
                        Err(format!("Expected gRPC {}, got transport error: {}", expected, e))
                    }
                }
            },
        ),

        // Assert the catalog RPC error message contains the given text (e.g.
        // `not_authenticated`).
        check_def(
            "the catalog RPC error message contains {string}",
            &[("catalog_result", "CatalogRpcResult")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected text")?.to_string();
                let result = ctx.get::<CatalogRpcResult>("catalog_result").ok_or("No catalog_result")?;
                match result {
                    CatalogRpcResult::Status { message, .. } if message.contains(&needle) => Ok(()),
                    CatalogRpcResult::Status { message, .. } => {
                        Err(format!("Error message '{}' doesn't contain '{}'", message, needle))
                    }
                    CatalogRpcResult::Success(_) => Err("Expected error, got success".to_string()),
                    CatalogRpcResult::Error(e) => {
                        Err(format!("Expected gRPC error, got transport error: {}", e))
                    }
                }
            },
        ),

        // Assert the catalog RPC succeeded (used for whitespace⇒standalone: a
        // served call proves the engine did NOT refuse).
        check_def(
            "the catalog RPC succeeds",
            &[("catalog_result", "CatalogRpcResult")],
            |ctx, _params| {
                let result = ctx.get::<CatalogRpcResult>("catalog_result").ok_or("No catalog_result")?;
                match result {
                    CatalogRpcResult::Success(_) => Ok(()),
                    CatalogRpcResult::Status { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                    CatalogRpcResult::Error(e) => {
                        Err(format!("Expected success, got transport error: {}", e))
                    }
                }
            },
        ),

        // ===== R2 — Principal binds to `sub` (spec Req 7) =====
        //
        // These steps spawn the REAL anvil-engine in Foundry mode with an
        // ACCEPTING stub verifier (ANVIL_TEST_SESSION_VERIFIER=stub_accept +
        // ANVIL_TEST_SESSION_SUB=<sub>), so a presented bearer verifies to a
        // canned VerifiedSession carrying that sub. They then drive begin /
        // snapshot / complete carrying a DIVERGENT caller actor_name and read
        // back the persisted status.yaml transition `actor`, proving the
        // engine binds the principal to the sub-derived value and never to the
        // self-asserted caller string.

        // Spawn the engine in Foundry mode with an accepting stub verifier
        // bound to the given sub, seeding a hearth that contains a track in
        // spec_review state (ready for a snapshot/complete transition).
        step_def(
            "the engine is started in Foundry mode with an accepting verifier for sub {string}",
            &[],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("engine_port", "u16"),
            ],
            |_ctx, params| {
                let sub = params.get_string(0).ok_or("Expected sub")?.to_string();
                let (process, hearth, handle, port) = spawn_foundry_engine_accept(&sub)?;
                seed_principal_binding_hearth(&hearth)?;
                let mut out = Context::new();
                out.set("engine_process", process);
                out.set("hearth_path", hearth);
                out.set("hearth_path_handle", handle);
                out.set("engine_port", port);
                Ok(out)
            },
        ),

        // Standalone (no Foundry env) engine over the same seeded hearth, for
        // the Req-3 no-regression scenario (caller actor_name persisted verbatim).
        step_def(
            "the engine is started in standalone mode over a principal-binding hearth",
            &[],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("engine_port", "u16"),
            ],
            |_ctx, _params| {
                let (process, hearth, handle, port) = spawn_standalone_engine()?;
                seed_principal_binding_hearth(&hearth)?;
                let mut out = Context::new();
                out.set("engine_process", process);
                out.set("hearth_path", hearth);
                out.set("hearth_path_handle", handle);
                out.set("engine_port", port);
                Ok(out)
            },
        ),

        // Call the begin RPC (create a track under the seeded proposal) with an
        // optional bearer and a caller-supplied actor_name. Threads hearth_path.
        async_step_def(
            "the begin RPC is called with bearer token {string} and actor_name {string} to create track {string} under parent {string}",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[("engine_process", "EngineProcess"), ("begin_result", "BeginRpcResult"), ("hearth_path", "PathBuf")],
            |mut ctx, params| async move {
                let bearer = params.get_string(0).ok_or("Expected bearer")?.to_string();
                let actor_name = params.get_string(1).ok_or("Expected actor_name")?.to_string();
                let track_name = params.get_string(2).ok_or("Expected track_name")?.to_string();
                let parent_id = params.get_string(3).ok_or("Expected parent_id")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let bearer_opt = if bearer.is_empty() { None } else { Some(bearer) };
                let result = call_begin_create_with_bearer(
                    &engine, bearer_opt.as_deref(), &actor_name, &track_name, &parent_id,
                ).await;
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("begin_result", result);
                out.set("hearth_path", hearth_path);
                Ok(out)
            },
        ),

        // Call the snapshot RPC with an optional bearer and a caller-supplied
        // actor_name, transitioning the seeded track. Threads hearth_path so the
        // persisted status.yaml can be read back.
        async_step_def(
            "the snapshot RPC is called with bearer token {string} and actor_name {string} on artifact {string} to state {string}",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[("engine_process", "EngineProcess"), ("snapshot_rpc_result", "SnapshotRpcResult"), ("hearth_path", "PathBuf")],
            |mut ctx, params| async move {
                let bearer = params.get_string(0).ok_or("Expected bearer")?.to_string();
                let actor_name = params.get_string(1).ok_or("Expected actor_name")?.to_string();
                let artifact_path = params.get_string(2).ok_or("Expected artifact_path")?.to_string();
                let to_state = params.get_string(3).ok_or("Expected to_state")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let bearer_opt = if bearer.is_empty() { None } else { Some(bearer) };
                let result = call_snapshot_with_bearer(
                    &engine, bearer_opt.as_deref(), &actor_name, &artifact_path, &to_state,
                ).await;
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("snapshot_rpc_result", result);
                out.set("hearth_path", hearth_path);
                Ok(out)
            },
        ),

        // Call the complete RPC with an optional bearer and a caller-supplied
        // actor_name. Threads hearth_path for status.yaml read-back.
        async_step_def(
            "the complete RPC is called with bearer token {string} and actor_name {string} on artifact {string} with satisfaction {string}",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[("engine_process", "EngineProcess"), ("complete_rpc_result", "CompleteRpcResult"), ("hearth_path", "PathBuf")],
            |mut ctx, params| async move {
                let bearer = params.get_string(0).ok_or("Expected bearer")?.to_string();
                let actor_name = params.get_string(1).ok_or("Expected actor_name")?.to_string();
                let artifact_path = params.get_string(2).ok_or("Expected artifact_path")?.to_string();
                let satisfaction = params.get_string(3).ok_or("Expected satisfaction")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let bearer_opt = if bearer.is_empty() { None } else { Some(bearer) };
                let result = call_complete_with_bearer(
                    &engine, bearer_opt.as_deref(), &actor_name, &artifact_path, &satisfaction,
                ).await;
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("complete_rpc_result", result);
                out.set("hearth_path", hearth_path);
                Ok(out)
            },
        ),

        // Read the persisted status.yaml at <hearth>/<artifact_path> and assert
        // the most recent transition `actor:` equals the expected principal.
        // This is the load-bearing Req-7 read-back: it inspects the PERSISTED
        // event log, not the RPC response.
        check_def(
            "the persisted transition actor in {string} is {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let artifact_path = params.get_string(0).ok_or("Expected artifact_path")?.to_string();
                let expected = params.get_string(1).ok_or("Expected actor")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let actor = last_transition_actor(hearth, &artifact_path)?;
                if actor == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected persisted transition actor '{}', got '{}'",
                        expected, actor
                    ))
                }
            },
        ),

        // begin-specific read-back: the created track directory is timestamped
        // (tracks/<ts>_<snake_name>), so its path is not knowable in the feature
        // — read it from the begin RPC response's track_path and inspect the
        // persisted status.yaml transition actor there.
        check_def(
            "the begin RPC response track_path status.yaml transition actor is {string}",
            &[("begin_result", "BeginRpcResult"), ("hearth_path", "PathBuf")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected actor")?.to_string();
                let result = ctx.get::<BeginRpcResult>("begin_result").ok_or("No begin_result")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let track_path = match result {
                    BeginRpcResult::Success(r) => r.track_path.clone(),
                    BeginRpcResult::Error { code, message } => {
                        return Err(format!("Expected success, got gRPC {}: {}", code, message));
                    }
                };
                if track_path.is_empty() {
                    return Err("begin_result track_path is empty".to_string());
                }
                let actor = last_transition_actor(hearth, &track_path)?;
                if actor == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected persisted transition actor '{}', got '{}'",
                        expected, actor
                    ))
                }
            },
        ),

        check_def(
            "the begin RPC response track_path status.yaml transition actor is not {string}",
            &[("begin_result", "BeginRpcResult"), ("hearth_path", "PathBuf")],
            |ctx, params| {
                let forbidden = params.get_string(0).ok_or("Expected actor")?.to_string();
                let result = ctx.get::<BeginRpcResult>("begin_result").ok_or("No begin_result")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let track_path = match result {
                    BeginRpcResult::Success(r) => r.track_path.clone(),
                    BeginRpcResult::Error { code, message } => {
                        return Err(format!("Expected success, got gRPC {}: {}", code, message));
                    }
                };
                let actor = last_transition_actor(hearth, &track_path)?;
                if actor == forbidden {
                    Err(format!(
                        "Self-asserted actor '{}' was persisted as the principal — laundering gap",
                        forbidden
                    ))
                } else {
                    Ok(())
                }
            },
        ),

        // Negative read-back: assert the self-asserted caller string is NEVER
        // present in the persisted status.yaml transition actor line.
        check_def(
            "the persisted transition actor in {string} is not {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let artifact_path = params.get_string(0).ok_or("Expected artifact_path")?.to_string();
                let forbidden = params.get_string(1).ok_or("Expected actor")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let actor = last_transition_actor(hearth, &artifact_path)?;
                if actor == forbidden {
                    Err(format!(
                        "Self-asserted actor '{}' was persisted as the principal — laundering gap",
                        forbidden
                    ))
                } else {
                    Ok(())
                }
            },
        ),

        // ===== BeginAdoptionStatus RPC step defs (BP3) =====
        // Pure-read RPC: no mutation, no write lock. The When step calls the RPC
        // and stores the result; the Then step asserts on has_open_begin.
        async_step_def(
            "the begin_adoption_status RPC is called with:",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("begin_adoption_status_result", "BeginAdoptionStatusRpcResult"),
            ],
            |mut ctx, params| async move {
                let table = params
                    .data_table()
                    .ok_or("Expected data table for begin_adoption_status")?;
                let mut actor_name = String::new();
                let mut artifact_path = String::new();
                let mut state = String::new();
                // Build key-value pairs from BOTH headers (first row) AND
                // subsequent rows — brine treats the first table row as
                // `table.headers` and remaining rows as `table.rows`.
                // Mirroring the pattern in the complete-RPC step.
                let mut pairs: Vec<(String, String)> = Vec::new();
                if table.headers.len() >= 2 {
                    pairs.push((table.headers[0].trim().to_string(), table.headers[1].trim().to_string()));
                }
                for row in &table.rows {
                    if row.len() >= 2 {
                        pairs.push((row[0].trim().to_string(), row[1].trim().to_string()));
                    }
                }
                for (key, val) in pairs {
                    match key.as_str() {
                        "actor_name" => actor_name = val,
                        "artifact_path" => artifact_path = val,
                        "state" => state = val,
                        _ => {}
                    }
                }
                let hearth = ctx
                    .take::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?;
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let port = engine.port;

                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(
                            anvil_engine::proto::BeginAdoptionStatusRequest {
                                hearth_path: String::new(),
                                actor_name,
                                artifact_path,
                                state,
                            },
                        );
                        match client.begin_adoption_status(request).await {
                            Ok(response) => {
                                BeginAdoptionStatusRpcResult::Success(response.into_inner())
                            }
                            Err(status) => BeginAdoptionStatusRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => BeginAdoptionStatusRpcResult::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth);
                out.set("begin_adoption_status_result", result);
                Ok(out)
            },
        ),
        check_def(
            "the begin_adoption_status RPC response has_open_begin is {string}",
            &[("begin_adoption_status_result", "BeginAdoptionStatusRpcResult")],
            |ctx, params| {
                let expected_str = params
                    .get_string(0)
                    .ok_or("Expected true or false")?;
                let expected = match expected_str {
                    "true" => true,
                    "false" => false,
                    other => return Err(format!("Expected 'true' or 'false', got '{}'", other)),
                };
                let r = ctx
                    .get::<BeginAdoptionStatusRpcResult>("begin_adoption_status_result")
                    .ok_or("No begin_adoption_status_result")?;
                match r {
                    BeginAdoptionStatusRpcResult::Success(resp) => {
                        if resp.has_open_begin == expected {
                            Ok(())
                        } else {
                            Err(format!(
                                "Expected has_open_begin={}, got {}",
                                expected, resp.has_open_begin
                            ))
                        }
                    }
                    BeginAdoptionStatusRpcResult::Error { code, message } => Err(format!(
                        "Expected success, got gRPC {}: {}",
                        code, message
                    )),
                }
            },
        ),
        // ===== Amend RPC (B5b BP3/BP4) =====
        async_step_def(
            "the amend RPC is called with:",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("amend_rpc_result", "AmendRpcResult"),
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("global_playbooks_hearth_path", "PathBuf"),
            ],
            |mut ctx, params| async move {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let hearth_path = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let global = ctx.get::<PathBuf>("global_playbooks_hearth_path").cloned();
                let port = engine.port;
                let table = params.data_table().ok_or("Expected data table")?;

                let mut artifact_path = String::new();
                let mut kind = String::new();
                let mut target_document = String::new();
                let mut target_id = String::new();
                let mut op_kind = String::new();
                let mut body = String::new();
                let mut new_kind = String::new();
                let mut anchor = String::new();
                let mut actor_name = String::new();
                let mut actor_type = String::new();
                let mut actor_model = String::new();
                let mut actor_provider = String::new();
                let mut actor_context_window: i64 = 0;
                let mut actor_sdk_version = String::new();
                let mut actor_entrypoint = String::new();
                let mut amend_hearth_path = String::new();

                let mut pairs: Vec<(String, String)> = Vec::new();
                if table.headers.len() >= 2 {
                    pairs.push((
                        table.headers[0].trim().to_string(),
                        table.headers[1].trim().to_string(),
                    ));
                }
                for row in &table.rows {
                    if row.len() >= 2 {
                        pairs.push((row[0].trim().to_string(), row[1].trim().to_string()));
                    }
                }
                for (key, value) in pairs {
                    match key.as_str() {
                        "artifact_path" => artifact_path = value,
                        "kind" => kind = value,
                        "target_document" => target_document = value,
                        "target_id" => target_id = value,
                        "op_kind" => op_kind = value,
                        "body" => body = value,
                        "new_kind" => new_kind = value,
                        "anchor" => anchor = value,
                        "actor_name" => actor_name = value,
                        "actor_type" => actor_type = value,
                        "actor_model" => actor_model = value,
                        "actor_provider" => actor_provider = value,
                        "actor_context_window" => {
                            actor_context_window = value.parse().unwrap_or(0)
                        }
                        "actor_sdk_version" => actor_sdk_version = value,
                        "actor_entrypoint" => actor_entrypoint = value,
                        "hearth_path" => {
                            amend_hearth_path = match value.as_str() {
                                "<hearth>" | "<hearth_x>" => {
                                    hearth_path.to_string_lossy().into_owned()
                                }
                                other => other.to_string(),
                            };
                        }
                        other => return Err(format!("Unknown amend RPC field: '{}'", other)),
                    }
                }

                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::AmendRequest {
                            hearth_path: amend_hearth_path,
                            artifact_path,
                            kind,
                            target_document,
                            target_id,
                            op_kind,
                            body,
                            new_kind,
                            anchor,
                            actor_name,
                            actor_type,
                            actor_model,
                            actor_provider,
                            actor_context_window,
                            actor_sdk_version,
                            actor_entrypoint,
                        });
                        match client.amend(request).await {
                            Ok(response) => AmendRpcResult::Success(response.into_inner()),
                            Err(status) => AmendRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => AmendRpcResult::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };

                let mut out = Context::new();
                out.set("amend_rpc_result", result);
                out.set("engine_process", engine);
                out.set("hearth_path", hearth_path);
                if let Some(global) = global {
                    out.set("global_playbooks_hearth_path", global);
                }
                Ok(out)
            },
        ),
        check_def(
            "the amend RPC response op_id matches {string}",
            &[("amend_rpc_result", "AmendRpcResult")],
            |ctx, params| {
                let pattern = params.get_string(0).ok_or("Expected pattern")?.to_string();
                let r = ctx
                    .get::<AmendRpcResult>("amend_rpc_result")
                    .ok_or("No amend_rpc_result")?;
                match r {
                    AmendRpcResult::Success(resp) => {
                        use crate::snapshot::SimpleRegex;
                        if SimpleRegex::matches(&pattern, &resp.op_id) {
                            Ok(())
                        } else {
                            Err(format!(
                                "op_id '{}' does not match '{}'",
                                resp.op_id, pattern
                            ))
                        }
                    }
                    AmendRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the amend RPC response new_state is {string}",
            &[("amend_rpc_result", "AmendRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).unwrap_or("").to_string();
                let r = ctx
                    .get::<AmendRpcResult>("amend_rpc_result")
                    .ok_or("No amend_rpc_result")?;
                match r {
                    AmendRpcResult::Success(resp) if resp.new_state == expected => Ok(()),
                    AmendRpcResult::Success(resp) => Err(format!(
                        "Expected new_state '{}', got '{}'",
                        expected, resp.new_state
                    )),
                    AmendRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the amend RPC returns gRPC status {string}",
            &[("amend_rpc_result", "AmendRpcResult")],
            |ctx, params| {
                let expected_code = params.get_string(0).ok_or("Missing status code")?.to_string();
                let r = ctx
                    .get::<AmendRpcResult>("amend_rpc_result")
                    .ok_or("No amend_rpc_result")?;
                match r {
                    AmendRpcResult::Error { code, .. } if *code == expected_code => Ok(()),
                    AmendRpcResult::Error { code, message } => {
                        if code.contains(&expected_code) {
                            Ok(())
                        } else {
                            Err(format!(
                                "Expected gRPC {}, got {} ({})",
                                expected_code, code, message
                            ))
                        }
                    }
                    AmendRpcResult::Success(_) => {
                        Err(format!("Expected gRPC {} error, got success", expected_code))
                    }
                }
            },
        ),
        check_def(
            "the amend RPC error message contains {string}",
            &[("amend_rpc_result", "AmendRpcResult")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected text")?;
                let r = ctx
                    .get::<AmendRpcResult>("amend_rpc_result")
                    .ok_or("No amend_rpc_result")?;
                match r {
                    AmendRpcResult::Error { message, .. } if message.contains(needle) => Ok(()),
                    AmendRpcResult::Error { message, .. } => Err(format!(
                        "Error message '{}' doesn't contain '{}'",
                        message, needle
                    )),
                    AmendRpcResult::Success(_) => Err("Expected error, got success".to_string()),
                }
            },
        ),
        // ===== PersistPlaybook RPC (track 1a BP3/BP4) =====
        // A SEPARATE temp owner-home, independent of the engine hearth. Brine
        // data-table values are static strings, so the absolute owner-home path
        // is created here and threaded via context, not via the table.
        step_def(
            "a separate temp owner-home directory",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("persist_owner_home", "PathBuf"),
                ("persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |mut ctx, _params| {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let hearth_path = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let dir = tempfile::TempDir::new()
                    .map_err(|e| format!("temp owner-home: {}", e))?;
                let path = dir.path().to_path_buf();
                let handle: Arc<Mutex<Option<tempfile::TempDir>>> =
                    Arc::new(Mutex::new(Some(dir)));
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth_path);
                out.set("persist_owner_home", path);
                out.set("persist_owner_home_handle", handle);
                Ok(out)
            },
        ),
        // Records the count of entries in the engine hearth's workflows/ dir
        // (A4): the headline boundary — persisting to the owner-home must not
        // add an entry under the engine hearth.
        step_def(
            "the engine hearth playbooks entry count is recorded",
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("persist_owner_home", "PathBuf"),
                ("persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("persist_owner_home", "PathBuf"),
                ("persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("persist_hearth_playbooks_before", "usize"),
            ],
            |mut ctx, _params| {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let hearth_path = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let owner_home = ctx
                    .get::<PathBuf>("persist_owner_home")
                    .ok_or("No persist_owner_home")?
                    .clone();
                let handle = ctx
                    .get::<Arc<Mutex<Option<tempfile::TempDir>>>>("persist_owner_home_handle")
                    .ok_or("No persist_owner_home_handle")?
                    .clone();
                let count = count_dir_entries(&hearth_path.join("playbooks"));
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth_path);
                out.set("persist_owner_home", owner_home);
                out.set("persist_owner_home_handle", handle);
                out.set("persist_hearth_playbooks_before", count);
                Ok(out)
            },
        ),
        // C-d.1 round 4 (MEDIUM-3 i). The `HearthRegistrationBlocked` ->
        // FAILED_PRECONDITION mapping lived in ONE production line and in one
        // sentence of the record: no engine and no MCP feature mentioned either
        // the variant or its code, so the newly-introduced RPC status rested on
        // reading. This fixture puts a shadow-only pair of hearth roots under the
        // TARGET owner-home so the RPC has to answer for it.
        step_def(
            "that owner-home carries definitions under BOTH hearth roots",
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("persist_owner_home", "PathBuf"),
                ("persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("persist_owner_home", "PathBuf"),
                ("persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |mut ctx, _params| {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let hearth_path = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let owner_home = ctx
                    .get::<PathBuf>("persist_owner_home")
                    .ok_or("No persist_owner_home")?
                    .clone();
                let handle = ctx
                    .get::<Arc<Mutex<Option<tempfile::TempDir>>>>("persist_owner_home_handle")
                    .ok_or("No persist_owner_home_handle")?
                    .clone();
                // Zero overlap: only SHADOWING can be what blocks.
                for (root, kind) in [
                    ("workflows", "legacy_only_seeded_kind"),
                    ("playbooks", "canonical_only_seeded_kind"),
                ] {
                    let d = owner_home.join(root).join(kind);
                    std::fs::create_dir_all(d.join("hooks"))
                        .map_err(|e| format!("create dir: {}", e))?;
                    std::fs::write(
                        d.join("machine.yaml"),
                        crate::conformant_machine_yaml(kind),
                    )
                    .map_err(|e| format!("write seed machine: {}", e))?;
                    std::fs::write(d.join("hooks").join("intent.md"), "Persist intent hook body.")
                        .map_err(|e| format!("write seed hook: {}", e))?;
                }
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth_path);
                out.set("persist_owner_home", owner_home);
                out.set("persist_owner_home_handle", handle);
                Ok(out)
            },
        ),
        // DELIBERATELY ABSENT, and stated so it is not later added by reflex: an
        // assertion that "neither hearth root was modified" after this refusal
        // CANNOT FAIL. On every state the guard refuses, nothing would have moved
        // anyway — `migrate_legacy_hearth_dir` refuses the same states — so the
        // check is green under the guarded and the unguarded implementation
        // alike. The engine's pre-construction guard is structural (the mutating
        // constructor is never reached on a refusing hearth) and has no
        // behavioral delta a scenario can observe. See implementation-c.md §37.5.
        async_step_def(
            "the PersistPlaybook RPC is called for kind {string} under that owner-home with:",
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("persist_owner_home", "PathBuf"),
                ("persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                ("persist_rpc_result", "PersistPlaybookRpcResult"),
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("persist_owner_home", "PathBuf"),
                ("persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
                // Optional pass-through: only the BP4 boundary scenario records it
                // upstream; declared here so retain_keys keeps it for the
                // "entry count is unchanged" check after this Map step.
                ("persist_hearth_playbooks_before", "usize"),
            ],
            |mut ctx, params| async move {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let owner_home = ctx
                    .get::<PathBuf>("persist_owner_home")
                    .ok_or("No persist_owner_home")?
                    .clone();
                let handle = ctx
                    .get::<Arc<Mutex<Option<tempfile::TempDir>>>>("persist_owner_home_handle")
                    .ok_or("No persist_owner_home_handle")?
                    .clone();
                // Carry the recorded engine-hearth entry count through when the
                // boundary scenario (BP4) set it; absent for the BP3 scenarios.
                let playbooks_before = ctx.get::<usize>("persist_hearth_playbooks_before").copied();
                let table = params.data_table().ok_or("Expected data table")?;

                // Default actor identity; overridable via the data table so the
                // authorize/identity scenarios can blank actor_name.
                let mut actor_name = "Persist-Doer-700001".to_string();
                let mut actor_type = "agent".to_string();
                let mut actor_model = "claude-opus-4-8".to_string();
                let mut actor_provider = "anthropic".to_string();
                // The direct persist RPC is the enforcing WRITE boundary: the
                // default machine must be conformant (outcome_predicate + a hook on
                // its non-terminal state) and carry that hook, or the RPC refuses
                // it. Scenarios override `machine` for the negative paths.
                let mut machine_yaml = crate::conformant_machine_yaml(&kind);
                // Hook files carried with the request (filename, content). The
                // conformant default references `intent.md`; it is auto-carried
                // below unless a scenario supplies its own hook rows.
                let mut hooks: Vec<(String, String)> = Vec::new();

                let mut pairs: Vec<(String, String)> = Vec::new();
                if table.headers.len() >= 2 {
                    pairs.push((
                        table.headers[0].trim().to_string(),
                        table.headers[1].trim().to_string(),
                    ));
                }
                for row in &table.rows {
                    if row.len() >= 2 {
                        pairs.push((row[0].trim().to_string(), row[1].trim().to_string()));
                    }
                }
                for (key, value) in pairs {
                    match key.as_str() {
                        "actor_name" => actor_name = value,
                        "actor_type" => actor_type = value,
                        "actor_model" => actor_model = value,
                        "actor_provider" => actor_provider = value,
                        // "loader_invalid" → swap in a loader-invalid machine.yaml.
                        "machine" if value == "loader_invalid" => {
                            machine_yaml = crate::loader_invalid_machine_yaml(&kind)
                        }
                        "machine" if value == "minimal" => {
                            machine_yaml = crate::minimal_machine_yaml(&kind)
                        }
                        "machine" if value == "different_minimal" => {
                            machine_yaml = crate::different_minimal_machine_yaml(&kind)
                        }
                        // A WRITE-boundary-conformant machine that is byte-different
                        // from the conformant default — for same-kind/different-
                        // content collision (ALREADY_EXISTS) under enforcement.
                        "machine" if value == "different_conformant" => {
                            machine_yaml = crate::different_conformant_machine_yaml(&kind)
                        }
                        // A machine declaring `hook: intent.md`; pair with a
                        // `hook | intent.md` row to persist a valid hook-bearing
                        // playbook.
                        "machine" if value == "hook_bearing" => {
                            machine_yaml = crate::machine_yaml_with_hook(&kind, "intent.md")
                        }
                        // A machine declaring `hook: missing.md` with NO matching
                        // hook carried — proves the unknown-hook rejection.
                        "machine" if value == "hook_missing" => {
                            machine_yaml = crate::machine_yaml_with_hook(&kind, "missing.md")
                        }
                        // A WRITE-boundary-conformant DRIVEN machine with a
                        // MEASURED initial step but NO evidence_obligation. Passes
                        // the always-on measurement gate; refused ONLY when the
                        // evidence-obligation dark-gate is on (T-EEC-1 P4 persist
                        // seam) with playbook_evidence_obligation_missing.
                        "machine" if value == "driven_measured_no_obligation" => {
                            machine_yaml = crate::machine_yaml_driven_measured_no_obligation(&kind)
                        }
                        // Attach a hook file named `value` with a synthetic body.
                        "hook" => {
                            hooks.push((value.clone(), format!("Hook body for {}", value)))
                        }
                        other => return Err(format!("Unknown PersistPlaybook RPC field: '{}'", other)),
                    }
                }

                // The conformant machines reference `intent.md`; carry it so the
                // enforcing loader resolves the reference (unless a scenario
                // already supplied it). Harmless for the negative-path machines,
                // which fail before/regardless of the reference.
                if !hooks.iter().any(|(name, _)| name == "intent.md") {
                    hooks.push(("intent.md".to_string(), "Hook body for intent.md".to_string()));
                }

                let proto_hooks: Vec<anvil_engine::proto::PlaybookHook> = hooks
                    .into_iter()
                    .map(|(name, content)| anvil_engine::proto::PlaybookHook { name, content })
                    .collect();
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::PersistPlaybookRequest {
                            hearth_path: String::new(),
                            owner_home: owner_home.to_string_lossy().into_owned(),
                            kind,
                            machine_yaml,
                            actor_name,
                            actor_type,
                            actor_model,
                            actor_provider,
                            actor_context_window: 0,
                            actor_sdk_version: String::new(),
                            actor_entrypoint: String::new(),
                            hooks: proto_hooks,
                        });
                        match client.persist_playbook(request).await {
                            Ok(response) => PersistPlaybookRpcResult::Success(response.into_inner()),
                            Err(status) => PersistPlaybookRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => PersistPlaybookRpcResult::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };

                let mut out = Context::new();
                out.set("persist_rpc_result", result);
                out.set("engine_process", engine);
                out.set("hearth_path", hearth_path);
                out.set("persist_owner_home", owner_home);
                out.set("persist_owner_home_handle", handle);
                if let Some(before) = playbooks_before {
                    out.set("persist_hearth_playbooks_before", before);
                }
                Ok(out)
            },
        ),
        async_step_def(
            "the PersistPlaybook RPC is called for kind {string} with raw owner_home {string} and:",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("persist_rpc_result", "PersistPlaybookRpcResult"),
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
            ],
            |mut ctx, params| async move {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let owner_home = params
                    .get_string(1)
                    .ok_or("Expected raw owner_home")?
                    .to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let hearth_path = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let table = params.data_table().ok_or("Expected data table")?;

                let mut actor_name = "Persist-Doer-700001".to_string();
                let mut actor_type = "agent".to_string();
                let mut actor_model = "claude-opus-4-8".to_string();
                let mut actor_provider = "anthropic".to_string();
                let mut machine_yaml = crate::minimal_machine_yaml(&kind);

                let mut pairs: Vec<(String, String)> = Vec::new();
                if table.headers.len() >= 2 {
                    pairs.push((
                        table.headers[0].trim().to_string(),
                        table.headers[1].trim().to_string(),
                    ));
                }
                for row in &table.rows {
                    if row.len() >= 2 {
                        pairs.push((row[0].trim().to_string(), row[1].trim().to_string()));
                    }
                }
                for (key, value) in pairs {
                    match key.as_str() {
                        "actor_name" => actor_name = value,
                        "actor_type" => actor_type = value,
                        "actor_model" => actor_model = value,
                        "actor_provider" => actor_provider = value,
                        "machine" if value == "loader_invalid" => {
                            machine_yaml = crate::loader_invalid_machine_yaml(&kind)
                        }
                        "machine" if value == "minimal" => {
                            machine_yaml = crate::minimal_machine_yaml(&kind)
                        }
                        "machine" if value == "different_minimal" => {
                            machine_yaml = crate::different_minimal_machine_yaml(&kind)
                        }
                        other => {
                            return Err(format!("Unknown PersistPlaybook RPC field: '{}'", other))
                        }
                    }
                }

                let addr = format!("http://127.0.0.1:{}", engine.port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::PersistPlaybookRequest {
                            hearth_path: String::new(),
                            owner_home,
                            kind,
                            machine_yaml,
                            actor_name,
                            actor_type,
                            actor_model,
                            actor_provider,
                            actor_context_window: 0,
                            actor_sdk_version: String::new(),
                            actor_entrypoint: String::new(),
                            hooks: Vec::new(),
                        });
                        match client.persist_playbook(request).await {
                            Ok(response) => PersistPlaybookRpcResult::Success(response.into_inner()),
                            Err(status) => PersistPlaybookRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => PersistPlaybookRpcResult::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };

                let mut out = Context::new();
                out.set("persist_rpc_result", result);
                out.set("engine_process", engine);
                out.set("hearth_path", hearth_path);
                Ok(out)
            },
        ),
        check_def(
            "the PersistPlaybook RPC response kind is {string}",
            &[("persist_rpc_result", "PersistPlaybookRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected kind")?;
                let r = ctx
                    .get::<PersistPlaybookRpcResult>("persist_rpc_result")
                    .ok_or("No persist_rpc_result")?;
                match r {
                    PersistPlaybookRpcResult::Success(resp) if resp.kind == expected => Ok(()),
                    PersistPlaybookRpcResult::Success(resp) => {
                        Err(format!("Expected kind '{}', got '{}'", expected, resp.kind))
                    }
                    PersistPlaybookRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the PersistPlaybook RPC written_path is under the owner-home",
            &[
                ("persist_rpc_result", "PersistPlaybookRpcResult"),
                ("persist_owner_home", "PathBuf"),
            ],
            |ctx, _params| {
                let owner_home = ctx
                    .get::<PathBuf>("persist_owner_home")
                    .ok_or("No persist_owner_home")?;
                let r = ctx
                    .get::<PersistPlaybookRpcResult>("persist_rpc_result")
                    .ok_or("No persist_rpc_result")?;
                match r {
                    PersistPlaybookRpcResult::Success(resp) => {
                        if resp.written_path.starts_with(&owner_home.to_string_lossy().into_owned()) {
                            Ok(())
                        } else {
                            Err(format!(
                                "written_path '{}' is not under owner-home '{}'",
                                resp.written_path,
                                owner_home.display()
                            ))
                        }
                    }
                    PersistPlaybookRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "a machine.yaml exists at {string} under the persist owner-home",
            &[("persist_owner_home", "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected rel path")?;
                let owner_home = ctx
                    .get::<PathBuf>("persist_owner_home")
                    .ok_or("No persist_owner_home")?;
                let path = owner_home.join(rel);
                if path.is_file() {
                    Ok(())
                } else {
                    Err(format!("Expected file at {}", path.display()))
                }
            },
        ),
        check_def(
            "no machine.yaml exists at {string} under the persist owner-home",
            &[("persist_owner_home", "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected rel path")?;
                let owner_home = ctx
                    .get::<PathBuf>("persist_owner_home")
                    .ok_or("No persist_owner_home")?;
                let path = owner_home.join(rel);
                if path.exists() {
                    Err(format!(
                        "Expected NO file at {}, but it exists",
                        path.display()
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "a hook file exists at {string} under the persist owner-home containing {string}",
            &[("persist_owner_home", "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected rel path")?;
                let expected = params.get_string(1).ok_or("Expected content")?;
                let owner_home = ctx
                    .get::<PathBuf>("persist_owner_home")
                    .ok_or("No persist_owner_home")?;
                let path = owner_home.join(rel);
                if !path.is_file() {
                    return Err(format!("Expected hook file at {}", path.display()));
                }
                let body = std::fs::read_to_string(&path).map_err(|e| {
                    format!("Failed to read hook file {}: {}", path.display(), e)
                })?;
                if body.contains(expected) {
                    Ok(())
                } else {
                    Err(format!(
                        "Hook file {} does not contain '{}' (got: {})",
                        path.display(),
                        expected,
                        body
                    ))
                }
            },
        ),
        check_def(
            "a fresh registry from the persist owner-home resolves kind {string}",
            &[("persist_owner_home", "PathBuf")],
            |ctx, params| {
                use anvil_core::domain::playbook::hearth_registry::HearthPlaybookRegistry;
                use anvil_core::domain::playbook::registry::PlaybookRegistry;
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let owner_home = ctx
                    .get::<PathBuf>("persist_owner_home")
                    .ok_or("No persist_owner_home")?;
                let registry = HearthPlaybookRegistry::new(owner_home.clone());
                if registry.machine_for(kind).is_some() {
                    Ok(())
                } else {
                    Err(format!(
                        "fresh registry from owner-home does not resolve kind '{}'",
                        kind
                    ))
                }
            },
        ),
        check_def(
            "a fresh registry from the engine hearth does not resolve kind {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                use anvil_core::domain::playbook::hearth_registry::HearthPlaybookRegistry;
                use anvil_core::domain::playbook::registry::PlaybookRegistry;
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let registry = HearthPlaybookRegistry::new(hearth_path.clone());
                if registry.machine_for(kind).is_none() {
                    Ok(())
                } else {
                    Err(format!(
                        "engine hearth registry unexpectedly resolves kind '{}'",
                        kind
                    ))
                }
            },
        ),
        check_def(
            "the engine hearth playbooks entry count is unchanged",
            &[
                ("hearth_path", "PathBuf"),
                ("persist_hearth_playbooks_before", "usize"),
            ],
            |ctx, _params| {
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let before = ctx
                    .get::<usize>("persist_hearth_playbooks_before")
                    .ok_or("No before count")?;
                let after = count_dir_entries(&hearth_path.join("playbooks"));
                if after == *before {
                    Ok(())
                } else {
                    Err(format!(
                        "engine hearth playbooks entry count changed: before={} after={}",
                        before, after
                    ))
                }
            },
        ),
        check_def(
            "the PersistPlaybook RPC returns gRPC status {string}",
            &[("persist_rpc_result", "PersistPlaybookRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Missing status code")?.to_string();
                let r = ctx
                    .get::<PersistPlaybookRpcResult>("persist_rpc_result")
                    .ok_or("No persist_rpc_result")?;
                match r {
                    PersistPlaybookRpcResult::Error { code, .. } if *code == expected => Ok(()),
                    PersistPlaybookRpcResult::Error { code, message } => Err(format!(
                        "Expected gRPC {}, got {} ({})",
                        expected, code, message
                    )),
                    PersistPlaybookRpcResult::Success(_) => {
                        Err(format!("Expected gRPC {} error, got success", expected))
                    }
                }
            },
        ),
        check_def(
            "the PersistPlaybook RPC error message contains {string}",
            &[("persist_rpc_result", "PersistPlaybookRpcResult")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected text")?;
                let r = ctx
                    .get::<PersistPlaybookRpcResult>("persist_rpc_result")
                    .ok_or("No persist_rpc_result")?;
                match r {
                    PersistPlaybookRpcResult::Error { message, .. } if message.contains(needle) => {
                        Ok(())
                    }
                    PersistPlaybookRpcResult::Error { message, .. } => Err(format!(
                        "Error message '{}' doesn't contain '{}'",
                        message, needle
                    )),
                    PersistPlaybookRpcResult::Success(_) => {
                        Err("Expected error, got success".to_string())
                    }
                }
            },
        ),
        check_def(
            "no relative playbook directory {string} exists under the engine working directory",
            &[],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected relative path")?;
                let _engine = ctx
                    .get::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let cwd = std::env::current_dir()
                    .map_err(|e| format!("Failed to resolve engine working directory: {}", e))?;
                let path = cwd.join(rel);
                if path.exists() {
                    Err(format!(
                        "Unexpected relative playbook directory exists at {}",
                        path.display()
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        // ===== BP2: engine create RPC for a parent-less domain machine =====
        step_def(
            "a hearth seeded with the knowledge_lifecycle machine",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, _params| {
                let (handle, tmp) = retained_temp_dir("anvil-knowledge-engine-")?;
                seed_knowledge_lifecycle_engine_hearth(&tmp)?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a hearth seeded with the decision_lifecycle machine",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, _params| {
                let (handle, tmp) = retained_temp_dir("anvil-decision-engine-")?;
                seed_decision_lifecycle_engine_hearth(&tmp)?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a hearth seeded with the learning_lifecycle machine",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, _params| {
                let (handle, tmp) = retained_temp_dir("anvil-learning-engine-")?;
                seed_learning_lifecycle_engine_hearth(&tmp)?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a hearth seeded with the initiative_lifecycle machine",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, _params| {
                let (handle, tmp) = retained_temp_dir("anvil-initiative-engine-")?;
                seed_initiative_lifecycle_engine_hearth(&tmp)?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a hearth seeded with the milestone_lifecycle machine",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, _params| {
                let (handle, tmp) = retained_temp_dir("anvil-milestone-engine-")?;
                seed_milestone_lifecycle_engine_hearth(&tmp)?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a hearth seeded with the review_probe playbook",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, _params| {
                let (handle, tmp) = retained_temp_dir("anvil-review-probe-engine-")?;
                seed_review_probe_engine_hearth(&tmp)?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a hearth seeded with the revision_probe playbook",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, _params| {
                let (handle, tmp) = retained_temp_dir("anvil-revision-probe-")?;
                seed_revision_probe_engine_hearth(&tmp)?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a hearth seeded with the proposal_lifecycle machine",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, _params| {
                let (handle, tmp) = retained_temp_dir("anvil-proposal-engine-")?;
                seed_proposal_lifecycle_engine_hearth(&tmp)?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a hearth seeded with the lore_query run-backed machine",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, _params| {
                let (handle, tmp) = retained_temp_dir("anvil-lore-query-engine-")?;
                seed_lore_query_engine_hearth(&tmp)?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a hearth seeded with the transition_probe playbook with empty measurements",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, _params| {
                let (handle, tmp) = retained_temp_dir("anvil-transition-probe-")?;
                seed_transition_probe_engine_hearth(&tmp)?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "the transition-measurement sink path is blocked by a directory",
            &[
                ("hearth_path", "PathBuf"),
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
            ],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
            ],
            |mut ctx, _params| {
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let port = *ctx.get::<u16>("engine_port").ok_or("No engine_port")?;
                std::fs::create_dir_all(hearth.join("transition-measurement.jsonl"))
                    .map_err(|e| format!("block transition-measurement sink: {}", e))?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                out.set("engine_process", engine);
                out.set("engine_port", port);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        step_def(
            "the playbook-measurement sink path is blocked by a directory",
            &[
                ("hearth_path", "PathBuf"),
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
            ],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
            ],
            |mut ctx, _params| {
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let port = *ctx.get::<u16>("engine_port").ok_or("No engine_port")?;
                std::fs::create_dir_all(hearth.join("playbook-measurement.jsonl"))
                    .map_err(|e| format!("block playbook-measurement sink: {}", e))?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                out.set("engine_process", engine);
                out.set("engine_port", port);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        // BP8: seed the knowledge machine hearth AND a knowledge artifact in a
        // given state, created by actor "Seed-000000" (so a DIFFERENT actor's
        // complete with no prior begin triggers the begin-adoption soft-warn).
        step_def(
            "a hearth seeded with the knowledge_lifecycle machine and a knowledge artifact {string} in state {string}",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let (handle, tmp) = retained_temp_dir("anvil-knowledge-warn-")?;
                seed_knowledge_lifecycle_engine_hearth(&tmp)?;
                let art_dir = tmp.join("knowledge").join(&id);
                std::fs::create_dir_all(&art_dir)
                    .map_err(|e| format!("Failed to create artifact dir: {}", e))?;
                let status = format!(
                    "version: 1\nkind: knowledge_lifecycle\nstate: {state}\nactors:\n  Seed-000000:\n    type: agent\n    configurations:\n      - at: \"2026-06-01T00:00:00Z\"\n        model: claude-opus-4-8\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: \"\"\n          entrypoint: claude-code\ntransitions:\n  - to: {state}\n    at: 2026-06-01T00:00:00Z\n    actor: Seed-000000\n    role: doer\n",
                    state = state
                );
                std::fs::write(art_dir.join("status.yaml"), status)
                    .map_err(|e| format!("Failed to write status.yaml: {}", e))?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        async_step_def(
            "the begin RPC is called to create a {string} artifact named {string} with no parent",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[("engine_process", "EngineProcess"), ("begin_result", "BeginRpcResult"), ("hearth_path", "PathBuf")],
            |mut ctx, params| async move {
                let artifact_type = params.get_string(0).ok_or("Expected artifact_type")?.to_string();
                let name = params.get_string(1).ok_or("Expected name")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let port = engine.port;
                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::BeginRequest {
                            hearth_path: String::new(),
                            artifact_type,
                            parent_id: String::new(),
                            track_name: name,
                            playbook_name: String::new(),
                            target_owner: String::new(),
                            approver: String::new(),
                            actor_name: "Rpc-Test-000000".to_string(),
                            actor_type: "agent".to_string(),
                            actor_model: "test".to_string(),
                            actor_provider: "test".to_string(),
                            actor_context_window: 0,
                            actor_sdk_version: String::new(),
                            actor_entrypoint: String::new(),
                            identifier: String::new(),
                            session_role: "creator".to_string(),
                            ctx_org: "Foundation".to_string(),
                            ctx_space: String::new(),
                            ctx_role: "read".to_string(),
                            ctx_clearance: "internal".to_string(),
                        ..Default::default()
                        });
                        match client.begin(request).await {
                            Ok(response) => BeginRpcResult::Success(response.into_inner()),
                            Err(status) => BeginRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => BeginRpcResult::Error { code: "UNAVAILABLE".to_string(), message: format!("Connection failed: {}", e) },
                };
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("begin_result", result);
                out.set("hearth_path", hearth_path);
                Ok(out)
            },
        ),
        async_step_def(
            "the begin RPC is called to create a {string} artifact named {string} with no parent for conversation {string} and project root {string}",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[("engine_process", "EngineProcess"), ("begin_result", "BeginRpcResult"), ("hearth_path", "PathBuf")],
            |mut ctx, params| async move {
                let artifact_type = params.get_string(0).ok_or("Expected artifact_type")?.to_string();
                let name = params.get_string(1).ok_or("Expected name")?.to_string();
                let conversation_id = params.get_string(2).ok_or("Expected conversation_id")?.to_string();
                let project_root = params.get_string(3).ok_or("Expected project_root")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::BeginRequest {
                            hearth_path: String::new(),
                            artifact_type,
                            parent_id: String::new(),
                            track_name: name,
                            playbook_name: String::new(),
                            target_owner: String::new(),
                            approver: String::new(),
                            actor_name: "Rpc-Test-000000".to_string(),
                            actor_type: "agent".to_string(),
                            actor_model: "test".to_string(),
                            actor_provider: "test".to_string(),
                            actor_context_window: 0,
                            actor_sdk_version: String::new(),
                            actor_entrypoint: String::new(),
                            identifier: String::new(),
                            session_role: "creator".to_string(),
                            ctx_org: "Foundation".to_string(),
                            ctx_space: String::new(),
                            ctx_role: "read".to_string(),
                            ctx_clearance: "internal".to_string(),
                            conversation_id,
                            project_root: project_root.clone(),
                            ..Default::default()
                        });
                        match client.begin(request).await {
                            Ok(response) => BeginRpcResult::Success(response.into_inner()),
                            Err(status) => BeginRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => BeginRpcResult::Error { code: "UNAVAILABLE".to_string(), message: format!("Connection failed: {}", e) },
                };
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("begin_result", result);
                out.set("hearth_path", hearth_path);
                Ok(out)
            },
        ),
        async_step_def(
            "the begin RPC is called for the request hearth to create a {string} artifact named {string} with no parent",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[("engine_process", "EngineProcess"), ("begin_result", "BeginRpcResult"), ("hearth_path", "PathBuf"), ("global_playbooks_hearth_path", "PathBuf")],
            |mut ctx, params| async move {
                let artifact_type = params.get_string(0).ok_or("Expected artifact_type")?.to_string();
                let name = params.get_string(1).ok_or("Expected name")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let global = ctx.get::<PathBuf>("global_playbooks_hearth_path").cloned();
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::BeginRequest {
                            hearth_path: hearth_path.to_string_lossy().into_owned(),
                            artifact_type,
                            parent_id: String::new(),
                            track_name: name,
                            playbook_name: String::new(),
                            target_owner: String::new(),
                            approver: String::new(),
                            actor_name: "Rpc-Test-000000".to_string(),
                            actor_type: "agent".to_string(),
                            actor_model: "test".to_string(),
                            actor_provider: "test".to_string(),
                            actor_context_window: 0,
                            actor_sdk_version: String::new(),
                            actor_entrypoint: String::new(),
                            identifier: String::new(),
                            session_role: "creator".to_string(),
                            ctx_org: "Foundation".to_string(),
                            ctx_space: String::new(),
                            ctx_role: "read".to_string(),
                            ctx_clearance: "internal".to_string(),
                            ..Default::default()
                        });
                        match client.begin(request).await {
                            Ok(response) => BeginRpcResult::Success(response.into_inner()),
                            Err(status) => BeginRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => BeginRpcResult::Error { code: "UNAVAILABLE".to_string(), message: format!("Connection failed: {}", e) },
                };
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("begin_result", result);
                out.set("hearth_path", hearth_path);
                if let Some(global) = global {
                    out.set("global_playbooks_hearth_path", global);
                }
                Ok(out)
            },
        ),
        async_step_def(
            "the routed begin RPC is called to create a {string} artifact named {string} with selected {string} and turn_id {string}",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[("engine_process", "EngineProcess"), ("begin_result", "BeginRpcResult"), ("hearth_path", "PathBuf")],
            |mut ctx, params| async move {
                let artifact_type = params.get_string(0).ok_or("Expected artifact_type")?.to_string();
                let name = params.get_string(1).ok_or("Expected name")?.to_string();
                let selected = params.get_string(2).ok_or("Expected selected")?.to_string();
                let turn_id = params.get_string(3).ok_or("Expected turn_id")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let port = engine.port;
                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::BeginRequest {
                            hearth_path: String::new(),
                            artifact_type,
                            parent_id: String::new(),
                            track_name: name.clone(),
                            playbook_name: String::new(),
                            target_owner: String::new(),
                            approver: String::new(),
                            actor_name: "Rpc-Test-000000".to_string(),
                            actor_type: "agent".to_string(),
                            actor_model: "test".to_string(),
                            actor_provider: "test".to_string(),
                            actor_context_window: 0,
                            actor_sdk_version: String::new(),
                            actor_entrypoint: String::new(),
                            identifier: String::new(),
                            session_role: "creator".to_string(),
                            ctx_org: "Foundation".to_string(),
                            ctx_space: String::new(),
                            ctx_role: "read".to_string(),
                            ctx_clearance: "internal".to_string(),
                            rd_turn_id: turn_id,
                            rd_input: format!("{} {}", name, "triage"),
                            rd_candidate_set: selected.clone(),
                            rd_selected: selected,
                            rd_confidence: "0.88".to_string(),
                            ..Default::default()
                        });
                        match client.begin(request).await {
                            Ok(response) => BeginRpcResult::Success(response.into_inner()),
                            Err(status) => BeginRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => BeginRpcResult::Error { code: "UNAVAILABLE".to_string(), message: format!("Connection failed: {}", e) },
                };
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("begin_result", result);
                out.set("hearth_path", hearth_path);
                Ok(out)
            },
        ),
        async_step_def(
            "the routed begin RPC is called twice for the request hearth to create a {string} artifact named {string} with selected {string} and turn_id {string}",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("begin_result", "BeginRpcResult"),
                ("hearth_path", "PathBuf"),
                ("global_playbooks_hearth_path", "PathBuf"),
            ],
            |mut ctx, params| async move {
                let artifact_type = params
                    .get_string(0)
                    .ok_or("Expected artifact_type")?
                    .to_string();
                let name = params.get_string(1).ok_or("Expected name")?.to_string();
                let selected = params.get_string(2).ok_or("Expected selected")?.to_string();
                let turn_id = params.get_string(3).ok_or("Expected turn_id")?.to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let hearth_path = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let global = ctx.get::<PathBuf>("global_playbooks_hearth_path").cloned();
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let make_request = || anvil_engine::proto::BeginRequest {
                            hearth_path: hearth_path.to_string_lossy().into_owned(),
                            artifact_type: artifact_type.clone(),
                            parent_id: String::new(),
                            track_name: name.clone(),
                            playbook_name: String::new(),
                            target_owner: String::new(),
                            approver: String::new(),
                            actor_name: "Rpc-Test-000000".to_string(),
                            actor_type: "agent".to_string(),
                            actor_model: "test".to_string(),
                            actor_provider: "test".to_string(),
                            actor_context_window: 0,
                            actor_sdk_version: String::new(),
                            actor_entrypoint: String::new(),
                            identifier: String::new(),
                            session_role: "creator".to_string(),
                            ctx_org: "Foundation".to_string(),
                            ctx_space: String::new(),
                            ctx_role: "read".to_string(),
                            ctx_clearance: "internal".to_string(),
                            rd_turn_id: turn_id.clone(),
                            rd_input: format!("{} {}", name, selected),
                            rd_candidate_set: selected.clone(),
                            rd_selected: selected.clone(),
                            rd_confidence: "0.88".to_string(),
                            ..Default::default()
                        };
                        if let Err(status) = client.begin(crate::surfaced(make_request())).await {
                            BeginRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            }
                        } else {
                            match client.begin(crate::surfaced(make_request())).await {
                                Ok(response) => BeginRpcResult::Success(response.into_inner()),
                                Err(status) => BeginRpcResult::Error {
                                    code: grpc_code_name(status.code()),
                                    message: status.message().to_string(),
                                },
                            }
                        }
                    }
                    Err(e) => BeginRpcResult::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("begin_result", result);
                out.set("hearth_path", hearth_path);
                if let Some(global) = global {
                    out.set("global_playbooks_hearth_path", global);
                }
                Ok(out)
            },
        ),
        async_step_def(
            "the begin RPC is called to create a {string} artifact named {string} with no parent and ctx org {string} role {string} clearance {string}",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[("engine_process", "EngineProcess"), ("begin_result", "BeginRpcResult"), ("hearth_path", "PathBuf")],
            |mut ctx, params| async move {
                let artifact_type = params.get_string(0).ok_or("Expected artifact_type")?.to_string();
                let name = params.get_string(1).ok_or("Expected name")?.to_string();
                let ctx_org = params.get_string(2).ok_or("Expected ctx org")?.to_string();
                let ctx_role = params.get_string(3).ok_or("Expected ctx role")?.to_string();
                let ctx_clearance = params.get_string(4).ok_or("Expected ctx clearance")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let port = engine.port;
                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::BeginRequest {
                            hearth_path: String::new(),
                            artifact_type,
                            parent_id: String::new(),
                            track_name: name,
                            playbook_name: String::new(),
                            target_owner: String::new(),
                            approver: String::new(),
                            actor_name: "Rpc-Test-000000".to_string(),
                            actor_type: "agent".to_string(),
                            actor_model: "test".to_string(),
                            actor_provider: "test".to_string(),
                            actor_context_window: 0,
                            actor_sdk_version: String::new(),
                            actor_entrypoint: String::new(),
                            identifier: String::new(),
                            session_role: "creator".to_string(),
                            ctx_org,
                            ctx_space: String::new(),
                            ctx_role,
                            ctx_clearance,
                            ..Default::default()
                        });
                        match client.begin(request).await {
                            Ok(response) => BeginRpcResult::Success(response.into_inner()),
                            Err(status) => BeginRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => BeginRpcResult::Error { code: "UNAVAILABLE".to_string(), message: format!("Connection failed: {}", e) },
                };
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("begin_result", result);
                out.set("hearth_path", hearth_path);
                Ok(out)
            },
        ),
        async_step_def(
            "the begin RPC is called to create a playbook named {string} under parent {string} with ctx org {string} role {string} clearance {string}",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[("engine_process", "EngineProcess"), ("begin_result", "BeginRpcResult"), ("hearth_path", "PathBuf")],
            |mut ctx, params| async move {
                let playbook_name = params.get_string(0).ok_or("Expected playbook_name")?.to_string();
                let parent_id = params.get_string(1).ok_or("Expected parent_id")?.to_string();
                let ctx_org = params.get_string(2).ok_or("Expected ctx org")?.to_string();
                let ctx_role = params.get_string(3).ok_or("Expected ctx role")?.to_string();
                let ctx_clearance = params.get_string(4).ok_or("Expected ctx clearance")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let port = engine.port;
                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::BeginRequest {
                            hearth_path: String::new(),
                            artifact_type: "playbook".to_string(),
                            parent_id,
                            track_name: String::new(),
                            playbook_name,
                            target_owner: String::new(),
                            approver: "Approver-E2E".to_string(),
                            actor_name: "Playbook-Rpc-Test-000000".to_string(),
                            actor_type: "agent".to_string(),
                            actor_model: "test".to_string(),
                            actor_provider: "test".to_string(),
                            actor_context_window: 0,
                            actor_sdk_version: String::new(),
                            actor_entrypoint: String::new(),
                            identifier: String::new(),
                            session_role: "creator".to_string(),
                            ctx_org,
                            ctx_space: String::new(),
                            ctx_role,
                            ctx_clearance,
                            ..Default::default()
                        });
                        match client.begin(request).await {
                            Ok(response) => BeginRpcResult::Success(response.into_inner()),
                            Err(status) => BeginRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => BeginRpcResult::Error { code: "UNAVAILABLE".to_string(), message: format!("Connection failed: {}", e) },
                };
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("begin_result", result);
                out.set("hearth_path", hearth_path);
                Ok(out)
            },
        ),
        check_def(
            "the begin RPC response track_path starts with {string}",
            &[("begin_result", "BeginRpcResult")],
            |ctx, params| {
                let prefix = params.get_string(0).ok_or("Expected prefix")?.to_string();
                let result = ctx.get::<BeginRpcResult>("begin_result").ok_or("No begin_result")?;
                match result {
                    BeginRpcResult::Success(r) if r.track_path.starts_with(&prefix) => Ok(()),
                    BeginRpcResult::Success(r) => Err(format!(
                        "track_path '{}' does not start with '{}'",
                        r.track_path, prefix
                    )),
                    BeginRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the begin RPC response track_path contains {string}",
            &[("begin_result", "BeginRpcResult")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected text")?.to_string();
                let result = ctx.get::<BeginRpcResult>("begin_result").ok_or("No begin_result")?;
                match result {
                    BeginRpcResult::Success(r) if r.track_path.contains(&needle) => Ok(()),
                    BeginRpcResult::Success(r) => Err(format!(
                        "track_path '{}' does not contain '{}'",
                        r.track_path, needle
                    )),
                    BeginRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the begin RPC response track_path has no {string} file",
            &[("begin_result", "BeginRpcResult"), ("hearth_path", "PathBuf")],
            |ctx, params| {
                let filename = params.get_string(0).ok_or("Expected filename")?.to_string();
                let result = ctx.get::<BeginRpcResult>("begin_result").ok_or("No begin_result")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let track_path = match result {
                    BeginRpcResult::Success(r) => &r.track_path,
                    BeginRpcResult::Error { code, message } => {
                        return Err(format!("Expected success, got gRPC {}: {}", code, message));
                    }
                };
                let candidate = hearth.join(track_path).join(&filename);
                if candidate.exists() {
                    Err(format!("Expected NO {} file, but it exists at {}", filename, candidate.display()))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the begin RPC response track_path has file {string}",
            &[("begin_result", "BeginRpcResult"), ("hearth_path", "PathBuf")],
            |ctx, params| {
                let filename = params.get_string(0).ok_or("Expected filename")?.to_string();
                let result = ctx.get::<BeginRpcResult>("begin_result").ok_or("No begin_result")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let track_path = match result {
                    BeginRpcResult::Success(r) => &r.track_path,
                    BeginRpcResult::Error { code, message } => {
                        return Err(format!("Expected success, got gRPC {}: {}", code, message));
                    }
                };
                let candidate = hearth.join(track_path).join(&filename);
                if candidate.exists() {
                    Ok(())
                } else {
                    Err(format!("Expected {} file at {}", filename, candidate.display()))
                }
            },
        ),
        async_step_def(
            "the complete RPC is called on the begin RPC response artifact with satisfaction {string}",
            &[("engine_process", "EngineProcess"), ("begin_result", "BeginRpcResult"), ("hearth_path", "PathBuf")],
            &[
                ("complete_rpc_result", "CompleteRpcResult"),
                ("engine_process", "EngineProcess"),
                ("begin_result", "BeginRpcResult"),
                ("hearth_path", "PathBuf"),
            ],
            |mut ctx, params| async move {
                let satisfaction = params.get_string(0).ok_or("Expected satisfaction")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let begin_result = ctx
                    .get::<BeginRpcResult>("begin_result")
                    .ok_or("No begin_result")?
                    .clone();
                let artifact_path = match &begin_result {
                    BeginRpcResult::Success(resp) => resp.track_path.clone(),
                    BeginRpcResult::Error { code, message } => {
                        return Err(format!("Expected begin success, got gRPC {}: {}", code, message));
                    }
                };
                let result = call_complete_with_bearer(
                    &engine,
                    None,
                    "Rpc-Test-000000",
                    &artifact_path,
                    &satisfaction,
                )
                .await;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let mut out = Context::new();
                out.set("complete_rpc_result", result);
                out.set("engine_process", engine);
                out.set("begin_result", begin_result);
                out.set("hearth_path", hearth_path);
                Ok(out)
            },
        ),
        async_step_def(
            "the complete RPC is called on the begin RPC response artifact with satisfaction {string} and project root {string}",
            &[("engine_process", "EngineProcess"), ("begin_result", "BeginRpcResult"), ("hearth_path", "PathBuf")],
            &[
                ("complete_rpc_result", "CompleteRpcResult"),
                ("engine_process", "EngineProcess"),
                ("begin_result", "BeginRpcResult"),
                ("hearth_path", "PathBuf"),
            ],
            |mut ctx, params| async move {
                let satisfaction = params.get_string(0).ok_or("Expected satisfaction")?.to_string();
                let project_root = params.get_string(1).ok_or("Expected project_root")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let begin_result = ctx
                    .get::<BeginRpcResult>("begin_result")
                    .ok_or("No begin_result")?
                    .clone();
                let artifact_path = match &begin_result {
                    BeginRpcResult::Success(resp) => resp.track_path.clone(),
                    BeginRpcResult::Error { code, message } => {
                        return Err(format!("Expected begin success, got gRPC {}: {}", code, message));
                    }
                };
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::CompleteRequest {
                            hearth_path: String::new(),
                            artifact_path,
                            actor_name: "Rpc-Test-000000".to_string(),
                            actor_type: "agent".to_string(),
                            actor_model: "test".to_string(),
                            actor_provider: "test".to_string(),
                            actor_context_window: 0,
                            actor_sdk_version: String::new(),
                            actor_entrypoint: String::new(),
                            satisfaction,
                            approver: String::new(),
                            note: String::new(),
                            reflection_notes: String::new(),
                            findings: String::new(),
                            conversation_id: String::new(),
                            project_root: project_root.clone(),
                            claimed_evidence: Vec::new(),
                        });
                        match client.complete(request).await {
                            Ok(response) => CompleteRpcResult::Success(response.into_inner()),
                            Err(status) => CompleteRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => CompleteRpcResult::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let mut out = Context::new();
                out.set("complete_rpc_result", result);
                out.set("engine_process", engine);
                out.set("begin_result", begin_result);
                out.set("hearth_path", hearth_path);
                Ok(out)
            },
        ),
        async_step_def(
            "the snapshot RPC is called on the begin RPC response artifact to state {string} with role {string}",
            &[("engine_process", "EngineProcess"), ("begin_result", "BeginRpcResult"), ("hearth_path", "PathBuf")],
            &[
                ("snapshot_rpc_result", "SnapshotRpcResult"),
                ("engine_process", "EngineProcess"),
                ("begin_result", "BeginRpcResult"),
                ("hearth_path", "PathBuf"),
            ],
            |mut ctx, params| async move {
                let to_state = params.get_string(0).ok_or("Expected to_state")?.to_string();
                let actor_role = params.get_string(1).ok_or("Expected role")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let begin_result = ctx
                    .get::<BeginRpcResult>("begin_result")
                    .ok_or("No begin_result")?
                    .clone();
                let artifact_path = match &begin_result {
                    BeginRpcResult::Success(resp) => resp.track_path.clone(),
                    BeginRpcResult::Error { code, message } => {
                        return Err(format!("Expected begin success, got gRPC {}: {}", code, message));
                    }
                };
                let req = anvil_engine::proto::SnapshotRequest {
                    hearth_path: String::new(),
                    artifact_path,
                    to_state,
                    actor_name: "Rpc-Snapshot-000000".to_string(),
                    actor_role,
                    approver: String::new(),
                    note: String::new(),
                    actor_type: "agent".to_string(),
                    actor_model: "claude-opus-4-8".to_string(),
                    actor_provider: "anthropic".to_string(),
                    actor_context_window: 0,
                    actor_sdk_version: String::new(),
                    actor_entrypoint: String::new(),
                    projection_only: false,
                    event_type: String::new(),
                    conversation_id: String::new(),
                    project_root: String::new(),
                    claimed_evidence: Vec::new(),
                };
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => match client.snapshot(crate::surfaced(req)).await {
                        Ok(response) => SnapshotRpcResult::Success(response.into_inner()),
                        Err(status) => SnapshotRpcResult::Error {
                            code: grpc_code_name(status.code()),
                            message: status.message().to_string(),
                        },
                    },
                    Err(e) => SnapshotRpcResult::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let mut out = Context::new();
                out.set("snapshot_rpc_result", result);
                out.set("engine_process", engine);
                out.set("begin_result", begin_result);
                out.set("hearth_path", hearth_path);
                Ok(out)
            },
        ),
        async_step_def(
            "the snapshot RPC is called on the begin RPC response artifact to state {string} with role {string} and project root {string}",
            &[("engine_process", "EngineProcess"), ("begin_result", "BeginRpcResult"), ("hearth_path", "PathBuf")],
            &[
                ("snapshot_rpc_result", "SnapshotRpcResult"),
                ("engine_process", "EngineProcess"),
                ("begin_result", "BeginRpcResult"),
                ("hearth_path", "PathBuf"),
            ],
            |mut ctx, params| async move {
                let to_state = params.get_string(0).ok_or("Expected to_state")?.to_string();
                let actor_role = params.get_string(1).ok_or("Expected role")?.to_string();
                let project_root = params.get_string(2).ok_or("Expected project_root")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let begin_result = ctx
                    .get::<BeginRpcResult>("begin_result")
                    .ok_or("No begin_result")?
                    .clone();
                let artifact_path = match &begin_result {
                    BeginRpcResult::Success(resp) => resp.track_path.clone(),
                    BeginRpcResult::Error { code, message } => {
                        return Err(format!("Expected begin success, got gRPC {}: {}", code, message));
                    }
                };
                let req = anvil_engine::proto::SnapshotRequest {
                    hearth_path: String::new(),
                    artifact_path,
                    to_state,
                    actor_name: "Rpc-Snapshot-000000".to_string(),
                    actor_role,
                    approver: String::new(),
                    note: String::new(),
                    actor_type: "agent".to_string(),
                    actor_model: "claude-opus-4-8".to_string(),
                    actor_provider: "anthropic".to_string(),
                    actor_context_window: 0,
                    actor_sdk_version: String::new(),
                    actor_entrypoint: String::new(),
                    projection_only: false,
                    event_type: String::new(),
                    conversation_id: String::new(),
                    project_root,
                    claimed_evidence: Vec::new(),
                };
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => match client.snapshot(crate::surfaced(req)).await {
                        Ok(response) => SnapshotRpcResult::Success(response.into_inner()),
                        Err(status) => SnapshotRpcResult::Error {
                            code: grpc_code_name(status.code()),
                            message: status.message().to_string(),
                        },
                    },
                    Err(e) => SnapshotRpcResult::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let mut out = Context::new();
                out.set("snapshot_rpc_result", result);
                out.set("engine_process", engine);
                out.set("begin_result", begin_result);
                out.set("hearth_path", hearth_path);
                Ok(out)
            },
        ),
        check_def(
            "no registry markdown file contains the begin RPC response track_path",
            &[("begin_result", "BeginRpcResult"), ("hearth_path", "PathBuf")],
            |ctx, _params| {
                let result = ctx.get::<BeginRpcResult>("begin_result").ok_or("No begin_result")?;
                let track_path = match result {
                    BeginRpcResult::Success(resp) => &resp.track_path,
                    BeginRpcResult::Error { code, message } => {
                        return Err(format!("Expected begin success, got gRPC {}: {}", code, message));
                    }
                };
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let entries = std::fs::read_dir(hearth)
                    .map_err(|e| format!("Failed to read hearth {}: {}", hearth.display(), e))?;
                for entry in entries {
                    let entry = entry.map_err(|e| format!("Failed to read hearth entry: {}", e))?;
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("md") {
                        continue;
                    }
                    let content = std::fs::read_to_string(&path)
                        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
                    if content.contains(track_path) {
                        return Err(format!(
                            "registry markdown file {} contains track_path '{}'",
                            path.display(),
                            track_path
                        ));
                    }
                }
                Ok(())
            },
        ),
        // ===== BP6 HEADLINE: drive a knowledge_lifecycle artifact through the
        // full happy path ingesting → published via begin-create + a sequence of
        // complete RPCs. Each hop asserts the machine-declared (to_state, role)
        // sourced from machine.yaml. ZERO "track" literals on the path. =====
        async_step_def(
            "the engine drives a knowledge_lifecycle artifact from ingesting to published",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("e2e_artifact_path", "String"),
                ("e2e_final_state", "String"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let port = engine.port;
                let addr = format!("http://127.0.0.1:{}", port);
                let mut client = anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr)
                    .await
                    .map_err(|e| format!("Connection failed: {}", e))?;

                // --- begin-create the knowledge artifact (no parent) ---
                let begin_resp = client
                    .begin(crate::surfaced(anvil_engine::proto::BeginRequest {
                        hearth_path: String::new(),
                        artifact_type: "knowledge_lifecycle".to_string(),
                        parent_id: String::new(),
                        track_name: "headline topic".to_string(),
                        playbook_name: String::new(),
                        target_owner: String::new(),
                        approver: String::new(),
                        actor_name: "Driver-E2E-100000".to_string(),
                        actor_type: "agent".to_string(),
                        actor_model: "claude-opus-4-8".to_string(),
                        actor_provider: "anthropic".to_string(),
                        actor_context_window: 0,
                        actor_sdk_version: String::new(),
                        actor_entrypoint: String::new(),
                        identifier: String::new(),
                        session_role: "creator".to_string(),
                        ctx_org: "Foundation".to_string(),
                        ctx_space: String::new(),
                        ctx_role: "read".to_string(),
                        ctx_clearance: "internal".to_string(),
                    ..Default::default()
                    }))
                    .await
                    .map_err(|s| format!("begin failed: {}: {}", grpc_code_name(s.code()), s.message()))?
                    .into_inner();
                let artifact_path = begin_resp_track_path(&begin_resp);
                if begin_resp.state != "ingesting" {
                    return Err(format!("expected create state ingesting, got '{}'", begin_resp.state));
                }

                // --- the complete chain: (satisfaction, expected_new_state) ---
                let hops: &[(&str, &str)] = &[
                    ("", "ingest_review"),          // ingest doer
                    ("approved", "organizing"),     // ingest reviewer
                    ("", "compiling"),              // organize doer
                    ("", "compile_review"),         // compile doer
                    ("approved", "validating"),     // compile reviewer
                    ("", "validation_review"),      // validate doer
                    ("approved", "published"),      // validation reviewer (role publish)
                ];
                let mut final_state = begin_resp.state.clone();
                for (satisfaction, expected) in hops {
                    let resp = client
                        .complete(crate::surfaced(anvil_engine::proto::CompleteRequest {
                            artifact_path: artifact_path.clone(),
                            actor_name: "Driver-E2E-100000".to_string(),
                            actor_type: "agent".to_string(),
                            actor_model: "claude-opus-4-8".to_string(),
                            actor_provider: "anthropic".to_string(),
                            actor_context_window: 0,
                            actor_sdk_version: String::new(),
                            actor_entrypoint: String::new(),
                            satisfaction: satisfaction.to_string(),
                            approver: "Approver-E2E".to_string(),
                            note: String::new(),
                            reflection_notes: String::new(),
                            findings: String::new(),
                            hearth_path: String::new(),
                            conversation_id: String::new(),
                            project_root: String::new(),
                            claimed_evidence: Vec::new(),
                        }))
                        .await
                        .map_err(|s| {
                            format!(
                                "complete (sat='{}', expect '{}') failed: {}: {}",
                                satisfaction,
                                expected,
                                grpc_code_name(s.code()),
                                s.message()
                            )
                        })?
                        .into_inner();
                    if &resp.new_state != expected {
                        return Err(format!(
                            "complete (sat='{}') expected new_state '{}', got '{}'",
                            satisfaction, expected, resp.new_state
                        ));
                    }
                    final_state = resp.new_state.clone();
                }

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth_path);
                out.set("e2e_artifact_path", artifact_path);
                out.set("e2e_final_state", final_state);
                Ok(out)
            },
        ),
        async_step_def(
            "the engine drives a decision artifact through review amend and retired",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("e2e_artifact_path", "String"),
                ("e2e_final_state", "String"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let port = engine.port;
                let addr = format!("http://127.0.0.1:{}", port);
                let mut client = anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr)
                    .await
                    .map_err(|e| format!("Connection failed: {}", e))?;

                let begin_resp = client
                    .begin(crate::surfaced(anvil_engine::proto::BeginRequest {
                        hearth_path: String::new(),
                        artifact_type: "decision".to_string(),
                        parent_id: String::new(),
                        track_name: "decision e2e".to_string(),
                        playbook_name: String::new(),
                        target_owner: String::new(),
                        approver: "Approver-E2E".to_string(),
                        actor_name: "Decision-E2E-100000".to_string(),
                        actor_type: "agent".to_string(),
                        actor_model: "test".to_string(),
                        actor_provider: "test".to_string(),
                        actor_context_window: 0,
                        actor_sdk_version: String::new(),
                        actor_entrypoint: String::new(),
                        identifier: String::new(),
                        session_role: "creator".to_string(),
                        ctx_org: "Foundation".to_string(),
                        ctx_space: String::new(),
                        ctx_role: "read".to_string(),
                        ctx_clearance: "internal".to_string(),
                        ..Default::default()
                    }))
                    .await
                    .map_err(|s| format!("begin failed: {}: {}", grpc_code_name(s.code()), s.message()))?
                    .into_inner();
                let artifact_path = begin_resp_track_path(&begin_resp);
                if begin_resp.state != "tension" {
                    return Err(format!("expected create state tension, got '{}'", begin_resp.state));
                }

                snapshot_to(&mut client, &artifact_path, "tension_review", "decide").await?;
                complete_to(&mut client, &artifact_path, "satisfied", "tension").await?;
                snapshot_to(&mut client, &artifact_path, "decided", "decide").await?;
                snapshot_to(&mut client, &artifact_path, "decision_review", "decide").await?;
                complete_to(&mut client, &artifact_path, "satisfied", "decided").await?;
                snapshot_to(&mut client, &artifact_path, "amend", "amend").await?;
                snapshot_to(&mut client, &artifact_path, "amend_review", "amend").await?;
                complete_to(&mut client, &artifact_path, "satisfied", "decided").await?;
                snapshot_to(&mut client, &artifact_path, "retired", "decide").await?;

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth_path);
                out.set("e2e_artifact_path", artifact_path);
                out.set("e2e_final_state", "retired".to_string());
                Ok(out)
            },
        ),
        async_step_def(
            "the engine drives a learning artifact through review establish amend and retired",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("e2e_artifact_path", "String"),
                ("e2e_final_state", "String"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let port = engine.port;
                let addr = format!("http://127.0.0.1:{}", port);
                let mut client = anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr)
                    .await
                    .map_err(|e| format!("Connection failed: {}", e))?;

                let begin_resp = client
                    .begin(crate::surfaced(anvil_engine::proto::BeginRequest {
                        hearth_path: String::new(),
                        artifact_type: "learning".to_string(),
                        parent_id: String::new(),
                        track_name: "learning e2e".to_string(),
                        playbook_name: String::new(),
                        target_owner: String::new(),
                        approver: "Approver-E2E".to_string(),
                        actor_name: "Decision-E2E-100000".to_string(),
                        actor_type: "agent".to_string(),
                        actor_model: "test".to_string(),
                        actor_provider: "test".to_string(),
                        actor_context_window: 0,
                        actor_sdk_version: String::new(),
                        actor_entrypoint: String::new(),
                        identifier: String::new(),
                        session_role: "creator".to_string(),
                        ctx_org: "Foundation".to_string(),
                        ctx_space: String::new(),
                        ctx_role: "read".to_string(),
                        ctx_clearance: "internal".to_string(),
                        ..Default::default()
                    }))
                    .await
                    .map_err(|s| format!("begin failed: {}: {}", grpc_code_name(s.code()), s.message()))?
                    .into_inner();
                let artifact_path = begin_resp_track_path(&begin_resp);
                if begin_resp.state != "observation" {
                    return Err(format!("expected create state observation, got '{}'", begin_resp.state));
                }

                snapshot_to(&mut client, &artifact_path, "observation_review", "learn").await?;
                complete_to(&mut client, &artifact_path, "satisfied", "observation").await?;
                snapshot_to(&mut client, &artifact_path, "conclusion", "learn").await?;
                snapshot_to(&mut client, &artifact_path, "conclusion_review", "learn").await?;
                complete_to(&mut client, &artifact_path, "satisfied", "established").await?;
                snapshot_to(&mut client, &artifact_path, "amend", "amend").await?;
                snapshot_to(&mut client, &artifact_path, "amend_review", "amend").await?;
                complete_to(&mut client, &artifact_path, "satisfied", "established").await?;
                snapshot_to(&mut client, &artifact_path, "retired", "learn").await?;

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth_path);
                out.set("e2e_artifact_path", artifact_path);
                out.set("e2e_final_state", "retired".to_string());
                Ok(out)
            },
        ),
        async_step_def(
            "the engine drives an initiative artifact through review promote demote log reflect and retired",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("e2e_artifact_path", "String"),
                ("e2e_final_state", "String"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let port = engine.port;
                let addr = format!("http://127.0.0.1:{}", port);
                let mut client = anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr)
                    .await
                    .map_err(|e| format!("Connection failed: {}", e))?;

                let begin_resp = client
                    .begin(crate::surfaced(anvil_engine::proto::BeginRequest {
                        hearth_path: String::new(),
                        artifact_type: "initiative".to_string(),
                        parent_id: String::new(),
                        track_name: "initiative e2e".to_string(),
                        playbook_name: String::new(),
                        target_owner: String::new(),
                        approver: "Approver-E2E".to_string(),
                        actor_name: "Initiative-E2E-100000".to_string(),
                        actor_type: "agent".to_string(),
                        actor_model: "test".to_string(),
                        actor_provider: "test".to_string(),
                        actor_context_window: 0,
                        actor_sdk_version: String::new(),
                        actor_entrypoint: String::new(),
                        identifier: String::new(),
                        session_role: "creator".to_string(),
                        ctx_org: "Foundation".to_string(),
                        ctx_space: String::new(),
                        ctx_role: "read".to_string(),
                        ctx_clearance: "internal".to_string(),
                        ..Default::default()
                    }))
                    .await
                    .map_err(|s| format!("begin failed: {}: {}", grpc_code_name(s.code()), s.message()))?
                    .into_inner();
                let artifact_path = begin_resp_track_path(&begin_resp);
                if begin_resp.state != "draft" {
                    return Err(format!("expected create state draft, got '{}'", begin_resp.state));
                }

                snapshot_to(&mut client, &artifact_path, "draft_review", "draft").await?;
                complete_to(&mut client, &artifact_path, "needs_revision", "draft_revision").await?;
                snapshot_to(&mut client, &artifact_path, "draft_review", "draft").await?;
                complete_to(&mut client, &artifact_path, "satisfied", "active").await?;
                snapshot_to(&mut client, &artifact_path, "promoted", "promote").await?;
                snapshot_to(&mut client, &artifact_path, "active", "demote").await?;

                let evidence_path = hearth_path.join(&artifact_path).join("evidence.md");
                std::fs::write(&evidence_path, "#### 2026-06-14T00:00:00Z — e2e — advance\nlog self-edge kept active\n")
                    .map_err(|e| format!("Failed to write {}: {}", evidence_path.display(), e))?;
                snapshot_to(&mut client, &artifact_path, "active", "log").await?;
                assert_resolved_state(&hearth_path, &artifact_path, "active")?;

                let reflection_path = hearth_path.join(&artifact_path).join("reflection.md");
                std::fs::write(&reflection_path, "#### 2026-06-14T00:00:00Z\nreflect self-edge kept active\n")
                    .map_err(|e| format!("Failed to write {}: {}", reflection_path.display(), e))?;
                snapshot_to(&mut client, &artifact_path, "active", "reflect").await?;
                assert_resolved_state(&hearth_path, &artifact_path, "active")?;

                snapshot_to(&mut client, &artifact_path, "retired", "retire").await?;

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth_path);
                out.set("e2e_artifact_path", artifact_path);
                out.set("e2e_final_state", "retired".to_string());
                Ok(out)
            },
        ),
        async_step_def(
            "the engine drives a milestone artifact through draft amend reflection and completed",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("e2e_artifact_path", "String"),
                ("e2e_final_state", "String"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let port = engine.port;
                let addr = format!("http://127.0.0.1:{}", port);
                let mut client = anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr)
                    .await
                    .map_err(|e| format!("Connection failed: {}", e))?;

                let begin_resp = client
                    .begin(crate::surfaced(anvil_engine::proto::BeginRequest {
                        hearth_path: String::new(),
                        artifact_type: "milestone".to_string(),
                        parent_id: String::new(),
                        track_name: "milestone e2e".to_string(),
                        playbook_name: String::new(),
                        target_owner: String::new(),
                        approver: "Approver-E2E".to_string(),
                        actor_name: "Milestone-E2E-100000".to_string(),
                        actor_type: "agent".to_string(),
                        actor_model: "test".to_string(),
                        actor_provider: "test".to_string(),
                        actor_context_window: 0,
                        actor_sdk_version: String::new(),
                        actor_entrypoint: String::new(),
                        identifier: String::new(),
                        session_role: "creator".to_string(),
                        ctx_org: "Foundation".to_string(),
                        ctx_space: String::new(),
                        ctx_role: "read".to_string(),
                        ctx_clearance: "internal".to_string(),
                        ..Default::default()
                    }))
                    .await
                    .map_err(|s| format!("begin failed: {}: {}", grpc_code_name(s.code()), s.message()))?
                    .into_inner();
                let artifact_path = begin_resp_track_path(&begin_resp);
                if begin_resp.state != "draft" {
                    return Err(format!("expected create state draft, got '{}'", begin_resp.state));
                }

                let artifact_dir = hearth_path.join(&artifact_path);
                std::fs::write(
                    artifact_dir.join("definition.md"),
                    "# Milestone E2E Definition\n\nOutcome reaches users.\n",
                )
                .map_err(|e| format!("Failed to write definition.md: {}", e))?;
                std::fs::write(artifact_dir.join("review.md"), "draft satisfied\n")
                    .map_err(|e| format!("Failed to write review.md: {}", e))?;
                snapshot_to(&mut client, &artifact_path, "draft_review", "doer").await?;
                complete_to(&mut client, &artifact_path, "needs_revision", "draft_revision").await?;
                snapshot_to(&mut client, &artifact_path, "draft_review", "doer").await?;
                complete_to(&mut client, &artifact_path, "satisfied", "active").await?;

                std::fs::write(
                    artifact_dir.join("amendments.md"),
                    "#### 2026-06-14T00:00:00Z\nmilestone amend loop returned active\n",
                )
                .map_err(|e| format!("Failed to write amendments.md: {}", e))?;
                snapshot_to(&mut client, &artifact_path, "amend", "amend").await?;
                snapshot_to(&mut client, &artifact_path, "amend_review", "amend").await?;
                complete_to(&mut client, &artifact_path, "needs_revision", "amend_revision").await?;
                snapshot_to(&mut client, &artifact_path, "amend_review", "amend").await?;
                complete_to(&mut client, &artifact_path, "satisfied", "active").await?;

                std::fs::write(
                    artifact_dir.join("reflection.md"),
                    "#### 2026-06-14T00:00:00Z\nmilestone reflection loop returned active\n",
                )
                .map_err(|e| format!("Failed to write reflection.md: {}", e))?;
                std::fs::write(artifact_dir.join("reflection.review.md"), "reflection satisfied\n")
                    .map_err(|e| format!("Failed to write reflection.review.md: {}", e))?;
                snapshot_to(&mut client, &artifact_path, "reflecting", "reflect").await?;
                snapshot_to(&mut client, &artifact_path, "reflection_review", "reflect").await?;
                complete_to(&mut client, &artifact_path, "needs_revision", "reflection_revision").await?;
                snapshot_to(&mut client, &artifact_path, "reflection_review", "reflect").await?;
                complete_to(&mut client, &artifact_path, "satisfied", "active").await?;

                snapshot_to(&mut client, &artifact_path, "completed", "doer").await?;

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth_path);
                out.set("e2e_artifact_path", artifact_path);
                out.set("e2e_final_state", "completed".to_string());
                Ok(out)
            },
        ),
        async_step_def(
            "the engine drives a milestone artifact through draft review and abandoned",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("e2e_artifact_path", "String"),
                ("e2e_final_state", "String"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let port = engine.port;
                let addr = format!("http://127.0.0.1:{}", port);
                let mut client = anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr)
                    .await
                    .map_err(|e| format!("Connection failed: {}", e))?;

                let begin_resp = client
                    .begin(crate::surfaced(anvil_engine::proto::BeginRequest {
                        hearth_path: String::new(),
                        artifact_type: "milestone".to_string(),
                        parent_id: String::new(),
                        track_name: "abandoned milestone e2e".to_string(),
                        playbook_name: String::new(),
                        target_owner: String::new(),
                        approver: "Approver-E2E".to_string(),
                        actor_name: "Milestone-Abandon-E2E-100000".to_string(),
                        actor_type: "agent".to_string(),
                        actor_model: "test".to_string(),
                        actor_provider: "test".to_string(),
                        actor_context_window: 0,
                        actor_sdk_version: String::new(),
                        actor_entrypoint: String::new(),
                        identifier: String::new(),
                        session_role: "creator".to_string(),
                        ctx_org: "Foundation".to_string(),
                        ctx_space: String::new(),
                        ctx_role: "read".to_string(),
                        ctx_clearance: "internal".to_string(),
                        ..Default::default()
                    }))
                    .await
                    .map_err(|s| format!("begin failed: {}: {}", grpc_code_name(s.code()), s.message()))?
                    .into_inner();
                let artifact_path = begin_resp_track_path(&begin_resp);
                if begin_resp.state != "draft" {
                    return Err(format!("expected create state draft, got '{}'", begin_resp.state));
                }

                snapshot_to(&mut client, &artifact_path, "draft_review", "doer").await?;
                complete_to(&mut client, &artifact_path, "satisfied", "active").await?;
                snapshot_to(&mut client, &artifact_path, "abandoned", "doer").await?;

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth_path);
                out.set("e2e_artifact_path", artifact_path);
                out.set("e2e_final_state", "abandoned".to_string());
                Ok(out)
            },
        ),
        async_step_def(
            "the engine drives a proposal artifact through reviews amend reflection and completed",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("e2e_artifact_path", "String"),
                ("e2e_final_state", "String"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let port = engine.port;
                let addr = format!("http://127.0.0.1:{}", port);
                let mut client = anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr)
                    .await
                    .map_err(|e| format!("Connection failed: {}", e))?;

                let begin_resp = client
                    .begin(crate::surfaced(anvil_engine::proto::BeginRequest {
                        hearth_path: String::new(),
                        artifact_type: "proposal".to_string(),
                        parent_id: String::new(),
                        track_name: "proposal e2e".to_string(),
                        playbook_name: String::new(),
                        target_owner: String::new(),
                        approver: "Approver-E2E".to_string(),
                        actor_name: "Proposal-E2E-100000".to_string(),
                        actor_type: "agent".to_string(),
                        actor_model: "test".to_string(),
                        actor_provider: "test".to_string(),
                        actor_context_window: 0,
                        actor_sdk_version: String::new(),
                        actor_entrypoint: String::new(),
                        identifier: String::new(),
                        session_role: "creator".to_string(),
                        ctx_org: "Foundation".to_string(),
                        ctx_space: String::new(),
                        ctx_role: "read".to_string(),
                        ctx_clearance: "internal".to_string(),
                        ..Default::default()
                    }))
                    .await
                    .map_err(|s| format!("begin failed: {}: {}", grpc_code_name(s.code()), s.message()))?
                    .into_inner();
                let artifact_path = begin_resp_track_path(&begin_resp);
                if begin_resp.state != "vision" {
                    return Err(format!("expected create state vision, got '{}'", begin_resp.state));
                }

                let artifact_dir = hearth_path.join(&artifact_path);
                std::fs::write(artifact_dir.join("vision.md"), "# Proposal E2E Vision\n")
                    .map_err(|e| format!("Failed to write vision.md: {}", e))?;
                std::fs::write(artifact_dir.join("vision.review.md"), "vision satisfied\n")
                    .map_err(|e| format!("Failed to write vision.review.md: {}", e))?;
                snapshot_to(&mut client, &artifact_path, "vision_review", "envision").await?;
                complete_to(&mut client, &artifact_path, "satisfied", "draft").await?;

                std::fs::write(artifact_dir.join("proposal.md"), "# Proposal E2E Approach\n")
                    .map_err(|e| format!("Failed to write proposal.md: {}", e))?;
                std::fs::write(artifact_dir.join("proposal.review.md"), "draft satisfied\n")
                    .map_err(|e| format!("Failed to write proposal.review.md: {}", e))?;
                snapshot_to(&mut client, &artifact_path, "draft_review", "propose").await?;
                complete_to(&mut client, &artifact_path, "satisfied", "proposal").await?;
                snapshot_to(&mut client, &artifact_path, "proposal_review", "propose").await?;
                complete_to(&mut client, &artifact_path, "satisfied", "active").await?;

                std::fs::write(
                    artifact_dir.join("proposal.amendments.md"),
                    "#### 2026-06-14T00:00:00Z\namend loop returned active\n",
                )
                .map_err(|e| format!("Failed to write proposal.amendments.md: {}", e))?;
                snapshot_to(&mut client, &artifact_path, "amend", "amend").await?;
                snapshot_to(&mut client, &artifact_path, "amend_review", "amend").await?;
                complete_to(&mut client, &artifact_path, "needs_revision", "amend_revision").await?;
                snapshot_to(&mut client, &artifact_path, "amend_review", "amend").await?;
                complete_to(&mut client, &artifact_path, "satisfied", "active").await?;

                std::fs::write(
                    artifact_dir.join("reflection.md"),
                    "#### 2026-06-14T00:00:00Z\nreflection loop returned active\n",
                )
                .map_err(|e| format!("Failed to write reflection.md: {}", e))?;
                std::fs::write(artifact_dir.join("reflection.review.md"), "reflection satisfied\n")
                    .map_err(|e| format!("Failed to write reflection.review.md: {}", e))?;
                snapshot_to(&mut client, &artifact_path, "reflecting", "reflect").await?;
                snapshot_to(&mut client, &artifact_path, "reflection_review", "reflect").await?;
                complete_to(&mut client, &artifact_path, "needs_revision", "reflection_revision").await?;
                snapshot_to(&mut client, &artifact_path, "reflection_review", "reflect").await?;
                complete_to(&mut client, &artifact_path, "satisfied", "active").await?;

                snapshot_to(&mut client, &artifact_path, "completed", "doer").await?;

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth_path);
                out.set("e2e_artifact_path", artifact_path);
                out.set("e2e_final_state", "completed".to_string());
                Ok(out)
            },
        ),
        async_step_def(
            "the engine drives a milestone artifact through review amend reflection and completed",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("e2e_artifact_path", "String"),
                ("e2e_final_state", "String"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let hearth_path = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let mut client = anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr)
                    .await
                    .map_err(|e| format!("Connection failed: {}", e))?;

                let begin_resp = client
                    .begin(crate::surfaced(anvil_engine::proto::BeginRequest {
                        hearth_path: String::new(),
                        artifact_type: "milestone".to_string(),
                        parent_id: String::new(),
                        track_name: "milestone e2e".to_string(),
                        playbook_name: String::new(),
                        target_owner: String::new(),
                        approver: "Approver-E2E".to_string(),
                        actor_name: "Milestone-E2E-100000".to_string(),
                        actor_type: "agent".to_string(),
                        actor_model: "test".to_string(),
                        actor_provider: "test".to_string(),
                        actor_context_window: 0,
                        actor_sdk_version: String::new(),
                        actor_entrypoint: String::new(),
                        identifier: String::new(),
                        session_role: "creator".to_string(),
                        ctx_org: "Foundation".to_string(),
                        ctx_space: String::new(),
                        ctx_role: "read".to_string(),
                        ctx_clearance: "internal".to_string(),
                        ..Default::default()
                    }))
                    .await
                    .map_err(|s| {
                        format!(
                            "begin failed: {}: {}",
                            grpc_code_name(s.code()),
                            s.message()
                        )
                    })?
                    .into_inner();
                let artifact_path = begin_resp_track_path(&begin_resp);
                if begin_resp.state != "draft" {
                    return Err(format!(
                        "expected create state draft, got '{}'",
                        begin_resp.state
                    ));
                }

                let artifact_dir = hearth_path.join(&artifact_path);
                std::fs::write(
                    artifact_dir.join("definition.md"),
                    "# Milestone E2E Definition\n",
                )
                .map_err(|e| format!("Failed to write definition.md: {}", e))?;
                std::fs::write(artifact_dir.join("review.md"), "draft satisfied\n")
                    .map_err(|e| format!("Failed to write review.md: {}", e))?;
                snapshot_to(&mut client, &artifact_path, "draft_review", "doer").await?;
                complete_to(&mut client, &artifact_path, "needs_revision", "draft_revision")
                    .await?;
                snapshot_to(&mut client, &artifact_path, "draft_review", "doer").await?;
                complete_to(&mut client, &artifact_path, "satisfied", "active").await?;

                std::fs::write(
                    artifact_dir.join("amendments.md"),
                    "#### 2026-06-14T00:00:00Z\namend loop returned active\n",
                )
                .map_err(|e| format!("Failed to write amendments.md: {}", e))?;
                snapshot_to(&mut client, &artifact_path, "amend", "amend").await?;
                snapshot_to(&mut client, &artifact_path, "amend_review", "amend").await?;
                complete_to(&mut client, &artifact_path, "needs_revision", "amend_revision")
                    .await?;
                snapshot_to(&mut client, &artifact_path, "amend_review", "amend").await?;
                complete_to(&mut client, &artifact_path, "satisfied", "active").await?;

                std::fs::write(
                    artifact_dir.join("reflection.md"),
                    "#### 2026-06-14T00:00:00Z\nreflection loop returned active\n",
                )
                .map_err(|e| format!("Failed to write reflection.md: {}", e))?;
                std::fs::write(
                    artifact_dir.join("reflection.review.md"),
                    "reflection satisfied\n",
                )
                .map_err(|e| format!("Failed to write reflection.review.md: {}", e))?;
                snapshot_to(&mut client, &artifact_path, "reflecting", "reflect").await?;
                snapshot_to(&mut client, &artifact_path, "reflection_review", "reflect").await?;
                complete_to(
                    &mut client,
                    &artifact_path,
                    "needs_revision",
                    "reflection_revision",
                )
                .await?;
                snapshot_to(&mut client, &artifact_path, "reflection_review", "reflect").await?;
                complete_to(&mut client, &artifact_path, "satisfied", "active").await?;

                snapshot_to(&mut client, &artifact_path, "completed", "doer").await?;

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth_path);
                out.set("e2e_artifact_path", artifact_path);
                out.set("e2e_final_state", "completed".to_string());
                Ok(out)
            },
        ),
        check_def(
            "the e2e final state is {string}",
            &[("e2e_final_state", "String")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?;
                let actual = ctx.get::<String>("e2e_final_state").ok_or("No e2e_final_state")?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("Expected final state '{}', got '{}'", expected, actual))
                }
            },
        ),
        check_def(
            "the e2e artifact path starts with {string}",
            &[("e2e_artifact_path", "String")],
            |ctx, params| {
                let prefix = params.get_string(0).ok_or("Expected prefix")?;
                let actual = ctx.get::<String>("e2e_artifact_path").ok_or("No e2e_artifact_path")?;
                if actual.starts_with(prefix) {
                    Ok(())
                } else {
                    Err(format!("Expected artifact path to start with '{}', got '{}'", prefix, actual))
                }
            },
        ),
        check_def(
            "the e2e artifact status.yaml contains {string}",
            &[("e2e_artifact_path", "String"), ("hearth_path", "PathBuf")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?.to_string();
                let artifact_path = ctx.get::<String>("e2e_artifact_path").ok_or("No e2e_artifact_path")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let status_path = hearth.join(artifact_path).join("status.yaml");
                let content = std::fs::read_to_string(&status_path)
                    .map_err(|e| format!("Failed to read {}: {}", status_path.display(), e))?;
                if content.contains(&needle) {
                    Ok(())
                } else {
                    Err(format!("status.yaml does not contain '{}'. Content:\n{}", needle, content))
                }
            },
        ),
        // Resolved-state assertions over the per-file transition event store
        // (event-store upcast). State is the fold over `<artifact>/transitions/`
        // (merged with any legacy array), NOT a status.yaml `state:` line.
        check_def(
            "the e2e artifact resolved state is {string}",
            &[("e2e_artifact_path", "String"), ("hearth_path", "PathBuf")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?.to_string();
                let artifact_path = ctx.get::<String>("e2e_artifact_path").ok_or("No e2e_artifact_path")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let state = resolved_state_in_hearth(hearth, artifact_path)?;
                if state == expected {
                    Ok(())
                } else {
                    Err(format!("Expected e2e resolved state '{}', got '{}'", expected, state))
                }
            },
        ),
        check_def(
            "the resolved state of {string} in the hearth is {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let artifact_path = params.get_string(0).ok_or("Expected artifact")?.to_string();
                let expected = params.get_string(1).ok_or("Expected state")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let state = resolved_state_in_hearth(hearth, &artifact_path)?;
                if state == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected resolved state '{}' for '{}', got '{}'",
                        expected, artifact_path, state
                    ))
                }
            },
        ),
        // Assert SOME transition event file under `<artifact>/transitions/`
        // contains the given substring (e.g. "to: amend").
        check_def(
            "a hearth transition event for {string} contains {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let artifact_path = params.get_string(0).ok_or("Expected artifact")?.to_string();
                let needle = params.get_string(1).ok_or("Expected text")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let dir = hearth.join(&artifact_path).join("transitions");
                let entries = std::fs::read_dir(&dir)
                    .map_err(|e| format!("No transitions dir for '{}': {}", artifact_path, e))?;
                for entry in entries.flatten() {
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        if content.contains(&needle) {
                            return Ok(());
                        }
                    }
                }
                Err(format!(
                    "No transition event file for '{}' contains '{}'",
                    artifact_path, needle
                ))
            },
        ),
        check_def(
            "the e2e artifact file {string} contains {string}",
            &[("e2e_artifact_path", "String"), ("hearth_path", "PathBuf")],
            |ctx, params| {
                let filename = params.get_string(0).ok_or("Expected filename")?.to_string();
                let needle = params.get_string(1).ok_or("Expected needle")?.to_string();
                let artifact_path = ctx.get::<String>("e2e_artifact_path").ok_or("No e2e_artifact_path")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let file_path = hearth.join(artifact_path).join(&filename);
                let content = std::fs::read_to_string(&file_path)
                    .map_err(|e| format!("Failed to read {}: {}", file_path.display(), e))?;
                if content.contains(&needle) {
                    Ok(())
                } else {
                    Err(format!("{} does not contain '{}'. Content:\n{}", filename, needle, content))
                }
            },
        ),
        // ===== Route RPC (playbook_routing_layer BP2) =====
        step_def(
            "a route hearth seeded with the knowledge_lifecycle machine",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, _params| {
                let (handle, tmp) = fresh_route_hearth("kl")?;
                seed_empty_route_hearth(&tmp)?;
                let wf_dir = tmp.join("playbooks").join("20260529T0409_knowledge_lifecycle");
                std::fs::create_dir_all(&wf_dir)
                    .map_err(|e| format!("Failed to create playbook dir: {}", e))?;
                std::fs::write(
                    wf_dir.join("machine.yaml"),
                    crate::query_port::knowledge_lifecycle_machine_yaml(),
                )
                .map_err(|e| format!("Failed to write machine.yaml: {}", e))?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a route hearth seeded with consulting recap fixtures plus seed playbooks",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, _params| {
                let (handle, tmp) = fresh_route_hearth("consulting-recap")?;
                seed_empty_route_hearth(&tmp)?;
                copy_fixture_playbook("daily_recap", &tmp)?;
                copy_fixture_playbook("weekly_recap", &tmp)?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        // route_response_mirrors_begin H2 — a matching candidate set whose full
        // annotations exceed ROUTE_RESPONSE_BUDGET_BYTES (8192). Two driven
        // machines share the trigger so both co-match; each carries a ~3KB
        // description (intent falls back to it) + a 4-state step_outline, so the
        // assembled annotations exceed the budget and force the truncation
        // fallback (drop step_outline, then collapse to kind + description).
        step_def(
            "a route hearth seeded with two oversized-annotation driven machines sharing trigger {string}",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, params| {
                let trigger = params.get_string(0).ok_or("Expected trigger")?.to_string();
                let (handle, tmp) = fresh_route_hearth("budget")?;
                seed_empty_route_hearth(&tmp)?;
                // 3000 bytes each: full annotations (~2*desc + step_outline per
                // candidate, ~12KB total) exceed 8192; after dropping step_outline
                // they still exceed (intent==desc); collapsed to kind+description
                // (~6KB) they fit — a deterministic three-stage fallback.
                write_oversized_route_machine(&tmp, "budget_alpha", &trigger, 3000)?;
                write_oversized_route_machine(&tmp, "budget_beta", &trigger, 3000)?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        // route_response_mirrors_begin H3 (fail-open): a single-resolution machine
        // whose initial (state, doer) declares a hook file that does not exist on
        // disk. The guidance enrichment read errors; the engine must fail open and
        // still return a valid THIN route (candidate surfaced, guidance empty).
        step_def(
            "a route hearth with one driven machine kind {string} trigger {string} declaring a missing hook",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let trigger = params.get_string(1).ok_or("Expected trigger")?.to_string();
                let (handle, tmp) = fresh_route_hearth("brokenhook")?;
                seed_empty_route_hearth(&tmp)?;
                write_route_machine_with_broken_hook(&tmp, &kind, &trigger)?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        // Scenario B (hearth self-heal): a hearth-less engine whose ONLY hearth
        // is a GLOBAL playbooks hearth carrying an unrestricted triggered machine.
        // A route turn with no caller hearth resolves the kind from the global
        // registry but must attribute to the `__unattributed__` bucket UNDER the
        // global hearth — never the global hearth root.
        step_def(
            "a hearth-less engine with a global playbooks hearth carrying driven kind {string} trigger {string}",
            &[],
            &[
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("global_playbooks_hearth_path", "PathBuf"),
            ],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let trigger = params.get_string(1).ok_or("Expected trigger")?.to_string();
                let (handle, base) = retained_temp_dir("anvil-selfheal-global-")?;
                let global = base.join("global-hearth");
                seed_empty_route_hearth(&global)?;
                write_route_machine_with_trigger(&global, &kind, &trigger)?;

                // The permitted root is the temp parent so the global hearth AND
                // the `__unattributed__` bucket created beneath it are both
                // discoverable (fail-closed policy).
                let mut process = start_engine_for_a2(None, &[base.clone()], Some(&global))?;
                process.retain_temp_dir(&handle);
                let mut out = Context::new();
                out.set("engine_port", process.port);
                out.set("engine_process", process);
                out.set("hearth_path_handle", handle);
                out.set("global_playbooks_hearth_path", global);
                Ok(out)
            },
        ),
        step_def(
            "a route hearth with one unrestricted driven machine kind {string} trigger {string}",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let trigger = params.get_string(1).ok_or("Expected trigger")?.to_string();
                let (handle, tmp) = fresh_route_hearth("turn")?;
                seed_empty_route_hearth(&tmp)?;
                write_route_machine_with_trigger(&tmp, &kind, &trigger)?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a route hearth with one unrestricted driven machine kind {string} trigger {string} purpose {string} required fields {string}",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let trigger = params.get_string(1).ok_or("Expected trigger")?.to_string();
                let purpose = params.get_string(2).ok_or("Expected purpose")?.to_string();
                let raw = params.get_string(3).ok_or("Expected required fields")?;
                let required_fields: Vec<String> = raw
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                let (handle, tmp) = fresh_route_hearth("turnmeta")?;
                seed_empty_route_hearth(&tmp)?;
                write_route_machine_with_trigger_meta(&tmp, &kind, &trigger, &purpose, &required_fields)?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a hearth seeded with the restricted knowledge_lifecycle machine",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, _params| {
                let (handle, tmp) = retained_temp_dir("anvil-restricted-knowledge-")?;
                seed_knowledge_lifecycle_engine_hearth(&tmp)?;
                let wf_dir = tmp.join("playbooks").join("20260529T0409_knowledge_lifecycle");
                std::fs::write(
                    wf_dir.join("machine.yaml"),
                    machine_yaml_with_access(
                        crate::query_port::knowledge_lifecycle_machine_yaml(),
                        "acme",
                        "admin",
                        "phi",
                    )?,
                )
                .map_err(|e| format!("Failed to write restricted machine.yaml: {}", e))?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a hearth seeded with the restricted playbook machine and an active parent track",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, _params| {
                let (handle, tmp) = retained_temp_dir("anvil-restricted-playbook-")?;
                seed_restricted_playbook_engine_hearth(&tmp)?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a route hearth with only a free {string} machine",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let (handle, tmp) = fresh_route_hearth("freeonly")?;
                seed_empty_route_hearth(&tmp)?;
                write_route_machine(&tmp, &kind, "free")?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "the route hearth also has a driven {string} machine",
            &[("hearth_path", "PathBuf")],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                write_route_machine(&hearth, &kind, "driven")?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        // resume_aware_routing — a driven machine carrying a trigger so a
        // new-intent message equal to the trigger resolves to it (no-hijack test).
        step_def(
            "the route hearth also has a driven {string} machine with trigger {string}",
            &[("hearth_path", "PathBuf")],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let trigger = params.get_string(1).ok_or("Expected trigger")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                write_route_machine_with_trigger(&hearth, &kind, &trigger)?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        // resume_aware_routing — seed an open (begun, non-terminal) artifact with a
        // durable open-begin marker carrying a conversation_id, so the route
        // handler's resume pre-check can bridge a continuation message to it. The
        // kind's terminality resolves via the seed registry (e.g. "track").
        step_def(
            "the route hearth has an open {string} artifact {string} in state {string} begun for conversation {string} at {string}",
            &[("hearth_path", "PathBuf")],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let id = params.get_string(1).ok_or("Expected id")?.to_string();
                let state = params.get_string(2).ok_or("Expected state")?.to_string();
                let conv = params.get_string(3).ok_or("Expected conversation")?.to_string();
                let at = params.get_string(4).ok_or("Expected begun-at")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                write_open_artifact(&hearth, &kind, &id, &state, &conv, &at)?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        step_def(
            "the route hearth also has a driven {string} machine with access org {string} role {string} sensitivity {string}",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let org = params.get_string(1).ok_or("Expected org")?.to_string();
                let role = params.get_string(2).ok_or("Expected role")?.to_string();
                let sensitivity = params
                    .get_string(3)
                    .ok_or("Expected sensitivity")?
                    .to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                write_route_machine_with_access(&hearth, &kind, "driven", &org, &role, &sensitivity)?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        step_def(
            "the route hearth also has a free {string} machine",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                write_route_machine(&hearth, &kind, "free")?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        step_def(
            "the route hearth also has a malformed machine.yaml",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, _params| {
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let dir = hearth.join("playbooks").join("broken_dir");
                std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create dir: {}", e))?;
                std::fs::write(dir.join("machine.yaml"), "this: is: not: valid: [[[")
                    .map_err(|e| format!("Failed to write malformed machine.yaml: {}", e))?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        step_def(
            "a driven {string} machine is dropped into the route hearth",
            &[("hearth_path", "PathBuf"), ("engine_process", "EngineProcess")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("engine_process", "EngineProcess"),
            ],
            |mut ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                write_route_machine_with_trigger(&hearth, &kind, "second")?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        // ---- anvil-hooks route-turn (real binary against a real engine) ----
        step_def(
            "anvil-hooks route-turn runs with message {string} against that engine",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("rt_exit", "i64"),
                ("rt_stdout", "String"),
            ],
            |mut ctx, params| {
                let message = params.get_string(0).ok_or("Expected message")?.to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let port = engine.port;
                let hearth = ctx.get::<PathBuf>("hearth_path").cloned();
                crate::harness::ensure_binary("anvil-hooks");
                let bin = crate::harness::binary_path("anvil-hooks");
                let mut cmd = Command::new(&bin);
                cmd.arg("route-turn")
                    .arg("--message")
                    .arg(&message)
                    .arg("--port")
                    .arg(port.to_string());
                if let Some(h) = &hearth {
                    cmd.arg("--hearth").arg(h.to_str().unwrap());
                }
                apply_kiln_router_env(&ctx, &mut cmd);
                let output = cmd.output().map_err(|e| format!("run route-turn: {}", e))?;
                let code = output.status.code().unwrap_or(-1);
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let mut out = Context::new();
                out.set("engine_process", engine);
                if let Some(h) = hearth {
                    out.set("hearth_path", h);
                }
                out.set("rt_exit", code as i64);
                out.set("rt_stdout", stdout);
                Ok(out)
            },
        ),
        step_def(
            "anvil-hooks route-turn runs with message {string} and source {string} against that engine",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                ("rt_exit", "i64"),
                ("rt_stdout", "String"),
            ],
            |mut ctx, params| {
                let message = params.get_string(0).ok_or("Expected message")?.to_string();
                let source = params.get_string(1).ok_or("Expected source")?.to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let port = engine.port;
                let hearth = ctx.get::<PathBuf>("hearth_path").cloned();
                crate::harness::ensure_binary("anvil-hooks");
                let bin = crate::harness::binary_path("anvil-hooks");
                let mut cmd = Command::new(&bin);
                cmd.arg("route-turn")
                    .arg("--message")
                    .arg(&message)
                    .arg("--source")
                    .arg(&source)
                    .arg("--port")
                    .arg(port.to_string());
                if let Some(h) = &hearth {
                    cmd.arg("--hearth").arg(h.to_str().unwrap());
                }
                apply_kiln_router_env(&ctx, &mut cmd);
                let output = cmd.output().map_err(|e| format!("run route-turn: {}", e))?;
                let code = output.status.code().unwrap_or(-1);
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let mut out = Context::new();
                out.set("engine_process", engine);
                if let Some(h) = hearth {
                    out.set("hearth_path", h);
                }
                out.set("rt_exit", code as i64);
                out.set("rt_stdout", stdout);
                Ok(out)
            },
        ),
        // resume_aware_routing H2 — drive the real binary feeding a Claude Code
        // UserPromptSubmit JSON on STDIN (no --message flag), so the binary's
        // stdin parse extracts BOTH the prompt and the session_id and carries the
        // session_id as the RouteRequest.conversation_id. Proves the conversation
        // bridge end-to-end (engine resume pre-check finds the open playbook).
        step_def(
            "anvil-hooks route-turn runs with stdin UserPromptSubmit session_id {string} prompt {string} against that engine",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                // Carry the hearth forward like the sibling route-turn steps do.
                // Without it nothing downstream can inspect delivery-log.jsonl,
                // so this variant could only ever assert stdout — which is the
                // half of the chain that was never in doubt.
                ("hearth_path", "PathBuf"),
                ("rt_exit", "i64"),
                ("rt_stdout", "String"),
            ],
            |mut ctx, params| {
                use std::io::Write;
                let session_id = params.get_string(0).ok_or("Expected session_id")?.to_string();
                let prompt = params.get_string(1).ok_or("Expected prompt")?.to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let port = engine.port;
                let hearth = ctx.get::<PathBuf>("hearth_path").cloned();
                crate::harness::ensure_binary("anvil-hooks");
                let bin = crate::harness::binary_path("anvil-hooks");
                let mut cmd = Command::new(&bin);
                cmd.arg("route-turn").arg("--port").arg(port.to_string());
                if let Some(h) = &hearth {
                    cmd.arg("--hearth").arg(h.to_str().unwrap());
                }
                apply_kiln_router_env(&ctx, &mut cmd);
                cmd.stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped());
                let mut child = cmd.spawn().map_err(|e| format!("spawn route-turn: {}", e))?;
                let payload = serde_json::json!({
                    "hook_event_name": "UserPromptSubmit",
                    "session_id": session_id,
                    "prompt": prompt,
                })
                .to_string();
                child
                    .stdin
                    .take()
                    .ok_or("no stdin")?
                    .write_all(payload.as_bytes())
                    .map_err(|e| format!("write stdin: {}", e))?;
                let output = child
                    .wait_with_output()
                    .map_err(|e| format!("wait route-turn: {}", e))?;
                let code = output.status.code().unwrap_or(-1);
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let mut out = Context::new();
                out.set("engine_process", engine);
                if let Some(h) = hearth {
                    out.set("hearth_path", h);
                }
                out.set("rt_exit", code as i64);
                out.set("rt_stdout", stdout);
                Ok(out)
            },
        ),
        step_def(
            "the route-turn hook receives continuation message {string} for conversation {string}",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                ("rt_exit", "i64"),
                ("rt_stdout", "String"),
                ("hearth_path", "PathBuf"),
            ],
            |mut ctx, params| {
                use std::io::Write;
                let prompt = params.get_string(0).ok_or("Expected continuation message")?;
                let session_id = params.get_string(1).ok_or("Expected conversation id")?;
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").cloned();
                crate::harness::ensure_binary("anvil-hooks");
                let mut cmd = Command::new(crate::harness::binary_path("anvil-hooks"));
                cmd.arg("route-turn")
                    .arg("--port")
                    .arg(engine.port.to_string());
                if let Some(path) = &hearth {
                    cmd.arg("--hearth").arg(path);
                }
                if ctx.get::<bool>("al_enabled").copied().unwrap_or(false) {
                    cmd.env("ANVIL_ABSTENTION_LEDGER", "on");
                }
                cmd.stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped());
                let mut child = cmd.spawn().map_err(|e| format!("spawn route-turn: {}", e))?;
                let payload = serde_json::json!({
                    "hook_event_name": "UserPromptSubmit",
                    "session_id": session_id,
                    "prompt": prompt,
                })
                .to_string();
                child
                    .stdin
                    .take()
                    .ok_or("no stdin")?
                    .write_all(payload.as_bytes())
                    .map_err(|e| format!("write stdin: {}", e))?;
                let output = child
                    .wait_with_output()
                    .map_err(|e| format!("wait route-turn: {}", e))?;
                let mut out = Context::new();
                out.set("engine_process", engine);
                if let Some(path) = hearth {
                    out.set("hearth_path", path);
                }
                out.set("rt_exit", output.status.code().unwrap_or(-1) as i64);
                out.set(
                    "rt_stdout",
                    String::from_utf8_lossy(&output.stdout).to_string(),
                );
                Ok(out)
            },
        ),
        check_def(
            "the route-turn guidance resumes artifact {string}",
            &[("rt_stdout", "String")],
            |ctx, params| {
                let artifact_id = params.get_string(0).ok_or("Expected artifact id")?;
                let stdout = ctx.get::<String>("rt_stdout").ok_or("No route-turn output")?;
                if stdout.contains(artifact_id) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected route-turn guidance to contain resumed artifact '{}', got {:?}",
                        artifact_id, stdout
                    ))
                }
            },
        ),
        // context_aware_routing: a capturing Kiln stub that ALSO writes a Claude
        // Code transcript fixture into the hearth. The stub records the full HTTP
        // request to `<hearth>/kiln_request.txt` so a later check can prove the
        // hook extracted the recent context + in-progress signal from the
        // transcript tail and threaded them into the router prompt. Folded into one
        // step (write fixture + start stub) so the chain matches the passing kiln
        // scenarios (hearth → kiln stub → engine → route-turn).
        step_def(
            "the kiln router HTTP stub returns verdict kind {string}, records the request, and the hearth has a transcript fixture:",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("kiln_router_port", "i64"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let fixture = params
                    .doc_string()
                    .ok_or("Expected a transcript fixture doc string")?
                    .to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                std::fs::write(hearth.join("transcript.jsonl"), fixture)
                    .map_err(|e| format!("write transcript fixture: {}", e))?;
                let request_file = hearth.join("kiln_request.txt");
                let port = start_kiln_router_capturing_stub(
                    &request_file,
                    serde_json::json!({ "kind": kind }),
                )?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                out.set("kiln_router_port", i64::from(port));
                Ok(out)
            },
        ),
        // context_aware_routing: drive the real binary with a Claude Code
        // UserPromptSubmit JSON carrying `transcript_path` pointed at the hearth
        // fixture, so the binary tails it, distills the context + in-progress
        // signal, and threads them to the (capturing) Kiln stub.
        step_def(
            "anvil-hooks route-turn runs with stdin UserPromptSubmit prompt {string} and the hearth transcript against that engine",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("rt_exit", "i64"),
                ("rt_stdout", "String"),
            ],
            |mut ctx, params| {
                use std::io::Write;
                let prompt = params.get_string(0).ok_or("Expected prompt")?.to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let port = engine.port;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let transcript_path = hearth.join("transcript.jsonl");
                crate::harness::ensure_binary("anvil-hooks");
                let bin = crate::harness::binary_path("anvil-hooks");
                let mut cmd = Command::new(&bin);
                cmd.arg("route-turn").arg("--port").arg(port.to_string());
                cmd.arg("--hearth").arg(hearth.to_str().unwrap());
                apply_kiln_router_env(&ctx, &mut cmd);
                cmd.stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped());
                let mut child = cmd.spawn().map_err(|e| format!("spawn route-turn: {}", e))?;
                let payload = serde_json::json!({
                    "hook_event_name": "UserPromptSubmit",
                    "session_id": "ctx-aware-sess",
                    "prompt": prompt,
                    "transcript_path": transcript_path.to_str().unwrap(),
                })
                .to_string();
                child
                    .stdin
                    .take()
                    .ok_or("no stdin")?
                    .write_all(payload.as_bytes())
                    .map_err(|e| format!("write stdin: {}", e))?;
                let output = child
                    .wait_with_output()
                    .map_err(|e| format!("wait route-turn: {}", e))?;
                let code = output.status.code().unwrap_or(-1);
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth);
                out.set("rt_exit", code as i64);
                out.set("rt_stdout", stdout);
                Ok(out)
            },
        ),
        check_def(
            "the captured kiln request contains {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let request = std::fs::read_to_string(hearth.join("kiln_request.txt"))
                    .map_err(|e| format!("read captured kiln request: {}", e))?;
                if request.contains(needle.as_ref() as &str) {
                    Ok(())
                } else {
                    Err(format!(
                        "captured kiln request does not contain '{}'. Request was:\n{}",
                        needle, request
                    ))
                }
            },
        ),
        step_def(
            "anvil-hooks route-turn runs with message {string} against a dead engine",
            &[],
            &[("rt_exit", "i64"), ("rt_stdout", "String")],
            |_ctx, params| {
                let message = params.get_string(0).ok_or("Expected message")?.to_string();
                // Grab (and immediately release) a free port: nothing is listening
                // there, so the route-turn dial must fail open.
                let listener = std::net::TcpListener::bind("127.0.0.1:0")
                    .map_err(|e| format!("bind: {}", e))?;
                let port = listener
                    .local_addr()
                    .map_err(|e| format!("addr: {}", e))?
                    .port();
                drop(listener);
                crate::harness::ensure_binary("anvil-hooks");
                let bin = crate::harness::binary_path("anvil-hooks");
                let output = Command::new(&bin)
                    .arg("route-turn")
                    .arg("--message")
                    .arg(&message)
                    .arg("--port")
                    .arg(port.to_string())
                    .env("ANVIL_TELEMETRY_DIR", brine_fleet_telemetry_dir())
                    .output()
                    .map_err(|e| format!("run route-turn: {}", e))?;
                let code = output.status.code().unwrap_or(-1);
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let mut out = Context::new();
                out.set("rt_exit", code as i64);
                out.set("rt_stdout", stdout);
                Ok(out)
            },
        ),
        check_def(
            "the anvil-hooks route-turn command exits 0",
            &[("rt_exit", "i64")],
            |ctx, _params| {
                let code = *ctx.get::<i64>("rt_exit").ok_or("No exit code")?;
                if code == 0 {
                    Ok(())
                } else {
                    Err(format!("expected exit 0, got {}", code))
                }
            },
        ),
        check_def(
            "the anvil-hooks route-turn output contains {string}",
            &[("rt_stdout", "String")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let out = ctx.get::<String>("rt_stdout").ok_or("No stdout")?;
                if out.contains(needle.as_ref() as &str) {
                    Ok(())
                } else {
                    Err(format!("stdout '{}' does not contain '{}'", out, needle))
                }
            },
        ),
        check_def(
            "the anvil-hooks route-turn output does not contain {string}",
            &[("rt_stdout", "String")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let out = ctx.get::<String>("rt_stdout").ok_or("No stdout")?;
                if out.contains(needle.as_ref() as &str) {
                    Err(format!("stdout '{}' unexpectedly contains '{}'", out, needle))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the anvil-hooks route-turn output is empty",
            &[("rt_stdout", "String")],
            |ctx, _params| {
                let out = ctx.get::<String>("rt_stdout").ok_or("No stdout")?;
                if out.trim().is_empty() {
                    Ok(())
                } else {
                    Err(format!("expected empty stdout, got '{}'", out))
                }
            },
        ),
        // ── the single-tier router: one Kiln → Fireworks call ──
        // One flexible step drives the REAL anvil-hooks binary with ONE injectable
        // stub Kiln port. The kiln spec is one of: `answer:<kind>` (a capturing stub
        // that records the request + replies with that verdict), `hang` (accepts +
        // never responds → the time-box ends it), `closed` (a free port nobody listens
        // on → transport_err), or `absent` (no port env → resolves to the default
        // gateway port, unreachable). The step binds the stub, applies a HERMETIC env
        // (clearing any ambient router config), runs the hook, and captures stdout /
        // stderr / exit / elapsed + the captured request file + the telemetry file.
        step_def(
            "anvil-hooks route-turn runs with message {string} kiln {string} token {string} model {string} enabled {string} timeout {int} against that engine",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("rt_exit", "i64"),
                ("rt_stdout", "String"),
                ("rt_stderr", "String"),
                ("rt_elapsed_ms", "i64"),
                ("kiln_request", "String"),
                ("router_telemetry_file", "String"),
            ],
            |mut ctx, params| {
                let message = params.get_string(0).ok_or("Expected message")?.to_string();
                let kiln_spec = params.get_string(1).ok_or("Expected kiln spec")?.to_string();
                let token = params.get_string(2).ok_or("Expected token")?.to_string();
                let model = params.get_string(3).ok_or("Expected model")?.to_string();
                let enabled = params.get_string(4).ok_or("Expected enabled")?.to_string();
                let timeout = params.get_int(5).ok_or("Expected timeout")?;

                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let port = engine.port;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();

                // Bind the single injectable Kiln stub port per its spec.
                let (kiln_port, kiln_request) = build_tier_stub(&hearth, "kiln", &kiln_spec)?;

                crate::harness::ensure_binary("anvil-hooks");
                let bin = crate::harness::binary_path("anvil-hooks");
                let mut cmd = Command::new(&bin);
                cmd.arg("route-turn")
                    .arg("--message")
                    .arg(&message)
                    .arg("--port")
                    .arg(port.to_string())
                    .arg("--hearth")
                    .arg(hearth.to_str().unwrap());

                // Hermetic env: clear any ambient router config the developer may have
                // set, then apply exactly what this scenario declares.
                for key in [
                    "ANVIL_ROUTER_ENABLED",
                    "ANVIL_KILN_PORT",
                    "ANVIL_KILN_TOKEN",
                    "ANVIL_ROUTER_MODEL",
                    "ANVIL_ROUTER_TARGET",
                    "ANVIL_KILN_ORG",
                    "ANVIL_ROUTER_CONFIG_FILE",
                    "ANVIL_ROUTER_TELEMETRY_FILE",
                ] {
                    cmd.env_remove(key);
                }
                // Never let a real ~/.foundry shared-secret file leak a bearer into a
                // test: point the token-file at a path that does not exist.
                cmd.env(
                    "ANVIL_KILN_TOKEN_FILE",
                    hearth.join("no-such-token").to_str().unwrap(),
                );
                // Hermetic against the durable router config file: point it at a path
                // that does not exist so these env-driven scenarios can never be
                // poisoned by a developer's real ~/.anvil/router.json (that file source
                // is exercised separately by the config-file scenarios).
                cmd.env(
                    "ANVIL_ROUTER_CONFIG_FILE",
                    hearth.join("no-such-router-config.json").to_str().unwrap(),
                );
                // Route the durable telemetry append at a per-scenario file so the
                // persistence sink is assertable without touching a real ~/.anvil.
                let telemetry_file = hearth.join("router-telemetry.jsonl");
                cmd.env("ANVIL_ROUTER_TELEMETRY_FILE", telemetry_file.to_str().unwrap());
                // Anvil's UNCONDITIONAL fleet telemetry (distinct from the router
                // telemetry file above) → throwaway dir, never the real ~/.anvil.
                cmd.env("ANVIL_TELEMETRY_DIR", brine_fleet_telemetry_dir());
                match enabled.as_str() {
                    "on" => {
                        cmd.env("ANVIL_ROUTER_ENABLED", "on");
                    }
                    "off" => {
                        cmd.env("ANVIL_ROUTER_ENABLED", "off");
                    }
                    _ => { /* unset — default is enabled */ }
                }
                if let Some(p) = kiln_port {
                    cmd.env("ANVIL_KILN_PORT", p.to_string());
                }
                if !token.is_empty() {
                    cmd.env("ANVIL_KILN_TOKEN", &token);
                }
                if !model.is_empty() {
                    cmd.env("ANVIL_ROUTER_MODEL", &model);
                }
                if timeout > 0 {
                    cmd.env("ANVIL_KILN_TIMEOUT_MS", timeout.to_string());
                }

                let started = std::time::Instant::now();
                let output = cmd.output().map_err(|e| format!("run route-turn: {}", e))?;
                let elapsed_ms = started.elapsed().as_millis() as i64;

                let code = output.status.code().unwrap_or(-1);
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth);
                out.set("rt_exit", code as i64);
                out.set("rt_stdout", stdout);
                out.set("rt_stderr", stderr);
                out.set("rt_elapsed_ms", elapsed_ms);
                out.set("kiln_request", kiln_request.unwrap_or_default());
                let telemetry_contents =
                    std::fs::read_to_string(&telemetry_file).unwrap_or_default();
                out.set("router_telemetry_file", telemetry_contents);
                Ok(out)
            },
        ),
        check_def(
            "the router telemetry file contains {string}",
            &[("router_telemetry_file", "String")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let contents = ctx
                    .get::<String>("router_telemetry_file")
                    .ok_or("No router_telemetry_file")?;
                if contents.contains(needle.as_ref() as &str) {
                    Ok(())
                } else {
                    Err(format!(
                        "router telemetry file '{}' does not contain '{}'",
                        contents, needle
                    ))
                }
            },
        ),
        // ── the durable config-file source (~/.anvil/router.json) ──
        // Drives the REAL anvil-hooks binary with a TEMP $HOME containing (or not) a
        // ~/.anvil/router.json fixture, proving the router resolves the model / kill
        // switch from the file with precedence ENV > file > default. `router-file` is
        // one of: `absent` (write no file), `malformed` (write junk → treated as absent),
        // `empty-object` (write `{}`), or `enabled:<value>` ("on"/"off") to write as
        // `{"enabled":<value>[,"model":<model>]}`.
        step_def(
            "anvil-hooks route-turn runs with message {string} kiln {string} router-file {string} model {string} timeout {int} against that engine",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("rt_exit", "i64"),
                ("rt_stdout", "String"),
                ("rt_stderr", "String"),
                ("rt_elapsed_ms", "i64"),
                ("kiln_request", "String"),
                ("router_telemetry_file", "String"),
            ],
            |mut ctx, params| {
                let message = params.get_string(0).ok_or("Expected message")?.to_string();
                let kiln_spec = params.get_string(1).ok_or("Expected kiln spec")?.to_string();
                let router_file = params.get_string(2).ok_or("Expected router-file")?.to_string();
                let model = params.get_string(3).ok_or("Expected model")?.to_string();
                let timeout = params.get_int(4).ok_or("Expected timeout")?;

                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let port = engine.port;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();

                let (kiln_port, kiln_request) = build_tier_stub(&hearth, "kiln", &kiln_spec)?;

                // A clean temp $HOME so the router config file resolves via its DEFAULT
                // path ($HOME/.anvil/router.json) — no ANVIL_ROUTER_CONFIG_FILE override.
                let home = hearth.join("router_home");
                let anvil_dir = home.join(".anvil");
                std::fs::create_dir_all(&anvil_dir)
                    .map_err(|e| format!("mk temp HOME/.anvil: {}", e))?;
                let config_path = anvil_dir.join("router.json");
                match router_file.as_str() {
                    "absent" => { /* write nothing — proves neither-source → default */ }
                    "malformed" => {
                        std::fs::write(&config_path, "not json {")
                            .map_err(|e| format!("write malformed router.json: {}", e))?;
                    }
                    "empty-object" => {
                        std::fs::write(&config_path, "{}")
                            .map_err(|e| format!("write empty router.json: {}", e))?;
                    }
                    other => {
                        // `enabled:<value>` → {"enabled":<value>[,"model":<model>]}.
                        let enabled_value = other.strip_prefix("enabled:").unwrap_or(other);
                        let mut obj = serde_json::Map::new();
                        obj.insert(
                            "enabled".to_string(),
                            serde_json::Value::String(enabled_value.to_string()),
                        );
                        if !model.is_empty() {
                            obj.insert(
                                "model".to_string(),
                                serde_json::Value::String(model.clone()),
                            );
                        }
                        std::fs::write(
                            &config_path,
                            serde_json::Value::Object(obj).to_string(),
                        )
                        .map_err(|e| format!("write router.json: {}", e))?;
                    }
                }

                crate::harness::ensure_binary("anvil-hooks");
                let bin = crate::harness::binary_path("anvil-hooks");
                let mut cmd = Command::new(&bin);
                cmd.arg("route-turn")
                    .arg("--message")
                    .arg(&message)
                    .arg("--port")
                    .arg(port.to_string())
                    .arg("--hearth")
                    .arg(hearth.to_str().unwrap());

                // Hermetic env: clear ambient router config, including the config-file
                // and token-file overrides (so both resolve under the temp $HOME).
                for key in [
                    "ANVIL_ROUTER_ENABLED",
                    "ANVIL_KILN_PORT",
                    "ANVIL_KILN_TOKEN",
                    "ANVIL_KILN_TOKEN_FILE",
                    "ANVIL_ROUTER_MODEL",
                    "ANVIL_ROUTER_TARGET",
                    "ANVIL_KILN_ORG",
                    "ANVIL_ROUTER_CONFIG_FILE",
                    "ANVIL_ROUTER_TELEMETRY_FILE",
                ] {
                    cmd.env_remove(key);
                }
                cmd.env("HOME", home.to_str().unwrap());
                let telemetry_file = hearth.join("router-telemetry.jsonl");
                cmd.env("ANVIL_ROUTER_TELEMETRY_FILE", telemetry_file.to_str().unwrap());
                // Fleet telemetry → throwaway dir (belt-and-suspenders over the temp
                // HOME above), never the real ~/.anvil.
                cmd.env("ANVIL_TELEMETRY_DIR", brine_fleet_telemetry_dir());
                if let Some(p) = kiln_port {
                    cmd.env("ANVIL_KILN_PORT", p.to_string());
                }
                if timeout > 0 {
                    cmd.env("ANVIL_KILN_TIMEOUT_MS", timeout.to_string());
                }

                let started = std::time::Instant::now();
                let output = cmd.output().map_err(|e| format!("run route-turn: {}", e))?;
                let elapsed_ms = started.elapsed().as_millis() as i64;

                let code = output.status.code().unwrap_or(-1);
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth);
                out.set("rt_exit", code as i64);
                out.set("rt_stdout", stdout);
                out.set("rt_stderr", stderr);
                out.set("rt_elapsed_ms", elapsed_ms);
                out.set("kiln_request", kiln_request.unwrap_or_default());
                let telemetry_contents =
                    std::fs::read_to_string(&telemetry_file).unwrap_or_default();
                out.set("router_telemetry_file", telemetry_contents);
                Ok(out)
            },
        ),
        check_def(
            "the router telemetry contains {string}",
            &[("rt_stderr", "String")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let stderr = ctx.get::<String>("rt_stderr").ok_or("No stderr")?;
                if stderr.contains(needle.as_ref() as &str) {
                    Ok(())
                } else {
                    Err(format!(
                        "router telemetry '{}' does not contain '{}'",
                        stderr, needle
                    ))
                }
            },
        ),
        check_def(
            "the router telemetry does not contain {string}",
            &[("rt_stderr", "String")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let stderr = ctx.get::<String>("rt_stderr").ok_or("No stderr")?;
                if stderr.contains(needle.as_ref() as &str) {
                    Err(format!(
                        "router telemetry unexpectedly contains '{}': {}",
                        needle, stderr
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the route-turn elapsed under {int} ms",
            &[("rt_elapsed_ms", "i64")],
            |ctx, params| {
                let bound = params.get_int(0).ok_or("Expected bound")?;
                let elapsed = *ctx.get::<i64>("rt_elapsed_ms").ok_or("No elapsed")?;
                if elapsed < bound {
                    Ok(())
                } else {
                    Err(format!(
                        "route-turn elapsed {} ms, expected under {} ms",
                        elapsed, bound
                    ))
                }
            },
        ),
        check_def(
            "the kiln stub captured a request",
            &[("kiln_request", "String")],
            |ctx, _params| assert_tier_captured(&ctx, "kiln_request", true),
        ),
        check_def(
            "the kiln stub captured no request",
            &[("kiln_request", "String")],
            |ctx, _params| assert_tier_captured(&ctx, "kiln_request", false),
        ),
        check_def(
            "the kiln captured request contains {string}",
            &[("kiln_request", "String")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                tier_request_contains(&ctx, "kiln_request", needle.as_ref() as &str, true)
            },
        ),
        check_def(
            "the kiln captured request does not contain {string}",
            &[("kiln_request", "String")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                tier_request_contains(&ctx, "kiln_request", needle.as_ref() as &str, false)
            },
        ),
        // ── anvil-hooks begin: the reliable direct-engine begin channel ──
        // Proves the real binary begins a playbook over the engine's Begin RPC
        // WITHOUT the MCP broker — the tool-availability conversion lever. We
        // capture stdout+stderr combined (success prints to stdout, rejection to
        // stderr) so the `output contains` check works for either outcome.
        step_def(
            "anvil-hooks begin runs with artifact-type {string} name {string} parent {string} approver {string} against that engine",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("begin_exit", "i64"),
                ("begin_output", "String"),
            ],
            |ctx, params| {
                let artifact_type = params.get_string(0).ok_or("Expected artifact-type")?.to_string();
                let name = params.get_string(1).ok_or("Expected name")?.to_string();
                let parent = params.get_string(2).ok_or("Expected parent")?.to_string();
                let approver = params.get_string(3).ok_or("Expected approver")?.to_string();
                run_anvil_hooks_begin(
                    ctx,
                    &[
                        "--artifact-type".to_string(),
                        artifact_type,
                        "--name".to_string(),
                        name,
                        "--parent-id".to_string(),
                        parent,
                        "--approver".to_string(),
                        approver,
                    ],
                )
            },
        ),
        step_def(
            "anvil-hooks begin runs with artifact-type {string} and no parent against that engine",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("begin_exit", "i64"),
                ("begin_output", "String"),
            ],
            |ctx, params| {
                let artifact_type = params.get_string(0).ok_or("Expected artifact-type")?.to_string();
                run_anvil_hooks_begin(ctx, &["--artifact-type".to_string(), artifact_type])
            },
        ),
        // anvil-hooks begin with two generic `--field k=v` args → BeginRequest.create_fields.
        // Proves the CLI satisfies machine-declared required fields (domain kinds) in one call.
        step_def(
            "anvil-hooks begin runs with artifact-type {string} and fields {string}={string} {string}={string} against that engine",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("begin_exit", "i64"),
                ("begin_output", "String"),
            ],
            |ctx, params| {
                let artifact_type = params.get_string(0).ok_or("Expected artifact-type")?.to_string();
                let k1 = params.get_string(1).ok_or("Expected field1 key")?.to_string();
                let v1 = params.get_string(2).ok_or("Expected field1 val")?.to_string();
                let k2 = params.get_string(3).ok_or("Expected field2 key")?.to_string();
                let v2 = params.get_string(4).ok_or("Expected field2 val")?.to_string();
                run_anvil_hooks_begin(
                    ctx,
                    &[
                        "--artifact-type".to_string(),
                        artifact_type,
                        "--field".to_string(),
                        format!("{}={}", k1, v1),
                        "--field".to_string(),
                        format!("{}={}", k2, v2),
                    ],
                )
            },
        ),
        // ── begin's RESUME mode: `--identifier <existing>` ──
        // The duplicate-artifact fix. `--identifier` alone re-enters an existing
        // artifact (BeginRequest.identifier — the mode the wire and the MCP shim
        // already had) instead of minting a new one.
        step_def(
            "anvil-hooks begin runs with identifier {string} against that engine",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("begin_exit", "i64"),
                ("begin_output", "String"),
            ],
            |ctx, params| {
                let identifier = params.get_string(0).ok_or("Expected identifier")?.to_string();
                run_anvil_hooks_begin(ctx, &["--identifier".to_string(), identifier])
            },
        ),
        // Both modes at once. The scenario's artifact-directory count is the
        // load-bearing assertion: the ambiguity must be REFUSED, never resolved
        // by silently preferring the creation flags (which minted the duplicate).
        step_def(
            "anvil-hooks begin runs with identifier {string} and artifact-type {string} name {string} parent {string} approver {string} against that engine",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("begin_exit", "i64"),
                ("begin_output", "String"),
            ],
            |ctx, params| {
                let identifier = params.get_string(0).ok_or("Expected identifier")?.to_string();
                let artifact_type = params.get_string(1).ok_or("Expected artifact-type")?.to_string();
                let name = params.get_string(2).ok_or("Expected name")?.to_string();
                let parent = params.get_string(3).ok_or("Expected parent")?.to_string();
                let approver = params.get_string(4).ok_or("Expected approver")?.to_string();
                // Every creation flag is supplied and VALID: pre-fix this call
                // sailed past the engine's required-field checks and minted the
                // duplicate track. Nothing but the ambiguity refusal can stop it.
                run_anvil_hooks_begin(
                    ctx,
                    &[
                        "--identifier".to_string(),
                        identifier,
                        "--artifact-type".to_string(),
                        artifact_type,
                        "--name".to_string(),
                        name,
                        "--parent-id".to_string(),
                        parent,
                        "--approver".to_string(),
                        approver,
                    ],
                )
            },
        ),
        // Neither mode: proves --artifact-type is still required in creation mode
        // and that the refusal names BOTH ways in.
        step_def(
            "anvil-hooks begin runs with neither artifact-type nor identifier against that engine",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("begin_exit", "i64"),
                ("begin_output", "String"),
            ],
            |ctx, _params| run_anvil_hooks_begin(ctx, &[]),
        ),
        check_def(
            "the anvil-hooks begin command exits {int}",
            &[("begin_exit", "i64")],
            |ctx, params| {
                let want = params.get_int(0).ok_or("Expected exit code")?;
                let got = *ctx.get::<i64>("begin_exit").ok_or("No begin exit code")?;
                if got == want {
                    Ok(())
                } else {
                    Err(format!("expected exit {}, got {}", want, got))
                }
            },
        ),
        check_def(
            "the anvil-hooks begin output contains {string}",
            &[("begin_output", "String")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let out = ctx.get::<String>("begin_output").ok_or("No begin output")?;
                if out.contains(needle.as_ref() as &str) {
                    Ok(())
                } else {
                    Err(format!("begin output '{}' does not contain '{}'", out, needle))
                }
            },
        ),
        step_def(
            "the kiln router HTTP stub returns verdict kind {string}",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("kiln_router_port", "i64"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let mut out =
                    start_kiln_router_http_stub(hearth, serde_json::json!({ "kind": kind }))?;
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        step_def(
            "the kiln router HTTP stub returns verdict kind {string} why {string}",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("kiln_router_port", "i64"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let why = params.get_string(1).ok_or("Expected why")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let mut out = start_kiln_router_http_stub(
                    hearth,
                    serde_json::json!({ "kind": kind, "why": why }),
                )?;
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        step_def(
            "the kiln router HTTP stub returns verdict abstain",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("kiln_router_port", "i64"),
            ],
            |ctx, _params| {
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let mut out = start_kiln_router_http_stub(
                    hearth,
                    serde_json::json!({ "kind": "abstain" }),
                )?;
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        // engine_call_failed used to name three faults at once. These two variants
        // drive the two that matter — an ABSENT engine and a blown DEADLINE — so the
        // delivery log can be asserted to tell them apart.
        step_def(
            "anvil-hooks route-turn runs with message {string} against a closed port",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("rt_exit", "i64"),
                ("rt_stdout", "String"),
            ],
            |mut ctx, params| {
                let message = params.get_string(0).ok_or("Expected message")?.to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                // A port nothing is bound to: connect() refuses immediately, which is
                // exactly the live failure shape (a burst of refusals while the engine
                // is down), NOT a slow answer.
                let dead = free_port()?;
                let hearth = ctx.get::<PathBuf>("hearth_path").cloned();
                crate::harness::ensure_binary("anvil-hooks");
                let bin = crate::harness::binary_path("anvil-hooks");
                let mut cmd = Command::new(&bin);
                cmd.arg("route-turn")
                    .arg("--message")
                    .arg(&message)
                    .arg("--port")
                    .arg(dead.to_string());
                if let Some(h) = &hearth {
                    cmd.arg("--hearth").arg(h.to_str().unwrap());
                }
                apply_kiln_router_env(&ctx, &mut cmd);
                let output = cmd.output().map_err(|e| format!("run route-turn: {}", e))?;
                let mut out = Context::new();
                out.set("engine_process", engine);
                if let Some(h) = hearth {
                    out.set("hearth_path", h);
                }
                out.set("rt_exit", output.status.code().unwrap_or(-1) as i64);
                out.set("rt_stdout", String::from_utf8_lossy(&output.stdout).to_string());
                Ok(out)
            },
        ),
        step_def(
            "anvil-hooks route-turn runs with message {string} against that engine with a 1ms cap",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("rt_exit", "i64"),
                ("rt_stdout", "String"),
            ],
            |mut ctx, params| {
                let message = params.get_string(0).ok_or("Expected message")?.to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let port = engine.port;
                let hearth = ctx.get::<PathBuf>("hearth_path").cloned();
                crate::harness::ensure_binary("anvil-hooks");
                let bin = crate::harness::binary_path("anvil-hooks");
                let mut cmd = Command::new(&bin);
                cmd.arg("route-turn")
                    .arg("--message")
                    .arg(&message)
                    .arg("--port")
                    .arg(port.to_string())
                    // A live engine that cannot possibly answer in time: isolates the
                    // DEADLINE path from the unreachable one.
                    .env("ANVIL_ROUTE_TURN_TIMEOUT_MS", "1");
                if let Some(h) = &hearth {
                    cmd.arg("--hearth").arg(h.to_str().unwrap());
                }
                apply_kiln_router_env(&ctx, &mut cmd);
                let output = cmd.output().map_err(|e| format!("run route-turn: {}", e))?;
                let mut out = Context::new();
                out.set("engine_process", engine);
                if let Some(h) = hearth {
                    out.set("hearth_path", h);
                }
                out.set("rt_exit", output.status.code().unwrap_or(-1) as i64);
                out.set("rt_stdout", String::from_utf8_lossy(&output.stdout).to_string());
                Ok(out)
            },
        ),
        // Phase 1 of the continuation track: resume_source must survive the WHOLE
        // chain — engine decision -> RouteResponse -> hook -> delivery-log.jsonl.
        // Asserted at the LOG, never at the response, because the failure this
        // guards against is a field that stops halfway. The 1500ms delivery
        // outage earlier this cycle was exactly that: an unlogged early return on
        // this path, which looked identical to a quiet router for days.
        check_def(
            "the delivery log records resume_source {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let want = params
                    .get_string(0)
                    .ok_or("Expected resume_source")?
                    .to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let log = hearth.join("delivery-log.jsonl");
                let body = std::fs::read_to_string(&log)
                    .map_err(|e| format!("read {}: {e}", log.display()))?;
                let last = body
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .next_back()
                    .ok_or("delivery log is empty")?;
                let row: serde_json::Value =
                    serde_json::from_str(last).map_err(|e| format!("bad delivery row: {e}"))?;
                let got = row["resume_source"].as_str().unwrap_or("<missing>");
                if got != want {
                    return Err(format!(
                        "delivery resume_source was {got:?}, expected {want:?} (row: {last})"
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the delivery log records outcome {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected outcome")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let log = hearth.join("delivery-log.jsonl");
                let body = std::fs::read_to_string(&log)
                    .map_err(|e| format!("read {}: {e}", log.display()))?;
                let last = body
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .next_back()
                    .ok_or("delivery log is empty")?;
                let row: serde_json::Value =
                    serde_json::from_str(last).map_err(|e| format!("bad delivery row: {e}"))?;
                let got = row["outcome"].as_str().unwrap_or("<missing>");
                if got != want {
                    return Err(format!("delivery outcome was {got:?}, expected {want:?}"));
                }
                Ok(())
            },
        ),
        // T-RDG — the notice is NOT guidance. It rides on the same stdout, so the
        // ONLY thing keeping it out of the delivered denominator is that the row is
        // computed before it is appended. Without this assertion that ordering is
        // unguarded, and a degraded turn that suggested nothing would start
        // reporting as a delivery.
        check_def(
            "the delivery log records guidance_produced {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected true/false")? == "true";
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                wait_for_delivery_rows(
                    hearth,
                    &format!("a last row with guidance_produced {}", want),
                    |rows| {
                        rows.last()
                            .and_then(|row| row.get("guidance_produced"))
                            .and_then(serde_json::Value::as_bool)
                            == Some(want)
                    },
                )?;
                Ok(())
            },
        ),
        // T-RDG — WHY the selector was absent, asserted at the LOG. The telemetry
        // line and this row are written by two different code paths; asserting
        // both on the same scenario is what keeps them from drifting.
        check_def(
            "the delivery log records router_cause {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected router_cause")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                wait_for_delivery_rows(
                    hearth,
                    &format!("a last row with router_cause {:?}", want),
                    |rows| delivery_string(rows, "router_cause") == want,
                )?;
                Ok(())
            },
        ),
        // T1 — the delivered kind and the conversation key, per FIELD.
        //
        // Deliberately NOT one parameterized `the delivery log records {string}
        // {string}`: a broader pattern registered later silently shadows the two
        // members above, and `Registry::from_defs` only panics on an EXACT
        // duplicate, so the shadow would be invisible.
        check_def(
            "the delivery log records guidance_kind {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let want = params
                    .get_string(0)
                    .ok_or("Expected guidance_kind")?
                    .to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                wait_for_delivery_rows(
                    hearth,
                    &format!("a last row with guidance_kind {:?}", want),
                    |rows| delivery_string(rows, "guidance_kind") == want,
                )?;
                Ok(())
            },
        ),
        check_def(
            "the delivery log records conversation_hash {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let want = params
                    .get_string(0)
                    .ok_or("Expected conversation_hash")?
                    .to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                wait_for_delivery_rows(
                    hearth,
                    &format!("a last row with conversation_hash {:?}", want),
                    |rows| delivery_string(rows, "conversation_hash") == want,
                )?;
                Ok(())
            },
        ),
        // A THRESHOLD, not an equality, and the reason is worth stating. The
        // count the hook records is the size of the engine's GRANTED candidate
        // set, which includes the compiled seed playbooks — so the exact number
        // is a property of the seed registry, which T1 neither owns nor should
        // pin. What the fixture does determine is that two triggered driven
        // machines exist, so the turn was offered a MENU; that is the fact this
        // scenario is about, and "more than one candidate and no kind" is
        // exactly it. An equality here would have been the observed number
        // written back as an expectation.
        check_def(
            "the delivery log records more than {int} engine_candidates",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let floor = params.get_int(0).ok_or("Expected a candidate floor")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                wait_for_delivery_rows(
                    hearth,
                    &format!("a last row with more than {} engine_candidates", floor),
                    |rows| {
                        rows.last()
                            .and_then(|row| row.get("engine_candidates"))
                            .and_then(serde_json::Value::as_i64)
                            .is_some_and(|got| got > floor)
                    },
                )?;
                Ok(())
            },
        ),
        // The glued-line guard at the binary seam. `read_jsonl_text` fails the
        // WHOLE vector on one unparseable line, so "N parseable rows" is also
        // "no line holds two objects" — which is the defect this sink already
        // shipped once, on live line 110.
        check_def(
            "the delivery log holds {int} parseable rows",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let want = params.get_int(0).ok_or("Expected row count")? as usize;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                wait_for_delivery_rows(
                    hearth,
                    &format!("{} parseable rows", want),
                    |rows| rows.len() == want,
                )?;
                Ok(())
            },
        ),
        // The POSITIVE half of the redaction proof. A `does not contain raw
        // text` assertion beside it is only meaningful if something WAS
        // written and it is a hash — otherwise an empty sink proves the
        // property by holding nothing.
        check_def(
            "the delivery log records a hashed conversation identity",
            &[("hearth_path", "PathBuf")],
            |ctx, _params| {
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let rows = wait_for_delivery_rows(hearth, "any row", |rows| !rows.is_empty())?;
                let got = delivery_string(&rows, "conversation_hash");
                if got == UNKNOWN_CONVERSATION_HASH {
                    return Err(format!(
                        "delivery conversation_hash is the unknown sentinel {got:?} — \
                         the engine answered, so this row should carry a real hash"
                    ));
                }
                if got.len() != ACTOR_HASH_HEX_LEN
                    || !got.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
                {
                    return Err(format!(
                        "delivery conversation_hash {got:?} is not {ACTOR_HASH_HEX_LEN} \
                         lowercase hex characters"
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the delivery log records a non-path project label",
            &[("hearth_path", "PathBuf")],
            |ctx, _params| {
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let rows = wait_for_delivery_rows(hearth, "any row", |rows| !rows.is_empty())?;
                let got = delivery_string(&rows, "project_label");
                if got.is_empty() || got == "<missing>" {
                    return Err(format!(
                        "delivery row carries no project_label ({got:?}); the hook has a \
                         current directory, so a label is always derivable"
                    ));
                }
                if got.contains(std::path::MAIN_SEPARATOR) {
                    return Err(format!(
                        "delivery project_label {got:?} is a PATH, not a basename"
                    ));
                }
                Ok(())
            },
        ),
        // THE load-bearing scenario of this track. Two different binaries write
        // these two rows; if they land in different keyspaces the join is
        // silently empty and the only symptom is a low coverage number with no
        // visible cause. Asserted at the two SINKS, never against a recomputed
        // hash — an offline recomputation would hide exactly this failure.
        check_def(
            "the delivery log conversation_hash equals the activity log {string} row conversation_hash",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let command = params.get_string(0).ok_or("Expected command")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let rows = wait_for_delivery_rows(hearth, "any row", |rows| !rows.is_empty())?;
                let delivery_hash = delivery_string(&rows, "conversation_hash");
                let record = wait_for_activity_record(hearth, &command)?;
                let activity_hash =
                    string_field(&record, "conversation_hash").unwrap_or("<missing>");
                if activity_hash.is_empty() || activity_hash == "<missing>" {
                    return Err(format!(
                        "activity-log {command:?} row carries no conversation_hash ({record}) — \
                         the scenario is asserting against a row that was never keyed"
                    ));
                }
                if delivery_hash != activity_hash {
                    return Err(format!(
                        "keyspace split: delivery conversation_hash {delivery_hash:?} != \
                         activity-log {command:?} conversation_hash {activity_hash:?}"
                    ));
                }
                Ok(())
            },
        ),
        step_def(
            "the kiln router port is closed",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("kiln_router_port", "i64"),
            ],
            |ctx, _params| {
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let port = closed_kiln_router_port()?;
                let mut out = Context::new();
                out.set("hearth_path", hearth.to_path_buf());
                out.set("kiln_router_port", i64::from(port));
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        // Route with an EMPTY caller hearth_path, preserving the global playbooks
        // hearth path forward so the unattributed-bucket assertions can inspect
        // it (Scenario B, hearth self-heal).
        async_step_def(
            "the route RPC is called with no caller hearth and message {string}",
            &[("engine_process", "EngineProcess"), ("global_playbooks_hearth_path", "PathBuf")],
            &[("route_result", "RouteRpcResult"), ("engine_process", "EngineProcess"), ("global_playbooks_hearth_path", "PathBuf")],
            |mut ctx, params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let port = engine.port;
                let global = ctx.get::<PathBuf>("global_playbooks_hearth_path").cloned();
                let message = params.get_string(0).ok_or("Missing message")?.to_string();
                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::RouteRequest {
            recent_context: String::new(),
            prior_proposal: None,
                            hearth_path: String::new(),
                            message,
                            ctx_org: "Foundation".to_string(),
                            ctx_role: "read".to_string(),
                            ctx_clearance: "internal".to_string(),
                            ..Default::default()
                        });
                        match client.route(request).await {
                            Ok(response) => RouteRpcResult::Success(response.into_inner()),
                            Err(status) => RouteRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => RouteRpcResult::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };
                let mut out = Context::new();
                out.set("route_result", result);
                out.set("engine_process", engine);
                if let Some(g) = global { out.set("global_playbooks_hearth_path", g); }
                Ok(out)
            },
        ),
        async_step_def(
            "the route RPC is called with message {string}",
            &[("engine_process", "EngineProcess")],
            &[("route_result", "RouteRpcResult"), ("engine_process", "EngineProcess"), ("hearth_path", "PathBuf"), ("semantic_kiln_request", "String")],
            |mut ctx, params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let port = engine.port;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").cloned();
                let semantic_kiln_request =
                    ctx.get::<String>("semantic_kiln_request").cloned();
                let message = params.get_string(0).ok_or("Missing message")?.to_string();
                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::RouteRequest {
            recent_context: String::new(),
            prior_proposal: None,
                            hearth_path: String::new(),
                            message,
                            signal: String::new(),
                            surface: String::new(),
                            parent_id: String::new(),
                            conversation_id: String::new(),
                            actor_name: String::new(),
                            actor_type: String::new(),
                            actor_model: String::new(),
                            actor_provider: String::new(),
                            ctx_org: "Foundation".to_string(),
                            ctx_space: String::new(),
                            ctx_role: "read".to_string(),
                            ctx_clearance: "internal".to_string(),
                        ..Default::default()
                        });
                        match client.route(request).await {
                            Ok(response) => RouteRpcResult::Success(response.into_inner()),
                            Err(status) => RouteRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => RouteRpcResult::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };
                let mut out = Context::new();
                out.set("route_result", result);
                out.set("engine_process", engine);
                if let Some(hp) = hearth_path {
                    out.set("hearth_path", hp);
                }
                if let Some(path) = semantic_kiln_request {
                    out.set("semantic_kiln_request", path);
                }
                Ok(out)
            },
        ),
        // route_response_mirrors_begin H3 (genuine-error-still-surfaces): a route
        // RPC against a path that is NOT a hearth fails the resolve_hearth gate —
        // a real engine error that must propagate as a gRPC error, NOT be swallowed
        // by the enrichment fail-open (which only catches the guidance read).
        async_step_def(
            "the route RPC is called with message {string} for non-hearth path {string}",
            &[("engine_process", "EngineProcess")],
            &[("route_result", "RouteRpcResult"), ("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            |mut ctx, params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let port = engine.port;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").cloned();
                let message = params.get_string(0).ok_or("Missing message")?.to_string();
                let bad_path = params.get_string(1).ok_or("Missing path")?.to_string();
                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::RouteRequest {
            recent_context: String::new(),
            prior_proposal: None,
                            hearth_path: bad_path,
                            message,
                            signal: String::new(),
                            surface: String::new(),
                            parent_id: String::new(),
                            conversation_id: String::new(),
                            actor_name: String::new(),
                            actor_type: String::new(),
                            actor_model: String::new(),
                            actor_provider: String::new(),
                            ctx_org: "Foundation".to_string(),
                            ctx_space: String::new(),
                            ctx_role: "read".to_string(),
                            ctx_clearance: "internal".to_string(),
                            ..Default::default()
                        });
                        match client.route(request).await {
                            Ok(response) => RouteRpcResult::Success(response.into_inner()),
                            Err(status) => RouteRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => RouteRpcResult::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };
                let mut out = Context::new();
                out.set("route_result", result);
                out.set("engine_process", engine);
                if let Some(hp) = hearth_path {
                    out.set("hearth_path", hp);
                }
                Ok(out)
            },
        ),
        check_def(
            "the route RPC returns a gRPC error containing {string}",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected substring")?.to_string();
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Error { message, .. } if message.contains(&needle) => Ok(()),
                    RouteRpcResult::Error { code, message } => Err(format!(
                        "route errored ({}) but message {:?} does not contain {:?}", code, message, needle
                    )),
                    RouteRpcResult::Success(resp) => Err(format!(
                        "expected a gRPC error, got a successful route: outcome={:?}", resp.resolution_outcome
                    )),
                }
            },
        ),
        async_step_def(
            "the route RPC is called with message {string}, signal {string}, and conversation_id {string}",
            &[("engine_process", "EngineProcess")],
            &[("route_result", "RouteRpcResult"), ("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            |mut ctx, params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let port = engine.port;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").cloned();
                let message = params.get_string(0).ok_or("Missing message")?.to_string();
                let signal = params.get_string(1).ok_or("Missing signal")?.to_string();
                let conversation_id = params.get_string(2).ok_or("Missing conversation_id")?.to_string();
                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::RouteRequest {
            recent_context: String::new(),
            prior_proposal: None,
                            hearth_path: String::new(),
                            message,
                            signal,
                            surface: String::new(),
                            parent_id: String::new(),
                            conversation_id,
                            actor_name: String::new(),
                            actor_type: String::new(),
                            actor_model: String::new(),
                            actor_provider: String::new(),
                            ctx_org: "Foundation".to_string(),
                            ctx_space: String::new(),
                            ctx_role: "read".to_string(),
                            ctx_clearance: "internal".to_string(),
                            ..Default::default()
                        });
                        match client.route(request).await {
                            Ok(response) => RouteRpcResult::Success(response.into_inner()),
                            Err(status) => RouteRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => RouteRpcResult::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };
                let mut out = Context::new();
                out.set("route_result", result);
                out.set("engine_process", engine);
                if let Some(hp) = hearth_path {
                    out.set("hearth_path", hp);
                }
                Ok(out)
            },
        ),
        async_step_def(
            "the route RPC is called with message {string}, conversation_id {string}, context {string}, and proposal {string}",
            &[("engine_process", "EngineProcess")],
            &[("route_result", "RouteRpcResult"), ("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            |mut ctx, params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let port = engine.port;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").cloned();
                let message = params.get_string(0).ok_or("Missing message")?.to_string();
                let signal = String::new();
                let conversation_id = params.get_string(1).ok_or("Missing conversation_id")?.to_string();
                // continuation_recognition: the two fields only the hook has.
                // Exercised here at the RPC so the ENGINE-side procedure is
                // proven directly, without a transcript fixture in the loop.
                let recent_context_in = params.get_string(2).unwrap_or_default().to_string();
                let proposal_in = params.get_string(3).unwrap_or_default().to_string();
                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::RouteRequest {
                            recent_context: recent_context_in,
                            prior_proposal: if proposal_in.is_empty() {
                                None
                            } else {
                                Some(anvil_engine::proto::PriorProposal {
                                    text: proposal_in,
                                    was_truncated: false,
                                })
                            },
                            hearth_path: String::new(),
                            message,
                            signal,
                            surface: String::new(),
                            parent_id: String::new(),
                            conversation_id,
                            actor_name: String::new(),
                            actor_type: String::new(),
                            actor_model: String::new(),
                            actor_provider: String::new(),
                            ctx_org: "Foundation".to_string(),
                            ctx_space: String::new(),
                            ctx_role: "read".to_string(),
                            ctx_clearance: "internal".to_string(),
                            ..Default::default()
                        });
                        match client.route(request).await {
                            Ok(response) => RouteRpcResult::Success(response.into_inner()),
                            Err(status) => RouteRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => RouteRpcResult::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };
                let mut out = Context::new();
                out.set("route_result", result);
                out.set("engine_process", engine);
                if let Some(hp) = hearth_path {
                    out.set("hearth_path", hp);
                }
                Ok(out)
            },
        ),
        async_step_def(
            "the route RPC is called with message {string}, signal {string}, conversation_id {string}, and project root {string}",
            &[("engine_process", "EngineProcess")],
            &[
                ("route_result", "RouteRpcResult"),
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("route_project_root", "PathBuf"),
            ],
            |mut ctx, params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let port = engine.port;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").cloned();
                let message = params.get_string(0).ok_or("Missing message")?.to_string();
                let signal = params.get_string(1).ok_or("Missing signal")?.to_string();
                let conversation_id = params.get_string(2).ok_or("Missing conversation_id")?.to_string();
                let project_root = params.get_string(3).ok_or("Missing project_root")?.to_string();
                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::RouteRequest {
            recent_context: String::new(),
            prior_proposal: None,
                            hearth_path: String::new(),
                            message,
                            signal,
                            surface: String::new(),
                            parent_id: String::new(),
                            conversation_id,
                            actor_name: String::new(),
                            actor_type: String::new(),
                            actor_model: String::new(),
                            actor_provider: String::new(),
                            ctx_org: "Consulting".to_string(),
                            ctx_space: String::new(),
                            ctx_role: "read".to_string(),
                            ctx_clearance: "internal".to_string(),
                            source: String::new(),
                            project_root: project_root.clone(),
                        });
                        match client.route(request).await {
                            Ok(response) => RouteRpcResult::Success(response.into_inner()),
                            Err(status) => RouteRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => RouteRpcResult::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };
                let mut out = Context::new();
                out.set("route_result", result);
                out.set("engine_process", engine);
                if let Some(hp) = hearth_path {
                    out.set("hearth_path", hp);
                }
                out.set("route_project_root", PathBuf::from(project_root));
                Ok(out)
            },
        ),
        async_step_def(
            "the route RPC is called with message {string} and ctx org {string} role {string} clearance {string}",
            &[("engine_process", "EngineProcess")],
            &[("route_result", "RouteRpcResult"), ("engine_process", "EngineProcess"), ("hearth_path", "PathBuf"), ("semantic_kiln_request", "String")],
            |mut ctx, params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let port = engine.port;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").cloned();
                let semantic_kiln_request =
                    ctx.get::<String>("semantic_kiln_request").cloned();
                let message = params.get_string(0).ok_or("Missing message")?.to_string();
                let ctx_org = params.get_string(1).ok_or("Missing ctx org")?.to_string();
                let ctx_role = params.get_string(2).ok_or("Missing ctx role")?.to_string();
                let ctx_clearance = params.get_string(3).ok_or("Missing ctx clearance")?.to_string();
                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::RouteRequest {
            recent_context: String::new(),
            prior_proposal: None,
                            hearth_path: String::new(),
                            message,
                            signal: String::new(),
                            surface: String::new(),
                            parent_id: String::new(),
                            conversation_id: String::new(),
                            actor_name: String::new(),
                            actor_type: String::new(),
                            actor_model: String::new(),
                            actor_provider: String::new(),
                            ctx_org,
                            ctx_space: String::new(),
                            ctx_role,
                            ctx_clearance,
                            source: String::new(),
                            project_root: String::new(),
                        });
                        match client.route(request).await {
                            Ok(response) => RouteRpcResult::Success(response.into_inner()),
                            Err(status) => RouteRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => RouteRpcResult::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };
                let mut out = Context::new();
                out.set("route_result", result);
                out.set("engine_process", engine);
                if let Some(hp) = hearth_path {
                    out.set("hearth_path", hp);
                }
                if let Some(path) = semantic_kiln_request {
                    out.set("semantic_kiln_request", path);
                }
                Ok(out)
            },
        ),
        async_step_def(
            "the route RPC is called with message {string} and source {string}",
            &[("engine_process", "EngineProcess")],
            &[("route_result", "RouteRpcResult"), ("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            |mut ctx, params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let port = engine.port;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").cloned();
                let message = params.get_string(0).ok_or("Missing message")?.to_string();
                let source = params.get_string(1).ok_or("Missing source")?.to_string();
                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::RouteRequest {
            recent_context: String::new(),
            prior_proposal: None,
                            hearth_path: String::new(),
                            message,
                            source,
                            ..Default::default()
                        });
                        match client.route(request).await {
                            Ok(response) => RouteRpcResult::Success(response.into_inner()),
                            Err(status) => RouteRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => RouteRpcResult::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };
                let mut out = Context::new();
                out.set("route_result", result);
                out.set("engine_process", engine);
                if let Some(hp) = hearth_path {
                    out.set("hearth_path", hp);
                }
                Ok(out)
            },
        ),
        async_step_def(
            "the route RPC is called with message {string}, signal {string}, conversation_id {string}, ctx org {string} role {string} clearance {string}",
            &[("engine_process", "EngineProcess")],
            &[("route_result", "RouteRpcResult"), ("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            |mut ctx, params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let port = engine.port;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").cloned();
                let message = params.get_string(0).ok_or("Missing message")?.to_string();
                let signal = params.get_string(1).ok_or("Missing signal")?.to_string();
                let conversation_id = params.get_string(2).ok_or("Missing conversation_id")?.to_string();
                let ctx_org = params.get_string(3).ok_or("Missing ctx org")?.to_string();
                let ctx_role = params.get_string(4).ok_or("Missing ctx role")?.to_string();
                let ctx_clearance = params.get_string(5).ok_or("Missing ctx clearance")?.to_string();
                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::RouteRequest {
            recent_context: String::new(),
            prior_proposal: None,
                            hearth_path: String::new(),
                            message,
                            signal,
                            surface: String::new(),
                            parent_id: String::new(),
                            conversation_id,
                            actor_name: String::new(),
                            actor_type: String::new(),
                            actor_model: String::new(),
                            actor_provider: String::new(),
                            ctx_org,
                            ctx_space: String::new(),
                            ctx_role,
                            ctx_clearance,
                            ..Default::default()
                        });
                        match client.route(request).await {
                            Ok(response) => RouteRpcResult::Success(response.into_inner()),
                            Err(status) => RouteRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => RouteRpcResult::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };
                let mut out = Context::new();
                out.set("route_result", result);
                out.set("engine_process", engine);
                if let Some(hp) = hearth_path {
                    out.set("hearth_path", hp);
                }
                Ok(out)
            },
        ),
        check_def(
            "the route outcome is {string}",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected outcome")?.to_string();
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) if resp.outcome == expected => Ok(()),
                    RouteRpcResult::Success(resp) => Err(format!("Expected outcome '{}', got '{}'", expected, resp.outcome)),
                    RouteRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the serialized route RPC response matches the main-branch no-match golden",
            &[("route_result", "RouteRpcResult")],
            |ctx, _params| {
                let result = ctx
                    .get::<RouteRpcResult>("route_result")
                    .ok_or("No route_result")?;
                let mut actual = match result {
                    RouteRpcResult::Success(response) => response.clone(),
                    RouteRpcResult::Error { code, message } => {
                        return Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                };
                actual.resolved_hearth = "<hearth>".to_string();
                // conversation_scoped_join added field 17. It is deployment-salt
                // derived, so its VALUE cannot be a byte golden — but its shape
                // can, and an EMPTY one would mean the hook persists an
                // unjoinable row. Assert the shape (16 lowercase hex, the
                // actor_hash truncation), then normalise so the rest of the
                // message stays a byte comparison.
                let hash = actual.conversation_hash.clone();
                if hash.len() != 16 || !hash.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()) {
                    return Err(format!(
                        "RouteResponse.conversation_hash must be 16 lowercase hex chars, got '{}'",
                        hash
                    ));
                }
                actual.conversation_hash = "<conversation_hash>".to_string();
                let expected = anvil_engine::proto::RouteResponse {
                    resume_source: String::new(),
                    outcome: "no_match".to_string(),
                    candidates: Vec::new(),
                    handoff: "candidate_playbook_intake".to_string(),
                    intent: "daily recap".to_string(),
                    resolved_hearth: "<hearth>".to_string(),
                    selected_kind: String::new(),
                    resolution_outcome: "no_match".to_string(),
                    matching_candidates: Vec::new(),
                    guidance: String::new(),
                    resume_artifact_id: String::new(),
                    resume_kind: String::new(),
                    resume_state: String::new(),
                    resume_guidance: String::new(),
                    resume_advance_action: String::new(),
                    park_hint: None,
                    conversation_hash: "<conversation_hash>".to_string(),
                };
                let actual_bytes = actual.encode_to_vec();
                let expected_bytes = expected.encode_to_vec();
                if actual_bytes == expected_bytes {
                    Ok(())
                } else {
                    Err(format!(
                        "Serialized RouteResponse differs from main golden: actual={:02x?}, expected={:02x?}",
                        actual_bytes, expected_bytes
                    ))
                }
            },
        ),
        check_def(
            "the route resolution outcome is {string}",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let expected = params
                    .get_string(0)
                    .ok_or("Expected resolution outcome")?
                    .to_string();
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) if resp.resolution_outcome == expected => Ok(()),
                    RouteRpcResult::Success(resp) => Err(format!(
                        "Expected resolution outcome '{}', got '{}'",
                        expected, resp.resolution_outcome
                    )),
                    RouteRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        // resume_aware_routing — resume RouteResponse field assertions.
        // continuation_recognition: WHICH path produced this resume. With a run
        // open, several paths can legitimately return a resume response — the
        // continuation procedure and two mid-run check-in nudges — so the
        // artifact id cannot tell them apart and the outcome label cannot either.
        // This is the only assertion that can.
        check_def(
            "the route resume source is {string}",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let expected = params
                    .get_string(0)
                    .ok_or("Expected resume source")?
                    .to_string();
                let result = ctx
                    .get::<RouteRpcResult>("route_result")
                    .ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) if resp.resume_source == expected => Ok(()),
                    RouteRpcResult::Success(resp) => Err(format!(
                        "Expected resume source '{}', got '{}'",
                        expected, resp.resume_source
                    )),
                    RouteRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the route resume artifact id is {string}",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected resume artifact id")?.to_string();
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) if resp.resume_artifact_id == expected => Ok(()),
                    RouteRpcResult::Success(resp) => Err(format!(
                        "Expected resume artifact id '{}', got '{}'",
                        expected, resp.resume_artifact_id
                    )),
                    RouteRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the route resume kind is {string}",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected resume kind")?.to_string();
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) if resp.resume_kind == expected => Ok(()),
                    RouteRpcResult::Success(resp) => Err(format!(
                        "Expected resume kind '{}', got '{}'",
                        expected, resp.resume_kind
                    )),
                    RouteRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the route resume state is {string}",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected resume state")?.to_string();
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) if resp.resume_state == expected => Ok(()),
                    RouteRpcResult::Success(resp) => Err(format!(
                        "Expected resume state '{}', got '{}'",
                        expected, resp.resume_state
                    )),
                    RouteRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the route resume advance action is {string}",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected resume advance action")?.to_string();
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) if resp.resume_advance_action == expected => Ok(()),
                    RouteRpcResult::Success(resp) => Err(format!(
                        "Expected resume advance action '{}', got '{}'",
                        expected, resp.resume_advance_action
                    )),
                    RouteRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the route resume guidance contains {string}",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected guidance substring")?.to_string();
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) if resp.resume_guidance.contains(&needle) => Ok(()),
                    RouteRpcResult::Success(resp) => Err(format!(
                        "Expected resume guidance to contain '{}', got '{}'",
                        needle, resp.resume_guidance
                    )),
                    RouteRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        // Scenario B (hearth self-heal): the routing-activity record lands in the
        // `__unattributed__` bucket UNDER the global hearth, not the global root.
        check_def(
            "the (unattributed) bucket under the global hearth has a routing-activity record",
            &[("global_playbooks_hearth_path", "PathBuf")],
            |ctx, _params| {
                let global = ctx
                    .get::<PathBuf>("global_playbooks_hearth_path")
                    .ok_or("No global_playbooks_hearth_path")?;
                let sink = global
                    .join("__unattributed__")
                    .join("routing-activity.jsonl");
                let contents = std::fs::read_to_string(&sink).map_err(|e| {
                    format!("expected sink at {}: {}", sink.display(), e)
                })?;
                if contents.trim().is_empty() {
                    return Err(format!("sink {} is empty", sink.display()));
                }
                Ok(())
            },
        ),
        check_def(
            "the global hearth root has no routing-activity record",
            &[("global_playbooks_hearth_path", "PathBuf")],
            |ctx, _params| {
                let global = ctx
                    .get::<PathBuf>("global_playbooks_hearth_path")
                    .ok_or("No global_playbooks_hearth_path")?;
                let sink = global.join("routing-activity.jsonl");
                match std::fs::read_to_string(&sink) {
                    Err(_) => Ok(()), // absent ⇒ nothing attributed to the root
                    Ok(c) if c.trim().is_empty() => Ok(()),
                    Ok(c) => Err(format!(
                        "global root sink {} unexpectedly has records: {}",
                        sink.display(),
                        c.trim()
                    )),
                }
            },
        ),
        check_def(
            "the route selected kind is {string}",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected selected kind")?.to_string();
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) if resp.selected_kind == expected => Ok(()),
                    RouteRpcResult::Success(resp) => Err(format!(
                        "Expected selected kind '{}', got '{}'",
                        expected, resp.selected_kind
                    )),
                    RouteRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "no route selected kind is set",
            &[("route_result", "RouteRpcResult")],
            |ctx, _params| {
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) if resp.selected_kind.is_empty() => Ok(()),
                    RouteRpcResult::Success(resp) => Err(format!(
                        "Expected no selected kind, got '{}'",
                        resp.selected_kind
                    )),
                    RouteRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the route matching candidates are exactly {string}",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let expected = parse_csv(
                    params
                        .get_string(0)
                        .ok_or("Expected matching candidates")?
                        .as_ref(),
                );
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) if resp.matching_candidates == expected => Ok(()),
                    RouteRpcResult::Success(resp) => Err(format!(
                        "Expected matching candidates {:?}, got {:?}",
                        expected, resp.matching_candidates
                    )),
                    RouteRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        // resume-signal context-awareness — park_hint RouteResponse field asserts.
        check_def(
            "the route park hint artifact id is {string}",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected park hint artifact id")?.to_string();
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) => match &resp.park_hint {
                        Some(p) if p.artifact_id == expected => Ok(()),
                        Some(p) => Err(format!(
                            "Expected park hint artifact id '{}', got '{}'",
                            expected, p.artifact_id
                        )),
                        None => Err(format!(
                            "Expected park hint artifact id '{}', but no park_hint is set",
                            expected
                        )),
                    },
                    RouteRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the route park hint kind is {string}",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected park hint kind")?.to_string();
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) => match &resp.park_hint {
                        Some(p) if p.kind == expected => Ok(()),
                        Some(p) => Err(format!("Expected park hint kind '{}', got '{}'", expected, p.kind)),
                        None => Err(format!("Expected park hint kind '{}', but no park_hint is set", expected)),
                    },
                    RouteRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the route park hint state is {string}",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected park hint state")?.to_string();
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) => match &resp.park_hint {
                        Some(p) if p.state == expected => Ok(()),
                        Some(p) => Err(format!("Expected park hint state '{}', got '{}'", expected, p.state)),
                        None => Err(format!("Expected park hint state '{}', but no park_hint is set", expected)),
                    },
                    RouteRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the route park hint park action is {string}",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected park hint action")?.to_string();
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) => match &resp.park_hint {
                        Some(p) if p.park_action == expected => Ok(()),
                        Some(p) => Err(format!(
                            "Expected park hint action '{}', got '{}'",
                            expected, p.park_action
                        )),
                        None => Err(format!(
                            "Expected park hint action '{}', but no park_hint is set",
                            expected
                        )),
                    },
                    RouteRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "no route park hint is set",
            &[("route_result", "RouteRpcResult")],
            |ctx, _params| {
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) if resp.park_hint.is_none() => Ok(()),
                    RouteRpcResult::Success(resp) => Err(format!(
                        "Expected no park_hint, got {:?}",
                        resp.park_hint
                    )),
                    RouteRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the route candidates include kind {string}",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) => {
                        if resp.candidates.iter().any(|c| c.kind == kind) { Ok(()) }
                        else { Err(format!("Expected kind '{}' in candidates {:?}", kind, resp.candidates.iter().map(|c| &c.kind).collect::<Vec<_>>())) }
                    }
                    RouteRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the route candidates do not include kind {string}",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) => {
                        if resp.candidates.iter().any(|c| c.kind == kind) {
                            Err(format!("Expected kind '{}' absent but found in candidates", kind))
                        } else { Ok(()) }
                    }
                    RouteRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the route candidate {string} carries a description and required_fields metadata",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) => {
                        match resp.candidates.iter().find(|c| c.kind == kind) {
                            Some(c) if !c.description.is_empty() => Ok(()),
                            Some(_) => Err(format!("Candidate '{}' has empty description", kind)),
                            None => Err(format!("Candidate '{}' not found", kind)),
                        }
                    }
                    RouteRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the route candidate {string} description is {string}",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let expected = params.get_string(1).ok_or("Expected description")?;
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) => {
                        match resp.candidates.iter().find(|c| c.kind == kind) {
                            Some(c) if c.description == expected.as_ref() as &str => Ok(()),
                            Some(c) => Err(format!(
                                "Expected candidate '{}' description '{}' but got '{}'",
                                kind, expected, c.description
                            )),
                            None => Err(format!("Candidate '{}' not found", kind)),
                        }
                    }
                    RouteRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the route candidate {string} description is not {string}",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let forbidden = params.get_string(1).ok_or("Expected description")?;
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) => {
                        match resp.candidates.iter().find(|c| c.kind == kind) {
                            Some(c) if c.description != forbidden.as_ref() as &str => Ok(()),
                            Some(c) => Err(format!(
                                "Expected candidate '{}' description not to be '{}'",
                                kind, c.description
                            )),
                            None => Err(format!("Candidate '{}' not found", kind)),
                        }
                    }
                    RouteRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the route candidates equal every driven seed and cached route machine",
            &[("route_result", "RouteRpcResult"), ("hearth_path", "PathBuf")],
            |ctx, _params| {
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let response = match result {
                    RouteRpcResult::Success(resp) => resp,
                    RouteRpcResult::Error { code, message } => {
                        return Err(format!("Expected success, got gRPC {}: {}", code, message));
                    }
                };
                let registry = CompositePlaybookRegistry::new(
                    HearthPlaybookRegistry::new(hearth),
                    SeedPlaybookRegistry,
                );
                let expected = driven_candidates(&registry)
                    .into_iter()
                    .map(|m| m.kind.clone())
                    .collect::<std::collections::BTreeSet<_>>();
                let actual = response
                    .candidates
                    .iter()
                    .map(|c| c.kind.clone())
                    .collect::<std::collections::BTreeSet<_>>();
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected default-safe route candidates {:?}, got {:?}",
                        expected, actual
                    ))
                }
            },
        ),
        check_def(
            "the route handoff is {string}",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected handoff")?.to_string();
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) if resp.handoff == expected => Ok(()),
                    RouteRpcResult::Success(resp) => Err(format!("Expected handoff '{}', got '{}'", expected, resp.handoff)),
                    RouteRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the route intent echoes {string}",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected intent")?.to_string();
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) if resp.intent == expected => Ok(()),
                    RouteRpcResult::Success(resp) => Err(format!("Expected intent '{}', got '{}'", expected, resp.intent)),
                    RouteRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the route call did not error",
            &[("route_result", "RouteRpcResult")],
            |ctx, _params| {
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(_) => Ok(()),
                    RouteRpcResult::Error { code, message } => Err(format!("Expected success (not an error), got gRPC {}: {}", code, message)),
                }
            },
        ),
        // ===== route_response_mirrors_begin Phase 2/3 — annotations + guidance =====
        check_def(
            "the route guidance contains {string}",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected substring")?.to_string();
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) if resp.guidance.contains(&needle) => Ok(()),
                    RouteRpcResult::Success(resp) => Err(format!(
                        "Expected guidance to contain {:?}, got {:?}", needle, resp.guidance
                    )),
                    RouteRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the route guidance is empty",
            &[("route_result", "RouteRpcResult")],
            |ctx, _params| {
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) if resp.guidance.is_empty() => Ok(()),
                    RouteRpcResult::Success(resp) => Err(format!(
                        "Expected empty guidance, got {:?}", resp.guidance
                    )),
                    RouteRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the route guidance equals the playbook hook body {string} for {string} in the route hearth",
            &[("route_result", "RouteRpcResult"), ("hearth_path", "PathBuf")],
            |ctx, params| {
                let filename = params.get_string(0).ok_or("Expected hook filename")?.to_string();
                let playbook = params.get_string(1).ok_or("Expected playbook id")?.to_string();
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let hook_path = hearth.join("playbooks").join(&playbook).join("hooks").join(&filename);
                let expected = std::fs::read_to_string(&hook_path)
                    .map_err(|e| format!("read hook {}: {}", hook_path.display(), e))?;
                match result {
                    RouteRpcResult::Success(resp) if resp.guidance == expected => Ok(()),
                    RouteRpcResult::Success(resp) => Err(format!(
                        "Expected guidance to equal hook body ({} bytes), got {} bytes: {:?}",
                        expected.len(), resp.guidance.len(), resp.guidance
                    )),
                    RouteRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the route candidate {string} intent contains {string}",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let needle = params.get_string(1).ok_or("Expected substring")?.to_string();
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) => match resp.candidates.iter().find(|c| c.kind == kind) {
                        Some(c) if c.intent.contains(&needle) => Ok(()),
                        Some(c) => Err(format!("Candidate {} intent {:?} does not contain {:?}", kind, c.intent, needle)),
                        None => Err(format!("Candidate {} not found", kind)),
                    },
                    RouteRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the route candidate {string} step_outline starts with {string} and has at least 1 step",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let first = params.get_string(1).ok_or("Expected first state")?.to_string();
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) => match resp.candidates.iter().find(|c| c.kind == kind) {
                        Some(c) if !c.step_outline.is_empty() && c.step_outline[0] == first => Ok(()),
                        Some(c) => Err(format!(
                            "Candidate {} step_outline {:?} does not start with {:?} (or is empty)",
                            kind, c.step_outline, first
                        )),
                        None => Err(format!("Candidate {} not found", kind)),
                    },
                    RouteRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the route candidate {string} route_triggers contain {string}",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let needle = params.get_string(1).ok_or("Expected substring")?.to_string();
                let result = ctx
                    .get::<RouteRpcResult>("route_result")
                    .ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) => {
                        match resp.candidates.iter().find(|c| c.kind == kind) {
                            Some(c) => {
                                if c.route_triggers
                                    .iter()
                                    .any(|trigger| trigger.contains(&needle))
                                {
                                    Ok(())
                                } else {
                                    Err(format!(
                                        "Candidate {} route_triggers {:?} do not contain {:?}",
                                        kind, c.route_triggers, needle
                                    ))
                                }
                            }
                            None => Err(format!("Candidate {} not found", kind)),
                        }
                    }
                    RouteRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the route candidate {string} why_fits contains {string}",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let needle = params.get_string(1).ok_or("Expected substring")?.to_string();
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) => match resp.candidates.iter().find(|c| c.kind == kind) {
                        Some(c) if c.why_fits.contains(&needle) => Ok(()),
                        Some(c) => Err(format!("Candidate {} why_fits {:?} does not contain {:?}", kind, c.why_fits, needle)),
                        None => Err(format!("Candidate {} not found", kind)),
                    },
                    RouteRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the route candidate {string} has no why_fits",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) => match resp.candidates.iter().find(|c| c.kind == kind) {
                        Some(c) if c.why_fits.is_empty() => Ok(()),
                        Some(c) => Err(format!("Candidate {} expected empty why_fits, got {:?}", kind, c.why_fits)),
                        None => Err(format!("Candidate {} not found", kind)),
                    },
                    RouteRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        // route_response_mirrors_begin H2 — budget truncation assertions. The
        // serialized size of a candidate is measured the SAME way the budget
        // accounts for it: kind + description + route_triggers + intent +
        // why_fits + step_outline.
        check_def(
            "every route candidate has its step_outline dropped",
            &[("route_result", "RouteRpcResult")],
            |ctx, _params| {
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) => {
                        if resp.candidates.is_empty() {
                            return Err("expected a non-empty candidate set".to_string());
                        }
                        for c in &resp.candidates {
                            if !c.step_outline.is_empty() {
                                return Err(format!(
                                    "candidate {} step_outline not dropped: {:?}",
                                    c.kind, c.step_outline
                                ));
                            }
                        }
                        Ok(())
                    }
                    RouteRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        check_def(
            "the route candidate set total is within {int} bytes",
            &[("route_result", "RouteRpcResult")],
            |ctx, params| {
                let budget = params.get_int(0).ok_or("Expected budget")? as usize;
                let result = ctx.get::<RouteRpcResult>("route_result").ok_or("No route_result")?;
                match result {
                    RouteRpcResult::Success(resp) => {
                        let total: usize = resp
                            .candidates
                            .iter()
                            .map(|c| {
                                c.kind.len()
                                    + c.description.len()
                                    + c.route_triggers.iter().map(String::len).sum::<usize>()
                                    + c.intent.len()
                                    + c.why_fits.len()
                                    + c.step_outline.iter().map(String::len).sum::<usize>()
                            })
                            .sum();
                        if total <= budget {
                            Ok(())
                        } else {
                            Err(format!(
                                "candidate set serialized total {} exceeds budget {}",
                                total, budget
                            ))
                        }
                    }
                    RouteRpcResult::Error { code, message } => Err(format!("Expected success, got gRPC {}: {}", code, message)),
                }
            },
        ),
        // ===== Builder (playbook_generation) e2e + measurement (AC4/AC5) =====
        // Seed an engine hearth holding the builder machine + hooks/gathering.md
        // + an ACTIVE parent track (the builder's parent_kind: track makes
        // begin-create enforce parent active + kind track).
        step_def(
            "a hearth seeded with the builder machine and an active parent track",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, _params| {
                let (handle, tmp) = retained_temp_dir("anvil-builder-engine-")?;
                seed_builder_engine_hearth(&tmp)?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a hearth seeded with an evidence-compliant builder machine and an active parent track",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, _params| {
                let (handle, tmp) = retained_temp_dir("anvil-builder-evidence-engine-")?;
                seed_evidence_compliant_builder_engine_hearth(&tmp)?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        async_step_def(
            "a free candidate carrying evidence obligations is protobuf round-tripped and submitted",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("candidate_wire_succeeded", "bool"),
                ("candidate_wire_status", "String"),
                ("candidate_wire_error", "String"),
                ("candidate_wire_context", "JsonValue"),
                ("candidate_wire_bytes_unchanged", "bool"),
            ],
            |ctx, _params| async move {
                submit_candidate_wire_fixture(
                    ctx,
                    Some("free"),
                    &["verifiable_citation", "artifact_of_consequence"],
                    false,
                )
                .await
            },
        ),
        async_step_def(
            "a legacy candidate payload is protobuf round-tripped and submitted",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("candidate_wire_succeeded", "bool"),
                ("candidate_wire_status", "String"),
                ("candidate_wire_error", "String"),
                ("candidate_wire_context", "JsonValue"),
                ("candidate_wire_bytes_unchanged", "bool"),
            ],
            |ctx, _params| async move {
                submit_candidate_wire_fixture(ctx, None, &[], true).await
            },
        ),
        async_step_def(
            "a candidate carrying unknown evidence class {string} is protobuf round-tripped and submitted",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("candidate_wire_succeeded", "bool"),
                ("candidate_wire_status", "String"),
                ("candidate_wire_error", "String"),
                ("candidate_wire_context", "JsonValue"),
                ("candidate_wire_bytes_unchanged", "bool"),
            ],
            |ctx, params| {
                let evidence_class = params.get_string(0).map(ToString::to_string);
                async move {
                    let evidence_class = evidence_class.ok_or("Expected evidence class")?;
                    submit_candidate_wire_fixture(
                        ctx,
                        Some("driven"),
                        &[evidence_class.as_str()],
                        false,
                    )
                    .await
                }
            },
        ),
        async_step_def(
            "a candidate carrying unknown register {string} is protobuf round-tripped and submitted",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("candidate_wire_succeeded", "bool"),
                ("candidate_wire_status", "String"),
                ("candidate_wire_error", "String"),
                ("candidate_wire_context", "JsonValue"),
                ("candidate_wire_bytes_unchanged", "bool"),
            ],
            |ctx, params| {
                let register = params.get_string(0).map(ToString::to_string);
                async move {
                    let register = register.ok_or("Expected register")?;
                    submit_candidate_wire_fixture(ctx, Some(register.as_str()), &[], false).await
                }
            },
        ),
        check_def(
            "the candidate wire intake succeeds",
            &[("candidate_wire_succeeded", "bool")],
            |ctx, _params| {
                if *ctx
                    .get::<bool>("candidate_wire_succeeded")
                    .ok_or("No candidate wire result")?
                {
                    Ok(())
                } else {
                    let status = ctx
                        .get::<String>("candidate_wire_status")
                        .map(String::as_str)
                        .unwrap_or("");
                    let error = ctx
                        .get::<String>("candidate_wire_error")
                        .map(String::as_str)
                        .unwrap_or("");
                    Err(format!(
                        "Expected candidate wire intake to succeed, got {}: {}",
                        status, error
                    ))
                }
            },
        ),
        check_def(
            "the legacy protobuf payload bytes are unchanged",
            &[("candidate_wire_bytes_unchanged", "bool")],
            |ctx, _params| {
                if *ctx
                    .get::<bool>("candidate_wire_bytes_unchanged")
                    .ok_or("No candidate wire byte comparison")?
                {
                    Ok(())
                } else {
                    Err("Legacy protobuf bytes changed after decode/re-encode".to_string())
                }
            },
        ),
        check_def(
            "the mapped candidate register is {string}",
            &[("candidate_wire_context", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected register")?;
                let candidate_json = candidate_from_wire_context(&ctx)?;
                let candidate = serde_json::from_value::<
                    anvil_core::domain::playbook::candidate::CandidatePlaybook,
                >(candidate_json.clone())
                .map_err(|error| format!("Deserialize mapped candidate: {}", error))?;
                let actual = match candidate.register {
                    anvil_core::domain::playbook::types::Register::Driven => "driven",
                    anvil_core::domain::playbook::types::Register::Free => "free",
                };
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected mapped candidate register '{}', got '{}'. Candidate: {}",
                        expected, actual, candidate_json
                    ))
                }
            },
        ),
        check_def(
            "mapped proposed state {string} has ordered evidence obligations:",
            &[("candidate_wire_context", "JsonValue")],
            |ctx, params| {
                let state = params.get_string(0).ok_or("Expected state")?;
                let table = params.data_table().ok_or("Expected evidence obligation table")?;
                let class_idx = table
                    .headers
                    .iter()
                    .position(|header| header == "evidence_class")
                    .ok_or("Missing evidence_class column")?;
                let expected = table
                    .rows
                    .iter()
                    .map(|row| {
                        row.get(class_idx)
                            .cloned()
                            .ok_or_else(|| "Missing evidence_class cell".to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let proposed = proposed_state_from_wire_context(&ctx, state)?;
                let actual = proposed
                    .get("evidence_obligation")
                    .and_then(serde_json::Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(serde_json::Value::as_str)
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected ordered evidence obligations {:?}, got {:?}. Proposed state: {}",
                        expected, actual, proposed
                    ))
                }
            },
        ),
        check_def(
            "mapped proposed state {string} has no evidence obligations",
            &[("candidate_wire_context", "JsonValue")],
            |ctx, params| {
                let state = params.get_string(0).ok_or("Expected state")?;
                let proposed = proposed_state_from_wire_context(&ctx, state)?;
                let is_empty = proposed
                    .get("evidence_obligation")
                    .and_then(serde_json::Value::as_array)
                    .map(|items| items.is_empty())
                    .unwrap_or(true);
                if is_empty {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected no evidence obligations. Proposed state: {}",
                        proposed
                    ))
                }
            },
        ),
        check_def(
            "the serialized legacy candidate omits extension defaults",
            &[("candidate_wire_context", "JsonValue")],
            |ctx, _params| {
                let candidate = candidate_from_wire_context(&ctx)?;
                if candidate.get("register").is_some() {
                    return Err(format!(
                        "Expected legacy candidate serialization to omit register. Candidate: {}",
                        candidate
                    ));
                }
                let proposed = proposed_state_from_wire_context(&ctx, "triage")?;
                if proposed.get("evidence_obligation").is_some() {
                    return Err(format!(
                        "Expected legacy proposed-state serialization to omit evidence_obligation. Proposed state: {}",
                        proposed
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the candidate wire intake is refused with gRPC status {string}",
            &[("candidate_wire_succeeded", "bool"), ("candidate_wire_status", "String")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected gRPC status")?;
                let succeeded = ctx
                    .get::<bool>("candidate_wire_succeeded")
                    .ok_or("No candidate wire result")?;
                let actual = ctx
                    .get::<String>("candidate_wire_status")
                    .ok_or("No candidate wire status")?;
                if !*succeeded && actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected candidate wire refusal {}, got succeeded={} status='{}'",
                        expected, succeeded, actual
                    ))
                }
            },
        ),
        check_def(
            "the candidate wire intake error names {string}",
            &[("candidate_wire_error", "String")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected error token")?;
                let actual = ctx
                    .get::<String>("candidate_wire_error")
                    .ok_or("No candidate wire error")?;
                if actual.contains(expected) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected candidate wire error to name '{}', got '{}'",
                        expected, actual
                    ))
                }
            },
        ),
        // ===== daily_recap BP2: registry-resolvable + measured engine drive =====
        step_def(
            "a hearth seeded with the daily_recap machine",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, _params| {
                let (handle, tmp) = retained_temp_dir("anvil-daily-recap-engine-")?;
                seed_daily_recap_engine_hearth(&tmp)?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        check_def(
            "the temp owner-home registry resolves daily_recap with states and measurements",
            &[("hearth_path", "PathBuf")],
            |ctx, _params| {
                use anvil_core::domain::playbook::registry::PlaybookRegistry;
                let owner_home = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let registry = HearthPlaybookRegistry::new(owner_home.clone());
                if !registry.invalid_artifacts().is_empty() {
                    return Err(format!(
                        "daily_recap registry load errors: {:?}",
                        registry.invalid_artifacts()
                    ));
                }
                let machine = registry
                    .machine_for("daily_recap")
                    .ok_or("daily_recap was not registry-resolvable from temp owner-home")?;
                let expected_states = [
                    ("gathering", "doer"),
                    ("gathering_review", "reviewer"),
                    ("synthesizing", "doer"),
                    ("synthesizing_review", "reviewer"),
                    ("reporting", "doer"),
                    ("reporting_review", "reviewer"),
                    ("completed", ""),
                ];
                let mut errors = Vec::new();
                for (state_name, role) in expected_states {
                    let Some(state) = machine.states.iter().find(|s| s.name == state_name) else {
                        errors.push(format!("missing state {}", state_name));
                        continue;
                    };
                    if !role.is_empty() {
                        match state.measurement_by_role.get(role) {
                            Some(measurement)
                                if !measurement.intent.is_empty()
                                    && !measurement.expected_output.is_empty() => {}
                            Some(_) => errors.push(format!(
                                "{} {} measurement has empty fields",
                                state_name, role
                            )),
                            None => errors.push(format!(
                                "{} missing {} measurement",
                                state_name, role
                            )),
                        }
                    }
                }
                let happy_path = [
                    ("gathering", "gathering_review", "doer", None),
                    (
                        "gathering_review",
                        "synthesizing",
                        "reviewer",
                        Some("satisfied"),
                    ),
                    ("synthesizing", "synthesizing_review", "doer", None),
                    (
                        "synthesizing_review",
                        "reporting",
                        "reviewer",
                        Some("satisfied"),
                    ),
                    ("reporting", "reporting_review", "doer", None),
                    (
                        "reporting_review",
                        "completed",
                        "reviewer",
                        Some("satisfied"),
                    ),
                ];
                for (from, to, role, satisfaction) in happy_path {
                    let found = machine.transitions.iter().any(|transition| {
                        transition.from_state == from
                            && transition.to_state == to
                            && transition.required_role == role
                            && match (&transition.required_satisfaction, satisfaction) {
                                (None, None) => true,
                                (Some(actual), Some(expected)) => {
                                    actual == &[expected.to_string()]
                                }
                                _ => false,
                            }
                    });
                    if !found {
                        errors.push(format!(
                            "missing happy-path transition {} -> {} role {} satisfaction {:?}",
                            from, to, role, satisfaction
                        ));
                    }
                }
                if errors.is_empty() {
                    Ok(())
                } else {
                    Err(errors.join("; "))
                }
            },
        ),
        async_step_def(
            "the engine begins, checks in, and drives a daily_recap artifact from gathering to completed",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("e2e_artifact_path", "String"),
                ("e2e_final_state", "String"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let hearth_path = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let mut client =
                    anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr)
                        .await
                        .map_err(|e| format!("Connection failed: {}", e))?;

                let begin_resp = client
                    .begin(crate::surfaced(anvil_engine::proto::BeginRequest {
                        hearth_path: String::new(),
                        artifact_type: "daily_recap".to_string(),
                        parent_id: String::new(),
                        track_name: "daily recap e2e".to_string(),
                        playbook_name: String::new(),
                        target_owner: String::new(),
                        approver: String::new(),
                        actor_name: "Daily-Recap-E2E-100000".to_string(),
                        actor_type: "agent".to_string(),
                        actor_model: "test".to_string(),
                        actor_provider: "test".to_string(),
                        actor_context_window: 0,
                        actor_sdk_version: String::new(),
                        actor_entrypoint: String::new(),
                        identifier: String::new(),
                        session_role: "creator".to_string(),
                        // daily_recap is consulting-kit-owned (access.org=Consulting, not the
                        // Foundation wildcard), so begin requires a consulting-granted principal.
                        ctx_org: "Consulting".to_string(),
                        ctx_space: String::new(),
                        ctx_role: "read".to_string(),
                        ctx_clearance: "internal".to_string(),
                        ..Default::default()
                    }))
                    .await
                    .map_err(|s| {
                        format!(
                            "begin failed: {}: {}",
                            grpc_code_name(s.code()),
                            s.message()
                        )
                    })?
                    .into_inner();
                let artifact_path = begin_resp_track_path(&begin_resp);
                if begin_resp.state != "gathering" {
                    return Err(format!(
                        "expected create state gathering, got '{}'",
                        begin_resp.state
                    ));
                }

                client
                    .checkin(crate::surfaced(anvil_engine::proto::CheckinRequest {
                        hearth_path: String::new(),
                        role: "creator".to_string(),
                        actor_type: "agent".to_string(),
                        actor_model: "test".to_string(),
                        actor_provider: "test".to_string(),
                        actor_name: "Daily-Recap-E2E-100000".to_string(),
                    }))
                    .await
                    .map_err(|s| {
                        format!(
                            "checkin failed: {}: {}",
                            grpc_code_name(s.code()),
                            s.message()
                        )
                    })?
                    .into_inner();

                let hops: &[(&str, &str)] = &[
                    ("", "gathering_review"),
                    ("satisfied", "synthesizing"),
                    ("", "synthesizing_review"),
                    ("satisfied", "reporting"),
                    ("", "reporting_review"),
                    ("satisfied", "completed"),
                ];
                let final_state = drive_daily_recap_hops(&mut client, &artifact_path, hops).await?;

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth_path);
                out.set("e2e_artifact_path", artifact_path);
                out.set("e2e_final_state", final_state);
                Ok(out)
            },
        ),
        // ===== target_owner begin (Anvil-lane 1b, A1/A2) =====
        // Begin-create a playbook_generation against the active builder parent
        // with a given target_owner; stores e2e_artifact_path + hearth_path so
        // the existing raw status.yaml-contains check can assert the recorded
        // value. begin_result also captured for error assertions.
        async_step_def(
            "the begin RPC creates a playbook_generation with target_owner {string}",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("begin_result", "BeginRpcResult"),
                ("e2e_artifact_path", "String"),
            ],
            |mut ctx, params| async move {
                let target_owner = params.get_string(0).ok_or("Expected target_owner")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let (result, artifact_path) =
                    match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                        Ok(mut client) => {
                            let request = crate::surfaced(anvil_engine::proto::BeginRequest {
                                hearth_path: String::new(),
                                artifact_type: "playbook_generation".to_string(),
                                parent_id: BUILDER_PARENT_TRACK_ID.to_string(),
                                track_name: "target owner e2e".to_string(),
                                playbook_name: "temper".to_string(),
                                target_owner,
                                approver: "Approver-E2E".to_string(),
                                actor_name: "Builder-E2E-100000".to_string(),
                                actor_type: "agent".to_string(),
                                actor_model: "claude-opus-4-8".to_string(),
                                actor_provider: "anthropic".to_string(),
                                actor_context_window: 0,
                                actor_sdk_version: String::new(),
                                actor_entrypoint: String::new(),
                                identifier: String::new(),
                                session_role: "creator".to_string(),
                                ctx_org: "Foundation".to_string(),
                                ctx_space: String::new(),
                                ctx_role: "read".to_string(),
                                ctx_clearance: "internal".to_string(),
                            ..Default::default()
                            });
                            match client.begin(request).await {
                                Ok(response) => {
                                    let inner = response.into_inner();
                                    let path = begin_resp_track_path(&inner);
                                    (BeginRpcResult::Success(inner), path)
                                }
                                Err(status) => (
                                    BeginRpcResult::Error {
                                        code: grpc_code_name(status.code()),
                                        message: status.message().to_string(),
                                    },
                                    String::new(),
                                ),
                            }
                        }
                        Err(e) => (
                            BeginRpcResult::Error {
                                code: "UNAVAILABLE".to_string(),
                                message: format!("Connection failed: {}", e),
                            },
                            String::new(),
                        ),
                    };
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth_path);
                out.set("begin_result", result);
                out.set("e2e_artifact_path", artifact_path);
                Ok(out)
            },
        ),
        // ===== generic create_fields begin (kit-action adoption dogfood) =====
        step_def(
            "a hearth seeded with a lore_query machine requiring question and requester",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, _params| {
                let (handle, tmp) = retained_temp_dir("anvil-lore-query-engine-")?;
                seed_lore_query_generic_fields_hearth(&tmp)?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        async_step_def(
            "the begin RPC creates a lore_query with no create_fields",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("begin_result", "BeginRpcResult"),
                ("e2e_artifact_path", "String"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let (result, artifact_path) =
                    begin_lore_query_rpc(engine.port, std::collections::HashMap::new()).await;
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth_path);
                out.set("begin_result", result);
                out.set("e2e_artifact_path", artifact_path);
                Ok(out)
            },
        ),
        async_step_def(
            "the begin RPC creates a lore_query with create_fields question {string} requester {string}",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("begin_result", "BeginRpcResult"),
                ("e2e_artifact_path", "String"),
            ],
            |mut ctx, params| async move {
                let question = params.get_string(0).ok_or("Expected question")?.to_string();
                let requester = params.get_string(1).ok_or("Expected requester")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let mut fields = std::collections::HashMap::new();
                fields.insert("question".to_string(), question);
                fields.insert("requester".to_string(), requester);
                let (result, artifact_path) = begin_lore_query_rpc(engine.port, fields).await;
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth_path);
                out.set("begin_result", result);
                out.set("e2e_artifact_path", artifact_path);
                Ok(out)
            },
        ),
        // ===== K5 supervision-bind seam steps (R8/R9/R10) =====
        // Prove the dark-by-default K5 hardening at the REAL gRPC engine seam: the
        // R10 bind-time bindability precondition (create-or-nothing), the R9
        // terminal-resolve no-op on Complete/Snapshot, and that the whole
        // precondition is inert when `ANVIL_K5_BIND` is unset. Seeds a `k5_probe`
        // driven machine (one of several shapes) into a throwaway hearth, so the
        // begin/complete/snapshot RPC steps already in this module drive it.
        step_def(
            "a hearth seeded with the K5 {string} machine",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let shape = params
                    .get_string(0)
                    .ok_or("Expected a K5 machine shape name")?
                    .to_string();
                let (handle, tmp) = retained_temp_dir("anvil-k5-probe-engine-")?;
                seed_k5_probe_hearth(&tmp, &shape)?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        // K5 flag-ON engine spawn: identical to "started with that hearth" but flips
        // ANVIL_K5_BIND=1 so the R10 bindability precondition + the R9
        // terminal-resolve no-op guard are active in the engine process. The base
        // `anvil_engine_command` env_removes the flag, so the explicit `.env` below
        // is the ONLY thing that turns the seam on — no default subprocess inherits
        // it (A7). Also strips the hook-install side effect like the other flag-on
        // spawns.
        step_def(
            "the engine is started with that hearth and K5 supervision bind on",
            &[("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("engine_port", "u16"),
                ("kiln_router_port", "i64"),
            ],
            |ctx, _params| {
                let hearth_path = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();

                let port = free_port()?;
                let binary = crate::harness::binary_path("anvil-engine");
                let temper_home = hearth_path.join("__temper_home__");

                let child = anvil_engine_command(&binary)
                    .arg("--hearth")
                    .arg(hearth_path.to_str().unwrap())
                    .arg("--port")
                    .arg(port.to_string())
                    .env("ANVIL_TEMPER_HOME", temper_home.to_str().unwrap())
                    .env("ANVIL_TELEMETRY_SALT", "brine-test-salt")
                    .env("ANVIL_K5_BIND", "1")
                    .env("ANVIL_SKIP_HOOK_INSTALL", "1")
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap_or_else(|e| {
                        panic!(
                            "Failed to start anvil-engine at {}: {}",
                            binary.display(),
                            e
                        )
                    });

                if !wait_until_grpc_ready(port) {
                    return Err(format!(
                        "anvil-engine did not answer gRPC HealthCheck on 127.0.0.1:{} within timeout",
                        port
                    ));
                }

                let mut process = EngineProcess::new(child, port);
                retain_ctx_temp_dir(&mut process, &ctx, "hearth_path_handle");
                let mut out = Context::new();
                out.set("engine_process", process);
                out.set("hearth_path", hearth_path);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                out.set("engine_port", port);
                if let Some(kiln_port) = ctx.get::<i64>("kiln_router_port") {
                    out.set("kiln_router_port", *kiln_port);
                }
                Ok(out)
            },
        ),
        // A5 idempotency: snapshot the freshly-resolved instance's directory
        // (status.yaml + transition ledger), issue a REPEAT plain-doer Complete,
        // and assert the on-disk tree is byte-identical — the R9 no-op must write
        // NO second transition. Self-contained (record -> repeat -> compare) so an
        // intervening RPC step can never drop the recorded tree from context.
        async_step_def(
            "a repeat Complete on the bound instance is a byte-identical no-op",
            &[
                ("engine_process", "EngineProcess"),
                ("begin_result", "BeginRpcResult"),
                ("hearth_path", "PathBuf"),
            ],
            &[
                ("engine_process", "EngineProcess"),
                ("begin_result", "BeginRpcResult"),
                ("hearth_path", "PathBuf"),
                ("complete_rpc_result", "CompleteRpcResult"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let begin_result = ctx
                    .get::<BeginRpcResult>("begin_result")
                    .ok_or("No begin_result")?
                    .clone();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let artifact_path = match &begin_result {
                    BeginRpcResult::Success(r) => r.track_path.clone(),
                    BeginRpcResult::Error { code, message } => {
                        return Err(format!("Expected begin success, got gRPC {}: {}", code, message));
                    }
                };
                let before = snapshot_tree(&hearth.join(&artifact_path));
                let result =
                    call_complete_with_bearer(&engine, None, "Rpc-Test-000000", &artifact_path, "")
                        .await;
                let after = snapshot_tree(&hearth.join(&artifact_path));
                if before != after {
                    return Err(format!(
                        "repeat Complete mutated the instance ledger ({} files before, {} after): the R9 no-op must write no second transition",
                        before.len(),
                        after.len()
                    ));
                }
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("begin_result", begin_result);
                out.set("hearth_path", hearth);
                out.set("complete_rpc_result", result);
                Ok(out)
            },
        ),
        // A6 idempotency: same proof for the abandon cancel leg — a REPEAT
        // Snapshot(to_state:"abandoned") on the already-abandoned instance must be
        // a byte-identical no-op (R9), not a second free-form ledger write.
        async_step_def(
            "a repeat abandon Snapshot on the bound instance is a byte-identical no-op",
            &[
                ("engine_process", "EngineProcess"),
                ("begin_result", "BeginRpcResult"),
                ("hearth_path", "PathBuf"),
            ],
            &[
                ("engine_process", "EngineProcess"),
                ("begin_result", "BeginRpcResult"),
                ("hearth_path", "PathBuf"),
                ("snapshot_rpc_result", "SnapshotRpcResult"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let begin_result = ctx
                    .get::<BeginRpcResult>("begin_result")
                    .ok_or("No begin_result")?
                    .clone();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let artifact_path = match &begin_result {
                    BeginRpcResult::Success(r) => r.track_path.clone(),
                    BeginRpcResult::Error { code, message } => {
                        return Err(format!("Expected begin success, got gRPC {}: {}", code, message));
                    }
                };
                let before = snapshot_tree(&hearth.join(&artifact_path));
                let req = anvil_engine::proto::SnapshotRequest {
                    hearth_path: String::new(),
                    artifact_path: artifact_path.clone(),
                    to_state: "abandoned".to_string(),
                    actor_name: "Rpc-Snapshot-000000".to_string(),
                    actor_role: "doer".to_string(),
                    approver: String::new(),
                    note: String::new(),
                    actor_type: "agent".to_string(),
                    actor_model: "claude-opus-4-8".to_string(),
                    actor_provider: "anthropic".to_string(),
                    actor_context_window: 0,
                    actor_sdk_version: String::new(),
                    actor_entrypoint: String::new(),
                    projection_only: false,
                    event_type: String::new(),
                    conversation_id: String::new(),
                    project_root: String::new(),
                    claimed_evidence: Vec::new(),
                };
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => match client.snapshot(crate::surfaced(req)).await {
                        Ok(response) => SnapshotRpcResult::Success(response.into_inner()),
                        Err(status) => SnapshotRpcResult::Error {
                            code: grpc_code_name(status.code()),
                            message: status.message().to_string(),
                        },
                    },
                    Err(e) => SnapshotRpcResult::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };
                let after = snapshot_tree(&hearth.join(&artifact_path));
                if before != after {
                    return Err(format!(
                        "repeat abandon Snapshot mutated the instance ledger ({} files before, {} after): the R9 no-op must write no second transition",
                        before.len(),
                        after.len()
                    ));
                }
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("begin_result", begin_result);
                out.set("hearth_path", hearth);
                out.set("snapshot_rpc_result", result);
                Ok(out)
            },
        ),
        // A1 exactly-one: count the instance directories the bind scaffolded under
        // a driven kind's directory (0 on a create-or-nothing reject; 1 on a bind).
        check_def(
            "the hearth contains exactly {int} artifact directories under {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected a count")? as usize;
                let dir = params.get_string(1).ok_or("Expected a directory")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let path = hearth.join(&dir);
                let mut dirs = Vec::new();
                if path.exists() {
                    for entry in std::fs::read_dir(&path)
                        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?
                    {
                        let entry = entry.map_err(|e| e.to_string())?;
                        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
                            dirs.push(entry.file_name().to_string_lossy().to_string());
                        }
                    }
                }
                if dirs.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected exactly {} artifact director(y/ies) under {}, found {}: {:?}",
                        expected,
                        dir,
                        dirs.len(),
                        dirs
                    ))
                }
            },
        ),
        // A2 idempotent bind (R2): a SECOND begin correlated to the same run
        // (conversation_id) as the instance already in `begin_result` must resolve
        // and return the SAME instance, minting no second. Self-contained (reads the
        // first bind from context, issues the repeat, asserts identity) so an
        // intervening step can never drop the first track_path — mirrors the A5/A6
        // byte-identity steps. The final artifact-dir count proves no second mint.
        async_step_def(
            "a repeat begin on conversation {string} to create a {string} artifact resolves to the same bound instance",
            &[
                ("engine_process", "EngineProcess"),
                ("begin_result", "BeginRpcResult"),
                ("hearth_path", "PathBuf"),
            ],
            &[
                ("engine_process", "EngineProcess"),
                ("begin_result", "BeginRpcResult"),
                ("hearth_path", "PathBuf"),
            ],
            |mut ctx, params| async move {
                let conversation_id =
                    params.get_string(0).ok_or("Expected conversation_id")?.to_string();
                let artifact_type =
                    params.get_string(1).ok_or("Expected artifact_type")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let first = ctx
                    .get::<BeginRpcResult>("begin_result")
                    .ok_or("No begin_result")?
                    .clone();
                let first_track_path = match &first {
                    BeginRpcResult::Success(r) => r.track_path.clone(),
                    BeginRpcResult::Error { code, message } => {
                        return Err(format!(
                            "Expected the first bind to succeed, got gRPC {}: {}",
                            code, message
                        ));
                    }
                };
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::BeginRequest {
                            hearth_path: String::new(),
                            artifact_type,
                            parent_id: String::new(),
                            track_name: "repeat bind".to_string(),
                            playbook_name: String::new(),
                            target_owner: String::new(),
                            approver: String::new(),
                            actor_name: "Rpc-Test-000000".to_string(),
                            actor_type: "agent".to_string(),
                            actor_model: "test".to_string(),
                            actor_provider: "test".to_string(),
                            actor_context_window: 0,
                            actor_sdk_version: String::new(),
                            actor_entrypoint: String::new(),
                            identifier: String::new(),
                            session_role: "creator".to_string(),
                            ctx_org: "Foundation".to_string(),
                            ctx_space: String::new(),
                            ctx_role: "read".to_string(),
                            ctx_clearance: "internal".to_string(),
                            conversation_id,
                            ..Default::default()
                        });
                        match client.begin(request).await {
                            Ok(response) => BeginRpcResult::Success(response.into_inner()),
                            Err(status) => BeginRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => BeginRpcResult::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };
                let second_track_path = match &result {
                    BeginRpcResult::Success(r) => r.track_path.clone(),
                    BeginRpcResult::Error { code, message } => {
                        return Err(format!(
                            "Expected the repeat bind to succeed, got gRPC {}: {}",
                            code, message
                        ));
                    }
                };
                if second_track_path != first_track_path {
                    return Err(format!(
                        "R2 idempotent bind violated: repeat begin on the same conversation returned a DIFFERENT instance ('{}' vs first '{}') — a second instance was minted",
                        second_track_path, first_track_path
                    ));
                }
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("begin_result", result);
                out.set("hearth_path", hearth_path);
                Ok(out)
            },
        ),
        // §4.3a actor-identity class at the K5 create seam: a playbook-creation begin
        // with one blanked actor_* field is rejected create-or-nothing. Mirrors the
        // plain create-no-parent step but zeroes the named actor field so the reject
        // is provably the actor precondition.
        async_step_def(
            "the begin RPC is called to create a {string} artifact named {string} with no parent and empty {string}",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[("engine_process", "EngineProcess"), ("begin_result", "BeginRpcResult"), ("hearth_path", "PathBuf")],
            |mut ctx, params| async move {
                let artifact_type = params.get_string(0).ok_or("Expected artifact_type")?.to_string();
                let name = params.get_string(1).ok_or("Expected name")?.to_string();
                let empty_field = params.get_string(2).ok_or("Expected empty field name")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let mut actor_name = "Rpc-Test-000000".to_string();
                let mut actor_type = "agent".to_string();
                let mut actor_model = "test".to_string();
                let mut actor_provider = "test".to_string();
                match empty_field.as_str() {
                    "actor_name" => actor_name = String::new(),
                    "actor_type" => actor_type = String::new(),
                    "actor_model" => actor_model = String::new(),
                    "actor_provider" => actor_provider = String::new(),
                    other => return Err(format!("Unsupported empty actor field '{}'", other)),
                }
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = crate::surfaced(anvil_engine::proto::BeginRequest {
                            hearth_path: String::new(),
                            artifact_type,
                            parent_id: String::new(),
                            track_name: name,
                            playbook_name: String::new(),
                            target_owner: String::new(),
                            approver: String::new(),
                            actor_name,
                            actor_type,
                            actor_model,
                            actor_provider,
                            actor_context_window: 0,
                            actor_sdk_version: String::new(),
                            actor_entrypoint: String::new(),
                            identifier: String::new(),
                            session_role: "creator".to_string(),
                            ctx_org: "Foundation".to_string(),
                            ctx_space: String::new(),
                            ctx_role: "read".to_string(),
                            ctx_clearance: "internal".to_string(),
                            ..Default::default()
                        });
                        match client.begin(request).await {
                            Ok(response) => BeginRpcResult::Success(response.into_inner()),
                            Err(status) => BeginRpcResult::Error {
                                code: grpc_code_name(status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => BeginRpcResult::Error { code: "UNAVAILABLE".to_string(), message: format!("Connection failed: {}", e) },
                };
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("begin_result", result);
                out.set("hearth_path", hearth_path);
                Ok(out)
            },
        ),
        // AC4 HEADLINE: begin-create a playbook_generation artifact against the
        // active parent track, then drive the 14-hop happy path to `completed`,
        // asserting each hop's machine-declared new_state internally.
        async_step_def(
            "the engine drives a playbook_generation artifact from gathering to completed",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("e2e_artifact_path", "String"),
                ("e2e_final_state", "String"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let mut client = anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr)
                    .await
                    .map_err(|e| format!("Connection failed: {}", e))?;

                let begin_resp = client
                    .begin(crate::surfaced(anvil_engine::proto::BeginRequest {
                        hearth_path: String::new(),
                        artifact_type: "playbook_generation".to_string(),
                        parent_id: BUILDER_PARENT_TRACK_ID.to_string(),
                        track_name: "builder e2e".to_string(),
                        playbook_name: "temper".to_string(),
                        target_owner: "kit:test-owner".to_string(),
                        approver: "Approver-E2E".to_string(),
                        actor_name: "Builder-E2E-100000".to_string(),
                        actor_type: "agent".to_string(),
                        actor_model: "claude-opus-4-8".to_string(),
                        actor_provider: "anthropic".to_string(),
                        actor_context_window: 0,
                        actor_sdk_version: String::new(),
                        actor_entrypoint: String::new(),
                        identifier: String::new(),
                        session_role: "creator".to_string(),
                        ctx_org: "Foundation".to_string(),
                        ctx_space: String::new(),
                        ctx_role: "read".to_string(),
                        ctx_clearance: "internal".to_string(),
                    ..Default::default()
                    }))
                    .await
                    .map_err(|s| format!("begin failed: {}: {}", grpc_code_name(s.code()), s.message()))?
                    .into_inner();
                let artifact_path = begin_resp_track_path(&begin_resp);
                if begin_resp.state != "gathering" {
                    return Err(format!("expected create state gathering, got '{}'", begin_resp.state));
                }

                let hops: &[(&str, &str)] = &[
                    ("", "gathering_review"),
                    ("approved", "analyzing"),
                    ("", "analyze_review"),
                    ("approved", "modeling"),
                    ("", "model_review"),
                    ("approved", "testing"),
                    ("", "test_review"),
                    ("approved", "trial_run"),
                    ("", "trial_review"),
                    ("approved", "reflecting"),
                    ("", "reflection_review"),
                    ("approved", "evolving"),
                    ("", "evolve_review"),
                    ("approved", "completed"),
                ];
                let final_state = drive_builder_hops(&mut client, &artifact_path, hops).await?;

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth_path);
                out.set("e2e_artifact_path", artifact_path);
                out.set("e2e_final_state", final_state);
                Ok(out)
            },
        ),
        // AC2/AC3 over the RPC: begin then drive only the first gate to analyzing.
        async_step_def(
            "the engine drives a playbook_generation artifact through the first gate",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("e2e_artifact_path", "String"),
                ("e2e_final_state", "String"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let mut client = anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr)
                    .await
                    .map_err(|e| format!("Connection failed: {}", e))?;

                let begin_resp = client
                    .begin(crate::surfaced(anvil_engine::proto::BeginRequest {
                        hearth_path: String::new(),
                        artifact_type: "playbook_generation".to_string(),
                        parent_id: BUILDER_PARENT_TRACK_ID.to_string(),
                        track_name: "builder gate".to_string(),
                        playbook_name: "temper".to_string(),
                        target_owner: "kit:gate-owner".to_string(),
                        approver: "Approver-E2E".to_string(),
                        actor_name: "Builder-Gate-100000".to_string(),
                        actor_type: "agent".to_string(),
                        actor_model: "claude-opus-4-8".to_string(),
                        actor_provider: "anthropic".to_string(),
                        actor_context_window: 0,
                        actor_sdk_version: String::new(),
                        actor_entrypoint: String::new(),
                        identifier: String::new(),
                        session_role: "creator".to_string(),
                        ctx_org: "Foundation".to_string(),
                        ctx_space: String::new(),
                        ctx_role: "read".to_string(),
                        ctx_clearance: "internal".to_string(),
                    ..Default::default()
                    }))
                    .await
                    .map_err(|s| format!("begin failed: {}: {}", grpc_code_name(s.code()), s.message()))?
                    .into_inner();
                let artifact_path = begin_resp_track_path(&begin_resp);
                if begin_resp.state != "gathering" {
                    return Err(format!("expected create state gathering, got '{}'", begin_resp.state));
                }

                let hops: &[(&str, &str)] = &[
                    ("", "gathering_review"),
                    ("approved", "analyzing"),
                ];
                let final_state = drive_builder_hops(&mut client, &artifact_path, hops).await?;

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth_path);
                out.set("e2e_artifact_path", artifact_path);
                out.set("e2e_final_state", final_state);
                Ok(out)
            },
        ),
        async_step_def(
            "the engine intake of an anchored candidate playbook is attempted",
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("persist_owner_home", "PathBuf"),
                ("persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("persist_owner_home", "PathBuf"),
                ("persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("intake_refused", "bool"),
                ("intake_status_code", "String"),
                ("intake_error_message", "String"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let owner_home = ctx
                    .get::<PathBuf>("persist_owner_home")
                    .ok_or("No persist_owner_home")?
                    .clone();
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let mut client =
                    anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr)
                        .await
                        .map_err(|e| format!("Connection failed: {}", e))?;

                let result = client
                    .intake_candidate_playbook(crate::surfaced(
                        anchored_candidate_intake_request(&owner_home),
                    ))
                    .await;

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth_path);
                out.set("persist_owner_home", owner_home);
                if let Some(handle) =
                    ctx.take::<Arc<Mutex<Option<tempfile::TempDir>>>>("persist_owner_home_handle")
                {
                    out.set("persist_owner_home_handle", handle);
                }
                match result {
                    Ok(_response) => {
                        out.set("intake_refused", false);
                        out.set("intake_status_code", String::new());
                        out.set("intake_error_message", String::new());
                    }
                    Err(status) => {
                        out.set("intake_refused", true);
                        out.set("intake_status_code", grpc_code_name(status.code()).to_string());
                        out.set("intake_error_message", status.message().to_string());
                    }
                }
                Ok(out)
            },
        ),
        async_step_def(
            "the engine intake of a FREE anchored candidate declaring evidence obligations is attempted",
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("persist_owner_home", "PathBuf"),
                ("persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("persist_owner_home", "PathBuf"),
                ("persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("intake_refused", "bool"),
                ("intake_status_code", "String"),
                ("intake_error_message", "String"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let hearth_path = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let owner_home = ctx
                    .get::<PathBuf>("persist_owner_home")
                    .ok_or("No persist_owner_home")?
                    .clone();
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let mut client =
                    anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr)
                        .await
                        .map_err(|e| format!("Connection failed: {}", e))?;

                let result = client
                    .intake_candidate_playbook(crate::surfaced(
                        anchored_candidate_intake_request_with_evidence(&owner_home, "free"),
                    ))
                    .await;

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth_path);
                out.set("persist_owner_home", owner_home);
                if let Some(handle) =
                    ctx.take::<Arc<Mutex<Option<tempfile::TempDir>>>>("persist_owner_home_handle")
                {
                    out.set("persist_owner_home_handle", handle);
                }
                match result {
                    Ok(_response) => {
                        out.set("intake_refused", false);
                        out.set("intake_status_code", String::new());
                        out.set("intake_error_message", String::new());
                    }
                    Err(status) => {
                        out.set("intake_refused", true);
                        out.set("intake_status_code", grpc_code_name(status.code()).to_string());
                        out.set("intake_error_message", status.message().to_string());
                    }
                }
                Ok(out)
            },
        ),
        check_def(
            "the candidate intake is refused with gRPC status {string}",
            &[("intake_refused", "bool"), ("intake_status_code", "String")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected status")?;
                let refused = ctx.get::<bool>("intake_refused").ok_or("No intake_refused")?;
                if !*refused {
                    return Err("Expected intake to be refused, but it succeeded".to_string());
                }
                let code = ctx
                    .get::<String>("intake_status_code")
                    .ok_or("No intake_status_code")?;
                if code == expected {
                    Ok(())
                } else {
                    Err(format!("Expected gRPC status '{}', got '{}'", expected, code))
                }
            },
        ),
        check_def(
            "the candidate intake error message contains {string}",
            &[("intake_error_message", "String")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected substring")?;
                let message = ctx
                    .get::<String>("intake_error_message")
                    .ok_or("No intake_error_message")?;
                if message.contains(expected) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected intake error to contain '{}', got: {}",
                        expected, message
                    ))
                }
            },
        ),
        check_def(
            "no intake instance is written",
            &[("hearth_path", "PathBuf")],
            |ctx, _params| {
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let instance_count = count_dir_entries(&hearth_path.join("workflow_generations"));
                if instance_count == 0 {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected no playbook-generation intake instance, found {}",
                        instance_count
                    ))
                }
            },
        ),
        async_step_def(
            "the engine intakes an anchored candidate playbook and drives it to completed",
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("persist_owner_home", "PathBuf"),
                ("persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("persist_owner_home", "PathBuf"),
                ("persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("e2e_artifact_path", "String"),
                ("e2e_final_state", "String"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let owner_home = ctx
                    .get::<PathBuf>("persist_owner_home")
                    .ok_or("No persist_owner_home")?
                    .clone();
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let mut client = anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr)
                    .await
                    .map_err(|e| format!("Connection failed: {}", e))?;

                let response = client
                    .intake_candidate_playbook(crate::surfaced(
                        anchored_candidate_intake_request(&owner_home),
                    ))
                    .await
                    .map_err(|s| {
                        format!(
                            "intake_candidate_playbook failed: {}: {}",
                            grpc_code_name(s.code()),
                            s.message()
                        )
                    })?
                    .into_inner();
                let artifact_path = format!("workflow_generations/{}", response.instance_id);

                let hops: &[(&str, &str)] = &[
                    ("", "gathering_review"),
                    ("approved", "analyzing"),
                    ("", "analyze_review"),
                    ("approved", "modeling"),
                    ("", "model_review"),
                    ("approved", "testing"),
                    ("", "test_review"),
                    ("approved", "trial_run"),
                    ("", "trial_review"),
                    ("approved", "reflecting"),
                    ("", "reflection_review"),
                    ("approved", "evolving"),
                    ("", "evolve_review"),
                    ("approved", "completed"),
                ];
                let final_state = drive_builder_hops(&mut client, &artifact_path, hops).await?;

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth_path);
                out.set("persist_owner_home", owner_home);
                if let Some(handle) =
                    ctx.take::<Arc<Mutex<Option<tempfile::TempDir>>>>("persist_owner_home_handle")
                {
                    out.set("persist_owner_home_handle", handle);
                }
                out.set("e2e_artifact_path", artifact_path);
                out.set("e2e_final_state", final_state);
                Ok(out)
            },
        ),
        async_step_def(
            "the engine intakes a DRIVEN anchored candidate declaring evidence obligations and drives it to completed",
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("persist_owner_home", "PathBuf"),
                ("persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("persist_owner_home", "PathBuf"),
                ("persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("e2e_artifact_path", "String"),
                ("e2e_final_state", "String"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let hearth_path = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let owner_home = ctx
                    .get::<PathBuf>("persist_owner_home")
                    .ok_or("No persist_owner_home")?
                    .clone();
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let mut client =
                    anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr)
                        .await
                        .map_err(|e| format!("Connection failed: {}", e))?;

                let response = client
                    .intake_candidate_playbook(crate::surfaced(
                        anchored_candidate_intake_request_with_evidence(&owner_home, "driven"),
                    ))
                    .await
                    .map_err(|s| {
                        format!(
                            "intake_candidate_playbook failed: {}: {}",
                            grpc_code_name(s.code()),
                            s.message()
                        )
                    })?
                    .into_inner();
                let artifact_path = format!("workflow_generations/{}", response.instance_id);

                let hops: &[(&str, &str)] = &[
                    ("", "gathering_review"),
                    ("approved", "analyzing"),
                    ("", "analyze_review"),
                    ("approved", "modeling"),
                    ("", "model_review"),
                    ("approved", "testing"),
                    ("", "test_review"),
                    ("approved", "trial_run"),
                    ("", "trial_review"),
                    ("approved", "reflecting"),
                    ("", "reflection_review"),
                    ("approved", "evolving"),
                    ("", "evolve_review"),
                    ("approved", "completed"),
                ];
                let final_state = drive_builder_hops(&mut client, &artifact_path, hops).await?;

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth_path);
                out.set("persist_owner_home", owner_home);
                if let Some(handle) =
                    ctx.take::<Arc<Mutex<Option<tempfile::TempDir>>>>("persist_owner_home_handle")
                {
                    out.set("persist_owner_home_handle", handle);
                }
                out.set("e2e_artifact_path", artifact_path);
                out.set("e2e_final_state", final_state);
                Ok(out)
            },
        ),
        check_def(
            "the persisted generated machine carries evidence obligations:",
            &[("persist_owner_home", "PathBuf")],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected evidence obligation table")?;
                let state_idx = column_index(table, "state")?;
                let role_idx = column_index(table, "role")?;
                let classes_idx = column_index(table, "evidence_classes")?;
                let owner_home = ctx
                    .get::<PathBuf>("persist_owner_home")
                    .ok_or("No persist_owner_home")?;
                let machine_path = owner_home
                    .join("playbooks")
                    .join("evidence_triage")
                    .join("machine.yaml");
                let machine_text = std::fs::read_to_string(&machine_path)
                    .map_err(|e| format!("read {}: {}", machine_path.display(), e))?;
                let machine: anvil_core::domain::playbook::types::PlaybookMachine =
                    serde_yaml::from_str(&machine_text)
                        .map_err(|e| format!("parse {}: {}", machine_path.display(), e))?;

                for row in &table.rows {
                    let state_name = row.get(state_idx).ok_or("Missing state cell")?;
                    let role = row.get(role_idx).ok_or("Missing role cell")?;
                    let expected = parse_csv(
                        row.get(classes_idx).ok_or("Missing evidence_classes cell")?,
                    );
                    let state = machine
                        .states
                        .iter()
                        .find(|state| state.name == *state_name)
                        .ok_or_else(|| format!("Persisted machine has no state '{}'", state_name))?;
                    let spec = state.measurement_by_role.get(role).ok_or_else(|| {
                        format!(
                            "Persisted machine state '{}' has no measurement for role '{}'",
                            state_name, role
                        )
                    })?;
                    let actual = spec
                        .evidence_obligation
                        .iter()
                        .map(|class| match class {
                            anvil_core::domain::playbook::types::EvidenceClass::ArtifactOfConsequence => {
                                "artifact_of_consequence".to_string()
                            }
                            anvil_core::domain::playbook::types::EvidenceClass::VerifiableCitation => {
                                "verifiable_citation".to_string()
                            }
                            anvil_core::domain::playbook::types::EvidenceClass::SelfDescription => {
                                "self_description".to_string()
                            }
                        })
                        .collect::<Vec<_>>();
                    if actual != expected {
                        return Err(format!(
                            "Expected state '{}' role '{}' evidence obligations {:?}, got {:?}",
                            state_name, role, expected, actual
                        ));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "the persisted generated machine has success rubric anchors:",
            &[("persist_owner_home", "PathBuf")],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected anchors table")?;
                let owner_home = ctx
                    .get::<PathBuf>("persist_owner_home")
                    .ok_or("No persist_owner_home")?;
                let machine_path = owner_home
                    .join("playbooks")
                    .join("evidence_triage")
                    .join("machine.yaml");
                let machine_text = std::fs::read_to_string(&machine_path)
                    .map_err(|e| format!("read {}: {}", machine_path.display(), e))?;
                let machine: anvil_core::domain::playbook::types::PlaybookMachine =
                    serde_yaml::from_str(&machine_text)
                        .map_err(|e| format!("parse {}: {}", machine_path.display(), e))?;
                let actual = machine
                    .success_rubric
                    .ok_or("persisted machine has no success_rubric")?
                    .anchors;
                let expected = parse_anchor_table(table)?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("Expected anchors {:?}, got {:?}", expected, actual))
                }
            },
        ),
        check_def(
            "an exemplar markdown exists at {string} under the persist owner-home",
            &[("persist_owner_home", "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected rel path")?;
                let owner_home = ctx
                    .get::<PathBuf>("persist_owner_home")
                    .ok_or("No persist_owner_home")?;
                let path = owner_home.join(rel);
                if path.is_file() {
                    Ok(())
                } else {
                    Err(format!("Expected file at {}", path.display()))
                }
            },
        ),
        // AC6: route over a hearth holding the builder machine includes
        // playbook_generation as a driven candidate (register defaults driven).
        step_def(
            "a route hearth seeded with the builder machine",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, _params| {
                let (handle, tmp) = fresh_route_hearth("builder")?;
                seed_empty_route_hearth(&tmp)?;
                crate::builder::seed_builder_hearth(&tmp, None)?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
    ]
}

async fn submit_candidate_wire_fixture(
    mut ctx: Context,
    register: Option<&str>,
    evidence_obligation: &[&str],
    legacy_payload: bool,
) -> Result<Context, String> {
    let engine = ctx
        .take::<EngineProcess>("engine_process")
        .ok_or("No engine_process")?;
    let hearth_path = ctx
        .get::<PathBuf>("hearth_path")
        .ok_or("No hearth_path")?
        .clone();

    let raw_candidate = if legacy_payload {
        candidate_wire_proto(vec![candidate_wire_proposed_state()]).encode_to_vec()
    } else {
        let mut raw = candidate_wire_proto(Vec::new()).encode_to_vec();
        let mut proposed_state = Vec::new();
        append_proto_string(&mut proposed_state, 1, "triage");
        append_proto_string(&mut proposed_state, 2, "doer");
        append_proto_string(
            &mut proposed_state,
            3,
            "Triage the candidate evidence declarations.",
        );
        append_proto_string(
            &mut proposed_state,
            4,
            "A triage report with cited findings.",
        );
        for evidence_class in evidence_obligation {
            append_proto_string(&mut proposed_state, 6, evidence_class);
        }
        append_proto_message(&mut raw, 5, &proposed_state);
        if let Some(register) = register {
            append_proto_string(&mut raw, 14, register);
        }
        raw
    };

    let candidate = anvil_engine::proto::CandidatePlaybook::decode(raw_candidate.as_slice())
        .map_err(|error| format!("decode candidate wire fixture: {}", error))?;
    let bytes_unchanged = candidate.encode_to_vec() == raw_candidate;

    let addr = format!("http://127.0.0.1:{}", engine.port);
    let mut client = anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr)
        .await
        .map_err(|error| format!("connect candidate wire intake: {}", error))?;
    let result = client
        .intake_candidate_playbook(crate::surfaced(
            anvil_engine::proto::IntakeCandidatePlaybookRequest {
                candidate: Some(candidate),
                target_owner: "/tmp/anvil-candidate-wire-owner".to_string(),
                parent_id: BUILDER_PARENT_TRACK_ID.to_string(),
                approver: "Wire-Approver-100000".to_string(),
                actor_name: "Wire-Actor-100000".to_string(),
                actor_type: "agent".to_string(),
                actor_model: "brine".to_string(),
                actor_provider: "test".to_string(),
                actor_context_window: 0,
                actor_sdk_version: String::new(),
                actor_entrypoint: String::new(),
                hearth_path: String::new(),
            },
        ))
        .await;

    let mut out = Context::new();
    out.set("engine_process", engine);
    out.set("hearth_path", hearth_path.clone());
    out.set("candidate_wire_bytes_unchanged", bytes_unchanged);
    match result {
        Ok(response) => {
            let instance_id = response.into_inner().instance_id;
            let context_path = hearth_path
                .join("workflow_generations")
                .join(instance_id)
                .join("generation-context.json");
            let context_text = std::fs::read_to_string(&context_path).map_err(|error| {
                format!(
                    "read candidate wire generation context {}: {}",
                    context_path.display(),
                    error
                )
            })?;
            let context_json =
                serde_json::from_str::<serde_json::Value>(&context_text).map_err(|error| {
                    format!(
                        "parse candidate wire generation context {}: {}",
                        context_path.display(),
                        error
                    )
                })?;
            out.set("candidate_wire_succeeded", true);
            out.set("candidate_wire_status", String::new());
            out.set("candidate_wire_error", String::new());
            out.set("candidate_wire_context", context_json);
        }
        Err(status) => {
            out.set("candidate_wire_succeeded", false);
            out.set(
                "candidate_wire_status",
                grpc_code_name(status.code()).to_string(),
            );
            out.set("candidate_wire_error", status.message().to_string());
            out.set("candidate_wire_context", serde_json::Value::Null);
        }
    }
    Ok(out)
}

fn candidate_wire_proto(
    proposed_states: Vec<anvil_engine::proto::candidate_playbook::ProposedState>,
) -> anvil_engine::proto::CandidatePlaybook {
    anvil_engine::proto::CandidatePlaybook {
        source: "lore".to_string(),
        intent: "Candidate Evidence Wire".to_string(),
        at: "2026-07-19T19:00:00Z".to_string(),
        evidence: vec!["obs-wire-1".to_string()],
        proposed_states,
        route_description: "Route here when the user asks to AUTHOR candidate evidence playbooks."
            .to_string(),
        route_triggers: vec!["author candidate evidence playbook".to_string()],
        projection_targets: vec!["workflows.md".to_string()],
        success_rubric: None,
        anchors: Vec::new(),
        exemplars: Vec::new(),
        ledger_classification: None,
        none_yet_justification: None,
        ..Default::default()
    }
}

fn candidate_wire_proposed_state() -> anvil_engine::proto::candidate_playbook::ProposedState {
    anvil_engine::proto::candidate_playbook::ProposedState {
        state: "triage".to_string(),
        role: "doer".to_string(),
        intent: "Triage the candidate evidence declarations.".to_string(),
        expected_output: "A triage report with cited findings.".to_string(),
        ..Default::default()
    }
}

fn append_proto_string(bytes: &mut Vec<u8>, field_number: u32, value: &str) {
    append_proto_message(bytes, field_number, value.as_bytes());
}

fn append_proto_message(bytes: &mut Vec<u8>, field_number: u32, value: &[u8]) {
    append_proto_varint(bytes, u64::from((field_number << 3) | 2));
    append_proto_varint(bytes, value.len() as u64);
    bytes.extend_from_slice(value);
}

fn append_proto_varint(bytes: &mut Vec<u8>, mut value: u64) {
    loop {
        if value < 0x80 {
            bytes.push(value as u8);
            return;
        }
        bytes.push((value as u8) | 0x80);
        value >>= 7;
    }
}

fn candidate_from_wire_context(ctx: &Context) -> Result<&serde_json::Value, String> {
    ctx.get::<serde_json::Value>("candidate_wire_context")
        .and_then(|context| context.get("seed"))
        .and_then(|seed| seed.get("candidate"))
        .ok_or_else(|| "Candidate wire generation context has no seed.candidate".to_string())
}

fn proposed_state_from_wire_context<'a>(
    ctx: &'a Context,
    state: &str,
) -> Result<&'a serde_json::Value, String> {
    candidate_from_wire_context(ctx)?
        .get("proposed_states")
        .and_then(serde_json::Value::as_array)
        .and_then(|states| {
            states.iter().find(|candidate_state| {
                candidate_state
                    .get("state")
                    .and_then(serde_json::Value::as_str)
                    == Some(state)
            })
        })
        .ok_or_else(|| format!("Mapped candidate has no proposed state '{}'", state))
}

/// Drive a sequence of complete RPCs over the builder artifact, asserting each
/// hop's machine-declared new_state. Returns the final state.
async fn drive_builder_hops(
    client: &mut anvil_engine::proto::anvil_service_client::AnvilServiceClient<
        tonic::transport::Channel,
    >,
    artifact_path: &str,
    hops: &[(&str, &str)],
) -> Result<String, String> {
    let mut final_state = String::new();
    for (satisfaction, expected) in hops {
        let resp = client
            .complete(crate::surfaced(anvil_engine::proto::CompleteRequest {
                artifact_path: artifact_path.to_string(),
                actor_name: "Builder-E2E-100000".to_string(),
                actor_type: "agent".to_string(),
                actor_model: "claude-opus-4-8".to_string(),
                actor_provider: "anthropic".to_string(),
                actor_context_window: 0,
                actor_sdk_version: String::new(),
                actor_entrypoint: String::new(),
                satisfaction: satisfaction.to_string(),
                approver: "Approver-E2E".to_string(),
                note: String::new(),
                reflection_notes: String::new(),
                findings: String::new(),
                hearth_path: String::new(),
                conversation_id: String::new(),
                project_root: String::new(),
                claimed_evidence: Vec::new(),
            }))
            .await
            .map_err(|s| {
                format!(
                    "complete (sat='{}', expect '{}') failed: {}: {}",
                    satisfaction,
                    expected,
                    grpc_code_name(s.code()),
                    s.message()
                )
            })?
            .into_inner();
        if &resp.new_state != expected {
            return Err(format!(
                "complete (sat='{}') expected new_state '{}', got '{}'",
                satisfaction, expected, resp.new_state
            ));
        }
        final_state = resp.new_state.clone();
    }
    Ok(final_state)
}

fn anchored_candidate_intake_request(
    owner_home: &Path,
) -> anvil_engine::proto::IntakeCandidatePlaybookRequest {
    use anvil_engine::proto::candidate_playbook as cp;

    anvil_engine::proto::IntakeCandidatePlaybookRequest {
        candidate: Some(anvil_engine::proto::CandidatePlaybook {
            source: "lore".to_string(),
            intent: "Evidence Triage".to_string(),
            at: "2026-07-04T00:00:00Z".to_string(),
            evidence: vec!["obs-anchor-1".to_string()],
            proposed_states: vec![cp::ProposedState {
                state: "triage".to_string(),
                role: "doer".to_string(),
                intent: "Triage the new evidence.".to_string(),
                expected_output: "A triage note with routing.".to_string(),
                evidence_obligation: Vec::new(),
            }],
            route_description: "Route here when the user asks to triage evidence.".to_string(),
            route_triggers: vec!["triage evidence".to_string()],
            projection_targets: vec!["workflows.md".to_string()],
            success_rubric: Some(cp::SuccessRubric {
                dimensions: vec![
                    cp::RubricDimension {
                        dimension: "correctness".to_string(),
                        weight: 3,
                        evidence_class: "artifact_of_consequence".to_string(),
                    },
                    cp::RubricDimension {
                        dimension: "research_rigor".to_string(),
                        weight: 2,
                        evidence_class: "verifiable_citation".to_string(),
                    },
                ],
                grader: "fixture-grader".to_string(),
                lagging_signals: vec!["fixture_outcome".to_string()],
                anchors: Vec::new(),
            }),
            anchors: vec![
                cp::AnchorRef {
                    instance: "triage-good".to_string(),
                    band: "good".to_string(),
                },
                cp::AnchorRef {
                    instance: "triage-trap".to_string(),
                    band: "trap".to_string(),
                },
            ],
            exemplars: vec![
                anchored_proto_exemplar("triage-good", "good", "A distilled good triage pattern."),
                anchored_proto_exemplar("triage-trap", "trap", "A distilled trap triage pattern."),
            ],
            ledger_classification: Some(cp::LedgerClassification {
                corpus: "fixture corpus".to_string(),
                ledger: "fixture ledger".to_string(),
                classification: "has_examples".to_string(),
            }),
            none_yet_justification: None,
            register: String::new(),
        }),
        target_owner: owner_home.to_string_lossy().into_owned(),
        parent_id: BUILDER_PARENT_TRACK_ID.to_string(),
        approver: "Approver-E2E".to_string(),
        actor_name: "Builder-E2E-100000".to_string(),
        actor_type: "agent".to_string(),
        actor_model: "claude-opus-4-8".to_string(),
        actor_provider: "anthropic".to_string(),
        actor_context_window: 0,
        actor_sdk_version: String::new(),
        actor_entrypoint: String::new(),
        hearth_path: String::new(),
    }
}

fn anchored_candidate_intake_request_with_evidence(
    owner_home: &Path,
    register: &str,
) -> anvil_engine::proto::IntakeCandidatePlaybookRequest {
    let mut request = anchored_candidate_intake_request(owner_home);
    if let Some(candidate) = request.candidate.as_mut() {
        candidate.register = register.to_string();
        for state in &mut candidate.proposed_states {
            state.evidence_obligation = vec![
                "verifiable_citation".to_string(),
                "artifact_of_consequence".to_string(),
            ];
        }
    }
    request
}

fn anchored_proto_exemplar(
    id: &str,
    band: &str,
    body: &str,
) -> anvil_engine::proto::candidate_playbook::Exemplar {
    use anvil_engine::proto::candidate_playbook as cp;

    cp::Exemplar {
        frontmatter: Some(cp::ExemplarFrontmatter {
            id: id.to_string(),
            band: band.to_string(),
            dimensions: vec!["correctness".to_string(), "research_rigor".to_string()],
            evidence_class: "artifact_of_consequence".to_string(),
            outcome_link: Some(cp::OutcomeLink {
                authority: "brine".to_string(),
                opaque_ref: format!("fixture:{}", id),
                verified_at: "2026-07-04T00:00:00Z".to_string(),
            }),
            provenance: Some(cp::ExemplarProvenance {
                source: "internal".to_string(),
                corpus: "fixture".to_string(),
            }),
            playbook_version: "fixture-version".to_string(),
            refreshed_at: "2026-07-04T00:00:00Z".to_string(),
        }),
        body: body.to_string(),
    }
}

fn parse_anchor_table(
    table: &DataTable,
) -> Result<Vec<anvil_core::domain::playbook::types::AnchorRef>, String> {
    let instance_idx = table
        .headers
        .iter()
        .position(|header| header == "instance")
        .ok_or("Missing instance column")?;
    let band_idx = table
        .headers
        .iter()
        .position(|header| header == "band")
        .ok_or("Missing band column")?;
    table
        .rows
        .iter()
        .map(|row| {
            Ok(anvil_core::domain::playbook::types::AnchorRef {
                instance: row
                    .get(instance_idx)
                    .ok_or("Missing instance cell")?
                    .to_string(),
                band: row.get(band_idx).ok_or("Missing band cell")?.to_string(),
            })
        })
        .collect()
}

/// Drive a sequence of complete RPCs over the daily_recap artifact, asserting
/// each happy-path hop's machine-declared new_state. Returns the final state.
async fn drive_daily_recap_hops(
    client: &mut anvil_engine::proto::anvil_service_client::AnvilServiceClient<
        tonic::transport::Channel,
    >,
    artifact_path: &str,
    hops: &[(&str, &str)],
) -> Result<String, String> {
    let mut final_state = String::new();
    for (satisfaction, expected) in hops {
        let resp = client
            .complete(crate::surfaced(anvil_engine::proto::CompleteRequest {
                artifact_path: artifact_path.to_string(),
                actor_name: "Daily-Recap-E2E-100000".to_string(),
                actor_type: "agent".to_string(),
                actor_model: "test".to_string(),
                actor_provider: "test".to_string(),
                actor_context_window: 0,
                actor_sdk_version: String::new(),
                actor_entrypoint: String::new(),
                satisfaction: satisfaction.to_string(),
                approver: String::new(),
                note: String::new(),
                reflection_notes: String::new(),
                findings: String::new(),
                hearth_path: String::new(),
                conversation_id: String::new(),
                project_root: String::new(),
                claimed_evidence: Vec::new(),
            }))
            .await
            .map_err(|s| {
                format!(
                    "daily_recap complete (sat='{}', expect '{}') failed: {}: {}",
                    satisfaction,
                    expected,
                    grpc_code_name(s.code()),
                    s.message()
                )
            })?
            .into_inner();
        if &resp.new_state != expected {
            return Err(format!(
                "daily_recap complete (sat='{}') expected new_state '{}', got '{}'",
                satisfaction, expected, resp.new_state
            ));
        }
        final_state = resp.new_state.clone();
    }
    Ok(final_state)
}

/// Extract the created artifact's relative path from a begin response.
fn begin_resp_track_path(resp: &anvil_engine::proto::BeginResponse) -> String {
    resp.track_path.clone()
}

/// Seed an engine hearth that physically holds the knowledge_lifecycle
/// machine.yaml (resolved only via HearthPlaybookRegistry) plus the structural
/// signature the engine hearth predicate checks (tracks/ + tracks.md) and the
/// knowledge.md registry the first-complete entry creation appends to.
fn seed_knowledge_lifecycle_engine_hearth(tmp: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(tmp).map_err(|e| format!("Failed to create hearth: {}", e))?;
    std::fs::create_dir_all(tmp.join("tracks"))
        .map_err(|e| format!("Failed to create tracks dir: {}", e))?;
    std::fs::write(tmp.join("tracks.md"), "# Tracks\n")
        .map_err(|e| format!("Failed to write tracks.md: {}", e))?;
    // knowledge registry + directory so the complete-path entry creation lands.
    std::fs::create_dir_all(tmp.join("knowledge"))
        .map_err(|e| format!("Failed to create knowledge dir: {}", e))?;
    std::fs::write(tmp.join("knowledge.md"), "# Knowledge\n")
        .map_err(|e| format!("Failed to write knowledge.md: {}", e))?;
    let proj_dir = tmp.join("projections");
    std::fs::create_dir_all(&proj_dir)
        .map_err(|e| format!("Failed to create projections dir: {}", e))?;
    std::fs::write(
        proj_dir.join("execution.md"),
        "---\nincremental_count: 0\nbase_snapshot: 2026-06-01T00:00:00Z\nlast_updated: 2026-06-01T00:00:00Z\nafter_event: \"seed\"\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n",
    )
    .map_err(|e| format!("Failed to write execution.md: {}", e))?;
    let wf_dir = tmp
        .join("playbooks")
        .join("20260529T0409_knowledge_lifecycle");
    std::fs::create_dir_all(&wf_dir)
        .map_err(|e| format!("Failed to create playbook dir: {}", e))?;
    std::fs::write(
        wf_dir.join("machine.yaml"),
        crate::query_port::knowledge_lifecycle_machine_yaml(),
    )
    .map_err(|e| format!("Failed to write machine.yaml: {}", e))?;
    Ok(())
}

fn seed_decision_lifecycle_engine_hearth(tmp: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(tmp).map_err(|e| format!("Failed to create hearth: {}", e))?;
    std::fs::create_dir_all(tmp.join("decisions"))
        .map_err(|e| format!("Failed to create decisions dir: {}", e))?;
    std::fs::write(
        tmp.join("decisions.md"),
        "# Decisions\n\n## tension\n\n## investigating\n\n## decided\n\n## retired\n",
    )
    .map_err(|e| format!("Failed to write decisions.md: {}", e))?;
    let projection_dir = tmp.join("projections");
    std::fs::create_dir_all(&projection_dir)
        .map_err(|e| format!("Failed to create projections dir: {}", e))?;
    std::fs::write(
        projection_dir.join("decisions.md"),
        "---\nincremental_count: 0\nbase_snapshot: 2026-06-14T00:00:00Z\nlast_updated: 2026-06-14T00:00:00Z\nafter_event: \"\"\n---\n\n# Decisions Projection\n\n",
    )
    .map_err(|e| format!("Failed to write projections/decisions.md: {}", e))?;

    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("Expected workspace root")?
        .join("playbooks")
        .join("decision_lifecycle");
    let dst = tmp.join("playbooks").join("decision_lifecycle");
    copy_dir_recursive(&src, &dst)
        .map_err(|e| format!("Failed to copy decision_lifecycle playbook: {}", e))?;
    Ok(())
}

fn seed_learning_lifecycle_engine_hearth(tmp: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(tmp).map_err(|e| format!("Failed to create hearth: {}", e))?;
    std::fs::create_dir_all(tmp.join("learnings"))
        .map_err(|e| format!("Failed to create learnings dir: {}", e))?;
    std::fs::write(
        tmp.join("learnings.md"),
        "# Learnings\n\n## observation\n\n## conclusion\n\n## established\n\n## graduated\n\n## retired\n",
    )
    .map_err(|e| format!("Failed to write learnings.md: {}", e))?;

    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("Expected workspace root")?
        .join("playbooks")
        .join("learning_lifecycle");
    let dst = tmp.join("playbooks").join("learning_lifecycle");
    copy_dir_recursive(&src, &dst)
        .map_err(|e| format!("Failed to copy learning_lifecycle playbook: {}", e))?;
    Ok(())
}

fn seed_initiative_lifecycle_engine_hearth(tmp: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(tmp).map_err(|e| format!("Failed to create hearth: {}", e))?;
    std::fs::create_dir_all(tmp.join("initiatives"))
        .map_err(|e| format!("Failed to create initiatives dir: {}", e))?;
    std::fs::write(
        tmp.join("initiatives.md"),
        "# Initiatives\n\n## draft\n\n## active\n\n## promoted\n\n## retired\n",
    )
    .map_err(|e| format!("Failed to write initiatives.md: {}", e))?;

    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("Expected workspace root")?
        .join("playbooks")
        .join("initiative_lifecycle");
    let dst = tmp.join("playbooks").join("initiative_lifecycle");
    copy_dir_recursive(&src, &dst)
        .map_err(|e| format!("Failed to copy initiative_lifecycle playbook: {}", e))?;
    Ok(())
}

fn seed_milestone_lifecycle_engine_hearth(tmp: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(tmp).map_err(|e| format!("Failed to create hearth: {}", e))?;
    std::fs::create_dir_all(tmp.join("tracks"))
        .map_err(|e| format!("Failed to create tracks dir: {}", e))?;
    std::fs::write(tmp.join("tracks.md"), "# Tracks\n")
        .map_err(|e| format!("Failed to write tracks.md: {}", e))?;
    std::fs::create_dir_all(tmp.join("milestones"))
        .map_err(|e| format!("Failed to create milestones dir: {}", e))?;
    std::fs::write(
        tmp.join("milestones.md"),
        "# Milestones\n\n## draft\n\n## active\n\n## reflecting\n\n## completed\n\n## superseded\n\n## abandoned\n",
    )
    .map_err(|e| format!("Failed to write milestones.md: {}", e))?;
    let projection_dir = tmp.join("projections");
    std::fs::create_dir_all(&projection_dir)
        .map_err(|e| format!("Failed to create projections dir: {}", e))?;
    std::fs::write(
        projection_dir.join("intent.md"),
        "---\nincremental_count: 0\nbase_snapshot: 2026-06-14T00:00:00Z\nlast_updated: 2026-06-14T00:00:00Z\nafter_event: \"\"\n---\n\n# Anvil - State of Intent\n\n## Milestones\n\n## Proposals\n\n",
    )
    .map_err(|e| format!("Failed to write projections/intent.md: {}", e))?;

    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("Expected workspace root")?
        .join("playbooks")
        .join("milestone_lifecycle");
    let dst = tmp.join("playbooks").join("milestone_lifecycle");
    copy_dir_recursive(&src, &dst)
        .map_err(|e| format!("Failed to copy milestone_lifecycle playbook: {}", e))?;
    Ok(())
}

fn seed_proposal_lifecycle_engine_hearth(tmp: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(tmp).map_err(|e| format!("Failed to create hearth: {}", e))?;
    std::fs::create_dir_all(tmp.join("tracks"))
        .map_err(|e| format!("Failed to create tracks dir: {}", e))?;
    std::fs::write(tmp.join("tracks.md"), "# Tracks\n")
        .map_err(|e| format!("Failed to write tracks.md: {}", e))?;
    std::fs::create_dir_all(tmp.join("proposals"))
        .map_err(|e| format!("Failed to create proposals dir: {}", e))?;
    std::fs::write(
        tmp.join("proposals.md"),
        "# Proposals\n\n## vision\n\n## draft\n\n## active\n\n## reflecting\n\n## completed\n\n## superseded\n\n## abandoned\n",
    )
    .map_err(|e| format!("Failed to write proposals.md: {}", e))?;
    let projection_dir = tmp.join("projections");
    std::fs::create_dir_all(&projection_dir)
        .map_err(|e| format!("Failed to create projections dir: {}", e))?;
    std::fs::write(
        projection_dir.join("intent.md"),
        "---\nincremental_count: 0\nbase_snapshot: 2026-06-14T00:00:00Z\nlast_updated: 2026-06-14T00:00:00Z\nafter_event: \"\"\n---\n\n# Anvil - State of Intent\n\n## Proposals\n\n## Milestones\n\n",
    )
    .map_err(|e| format!("Failed to write projections/intent.md: {}", e))?;

    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("Expected workspace root")?
        .join("playbooks")
        .join("proposal_lifecycle");
    let dst = tmp.join("playbooks").join("proposal_lifecycle");
    copy_dir_recursive(&src, &dst)
        .map_err(|e| format!("Failed to copy proposal_lifecycle playbook: {}", e))?;
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Seed a hearth with a directory-less driven playbook. The machine has
/// `directory: ""` and `registry: ""`; begin must create a run record instead
/// of treating the empty directory as the hearth root.
fn seed_lore_query_engine_hearth(tmp: &std::path::Path) -> Result<(), String> {
    seed_empty_route_hearth(tmp)?;
    let proj_dir = tmp.join("projections");
    std::fs::create_dir_all(&proj_dir)
        .map_err(|e| format!("Failed to create projections dir: {}", e))?;
    std::fs::write(
        proj_dir.join("execution.md"),
        "---\nincremental_count: 0\nbase_snapshot: 2026-06-01T00:00:00Z\nlast_updated: 2026-06-01T00:00:00Z\nafter_event: \"seed\"\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n",
    )
    .map_err(|e| format!("Failed to write execution.md: {}", e))?;
    copy_fixture_playbook("lore_query", tmp)
}

fn seed_transition_probe_engine_hearth(tmp: &std::path::Path) -> Result<(), String> {
    seed_empty_route_hearth(tmp)?;
    std::fs::create_dir_all(tmp.join("transition_probes"))
        .map_err(|e| format!("Failed to create transition_probes dir: {}", e))?;
    std::fs::write(tmp.join("transition_probes.md"), "# Transition Probes\n")
        .map_err(|e| format!("Failed to write transition_probes.md: {}", e))?;
    let proj_dir = tmp.join("projections");
    std::fs::create_dir_all(&proj_dir)
        .map_err(|e| format!("Failed to create projections dir: {}", e))?;
    std::fs::write(
        proj_dir.join("execution.md"),
        "---\nincremental_count: 0\nbase_snapshot: 2026-06-24T00:00:00Z\nlast_updated: 2026-06-24T00:00:00Z\nafter_event: \"seed\"\n---\n\n# Anvil — State of Execution\n\n## Active (0)\n\n## Completed (0)\n",
    )
    .map_err(|e| format!("Failed to write execution.md: {}", e))?;
    let wf_dir = tmp.join("playbooks").join("transition_probe");
    std::fs::create_dir_all(&wf_dir)
        .map_err(|e| format!("Failed to create transition_probe playbook dir: {}", e))?;
    std::fs::write(
        wf_dir.join("machine.yaml"),
        crate::minimal_machine_yaml("transition_probe"),
    )
    .map_err(|e| format!("Failed to write transition_probe machine.yaml: {}", e))?;
    Ok(())
}

/// A minimal machine with a REVIEW GATE whose only outgoing transition targets
/// a terminal state (so it is the final/E2E gate). Drives Phase D review-verdict
/// capture: active --doer--> review (review gate) --reviewer(satisfied)-->
/// completed (terminal).
fn review_probe_machine_yaml() -> String {
    r#"kind: review_probe
directory: review_probes
registry: review_probes.md
description: "Review probe machine for verdict testing."
required_fields: []
roles:
  - doer
  - reviewer
states:
  - name: active
    role_filters: []
    registry_section: active
    projection_targets: []
    is_review_gate: false
    is_terminal: false
  - name: review
    role_filters: []
    registry_section: active
    projection_targets: []
    is_review_gate: true
    is_terminal: false
  - name: completed
    role_filters: []
    registry_section: completed
    projection_targets: []
    is_review_gate: false
    is_terminal: true
transitions:
  - from_state: active
    to_state: review
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
  - from_state: review
    to_state: completed
    required_role: reviewer
    required_satisfaction: [satisfied]
    requires_approver: false
"#
    .to_string()
}

/// A machine that can be driven both ways: straight to terminal, or through a
/// REVISION state and back. The revision route is what makes a run UNCLEAN, and
/// the `_revision` suffix is the one shared structural rule
/// (`anvil_core::domain::shared_types::is_revision_state`) that
/// `playbook_run_fidelity`, `step_two_by_two` and the cleanliness grader all
/// read. Without a machine that can be sent back, every scenario would grade a
/// clean run and the grader's zero would be unreachable — the shape a constant
/// grader hides in.
fn revision_probe_machine_yaml() -> String {
    r#"kind: revision_probe
directory: revision_probes
registry: revision_probes.md
description: "Revision probe machine for cleanliness grading."
required_fields: []
roles:
  - doer
  - reviewer
states:
  - name: active
    role_filters: []
    registry_section: active
    projection_targets: []
    is_review_gate: false
    is_terminal: false
  - name: active_review
    role_filters: []
    registry_section: active
    projection_targets: []
    is_review_gate: true
    is_terminal: false
  - name: active_revision
    role_filters: []
    registry_section: active
    projection_targets: []
    is_review_gate: false
    is_terminal: false
  - name: completed
    role_filters: []
    registry_section: completed
    projection_targets: []
    is_review_gate: false
    is_terminal: true
transitions:
  - from_state: active
    to_state: active_review
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
  - from_state: active_review
    to_state: active_revision
    required_role: reviewer
    required_satisfaction: [full_revision]
    requires_approver: false
  - from_state: active_revision
    to_state: active_review
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
  - from_state: active_review
    to_state: completed
    required_role: reviewer
    required_satisfaction: [satisfied]
    requires_approver: false
"#
    .to_string()
}

/// Seed an engine hearth holding the revision_probe playbook plus the
/// registry/directory + execution projection the engine reads.
fn seed_revision_probe_engine_hearth(tmp: &std::path::Path) -> Result<(), String> {
    seed_empty_route_hearth(tmp)?;
    std::fs::create_dir_all(tmp.join("revision_probes"))
        .map_err(|e| format!("Failed to create revision_probes dir: {}", e))?;
    std::fs::write(tmp.join("revision_probes.md"), "# Revision Probes\n")
        .map_err(|e| format!("Failed to write revision_probes.md: {}", e))?;
    let proj_dir = tmp.join("projections");
    std::fs::create_dir_all(&proj_dir)
        .map_err(|e| format!("Failed to create projections dir: {}", e))?;
    std::fs::write(
        proj_dir.join("execution.md"),
        "---\nincremental_count: 0\nbase_snapshot: 2026-08-01T00:00:00Z\nlast_updated: 2026-08-01T00:00:00Z\nafter_event: \"seed\"\n---\n\n# Anvil — State of Execution\n\n## Active (0)\n\n## Completed (0)\n",
    )
    .map_err(|e| format!("Failed to write execution.md: {}", e))?;
    let wf_dir = tmp.join("playbooks").join("revision_probe");
    std::fs::create_dir_all(&wf_dir)
        .map_err(|e| format!("Failed to create revision_probe playbook dir: {}", e))?;
    std::fs::write(wf_dir.join("machine.yaml"), revision_probe_machine_yaml())
        .map_err(|e| format!("Failed to write revision_probe machine.yaml: {}", e))?;
    Ok(())
}

/// Seed an engine hearth holding the review_probe playbook (a review-gate
/// machine) plus the registry/directory + execution projection the engine reads.
fn seed_review_probe_engine_hearth(tmp: &std::path::Path) -> Result<(), String> {
    seed_empty_route_hearth(tmp)?;
    std::fs::create_dir_all(tmp.join("review_probes"))
        .map_err(|e| format!("Failed to create review_probes dir: {}", e))?;
    std::fs::write(tmp.join("review_probes.md"), "# Review Probes\n")
        .map_err(|e| format!("Failed to write review_probes.md: {}", e))?;
    let proj_dir = tmp.join("projections");
    std::fs::create_dir_all(&proj_dir)
        .map_err(|e| format!("Failed to create projections dir: {}", e))?;
    std::fs::write(
        proj_dir.join("execution.md"),
        "---\nincremental_count: 0\nbase_snapshot: 2026-07-01T00:00:00Z\nlast_updated: 2026-07-01T00:00:00Z\nafter_event: \"seed\"\n---\n\n# Anvil — State of Execution\n\n## Active (0)\n\n## Completed (0)\n",
    )
    .map_err(|e| format!("Failed to write execution.md: {}", e))?;
    let wf_dir = tmp.join("playbooks").join("review_probe");
    std::fs::create_dir_all(&wf_dir)
        .map_err(|e| format!("Failed to create review_probe playbook dir: {}", e))?;
    std::fs::write(wf_dir.join("machine.yaml"), review_probe_machine_yaml())
        .map_err(|e| format!("Failed to write review_probe machine.yaml: {}", e))?;
    Ok(())
}

/// The active parent track id seeded for the builder e2e fixture. The builder's
/// `parent_kind: track` makes begin-create enforce that this parent exists, is
/// `state: active`, and is `kind: track`.
const BUILDER_PARENT_TRACK_ID: &str = "20260606T0000_builder_parent_track";

/// Seed an engine hearth that physically holds the builder machine.yaml +
/// hooks/gathering.md (resolved only via HearthPlaybookRegistry), the
/// workflow_generations/ directory + registry the first-complete entry creation
/// appends to, the projections/execution.md the engine reads, and an ACTIVE
/// parent track so the builder begin-create passes parent enforcement.
fn seed_builder_engine_hearth(tmp: &std::path::Path) -> Result<(), String> {
    crate::builder::seed_builder_hearth(tmp, Some(BUILDER_PARENT_TRACK_ID))?;
    let proj_dir = tmp.join("projections");
    std::fs::create_dir_all(&proj_dir)
        .map_err(|e| format!("Failed to create projections dir: {}", e))?;
    std::fs::write(
        proj_dir.join("execution.md"),
        "---\nincremental_count: 0\nbase_snapshot: 2026-06-01T00:00:00Z\nlast_updated: 2026-06-01T00:00:00Z\nafter_event: \"seed\"\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n",
    )
    .map_err(|e| format!("Failed to write execution.md: {}", e))?;
    Ok(())
}

fn seed_evidence_compliant_builder_engine_hearth(tmp: &std::path::Path) -> Result<(), String> {
    seed_builder_engine_hearth(tmp)?;
    let machine_path = tmp
        .join("playbooks")
        .join(crate::builder::BUILDER_PLAYBOOK_ID)
        .join("machine.yaml");
    let machine_text = std::fs::read_to_string(&machine_path)
        .map_err(|e| format!("Failed to read {}: {}", machine_path.display(), e))?;
    let mut machine: anvil_core::domain::playbook::types::PlaybookMachine =
        serde_yaml::from_str(&machine_text)
            .map_err(|e| format!("Failed to parse {}: {}", machine_path.display(), e))?;
    for state in &mut machine.states {
        for spec in state.measurement_by_role.values_mut() {
            if spec.evidence_obligation.is_empty() {
                spec.evidence_obligation =
                    vec![anvil_core::domain::playbook::types::EvidenceClass::ArtifactOfConsequence];
            }
        }
    }
    let machine_yaml = serde_yaml::to_string(&machine)
        .map_err(|e| format!("Failed to serialize {}: {}", machine_path.display(), e))?;
    std::fs::write(&machine_path, machine_yaml)
        .map_err(|e| format!("Failed to write {}: {}", machine_path.display(), e))
}

fn daily_recap_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("daily_recap")
}

fn read_daily_recap_fixture_file(rel: &str) -> Result<String, String> {
    let path = daily_recap_fixture_dir().join(rel);
    std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "Failed to read daily_recap fixture file {}: {}",
            path.display(),
            e
        )
    })
}

/// Seed an engine hearth/owner-home with the daily_recap fixture playbook files,
/// the daily_recaps registry/directory used by first-complete entry creation,
/// the structural tracks signature, and the execution projection the engine
/// reads while driving the artifact.
/// Begin-create a lore_query over the real engine, supplying a generic
/// `create_fields` map. Returns the BeginRpcResult plus the created artifact's
/// hearth-relative path (empty on error) for status.yaml assertions.
async fn begin_lore_query_rpc(
    port: u16,
    create_fields: std::collections::HashMap<String, String>,
) -> (BeginRpcResult, String) {
    let addr = format!("http://127.0.0.1:{}", port);
    match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
        Ok(mut client) => {
            let request = crate::surfaced(anvil_engine::proto::BeginRequest {
                artifact_type: "lore_query".to_string(),
                track_name: "lore query e2e".to_string(),
                approver: "Approver-E2E".to_string(),
                actor_name: "LoreQuery-E2E-100000".to_string(),
                actor_type: "agent".to_string(),
                actor_model: "claude-opus-4-8".to_string(),
                actor_provider: "anthropic".to_string(),
                session_role: "creator".to_string(),
                ctx_org: "Foundation".to_string(),
                ctx_role: "read".to_string(),
                ctx_clearance: "internal".to_string(),
                create_fields,
                ..Default::default()
            });
            match client.begin(request).await {
                Ok(response) => {
                    let inner = response.into_inner();
                    let path = begin_resp_track_path(&inner);
                    (BeginRpcResult::Success(inner), path)
                }
                Err(status) => (
                    BeginRpcResult::Error {
                        code: grpc_code_name(status.code()),
                        message: status.message().to_string(),
                    },
                    String::new(),
                ),
            }
        }
        Err(e) => (
            BeginRpcResult::Error {
                code: "UNAVAILABLE".to_string(),
                message: format!("Connection failed: {}", e),
            },
            String::new(),
        ),
    }
}

/// Seed a hearth holding a lore_query machine that declares the generic
/// required fields `question` + `requester` (outside the builtin set), with a
/// real artifact directory so the created instance persists a status.yaml.
fn seed_lore_query_generic_fields_hearth(tmp: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(tmp).map_err(|e| format!("Failed to create hearth: {}", e))?;
    std::fs::create_dir_all(tmp.join("tracks"))
        .map_err(|e| format!("Failed to create tracks dir: {}", e))?;
    std::fs::write(tmp.join("tracks.md"), "# Tracks\n")
        .map_err(|e| format!("Failed to write tracks.md: {}", e))?;
    std::fs::create_dir_all(tmp.join("lore_queries"))
        .map_err(|e| format!("Failed to create lore_queries dir: {}", e))?;
    std::fs::write(tmp.join("lore_queries.md"), "# Lore Queries\n")
        .map_err(|e| format!("Failed to write lore_queries.md: {}", e))?;

    let wf_dir = tmp.join("playbooks").join("20260609T1322_lore_query");
    let hooks_dir = wf_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir)
        .map_err(|e| format!("Failed to create lore_query hooks dir: {}", e))?;
    std::fs::write(
        wf_dir.join("machine.yaml"),
        LORE_QUERY_GENERIC_FIELDS_MACHINE_YAML,
    )
    .map_err(|e| format!("Failed to write lore_query machine.yaml: {}", e))?;
    std::fs::write(
        hooks_dir.join("answering.md"),
        "# Answering\n\nAnswer: {{question}} (for {{requester}})\n",
    )
    .map_err(|e| format!("Failed to write lore_query hook: {}", e))?;
    Ok(())
}

const LORE_QUERY_GENERIC_FIELDS_MACHINE_YAML: &str = r#"kind: lore_query
directory: lore_queries
registry: lore_queries.md
parent_kind: ~
description: "Answer a natural-language question from Lore evidence."
required_fields:
  - name: question
    field_type: string
    description: The natural-language question to answer.
  - name: requester
    field_type: actor_name
    description: Actor that initiated the query run.
roles:
  - doer
  - reviewer
states:
  - name: answering
    role_filters:
      - doer_actionable
    registry_section: "Answering"
    projection_targets: []
    is_review_gate: false
    is_terminal: false
    hook: answering.md
  - name: completed
    role_filters:
      - terminal
    registry_section: "Completed"
    projection_targets: []
    is_review_gate: false
    is_terminal: true
    hook: ~
transitions:
  - from_state: answering
    to_state: completed
    required_role: doer
    requires_approver: false
"#;

/// Seed a throwaway hearth holding a single `k5_probe` DRIVEN machine in one of
/// several shapes, plus the structural signature a driven-playbook create needs
/// (tracks/ + tracks.md and the k5_probes/ instance dir + k5_probes.md registry).
/// Every shape shares kind=k5_probe / directory=k5_probes so the begin / complete
/// / snapshot RPC steps drive them uniformly; only the states/transitions vary, to
/// exercise each leg of the R10 bindability precondition at the real gRPC seam.
fn seed_k5_probe_hearth(tmp: &std::path::Path, shape: &str) -> Result<(), String> {
    std::fs::create_dir_all(tmp).map_err(|e| format!("Failed to create hearth: {}", e))?;
    std::fs::create_dir_all(tmp.join("tracks"))
        .map_err(|e| format!("Failed to create tracks dir: {}", e))?;
    std::fs::write(tmp.join("tracks.md"), "# Tracks\n")
        .map_err(|e| format!("Failed to write tracks.md: {}", e))?;
    std::fs::create_dir_all(tmp.join("k5_probes"))
        .map_err(|e| format!("Failed to create k5_probes dir: {}", e))?;
    std::fs::write(tmp.join("k5_probes.md"), "# K5 Probes\n")
        .map_err(|e| format!("Failed to write k5_probes.md: {}", e))?;

    let wf_dir = tmp.join("playbooks").join("k5_probe");
    std::fs::create_dir_all(&wf_dir)
        .map_err(|e| format!("Failed to create k5_probe playbook dir: {}", e))?;
    std::fs::write(wf_dir.join("machine.yaml"), k5_probe_machine_yaml(shape)?)
        .map_err(|e| format!("Failed to write k5_probe machine.yaml: {}", e))?;
    Ok(())
}

/// Render one fully-specified state entry for a `k5_probe` machine.yaml (mirrors
/// the `k5_bindable_min` fixture's field set so the loader accepts it).
fn k5_probe_state(name: &str, section: &str, gate: bool, terminal: bool) -> String {
    format!(
        "  - name: {name}\n    role_filters: []\n    registry_section: {section}\n    projection_targets: []\n    is_review_gate: {gate}\n    is_terminal: {terminal}\n    hook: ~\n    hooks_by_role: {{}}\n    measurement_by_role: {{}}\n"
    )
}

/// Render the full DRIVEN `k5_probe` machine.yaml for a named shape. Each shape is
/// a valid, loadable driven machine that differs ONLY in whether it satisfies the
/// R10 K5-bindability precondition (bindability.rs), so a rejection at the seam is
/// provably the bindability gate and not a load failure.
fn k5_probe_machine_yaml(shape: &str) -> Result<String, String> {
    let header = "kind: k5_probe\ndirectory: k5_probes\nregistry: k5_probes.md\nparent_kind: ~\ndescription: \"K5 seam probe machine.\"\n";
    let roles = "roles:\n  - doer\n  - reviewer\n";
    // A machine-declared required create field, present ONLY for the
    // "requires-field" shape so a begin omitting it is rejected create-or-nothing
    // (§4.3a `required_field_missing`); every other shape declares none.
    let required_fields = match shape {
        "requires-field" => {
            "required_fields:\n  - name: dossier_ref\n    field_type: string\n    description: \"probe-required create field\"\n"
        }
        _ => "required_fields: []\n",
    };
    let (states, transitions): (String, &str) = match shape {
        // BINDABLE: initial `drafting` has the single null-sat doer edge straight to
        // terminal `completed`, plus a [abandoned]-satisfaction park edge (check b).
        // "requires-field" shares the bindable shape so a rejection is provably the
        // missing-field precondition, not a bindability failure.
        "bindable" | "requires-field" => (
            format!(
                "{}{}{}",
                k5_probe_state("drafting", "active", false, false),
                k5_probe_state("completed", "completed", false, true),
                k5_probe_state("abandoned", "abandoned", false, true),
            ),
            "transitions:\n  - {from_state: drafting, to_state: completed, required_role: doer, required_satisfaction: ~, requires_approver: false}\n  - {from_state: drafting, to_state: abandoned, required_role: doer, required_satisfaction: [abandoned], requires_approver: false}\n",
        ),
        // `completed` sits behind an is_review_gate state → from `drafting` the sole
        // forward candidate targets `review`, not `completed` (check (a)-3).
        "review-gated-completed" => (
            format!(
                "{}{}{}{}",
                k5_probe_state("drafting", "active", false, false),
                k5_probe_state("review", "active", true, false),
                k5_probe_state("completed", "completed", false, true),
                k5_probe_state("abandoned", "abandoned", false, true),
            ),
            "transitions:\n  - {from_state: drafting, to_state: review, required_role: doer, required_satisfaction: ~, requires_approver: false}\n  - {from_state: review, to_state: completed, required_role: reviewer, required_satisfaction: [satisfied], requires_approver: false}\n  - {from_state: drafting, to_state: abandoned, required_role: doer, required_satisfaction: [abandoned], requires_approver: false}\n",
        ),
        // Two doer steps: from `drafting`, C = {drafting->s1}, target != completed
        // (the original M1 >1-step class; check (a)-3).
        "two-doer-step" => (
            format!(
                "{}{}{}{}",
                k5_probe_state("drafting", "active", false, false),
                k5_probe_state("s1", "active", false, false),
                k5_probe_state("completed", "completed", false, true),
                k5_probe_state("abandoned", "abandoned", false, true),
            ),
            "transitions:\n  - {from_state: drafting, to_state: s1, required_role: doer, required_satisfaction: ~, requires_approver: false}\n  - {from_state: s1, to_state: completed, required_role: doer, required_satisfaction: ~, requires_approver: false}\n  - {from_state: drafting, to_state: abandoned, required_role: doer, required_satisfaction: [abandoned], requires_approver: false}\n",
        ),
        // Sibling forward edge: drafting->completed AND drafting->s1, both null-sat
        // doer → |C| == 2 (the complete.rs terminal filter would strand at s1;
        // check (a)-2, the F1 recast fail-open).
        "sibling-forward-edge" => (
            format!(
                "{}{}{}{}",
                k5_probe_state("drafting", "active", false, false),
                k5_probe_state("s1", "active", false, false),
                k5_probe_state("completed", "completed", false, true),
                k5_probe_state("abandoned", "abandoned", false, true),
            ),
            "transitions:\n  - {from_state: drafting, to_state: completed, required_role: doer, required_satisfaction: ~, requires_approver: false}\n  - {from_state: drafting, to_state: s1, required_role: doer, required_satisfaction: ~, requires_approver: false}\n  - {from_state: s1, to_state: completed, required_role: doer, required_satisfaction: ~, requires_approver: false}\n  - {from_state: drafting, to_state: abandoned, required_role: doer, required_satisfaction: [abandoned], requires_approver: false}\n",
        ),
        // Null-sat abandon collision: drafting->completed AND drafting->abandoned,
        // both null-sat doer → |C| == 2 (this is WHY the park edge must carry
        // required_satisfaction: [abandoned]; check (a)-2 / F1).
        "null-sat-abandon" => (
            format!(
                "{}{}{}",
                k5_probe_state("drafting", "active", false, false),
                k5_probe_state("completed", "completed", false, true),
                k5_probe_state("abandoned", "abandoned", false, true),
            ),
            "transitions:\n  - {from_state: drafting, to_state: completed, required_role: doer, required_satisfaction: ~, requires_approver: false}\n  - {from_state: drafting, to_state: abandoned, required_role: doer, required_satisfaction: ~, requires_approver: false}\n",
        ),
        // Missing abandon edge (weekly_recap shape): single drafting->completed doer
        // edge, NO `abandoned` state/edge → passes check (a), fails check (b) (F2).
        "missing-abandon-edge" => (
            format!(
                "{}{}",
                k5_probe_state("drafting", "active", false, false),
                k5_probe_state("completed", "completed", false, true),
            ),
            "transitions:\n  - {from_state: drafting, to_state: completed, required_role: doer, required_satisfaction: ~, requires_approver: false}\n",
        ),
        other => return Err(format!("Unknown K5 probe machine shape '{}'", other)),
    };
    Ok(format!(
        "{header}{required_fields}{roles}states:\n{states}{transitions}register: driven\n"
    ))
}

fn seed_daily_recap_engine_hearth(tmp: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(tmp).map_err(|e| format!("Failed to create hearth: {}", e))?;
    std::fs::create_dir_all(tmp.join("tracks"))
        .map_err(|e| format!("Failed to create tracks dir: {}", e))?;
    std::fs::write(tmp.join("tracks.md"), "# Tracks\n")
        .map_err(|e| format!("Failed to write tracks.md: {}", e))?;
    std::fs::create_dir_all(tmp.join("daily_recaps"))
        .map_err(|e| format!("Failed to create daily_recaps dir: {}", e))?;
    std::fs::write(tmp.join("daily_recaps.md"), "# Daily Recaps\n")
        .map_err(|e| format!("Failed to write daily_recaps.md: {}", e))?;

    let proj_dir = tmp.join("projections");
    std::fs::create_dir_all(&proj_dir)
        .map_err(|e| format!("Failed to create projections dir: {}", e))?;
    std::fs::write(
        proj_dir.join("execution.md"),
        "---\nincremental_count: 0\nbase_snapshot: 2026-06-01T00:00:00Z\nlast_updated: 2026-06-01T00:00:00Z\nafter_event: \"seed\"\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n",
    )
    .map_err(|e| format!("Failed to write execution.md: {}", e))?;

    let wf_dir = tmp.join("playbooks").join("daily_recap");
    let hooks_dir = wf_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir)
        .map_err(|e| format!("Failed to create daily_recap hooks dir: {}", e))?;
    std::fs::write(
        wf_dir.join("machine.yaml"),
        read_daily_recap_fixture_file("machine.yaml")?,
    )
    .map_err(|e| format!("Failed to write daily_recap machine.yaml: {}", e))?;
    for hook in ["gathering.md", "synthesizing.md", "reporting.md"] {
        std::fs::write(
            hooks_dir.join(hook),
            read_daily_recap_fixture_file(&format!("hooks/{}", hook))?,
        )
        .map_err(|e| format!("Failed to write daily_recap hook {}: {}", hook, e))?;
    }
    Ok(())
}

/// Minimal-but-valid machine.yaml for a route fixture, with an explicit
/// `register`. The kind doubles as directory/registry names.
fn route_machine_yaml(kind: &str, register: &str) -> String {
    format!(
        r#"kind: {kind}
directory: {kind}s
registry: {kind}s.md
description: "Route fixture {kind} ({register})."
register: {register}
required_fields: []
roles:
  - doer
  - reviewer
states:
  - name: active
    role_filters: []
    registry_section: active
    projection_targets: []
    is_review_gate: false
    is_terminal: false
  - name: completed
    role_filters: []
    registry_section: completed
    projection_targets: []
    is_review_gate: false
    is_terminal: true
transitions:
  - from_state: active
    to_state: completed
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
"#,
        kind = kind,
        register = register
    )
}

fn route_machine_yaml_with_access(
    kind: &str,
    register: &str,
    org: &str,
    role: &str,
    sensitivity: &str,
) -> String {
    format!(
        r#"kind: {kind}
directory: {kind}s
registry: {kind}s.md
description: "Route fixture {kind} ({register})."
access:
  org: "{org}"
  min_role: {role}
  sensitivity: {sensitivity}
  space: ~
register: {register}
required_fields: []
roles:
  - doer
  - reviewer
states:
  - name: active
    role_filters: []
    registry_section: active
    projection_targets: []
    is_review_gate: false
    is_terminal: false
  - name: completed
    role_filters: []
    registry_section: completed
    projection_targets: []
    is_review_gate: false
    is_terminal: true
transitions:
  - from_state: active
    to_state: completed
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
"#,
        kind = kind,
        register = register,
        org = org,
        role = role,
        sensitivity = sensitivity
    )
}

fn machine_yaml_with_access(
    yaml: &str,
    org: &str,
    role: &str,
    sensitivity: &str,
) -> Result<String, String> {
    let marker = "required_fields:";
    let idx = yaml
        .find(marker)
        .ok_or_else(|| "machine.yaml fixture lacks required_fields marker".to_string())?;
    Ok(format!(
        "{}access:\n  org: \"{}\"\n  min_role: {}\n  sensitivity: {}\n  space: ~\n{}",
        &yaml[..idx],
        org,
        role,
        sensitivity,
        &yaml[idx..]
    ))
}

fn fixture_playbook_dir(kind: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(kind)
}

fn copy_fixture_playbook(kind: &str, hearth: &Path) -> Result<(), String> {
    let source = fixture_playbook_dir(kind);
    let target = hearth.join("playbooks").join(kind);
    std::fs::create_dir_all(&target)
        .map_err(|e| format!("Failed to create {}: {}", target.display(), e))?;

    let source_machine = source.join("machine.yaml");
    std::fs::copy(&source_machine, target.join("machine.yaml")).map_err(|e| {
        format!(
            "Failed to copy {} into route hearth: {}",
            source_machine.display(),
            e
        )
    })?;

    let source_hooks = source.join("hooks");
    if source_hooks.exists() {
        let target_hooks = target.join("hooks");
        std::fs::create_dir_all(&target_hooks)
            .map_err(|e| format!("Failed to create {}: {}", target_hooks.display(), e))?;
        for entry in std::fs::read_dir(&source_hooks)
            .map_err(|e| format!("Failed to read {}: {}", source_hooks.display(), e))?
        {
            let entry = entry.map_err(|e| format!("Failed to read hook entry: {}", e))?;
            if entry
                .file_type()
                .map_err(|e| format!("Failed to read hook file type: {}", e))?
                .is_file()
            {
                std::fs::copy(entry.path(), target_hooks.join(entry.file_name()))
                    .map_err(|e| format!("Failed to copy hook file: {}", e))?;
            }
        }
    }

    Ok(())
}

/// Create a minimal route-fixture hearth (the engine hearth predicate needs
/// tracks/ + tracks.md) with no playbook machines yet.
fn seed_empty_route_hearth(tmp: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(tmp.join("tracks"))
        .map_err(|e| format!("Failed to create tracks dir: {}", e))?;
    std::fs::write(tmp.join("tracks.md"), "# Tracks\n")
        .map_err(|e| format!("Failed to write tracks.md: {}", e))?;
    std::fs::create_dir_all(tmp.join("playbooks"))
        .map_err(|e| format!("Failed to create playbooks dir: {}", e))?;
    Ok(())
}

/// resume_aware_routing — seed an open (begun, non-terminal) artifact with a
/// durable open-begin marker carrying `conversation_id`. Writes
/// `{kind}s/{id}/status.yaml` with the kind/state, a single creation transition
/// by a creating actor, and an `activity:` begin marker (by a distinct actor)
/// stamping the conversation_id and begun-at timestamp.
fn write_open_artifact(
    hearth: &std::path::Path,
    kind: &str,
    id: &str,
    state: &str,
    conversation_id: &str,
    begun_at: &str,
) -> Result<(), String> {
    let dir = hearth.join(format!("{}s", kind)).join(id);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create artifact dir: {}", e))?;
    let status = format!(
        r#"version: 1
kind: {kind}
state: {state}
transitions:
  - to: {state}
    at: "2026-06-01T00:00:00Z"
    actor: Creator-000000
    role: doer
activity:
  - kind: begin
    actor: Beginner-111111
    state: {state}
    at: "{begun_at}"
    conversation_id: "{conversation_id}"
"#,
        kind = kind,
        state = state,
        begun_at = begun_at,
        conversation_id = conversation_id
    );
    std::fs::write(dir.join("status.yaml"), status)
        .map_err(|e| format!("Failed to write status.yaml: {}", e))
}

/// Write a route machine into the hearth under `workflows/{kind}_dir/`.
fn write_route_machine(hearth: &std::path::Path, kind: &str, register: &str) -> Result<(), String> {
    let dir = hearth.join("playbooks").join(format!("{}_dir", kind));
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create dir: {}", e))?;
    std::fs::write(dir.join("machine.yaml"), route_machine_yaml(kind, register))
        .map_err(|e| format!("Failed to write machine.yaml: {}", e))
}

/// An UNRESTRICTED driven route machine (no access block → granted under
/// default-safe ctx) carrying a single trigger phrase, so a route message equal
/// to the trigger auto-resolves to Single. Used by the route-turn binary seam.
fn write_route_machine_with_trigger(
    hearth: &std::path::Path,
    kind: &str,
    trigger: &str,
) -> Result<(), String> {
    let dir = hearth.join("playbooks").join(format!("{}_dir", kind));
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create dir: {}", e))?;
    let yaml = format!(
        r#"kind: {kind}
directory: {kind}s
registry: {kind}s.md
description: "Route fixture {kind} (driven, triggered)."
register: driven
route:
  triggers: ["{trigger}"]
required_fields: []
roles:
  - doer
  - reviewer
states:
  - name: active
    role_filters: []
    registry_section: active
    projection_targets: []
    is_review_gate: false
    is_terminal: false
  - name: completed
    role_filters: []
    registry_section: completed
    projection_targets: []
    is_review_gate: false
    is_terminal: true
transitions:
  - from_state: active
    to_state: completed
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
"#,
        kind = kind,
        trigger = trigger
    );
    std::fs::write(dir.join("machine.yaml"), yaml)
        .map_err(|e| format!("Failed to write machine.yaml: {}", e))
}

/// An UNRESTRICTED driven route machine carrying a trigger, a one-line
/// description (purpose), and required fields — so the route-turn binary seam can
/// assert that the spoon-fed guidance surfaces purpose + required fields.
fn write_route_machine_with_trigger_meta(
    hearth: &std::path::Path,
    kind: &str,
    trigger: &str,
    description: &str,
    required_fields: &[String],
) -> Result<(), String> {
    let dir = hearth.join("playbooks").join(format!("{}_dir", kind));
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create dir: {}", e))?;
    let fields_yaml = if required_fields.is_empty() {
        "required_fields: []\n".to_string()
    } else {
        let mut s = String::from("required_fields:\n");
        for f in required_fields {
            s.push_str(&format!(
                "  - name: {f}\n    field_type: string\n    description: \"{f}\"\n",
                f = f
            ));
        }
        s
    };
    let yaml = format!(
        r#"kind: {kind}
directory: {kind}s
registry: {kind}s.md
description: "{description}"
register: driven
route:
  triggers: ["{trigger}"]
{fields_yaml}roles:
  - doer
  - reviewer
states:
  - name: active
    role_filters: []
    registry_section: active
    projection_targets: []
    is_review_gate: false
    is_terminal: false
  - name: completed
    role_filters: []
    registry_section: completed
    projection_targets: []
    is_review_gate: false
    is_terminal: true
transitions:
  - from_state: active
    to_state: completed
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
"#,
        kind = kind,
        trigger = trigger,
        description = description,
        fields_yaml = fields_yaml,
    );
    std::fs::write(dir.join("machine.yaml"), yaml)
        .map_err(|e| format!("Failed to write machine.yaml: {}", e))
}

/// Write a route machine whose description is `description_bytes` long and whose
/// state list is multi-step, so its annotations (intent = description fallback +
/// step_outline) are large. Used by the route-response budget scenario to push a
/// matching candidate set past `ROUTE_RESPONSE_BUDGET_BYTES` and exercise the
/// deterministic truncation fallback. The trigger is shared so several such
/// machines co-match the same message.
fn write_oversized_route_machine(
    hearth: &std::path::Path,
    kind: &str,
    trigger: &str,
    description_bytes: usize,
) -> Result<(), String> {
    let dir = hearth.join("playbooks").join(format!("{}_dir", kind));
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create dir: {}", e))?;
    // A long single-line description (no quotes/newlines so the YAML stays valid).
    let description: String = std::iter::repeat('x').take(description_bytes).collect();
    let yaml = format!(
        r#"kind: {kind}
directory: {kind}s
registry: {kind}s.md
description: "{description}"
register: driven
route:
  triggers: ["{trigger}"]
required_fields: []
roles:
  - doer
  - reviewer
states:
  - name: gathering
    role_filters: []
    registry_section: gathering
    projection_targets: []
    is_review_gate: false
    is_terminal: false
  - name: drafting
    role_filters: []
    registry_section: drafting
    projection_targets: []
    is_review_gate: false
    is_terminal: false
  - name: reporting
    role_filters: []
    registry_section: reporting
    projection_targets: []
    is_review_gate: false
    is_terminal: false
  - name: completed
    role_filters: []
    registry_section: completed
    projection_targets: []
    is_review_gate: false
    is_terminal: true
transitions:
  - from_state: gathering
    to_state: drafting
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
  - from_state: drafting
    to_state: reporting
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
  - from_state: reporting
    to_state: completed
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
"#,
        kind = kind,
        trigger = trigger,
        description = description,
    );
    std::fs::write(dir.join("machine.yaml"), yaml)
        .map_err(|e| format!("Failed to write machine.yaml: {}", e))
}

/// Write a SINGLE-resolution driven route machine whose initial (state, doer)
/// hook file EXISTS (so the machine loads) but is UNREADABLE (mode 000), so the
/// route guidance enrichment read fails at request time. This exercises the
/// engine's fail-open: the Route RPC must still return a valid THIN response
/// (the candidate surfaced, `guidance` empty) — never a gRPC error. Routing
/// selection (single) is unchanged; only the enrichment fails.
fn write_route_machine_with_broken_hook(
    hearth: &std::path::Path,
    kind: &str,
    trigger: &str,
) -> Result<(), String> {
    let dir = hearth.join("playbooks").join(format!("{}_dir", kind));
    let hooks = dir.join("hooks");
    std::fs::create_dir_all(&hooks).map_err(|e| format!("Failed to create dir: {}", e))?;
    let yaml = format!(
        r#"kind: {kind}
directory: {kind}s
registry: {kind}s.md
description: "Route fixture {kind} with an unreadable hook."
register: driven
route:
  triggers: ["{trigger}"]
required_fields: []
roles:
  - doer
  - reviewer
states:
  - name: active
    role_filters: []
    registry_section: active
    projection_targets: []
    is_review_gate: false
    is_terminal: false
    hook: answering.md
  - name: completed
    role_filters: []
    registry_section: completed
    projection_targets: []
    is_review_gate: false
    is_terminal: true
transitions:
  - from_state: active
    to_state: completed
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
"#,
        kind = kind,
        trigger = trigger
    );
    std::fs::write(dir.join("machine.yaml"), yaml)
        .map_err(|e| format!("Failed to write machine.yaml: {}", e))?;
    let hook_path = hooks.join("answering.md");
    std::fs::write(&hook_path, "# Answering\n\nFirst-step body.\n")
        .map_err(|e| format!("Failed to write hook: {}", e))?;
    // Make the hook UNREADABLE so the load-time listing still sees it (the file
    // exists), but the request-time read_to_string fails — the deterministic
    // enrichment failure the engine must fail open on.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook_path, std::fs::Permissions::from_mode(0o000))
            .map_err(|e| format!("Failed to chmod hook: {}", e))?;
    }
    Ok(())
}

fn write_route_machine_with_access(
    hearth: &std::path::Path,
    kind: &str,
    register: &str,
    org: &str,
    role: &str,
    sensitivity: &str,
) -> Result<(), String> {
    let dir = hearth.join("playbooks").join(format!("{}_dir", kind));
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create dir: {}", e))?;
    std::fs::write(
        dir.join("machine.yaml"),
        route_machine_yaml_with_access(kind, register, org, role, sensitivity),
    )
    .map_err(|e| format!("Failed to write machine.yaml: {}", e))
}

const ACCESS_SCOPING_PARENT_TRACK_ID: &str = "20260607T0000_access_scoping_parent";

fn seed_restricted_playbook_engine_hearth(tmp: &std::path::Path) -> Result<(), String> {
    seed_empty_route_hearth(tmp)?;
    std::fs::create_dir_all(tmp.join("playbooks"))
        .map_err(|e| format!("Failed to create playbooks dir: {}", e))?;
    std::fs::write(tmp.join("workflows.md"), "# Playbooks\n\n## draft\n")
        .map_err(|e| format!("Failed to write workflows.md: {}", e))?;

    let track_dir = tmp.join("tracks").join(ACCESS_SCOPING_PARENT_TRACK_ID);
    std::fs::create_dir_all(&track_dir)
        .map_err(|e| format!("Failed to create parent track dir: {}", e))?;
    std::fs::write(
        track_dir.join("status.yaml"),
        "version: 1\nkind: track\nstate: active\n",
    )
    .map_err(|e| format!("Failed to write parent track status: {}", e))?;
    std::fs::write(track_dir.join("spec.md"), "# Access Scoping Parent\n")
        .map_err(|e| format!("Failed to write parent track spec: {}", e))?;

    let wf_dir = tmp
        .join("playbooks")
        .join("20260607T0000_workflow_lifecycle");
    std::fs::create_dir_all(&wf_dir)
        .map_err(|e| format!("Failed to create playbook lifecycle dir: {}", e))?;
    std::fs::write(
        wf_dir.join("machine.yaml"),
        route_machine_yaml_with_access("playbook", "driven", "acme", "admin", "phi"),
    )
    .map_err(|e| format!("Failed to write restricted playbook machine.yaml: {}", e))?;

    let proj_dir = tmp.join("projections");
    std::fs::create_dir_all(&proj_dir)
        .map_err(|e| format!("Failed to create projections dir: {}", e))?;
    std::fs::write(
        proj_dir.join("execution.md"),
        "---\nincremental_count: 0\nbase_snapshot: 2026-06-01T00:00:00Z\nlast_updated: 2026-06-01T00:00:00Z\nafter_event: \"seed\"\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n",
    )
    .map_err(|e| format!("Failed to write execution.md: {}", e))?;

    Ok(())
}

/// A unique scratch hearth under temp_dir, retained for scenario cleanup.
fn fresh_route_hearth(label: &str) -> Result<(RetainedTempDir, std::path::PathBuf), String> {
    retained_temp_dir(&format!("anvil-route-{}-", label))
}

fn apply_kiln_router_env(ctx: &Context, cmd: &mut Command) {
    let port = ctx
        .get::<i64>("kiln_router_port")
        .and_then(|port| u16::try_from(*port).ok())
        .or_else(|| closed_kiln_router_port().ok());
    if let Some(port) = port {
        cmd.env("ANVIL_KILN_PORT", port.to_string());
    }
    cmd.env("ANVIL_KILN_TIMEOUT_MS", "500");
    // Keep anvil's unconditional fleet telemetry out of the real `~/.anvil`.
    cmd.env("ANVIL_TELEMETRY_DIR", brine_fleet_telemetry_dir());
}

fn closed_kiln_router_port() -> Result<u16, String> {
    free_port()
}

/// context_aware_routing: a Kiln stub that RECORDS the full HTTP request to
/// `request_file` before replying with `content` as the assistant message. Reads
/// the request headers, parses `Content-Length`, and reads exactly that many body
/// bytes (the client keeps the socket open to read the reply, so we cannot wait
/// for EOF). Returns the bound port.
fn start_kiln_router_capturing_stub(
    request_file: &Path,
    content: serde_json::Value,
) -> Result<u16, String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("Failed to bind capturing kiln stub: {}", e))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("Failed to read capturing kiln stub port: {}", e))?
        .port();
    let body = serde_json::json!({
        "choices": [ { "message": { "content": content } } ]
    })
    .to_string();
    let request_file = request_file.to_path_buf();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        // Read until headers complete, then read the declared body length.
        let mut buf: Vec<u8> = Vec::new();
        let mut chunk = [0_u8; 8192];
        let mut header_end: Option<usize> = None;
        let mut content_length: Option<usize> = None;
        loop {
            if header_end.is_none() {
                if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
                    header_end = Some(pos + 4);
                    let headers = String::from_utf8_lossy(&buf[..pos]).to_lowercase();
                    for line in headers.lines() {
                        if let Some(v) = line.strip_prefix("content-length:") {
                            content_length = v.trim().parse::<usize>().ok();
                        }
                    }
                }
            }
            if let (Some(he), Some(cl)) = (header_end, content_length) {
                if buf.len() >= he + cl {
                    break;
                }
            }
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(_) => break,
            }
        }
        let _ = std::fs::write(&request_file, &buf);
        let response = format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             \r\n\
             {}",
            body.len(),
            body
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
    });
    Ok(port)
}

/// Build a Kiln router stub from a spec, returning `(Option<port>,
/// Option<request_file_path>)`. `answer:<kind>` binds a CAPTURING stub (records the
/// request + replies with `{kind}`); `hang` accepts + never responds; `closed` returns
/// a free port nobody listens on; `absent` sets no port.
fn build_tier_stub(
    hearth: &Path,
    label: &str,
    spec: &str,
) -> Result<(Option<u16>, Option<String>), String> {
    if spec == "absent" {
        return Ok((None, None));
    }
    if spec == "closed" {
        return Ok((Some(free_port()?), None));
    }
    if spec == "hang" {
        return Ok((Some(spawn_hung_kiln_stub()?), None));
    }
    // `error:<status>:<code>:<message>` — a stub that replies with a REAL kiln
    // error envelope at that status. Kiln returns structured JSON on every
    // refusal path (`{"error":{message,type,code}}`), so a stub that answered
    // with a bare status would test a shape production never sends.
    if let Some(rest) = spec.strip_prefix("error:") {
        let mut parts = rest.splitn(3, ':');
        let status: u16 = parts
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| format!("error stub spec '{}' has no status", spec))?;
        let code = parts.next().unwrap_or_default().to_string();
        let message = parts.next().unwrap_or_default().to_string();
        return Ok((Some(spawn_kiln_error_stub(status, &code, &message)?), None));
    }
    if let Some(kind) = spec.strip_prefix("answer:") {
        let request_file = hearth.join(format!("{}_request.txt", label));
        let port =
            start_kiln_router_capturing_stub(&request_file, serde_json::json!({ "kind": kind }))?;
        return Ok((Some(port), Some(request_file.to_string_lossy().to_string())));
    }
    Err(format!("unknown tier stub spec '{}'", spec))
}

/// A kiln stub that replies with a non-2xx status carrying kiln's own error
/// envelope. Reads and discards the request (the client keeps the socket open for
/// the reply), then writes the status line + body and closes.
fn spawn_kiln_error_stub(status: u16, code: &str, message: &str) -> Result<u16, String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("Failed to bind error kiln stub: {}", e))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("error kiln stub addr: {}", e))?
        .port();
    let body =
        serde_json::json!({ "error": { "message": message, "type": "kiln", "code": code } })
            .to_string();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut chunk = [0_u8; 8192];
        let _ = stream.read(&mut chunk);
        let response = format!(
            "HTTP/1.1 {status} ERROR\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             \r\n\
             {}",
            body.len(),
            body
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
    });
    Ok(port)
}

/// A kiln stub that accepts a connection and then HANGS (reads nothing, never
/// responds), so the client's per-tier time-box is what ends the attempt.
fn spawn_hung_kiln_stub() -> Result<u16, String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("Failed to bind hung kiln stub: {}", e))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("hung kiln stub addr: {}", e))?
        .port();
    std::thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            std::thread::sleep(std::time::Duration::from_secs(30));
            drop(stream);
        }
    });
    Ok(port)
}

/// Assert whether a tier's capturing stub recorded a request. An empty path means
/// no capturing stub was configured for that tier (a scenario-authoring error).
fn assert_tier_captured(ctx: &Context, key: &str, want: bool) -> Result<(), String> {
    let path = ctx
        .get::<String>(key)
        .ok_or_else(|| format!("No {}", key))?;
    if path.is_empty() {
        return Err(format!("{}: no capturing stub was configured", key));
    }
    let captured = std::fs::read(path)
        .map(|bytes| !bytes.is_empty())
        .unwrap_or(false);
    if captured == want {
        Ok(())
    } else {
        Err(format!(
            "{}: captured={}, wanted captured={}",
            key, captured, want
        ))
    }
}

/// Assert a tier's captured request body does / does not contain `needle`.
fn tier_request_contains(ctx: &Context, key: &str, needle: &str, want: bool) -> Result<(), String> {
    let path = ctx
        .get::<String>(key)
        .ok_or_else(|| format!("No {}", key))?;
    if path.is_empty() {
        return Err(format!("{}: no capturing stub was configured", key));
    }
    let body = std::fs::read_to_string(path).map_err(|e| format!("read {}: {}", key, e))?;
    if body.contains(needle) == want {
        Ok(())
    } else {
        Err(format!(
            "{} contains('{}')={}, wanted {}. Request was:\n{}",
            key,
            needle,
            body.contains(needle),
            want,
            body
        ))
    }
}

/// Find the first index of `needle` within `haystack`.
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn start_kiln_router_http_stub(
    hearth: &Path,
    content: serde_json::Value,
) -> Result<Context, String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("Failed to bind kiln router stub: {}", e))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("Failed to read kiln router stub port: {}", e))?
        .port();
    let body = serde_json::json!({
        "choices": [
            {
                "message": {
                    "content": content
                }
            }
        ]
    })
    .to_string();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request);
        let response = format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             \r\n\
             {}",
            body.len(),
            body
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
    });
    let mut out = Context::new();
    // Carry hearth_path forward: brine uses declared-output/replace semantics, so a
    // step that omits it would DROP hearth_path from the context and the following
    // "the engine is started with that hearth" step would be unsatisfied.
    out.set("hearth_path", hearth.to_path_buf());
    out.set("kiln_router_port", i64::from(port));
    Ok(out)
}

/// Spawn a real `anvil-engine` subprocess in Foundry mode for the R1 refusal
/// tests.
///
/// Sets `FOUNDRY_SESSION_TOKEN` (the at-spawn token, which the engine's
/// mode-detection reads to decide Standalone vs Foundry per D1) and a bogus
/// `FOUNDRY_BROKER_SOCKET` (never dialed when a stub verifier is selected).
/// When `stub` is `Some(kind)`, sets `ANVIL_TEST_SESSION_VERIFIER=<kind>` so
/// the debug build substitutes a hermetic test verifier for the broker
/// verifier (D4) — `stub_reject` rejects every token, `stub_unreachable`
/// simulates broker-unreachable (KeyFetch ⇒ fail-closed).
fn spawn_foundry_engine(
    session_token: &str,
    stub: Option<&str>,
) -> Result<(EngineProcess, PathBuf, RetainedTempDir, u16), String> {
    let (handle, tmp) = retained_temp_dir("anvil-test-foundry-hearth-")?;
    std::fs::create_dir_all(&tmp).map_err(|e| format!("Failed to create dir: {}", e))?;

    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("Failed to find free port: {}", e))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("Failed to get port: {}", e))?
        .port();
    drop(listener);

    let binary = crate::harness::binary_path("anvil-engine");

    let mut command = anvil_engine_command(&binary);
    command
        .arg("--hearth")
        .arg(tmp.to_str().unwrap())
        .arg("--port")
        .arg(port.to_string())
        .env("FOUNDRY_SESSION_TOKEN", session_token)
        .env(
            "FOUNDRY_BROKER_SOCKET",
            tmp.join("nonexistent-broker.sock").to_str().unwrap(),
        )
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(kind) = stub {
        command.env("ANVIL_TEST_SESSION_VERIFIER", kind);
    }

    let child = command.spawn().unwrap_or_else(|e| {
        panic!(
            "Failed to start anvil-engine at {}: {}",
            binary.display(),
            e
        )
    });

    // Poll the port until the engine accepts a TCP connection (readiness),
    // rather than a fixed sleep — under parallel scenario load a fixed wait
    // flakes. Foundry-mode startup does a little extra work (mode detection),
    // so wait up to ~5s.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            // Give the gRPC server a beat to finish binding after the TCP
            // listener is up.
            std::thread::sleep(std::time::Duration::from_millis(100));
            break;
        }
        if std::time::Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    let mut process = EngineProcess::without_capture(child, port);
    process.retain_temp_dir(&handle);
    Ok((process, tmp, handle, port))
}

/// Call the catalog RPC, optionally attaching `authorization: Bearer <jwt>`
/// gRPC metadata. Captures the gRPC status (code + message) on error so the
/// refusal assertions can check for `UNAUTHENTICATED` / `not_authenticated`.
async fn call_catalog_with_bearer(
    engine: &EngineProcess,
    bearer: Option<&str>,
) -> CatalogRpcResult {
    let addr = format!("http://127.0.0.1:{}", engine.port);
    match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
        Ok(mut client) => {
            let mut request = crate::surfaced(anvil_engine::proto::CatalogRequest {
                hearth_path: String::new(),
            });
            if let Some(token) = bearer {
                match format!("Bearer {}", token).parse() {
                    Ok(value) => {
                        request.metadata_mut().insert("authorization", value);
                    }
                    Err(e) => {
                        return CatalogRpcResult::Error(format!("Invalid bearer metadata: {}", e));
                    }
                }
            }
            match client.catalog(request).await {
                Ok(response) => {
                    CatalogRpcResult::Success(catalog_response_from_proto(response.into_inner()))
                }
                Err(status) => CatalogRpcResult::Status {
                    code: grpc_code_name(status.code()),
                    message: status.message().to_string(),
                },
            }
        }
        Err(e) => CatalogRpcResult::Error(format!("Connection failed: {}", e)),
    }
}

async fn call_catalog_with_hearth(engine: &EngineProcess, hearth_path: String) -> CatalogRpcResult {
    let addr = format!("http://127.0.0.1:{}", engine.port);
    match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
        Ok(mut client) => {
            let request = crate::surfaced(anvil_engine::proto::CatalogRequest { hearth_path });
            match client.catalog(request).await {
                Ok(response) => {
                    CatalogRpcResult::Success(catalog_response_from_proto(response.into_inner()))
                }
                Err(status) => CatalogRpcResult::Status {
                    code: grpc_code_name(status.code()),
                    message: status.message().to_string(),
                },
            }
        }
        Err(e) => CatalogRpcResult::Error(format!("Connection failed: {}", e)),
    }
}

// ===== R2 helpers — accept-path Foundry spawn + bearer RPC + read-back =====

/// Spawn a real `anvil-engine` in Foundry mode with an ACCEPTING stub verifier
/// bound to `sub` (D4 accept path): `ANVIL_TEST_SESSION_VERIFIER=stub_accept`
/// plus `ANVIL_TEST_SESSION_SUB=<sub>`. A presented bearer verifies to a canned
/// `VerifiedSession { sub, .. }`, with no live broker. Returns the process, the
/// (empty) temp hearth dir, and the bound port.
fn spawn_foundry_engine_accept(
    sub: &str,
) -> Result<(EngineProcess, PathBuf, RetainedTempDir, u16), String> {
    let (handle, tmp, port) = make_temp_hearth_and_port("anvil-test-foundry-accept")?;
    let binary = crate::harness::binary_path("anvil-engine");
    let mut command = anvil_engine_command(&binary);
    command
        .arg("--hearth")
        .arg(tmp.to_str().unwrap())
        .arg("--port")
        .arg(port.to_string())
        .env("FOUNDRY_SESSION_TOKEN", "test-session-token")
        .env(
            "FOUNDRY_BROKER_SOCKET",
            tmp.join("nonexistent-broker.sock").to_str().unwrap(),
        )
        .env("ANVIL_TEST_SESSION_VERIFIER", "stub_accept")
        .env("ANVIL_TEST_SESSION_SUB", sub)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let child = command.spawn().unwrap_or_else(|e| {
        panic!(
            "Failed to start anvil-engine at {}: {}",
            binary.display(),
            e
        )
    });
    wait_for_engine_ready(port);
    let mut process = EngineProcess::without_capture(child, port);
    process.retain_temp_dir(&handle);
    Ok((process, tmp, handle, port))
}

/// Spawn a real `anvil-engine` in standalone mode (no Foundry env) — today's
/// behavior. Used by the Req-3 no-regression scenario.
fn spawn_standalone_engine() -> Result<(EngineProcess, PathBuf, RetainedTempDir, u16), String> {
    let (handle, tmp, port) = make_temp_hearth_and_port("anvil-test-standalone-principal")?;
    let binary = crate::harness::binary_path("anvil-engine");
    let child = anvil_engine_command(&binary)
        .arg("--hearth")
        .arg(tmp.to_str().unwrap())
        .arg("--port")
        .arg(port.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| {
            panic!(
                "Failed to start anvil-engine at {}: {}",
                binary.display(),
                e
            )
        });
    wait_for_engine_ready(port);
    let mut process = EngineProcess::without_capture(child, port);
    process.retain_temp_dir(&handle);
    Ok((process, tmp, handle, port))
}

fn make_temp_hearth_and_port(prefix: &str) -> Result<(RetainedTempDir, PathBuf, u16), String> {
    let (handle, tmp) = retained_temp_dir(&format!("{}-", prefix))?;
    std::fs::create_dir_all(&tmp).map_err(|e| format!("Failed to create dir: {}", e))?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("Failed to find free port: {}", e))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("Failed to get port: {}", e))?
        .port();
    drop(listener);
    Ok((handle, tmp, port))
}

fn wait_for_engine_ready(port: u16) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            std::thread::sleep(std::time::Duration::from_millis(100));
            break;
        }
        if std::time::Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// Seed a hearth with a proposal (active) and a track in `spec_review` state,
/// so a snapshot (spec_review → plan) or complete transition can run, and a
/// begin (create track under the proposal) can run. The seeded status.yaml has
/// no transitions yet; the RPC under test appends the one we read back.
fn seed_principal_binding_hearth(hearth: &PathBuf) -> Result<(), String> {
    let proposal_dir = hearth.join("proposals/20260411T2021_anvil_workflow_engine");
    std::fs::create_dir_all(&proposal_dir).map_err(|e| e.to_string())?;
    std::fs::write(
        proposal_dir.join("status.yaml"),
        "version: 1\nstate: active\n",
    )
    .map_err(|e| e.to_string())?;

    let track_dir = hearth.join("tracks/20260417T1000_principal_track");
    std::fs::create_dir_all(&track_dir).map_err(|e| e.to_string())?;
    std::fs::write(
        track_dir.join("status.yaml"),
        "version: 1\nkind: track\nstate: spec_review\n",
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(
        track_dir.join("spec.md"),
        "# Principal Track\n\nSpec body.\n",
    )
    .map_err(|e| e.to_string())?;

    std::fs::write(
        hearth.join("tracks.md"),
        "# Tracks\n\n## spec\n\n## spec_review\n\n- [Principal Track](tracks/20260417T1000_principal_track/) — principal track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## plan\n\n## completed\n",
    )
    .map_err(|e| e.to_string())?;

    let proj_dir = hearth.join("projections");
    std::fs::create_dir_all(&proj_dir).map_err(|e| e.to_string())?;
    std::fs::write(
        proj_dir.join("execution.md"),
        "---\nincremental_count: 0\nbase_snapshot: 2026-04-16T00:00:00Z\nlast_updated: 2026-04-16T00:00:00Z\nafter_event: \"\"\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Planned (0)\n",
    )
    .map_err(|e| e.to_string())?;

    // begin (create flow) reads workflows/<id>/hooks/spec-writing.md to build
    // the spec artifact text; seed it so the create path reaches the persistence
    // step. The seed registry returns "20260422T0000_track_lifecycle" as the
    // playbook_id for "track"; the fs adapter reads hooks/ from that directory.
    let playbook_id = "20260422T0000_track_lifecycle";
    let hooks_dir = hearth.join("playbooks").join(playbook_id).join("hooks");
    std::fs::create_dir_all(&hooks_dir).map_err(|e| e.to_string())?;
    std::fs::write(hooks_dir.join("spec-writing.md"), "Spec writing guidance.")
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Read the most recent transition `actor:` from `<hearth>/<artifact_path>/status.yaml`.
/// Resolve an artifact's current state by folding its per-file transition event
/// store (merged with any legacy array) — the same seam every engine read site
/// uses. State is the `to` of the latest folded transition.
fn resolved_state_in_hearth(hearth: &PathBuf, artifact_path: &str) -> Result<String, String> {
    let dir = hearth.join(artifact_path);
    let status_path = dir.join("status.yaml");
    let content = std::fs::read_to_string(&status_path)
        .map_err(|e| format!("Failed to read {}: {}", status_path.display(), e))?;
    let status: anvil_core::domain::status::FullStatusYaml = serde_yaml::from_str(&content)
        .map_err(|e| format!("Invalid status.yaml {}: {}", status_path.display(), e))?;
    anvil_core::domain::transition_log::resolve_state_with_events(&status, &dir)
        .map_err(|e| format!("unreadable transition evidence at {}: {e}", dir.display()))?
        .ok_or_else(|| format!("No resolvable state for {}", dir.display()))
}

/// Public read-back of the persisted transition actor, for step modules in
/// other crates (`anvil-engine-steps`) that need the SAME reader the in-crate
/// principal-binding checks use. A second implementation would be free to read
/// the legacy `status.yaml` actor array and quietly disagree with this one.
pub fn last_transition_actor_at(hearth: &PathBuf, artifact_path: &str) -> Result<String, String> {
    last_transition_actor(hearth, artifact_path)
}

fn last_transition_actor(hearth: &PathBuf, artifact_path: &str) -> Result<String, String> {
    // The transition log is now one-file-per-event under
    // `<artifact>/transitions/`, NOT the legacy status.yaml `actor:` lines. Fold
    // the event files (merged with any legacy array) and read the latest entry's
    // actor — the persisted principal of the most recent transition.
    let dir = hearth.join(artifact_path);
    let status_path = dir.join("status.yaml");
    let content = std::fs::read_to_string(&status_path)
        .map_err(|e| format!("Failed to read {}: {}", status_path.display(), e))?;
    let status: anvil_core::domain::status::FullStatusYaml = serde_yaml::from_str(&content)
        .map_err(|e| format!("Invalid status.yaml {}: {}", status_path.display(), e))?;
    let history =
        anvil_core::domain::transition_log::resolve_transitions_with_events(&status, &dir)
            .map_err(|e| format!("unreadable transition evidence at {}: {e}", dir.display()))?;
    match history.last().and_then(|t| t.actor.clone()) {
        Some(a) => Ok(a),
        None => Err(format!(
            "No transition actor found in event store or legacy array for {}",
            dir.display()
        )),
    }
}

fn insert_bearer(
    request: &mut tonic::Request<impl Sized>,
    bearer: Option<&str>,
) -> Result<(), String> {
    if let Some(token) = bearer {
        let value = format!("Bearer {}", token)
            .parse()
            .map_err(|e| format!("Invalid bearer metadata: {}", e))?;
        request.metadata_mut().insert("authorization", value);
    }
    Ok(())
}

async fn call_begin_create_with_bearer(
    engine: &EngineProcess,
    bearer: Option<&str>,
    actor_name: &str,
    track_name: &str,
    parent_id: &str,
) -> BeginRpcResult {
    let addr = format!("http://127.0.0.1:{}", engine.port);
    match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
        Ok(mut client) => {
            let mut request = crate::surfaced(anvil_engine::proto::BeginRequest {
                hearth_path: String::new(),
                artifact_type: "track".to_string(),
                parent_id: parent_id.to_string(),
                track_name: track_name.to_string(),
                playbook_name: String::new(),
                target_owner: String::new(),
                approver: "Approver-E2E".to_string(),
                actor_name: actor_name.to_string(),
                actor_type: "agent".to_string(),
                actor_model: "test".to_string(),
                actor_provider: "test".to_string(),
                actor_context_window: 0,
                actor_sdk_version: String::new(),
                actor_entrypoint: String::new(),
                identifier: String::new(),
                session_role: "creator".to_string(),
                ctx_org: "Foundation".to_string(),
                ctx_space: String::new(),
                ctx_role: "read".to_string(),
                ctx_clearance: "internal".to_string(),
                ..Default::default()
            });
            if let Err(e) = insert_bearer(&mut request, bearer) {
                return BeginRpcResult::Error {
                    code: "INTERNAL".to_string(),
                    message: e,
                };
            }
            match client.begin(request).await {
                Ok(response) => BeginRpcResult::Success(response.into_inner()),
                Err(status) => BeginRpcResult::Error {
                    code: grpc_code_name(status.code()),
                    message: status.message().to_string(),
                },
            }
        }
        Err(e) => BeginRpcResult::Error {
            code: "UNAVAILABLE".to_string(),
            message: format!("Connection failed: {}", e),
        },
    }
}

async fn call_snapshot_with_bearer(
    engine: &EngineProcess,
    bearer: Option<&str>,
    actor_name: &str,
    artifact_path: &str,
    to_state: &str,
) -> SnapshotRpcResult {
    let addr = format!("http://127.0.0.1:{}", engine.port);
    match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
        Ok(mut client) => {
            let mut request = crate::surfaced(anvil_engine::proto::SnapshotRequest {
                hearth_path: String::new(),
                artifact_path: artifact_path.to_string(),
                to_state: to_state.to_string(),
                actor_name: actor_name.to_string(),
                actor_role: "review".to_string(),
                approver: String::new(),
                note: String::new(),
                actor_type: "agent".to_string(),
                actor_model: "test".to_string(),
                actor_provider: "test".to_string(),
                actor_context_window: 0,
                actor_sdk_version: String::new(),
                actor_entrypoint: String::new(),
                projection_only: false,
                event_type: String::new(),
                conversation_id: String::new(),
                project_root: String::new(),
                claimed_evidence: Vec::new(),
            });
            if let Err(e) = insert_bearer(&mut request, bearer) {
                return SnapshotRpcResult::Error {
                    code: "INTERNAL".to_string(),
                    message: e,
                };
            }
            match client.snapshot(request).await {
                Ok(response) => SnapshotRpcResult::Success(response.into_inner()),
                Err(status) => SnapshotRpcResult::Error {
                    code: grpc_code_name(status.code()),
                    message: status.message().to_string(),
                },
            }
        }
        Err(e) => SnapshotRpcResult::Error {
            code: "UNAVAILABLE".to_string(),
            message: format!("Connection failed: {}", e),
        },
    }
}

async fn snapshot_to(
    client: &mut anvil_engine::proto::anvil_service_client::AnvilServiceClient<
        tonic::transport::Channel,
    >,
    artifact_path: &str,
    to_state: &str,
    actor_role: &str,
) -> Result<(), String> {
    let response = client
        .snapshot(crate::surfaced(anvil_engine::proto::SnapshotRequest {
            hearth_path: String::new(),
            artifact_path: artifact_path.to_string(),
            to_state: to_state.to_string(),
            actor_name: "Decision-E2E-100000".to_string(),
            actor_role: actor_role.to_string(),
            approver: "Approver-E2E".to_string(),
            note: String::new(),
            actor_type: "agent".to_string(),
            actor_model: "test".to_string(),
            actor_provider: "test".to_string(),
            actor_context_window: 0,
            actor_sdk_version: String::new(),
            actor_entrypoint: String::new(),
            projection_only: false,
            event_type: String::new(),
            conversation_id: String::new(),
            project_root: String::new(),
            claimed_evidence: Vec::new(),
        }))
        .await
        .map_err(|s| {
            format!(
                "snapshot to '{}' failed: {}: {}",
                to_state,
                grpc_code_name(s.code()),
                s.message()
            )
        })?
        .into_inner();
    if response.success {
        Ok(())
    } else {
        Err(format!("snapshot to '{}' returned success=false", to_state))
    }
}

#[allow(dead_code)]
fn assert_status_contains(
    hearth_path: &Path,
    artifact_path: &str,
    needle: &str,
) -> Result<(), String> {
    let status_path = hearth_path.join(artifact_path).join("status.yaml");
    let content = std::fs::read_to_string(&status_path)
        .map_err(|e| format!("Failed to read {}: {}", status_path.display(), e))?;
    if content.contains(needle) {
        Ok(())
    } else {
        Err(format!(
            "status.yaml does not contain '{}'. Content:\n{}",
            needle, content
        ))
    }
}

/// Assert an artifact's resolved current state (folded over the per-file
/// transition event store) equals `expected`. Replaces text matching on the
/// status.yaml `state:` line, which the event-store upcast no longer writes.
fn assert_resolved_state(
    hearth_path: &Path,
    artifact_path: &str,
    expected: &str,
) -> Result<(), String> {
    let dir = hearth_path.join(artifact_path);
    let status_path = dir.join("status.yaml");
    let content = std::fs::read_to_string(&status_path)
        .map_err(|e| format!("Failed to read {}: {}", status_path.display(), e))?;
    let status: anvil_core::domain::status::FullStatusYaml = serde_yaml::from_str(&content)
        .map_err(|e| format!("Invalid status.yaml {}: {}", status_path.display(), e))?;
    let state = anvil_core::domain::transition_log::resolve_state_with_events(&status, &dir)
        .map_err(|e| format!("unreadable transition evidence at {}: {e}", dir.display()))?
        .ok_or_else(|| format!("No resolvable state for {}", dir.display()))?;
    if state == expected {
        Ok(())
    } else {
        Err(format!(
            "Expected resolved state '{}' for '{}', got '{}'",
            expected, artifact_path, state
        ))
    }
}

async fn complete_to(
    client: &mut anvil_engine::proto::anvil_service_client::AnvilServiceClient<
        tonic::transport::Channel,
    >,
    artifact_path: &str,
    satisfaction: &str,
    expected_state: &str,
) -> Result<(), String> {
    let response = client
        .complete(crate::surfaced(anvil_engine::proto::CompleteRequest {
            hearth_path: String::new(),
            artifact_path: artifact_path.to_string(),
            actor_name: "Decision-E2E-100000".to_string(),
            actor_type: "agent".to_string(),
            actor_model: "test".to_string(),
            actor_provider: "test".to_string(),
            actor_context_window: 0,
            actor_sdk_version: String::new(),
            actor_entrypoint: String::new(),
            satisfaction: satisfaction.to_string(),
            approver: "Approver-E2E".to_string(),
            note: String::new(),
            reflection_notes: String::new(),
            findings: String::new(),
            conversation_id: String::new(),
            project_root: String::new(),
            claimed_evidence: Vec::new(),
        }))
        .await
        .map_err(|s| {
            format!(
                "complete satisfaction '{}' failed: {}: {}",
                satisfaction,
                grpc_code_name(s.code()),
                s.message()
            )
        })?
        .into_inner();
    if response.new_state == expected_state {
        Ok(())
    } else {
        Err(format!(
            "complete satisfaction '{}' expected '{}', got '{}'",
            satisfaction, expected_state, response.new_state
        ))
    }
}

async fn call_complete_with_bearer(
    engine: &EngineProcess,
    bearer: Option<&str>,
    actor_name: &str,
    artifact_path: &str,
    satisfaction: &str,
) -> CompleteRpcResult {
    let addr = format!("http://127.0.0.1:{}", engine.port);
    match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
        Ok(mut client) => {
            let mut request = crate::surfaced(anvil_engine::proto::CompleteRequest {
                hearth_path: String::new(),
                artifact_path: artifact_path.to_string(),
                actor_name: actor_name.to_string(),
                actor_type: "agent".to_string(),
                actor_model: "test".to_string(),
                actor_provider: "test".to_string(),
                actor_context_window: 0,
                actor_sdk_version: String::new(),
                actor_entrypoint: String::new(),
                satisfaction: satisfaction.to_string(),
                approver: String::new(),
                note: String::new(),
                reflection_notes: String::new(),
                findings: String::new(),
                conversation_id: String::new(),
                project_root: String::new(),
                claimed_evidence: Vec::new(),
            });
            if let Err(e) = insert_bearer(&mut request, bearer) {
                return CompleteRpcResult::Error {
                    code: "INTERNAL".to_string(),
                    message: e,
                };
            }
            match client.complete(request).await {
                Ok(response) => CompleteRpcResult::Success(response.into_inner()),
                Err(status) => CompleteRpcResult::Error {
                    code: grpc_code_name(status.code()),
                    message: status.message().to_string(),
                },
            }
        }
        Err(e) => CompleteRpcResult::Error {
            code: "UNAVAILABLE".to_string(),
            message: format!("Connection failed: {}", e),
        },
    }
}

/// Resolve the expected resolved_hearth value for an assertion. "<hearth>" and
/// "<hearth_x>" expand to the default/X hearth; "<hearth_y>" expands to hearth Y
/// (when present in context); otherwise the literal value.
fn expected_resolved_hearth(
    raw: &str,
    hearth: &PathBuf,
    hearth_y: Option<&PathBuf>,
) -> Result<String, String> {
    match raw {
        "<hearth>" | "<hearth_x>" => Ok(hearth.to_string_lossy().into_owned()),
        "<hearth_y>" => hearth_y
            .map(|p| p.to_string_lossy().into_owned())
            .ok_or_else(|| "No hearth_y_path in context for <hearth_y>".to_string()),
        _ => Ok(raw.to_string()),
    }
}

/// Compare two paths for equality after best-effort canonicalization (so a
/// symlinked temp dir like macOS's /var → /private/var compares equal).
fn canonical_eq(a: &str, b: &str) -> bool {
    let ca = std::fs::canonicalize(a).ok();
    let cb = std::fs::canonicalize(b).ok();
    match (ca, cb) {
        (Some(pa), Some(pb)) => pa == pb,
        _ => a == b,
    }
}

/// Recorded byte contents of every file under a hearth subtree, for the
/// isolation assertion (a transition on X leaves Y byte-unchanged).
#[derive(Debug, Clone)]
struct HearthSnapshot {
    files: std::collections::BTreeMap<PathBuf, Vec<u8>>,
}

/// Match one JSON field value against the table's expected (string) value.
///
/// - String fields compare verbatim.
/// - Bool/number fields compare via their JSON display form (`true`, `42`).
/// - Array fields (e.g. the `events` list) match if the expected value names a
///   single element contained in the array, OR equals the comma-joined form of
///   all elements (so `events=ReviewTransition` matches `["ReviewTransition"]`).
/// - The sentinel `<non-empty>` asserts only that the field is present and not
///   an empty string (used for the non-deterministic resolved-hearth path).
fn json_field_matches(field: &serde_json::Value, expected: &str) -> bool {
    if expected == "<non-empty>" {
        return match field {
            serde_json::Value::String(s) => !s.is_empty(),
            serde_json::Value::Null => false,
            _ => true,
        };
    }
    match field {
        // Exact match, or — for comma-joined list fields like `events` — match
        // when the expected value names a single element of the list.
        serde_json::Value::String(s) => {
            s == expected || s.split(',').any(|tok| tok.trim() == expected)
        }
        serde_json::Value::Bool(b) => b.to_string() == expected,
        serde_json::Value::Number(n) => n.to_string() == expected,
        serde_json::Value::Array(items) => {
            let strs: Vec<String> = items
                .iter()
                .map(|i| match i {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect();
            strs.iter().any(|s| s == expected) || strs.join(",") == expected
        }
        other => other.to_string() == expected,
    }
}

fn free_engine_port() -> Result<u16, String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("Failed to find free port: {}", e))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("Failed to get port: {}", e))?
        .port();
    drop(listener);
    Ok(port)
}

/// Fourth copy of the readiness poll, collapsed into the shared helper.
/// Four hand-maintained copies of one wait is how a timeout gets fixed in one
/// place and stays broken in three — the same "generate, don't duplicate" trap
/// that bites every N-copy constant in this repo.
fn wait_for_engine_health_ready(port: u16) -> bool {
    wait_until_grpc_ready(port)
}

fn start_engine_for_a1(
    default_hearth: Option<&Path>,
    permitted_roots: &[PathBuf],
) -> Result<EngineProcess, String> {
    start_engine_for_a2(default_hearth, permitted_roots, None)
}

fn start_engine_for_a2(
    default_hearth: Option<&Path>,
    permitted_roots: &[PathBuf],
    global_playbooks_hearth: Option<&Path>,
) -> Result<EngineProcess, String> {
    let port = free_engine_port()?;
    let binary = crate::harness::binary_path("anvil-engine");
    let mut command = anvil_engine_command(&binary);
    if let Some(hearth) = default_hearth {
        command.arg("--hearth").arg(hearth);
    }
    if let Some(global) = global_playbooks_hearth {
        command.arg("--global-playbooks-hearth").arg(global);
    }
    for root in permitted_roots {
        command.arg("--permitted-root").arg(root);
    }
    let child = command
        .arg("--port")
        .arg(port.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!(
                "Failed to start anvil-engine at {}: {}",
                binary.display(),
                e
            )
        })?;
    let process = EngineProcess::new(child, port);
    if wait_for_engine_health_ready(port) {
        Ok(process)
    } else {
        Err(format!(
            "anvil-engine did not answer gRPC HealthCheck on 127.0.0.1:{} within timeout",
            port
        ))
    }
}

#[cfg(unix)]
fn create_dir_symlink(target: &Path, link: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(target, link).map_err(|e| {
        format!(
            "Failed to create symlink {} -> {}: {}",
            link.display(),
            target.display(),
            e
        )
    })
}

#[cfg(not(unix))]
fn create_dir_symlink(_target: &Path, _link: &Path) -> Result<(), String> {
    Err("symlink escape scenario requires Unix symlink support".to_string())
}

/// Recursively capture (relative-path → bytes) for every file under `root`.
fn snapshot_tree(root: &std::path::Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    let mut out = std::collections::BTreeMap::new();
    fn walk(
        base: &std::path::Path,
        dir: &std::path::Path,
        out: &mut std::collections::BTreeMap<PathBuf, Vec<u8>>,
    ) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else if let Ok(bytes) = std::fs::read(&path) {
                let rel = path.strip_prefix(base).unwrap_or(&path).to_path_buf();
                out.insert(rel, bytes);
            }
        }
    }
    walk(root, root, &mut out);
    out
}

/// Write a complete, predicate-satisfying hearth into `dir`: tracks/ with one
/// track in `spec`, a tracks.md registry naming it, and projections/execution.md.
/// `tag` distinguishes the two hearths' track names/ids so cross-hearth crossover
/// is detectable.
fn seed_standard_hearth(dir: &std::path::Path, tag: &str) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(dir);
    let proposal_id = format!("20260411T2021_anvil_workflow_engine_{}", tag.to_lowercase());
    let track_id = format!("20260419T1100_track_{}", tag.to_lowercase());
    let track_dir = dir.join("tracks").join(&track_id);
    std::fs::create_dir_all(&track_dir)
        .map_err(|e| format!("Failed to create track dir: {}", e))?;

    let proposal_dir = dir.join("proposals").join(&proposal_id);
    std::fs::create_dir_all(&proposal_dir)
        .map_err(|e| format!("Failed to create proposal dir: {}", e))?;
    std::fs::write(
        proposal_dir.join("status.yaml"),
        "version: 1\nstate: active\n",
    )
    .map_err(|e| format!("Failed to write proposal status: {}", e))?;

    let status = format!(
        "version: 1\nkind: track\nstate: spec\nproposal: {}\nactors:\ntransitions:\n  - to: spec\n    at: 2026-04-19T00:00:00Z\n    actor: Seed-000000\n    role: spec\n",
        proposal_id
    );
    std::fs::write(track_dir.join("status.yaml"), status)
        .map_err(|e| format!("Failed to write track status: {}", e))?;
    std::fs::write(
        track_dir.join("spec.md"),
        format!("# Track {}\n\nSpec body.\n", tag),
    )
    .map_err(|e| format!("Failed to write spec.md: {}", e))?;

    let tracks_md = format!(
        "# Tracks\n\n## spec\n\n- [Track {}](tracks/{}/) — track {} — [anvil-playbook-engine](proposals/{}/)\n\n## spec_review\n\n## plan\n\n## implementing\n",
        tag, track_id, tag, proposal_id
    );
    std::fs::write(dir.join("tracks.md"), tracks_md)
        .map_err(|e| format!("Failed to write tracks.md: {}", e))?;

    let proj_dir = dir.join("projections");
    std::fs::create_dir_all(&proj_dir)
        .map_err(|e| format!("Failed to create projections dir: {}", e))?;
    let execution = "---\nincremental_count: 0\nbase_snapshot: 2026-04-19T00:00:00Z\nlast_updated: 2026-04-19T00:00:00Z\nafter_event: \"\"\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Planned (0)\n\n## Implementing (0)\n";
    std::fs::write(proj_dir.join("execution.md"), execution)
        .map_err(|e| format!("Failed to write execution.md: {}", e))?;
    Ok(())
}

fn seed_knowledge_request_hearth(
    dir: &std::path::Path,
    include_playbook: bool,
    ingest_body: &str,
    include_published_amend_edge: bool,
) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir.join("tracks"))
        .map_err(|e| format!("Failed to create tracks dir: {}", e))?;
    std::fs::write(dir.join("tracks.md"), "# Tracks\n")
        .map_err(|e| format!("Failed to write tracks.md: {}", e))?;
    std::fs::create_dir_all(dir.join("knowledge"))
        .map_err(|e| format!("Failed to create knowledge dir: {}", e))?;
    std::fs::write(dir.join("knowledge.md"), "# Knowledge\n")
        .map_err(|e| format!("Failed to write knowledge.md: {}", e))?;
    let proj_dir = dir.join("projections");
    std::fs::create_dir_all(&proj_dir)
        .map_err(|e| format!("Failed to create projections dir: {}", e))?;
    std::fs::write(
        proj_dir.join("execution.md"),
        "---\nincremental_count: 0\nbase_snapshot: 2026-06-11T00:00:00Z\nlast_updated: 2026-06-11T00:00:00Z\nafter_event: \"seed\"\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n",
    )
    .map_err(|e| format!("Failed to write execution.md: {}", e))?;

    if include_playbook {
        seed_knowledge_playbook(dir, ingest_body, include_published_amend_edge)?;
    }

    Ok(())
}

fn seed_knowledge_request_hearth_measured(
    dir: &std::path::Path,
    include_playbook: bool,
    ingest_body: &str,
    intent: &str,
    expected_output: &str,
) -> Result<(), String> {
    seed_knowledge_request_hearth(dir, false, "", false)?;
    if include_playbook {
        seed_knowledge_playbook_measured(dir, ingest_body, intent, expected_output)?;
    }
    Ok(())
}

fn seed_knowledge_playbook(
    hearth: &std::path::Path,
    ingest_body: &str,
    include_published_amend_edge: bool,
) -> Result<(), String> {
    let wf_dir = hearth
        .join("playbooks")
        .join("20260529T0409_knowledge_lifecycle");
    let hooks_dir = wf_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir)
        .map_err(|e| format!("Failed to create playbook hooks dir: {}", e))?;
    std::fs::write(hooks_dir.join("ingest.md"), ingest_body)
        .map_err(|e| format!("Failed to write ingest hook: {}", e))?;
    std::fs::write(
        wf_dir.join("machine.yaml"),
        knowledge_lifecycle_machine_with_ingest_hook(include_published_amend_edge),
    )
    .map_err(|e| format!("Failed to write knowledge machine.yaml: {}", e))?;
    Ok(())
}

fn seed_knowledge_playbook_measured(
    hearth: &std::path::Path,
    ingest_body: &str,
    intent: &str,
    expected_output: &str,
) -> Result<(), String> {
    let wf_dir = hearth
        .join("playbooks")
        .join("20260529T0409_knowledge_lifecycle");
    let hooks_dir = wf_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir)
        .map_err(|e| format!("Failed to create playbook hooks dir: {}", e))?;
    std::fs::write(hooks_dir.join("ingest.md"), ingest_body)
        .map_err(|e| format!("Failed to write ingest hook: {}", e))?;
    std::fs::write(
        wf_dir.join("machine.yaml"),
        knowledge_lifecycle_machine_with_ingest_hook_and_measurement(intent, expected_output),
    )
    .map_err(|e| format!("Failed to write knowledge machine.yaml: {}", e))?;
    Ok(())
}

fn seed_malformed_knowledge_playbook(hearth: &std::path::Path) -> Result<(), String> {
    let wf_dir = hearth.join("playbooks").join("knowledge_lifecycle");
    std::fs::create_dir_all(&wf_dir)
        .map_err(|e| format!("Failed to create malformed playbook dir: {}", e))?;
    std::fs::write(wf_dir.join("machine.yaml"), ": invalid yaml content [\n")
        .map_err(|e| format!("Failed to write malformed machine.yaml: {}", e))?;
    Ok(())
}

fn knowledge_lifecycle_machine_with_ingest_hook(include_published_amend_edge: bool) -> String {
    let with_hook = crate::query_port::knowledge_lifecycle_machine_yaml().replacen(
        "    hook: ~",
        "    hooks_by_role:\n      doer: ingest.md",
        1,
    );
    if !include_published_amend_edge {
        return with_hook;
    }

    // The injected `amend` state is a leaf in this fixture (only inbound edge
    // published->amend). Mark it terminal so the machine satisfies the
    // registration-time contiguity gate (a non-terminal state must have an
    // outgoing transition).
    let with_amend_state = with_hook.replace(
        "  - name: rejected\n",
        "  - name: amend\n    role_filters:\n      - doer_actionable\n    registry_section: \"Amending\"\n    projection_targets: []\n    is_review_gate: false\n    is_terminal: true\n    hook: ~\n  - name: rejected\n",
    );
    format!(
        "{}  - from_state: published\n    to_state: amend\n    required_role: doer\n    required_satisfaction: ~\n    requires_approver: false\n",
        with_amend_state
    )
}

fn knowledge_lifecycle_machine_with_ingest_hook_and_measurement(
    intent: &str,
    expected_output: &str,
) -> String {
    let replacement = format!(
        concat!(
            "    hooks_by_role:\n",
            "      doer: ingest.md\n",
            "    measurement_by_role:\n",
            "      doer:\n",
            "        intent: {intent:?}\n",
            "        expected_output: {expected_output:?}"
        ),
        intent = intent,
        expected_output = expected_output
    );
    crate::query_port::knowledge_lifecycle_machine_yaml().replacen("    hook: ~", &replacement, 1)
}

fn seed_knowledge_artifact(hearth: &std::path::Path, id: &str, state: &str) -> Result<(), String> {
    let art_dir = hearth.join("knowledge").join(id);
    std::fs::create_dir_all(&art_dir)
        .map_err(|e| format!("Failed to create artifact dir: {}", e))?;
    let status = format!(
        "version: 1\nkind: knowledge_lifecycle\nstate: {state}\nactors:\n  Seed-000000:\n    type: agent\n    configurations:\n      - at: \"2026-06-11T00:00:00Z\"\n        model: test\n        provider: test\n        details:\n          context_window: 200000\n          sdk_version: \"\"\n          entrypoint: claude-code\ntransitions:\n  - to: {state}\n    at: 2026-06-11T00:00:00Z\n    actor: Seed-000000\n    role: doer\n",
        state = state
    );
    std::fs::write(art_dir.join("status.yaml"), status)
        .map_err(|e| format!("Failed to write artifact status.yaml: {}", e))?;
    std::fs::write(
        art_dir.join("definition.md"),
        format!("# {}\n\nSeed knowledge.\n", id),
    )
    .map_err(|e| format!("Failed to write definition.md: {}", e))?;
    Ok(())
}
