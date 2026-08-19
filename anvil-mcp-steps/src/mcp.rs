use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

use anvil_core::domain::route::CANDIDATE_PLAYBOOK_INTAKE;

use anvil_test_support::engine::EngineProcess;
use anvil_test_support::{carry_retained_temp_dir, retained_temp_dir, RetainedTempDir};

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

fn column_index(table: &DataTable, name: &str) -> Result<usize, String> {
    table
        .headers
        .iter()
        .position(|h| h == name)
        .ok_or_else(|| format!("Missing '{}' column", name))
}

fn parse_bool_param(raw: &str) -> Result<bool, String> {
    match raw {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("Expected 'true' or 'false', got '{}'", raw)),
    }
}

fn anvil_engine_command(binary: &PathBuf) -> Command {
    let mut command = Command::new(binary);
    command.env("ANVIL_RENDEZVOUS_DISABLE", "1");
    // Anvil records fleet telemetry UNCONDITIONALLY (consent is Foundry's emitter,
    // not anvil). Redirect the append target off the real `~/.anvil` for tests.
    command.env(
        "ANVIL_TELEMETRY_DIR",
        std::env::temp_dir().join("anvil-brine-telemetry"),
    );
    command
}

struct McpProcess {
    child: Child,
    owned_engine: Option<CanonicalEngineProcess>,
    temp_dir_handles: Vec<RetainedTempDir>,
}

struct CanonicalEngineProcess {
    child: Child,
    temp_dir_handles: Vec<RetainedTempDir>,
}

impl CanonicalEngineProcess {
    fn new(child: Child) -> Self {
        Self {
            child,
            temp_dir_handles: Vec::new(),
        }
    }

    fn retain_temp_dir(&mut self, handle: &RetainedTempDir) {
        self.temp_dir_handles.push(Arc::clone(handle));
    }
}

impl Drop for CanonicalEngineProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl McpProcess {
    fn new(child: Child) -> Self {
        Self {
            child,
            owned_engine: None,
            temp_dir_handles: Vec::new(),
        }
    }

    fn own_engine(&mut self, engine: CanonicalEngineProcess) {
        self.owned_engine = Some(engine);
    }

    fn retain_temp_dir(&mut self, handle: &RetainedTempDir) {
        self.temp_dir_handles.push(Arc::clone(handle));
    }

    fn send(&mut self, request: &serde_json::Value) {
        let stdin = self.child.stdin.as_mut().expect("Failed to get stdin");
        let msg = serde_json::to_string(request).expect("Failed to serialize request");
        writeln!(stdin, "{}", msg).expect("Failed to write to stdin");
        stdin.flush().expect("Failed to flush stdin");
    }

    fn read_response(&mut self) -> Result<serde_json::Value, String> {
        let stdout = self.child.stdout.as_mut().expect("Failed to get stdout");
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| format!("Failed to read from stdout: {}", e))?;
        if line.is_empty() {
            return Err("No response received (stdout closed)".to_string());
        }
        serde_json::from_str(&line).map_err(|e| {
            format!(
                "Failed to parse response JSON: {} (raw: {})",
                e,
                line.trim()
            )
        })
    }

    fn close_stdin_and_wait(&mut self) -> Result<(), String> {
        let _ = self.child.stdin.take();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match self.child.try_wait() {
                Ok(Some(_status)) => return Ok(()),
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Ok(None) => return Err("MCP shim did not exit after stdin EOF".to_string()),
                Err(e) => return Err(format!("Failed to wait for MCP shim exit: {}", e)),
            }
        }
    }
}

impl Drop for McpProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Narrow seam for T-ACT-2's `mcp_claimed_evidence` step module (T-ACT-2 MCP
/// leg): `McpProcess` is private to this file, so any step defined elsewhere
/// that needs to send an additional `complete` tools/call carrying a
/// `claimed_evidence` argument shape the existing flat key|value-table step
/// (`a complete tools/call is sent with:`) cannot express must go through
/// here rather than duplicating this file's stdio plumbing. Takes ownership
/// of the incoming context (matching every other step def's signature),
/// sends `request`, waits the same fixed window the existing complete-tool
/// step uses for the shim's own response latency, reads the response, and
/// returns a context with `mcp_process` (and `hearth_path`, if present)
/// carried forward alongside the raw response for the caller to fold into
/// its own output context.
pub(crate) fn claimed_evidence_tools_call(
    mut ctx: Context,
    request: &serde_json::Value,
) -> Result<(Context, serde_json::Value), String> {
    let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
    process.send(request);
    std::thread::sleep(std::time::Duration::from_millis(3000));
    let response = process.read_response()?;
    let mut out = Context::new();
    if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
        out.set("hearth_path", hp.clone());
    }
    out.set("mcp_process", process);
    Ok((out, response))
}

fn count_dir_entries(path: &std::path::Path) -> usize {
    std::fs::read_dir(path)
        .map(|entries| entries.filter_map(Result::ok).count())
        .unwrap_or(0)
}

fn isolated_dead_port() -> Result<u16, String> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))
        .map_err(|e| format!("Failed to reserve isolated port: {}", e))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("Failed to read isolated port: {}", e))?
        .port();
    drop(listener);
    Ok(port)
}

fn configure_engine_env(
    cmd: &mut Command,
    ctx: &Context,
) -> Result<Option<CanonicalEngineProcess>, String> {
    if let Some(port) = ctx.get::<u16>("engine_port") {
        cmd.env("ANVIL_ENGINE_PORT", port.to_string());
        Ok(None)
    } else if let Some(hearth_path) = ctx.get::<PathBuf>("hearth_path") {
        let (port, mut engine) = start_owned_engine_with_hearth(hearth_path)?;
        retain_ctx_temp_dir_for_canonical(&mut engine, ctx, "hearth_path_handle");
        cmd.env("ANVIL_ENGINE_PORT", port.to_string());
        Ok(Some(engine))
    } else {
        cmd.env("ANVIL_ENGINE_PORT", isolated_dead_port()?.to_string());
        Ok(None)
    }
}

fn carry_optional_engine_context(ctx: &mut Context, out: &mut Context) {
    if let Some(port) = ctx.get::<u16>("engine_port") {
        out.set("engine_port", *port);
    }
    if let Some(engine_process) = ctx.take::<EngineProcess>("engine_process") {
        out.set("engine_process", engine_process);
    }
    if let Some(canonical_engine_process) =
        ctx.take::<CanonicalEngineProcess>("canonical_engine_process")
    {
        out.set("canonical_engine_process", canonical_engine_process);
    }
}

fn retain_ctx_temp_dir(process: &mut McpProcess, ctx: &Context, key: &str) {
    if let Some(handle) = ctx.get::<RetainedTempDir>(key) {
        process.retain_temp_dir(handle);
    }
}

fn retain_ctx_temp_dir_for_canonical(
    process: &mut CanonicalEngineProcess,
    ctx: &Context,
    key: &str,
) {
    if let Some(handle) = ctx.get::<RetainedTempDir>(key) {
        process.retain_temp_dir(handle);
    }
}

fn carry_seam_path_context(ctx: &Context, out: &mut Context) {
    for key in [
        "seam_permitted_root",
        "global_playbooks_hearth_path",
        "seam_project_alpha_path",
        "seam_project_beta_path",
    ] {
        if let Some(path) = ctx.get::<PathBuf>(key) {
            out.set(key, path.clone());
        }
    }
}

/// Fifth copy of the engine readiness poll, collapsed into the one shared
/// helper. Its own 5s iteration bound is why a mutation test against
/// anvil-test-support's budget came back INERT for this path: the fix landed on
/// four copies and this crate kept its own. A duplicated wait is a duplicated
/// timeout, and only one of them gets fixed.
fn wait_for_mcp_engine_health(port: u16) -> bool {
    anvil_test_support::engine::wait_until_grpc_ready(port)
}

fn start_canonical_seam_daemon_on(
    port: u16,
    permitted_root: &std::path::Path,
    global_playbooks_hearth: &std::path::Path,
) -> Result<CanonicalEngineProcess, String> {
    let binary = anvil_test_support::harness::binary_path("anvil-engine");
    let child = anvil_engine_command(&binary)
        .arg("--port")
        .arg(port.to_string())
        .arg("--permitted-root")
        .arg(permitted_root)
        .arg("--global-playbooks-hearth")
        .arg(global_playbooks_hearth)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            format!(
                "Failed to start anvil-engine at {}: {}",
                binary.display(),
                e
            )
        })?;

    if !wait_for_mcp_engine_health(port) {
        return Err(format!(
            "anvil-engine did not answer gRPC HealthCheck on 127.0.0.1:{} within timeout",
            port
        ));
    }

    Ok(CanonicalEngineProcess::new(child))
}

fn start_owned_engine_with_hearth(
    hearth_path: &std::path::Path,
) -> Result<(u16, CanonicalEngineProcess), String> {
    let port = isolated_dead_port()?;
    let binary = anvil_test_support::harness::binary_path("anvil-engine");
    let child = anvil_engine_command(&binary)
        .arg("--hearth")
        .arg(hearth_path)
        .arg("--port")
        .arg(port.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            format!(
                "Failed to start anvil-engine at {}: {}",
                binary.display(),
                e
            )
        })?;

    if !wait_for_mcp_engine_health(port) {
        return Err(format!(
            "anvil-engine did not answer gRPC HealthCheck on 127.0.0.1:{} within timeout",
            port
        ));
    }

    Ok((port, CanonicalEngineProcess::new(child)))
}

fn seed_seam_project_hearth(dir: &std::path::Path, tag: &str) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(dir);
    let lower = tag.to_ascii_lowercase();
    let proposal_id = format!("20260411T2021_anvil_workflow_engine_{}", lower);
    let track_id = format!("20260419T1100_track_{}", lower);
    let track_dir = dir.join("tracks").join(&track_id);
    std::fs::create_dir_all(&track_dir)
        .map_err(|e| format!("Failed to create track dir: {}", e))?;
    std::fs::write(
        track_dir.join("status.yaml"),
        format!(
            "version: 1\nkind: track\nstate: spec\nproposal: {}\nactors:\ntransitions:\n  - to: spec\n    at: 2026-06-12T00:00:00Z\n    actor: Seed-000000\n    role: spec\n",
            proposal_id
        ),
    )
    .map_err(|e| format!("Failed to write track status.yaml: {}", e))?;
    std::fs::write(
        track_dir.join("spec.md"),
        format!("# Seam Project {}\n\nSpec body.\n", tag),
    )
    .map_err(|e| format!("Failed to write spec.md: {}", e))?;

    let proposal_dir = dir.join("proposals").join(&proposal_id);
    std::fs::create_dir_all(&proposal_dir)
        .map_err(|e| format!("Failed to create proposal dir: {}", e))?;
    std::fs::write(
        proposal_dir.join("status.yaml"),
        "version: 1\nstate: active\n",
    )
    .map_err(|e| format!("Failed to write proposal status.yaml: {}", e))?;

    std::fs::write(
        dir.join("tracks.md"),
        format!(
            "# Tracks\n\n## spec\n\n- [Track {tag}](tracks/{track_id}/) - track {tag} - [proposal](proposals/{proposal_id}/)\n\n## spec_review\n\n## plan\n\n## implementing\n",
            tag = tag,
            track_id = track_id,
            proposal_id = proposal_id
        ),
    )
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
        "---\nincremental_count: 0\nbase_snapshot: 2026-06-12T00:00:00Z\nlast_updated: 2026-06-12T00:00:00Z\nafter_event: \"seed\"\n---\n\n# Anvil - State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Planned (0)\n\n## Implementing (0)\n",
    )
    .map_err(|e| format!("Failed to write execution.md: {}", e))?;
    std::fs::write(dir.join(".hearth"), format!("path: {}\n", dir.display()))
        .map_err(|e| format!("Failed to write seam .hearth: {}", e))?;
    Ok(())
}

fn seed_seam_global_hearth(
    dir: &std::path::Path,
    ingest_body: &str,
    intent: &str,
    expected_output: &str,
) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir.join("tracks"))
        .map_err(|e| format!("Failed to create global tracks dir: {}", e))?;
    std::fs::write(dir.join("tracks.md"), "# Tracks\n")
        .map_err(|e| format!("Failed to write global tracks.md: {}", e))?;
    std::fs::create_dir_all(dir.join("knowledge"))
        .map_err(|e| format!("Failed to create global knowledge dir: {}", e))?;
    std::fs::write(dir.join("knowledge.md"), "# Knowledge\n")
        .map_err(|e| format!("Failed to write global knowledge.md: {}", e))?;
    let proj_dir = dir.join("projections");
    std::fs::create_dir_all(&proj_dir)
        .map_err(|e| format!("Failed to create global projections dir: {}", e))?;
    std::fs::write(
        proj_dir.join("execution.md"),
        "---\nincremental_count: 0\nbase_snapshot: 2026-06-12T00:00:00Z\nlast_updated: 2026-06-12T00:00:00Z\nafter_event: \"seed\"\n---\n\n# Anvil - State of Execution\n\n## Spec (0)\n",
    )
    .map_err(|e| format!("Failed to write global execution.md: {}", e))?;

    let wf_dir = dir
        .join("playbooks")
        .join("20260529T0409_knowledge_lifecycle");
    let hooks_dir = wf_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir)
        .map_err(|e| format!("Failed to create global playbook hooks dir: {}", e))?;
    std::fs::write(hooks_dir.join("ingest.md"), ingest_body)
        .map_err(|e| format!("Failed to write global ingest hook: {}", e))?;
    let machine = anvil_test_support::query_port::knowledge_lifecycle_machine_yaml().replacen(
        "    hook: ~",
        &format!(
            "    hooks_by_role:\n      doer: ingest.md\n    measurement_by_role:\n      doer:\n        intent: {intent:?}\n        expected_output: {expected_output:?}",
            intent = intent,
            expected_output = expected_output
        ),
        1,
    );
    std::fs::write(wf_dir.join("machine.yaml"), machine)
        .map_err(|e| format!("Failed to write global machine.yaml: {}", e))?;
    Ok(())
}

fn create_seam_work_dir(
    hearth_path: &std::path::Path,
    label: &str,
) -> Result<(RetainedTempDir, PathBuf), String> {
    let (handle, work_dir) = retained_temp_dir(&format!("{}-", label))?;
    std::fs::create_dir_all(&work_dir)
        .map_err(|e| format!("Failed to create work dir {}: {}", work_dir.display(), e))?;
    std::fs::write(
        work_dir.join(".hearth"),
        format!("path: {}\n", hearth_path.display()),
    )
    .map_err(|e| format!("Failed to write .hearth: {}", e))?;
    Ok((handle, work_dir))
}

fn parse_mcp_tool_text(response: &serde_json::Value) -> Result<serde_json::Value, String> {
    if response["result"]["isError"].as_bool().unwrap_or(false) {
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("(missing error text)");
        return Err(format!("MCP tool returned error: {}", text));
    }
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .ok_or_else(|| format!("Missing result content text in response: {}", response))?;
    serde_json::from_str(text)
        .map_err(|e| format!("Failed to parse MCP tool JSON: {}. Raw text: {:?}", e, text))
}

fn start_seam_mcp_process(
    work_dir: &std::path::Path,
    engine_port: u16,
) -> Result<McpProcess, String> {
    let binary = anvil_test_support::harness::binary_path("anvil-mcp");
    let child = Command::new(&binary)
        .current_dir(work_dir)
        .env("ANVIL_ENGINE_PORT", engine_port.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start anvil-mcp at {}: {}", binary.display(), e))?;
    Ok(McpProcess::new(child))
}

/// A REAL `anvil-mcp` stdio process attached to a caller-owned real engine.
///
/// `McpProcess` is private to this file, so a step module that needs its own
/// long-lived shim (the K8 e2e journey drives a dozen consecutive tool calls
/// against one session) goes through this narrow seam rather than duplicating
/// the stdio plumbing. Dropping it kills the child.
pub(crate) struct OwnedMcpShim {
    inner: McpProcess,
}

impl OwnedMcpShim {
    /// Spawn the shim with `work_dir` as cwd (it reads `.hearth` from there)
    /// pointed at an ALREADY-RUNNING engine, and complete the `initialize`
    /// handshake.
    pub(crate) fn start(work_dir: &std::path::Path, engine_port: u16) -> Result<Self, String> {
        let mut inner = start_seam_mcp_process(work_dir, engine_port)?;
        initialize_seam_mcp_process(&mut inner)?;
        Ok(Self { inner })
    }

    /// Send one JSON-RPC request and read its reply verbatim. A JSON-RPC error
    /// object is returned as-is, never converted into a fake success.
    pub(crate) fn request(
        &mut self,
        request: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.inner.send(request);
        self.inner.read_response()
    }
}

fn initialize_seam_mcp_process(process: &mut McpProcess) -> Result<(), String> {
    let initialize = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 0,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "brine-test",
                "version": "0.1.0"
            }
        }
    });
    process.send(&initialize);
    let _ = process.read_response()?;
    Ok(())
}

fn send_seam_catalog_request(process: &mut McpProcess) -> Result<(), String> {
    let catalog = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "tools/call",
        "params": {
            "name": "catalog",
            "arguments": {}
        }
    });
    process.send(&catalog);
    Ok(())
}

fn send_mcp_tool_call(
    process: &mut McpProcess,
    id: i64,
    tool_name: &str,
    arguments: serde_json::Value,
) -> Result<(), String> {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {
            "name": tool_name,
            "arguments": arguments
        }
    });
    process.send(&request);
    Ok(())
}

fn read_jsonl(path: &std::path::Path) -> Result<Vec<serde_json::Value>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {}", path.display(), e))?;
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .map_err(|e| format!("parse {} line {:?}: {}", path.display(), line, e))
        })
        .collect()
}

fn seam_project_path(ctx: &Context, label: &str) -> Result<PathBuf, String> {
    match label {
        "alpha" => ctx
            .get::<PathBuf>("seam_project_alpha_path")
            .cloned()
            .ok_or("No seam_project_alpha_path".to_string()),
        "beta" => ctx
            .get::<PathBuf>("seam_project_beta_path")
            .cloned()
            .ok_or("No seam_project_beta_path".to_string()),
        other => Err(format!("Unknown seam project '{}'", other)),
    }
}

fn carry_multi_hearth_context(ctx: &mut Context, out: &mut Context) {
    carry_optional_engine_context(ctx, out);
    carry_seam_path_context(ctx, out);
    if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
        out.set("hearth_path", hp.clone());
    }
    if let Some(work_dir) = ctx.get::<PathBuf>("work_dir") {
        out.set("work_dir", work_dir.clone());
    }
    carry_retained_temp_dir(ctx, out, "hearth_path_handle");
    carry_retained_temp_dir(ctx, out, "work_dir_handle");
}

fn assert_catalog_includes_and_excludes(
    catalog: &serde_json::Value,
    included: &str,
    excluded: &str,
) -> Result<(), String> {
    let artifacts = catalog["active_artifacts"]
        .as_array()
        .ok_or_else(|| format!("Missing active_artifacts in catalog: {}", catalog))?;
    let has_included = artifacts.iter().any(|a| a["id"].as_str() == Some(included));
    let has_excluded = artifacts.iter().any(|a| a["id"].as_str() == Some(excluded));
    if !has_included {
        return Err(format!(
            "Artifact '{}' not found in catalog. Found: {:?}",
            included,
            artifacts
                .iter()
                .filter_map(|a| a["id"].as_str())
                .collect::<Vec<_>>()
        ));
    }
    if has_excluded {
        return Err(format!(
            "Artifact '{}' crossed into the wrong project catalog",
            excluded
        ));
    }
    Ok(())
}

fn direct_child_engine_pids(parent_pid: u32) -> Result<Vec<u32>, String> {
    let output = Command::new("ps")
        .args(["-axo", "pid=,ppid=,comm="])
        .output()
        .map_err(|e| format!("Failed to inspect process table with ps: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "ps failed while inspecting process table: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut pids = Vec::new();
    for line in stdout.lines() {
        let mut parts = line.split_whitespace();
        let Some(pid_raw) = parts.next() else {
            continue;
        };
        let Some(ppid_raw) = parts.next() else {
            continue;
        };
        let comm = parts.collect::<Vec<_>>().join(" ");
        let Ok(pid) = pid_raw.parse::<u32>() else {
            continue;
        };
        let Ok(ppid) = ppid_raw.parse::<u32>() else {
            continue;
        };
        if ppid == parent_pid && comm.contains("anvil-engine") {
            pids.push(pid);
        }
    }
    Ok(pids)
}

fn assert_mcp_spawned_no_engine(process: &McpProcess) -> Result<(), String> {
    let children = direct_child_engine_pids(process.child.id())?;
    if children.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "MCP shim spawned direct child anvil-engine processes: {:?}",
            children
        ))
    }
}

fn run_catalog_shim_against_engine(work_dir: &PathBuf, engine_port: u16) -> Result<(), String> {
    let binary = anvil_test_support::harness::binary_path("anvil-mcp");

    let child = Command::new(&binary)
        .current_dir(work_dir)
        .env("ANVIL_ENGINE_PORT", engine_port.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("Failed to start anvil-mcp at {}: {}", binary.display(), e));
    let mut process = McpProcess::new(child);

    let initialize = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 0,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "brine-test",
                "version": "0.1.0"
            }
        }
    });
    process.send(&initialize);
    let _ = process.read_response()?;

    let catalog = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "tools/call",
        "params": {
            "name": "catalog",
            "arguments": {}
        }
    });
    process.send(&catalog);
    std::thread::sleep(std::time::Duration::from_millis(2000));
    let response = process.read_response()?;
    let content = &response["result"]["content"];
    if content.is_null() {
        return Err(format!(
            "Missing result.content in catalog response: {}",
            serde_json::to_string_pretty(&response).unwrap_or_default()
        ));
    }
    let text = content[0]["text"].as_str().ok_or_else(|| {
        format!(
            "Missing content[0].text in catalog response: {}",
            serde_json::to_string_pretty(&response).unwrap_or_default()
        )
    })?;
    let parsed: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| format!("Failed to parse catalog JSON: {}. Raw text: '{}'", e, text))?;
    let artifacts = parsed["active_artifacts"]
        .as_array()
        .ok_or("Missing active_artifacts in catalog response")?;
    if artifacts.is_empty() {
        return Err("Catalog response contained no active artifacts".to_string());
    }

    assert_mcp_spawned_no_engine(&process)?;

    process.close_stdin_and_wait()
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "the MCP shim is started",
            &[],
            &[("mcp_process", "McpProcess")],
            |ctx, _params| {
                let binary = anvil_test_support::harness::binary_path("anvil-mcp");

                let mut cmd = Command::new(&binary);
                let owned_engine = configure_engine_env(&mut cmd, &ctx)?;
                let child = cmd
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap_or_else(|e| {
                        panic!("Failed to start anvil-mcp at {}: {}", binary.display(), e)
                    });

                let mut process = McpProcess::new(child);
                if let Some(engine) = owned_engine {
                    process.own_engine(engine);
                }
                retain_ctx_temp_dir(&mut process, &ctx, "work_dir_handle");
                retain_ctx_temp_dir(&mut process, &ctx, "hearth_path_handle");
                let mut out = Context::new();
                out.set("mcp_process", process);
                Ok(out)
            },
        ),
        step_def(
            "an initialize request is sent with protocol version {string}",
            &[("mcp_process", "McpProcess")],
            &[
                ("mcp_process", "McpProcess"),
                ("response", "JsonValue"),
                ("request_id", "i64"),
            ],
            |mut ctx, params| {
                let version = params.get_string(0).ok_or("Expected version")?.to_string();
                let mut process = ctx.take::<McpProcess>("mcp_process")
                    .ok_or("No mcp_process")?;

                let request_id: i64 = 1;
                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": version,
                        "capabilities": {},
                        "clientInfo": {
                            "name": "brine-test",
                            "version": "0.1.0"
                        }
                    }
                });

                process.send(&request);
                let response = process.read_response()?;

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("response", response);
                out.set("request_id", request_id);
                Ok(out)
            },
        ),
        check_def(
            "the response has JSON-RPC version {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected version")?;
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let actual = response["jsonrpc"]
                    .as_str()
                    .ok_or("Missing jsonrpc field")?;
                if actual != expected {
                    return Err(format!("Expected jsonrpc {}, got {}", expected, actual));
                }
                Ok(())
            },
        ),
        check_def(
            "the response has the same request id",
            &[("response", "JsonValue"), ("request_id", "i64")],
            |ctx, _params| {
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let request_id = ctx.get::<i64>("request_id").ok_or("No request_id")?;
                let response_id = response["id"]
                    .as_i64()
                    .ok_or("Missing id field in response")?;
                if response_id != *request_id {
                    return Err(format!(
                        "Expected id {}, got {}",
                        request_id, response_id
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the result contains protocol version {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected version")?;
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let actual = response["result"]["protocolVersion"]
                    .as_str()
                    .ok_or("Missing result.protocolVersion")?;
                if actual != expected {
                    return Err(format!(
                        "Expected protocolVersion {}, got {}",
                        expected, actual
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the result contains server info with name {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected name")?;
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let actual = response["result"]["serverInfo"]["name"]
                    .as_str()
                    .ok_or("Missing result.serverInfo.name")?;
                if actual != expected {
                    return Err(format!(
                        "Expected serverInfo.name {}, got {}",
                        expected, actual
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the result contains capabilities with tools enabled",
            &[("response", "JsonValue")],
            |ctx, _params| {
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let capabilities = &response["result"]["capabilities"];
                if capabilities.is_null() {
                    return Err("Missing result.capabilities".to_string());
                }
                if capabilities["tools"].is_null() {
                    return Err("Missing result.capabilities.tools".to_string());
                }
                Ok(())
            },
        ),
        // --- tools/list steps ---
        step_def(
            "the MCP session is initialized",
            &[("mcp_process", "McpProcess")],
            &[
                ("mcp_process", "McpProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("work_dir", "PathBuf"),
                ("work_dir_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
            ],
            |mut ctx, _params| {
                let mut process = ctx
                    .take::<McpProcess>("mcp_process")
                    .ok_or("No mcp_process")?;

                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 0,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {},
                        "clientInfo": {
                            "name": "brine-test",
                            "version": "0.1.0"
                        }
                    }
                });

                process.send(&request);
                let _response = process.read_response()?;

                let mut out = Context::new();
                out.set("mcp_process", process);
                // Pass through hearth_path if available (needed by E2E verification steps)
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                if let Some(work_dir) = ctx.get::<PathBuf>("work_dir") {
                    out.set("work_dir", work_dir.clone());
                }
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                carry_retained_temp_dir(&ctx, &mut out, "work_dir_handle");
                carry_optional_engine_context(&mut ctx, &mut out);
                Ok(out)
            },
        ),
        step_def(
            "the MCP session is initialized while the running engine remains active",
            &[("mcp_process", "McpProcess"), ("engine_process", "EngineProcess")],
            &[
                ("mcp_process", "McpProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("work_dir", "PathBuf"),
                ("work_dir_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
            ],
            |mut ctx, _params| {
                let mut process = ctx
                    .take::<McpProcess>("mcp_process")
                    .ok_or("No mcp_process")?;
                let engine_process = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;

                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 0,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {},
                        "clientInfo": {
                            "name": "brine-test",
                            "version": "0.1.0"
                        }
                    }
                });

                process.send(&request);
                let _response = process.read_response()?;

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("engine_process", engine_process);
                if let Some(port) = ctx.get::<u16>("engine_port") {
                    out.set("engine_port", *port);
                }
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                if let Some(work_dir) = ctx.get::<PathBuf>("work_dir") {
                    out.set("work_dir", work_dir.clone());
                }
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                carry_retained_temp_dir(&ctx, &mut out, "work_dir_handle");
                carry_seam_path_context(&ctx, &mut out);
                Ok(out)
            },
        ),
        step_def(
            "the MCP session is initialized while the canonical engine remains active",
            &[
                ("mcp_process", "McpProcess"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
            ],
            &[
                ("mcp_process", "McpProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("work_dir", "PathBuf"),
                ("work_dir_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("engine_port", "u16"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
                ("global_playbooks_hearth_path", "PathBuf"),
                ("seam_permitted_root", "PathBuf"),
            ],
            |mut ctx, _params| {
                let mut process = ctx
                    .take::<McpProcess>("mcp_process")
                    .ok_or("No mcp_process")?;
                let canonical_engine_process = ctx
                    .take::<CanonicalEngineProcess>("canonical_engine_process")
                    .ok_or("No canonical_engine_process")?;

                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 0,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {},
                        "clientInfo": {
                            "name": "brine-test",
                            "version": "0.1.0"
                        }
                    }
                });

                process.send(&request);
                let _response = process.read_response()?;

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("canonical_engine_process", canonical_engine_process);
                if let Some(port) = ctx.get::<u16>("engine_port") {
                    out.set("engine_port", *port);
                }
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                if let Some(work_dir) = ctx.get::<PathBuf>("work_dir") {
                    out.set("work_dir", work_dir.clone());
                }
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                carry_retained_temp_dir(&ctx, &mut out, "work_dir_handle");
                carry_seam_path_context(&ctx, &mut out);
                Ok(out)
            },
        ),
        step_def(
            "a tools/list request is sent",
            &[("mcp_process", "McpProcess")],
            &[("mcp_process", "McpProcess"), ("response", "JsonValue")],
            |mut ctx, _params| {
                let mut process = ctx
                    .take::<McpProcess>("mcp_process")
                    .ok_or("No mcp_process")?;

                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "tools/list",
                    "params": {}
                });

                process.send(&request);
                let response = process.read_response()?;

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("response", response);
                Ok(out)
            },
        ),
        check_def(
            "the response contains a tool named {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected tool name")?;
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let tools = response["result"]["tools"]
                    .as_array()
                    .ok_or("Missing result.tools array")?;
                let found = tools.iter().any(|t| t["name"].as_str() == Some(expected));
                if !found {
                    return Err(format!(
                        "No tool named '{}' in tools list: {:?}",
                        expected,
                        tools.iter().filter_map(|t| t["name"].as_str()).collect::<Vec<_>>()
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the response does not contain a tool named {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let unexpected = params.get_string(0).ok_or("Expected tool name")?;
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let tools = response["result"]["tools"]
                    .as_array()
                    .ok_or("Missing result.tools array")?;
                let found = tools.iter().any(|t| t["name"].as_str() == Some(unexpected));
                if found {
                    return Err(format!(
                        "Unexpected tool named '{}' in tools list: {:?}",
                        unexpected,
                        tools.iter().filter_map(|t| t["name"].as_str()).collect::<Vec<_>>()
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the anvil_orchestrate no_match handoff tool is advertised",
            &[("response", "JsonValue")],
            |ctx, _params| {
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let tools = response["result"]["tools"]
                    .as_array()
                    .ok_or("Missing result.tools array")?;
                let found = tools
                    .iter()
                    .any(|t| t["name"].as_str() == Some(CANDIDATE_PLAYBOOK_INTAKE));
                if !found {
                    return Err(format!(
                        "The canonical no_match handoff '{}' is not advertised. Tools: {:?}",
                        CANDIDATE_PLAYBOOK_INTAKE,
                        tools.iter().filter_map(|t| t["name"].as_str()).collect::<Vec<_>>()
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the {string} tool requires field {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let tool_name = params.get_string(0).ok_or("Expected tool name")?;
                let field = params.get_string(1).ok_or("Expected field")?;
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let tools = response["result"]["tools"]
                    .as_array()
                    .ok_or("Missing result.tools array")?;
                let tool = tools
                    .iter()
                    .find(|t| t["name"].as_str() == Some(tool_name))
                    .ok_or_else(|| format!("No tool named '{}'", tool_name))?;
                let required = tool["inputSchema"]["required"]
                    .as_array()
                    .ok_or("Missing inputSchema.required array")?;
                if required.iter().any(|v| v.as_str() == Some(field)) {
                    Ok(())
                } else {
                    Err(format!(
                        "Tool '{}' does not require '{}'. Required: {:?}",
                        tool_name, field, required
                    ))
                }
            },
        ),
        check_def(
            "the {string} tool has schema property {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let tool_name = params.get_string(0).ok_or("Expected tool name")?;
                let field = params.get_string(1).ok_or("Expected field")?;
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let tools = response["result"]["tools"]
                    .as_array()
                    .ok_or("Missing result.tools array")?;
                let tool = tools
                    .iter()
                    .find(|t| t["name"].as_str() == Some(tool_name))
                    .ok_or_else(|| format!("No tool named '{}'", tool_name))?;
                if tool["inputSchema"]["properties"].get(field).is_some() {
                    Ok(())
                } else {
                    Err(format!(
                        "Tool '{}' schema lacks property '{}'. Schema: {}",
                        tool_name, field, tool["inputSchema"]
                    ))
                }
            },
        ),
        check_def(
            "the {string} tool does not have schema property {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let tool_name = params.get_string(0).ok_or("Expected tool name")?;
                let field = params.get_string(1).ok_or("Expected field")?;
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let tools = response["result"]["tools"]
                    .as_array()
                    .ok_or("Missing result.tools array")?;
                let tool = tools
                    .iter()
                    .find(|t| t["name"].as_str() == Some(tool_name))
                    .ok_or_else(|| format!("No tool named '{}'", tool_name))?;
                if tool["inputSchema"]["properties"].get(field).is_none() {
                    Ok(())
                } else {
                    Err(format!(
                        "Tool '{}' unexpectedly has property '{}'. Properties: {:?}",
                        tool_name,
                        field,
                        tool["inputSchema"]["properties"]
                    ))
                }
            },
        ),
        check_def(
            "the catalog tool has a description",
            &[("response", "JsonValue")],
            |ctx, _params| {
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let tools = response["result"]["tools"]
                    .as_array()
                    .ok_or("Missing result.tools array")?;
                let catalog = tools
                    .iter()
                    .find(|t| t["name"].as_str() == Some("catalog"))
                    .ok_or("No catalog tool found")?;
                let desc = catalog["description"]
                    .as_str()
                    .ok_or("Catalog tool missing description")?;
                if desc.is_empty() {
                    return Err("Catalog tool description is empty".to_string());
                }
                Ok(())
            },
        ),
        check_def(
            "the catalog tool has an input schema",
            &[("response", "JsonValue")],
            |ctx, _params| {
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let tools = response["result"]["tools"]
                    .as_array()
                    .ok_or("Missing result.tools array")?;
                let catalog = tools
                    .iter()
                    .find(|t| t["name"].as_str() == Some("catalog"))
                    .ok_or("No catalog tool found")?;
                let schema = &catalog["inputSchema"];
                if schema.is_null() {
                    return Err("Catalog tool missing inputSchema".to_string());
                }
                if schema["type"].as_str() != Some("object") {
                    return Err(format!(
                        "Expected inputSchema.type to be 'object', got {:?}",
                        schema["type"]
                    ));
                }
                Ok(())
            },
        ),
        // --- e2e integration steps ---
        step_def(
            "a .hearth file pointing to that directory",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("work_dir", "PathBuf"),
                ("work_dir_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
                ("global_playbooks_hearth_path", "PathBuf"),
                ("seam_permitted_root", "PathBuf"),
            ],
            |mut ctx, _params| {
                let hearth_path = ctx
                    .get::<std::path::PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();

                // Create a working directory with .hearth pointing to the hearth
                let (handle, work_dir) = retained_temp_dir("anvil-test-workdir-")?;
                std::fs::create_dir_all(&work_dir)
                    .map_err(|e| format!("Failed to create work dir: {}", e))?;

                let hearth_content = format!("path: {}\n", hearth_path.display());
                std::fs::write(work_dir.join(".hearth"), hearth_content)
                    .map_err(|e| format!("Failed to write .hearth: {}", e))?;

                let mut out = Context::new();
                out.set("hearth_path", hearth_path);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                out.set("work_dir", work_dir);
                out.set("work_dir_handle", handle);
                carry_optional_engine_context(&mut ctx, &mut out);
                carry_seam_path_context(&ctx, &mut out);
                Ok(out)
            },
        ),
        step_def(
            "a .hearth file pointing to that directory while the running engine remains active",
            &[
                ("hearth_path", "PathBuf"),
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
            ],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("work_dir", "PathBuf"),
                ("work_dir_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
            ],
            |mut ctx, _params| {
                let hearth_path = ctx
                    .get::<std::path::PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let engine_process = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let engine_port = *ctx.get::<u16>("engine_port").ok_or("No engine_port")?;

                let (handle, work_dir) = retained_temp_dir("anvil-test-workdir-")?;
                std::fs::create_dir_all(&work_dir)
                    .map_err(|e| format!("Failed to create work dir: {}", e))?;

                let hearth_content = format!("path: {}\n", hearth_path.display());
                std::fs::write(work_dir.join(".hearth"), hearth_content)
                    .map_err(|e| format!("Failed to write .hearth: {}", e))?;

                let mut out = Context::new();
                out.set("hearth_path", hearth_path);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                out.set("work_dir", work_dir);
                out.set("work_dir_handle", handle);
                out.set("engine_process", engine_process);
                out.set("engine_port", engine_port);
                Ok(out)
            },
        ),
        step_def(
            "no .hearth file in the working directory",
            &[],
            &[
                ("work_dir", "PathBuf"),
                ("work_dir_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, _params| {
                let (handle, work_dir) = retained_temp_dir("anvil-test-workdir-nohearth-")?;
                std::fs::create_dir_all(&work_dir)
                    .map_err(|e| format!("Failed to create work dir: {}", e))?;
                // No .hearth file created
                let mut out = Context::new();
                out.set("work_dir", work_dir);
                out.set("work_dir_handle", handle);
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        step_def(
            "the canonical engine is started with that hearth",
            &[("hearth_path", "PathBuf")],
            &[
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("work_dir", "PathBuf"),
                ("work_dir_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("engine_port", "u16"),
            ],
            |ctx, _params| {
                let hearth_path = ctx
                    .get::<std::path::PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let port = isolated_dead_port()?;

                let binary = anvil_test_support::harness::binary_path("anvil-engine");
                let child = anvil_engine_command(&binary)
                    .arg("--hearth")
                    .arg(hearth_path.to_str().unwrap_or(""))
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

                if !wait_for_mcp_engine_health(port) {
                    return Err(format!(
                        "anvil-engine did not answer gRPC HealthCheck on 127.0.0.1:{} within timeout",
                        port
                    ));
                }

                let mut canonical_engine_process = CanonicalEngineProcess::new(child);
                retain_ctx_temp_dir_for_canonical(
                    &mut canonical_engine_process,
                    &ctx,
                    "hearth_path_handle",
                );

                let mut out = Context::new();
                out.set("canonical_engine_process", canonical_engine_process);
                out.set("hearth_path", hearth_path);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                if let Some(work_dir) = ctx.get::<std::path::PathBuf>("work_dir") {
                    out.set("work_dir", work_dir.clone());
                }
                carry_retained_temp_dir(&ctx, &mut out, "work_dir_handle");
                out.set("engine_port", port);
                Ok(out)
            },
        ),
        step_def(
            "a foreign project hearth without knowledge_lifecycle and a global playbooks hearth with measured knowledge_lifecycle intent {string} expected_output {string} body {string}",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("seam_project_alpha_path", "PathBuf"),
                ("global_playbooks_hearth_path", "PathBuf"),
                ("seam_permitted_root", "PathBuf"),
            ],
            |_ctx, params| {
                let intent = params.get_string(0).ok_or("Expected intent")?.to_string();
                let expected_output = params
                    .get_string(1)
                    .ok_or("Expected expected_output")?
                    .to_string();
                let body = params.get_string(2).ok_or("Expected body")?.to_string();
                let (handle, base) = retained_temp_dir("anvil-mcp-seam-foreign-")?;
                let permitted_root = base.join("projects");
                let foreign = permitted_root.join("foreign-project");
                let global = base.join("global-playbooks-hearth");
                seed_seam_project_hearth(&foreign, "alpha")?;
                seed_seam_global_hearth(&global, &body, &intent, &expected_output)?;

                let mut out = Context::new();
                out.set("hearth_path", foreign.clone());
                out.set("hearth_path_handle", handle);
                out.set("seam_project_alpha_path", foreign);
                out.set("global_playbooks_hearth_path", global);
                out.set("seam_permitted_root", permitted_root);
                Ok(out)
            },
        ),
        step_def(
            "two seam project hearths under one permitted root and a global playbooks hearth with knowledge_lifecycle body {string}",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
                ("global_playbooks_hearth_path", "PathBuf"),
                ("seam_permitted_root", "PathBuf"),
            ],
            |_ctx, params| {
                let body = params.get_string(0).ok_or("Expected body")?.to_string();
                let (handle, base) = retained_temp_dir("anvil-mcp-seam-multi-")?;
                let permitted_root = base.join("projects");
                let alpha = permitted_root.join("alpha-project");
                let beta = permitted_root.join("beta-project");
                let global = base.join("global-playbooks-hearth");
                seed_seam_project_hearth(&alpha, "alpha")?;
                seed_seam_project_hearth(&beta, "beta")?;
                seed_seam_global_hearth(
                    &global,
                    &body,
                    "GLOBAL SEAM INTENT",
                    "GLOBAL SEAM OUTPUT",
                )?;

                let mut out = Context::new();
                out.set("hearth_path", alpha.clone());
                out.set("hearth_path_handle", handle);
                out.set("seam_project_alpha_path", alpha);
                out.set("seam_project_beta_path", beta);
                out.set("global_playbooks_hearth_path", global);
                out.set("seam_permitted_root", permitted_root);
                Ok(out)
            },
        ),
        step_def(
            "the canonical hearth-less daemon is started with the global playbooks hearth for the seam projects",
            &[("seam_permitted_root", "PathBuf"), ("global_playbooks_hearth_path", "PathBuf")],
            &[
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("engine_port", "u16"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
                ("global_playbooks_hearth_path", "PathBuf"),
                ("seam_permitted_root", "PathBuf"),
            ],
            |ctx, _params| {
                let permitted_root = ctx
                    .get::<PathBuf>("seam_permitted_root")
                    .ok_or("No seam_permitted_root")?
                    .clone();
                let global = ctx
                    .get::<PathBuf>("global_playbooks_hearth_path")
                    .ok_or("No global_playbooks_hearth_path")?
                    .clone();
                let port = isolated_dead_port()?;
                let mut canonical_engine_process =
                    start_canonical_seam_daemon_on(port, &permitted_root, &global)?;
                retain_ctx_temp_dir_for_canonical(
                    &mut canonical_engine_process,
                    &ctx,
                    "hearth_path_handle",
                );

                let mut out = Context::new();
                out.set("canonical_engine_process", canonical_engine_process);
                out.set("engine_port", port);
                if let Some(hearth) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hearth.clone());
                }
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                carry_seam_path_context(&ctx, &mut out);
                Ok(out)
            },
        ),
        step_def(
            "a .hearth file points to the foreign project hearth while the canonical daemon remains active",
            &[
                ("hearth_path", "PathBuf"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("engine_port", "u16"),
            ],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("work_dir", "PathBuf"),
                ("work_dir_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("engine_port", "u16"),
                ("global_playbooks_hearth_path", "PathBuf"),
                ("seam_permitted_root", "PathBuf"),
            ],
            |mut ctx, _params| {
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let canonical_engine_process = ctx
                    .take::<CanonicalEngineProcess>("canonical_engine_process")
                    .ok_or("No canonical_engine_process")?;
                let engine_port = *ctx.get::<u16>("engine_port").ok_or("No engine_port")?;
                let (work_dir_handle, work_dir) =
                    create_seam_work_dir(&hearth_path, "anvil-mcp-seam-workdir")?;

                let mut out = Context::new();
                out.set("hearth_path", hearth_path);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                out.set("work_dir", work_dir);
                out.set("work_dir_handle", work_dir_handle);
                out.set("canonical_engine_process", canonical_engine_process);
                out.set("engine_port", engine_port);
                carry_seam_path_context(&ctx, &mut out);
                Ok(out)
            },
        ),
        step_def(
            "the MCP shim is started in that working directory",
            &[("work_dir", "PathBuf")],
            &[
                ("mcp_process", "McpProcess"),
                ("work_dir", "PathBuf"),
                ("hearth_path", "PathBuf"),
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
            ],
            |mut ctx, _params| {
                let work_dir = ctx
                    .get::<std::path::PathBuf>("work_dir")
                    .ok_or("No work_dir")?
                    .clone();

                let binary = anvil_test_support::harness::binary_path("anvil-mcp");

                let mut cmd = Command::new(&binary);
                cmd.current_dir(&work_dir)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                let owned_engine = configure_engine_env(&mut cmd, &ctx)?;

                let child = cmd.spawn().unwrap_or_else(|e| {
                    panic!("Failed to start anvil-mcp at {}: {}", binary.display(), e)
                });

                let mut process = McpProcess::new(child);
                if let Some(engine) = owned_engine {
                    process.own_engine(engine);
                }
                retain_ctx_temp_dir(&mut process, &ctx, "work_dir_handle");
                retain_ctx_temp_dir(&mut process, &ctx, "hearth_path_handle");
                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("work_dir", work_dir);
                // Pass through hearth_path if available (needed by E2E verification steps)
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                carry_optional_engine_context(&mut ctx, &mut out);
                Ok(out)
            },
        ),
        step_def(
            "the MCP shim is started in that working directory with Claude Code session id {string}",
            &[("work_dir", "PathBuf")],
            &[
                ("mcp_process", "McpProcess"),
                ("work_dir", "PathBuf"),
                ("hearth_path", "PathBuf"),
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
            ],
            |mut ctx, params| {
                let session_id =
                    params.get_string(0).ok_or("Expected session id")?.to_string();
                let work_dir = ctx
                    .get::<std::path::PathBuf>("work_dir")
                    .ok_or("No work_dir")?
                    .clone();

                let binary = anvil_test_support::harness::binary_path("anvil-mcp");

                let mut cmd = Command::new(&binary);
                cmd.current_dir(&work_dir)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                // The shim defaults conversation_id to CLAUDE_CODE_SESSION_ID when a
                // call omits it — set it explicitly so this is deterministic in CI
                // (which has no inherited session env).
                cmd.env("CLAUDE_CODE_SESSION_ID", &session_id);
                let owned_engine = configure_engine_env(&mut cmd, &ctx)?;

                let child = cmd.spawn().unwrap_or_else(|e| {
                    panic!("Failed to start anvil-mcp at {}: {}", binary.display(), e)
                });

                let mut process = McpProcess::new(child);
                if let Some(engine) = owned_engine {
                    process.own_engine(engine);
                }
                retain_ctx_temp_dir(&mut process, &ctx, "work_dir_handle");
                retain_ctx_temp_dir(&mut process, &ctx, "hearth_path_handle");
                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("work_dir", work_dir);
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                carry_optional_engine_context(&mut ctx, &mut out);
                Ok(out)
            },
        ),
        step_def(
            "the MCP shim is started in that working directory with a dead engine endpoint",
            &[("work_dir", "PathBuf")],
            &[("mcp_process", "McpProcess"), ("work_dir", "PathBuf"), ("hearth_path", "PathBuf")],
            |ctx, _params| {
                let work_dir = ctx
                    .get::<std::path::PathBuf>("work_dir")
                    .ok_or("No work_dir")?
                    .clone();

                let binary = anvil_test_support::harness::binary_path("anvil-mcp");

                let mut cmd = Command::new(&binary);
                cmd.current_dir(&work_dir)
                    .env("ANVIL_ENGINE_PORT", isolated_dead_port()?.to_string())
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());

                let child = cmd.spawn().unwrap_or_else(|e| {
                    panic!("Failed to start anvil-mcp at {}: {}", binary.display(), e)
                });

                let mut process = McpProcess::new(child);
                retain_ctx_temp_dir(&mut process, &ctx, "work_dir_handle");
                retain_ctx_temp_dir(&mut process, &ctx, "hearth_path_handle");
                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("work_dir", work_dir);
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                Ok(out)
            },
        ),
        step_def(
            "the MCP shim is started in that working directory with canonical daemon discovery",
            &[
                ("work_dir", "PathBuf"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
            ],
            &[
                ("mcp_process", "McpProcess"),
                ("work_dir", "PathBuf"),
                ("hearth_path", "PathBuf"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("engine_port", "u16"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
                ("global_playbooks_hearth_path", "PathBuf"),
                ("seam_permitted_root", "PathBuf"),
            ],
            |mut ctx, _params| {
                let work_dir = ctx
                    .get::<std::path::PathBuf>("work_dir")
                    .ok_or("No work_dir")?
                    .clone();
                let canonical_engine_process = ctx
                    .take::<CanonicalEngineProcess>("canonical_engine_process")
                    .ok_or("No canonical_engine_process")?;
                let engine_port = *ctx.get::<u16>("engine_port").ok_or("No engine_port")?;

                let binary = anvil_test_support::harness::binary_path("anvil-mcp");

                let mut cmd = Command::new(&binary);
                cmd.current_dir(&work_dir)
                    .env("ANVIL_ENGINE_PORT", engine_port.to_string())
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());

                let child = cmd.spawn().unwrap_or_else(|e| {
                    panic!("Failed to start anvil-mcp at {}: {}", binary.display(), e)
                });

                let mut process = McpProcess::new(child);
                retain_ctx_temp_dir(&mut process, &ctx, "work_dir_handle");
                retain_ctx_temp_dir(&mut process, &ctx, "hearth_path_handle");
                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("work_dir", work_dir);
                out.set("canonical_engine_process", canonical_engine_process);
                out.set("engine_port", engine_port);
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                carry_seam_path_context(&ctx, &mut out);
                Ok(out)
            },
        ),
        step_def(
            "the MCP shim is started in that working directory with explicit hearth path",
            &[("work_dir", "PathBuf"), ("hearth_path", "PathBuf")],
            &[("mcp_process", "McpProcess"), ("work_dir", "PathBuf"), ("hearth_path", "PathBuf")],
            |ctx, _params| {
                let work_dir = ctx
                    .get::<std::path::PathBuf>("work_dir")
                    .ok_or("No work_dir")?
                    .clone();
                let hearth_path = ctx
                    .get::<std::path::PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();

                let binary = anvil_test_support::harness::binary_path("anvil-mcp");

                let mut cmd = Command::new(&binary);
                cmd.current_dir(&work_dir)
                    .arg("--hearth")
                    .arg(&hearth_path)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                let owned_engine = configure_engine_env(&mut cmd, &ctx)?;

                let child = cmd.spawn().unwrap_or_else(|e| {
                    panic!("Failed to start anvil-mcp at {}: {}", binary.display(), e)
                });

                let mut process = McpProcess::new(child);
                if let Some(engine) = owned_engine {
                    process.own_engine(engine);
                }
                retain_ctx_temp_dir(&mut process, &ctx, "work_dir_handle");
                retain_ctx_temp_dir(&mut process, &ctx, "hearth_path_handle");
                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("work_dir", work_dir);
                out.set("hearth_path", hearth_path);
                Ok(out)
            },
        ),
        // Resilience seam (shim hardening Fix #2): start the shim against a
        // real running engine, but set ANVIL_TEST_FORCE_TRANSPORT_FAIL_ONCE so
        // the FIRST engine call fails as if the engine had just restarted. The
        // shim must reconnect and retry once, so the call still succeeds.
        step_def(
            "the MCP shim is started in that working directory with the running engine endpoint and a forced first-call transport failure",
            &[
                ("work_dir", "PathBuf"),
                ("engine_port", "u16"),
                ("engine_process", "EngineProcess"),
            ],
            &[
                ("mcp_process", "McpProcess"),
                ("work_dir", "PathBuf"),
                ("hearth_path", "PathBuf"),
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
            ],
            |mut ctx, _params| {
                let work_dir = ctx
                    .get::<std::path::PathBuf>("work_dir")
                    .ok_or("No work_dir")?
                    .clone();
                let engine_process = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;

                let binary = anvil_test_support::harness::binary_path("anvil-mcp");
                let engine_port = ctx.get::<u16>("engine_port").ok_or("No engine_port")?;
                let fail_marker = work_dir.join(".anvil-test-force-transport-fail-once");
                std::fs::write(&fail_marker, b"1")
                    .map_err(|e| format!("Failed to write fail marker: {}", e))?;

                let mut cmd = Command::new(&binary);
                cmd.current_dir(&work_dir)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                cmd.env("ANVIL_ENGINE_PORT", engine_port.to_string());
                cmd.env("ANVIL_TEST_FORCE_TRANSPORT_FAIL_ONCE", &fail_marker);

                let child = cmd.spawn().unwrap_or_else(|e| {
                    panic!("Failed to start anvil-mcp at {}: {}", binary.display(), e)
                });

                let mut process = McpProcess::new(child);
                retain_ctx_temp_dir(&mut process, &ctx, "work_dir_handle");
                retain_ctx_temp_dir(&mut process, &ctx, "hearth_path_handle");
                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("work_dir", work_dir);
                out.set("engine_process", engine_process);
                out.set("engine_port", *engine_port);
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                carry_seam_path_context(&ctx, &mut out);
                Ok(out)
            },
        ),
        // Resilience seam (shim hardening Fix #3): start the shim with
        // ANVIL_TEST_FORCE_RESULT_SERIALIZE_FAIL so that the tool-result
        // serialization path returns an error instead of (formerly) panicking.
        // The shim must return a tool error and stay alive for the next call.
        step_def(
            "the MCP shim is started in that working directory with the running engine endpoint and forced result serialization failure",
            &[
                ("work_dir", "PathBuf"),
                ("engine_port", "u16"),
                ("engine_process", "EngineProcess"),
            ],
            &[
                ("mcp_process", "McpProcess"),
                ("work_dir", "PathBuf"),
                ("hearth_path", "PathBuf"),
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
            ],
            |mut ctx, _params| {
                let work_dir = ctx
                    .get::<std::path::PathBuf>("work_dir")
                    .ok_or("No work_dir")?
                    .clone();
                let engine_process = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let engine_port = *ctx.get::<u16>("engine_port").ok_or("No engine_port")?;

                let binary = anvil_test_support::harness::binary_path("anvil-mcp");

                let mut cmd = Command::new(&binary);
                cmd.current_dir(&work_dir)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                cmd.env("ANVIL_ENGINE_PORT", engine_port.to_string());
                cmd.env("ANVIL_TEST_FORCE_RESULT_SERIALIZE_FAIL", "1");

                let child = cmd.spawn().unwrap_or_else(|e| {
                    panic!("Failed to start anvil-mcp at {}: {}", binary.display(), e)
                });

                let mut process = McpProcess::new(child);
                retain_ctx_temp_dir(&mut process, &ctx, "work_dir_handle");
                retain_ctx_temp_dir(&mut process, &ctx, "hearth_path_handle");
                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("work_dir", work_dir);
                out.set("engine_process", engine_process);
                out.set("engine_port", engine_port);
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                Ok(out)
            },
        ),
        step_def(
            "the MCP shim is started in that working directory with the running engine endpoint",
            &[
                ("work_dir", "PathBuf"),
                ("engine_port", "u16"),
                ("engine_process", "EngineProcess"),
            ],
            &[
                ("mcp_process", "McpProcess"),
                ("work_dir", "PathBuf"),
                ("hearth_path", "PathBuf"),
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
            ],
            |mut ctx, _params| {
                let work_dir = ctx
                    .get::<std::path::PathBuf>("work_dir")
                    .ok_or("No work_dir")?
                    .clone();
                let engine_process = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;

                let binary = anvil_test_support::harness::binary_path("anvil-mcp");
                let engine_port = ctx.get::<u16>("engine_port").ok_or("No engine_port")?;

                let mut cmd = Command::new(&binary);
                cmd.current_dir(&work_dir)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                cmd.env("ANVIL_ENGINE_PORT", engine_port.to_string());

                let child = cmd.spawn().unwrap_or_else(|e| {
                    panic!("Failed to start anvil-mcp at {}: {}", binary.display(), e)
                });

                let mut process = McpProcess::new(child);
                retain_ctx_temp_dir(&mut process, &ctx, "work_dir_handle");
                retain_ctx_temp_dir(&mut process, &ctx, "hearth_path_handle");
                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("work_dir", work_dir);
                out.set("engine_process", engine_process);
                out.set("engine_port", *engine_port);
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                carry_seam_path_context(&ctx, &mut out);
                Ok(out)
            },
        ),
        step_def(
            "the MCP shim is started in that working directory with the running engine endpoint and expected wire version {string}",
            &[
                ("work_dir", "PathBuf"),
                ("engine_port", "u16"),
                ("engine_process", "EngineProcess"),
            ],
            &[
                ("mcp_process", "McpProcess"),
                ("work_dir", "PathBuf"),
                ("hearth_path", "PathBuf"),
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
            ],
            |mut ctx, params| {
                let expected_version = params
                    .get_string(0)
                    .ok_or("Expected wire version")?
                    .to_string();
                let work_dir = ctx
                    .get::<std::path::PathBuf>("work_dir")
                    .ok_or("No work_dir")?
                    .clone();
                let engine_process = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;

                let binary = anvil_test_support::harness::binary_path("anvil-mcp");
                let engine_port = ctx.get::<u16>("engine_port").ok_or("No engine_port")?;

                let mut cmd = Command::new(&binary);
                cmd.current_dir(&work_dir)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                cmd.env("ANVIL_ENGINE_PORT", engine_port.to_string());
                cmd.env("ANVIL_SHIM_EXPECT_WIRE_VERSION", expected_version);

                let child = cmd.spawn().unwrap_or_else(|e| {
                    panic!("Failed to start anvil-mcp at {}: {}", binary.display(), e)
                });

                let mut process = McpProcess::new(child);
                retain_ctx_temp_dir(&mut process, &ctx, "work_dir_handle");
                retain_ctx_temp_dir(&mut process, &ctx, "hearth_path_handle");
                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("work_dir", work_dir);
                out.set("engine_process", engine_process);
                out.set("engine_port", *engine_port);
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                Ok(out)
            },
        ),
        // ===== R3: Foundry-mode shim steps (kit boundary + metadata) =====
        //
        // These steps launch the shim with FOUNDRY_SESSION_TOKEN set so the
        // shim's tri-state observe_foundry_session logic is exercised.
        // The engine is already running (from a prior "engine is started in
        // Foundry mode…" step) and its port is in context; the shim dials it.
        step_def(
            "the MCP shim is started in that working directory with the running engine endpoint and session token {string}",
            &[
                ("work_dir", "PathBuf"),
                ("engine_port", "u16"),
                ("engine_process", "EngineProcess"),
            ],
            &[
                ("mcp_process", "McpProcess"),
                ("work_dir", "PathBuf"),
                ("hearth_path", "PathBuf"),
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
            ],
            |mut ctx, params| {
                let session_token = params.get_string(0).ok_or("Expected session token")?.to_string();
                let work_dir = ctx
                    .get::<std::path::PathBuf>("work_dir")
                    .ok_or("No work_dir")?
                    .clone();
                let engine_process = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;

                let binary = anvil_test_support::harness::binary_path("anvil-mcp");
                let engine_port = ctx.get::<u16>("engine_port").ok_or("No engine_port")?;

                let mut cmd = Command::new(&binary);
                cmd.current_dir(&work_dir)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                cmd.env("ANVIL_ENGINE_PORT", engine_port.to_string());
                // Inject the Foundry session token — the shim reads this from its env
                // to decide standalone / foundry / refuse per spec Req 1 + Req 4.
                cmd.env("FOUNDRY_SESSION_TOKEN", &session_token);

                let child = cmd.spawn().unwrap_or_else(|e| {
                    panic!("Failed to start anvil-mcp at {}: {}", binary.display(), e)
                });

                let mut process = McpProcess::new(child);
                retain_ctx_temp_dir(&mut process, &ctx, "work_dir_handle");
                retain_ctx_temp_dir(&mut process, &ctx, "hearth_path_handle");
                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("work_dir", work_dir);
                out.set("engine_process", engine_process);
                out.set("engine_port", *engine_port);
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                Ok(out)
            },
        ),
        step_def(
            "the MCP shim is started without a working directory",
            &[("work_dir", "PathBuf")],
            &[("mcp_process", "McpProcess"), ("work_dir", "PathBuf")],
            |ctx, _params| {
                let work_dir = ctx
                    .get::<std::path::PathBuf>("work_dir")
                    .ok_or("No work_dir")?
                    .clone();

                let binary = anvil_test_support::harness::binary_path("anvil-mcp");

                // Start from /tmp so cwd does NOT have .hearth — forces roots-based discovery
                let mut cmd = Command::new(&binary);
                let owned_engine = configure_engine_env(&mut cmd, &ctx)?;
                let child = cmd
                    .current_dir("/tmp")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap_or_else(|e| {
                        panic!("Failed to start anvil-mcp at {}: {}", binary.display(), e)
                    });

                let mut process = McpProcess::new(child);
                if let Some(engine) = owned_engine {
                    process.own_engine(engine);
                }
                retain_ctx_temp_dir(&mut process, &ctx, "work_dir_handle");
                retain_ctx_temp_dir(&mut process, &ctx, "hearth_path_handle");
                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("work_dir", work_dir);
                Ok(out)
            },
        ),
        step_def(
            "two MCP shim invocations call catalog using the running engine endpoint",
            &[
                ("work_dir", "PathBuf"),
                ("engine_port", "u16"),
                ("engine_process", "EngineProcess"),
            ],
            &[("engine_process", "EngineProcess"), ("engine_port", "u16"), ("adopted_shim_count", "u16")],
            |mut ctx, _params| {
                let work_dir = ctx
                    .get::<PathBuf>("work_dir")
                    .ok_or("No work_dir")?
                    .clone();
                let engine_port = *ctx.get::<u16>("engine_port").ok_or("No engine_port")?;
                let engine_process = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;

                run_catalog_shim_against_engine(&work_dir, engine_port)?;
                run_catalog_shim_against_engine(&work_dir, engine_port)?;

                let mut out = Context::new();
                out.set("engine_process", engine_process);
                out.set("engine_port", engine_port);
                out.set("adopted_shim_count", 2_u16);
                Ok(out)
            },
        ),
        check_def(
            "both MCP shims spawned no fallback engine",
            &[("adopted_shim_count", "u16")],
            |ctx, _params| {
                let count = *ctx
                    .get::<u16>("adopted_shim_count")
                    .ok_or("No adopted_shim_count")?;
                if count == 2 {
                    Ok(())
                } else {
                    Err(format!("Expected 2 adopted shim invocations, got {}", count))
                }
            },
        ),
        check_def(
            "the MCP shim spawned no engine of its own",
            &[("mcp_process", "McpProcess")],
            |ctx, _params| {
                let process = ctx.get::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                assert_mcp_spawned_no_engine(process)
            },
        ),
        step_def(
            "the MCP session is initialized with roots pointing to the work directory",
            &[("mcp_process", "McpProcess"), ("work_dir", "PathBuf")],
            &[("mcp_process", "McpProcess")],
            |mut ctx, _params| {
                let mut process = ctx
                    .take::<McpProcess>("mcp_process")
                    .ok_or("No mcp_process")?;
                let work_dir = ctx
                    .get::<std::path::PathBuf>("work_dir")
                    .ok_or("No work_dir")?;

                let root_uri = format!("file://{}", work_dir.display());

                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 0,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {},
                        "clientInfo": {
                            "name": "brine-test",
                            "version": "0.1.0"
                        },
                        "roots": [{
                            "uri": root_uri
                        }]
                    }
                });

                process.send(&request);
                let _response = process.read_response()?;

                let mut out = Context::new();
                out.set("mcp_process", process);
                Ok(out)
            },
        ),
        step_def(
            "a tools/call request is sent for {string}",
            &[("mcp_process", "McpProcess")],
            &[
                ("mcp_process", "McpProcess"),
                ("response", "JsonValue"),
                ("hearth_path", "PathBuf"),
                ("work_dir", "PathBuf"),
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
            ],
            |mut ctx, params| {
                let tool_name = params.get_string(0).ok_or("Expected tool name")?.to_string();
                let mut process = ctx
                    .take::<McpProcess>("mcp_process")
                    .ok_or("No mcp_process")?;

                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 10,
                    "method": "tools/call",
                    "params": {
                        "name": tool_name,
                        "arguments": {}
                    }
                });

                process.send(&request);

                // Give the shim time to start the engine and make the gRPC call
                std::thread::sleep(std::time::Duration::from_millis(2000));

                let response = process.read_response()?;

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("response", response);
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                if let Some(work_dir) = ctx.get::<PathBuf>("work_dir") {
                    out.set("work_dir", work_dir.clone());
                }
                carry_optional_engine_context(&mut ctx, &mut out);
                Ok(out)
            },
        ),
        step_def(
            "a tools/call request is sent for {string} while the running engine remains active",
            &[("mcp_process", "McpProcess"), ("engine_process", "EngineProcess")],
            &[
                ("mcp_process", "McpProcess"),
                ("response", "JsonValue"),
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
            ],
            |mut ctx, params| {
                let tool_name = params
                    .get_string(0)
                    .ok_or("Expected tool name")?
                    .to_string();
                let mut process = ctx
                    .take::<McpProcess>("mcp_process")
                    .ok_or("No mcp_process")?;
                let engine_process = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;

                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 10,
                    "method": "tools/call",
                    "params": {
                        "name": tool_name,
                        "arguments": {}
                    }
                });

                process.send(&request);
                std::thread::sleep(std::time::Duration::from_millis(2000));
                let response = process.read_response()?;

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("response", response);
                out.set("engine_process", engine_process);
                if let Some(port) = ctx.get::<u16>("engine_port") {
                    out.set("engine_port", *port);
                }
                Ok(out)
            },
        ),
        step_def(
            "a tools/call request is sent for {string} while the canonical engine remains active",
            &[
                ("mcp_process", "McpProcess"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
            ],
            &[
                ("mcp_process", "McpProcess"),
                ("response", "JsonValue"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("engine_port", "u16"),
                ("hearth_path", "PathBuf"),
                ("work_dir", "PathBuf"),
                ("global_playbooks_hearth_path", "PathBuf"),
                ("seam_permitted_root", "PathBuf"),
            ],
            |mut ctx, params| {
                let tool_name = params
                    .get_string(0)
                    .ok_or("Expected tool name")?
                    .to_string();
                let mut process = ctx
                    .take::<McpProcess>("mcp_process")
                    .ok_or("No mcp_process")?;
                let canonical_engine_process = ctx
                    .take::<CanonicalEngineProcess>("canonical_engine_process")
                    .ok_or("No canonical_engine_process")?;

                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 10,
                    "method": "tools/call",
                    "params": {
                        "name": tool_name,
                        "arguments": {}
                    }
                });

                process.send(&request);
                std::thread::sleep(std::time::Duration::from_millis(2000));
                let response = process.read_response()?;

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("response", response);
                out.set("canonical_engine_process", canonical_engine_process);
                if let Some(port) = ctx.get::<u16>("engine_port") {
                    out.set("engine_port", *port);
                }
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                if let Some(work_dir) = ctx.get::<PathBuf>("work_dir") {
                    out.set("work_dir", work_dir.clone());
                }
                carry_seam_path_context(&ctx, &mut out);
                Ok(out)
            },
        ),
        check_def(
            "the MCP response contains active artifacts",
            &[("response", "JsonValue")],
            |ctx, _params| {
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let content = &response["result"]["content"];
                if content.is_null() {
                    return Err(format!(
                        "Missing result.content in response: {}",
                        serde_json::to_string_pretty(response).unwrap_or_default()
                    ));
                }
                // The tool result content should contain artifact data
                let text = content[0]["text"]
                    .as_str()
                    .ok_or_else(|| format!(
                        "Missing content[0].text. Full response: {}",
                        serde_json::to_string_pretty(response).unwrap_or_default()
                    ))?;
                let catalog: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse catalog JSON: {}. Raw text: '{}'", e, text))?;
                let artifacts = catalog["active_artifacts"]
                    .as_array()
                    .ok_or("Missing active_artifacts in catalog")?;
                if artifacts.is_empty() {
                    return Err("No active artifacts in catalog response".to_string());
                }
                Ok(())
            },
        ),
        check_def(
            "the MCP catalog result includes artifact {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let expected_id = params.get_string(0).ok_or("Expected id")?;
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("Missing content text")?;
                let catalog: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse catalog: {}", e))?;
                let artifacts = catalog["active_artifacts"]
                    .as_array()
                    .ok_or("Missing active_artifacts")?;
                let found = artifacts.iter().any(|a| a["id"].as_str() == Some(expected_id));
                if !found {
                    return Err(format!(
                        "Artifact '{}' not found in catalog. Found: {:?}",
                        expected_id,
                        artifacts.iter().filter_map(|a| a["id"].as_str()).collect::<Vec<_>>()
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the MCP catalog result does not include {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let excluded_id = params.get_string(0).ok_or("Expected id")?;
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("Missing content text")?;
                let catalog: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse catalog: {}", e))?;
                let artifacts = catalog["active_artifacts"]
                    .as_array()
                    .ok_or("Missing active_artifacts")?;
                if artifacts.iter().any(|a| a["id"].as_str() == Some(excluded_id)) {
                    return Err(format!(
                        "Artifact '{}' should not be in catalog but was found",
                        excluded_id
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the MCP catalog result includes available type {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let type_name = params.get_string(0).ok_or("Expected type name")?;
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("Missing content text")?;
                let catalog: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse catalog: {}", e))?;
                let types = catalog["available_types"]
                    .as_array()
                    .ok_or("Missing available_types")?;
                let found = types.iter().any(|t| t["name"].as_str() == Some(type_name));
                if !found {
                    return Err(format!("Available type '{}' not found", type_name));
                }
                Ok(())
            },
        ),
        check_def(
            "the MCP catalog result includes available playbook kind {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected playbook kind")?;
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let catalog = parse_mcp_tool_text(response)?;
                let kinds = catalog["available_artifact_kinds"]
                    .as_array()
                    .ok_or_else(|| {
                        format!(
                            "Missing available_artifact_kinds in MCP catalog result: {}",
                            catalog
                        )
                    })?;
                if kinds.iter().any(|k| k["kind"].as_str() == Some(kind)) {
                    Ok(())
                } else {
                    Err(format!(
                        "Playbook kind '{}' not found in MCP catalog available_artifact_kinds: {:?}",
                        kind,
                        kinds
                            .iter()
                            .filter_map(|k| k["kind"].as_str())
                            .collect::<Vec<_>>()
                    ))
                }
            },
        ),
        check_def(
            "the MCP catalog result includes available playbook kind {string} with described {string} and triggers {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected playbook kind")?;
                let expected_is_described = parse_bool_param(
                    params.get_string(1).ok_or("Expected described flag")?,
                )?;
                let expected_has_triggers = parse_bool_param(
                    params.get_string(2).ok_or("Expected triggers flag")?,
                )?;
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let catalog = parse_mcp_tool_text(response)?;
                let kinds = catalog["available_artifact_kinds"]
                    .as_array()
                    .ok_or_else(|| {
                        format!(
                            "Missing available_artifact_kinds in MCP catalog result: {}",
                            catalog
                        )
                    })?;
                let artifact_kind = kinds
                    .iter()
                    .find(|k| k["kind"].as_str() == Some(kind))
                    .ok_or_else(|| {
                        format!(
                            "Playbook kind '{}' not found in MCP catalog available_artifact_kinds: {:?}",
                            kind,
                            kinds
                                .iter()
                                .filter_map(|k| k["kind"].as_str())
                                .collect::<Vec<_>>()
                        )
                    })?;
                let actual_is_described =
                    artifact_kind["is_described"].as_bool().ok_or_else(|| {
                        format!(
                            "Playbook kind '{}' is missing boolean is_described: {}",
                            kind, artifact_kind
                        )
                    })?;
                let actual_has_triggers =
                    artifact_kind["has_triggers"].as_bool().ok_or_else(|| {
                        format!(
                            "Playbook kind '{}' is missing boolean has_triggers: {}",
                            kind, artifact_kind
                        )
                    })?;
                if actual_is_described != expected_is_described
                    || actual_has_triggers != expected_has_triggers
                {
                    Err(format!(
                        "Playbook kind '{}' expected is_described={} has_triggers={}, got is_described={} has_triggers={}",
                        kind,
                        expected_is_described,
                        expected_has_triggers,
                        actual_is_described,
                        actual_has_triggers
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        step_def(
            "a catalog tools/call is sent for seam project {string}",
            &[
                ("mcp_process", "McpProcess"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
            ],
            &[
                ("mcp_process", "McpProcess"),
                ("response", "JsonValue"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("engine_port", "u16"),
                ("hearth_path", "PathBuf"),
                ("work_dir", "PathBuf"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
                ("global_playbooks_hearth_path", "PathBuf"),
                ("seam_permitted_root", "PathBuf"),
            ],
            |mut ctx, params| {
                let label = params.get_string(0).ok_or("Expected project label")?;
                let project_path = seam_project_path(&ctx, label)?;
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                send_mcp_tool_call(
                    &mut process,
                    210,
                    "catalog",
                    serde_json::json!({ "project": project_path.display().to_string() }),
                )?;
                std::thread::sleep(std::time::Duration::from_millis(2000));
                let response = process.read_response()?;

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("response", response);
                carry_multi_hearth_context(&mut ctx, &mut out);
                Ok(out)
            },
        ),
        step_def(
            "a catalog tools/call is sent for seam hearth {string}",
            &[
                ("mcp_process", "McpProcess"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
            ],
            &[
                ("mcp_process", "McpProcess"),
                ("response", "JsonValue"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("engine_port", "u16"),
                ("hearth_path", "PathBuf"),
                ("work_dir", "PathBuf"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
                ("global_playbooks_hearth_path", "PathBuf"),
                ("seam_permitted_root", "PathBuf"),
            ],
            |mut ctx, params| {
                let label = params.get_string(0).ok_or("Expected hearth label")?;
                let hearth_path = seam_project_path(&ctx, label)?;
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                send_mcp_tool_call(
                    &mut process,
                    211,
                    "catalog",
                    serde_json::json!({ "hearth": hearth_path.display().to_string() }),
                )?;
                std::thread::sleep(std::time::Duration::from_millis(2000));
                let response = process.read_response()?;

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("response", response);
                carry_multi_hearth_context(&mut ctx, &mut out);
                Ok(out)
            },
        ),
        step_def(
            "a snapshot tools/call is sent for seam project {string} using its artifact path",
            &[
                ("mcp_process", "McpProcess"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
            ],
            &[
                ("mcp_process", "McpProcess"),
                ("response", "JsonValue"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("engine_port", "u16"),
                ("hearth_path", "PathBuf"),
                ("work_dir", "PathBuf"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
                ("global_playbooks_hearth_path", "PathBuf"),
                ("seam_permitted_root", "PathBuf"),
            ],
            |mut ctx, params| {
                let label = params.get_string(0).ok_or("Expected project label")?;
                let project_path = seam_project_path(&ctx, label)?;
                let artifact_id = format!("20260419T1100_track_{}", label);
                let artifact_path = project_path.join("tracks").join(&artifact_id);
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                send_mcp_tool_call(
                    &mut process,
                    212,
                    "snapshot",
                    serde_json::json!({
                        "artifact_path": artifact_path.display().to_string(),
                        "to_state": "spec_review",
                        "actor_role": "spec",
                        "actor_name": "Multi-Hearth-100000",
                        "actor_type": "agent",
                        "actor_model": "gpt-5-codex",
                        "actor_provider": "openai",
                        "note": "per-call hearth derivation"
                    }),
                )?;
                std::thread::sleep(std::time::Duration::from_millis(3000));
                let response = process.read_response()?;
                if response["result"]["isError"].as_bool().unwrap_or(false)
                    || !response["error"].is_null()
                {
                    return Err(format!(
                        "snapshot tools/call failed: {}",
                        serde_json::to_string_pretty(&response).unwrap_or_default()
                    ));
                }

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("response", response);
                carry_multi_hearth_context(&mut ctx, &mut out);
                Ok(out)
            },
        ),
        check_def(
            "seam project {string} artifact {string} is in state {string}",
            &[
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
            ],
            |ctx, params| {
                let label = params.get_string(0).ok_or("Expected project label")?;
                let artifact = params.get_string(1).ok_or("Expected artifact id")?;
                let expected = params.get_string(2).ok_or("Expected state")?;
                let project_path = seam_project_path(&ctx, label)?;
                let dir = project_path.join("tracks").join(artifact.as_ref() as &str);
                let status_path = dir.join("status.yaml");
                let content = std::fs::read_to_string(&status_path)
                    .map_err(|e| format!("Failed to read {}: {}", status_path.display(), e))?;
                // Resolve through the folding seam (per-file transition event
                // store merged with any legacy array), not a `state:` text match
                // — the event-store upcast no longer writes that line.
                let status: anvil_core::domain::status::FullStatusYaml =
                    serde_yaml::from_str(&content)
                        .map_err(|e| format!("Invalid status.yaml {}: {}", status_path.display(), e))?;
                let state =
                    anvil_core::domain::transition_log::resolve_state_with_events(&status, &dir)
                        .map_err(|e| format!("unreadable transition evidence: {e}"))?
                        .ok_or_else(|| format!("No resolvable state for {}", dir.display()))?;
                if state == expected.as_ref() as &str {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} resolved state '{}', got '{}'",
                        status_path.display(),
                        expected,
                        state
                    ))
                }
            },
        ),
        step_def(
            "a snapshot tools/call is sent for seam project {string} with an explicit hearth and a relative artifact path",
            &[
                ("mcp_process", "McpProcess"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
            ],
            &[
                ("mcp_process", "McpProcess"),
                ("response", "JsonValue"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("engine_port", "u16"),
                ("hearth_path", "PathBuf"),
                ("work_dir", "PathBuf"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
                ("global_playbooks_hearth_path", "PathBuf"),
                ("seam_permitted_root", "PathBuf"),
            ],
            |mut ctx, params| {
                let label = params.get_string(0).ok_or("Expected project label")?;
                let project_path = seam_project_path(&ctx, label)?;
                // RELATIVE artifact_path — by the tool contract it is relative
                // to the resolved hearth, NOT to the shim's cwd (which points
                // at the sibling `alpha` project here). The explicit `hearth`
                // arg must win regardless.
                let relative_artifact_path = format!("tracks/20260419T1100_track_{}", label);
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                send_mcp_tool_call(
                    &mut process,
                    220,
                    "snapshot",
                    serde_json::json!({
                        "hearth": project_path.display().to_string(),
                        "artifact_path": relative_artifact_path,
                        "to_state": "spec_review",
                        "actor_role": "spec",
                        "actor_name": "Write-Path-100000",
                        "actor_type": "agent",
                        "actor_model": "gpt-5-codex",
                        "actor_provider": "openai",
                        "note": "explicit hearth + relative artifact path"
                    }),
                )?;
                std::thread::sleep(std::time::Duration::from_millis(3000));
                let response = process.read_response()?;

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("response", response);
                carry_multi_hearth_context(&mut ctx, &mut out);
                Ok(out)
            },
        ),
        step_def(
            "a complete tools/call is sent for seam project {string} with an explicit hearth and a relative artifact path",
            &[
                ("mcp_process", "McpProcess"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
            ],
            &[
                ("mcp_process", "McpProcess"),
                ("complete_response", "JsonValue"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("engine_port", "u16"),
                ("hearth_path", "PathBuf"),
                ("work_dir", "PathBuf"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
                ("global_playbooks_hearth_path", "PathBuf"),
                ("seam_permitted_root", "PathBuf"),
            ],
            |mut ctx, params| {
                let label = params.get_string(0).ok_or("Expected project label")?;
                let project_path = seam_project_path(&ctx, label)?;
                let relative_artifact_path = format!("tracks/20260419T1100_track_{}", label);
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                send_mcp_tool_call(
                    &mut process,
                    221,
                    "complete",
                    serde_json::json!({
                        "hearth": project_path.display().to_string(),
                        "artifact_path": relative_artifact_path,
                        "actor_name": "Write-Path-200000",
                        "actor_type": "agent",
                        "actor_model": "gpt-5-codex",
                        "actor_provider": "openai",
                        "actor_context_window": 200000,
                        "actor_entrypoint": "claude-code"
                    }),
                )?;
                std::thread::sleep(std::time::Duration::from_millis(3000));
                let response = process.read_response()?;

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("complete_response", response);
                carry_multi_hearth_context(&mut ctx, &mut out);
                Ok(out)
            },
        ),
        step_def(
            "a snapshot tools/call for a relative artifact path with no hearth argument is sent",
            &[("mcp_process", "McpProcess")],
            &[
                ("mcp_process", "McpProcess"),
                ("response", "JsonValue"),
                ("work_dir", "PathBuf"),
            ],
            |mut ctx, _params| {
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                // No `hearth`/`project` arg, a relative `artifact_path`, and no
                // launch-default hearth (no .hearth in cwd). Relative derivation
                // cannot rescue this, so the ambiguous-hearth guard must survive.
                send_mcp_tool_call(
                    &mut process,
                    222,
                    "snapshot",
                    serde_json::json!({
                        "artifact_path": "tracks/20260419T1100_track_ghost",
                        "to_state": "spec_review",
                        "actor_role": "spec",
                        "actor_name": "Write-Path-300000",
                        "actor_type": "agent",
                        "actor_model": "gpt-5-codex",
                        "actor_provider": "openai"
                    }),
                )?;
                std::thread::sleep(std::time::Duration::from_millis(1000));
                let response = process.read_response()?;

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("response", response);
                if let Some(wd) = ctx.get::<PathBuf>("work_dir") {
                    out.set("work_dir", wd.clone());
                }
                Ok(out)
            },
        ),
        step_def(
            "a snapshot tools/call for a parent-escape artifact path with no hearth argument is sent",
            &[("mcp_process", "McpProcess")],
            &[
                ("mcp_process", "McpProcess"),
                ("response", "JsonValue"),
                ("work_dir", "PathBuf"),
            ],
            |mut ctx, _params| {
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                // A `..`-traversal artifact_path with NO `hearth`/`project` arg
                // and no launch-default hearth (no .hearth in cwd). Path-
                // independent SYNTAX validation runs BEFORE hearth resolution, so
                // this is refused as `invalid_artifact_path` — never
                // `ambiguous_hearth` (the pre-ordering-fix outcome).
                send_mcp_tool_call(
                    &mut process,
                    235,
                    "snapshot",
                    serde_json::json!({
                        "artifact_path": "../alpha-project/tracks/20260419T1100_track_alpha",
                        "to_state": "spec_review",
                        "actor_role": "spec",
                        "actor_name": "Write-Path-900000",
                        "actor_type": "agent",
                        "actor_model": "gpt-5-codex",
                        "actor_provider": "openai"
                    }),
                )?;
                std::thread::sleep(std::time::Duration::from_millis(1000));
                let response = process.read_response()?;

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("response", response);
                if let Some(wd) = ctx.get::<PathBuf>("work_dir") {
                    out.set("work_dir", wd.clone());
                }
                Ok(out)
            },
        ),
        step_def(
            "a nested working directory inside seam project {string}",
            &[
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
            ],
            &[
                ("work_dir", "PathBuf"),
                ("engine_process", "EngineProcess"),
                ("engine_port", "u16"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
                ("global_playbooks_hearth_path", "PathBuf"),
                ("seam_permitted_root", "PathBuf"),
            ],
            |mut ctx, params| {
                let label = params.get_string(0).ok_or("Expected project label")?;
                let project_path = seam_project_path(&ctx, label)?;
                // A directory NESTED inside the seam project, carrying NO
                // `.hearth` of its own. The shim's cwd will be here, so there is
                // no launch-default hearth; only the relative-artifact_path cwd
                // walk-up can climb to the project's `.hearth`. This is the
                // legitimate flow the unconditional early-return regressed.
                let work_dir = project_path.join("nested-cwd");
                std::fs::create_dir_all(&work_dir)
                    .map_err(|e| format!("Failed to create nested work dir: {}", e))?;

                let mut out = Context::new();
                out.set("work_dir", work_dir);
                carry_optional_engine_context(&mut ctx, &mut out);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                carry_seam_path_context(&ctx, &mut out);
                Ok(out)
            },
        ),
        step_def(
            "a snapshot tools/call is sent for seam project {string} with a relative artifact path and no hearth argument",
            &[
                ("mcp_process", "McpProcess"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
            ],
            &[
                ("mcp_process", "McpProcess"),
                ("response", "JsonValue"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("engine_port", "u16"),
                ("work_dir", "PathBuf"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
                ("global_playbooks_hearth_path", "PathBuf"),
                ("seam_permitted_root", "PathBuf"),
            ],
            |mut ctx, params| {
                let label = params.get_string(0).ok_or("Expected project label")?;
                // No explicit `hearth`/`project` arg: the shim must derive the
                // hearth by walking up from its cwd (a nested subdir of this
                // project) to the project's `.hearth`.
                let relative_artifact_path = format!("tracks/20260419T1100_track_{}", label);
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                send_mcp_tool_call(
                    &mut process,
                    230,
                    "snapshot",
                    serde_json::json!({
                        "artifact_path": relative_artifact_path,
                        "to_state": "spec_review",
                        "actor_role": "spec",
                        "actor_name": "Write-Path-400000",
                        "actor_type": "agent",
                        "actor_model": "gpt-5-codex",
                        "actor_provider": "openai",
                        "note": "nested-cwd walk-up, no explicit hearth"
                    }),
                )?;
                std::thread::sleep(std::time::Duration::from_millis(3000));
                let response = process.read_response()?;

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("response", response);
                carry_multi_hearth_context(&mut ctx, &mut out);
                Ok(out)
            },
        ),
        step_def(
            "a begin_adoption_status tools/call is sent for seam project {string} with an explicit hearth and a relative artifact path",
            &[
                ("mcp_process", "McpProcess"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
            ],
            &[
                ("mcp_process", "McpProcess"),
                ("response", "JsonValue"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("engine_port", "u16"),
                ("hearth_path", "PathBuf"),
                ("work_dir", "PathBuf"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
                ("global_playbooks_hearth_path", "PathBuf"),
                ("seam_permitted_root", "PathBuf"),
            ],
            |mut ctx, params| {
                let label = params.get_string(0).ok_or("Expected project label")?;
                let project_path = seam_project_path(&ctx, label)?;
                // begin_adoption_status is a READ tool that DOES carry a relative
                // artifact_path — it must follow the same precedence: explicit
                // hearth wins over the shim's cwd (which points at the sibling).
                let relative_artifact_path = format!("tracks/20260419T1100_track_{}", label);
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                send_mcp_tool_call(
                    &mut process,
                    231,
                    "begin_adoption_status",
                    serde_json::json!({
                        "hearth": project_path.display().to_string(),
                        "artifact_path": relative_artifact_path,
                        "actor_name": "Write-Path-500000",
                        "state": "spec"
                    }),
                )?;
                std::thread::sleep(std::time::Duration::from_millis(3000));
                let response = process.read_response()?;

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("response", response);
                carry_multi_hearth_context(&mut ctx, &mut out);
                Ok(out)
            },
        ),
        step_def(
            "an amend tools/call is sent for seam project {string} with an explicit hearth and a relative artifact path",
            &[
                ("mcp_process", "McpProcess"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
            ],
            &[
                ("mcp_process", "McpProcess"),
                ("amend_response", "JsonValue"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("engine_port", "u16"),
                ("hearth_path", "PathBuf"),
                ("work_dir", "PathBuf"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
                ("global_playbooks_hearth_path", "PathBuf"),
                ("seam_permitted_root", "PathBuf"),
            ],
            |mut ctx, params| {
                let label = params.get_string(0).ok_or("Expected project label")?;
                let project_path = seam_project_path(&ctx, label)?;
                let relative_artifact_path = format!("tracks/20260419T1100_track_{}", label);
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                send_mcp_tool_call(
                    &mut process,
                    232,
                    "amend",
                    serde_json::json!({
                        "hearth": project_path.display().to_string(),
                        "artifact_path": relative_artifact_path,
                        "kind": "spec",
                        "target_document": "spec",
                        "target_id": "AC-SEAM-1",
                        "op_kind": "add",
                        "new_kind": "acceptance_criterion",
                        "body": "Explicit-hearth amend from a sibling cwd.",
                        "actor_name": "Write-Path-600000",
                        "actor_type": "agent",
                        "actor_model": "gpt-5-codex",
                        "actor_provider": "openai"
                    }),
                )?;
                std::thread::sleep(std::time::Duration::from_millis(3000));
                let response = process.read_response()?;

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("amend_response", response);
                carry_multi_hearth_context(&mut ctx, &mut out);
                Ok(out)
            },
        ),
        step_def(
            "a snapshot tools/call is sent for seam project {string} with an explicit hearth and a parent-escape artifact path",
            &[
                ("mcp_process", "McpProcess"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
            ],
            &[
                ("mcp_process", "McpProcess"),
                ("response", "JsonValue"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("engine_port", "u16"),
                ("hearth_path", "PathBuf"),
                ("work_dir", "PathBuf"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
                ("global_playbooks_hearth_path", "PathBuf"),
                ("seam_permitted_root", "PathBuf"),
            ],
            |mut ctx, params| {
                let label = params.get_string(0).ok_or("Expected project label")?;
                let project_path = seam_project_path(&ctx, label)?;
                // A `..`-traversal artifact_path aimed at the sibling project. It
                // must be refused with `invalid_artifact_path` BEFORE any write —
                // never silently resolved, never `ambiguous_hearth`.
                let escape_artifact_path = "../alpha-project/tracks/20260419T1100_track_alpha";
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                send_mcp_tool_call(
                    &mut process,
                    233,
                    "snapshot",
                    serde_json::json!({
                        "hearth": project_path.display().to_string(),
                        "artifact_path": escape_artifact_path,
                        "to_state": "spec_review",
                        "actor_role": "spec",
                        "actor_name": "Write-Path-700000",
                        "actor_type": "agent",
                        "actor_model": "gpt-5-codex",
                        "actor_provider": "openai"
                    }),
                )?;
                std::thread::sleep(std::time::Duration::from_millis(2000));
                let response = process.read_response()?;

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("response", response);
                carry_multi_hearth_context(&mut ctx, &mut out);
                Ok(out)
            },
        ),
        step_def(
            "a snapshot tools/call is sent for seam project {string} with an explicit hearth and an absolute artifact path into seam project {string}",
            &[
                ("mcp_process", "McpProcess"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
            ],
            &[
                ("mcp_process", "McpProcess"),
                ("response", "JsonValue"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("engine_port", "u16"),
                ("hearth_path", "PathBuf"),
                ("work_dir", "PathBuf"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
                ("global_playbooks_hearth_path", "PathBuf"),
                ("seam_permitted_root", "PathBuf"),
            ],
            |mut ctx, params| {
                let hearth_label = params.get_string(0).ok_or("Expected hearth label")?;
                let target_label = params.get_string(1).ok_or("Expected target label")?;
                let hearth_project = seam_project_path(&ctx, hearth_label)?;
                let target_project = seam_project_path(&ctx, target_label)?;
                // An ABSOLUTE artifact_path pointing into the SIBLING project,
                // while the explicit hearth targets this one. Contains no `..`,
                // so it can only be caught by the resolves-outside-hearth check.
                let absolute_artifact_path = target_project
                    .join("tracks")
                    .join(format!("20260419T1100_track_{}", target_label))
                    .display()
                    .to_string();
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                send_mcp_tool_call(
                    &mut process,
                    234,
                    "snapshot",
                    serde_json::json!({
                        "hearth": hearth_project.display().to_string(),
                        "artifact_path": absolute_artifact_path,
                        "to_state": "spec_review",
                        "actor_role": "spec",
                        "actor_name": "Write-Path-800000",
                        "actor_type": "agent",
                        "actor_model": "gpt-5-codex",
                        "actor_provider": "openai"
                    }),
                )?;
                std::thread::sleep(std::time::Duration::from_millis(2000));
                let response = process.read_response()?;

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("response", response);
                carry_multi_hearth_context(&mut ctx, &mut out);
                Ok(out)
            },
        ),
        check_def(
            "the MCP response is a successful tool result containing {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?.to_string();
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                if !response["error"].is_null() {
                    return Err(format!(
                        "Expected a successful tool result, got JSON-RPC error: {}",
                        serde_json::to_string_pretty(response).unwrap_or_default()
                    ));
                }
                if response["result"]["isError"].as_bool().unwrap_or(false) {
                    return Err(format!(
                        "Expected a successful tool result, got tool error: {}",
                        serde_json::to_string_pretty(response).unwrap_or_default()
                    ));
                }
                let tool_text = response["result"]["content"][0]["text"]
                    .as_str()
                    .unwrap_or("");
                if tool_text.contains(&needle) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected successful tool text containing '{}', got '{}'",
                        needle, tool_text
                    ))
                }
            },
        ),
        step_def(
            "two MCP shims in different project hearths call catalog through the same canonical daemon",
            &[
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("engine_port", "u16"),
                ("seam_project_alpha_path", "PathBuf"),
                ("seam_project_beta_path", "PathBuf"),
            ],
            &[
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("engine_port", "u16"),
                ("seam_first_catalog", "JsonValue"),
                ("seam_second_catalog", "JsonValue"),
                ("seam_first_mcp_process", "McpProcess"),
                ("seam_second_mcp_process", "McpProcess"),
            ],
            |mut ctx, _params| {
                let canonical_engine_process = ctx
                    .take::<CanonicalEngineProcess>("canonical_engine_process")
                    .ok_or("No canonical_engine_process")?;
                let engine_port = *ctx.get::<u16>("engine_port").ok_or("No engine_port")?;
                let alpha = ctx
                    .get::<PathBuf>("seam_project_alpha_path")
                    .ok_or("No seam_project_alpha_path")?
                    .clone();
                let beta = ctx
                    .get::<PathBuf>("seam_project_beta_path")
                    .ok_or("No seam_project_beta_path")?
                    .clone();
                let (first_work_dir_handle, first_work_dir) =
                    create_seam_work_dir(&alpha, "anvil-mcp-seam-alpha-workdir")?;
                let (second_work_dir_handle, second_work_dir) =
                    create_seam_work_dir(&beta, "anvil-mcp-seam-beta-workdir")?;
                let mut first = start_seam_mcp_process(&first_work_dir, engine_port)?;
                let mut second = start_seam_mcp_process(&second_work_dir, engine_port)?;
                first.retain_temp_dir(&first_work_dir_handle);
                second.retain_temp_dir(&second_work_dir_handle);

                initialize_seam_mcp_process(&mut first)?;
                initialize_seam_mcp_process(&mut second)?;
                send_seam_catalog_request(&mut first)?;
                send_seam_catalog_request(&mut second)?;
                std::thread::sleep(std::time::Duration::from_millis(2000));
                let first_response = first.read_response()?;
                let second_response = second.read_response()?;
                let first_catalog = parse_mcp_tool_text(&first_response)?;
                let second_catalog = parse_mcp_tool_text(&second_response)?;

                let mut out = Context::new();
                out.set("canonical_engine_process", canonical_engine_process);
                out.set("engine_port", engine_port);
                out.set("seam_first_catalog", first_catalog);
                out.set("seam_second_catalog", second_catalog);
                out.set("seam_first_mcp_process", first);
                out.set("seam_second_mcp_process", second);
                carry_seam_path_context(&ctx, &mut out);
                Ok(out)
            },
        ),
        check_def(
            "the first seam project catalog includes artifact {string} and not {string}",
            &[("seam_first_catalog", "JsonValue")],
            |ctx, params| {
                let included = params.get_string(0).ok_or("Expected included artifact")?;
                let excluded = params.get_string(1).ok_or("Expected excluded artifact")?;
                assert_catalog_includes_and_excludes(
                    ctx.get::<serde_json::Value>("seam_first_catalog")
                        .ok_or("No seam_first_catalog")?,
                    included,
                    excluded,
                )
            },
        ),
        check_def(
            "the second seam project catalog includes artifact {string} and not {string}",
            &[("seam_second_catalog", "JsonValue")],
            |ctx, params| {
                let included = params.get_string(0).ok_or("Expected included artifact")?;
                let excluded = params.get_string(1).ok_or("Expected excluded artifact")?;
                assert_catalog_includes_and_excludes(
                    ctx.get::<serde_json::Value>("seam_second_catalog")
                        .ok_or("No seam_second_catalog")?,
                    included,
                    excluded,
                )
            },
        ),
        check_def(
            "both seam project shims spawned no fallback engine",
            &[
                ("seam_first_mcp_process", "McpProcess"),
                ("seam_second_mcp_process", "McpProcess"),
            ],
            |ctx, _params| {
                assert_mcp_spawned_no_engine(
                    ctx.get::<McpProcess>("seam_first_mcp_process")
                        .ok_or("No seam_first_mcp_process")?,
                )?;
                assert_mcp_spawned_no_engine(
                    ctx.get::<McpProcess>("seam_second_mcp_process")
                        .ok_or("No seam_second_mcp_process")?,
                )
            },
        ),
        step_def(
            "the canonical daemon is restarted on the same endpoint",
            &[
                ("mcp_process", "McpProcess"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("engine_port", "u16"),
                ("seam_permitted_root", "PathBuf"),
                ("global_playbooks_hearth_path", "PathBuf"),
            ],
            &[
                ("mcp_process", "McpProcess"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("engine_port", "u16"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("work_dir", "PathBuf"),
                ("work_dir_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("seam_permitted_root", "PathBuf"),
                ("global_playbooks_hearth_path", "PathBuf"),
            ],
            |mut ctx, _params| {
                let mcp_process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                let mut old = ctx
                    .take::<CanonicalEngineProcess>("canonical_engine_process")
                    .ok_or("No canonical_engine_process")?;
                let engine_port = *ctx.get::<u16>("engine_port").ok_or("No engine_port")?;
                let permitted_root = ctx
                    .get::<PathBuf>("seam_permitted_root")
                    .ok_or("No seam_permitted_root")?
                    .clone();
                let global = ctx
                    .get::<PathBuf>("global_playbooks_hearth_path")
                    .ok_or("No global_playbooks_hearth_path")?
                    .clone();

                let _ = old.child.kill();
                let _ = old.child.wait();
                std::thread::sleep(std::time::Duration::from_millis(250));
                let mut restarted =
                    start_canonical_seam_daemon_on(engine_port, &permitted_root, &global)?;
                retain_ctx_temp_dir_for_canonical(&mut restarted, &ctx, "hearth_path_handle");

                let mut out = Context::new();
                out.set("mcp_process", mcp_process);
                out.set("canonical_engine_process", restarted);
                out.set("engine_port", engine_port);
                if let Some(hearth) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hearth.clone());
                }
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                if let Some(work_dir) = ctx.get::<PathBuf>("work_dir") {
                    out.set("work_dir", work_dir.clone());
                }
                carry_retained_temp_dir(&ctx, &mut out, "work_dir_handle");
                carry_seam_path_context(&ctx, &mut out);
                Ok(out)
            },
        ),
        check_def(
            "the shim and canonical daemon have no leaked child engines",
            &[("mcp_process", "McpProcess"), ("canonical_engine_process", "CanonicalEngineProcess")],
            |ctx, _params| {
                let process = ctx.get::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                let canonical = ctx
                    .get::<CanonicalEngineProcess>("canonical_engine_process")
                    .ok_or("No canonical_engine_process")?;
                let shim_children = direct_child_engine_pids(process.child.id())?;
                if !shim_children.is_empty() {
                    return Err(format!(
                        "MCP shim leaked direct child anvil-engine processes: {:?}",
                        shim_children
                    ));
                }
                let daemon_children = direct_child_engine_pids(canonical.child.id())?;
                if !daemon_children.is_empty() {
                    return Err(format!(
                        "Canonical daemon unexpectedly has child anvil-engine processes: {:?}",
                        daemon_children
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the MCP response is a tool error",
            &[("response", "JsonValue")],
            |ctx, _params| {
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                // MCP tool errors can be:
                // 1. result.isError = true with content containing error message
                // 2. error field with code/message (JSON-RPC error)
                let is_error = response["result"]["isError"].as_bool().unwrap_or(false);
                let has_error = !response["error"].is_null();
                if !is_error && !has_error {
                    return Err(format!(
                        "Expected tool error, got: {}",
                        serde_json::to_string_pretty(response).unwrap_or_default()
                    ));
                }
                Ok(())
            },
        ),
        // ===== R3: JSON-RPC protocol error assertions =====
        //
        // A JSON-RPC protocol error has `error` at the top level (not
        // `result.isError`). Used by the kit-boundary refusal scenarios where the
        // shim surfaces an auth failure before the tool call is dispatched.
        check_def(
            "the MCP response is a JSON-RPC error",
            &[("response", "JsonValue")],
            |ctx, _params| {
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let has_error = !response["error"].is_null();
                if !has_error {
                    return Err(format!(
                        "Expected JSON-RPC error (top-level error field), got: {}",
                        serde_json::to_string_pretty(response).unwrap_or_default()
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the MCP response error message contains {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?.to_string();
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                // Check both JSON-RPC error message and tool-result content text.
                let jsonrpc_msg = response["error"]["message"]
                    .as_str()
                    .unwrap_or("");
                let tool_text = response["result"]["content"][0]["text"]
                    .as_str()
                    .unwrap_or("");
                if jsonrpc_msg.contains(&needle) || tool_text.contains(&needle) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected error message containing '{}', got JSON-RPC message='{}' tool_text='{}'",
                        needle, jsonrpc_msg, tool_text
                    ))
                }
            },
        ),
        check_def(
            "the MCP response error message names the running engine port",
            &[("response", "JsonValue"), ("engine_port", "u16")],
            |ctx, _params| {
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let port = ctx.get::<u16>("engine_port").ok_or("No engine_port")?;
                let needle = format!(":{}", port);
                let jsonrpc_msg = response["error"]["message"].as_str().unwrap_or("");
                let tool_text = response["result"]["content"][0]["text"]
                    .as_str()
                    .unwrap_or("");
                if jsonrpc_msg.contains(&needle) || tool_text.contains(&needle) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected error message to name running engine port '{}', got JSON-RPC message='{}' tool_text='{}'",
                        needle, jsonrpc_msg, tool_text
                    ))
                }
            },
        ),
        check_def(
            "the MCP response error message does not contain {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?.to_string();
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let jsonrpc_msg = response["error"]["message"].as_str().unwrap_or("");
                let tool_text = response["result"]["content"][0]["text"]
                    .as_str()
                    .unwrap_or("");
                if jsonrpc_msg.contains(&needle) || tool_text.contains(&needle) {
                    return Err(format!(
                        "Expected error message not to contain '{}', got JSON-RPC message='{}' tool_text='{}'",
                        needle, jsonrpc_msg, tool_text
                    ));
                }
                Ok(())
            },
        ),
        // ===== Tool description substring / negated-keyword checks =====
        check_def(
            "the {string} tool description contains {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let tool_name = params.get_string(0).ok_or("Missing tool name")?;
                let needle = params.get_string(1).ok_or("Missing needle")?;
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let tools = response["result"]["tools"].as_array().ok_or("Missing tools")?;
                let tool = tools.iter().find(|t| t["name"].as_str() == Some(tool_name))
                    .ok_or_else(|| format!("No {} tool found", tool_name))?;
                let desc = tool["description"].as_str().ok_or("Missing description")?;
                if desc.contains(needle) {
                    Ok(())
                } else {
                    Err(format!("'{}' tool description does not contain '{}'. description: {}", tool_name, needle, desc))
                }
            },
        ),
        check_def(
            "no tool description contains {string} (case-insensitive)",
            &[("response", "JsonValue")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Missing needle")?.to_lowercase();
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let tools = response["result"]["tools"].as_array().ok_or("Missing tools")?;
                let offenders: Vec<String> = tools.iter().filter_map(|t| {
                    let name = t["name"].as_str().unwrap_or("?");
                    let desc = t["description"].as_str().unwrap_or("");
                    if desc.to_lowercase().contains(&needle) {
                        Some(format!("{}: {}", name, desc))
                    } else {
                        None
                    }
                }).collect();
                if offenders.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "Tool descriptions contain forbidden keyword '{}' (case-insensitive):\n{}",
                        needle,
                        offenders.join("\n")
                    ))
                }
            },
        ),
        // ===== Checkin tool verification steps =====
        check_def(
            "the checkin tool has a description",
            &[("response", "JsonValue")],
            |ctx, _params| {
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let tools = response["result"]["tools"].as_array().ok_or("Missing tools")?;
                let checkin = tools.iter().find(|t| t["name"].as_str() == Some("checkin"))
                    .ok_or("No checkin tool found")?;
                let desc = checkin["description"].as_str().ok_or("Missing description")?;
                if desc.is_empty() {
                    return Err("Checkin tool description is empty".to_string());
                }
                Ok(())
            },
        ),
        check_def(
            "the checkin tool requires {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let field = params.get_string(0).ok_or("Missing field name")?.to_string();
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let tools = response["result"]["tools"].as_array().ok_or("Missing tools")?;
                let checkin = tools.iter().find(|t| t["name"].as_str() == Some("checkin"))
                    .ok_or("No checkin tool found")?;
                let required = checkin["inputSchema"]["required"].as_array()
                    .ok_or("Missing required array")?;
                let has_field = required.iter().any(|r| r.as_str() == Some(&field));
                if !has_field {
                    return Err(format!("'{}' not in required fields: {:?}", field, required));
                }
                Ok(())
            },
        ),
        // ===== Checkin E2E steps =====
        step_def(
            "a checkin tools/call is sent with:",
            &[("mcp_process", "McpProcess")],
            &[
                ("mcp_process", "McpProcess"),
                ("response", "JsonValue"),
                ("hearth_path", "PathBuf"),
                ("work_dir", "PathBuf"),
                ("engine_process", "EngineProcess"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("engine_port", "u16"),
                ("global_playbooks_hearth_path", "PathBuf"),
                ("seam_permitted_root", "PathBuf"),
            ],
            |mut ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let field_col = column_index(table, "field")?;
                let value_col = column_index(table, "value")?;
                let mut fields: std::collections::HashMap<String, String> = std::collections::HashMap::new();
                for row in &table.rows {
                    fields.insert(row[field_col].trim().to_string(), row[value_col].trim().to_string());
                }
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;

                let mut arguments = serde_json::json!({
                    "artifact_type": fields.get("artifact_type").cloned().unwrap_or_default(),
                    "parent_id": fields.get("parent_id").cloned().unwrap_or_default(),
                    "role": fields.get("role").cloned().unwrap_or_default(),
                    "track_name": fields.get("track_name").cloned().unwrap_or_default(),
                    "actor_type": fields.get("actor_type").cloned().unwrap_or_default(),
                    "actor_model": fields.get("actor_model").cloned().unwrap_or_default(),
                    "actor_provider": fields.get("actor_provider").cloned().unwrap_or_default(),
                });
                if let Some(v) = fields.get("approver") { arguments["approver"] = serde_json::json!(v); }
                if let Some(v) = fields.get("actor_context_window") { arguments["actor_context_window"] = serde_json::json!(v.parse::<i64>().unwrap_or(0)); }
                if let Some(v) = fields.get("actor_sdk_version") { arguments["actor_sdk_version"] = serde_json::json!(v); }
                if let Some(v) = fields.get("actor_entrypoint") { arguments["actor_entrypoint"] = serde_json::json!(v); }

                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 20,
                    "method": "tools/call",
                    "params": {
                        "name": "checkin",
                        "arguments": arguments
                    }
                });

                process.send(&request);
                std::thread::sleep(std::time::Duration::from_millis(3000));
                let response = process.read_response()?;

                let mut out = Context::new();
                // Preserve hearth_path if available
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                if let Some(work_dir) = ctx.get::<PathBuf>("work_dir") {
                    out.set("work_dir", work_dir.clone());
                }
                carry_optional_engine_context(&mut ctx, &mut out);
                carry_seam_path_context(&ctx, &mut out);
                out.set("mcp_process", process);
                out.set("response", response);
                Ok(out)
            },
        ),
        step_def(
            "a checkin tools/call is sent with parent {string}",
            &[("mcp_process", "McpProcess")],
            &[("mcp_process", "McpProcess"), ("response", "JsonValue")],
            |mut ctx, params| {
                let parent_id = params.get_string(0).ok_or("Missing parent")?.to_string();
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;

                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 20,
                    "method": "tools/call",
                    "params": {
                        "name": "checkin",
                        "arguments": {
                            "artifact_type": "track",
                            "parent_id": parent_id,
                            "role": "doer",
                            "track_name": "test",
                            "actor_type": "agent",
                            "actor_model": "test",
                            "actor_provider": "test"
                        }
                    }
                });

                process.send(&request);
                std::thread::sleep(std::time::Duration::from_millis(3000));
                let response = process.read_response()?;

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("response", response);
                Ok(out)
            },
        ),
        step_def(
            "a checkin tools/call is sent with artifact type {string}",
            &[("mcp_process", "McpProcess")],
            &[("mcp_process", "McpProcess"), ("response", "JsonValue")],
            |mut ctx, params| {
                let artifact_type = params.get_string(0).ok_or("Missing type")?.to_string();
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;

                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 20,
                    "method": "tools/call",
                    "params": {
                        "name": "checkin",
                        "arguments": {
                            "artifact_type": artifact_type,
                            "parent_id": "20260411T2021_anvil_workflow_engine",
                            "role": "doer",
                            "track_name": "test",
                            "actor_type": "agent",
                            "actor_model": "test",
                            "actor_provider": "test"
                        }
                    }
                });

                process.send(&request);
                std::thread::sleep(std::time::Duration::from_millis(3000));
                let response = process.read_response()?;

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("response", response);
                Ok(out)
            },
        ),
        // --- Checkin MCP response verification ---
        check_def(
            "the checkin MCP response has state {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Missing state")?.to_string();
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let text = response["result"]["content"][0]["text"].as_str()
                    .ok_or("No text in response")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse response JSON: {}", e))?;
                let state = parsed["state"].as_str().ok_or("No state in response")?;
                if state != expected {
                    return Err(format!("Expected state '{}', got '{}'", expected, state));
                }
                Ok(())
            },
        ),
        check_def(
            "the checkin MCP response has a generated actor name",
            &[("response", "JsonValue")],
            |ctx, _params| {
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let text = response["result"]["content"][0]["text"].as_str()
                    .ok_or("No text in response")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse: {}; raw={}", e, text))?;
                let name = parsed["actor_name"].as_str().ok_or("No actor_name")?;
                if !name.contains('-') || name.len() < 5 {
                    return Err(format!("Actor name '{}' doesn't look generated", name));
                }
                Ok(())
            },
        ),
        check_def(
            "the checkin MCP response has context text containing {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Missing text")?.to_string();
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let text = response["result"]["content"][0]["text"].as_str()
                    .ok_or("No text in response")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse: {}; raw={}", e, text))?;
                let context = parsed["context_text"].as_str().ok_or("No context_text")?;
                if !context.contains(&expected) {
                    return Err(format!("Context '{}' doesn't contain '{}'", context, expected));
                }
                Ok(())
            },
        ),
        check_def(
            "the checkin MCP response has a track path",
            &[("response", "JsonValue")],
            |ctx, _params| {
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let text = response["result"]["content"][0]["text"].as_str()
                    .ok_or("No text in response")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse: {}", e))?;
                let path = parsed["track_path"].as_str().ok_or("No track_path")?;
                if path.is_empty() {
                    return Err("Track path is empty".to_string());
                }
                Ok(())
            },
        ),
        check_def(
            "the hearth has a new track directory with status.yaml",
            &[("response", "JsonValue"), ("hearth_path", "PathBuf")],
            |ctx, _params| {
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let text = response["result"]["content"][0]["text"].as_str()
                    .ok_or("No text in response")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse: {}", e))?;
                let track_path = parsed["track_path"].as_str().ok_or("No track_path")?;
                let status_path = hearth.join(track_path).join("status.yaml");
                if !status_path.exists() {
                    return Err(format!("status.yaml not found at {}", status_path.display()));
                }
                Ok(())
            },
        ),
        check_def(
            "the hearth execution.md contains the new track in the {string} section",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let section = params.get_string(0).ok_or("Missing section")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let content = std::fs::read_to_string(hearth.join("projections/execution.md"))
                    .map_err(|e| format!("Failed to read execution.md: {}", e))?;
                let section_pat = format!("## {} (", section);
                let section_pos = content.find(&section_pat)
                    .ok_or_else(|| format!("Section '{}' not found in execution.md. Content:\n{}", section, content))?;
                let after = &content[section_pos..];
                let next = after[1..].find("\n## ").map(|p| p + 1).unwrap_or(after.len());
                let section_content = &after[..next];
                // Check for any track entry (we don't know the exact name from here)
                if !section_content.contains("| ") || section_content.lines().filter(|l| l.starts_with("| ") && !l.starts_with("| Track") && !l.starts_with("|---")).count() == 0 {
                    return Err(format!("No track entries found in '{}' section. Content:\n{}", section, section_content));
                }
                Ok(())
            },
        ),
        check_def(
            "the hearth tracks.md contains the new track under {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let section = params.get_string(0).ok_or("Missing section")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let content = std::fs::read_to_string(hearth.join("tracks.md"))
                    .map_err(|e| format!("Failed to read tracks.md: {}", e))?;
                let section_pos = content.find(&section)
                    .ok_or_else(|| format!("Section '{}' not found in tracks.md", section))?;
                let after = &content[section_pos + section.len()..];
                let next = after.find("\n## ").unwrap_or(after.len());
                let section_content = &after[..next];
                if !section_content.contains("my new track") && !section_content.contains("My New Track") {
                    return Err(format!("New track not found under '{}'. Section:\n{}", section, section_content));
                }
                Ok(())
            },
        ),
        // ===== Decomposed checkin flow steps =====
        step_def(
            "a checkin tools/call is sent with role {string} and:",
            &[("mcp_process", "McpProcess")],
            &[
                ("mcp_process", "McpProcess"),
                ("response", "JsonValue"),
                ("hearth_path", "PathBuf"),
                ("work_dir", "PathBuf"),
                ("engine_port", "u16"),
                ("engine_process", "EngineProcess"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("global_playbooks_hearth_path", "PathBuf"),
                ("seam_permitted_root", "PathBuf"),
            ],
            |mut ctx, params| {
                let role = params.get_string(0).ok_or("Expected role")?.to_string();
                let table = params.data_table().ok_or("Expected data table")?;
                let field_col = column_index(table, "field")?;
                let value_col = column_index(table, "value")?;
                let mut fields: std::collections::HashMap<String, String> = std::collections::HashMap::new();
                for row in &table.rows {
                    fields.insert(row[field_col].trim().to_string(), row[value_col].trim().to_string());
                }
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                let engine_process = ctx.take::<EngineProcess>("engine_process");

                let mut arguments = serde_json::json!({
                    "role": role,
                    "actor_type": fields.get("actor_type").cloned().unwrap_or_default(),
                    "actor_model": fields.get("actor_model").cloned().unwrap_or_default(),
                    "actor_provider": fields.get("actor_provider").cloned().unwrap_or_default(),
                });
                if let Some(v) = fields.get("actor_name") {
                    arguments["actor_name"] = serde_json::json!(v);
                }
                if let Some(v) = fields.get("actor_context_window") {
                    arguments["actor_context_window"] = serde_json::json!(v.parse::<i64>().unwrap_or(0));
                }
                if let Some(v) = fields.get("actor_sdk_version") {
                    arguments["actor_sdk_version"] = serde_json::json!(v);
                }
                if let Some(v) = fields.get("actor_entrypoint") {
                    arguments["actor_entrypoint"] = serde_json::json!(v);
                }

                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 30,
                    "method": "tools/call",
                    "params": {
                        "name": "checkin",
                        "arguments": arguments
                    }
                });

                process.send(&request);
                std::thread::sleep(std::time::Duration::from_millis(3000));
                let response = process.read_response()?;

                let mut out = Context::new();
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                if let Some(work_dir) = ctx.get::<PathBuf>("work_dir") {
                    out.set("work_dir", work_dir.clone());
                }
                if let Some(port) = ctx.get::<u16>("engine_port") {
                    out.set("engine_port", *port);
                }
                if let Some(engine_process) = engine_process {
                    out.set("engine_process", engine_process);
                }
                out.set("mcp_process", process);
                out.set("response", response);
                Ok(out)
            },
        ),
        step_def(
            "a describe tools/call is sent with type {string}",
            &[("mcp_process", "McpProcess")],
            &[("mcp_process", "McpProcess"), ("response", "JsonValue"), ("hearth_path", "PathBuf")],
            |mut ctx, params| {
                let type_name = params.get_string(0).ok_or("Expected type")?.to_string();
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;

                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 31,
                    "method": "tools/call",
                    "params": {
                        "name": "describe",
                        "arguments": {
                            "identifier": type_name
                        }
                    }
                });

                process.send(&request);
                std::thread::sleep(std::time::Duration::from_millis(3000));
                let response = process.read_response()?;

                let mut out = Context::new();
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                out.set("mcp_process", process);
                out.set("response", response);
                Ok(out)
            },
        ),
        step_def(
            "a begin tools/call is sent with:",
            &[("mcp_process", "McpProcess")],
            &[
                ("mcp_process", "McpProcess"),
                ("response", "JsonValue"),
                ("hearth_path", "PathBuf"),
                ("work_dir", "PathBuf"),
                ("engine_port", "u16"),
                ("engine_process", "EngineProcess"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("global_playbooks_hearth_path", "PathBuf"),
                ("seam_permitted_root", "PathBuf"),
            ],
            |mut ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let field_col = column_index(table, "field")?;
                let value_col = column_index(table, "value")?;
                let mut fields: std::collections::HashMap<String, String> = std::collections::HashMap::new();
                for row in &table.rows {
                    fields.insert(row[field_col].trim().to_string(), row[value_col].trim().to_string());
                }
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;

                // Per spec R2 of the checkin_backfill_spec_context track,
                // the shim forwards actor_* verbatim. Defaults below are
                // step-local (visible in this one step body, not hidden
                // in a shared helper) so each scenario sees the same
                // fixture identity unless it overrides a field via the
                // data table. Scenarios that test identity validation
                // explicitly set the relevant field to the empty string.
                let arguments = serde_json::json!({
                    "artifact_type": fields.get("artifact_type").cloned().unwrap_or_default(),
                    "parent_id": fields.get("parent_id").cloned().unwrap_or_default(),
                    "track_name": fields.get("track_name").cloned().unwrap_or_default(),
                    // Forward playbook_name + target_owner from the data table
                    // (HIGH-1: previously dropped — the shim could never receive
                    // them through this step). Default to empty when absent.
                    "playbook_name": fields.get("playbook_name").cloned().unwrap_or_default(),
                    "target_owner": fields.get("target_owner").cloned().unwrap_or_default(),
                    "approver": fields.get("approver").cloned().unwrap_or_default(),
                    "identifier": fields.get("identifier").cloned().unwrap_or_default(),
                    "actor_name": fields.get("actor_name").cloned().unwrap_or_else(|| "TestActor-000100".to_string()),
                    "actor_type": fields.get("actor_type").cloned().unwrap_or_else(|| "agent".to_string()),
                    "actor_model": fields.get("actor_model").cloned().unwrap_or_else(|| "test-model".to_string()),
                    "actor_provider": fields.get("actor_provider").cloned().unwrap_or_else(|| "test".to_string()),
                    "conversation_id": fields.get("conversation_id").cloned().unwrap_or_default(),
                    "surface": fields.get("surface").cloned().unwrap_or_default(),
                });

                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 32,
                    "method": "tools/call",
                    "params": {
                        "name": "begin",
                        "arguments": arguments
                    }
                });

                process.send(&request);
                std::thread::sleep(std::time::Duration::from_millis(3000));
                let response = process.read_response()?;

                let mut out = Context::new();
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                if let Some(work_dir) = ctx.get::<PathBuf>("work_dir") {
                    out.set("work_dir", work_dir.clone());
                }
                if let Some(port) = ctx.get::<u16>("engine_port") {
                    out.set("engine_port", *port);
                }
                carry_optional_engine_context(&mut ctx, &mut out);
                carry_seam_path_context(&ctx, &mut out);
                out.set("mcp_process", process);
                out.set("response", response);
                Ok(out)
            },
        ),
        step_def(
            "an anvil_orchestrate tools/call is sent with:",
            &[("mcp_process", "McpProcess")],
            &[
                ("mcp_process", "McpProcess"),
                ("response", "JsonValue"),
                ("hearth_path", "PathBuf"),
                ("work_dir", "PathBuf"),
                ("engine_port", "u16"),
                ("engine_process", "EngineProcess"),
                ("canonical_engine_process", "CanonicalEngineProcess"),
                ("global_playbooks_hearth_path", "PathBuf"),
                ("seam_permitted_root", "PathBuf"),
            ],
            |mut ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let field_col = column_index(table, "field")?;
                let value_col = column_index(table, "value")?;
                let mut fields: std::collections::HashMap<String, String> =
                    std::collections::HashMap::new();
                for row in &table.rows {
                    fields.insert(row[field_col].trim().to_string(), row[value_col].trim().to_string());
                }
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                // anvil_orchestrate begin-mode: `selection` IS the kind. Mirror the begin
                // step's arg forwarding (incl. playbook_name + target_owner) — the defect
                // under test is begin_selected dropping playbook_name for driven kinds.
                let arguments = serde_json::json!({
                    "selection": fields.get("selection").cloned().unwrap_or_default(),
                    "parent_id": fields.get("parent_id").cloned().unwrap_or_default(),
                    "track_name": fields.get("track_name").cloned().unwrap_or_default(),
                    "playbook_name": fields.get("playbook_name").cloned().unwrap_or_default(),
                    "target_owner": fields.get("target_owner").cloned().unwrap_or_default(),
                    "approver": fields.get("approver").cloned().unwrap_or_default(),
                    "actor_name": fields.get("actor_name").cloned().unwrap_or_else(|| "TestActor-000100".to_string()),
                    "actor_type": fields.get("actor_type").cloned().unwrap_or_else(|| "agent".to_string()),
                    "actor_model": fields.get("actor_model").cloned().unwrap_or_else(|| "test-model".to_string()),
                    "actor_provider": fields.get("actor_provider").cloned().unwrap_or_else(|| "test".to_string()),
                    "conversation_id": fields.get("conversation_id").cloned().unwrap_or_default(),
                    "surface": fields.get("surface").cloned().unwrap_or_else(|| "kiln".to_string()),
                });
                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 33,
                    "method": "tools/call",
                    "params": { "name": "anvil_orchestrate", "arguments": arguments }
                });
                process.send(&request);
                std::thread::sleep(std::time::Duration::from_millis(3000));
                let response = process.read_response()?;
                let mut out = Context::new();
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                if let Some(work_dir) = ctx.get::<PathBuf>("work_dir") {
                    out.set("work_dir", work_dir.clone());
                }
                if let Some(port) = ctx.get::<u16>("engine_port") {
                    out.set("engine_port", *port);
                }
                carry_optional_engine_context(&mut ctx, &mut out);
                carry_seam_path_context(&ctx, &mut out);
                out.set("mcp_process", process);
                out.set("response", response);
                Ok(out)
            },
        ),
        // ===== Checkin response verification (decomposed) =====
        check_def(
            "the checkin response actor_name is {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected name")?;
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let text = response["result"]["content"][0]["text"].as_str()
                    .ok_or("No text in response")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse: {}; raw={}", e, text))?;
                let name = parsed["actor_name"].as_str().ok_or("No actor_name")?;
                if name == expected {
                    Ok(())
                } else {
                    Err(format!("Expected actor_name '{}', got '{}'", expected, name))
                }
            },
        ),
        check_def(
            "the checkin response context contains {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected needle")?;
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let text = response["result"]["content"][0]["text"].as_str()
                    .ok_or("No text in response")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse: {}; raw={}", e, text))?;
                let context = parsed["context"].as_str().unwrap_or("");
                if context.contains(expected) {
                    Ok(())
                } else {
                    Err(format!("Expected context to contain '{}', got '{}'", expected, context))
                }
            },
        ),
        check_def(
            "the hearth's new track status.yaml contains {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let tracks_dir = hearth.join("tracks");
                let entries = std::fs::read_dir(&tracks_dir)
                    .map_err(|e| format!("read_dir failed on {}: {}", tracks_dir.display(), e))?;
                // Find the most recently modified track directory with a status.yaml.
                let mut latest: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
                for entry in entries {
                    let entry = entry.map_err(|e| format!("entry: {}", e))?;
                    let status_path = entry.path().join("status.yaml");
                    if !status_path.exists() {
                        continue;
                    }
                    let meta = std::fs::metadata(&status_path)
                        .map_err(|e| format!("metadata: {}", e))?;
                    let mtime = meta.modified().map_err(|e| format!("mtime: {}", e))?;
                    match &latest {
                        None => latest = Some((mtime, status_path)),
                        Some((prev_mtime, _)) if mtime > *prev_mtime => {
                            latest = Some((mtime, status_path));
                        }
                        _ => {}
                    }
                }
                let path = latest.ok_or_else(|| format!("No track with status.yaml under {}", tracks_dir.display()))?.1;
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("read {}: {}", path.display(), e))?;
                if content.contains(needle) {
                    Ok(())
                } else {
                    Err(format!(
                        "status.yaml at {} does not contain '{}'. Content:\n{}",
                        path.display(), needle, content
                    ))
                }
            },
        ),
        // Like the above, but asserts SOME per-file transition event under the
        // newest track's `transitions/` dir contains the needle — the transition
        // log moved out of status.yaml into the event store (upcast).
        check_def(
            "the hearth's new track transition event contains {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let tracks_dir = hearth.join("tracks");
                let entries = std::fs::read_dir(&tracks_dir)
                    .map_err(|e| format!("read_dir failed on {}: {}", tracks_dir.display(), e))?;
                let mut latest: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
                for entry in entries {
                    let entry = entry.map_err(|e| format!("entry: {}", e))?;
                    let status_path = entry.path().join("status.yaml");
                    if !status_path.exists() {
                        continue;
                    }
                    let meta = std::fs::metadata(&status_path)
                        .map_err(|e| format!("metadata: {}", e))?;
                    let mtime = meta.modified().map_err(|e| format!("mtime: {}", e))?;
                    match &latest {
                        None => latest = Some((mtime, entry.path())),
                        Some((prev_mtime, _)) if mtime > *prev_mtime => {
                            latest = Some((mtime, entry.path()));
                        }
                        _ => {}
                    }
                }
                let track_dir = latest
                    .ok_or_else(|| format!("No track with status.yaml under {}", tracks_dir.display()))?
                    .1;
                let events_dir = track_dir.join("transitions");
                let ev = std::fs::read_dir(&events_dir).map_err(|e| {
                    format!("No transitions dir under {}: {}", track_dir.display(), e)
                })?;
                for entry in ev.flatten() {
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        if content.contains(needle.as_ref() as &str) {
                            return Ok(());
                        }
                    }
                }
                Err(format!(
                    "No transition event under {} contains '{}'",
                    events_dir.display(),
                    needle
                ))
            },
        ),
        // ===== target_owner shim e2e (Anvil-lane 1b, A4) =====
        // Seed the builder machine + an active parent track into the shim's e2e
        // hearth (reusing the engine fixture's seeding), exposing hearth_path so
        // the standard `.hearth file pointing` + shim-start chain runs over it.
        step_def(
            "a hearth seeded with the builder machine and an active parent track {string}",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, params| {
                let parent = params.get_string(0).ok_or("Expected parent id")?.to_string();
                let (handle, tmp) = retained_temp_dir("anvil-test-builder-shim-")?;
                anvil_test_support::builder::seed_builder_hearth(&tmp, Some(&parent))?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        // Generic status.yaml-under-directory contains check, scanning the
        // most-recently-modified instance under <hearth>/<dir>/. Parallels the
        // tracks/-scoped check above but takes the artifact directory.
        check_def(
            "the hearth status.yaml under {string} contains {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let dir = params.get_string(0).ok_or("Expected directory")?;
                let needle = params.get_string(1).ok_or("Expected needle")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let scan_dir = hearth.join(dir);
                let entries = std::fs::read_dir(&scan_dir)
                    .map_err(|e| format!("read_dir failed on {}: {}", scan_dir.display(), e))?;
                let mut latest: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
                for entry in entries {
                    let entry = entry.map_err(|e| format!("entry: {}", e))?;
                    let status_path = entry.path().join("status.yaml");
                    if !status_path.exists() {
                        continue;
                    }
                    let meta = std::fs::metadata(&status_path)
                        .map_err(|e| format!("metadata: {}", e))?;
                    let mtime = meta.modified().map_err(|e| format!("mtime: {}", e))?;
                    match &latest {
                        None => latest = Some((mtime, status_path)),
                        Some((prev_mtime, _)) if mtime > *prev_mtime => {
                            latest = Some((mtime, status_path));
                        }
                        _ => {}
                    }
                }
                let path = latest
                    .ok_or_else(|| {
                        let response = ctx
                            .get::<serde_json::Value>("response")
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "(no response in context)".to_string());
                        format!(
                            "No instance with status.yaml under {}. Last MCP response: {}",
                            scan_dir.display(),
                            response
                        )
                    })?
                    .1;
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("read {}: {}", path.display(), e))?;
                if content.contains(needle) {
                    Ok(())
                } else {
                    Err(format!(
                        "status.yaml at {} does not contain '{}'. Content:\n{}",
                        path.display(), needle, content
                    ))
                }
            },
        ),
        check_def(
            "the checkin response has a generated actor name",
            &[("response", "JsonValue")],
            |ctx, _params| {
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let text = response["result"]["content"][0]["text"].as_str()
                    .ok_or("No text in response")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse: {}", e))?;
                let name = parsed["actor_name"].as_str().ok_or("No actor_name")?;
                if !name.contains('-') || name.len() < 5 {
                    return Err(format!("Actor name '{}' doesn't look generated", name));
                }
                Ok(())
            },
        ),
        check_def(
            "the checkin response includes available type {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let type_name = params.get_string(0).ok_or("Expected type name")?;
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let text = response["result"]["content"][0]["text"].as_str()
                    .ok_or("Missing content text")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse: {}", e))?;
                let types = parsed["available_types"].as_array()
                    .ok_or("Missing available_types")?;
                let found = types.iter().any(|t| t["name"].as_str() == Some(type_name));
                if !found {
                    return Err(format!("Available type '{}' not found in checkin response", type_name));
                }
                Ok(())
            },
        ),
        check_def(
            "the checkin response includes artifact {string} with state {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let artifact_id = params.get_string(0).ok_or("Expected id")?;
                let expected_state = params.get_string(1).ok_or("Expected state")?;
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let text = response["result"]["content"][0]["text"].as_str()
                    .ok_or("Missing content text")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse: {}", e))?;
                let artifacts = parsed["filtered_artifacts"].as_array()
                    .ok_or("Missing filtered_artifacts")?;
                let found = artifacts.iter().find(|a| a["id"].as_str() == Some(artifact_id));
                match found {
                    None => Err(format!("Artifact '{}' not found in filtered_artifacts", artifact_id)),
                    Some(a) => {
                        let state = a["state"].as_str().ok_or("Missing state")?;
                        if state != expected_state {
                            return Err(format!("Artifact '{}' state: expected '{}', got '{}'", artifact_id, expected_state, state));
                        }
                        Ok(())
                    }
                }
            },
        ),
        // ===== Describe response verification =====
        check_def(
            "the describe response has type name {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected name")?.to_string();
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let text = response["result"]["content"][0]["text"].as_str()
                    .ok_or("No text in response")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse: {}", e))?;
                let name = parsed["name"].as_str().ok_or("No name field")?;
                if name != expected {
                    return Err(format!("Expected type name '{}', got '{}'", expected, name));
                }
                Ok(())
            },
        ),
        check_def(
            "the describe response has required field {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let field = params.get_string(0).ok_or("Expected field")?.to_string();
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let text = response["result"]["content"][0]["text"].as_str()
                    .ok_or("No text in response")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse: {}", e))?;
                let fields = parsed["required_fields"].as_array()
                    .ok_or("No required_fields")?;
                let found = fields.iter().any(|f| f.as_str() == Some(&field));
                if !found {
                    return Err(format!("Required field '{}' not found", field));
                }
                Ok(())
            },
        ),
        check_def(
            "the describe response has parent type {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected parent type")?.to_string();
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let text = response["result"]["content"][0]["text"].as_str()
                    .ok_or("No text in response")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse: {}", e))?;
                let parent = parsed["parent_type"].as_str().ok_or("No parent_type")?;
                if parent != expected {
                    return Err(format!("Expected parent type '{}', got '{}'", expected, parent));
                }
                Ok(())
            },
        ),
        // ===== Begin response verification =====
        check_def(
            "the begin response has state {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?.to_string();
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let parsed = parse_mcp_tool_text(response)?;
                let state = parsed["state"].as_str().ok_or("No state")?;
                if state != expected {
                    return Err(format!("Expected state '{}', got '{}'", expected, state));
                }
                Ok(())
            },
        ),
        check_def(
            "the begin response has a track path",
            &[("response", "JsonValue")],
            |ctx, _params| {
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let parsed = parse_mcp_tool_text(response)?;
                let path = parsed["track_path"].as_str().ok_or("No track_path")?;
                if path.is_empty() {
                    return Err("Track path is empty".to_string());
                }
                Ok(())
            },
        ),
        check_def(
            "the activity log command {string} has a non-empty conversation_hash",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let command = params.get_string(0).ok_or("Expected command")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = read_jsonl(&hearth.join("activity-log.jsonl"))?;
                let record = records
                    .iter()
                    .find(|record| record["command"].as_str() == Some(command.as_str()))
                    .ok_or_else(|| format!("No activity-log command '{}': {:?}", command, records))?;
                let hash = record["conversation_hash"].as_str().unwrap_or("");
                if hash.is_empty() {
                    Err(format!(
                        "activity-log command '{}' lacks conversation_hash: {}",
                        command, record
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the begin response has context text containing {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected text")?.to_string();
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let parsed = parse_mcp_tool_text(response)?;
                let context = parsed["context_text"].as_str().ok_or("No context_text")?;
                if !context.contains(&expected) {
                    return Err(format!("Context doesn't contain '{}'", expected));
                }
                Ok(())
            },
        ),
        check_def(
            "the begin response context text does not contain {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected text")?.to_string();
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let parsed = parse_mcp_tool_text(response)?;
                // context_text may be absent (omitted when empty) — treat absent
                // as "does not contain".
                let context = parsed["context_text"].as_str().unwrap_or("");
                if context.contains(&needle) {
                    return Err(format!("Context unexpectedly contains '{}'", needle));
                }
                Ok(())
            },
        ),
        check_def(
            "the begin response field {string} is exactly {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let field = params.get_string(0).ok_or("Expected field")?.to_string();
                let expected = params.get_string(1).ok_or("Expected value")?.to_string();
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let parsed = parse_mcp_tool_text(response)?;
                let actual = parsed[&field]
                    .as_str()
                    .ok_or_else(|| format!("No '{}' in begin response: {}", field, parsed))?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected begin response field '{}' to equal '{}', got '{}'",
                        field, expected, actual
                    ))
                }
            },
        ),
        // R2.4: strict JSON key-set assertion — the key must be ABSENT, not
        // present-and-empty.
        check_def(
            "the begin response does not have key {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected key name")?;
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let parsed = parse_mcp_tool_text(response)?;
                if parsed.get(key.as_ref() as &str).is_none() {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected key '{}' to be absent in begin response, but it was present with value: {}",
                        key, parsed[key.as_ref() as &str]
                    ))
                }
            },
        ),
        // R2.5: the begin response review_doc_path ends with the given suffix.
        check_def(
            "the begin response review_doc_path ends with {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let suffix = params.get_string(0).ok_or("Expected suffix")?.to_string();
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let parsed = parse_mcp_tool_text(response)?;
                let path = parsed["review_doc_path"]
                    .as_str()
                    .ok_or("No review_doc_path in begin response")?;
                if path.ends_with(&suffix) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected review_doc_path to end with '{}', got '{}'",
                        suffix, path
                    ))
                }
            },
        ),
        // ===== Tool error with message content =====
        check_def(
            "the MCP response is a tool error containing {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected text")?.to_string();
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let is_error = response["result"]["isError"].as_bool().unwrap_or(false);
                if !is_error {
                    return Err(format!(
                        "Expected tool error, got: {}",
                        serde_json::to_string_pretty(response).unwrap_or_default()
                    ));
                }
                let text = response["result"]["content"][0]["text"].as_str()
                    .unwrap_or("");
                if !text.contains(&expected) {
                    return Err(format!("Error message '{}' doesn't contain '{}'", text, expected));
                }
                Ok(())
            },
        ),
        // (Hearth seeding steps — `the track … has spec.md`, `the hearth tracks.md is seeded`,
        //  `the hearth execution.md is seeded`, `the hearth forward.md is seeded` — live in
        //  `hearth.rs`. They are pure filesystem setup with no MCP involvement.)
        // ===== Review spec strand: MCP call variants =====
        step_def(
            "a describe tools/call is sent with id {string}",
            &[("mcp_process", "McpProcess")],
            &[("mcp_process", "McpProcess"), ("response", "JsonValue"), ("hearth_path", "PathBuf")],
            |mut ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;

                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 41,
                    "method": "tools/call",
                    "params": {
                        "name": "describe",
                        "arguments": {
                            "identifier": id
                        }
                    }
                });

                process.send(&request);
                std::thread::sleep(std::time::Duration::from_millis(3000));
                let response = process.read_response()?;

                let mut out = Context::new();
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                out.set("mcp_process", process);
                out.set("response", response);
                Ok(out)
            },
        ),
        step_def(
            "a begin tools/call is sent with identifier {string}",
            &[("mcp_process", "McpProcess")],
            &[("mcp_process", "McpProcess"), ("response", "JsonValue"), ("hearth_path", "PathBuf")],
            |mut ctx, params| {
                let id = params.get_string(0).ok_or("Expected identifier")?.to_string();
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;

                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 42,
                    "method": "tools/call",
                    "params": {
                        "name": "begin",
                        "arguments": {
                            "identifier": id,
                            "actor_name": "TestActor-000100",
                            "actor_type": "agent",
                            "actor_model": "test-model",
                            "actor_provider": "test"
                        }
                    }
                });

                process.send(&request);
                std::thread::sleep(std::time::Duration::from_millis(3000));
                let response = process.read_response()?;

                let mut out = Context::new();
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                out.set("mcp_process", process);
                out.set("response", response);
                Ok(out)
            },
        ),
        // ===== Review spec strand: execution_route & next_step assertions =====
        check_def(
            "the checkin response available type {string} has execution_route {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let type_name = params.get_string(0).ok_or("Expected type")?.to_string();
                let expected = params.get_string(1).ok_or("Expected value")?.to_string();
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let text = response["result"]["content"][0]["text"].as_str()
                    .ok_or("Missing content text")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse: {}", e))?;
                let types = parsed["available_types"].as_array()
                    .ok_or("Missing available_types")?;
                let found = types.iter().find(|t| t["name"].as_str() == Some(&type_name))
                    .ok_or_else(|| format!("Type '{}' not in available_types", type_name))?;
                let actual = found["execution_route"].as_str()
                    .ok_or_else(|| format!("Type '{}' missing execution_route", type_name))?;
                if actual != expected {
                    return Err(format!(
                        "Type '{}' execution_route: expected '{}', got '{}'",
                        type_name, expected, actual
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the checkin response artifact {string} has execution_route {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let artifact_id = params.get_string(0).ok_or("Expected id")?.to_string();
                let expected = params.get_string(1).ok_or("Expected value")?.to_string();
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let text = response["result"]["content"][0]["text"].as_str()
                    .ok_or("Missing content text")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse: {}", e))?;
                let artifacts = parsed["filtered_artifacts"].as_array()
                    .ok_or("Missing filtered_artifacts")?;
                let found = artifacts.iter().find(|a| a["id"].as_str() == Some(&artifact_id))
                    .ok_or_else(|| format!("Artifact '{}' not found", artifact_id))?;
                let actual = found["execution_route"].as_str()
                    .ok_or_else(|| format!("Artifact '{}' missing execution_route", artifact_id))?;
                if actual != expected {
                    return Err(format!(
                        "Artifact '{}' execution_route: expected '{}', got '{}'",
                        artifact_id, expected, actual
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the checkin response has a non-empty next_step text",
            &[("response", "JsonValue")],
            |ctx, _params| {
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let text = response["result"]["content"][0]["text"].as_str()
                    .ok_or("Missing content text")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse: {}", e))?;
                let next = parsed["next_step"].as_str().unwrap_or("");
                if next.is_empty() {
                    return Err(format!("checkin response missing non-empty next_step. Got: {}", parsed));
                }
                Ok(())
            },
        ),
        check_def(
            "the describe response has an action with execution_route {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected value")?.to_string();
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let text = response["result"]["content"][0]["text"].as_str()
                    .ok_or("Missing content text")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse: {}", e))?;
                let actions = parsed["available_actions"].as_array()
                    .ok_or("Missing available_actions")?;
                let found = actions.iter().any(|a| a["execution_route"].as_str() == Some(&expected));
                if !found {
                    return Err(format!(
                        "No action with execution_route '{}' found in actions: {:?}",
                        expected,
                        actions.iter().map(|a| a.clone()).collect::<Vec<_>>()
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the describe response has a non-empty next_step text",
            &[("response", "JsonValue")],
            |ctx, _params| {
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let text = response["result"]["content"][0]["text"].as_str()
                    .ok_or("Missing content text")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse: {}", e))?;
                let next = parsed["next_step"].as_str().unwrap_or("");
                if next.is_empty() {
                    return Err(format!("describe response missing non-empty next_step. Got: {}", parsed));
                }
                Ok(())
            },
        ),
        // ===== Review spec strand: begin response field assertions =====
        check_def(
            "the begin response has artifact_text containing {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected text")?.to_string();
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let text = response["result"]["content"][0]["text"].as_str()
                    .ok_or("Missing content text")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse: {}", e))?;
                let artifact_text = parsed["artifact_text"].as_str()
                    .ok_or("Missing artifact_text in begin response")?;
                if !artifact_text.contains(&expected) {
                    return Err(format!("artifact_text doesn't contain '{}'. Got: '{}'", expected, artifact_text));
                }
                Ok(())
            },
        ),
        check_def(
            "the begin response has review_context_text containing {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected text")?.to_string();
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let text = response["result"]["content"][0]["text"].as_str()
                    .ok_or("Missing content text")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse: {}", e))?;
                let review_context = parsed["review_context_text"].as_str()
                    .ok_or("Missing review_context_text in begin response")?;
                if !review_context.contains(&expected) {
                    return Err(format!("review_context_text doesn't contain '{}'. Got: '{}'", expected, review_context));
                }
                Ok(())
            },
        ),
        check_def(
            "the begin response has a review_doc_path",
            &[("response", "JsonValue")],
            |ctx, _params| {
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let text = response["result"]["content"][0]["text"].as_str()
                    .ok_or("Missing content text")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse: {}", e))?;
                let path = parsed["review_doc_path"].as_str()
                    .ok_or("Missing review_doc_path in begin response")?;
                if path.is_empty() {
                    return Err("review_doc_path is empty".to_string());
                }
                Ok(())
            },
        ),
        // ===== Review spec strand: registry / projection / filesystem assertions =====
        check_def(
            "the hearth tracks.md contains {string} under {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?.to_string();
                let section = params.get_string(1).ok_or("Expected section")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let content = std::fs::read_to_string(hearth.join("tracks.md"))
                    .map_err(|e| format!("Failed to read tracks.md: {}", e))?;
                let header_pat = format!("{}\n", section);
                let section_pos = content.find(&header_pat)
                    .ok_or_else(|| format!("Section '{}' not found in tracks.md", section))?;
                let after = &content[section_pos + header_pat.len()..];
                let next = after.find("\n## ").unwrap_or(after.len());
                let section_content = &after[..next];
                if !section_content.contains(&needle) {
                    return Err(format!(
                        "'{}' not found under '{}' in tracks.md. Section content:\n{}",
                        needle, section, section_content
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the hearth tracks.md does not contain {string} under {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?.to_string();
                let section = params.get_string(1).ok_or("Expected section")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let content = std::fs::read_to_string(hearth.join("tracks.md"))
                    .map_err(|e| format!("Failed to read tracks.md: {}", e))?;
                let header_pat = format!("{}\n", section);
                let section_pos = match content.find(&header_pat) {
                    Some(p) => p,
                    None => return Ok(()),
                };
                let after = &content[section_pos + header_pat.len()..];
                let next = after.find("\n## ").unwrap_or(after.len());
                let section_content = &after[..next];
                if section_content.contains(&needle) {
                    return Err(format!(
                        "'{}' unexpectedly found under '{}' in tracks.md",
                        needle, section
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the hearth execution.md contains {string} in the {string} section",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?.to_string();
                let section = params.get_string(1).ok_or("Expected section")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let content = std::fs::read_to_string(hearth.join("projections/execution.md"))
                    .map_err(|e| format!("Failed to read execution.md: {}", e))?;
                let header_pat = format!("## {} (", section);
                let section_pos = content.find(&header_pat)
                    .ok_or_else(|| format!("Section '{}' not found in execution.md", section))?;
                let after = &content[section_pos..];
                let next = after[1..].find("\n## ").map(|p| p + 1).unwrap_or(after.len());
                let section_content = &after[..next];
                if !section_content.contains(&needle) {
                    return Err(format!(
                        "'{}' not found in '{}' section. Content:\n{}",
                        needle, section, section_content
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the hearth has spec.review.md at track {string} starting with:",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let track_id = params.get_string(0).ok_or("Expected track id")?.to_string();
                let expected = params.doc_string().ok_or("Expected doc string with expected prefix")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let review_path = hearth.join("tracks").join(&track_id).join("spec.review.md");
                if !review_path.exists() {
                    return Err(format!("spec.review.md not found at {}", review_path.display()));
                }
                let content = std::fs::read_to_string(&review_path)
                    .map_err(|e| format!("Failed to read spec.review.md: {}", e))?;
                if !content.starts_with(&expected) {
                    return Err(format!(
                        "spec.review.md does not start with expected prefix.\nExpected:\n{}\nGot (first {} chars):\n{}",
                        expected,
                        expected.len().max(200),
                        content.chars().take(expected.len().max(200)).collect::<String>()
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the hearth has actor {string} in the actors table of track {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let actor_name = params.get_string(0).ok_or("Expected actor name")?.to_string();
                let track_id = params.get_string(1).ok_or("Expected track id")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let status_path = hearth.join("tracks").join(&track_id).join("status.yaml");
                let content = std::fs::read_to_string(&status_path)
                    .map_err(|e| format!("Failed to read status.yaml: {}", e))?;
                // Parse into a minimal view: find the `actors:` section
                // and check whether `  {name}:` appears at actor-indent
                // level before the next top-level key.
                let lines: Vec<&str> = content.lines().collect();
                let actors_idx = lines.iter().position(|l| l.trim_start() == "actors:")
                    .ok_or_else(|| format!("No `actors:` section in status.yaml:\n{}", content))?;
                let target = format!("  {}:", actor_name);
                let mut found = false;
                for j in (actors_idx + 1)..lines.len() {
                    if !lines[j].starts_with(' ') && !lines[j].is_empty() {
                        break; // left the actors section
                    }
                    if lines[j] == target {
                        found = true;
                        break;
                    }
                }
                if !found {
                    return Err(format!(
                        "Actor '{}' not found in actors table of status.yaml. Content:\n{}",
                        actor_name, content
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the hearth status.yaml of track {string} has an actor with model {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let track_id = params.get_string(0).ok_or("Expected track id")?.to_string();
                let expected_model = params.get_string(1).ok_or("Expected model")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let status_path = hearth.join("tracks").join(&track_id).join("status.yaml");
                let content = std::fs::read_to_string(&status_path)
                    .map_err(|e| format!("Failed to read status.yaml: {}", e))?;
                // Actors section must exist.
                if !content.lines().any(|l| l.trim_start() == "actors:") {
                    return Err(format!(
                        "No `actors:` section in status.yaml. Content:\n{}",
                        content
                    ));
                }
                // Must contain a `model: <expected>` line somewhere in the
                // actors region (before `transitions:`).
                let mut in_actors = false;
                let mut found = false;
                for line in content.lines() {
                    let trimmed = line.trim_start();
                    if trimmed == "actors:" { in_actors = true; continue; }
                    if in_actors
                        && !line.starts_with(' ')
                        && !line.is_empty()
                    {
                        // Top-level key after actors — exited.
                        break;
                    }
                    if in_actors && trimmed.starts_with("model:") {
                        let val = trimmed.trim_start_matches("model:").trim();
                        if val == expected_model {
                            found = true;
                            break;
                        }
                    }
                }
                if !found {
                    return Err(format!(
                        "No actor with `model: {}` in the actors section. Content:\n{}",
                        expected_model, content
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the hearth status.yaml of track {string} has an actor block with a configurations list",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let track_id = params.get_string(0).ok_or("Expected track id")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let status_path = hearth.join("tracks").join(&track_id).join("status.yaml");
                let content = std::fs::read_to_string(&status_path)
                    .map_err(|e| format!("Failed to read status.yaml: {}", e))?;
                if !content.contains("configurations:") {
                    return Err(format!(
                        "No `configurations:` key in actors section. Content:\n{}",
                        content
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the hearth forward.md is unchanged",
            &[("hearth_path", "PathBuf")],
            |ctx, _params| {
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let pre = std::fs::read_to_string(hearth.join("projections/.forward.md.seed"))
                    .map_err(|_| "No .forward.md.seed — did the 'forward.md is seeded with' step run?".to_string())?;
                let current = std::fs::read_to_string(hearth.join("projections/forward.md"))
                    .map_err(|e| format!("Failed to read forward.md: {}", e))?;
                if current != pre {
                    return Err(format!(
                        "forward.md changed. Expected:\n{}\n\nGot:\n{}",
                        pre, current
                    ));
                }
                Ok(())
            },
        ),

        // ===== Snapshot tool step defs =====
        step_def(
            "a snapshot tools/call is sent with:",
            &[("mcp_process", "McpProcess")],
            &[
                ("mcp_process", "McpProcess"),
                ("response", "JsonValue"),
                ("hearth_path", "PathBuf"),
                ("captured_actor_name", "String"),
            ],
            |mut ctx, params| {
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
                let mut fields: std::collections::HashMap<String, String> =
                    std::collections::HashMap::new();
                for (k, v) in pairs {
                    fields.insert(k, v);
                }
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;

                let mut arguments = serde_json::Map::new();
                for key in &[
                    "artifact_path",
                    "to_state",
                    "actor_role",
                    "approver",
                    "note",
                    "event_type",
                    "actor_name",
                    "actor_type",
                    "actor_model",
                    "actor_provider",
                    "actor_sdk_version",
                    "actor_entrypoint",
                ] {
                    if let Some(v) = fields.get(*key) {
                        arguments.insert((*key).to_string(), serde_json::Value::String(v.clone()));
                    }
                }
                if let Some(v) = fields.get("projection_only") {
                    arguments.insert(
                        "projection_only".to_string(),
                        serde_json::Value::Bool(v == "true"),
                    );
                }
                if let Some(v) = fields.get("actor_context_window") {
                    if let Ok(n) = v.parse::<i64>() {
                        arguments.insert(
                            "actor_context_window".to_string(),
                            serde_json::Value::Number(n.into()),
                        );
                    }
                }

                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 77,
                    "method": "tools/call",
                    "params": {
                        "name": "snapshot",
                        "arguments": serde_json::Value::Object(arguments)
                    }
                });
                process.send(&request);
                std::thread::sleep(std::time::Duration::from_millis(3000));
                let response = process.read_response()?;

                let mut out = Context::new();
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                if let Some(name) = ctx.get::<String>("captured_actor_name") {
                    out.set("captured_actor_name", name.clone());
                }
                out.set("mcp_process", process);
                out.set("response", response);
                Ok(out)
            },
        ),
        check_def(
            "the snapshot response actor_name matches {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let pattern = params.get_string(0).ok_or("Expected pattern")?;
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("Missing content text")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse: {}", e))?;
                let name = parsed["actor_name"].as_str().ok_or("No actor_name")?;
                if anvil_test_support::snapshot::SimpleRegex::matches(pattern, name) {
                    Ok(())
                } else {
                    Err(format!("actor_name '{}' does not match '{}'", name, pattern))
                }
            },
        ),
        check_def(
            "the snapshot response actor_name is {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected name")?;
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("Missing content text")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse: {}", e))?;
                let name = parsed["actor_name"].as_str().ok_or("No actor_name")?;
                if name == expected {
                    Ok(())
                } else {
                    Err(format!("Expected actor_name '{}', got '{}'", expected, name))
                }
            },
        ),
        check_def(
            "the snapshot response projections_updated contains {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected file")?;
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("Missing content text")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse: {}", e))?;
                let arr = parsed["projections_updated"]
                    .as_array()
                    .ok_or("No projections_updated array")?;
                if arr.iter().any(|v| v.as_str() == Some(expected)) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected '{}' in projections_updated, got {:?}",
                        expected, arr
                    ))
                }
            },
        ),
        check_def(
            "the snapshot tool has no actor_name property",
            &[("response", "JsonValue")],
            |ctx, _params| {
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let tools = response["result"]["tools"]
                    .as_array()
                    .ok_or("Missing result.tools array")?;
                let snapshot = tools
                    .iter()
                    .find(|t| t["name"].as_str() == Some("snapshot"))
                    .ok_or("No snapshot tool found")?;
                let props = &snapshot["inputSchema"]["properties"];
                if !props["actor_name"].is_null() {
                    return Err("snapshot tool schema should not include actor_name".to_string());
                }
                Ok(())
            },
        ),
        check_def(
            "the snapshot tool has optional property {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected name")?;
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let tools = response["result"]["tools"]
                    .as_array()
                    .ok_or("Missing result.tools array")?;
                let snapshot = tools
                    .iter()
                    .find(|t| t["name"].as_str() == Some("snapshot"))
                    .ok_or("No snapshot tool found")?;
                let props = &snapshot["inputSchema"]["properties"];
                if props[name].is_null() {
                    return Err(format!(
                        "snapshot tool schema missing optional property '{}'",
                        name
                    ));
                }
                let required = snapshot["inputSchema"]["required"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                if required.iter().any(|r| r == name) {
                    return Err(format!(
                        "Expected '{}' to be optional but it's listed in required",
                        name
                    ));
                }
                Ok(())
            },
        ),
        step_def(
            "the first snapshot response actor_name is captured",
            &[("response", "JsonValue"), ("mcp_process", "McpProcess")],
            &[
                ("response", "JsonValue"),
                ("mcp_process", "McpProcess"),
                ("hearth_path", "PathBuf"),
                ("captured_actor_name", "String"),
            ],
            |mut ctx, _params| {
                let response = ctx
                    .take::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("Missing content text")?
                    .to_string();
                let parsed: serde_json::Value = serde_json::from_str(&text)
                    .map_err(|e| format!("Failed to parse: {}", e))?;
                let name = parsed["actor_name"]
                    .as_str()
                    .ok_or("No actor_name in response")?
                    .to_string();
                let mcp_process = ctx
                    .take::<McpProcess>("mcp_process")
                    .ok_or("No mcp_process")?;
                let mut out = Context::new();
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                out.set("mcp_process", mcp_process);
                out.set("response", response);
                out.set("captured_actor_name", name);
                Ok(out)
            },
        ),
        check_def(
            "the snapshot response actor_name matches the captured first actor_name",
            &[("response", "JsonValue"), ("captured_actor_name", "String")],
            |ctx, _params| {
                let expected = ctx
                    .get::<String>("captured_actor_name")
                    .ok_or("No captured_actor_name")?;
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("Missing content text")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse: {}", e))?;
                let name = parsed["actor_name"].as_str().ok_or("No actor_name")?;
                if name == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected follow-up actor_name to match captured '{}', got '{}'",
                        expected, name
                    ))
                }
            },
        ),
        check_def(
            "the snapshot tool requires property {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected name")?;
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let tools = response["result"]["tools"]
                    .as_array()
                    .ok_or("Missing result.tools array")?;
                let snapshot = tools
                    .iter()
                    .find(|t| t["name"].as_str() == Some("snapshot"))
                    .ok_or("No snapshot tool found")?;
                let required = snapshot["inputSchema"]["required"]
                    .as_array()
                    .ok_or("No required array")?;
                if required.iter().any(|v| v.as_str() == Some(name)) {
                    Ok(())
                } else {
                    Err(format!("'{}' not in required list: {:?}", name, required))
                }
            },
        ),

        // ===== Complete tool step defs =====
        step_def(
            "a complete tools/call is sent with:",
            &[("mcp_process", "McpProcess")],
            &[
                ("mcp_process", "McpProcess"),
                ("complete_response", "JsonValue"),
                ("hearth_path", "PathBuf"),
            ],
            |mut ctx, params| {
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
                let mut fields: std::collections::HashMap<String, String> =
                    std::collections::HashMap::new();
                for (k, v) in pairs {
                    fields.insert(k, v);
                }
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;

                let mut arguments = serde_json::Map::new();
                for key in &[
                    "artifact_path",
                    "actor_name",
                    "actor_type",
                    "actor_model",
                    "actor_provider",
                    "actor_sdk_version",
                    "actor_entrypoint",
                    "satisfaction",
                    "findings",
                    "approver",
                    "note",
                    "reflection_notes",
                ] {
                    if let Some(v) = fields.get(*key) {
                        arguments.insert((*key).to_string(), serde_json::Value::String(v.clone()));
                    }
                }
                if let Some(v) = fields.get("actor_context_window") {
                    if let Ok(n) = v.parse::<i64>() {
                        arguments.insert(
                            "actor_context_window".to_string(),
                            serde_json::Value::Number(n.into()),
                        );
                    }
                }

                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 88,
                    "method": "tools/call",
                    "params": {
                        "name": "complete",
                        "arguments": serde_json::Value::Object(arguments)
                    }
                });
                process.send(&request);
                std::thread::sleep(std::time::Duration::from_millis(3000));
                let response = process.read_response()?;

                let mut out = Context::new();
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                out.set("mcp_process", process);
                out.set("complete_response", response);
                Ok(out)
            },
        ),
        check_def(
            "the complete response new_state is {string}",
            &[("complete_response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected new_state")?;
                let response = ctx
                    .get::<serde_json::Value>("complete_response")
                    .ok_or("No complete_response")?;
                if let Some(err) = response["result"]["content"][0]["text"]
                    .as_str()
                    .filter(|t| t.contains("isError") || response["result"]["isError"].as_bool().unwrap_or(false))
                {
                    return Err(format!("Complete tool returned error: {}", err));
                }
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("No text in response")?;
                // Check for isError at result level
                if response["result"]["isError"].as_bool().unwrap_or(false) {
                    return Err(format!("Complete tool returned error: {}", text));
                }
                let parsed: serde_json::Value =
                    serde_json::from_str(text).map_err(|e| format!("Invalid JSON: {}", e))?;
                let new_state = parsed["new_state"]
                    .as_str()
                    .ok_or("No new_state in response")?;
                if new_state == expected {
                    Ok(())
                } else {
                    Err(format!("Expected new_state '{}', got '{}'", expected, new_state))
                }
            },
        ),
        check_def(
            "the complete response transition_at is non-empty",
            &[("complete_response", "JsonValue")],
            |ctx, _params| {
                let response = ctx
                    .get::<serde_json::Value>("complete_response")
                    .ok_or("No complete_response")?;
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("No text in response")?;
                if response["result"]["isError"].as_bool().unwrap_or(false) {
                    return Err(format!("Complete tool returned error: {}", text));
                }
                let parsed: serde_json::Value =
                    serde_json::from_str(text).map_err(|e| format!("Invalid JSON: {}", e))?;
                let transition_at = parsed["transition_at"]
                    .as_str()
                    .ok_or("No transition_at in response")?;
                if transition_at.is_empty() {
                    Err("transition_at is empty".to_string())
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the complete response artifact_path is {string}",
            &[("complete_response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected artifact_path")?;
                let response = ctx
                    .get::<serde_json::Value>("complete_response")
                    .ok_or("No complete_response")?;
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("No text in response")?;
                if response["result"]["isError"].as_bool().unwrap_or(false) {
                    return Err(format!("Complete tool returned error: {}", text));
                }
                let parsed: serde_json::Value =
                    serde_json::from_str(text).map_err(|e| format!("Invalid JSON: {}", e))?;
                let artifact_path = parsed["artifact_path"]
                    .as_str()
                    .ok_or("No artifact_path in response")?;
                if artifact_path == expected {
                    Ok(())
                } else {
                    Err(format!("Expected artifact_path '{}', got '{}'", expected, artifact_path))
                }
            },
        ),
        check_def(
            "the tools list contains {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected tool name")?;
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let tools = response["result"]["tools"]
                    .as_array()
                    .ok_or("Missing result.tools array")?;
                if tools.iter().any(|t| t["name"].as_str() == Some(expected)) {
                    Ok(())
                } else {
                    let names: Vec<&str> = tools
                        .iter()
                        .filter_map(|t| t["name"].as_str())
                        .collect();
                    Err(format!("Expected tool '{}' in list {:?}", expected, names))
                }
            },
        ),
        check_def(
            "the {string} tool inputSchema requires {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let tool_name = params.get_string(0).ok_or("Missing tool name")?;
                let field = params.get_string(1).ok_or("Missing field name")?;
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let tools = response["result"]["tools"]
                    .as_array()
                    .ok_or("Missing result.tools array")?;
                let tool = tools
                    .iter()
                    .find(|t| t["name"].as_str() == Some(tool_name))
                    .ok_or_else(|| format!("No {} tool found in tools list", tool_name))?;
                let required = tool["inputSchema"]["required"]
                    .as_array()
                    .ok_or_else(|| format!("Missing inputSchema.required array on {} tool", tool_name))?;
                if required.iter().any(|r| r.as_str() == Some(field)) {
                    Ok(())
                } else {
                    Err(format!(
                        "Field '{}' not in {} inputSchema required: {:?}",
                        field,
                        tool_name,
                        required.iter().filter_map(|r| r.as_str()).collect::<Vec<_>>()
                    ))
                }
            },
        ),
        check_def(
            "the {string} tool inputSchema has property {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let tool_name = params.get_string(0).ok_or("Missing tool name")?;
                let field = params.get_string(1).ok_or("Missing property name")?;
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let tools = response["result"]["tools"]
                    .as_array()
                    .ok_or("Missing result.tools array")?;
                let tool = tools
                    .iter()
                    .find(|t| t["name"].as_str() == Some(tool_name))
                    .ok_or_else(|| format!("No {} tool found in tools list", tool_name))?;
                if tool["inputSchema"]["properties"].get(field).is_some() {
                    Ok(())
                } else {
                    Err(format!(
                        "Property '{}' not found in {} inputSchema properties",
                        field, tool_name
                    ))
                }
            },
        ),
        check_def(
            "the {string} tool inputSchema has no property {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let tool_name = params.get_string(0).ok_or("Missing tool name")?;
                let field = params.get_string(1).ok_or("Missing property name")?;
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let tools = response["result"]["tools"]
                    .as_array()
                    .ok_or("Missing result.tools array")?;
                let tool = tools
                    .iter()
                    .find(|t| t["name"].as_str() == Some(tool_name))
                    .ok_or_else(|| format!("No {} tool found in tools list", tool_name))?;
                if tool["inputSchema"]["properties"].get(field).is_none() {
                    Ok(())
                } else {
                    Err(format!(
                        "Property '{}' unexpectedly found in {} inputSchema properties",
                        field, tool_name
                    ))
                }
            },
        ),
        // ===== Complete reflection_notes — key-presence and path checks =====
        check_def(
            "the complete response has key {string}",
            &[("complete_response", "JsonValue")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected key name")?;
                let response = ctx
                    .get::<serde_json::Value>("complete_response")
                    .ok_or("No complete_response")?;
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("No text in response")?;
                if response["result"]["isError"].as_bool().unwrap_or(false) {
                    return Err(format!("Complete tool returned error: {}", text));
                }
                let parsed: serde_json::Value =
                    serde_json::from_str(text).map_err(|e| format!("Invalid JSON: {}", e))?;
                if parsed.get(key).is_some() {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected key '{}' in complete response, but it was absent. Response: {}",
                        key, text
                    ))
                }
            },
        ),
        check_def(
            "the complete response does not have key {string}",
            &[("complete_response", "JsonValue")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected key name")?;
                let response = ctx
                    .get::<serde_json::Value>("complete_response")
                    .ok_or("No complete_response")?;
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("No text in response")?;
                if response["result"]["isError"].as_bool().unwrap_or(false) {
                    return Err(format!("Complete tool returned error: {}", text));
                }
                let parsed: serde_json::Value =
                    serde_json::from_str(text).map_err(|e| format!("Invalid JSON: {}", e))?;
                if parsed.get(key).is_none() {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected key '{}' to be absent in complete response, but it was present with value: {}",
                        key, parsed[key]
                    ))
                }
            },
        ),
        check_def(
            "the complete response reflection_path ends with {string}",
            &[("complete_response", "JsonValue")],
            |ctx, params| {
                let suffix = params.get_string(0).ok_or("Expected suffix")?;
                let response = ctx
                    .get::<serde_json::Value>("complete_response")
                    .ok_or("No complete_response")?;
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("No text in response")?;
                if response["result"]["isError"].as_bool().unwrap_or(false) {
                    return Err(format!("Complete tool returned error: {}", text));
                }
                let parsed: serde_json::Value =
                    serde_json::from_str(text).map_err(|e| format!("Invalid JSON: {}", e))?;
                let path = parsed["reflection_path"]
                    .as_str()
                    .ok_or("No reflection_path in complete response")?;
                if path.ends_with(suffix) {
                    Ok(())
                } else {
                    Err(format!(
                        "reflection_path '{}' does not end with '{}'",
                        path, suffix
                    ))
                }
            },
        ),
        check_def(
            "the complete response carry_forward_path ends with {string}",
            &[("complete_response", "JsonValue")],
            |ctx, params| {
                let suffix = params.get_string(0).ok_or("Expected suffix")?;
                let response = ctx
                    .get::<serde_json::Value>("complete_response")
                    .ok_or("No complete_response")?;
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("No text in response")?;
                if response["result"]["isError"].as_bool().unwrap_or(false) {
                    return Err(format!("Complete tool returned error: {}", text));
                }
                let parsed: serde_json::Value =
                    serde_json::from_str(text).map_err(|e| format!("Invalid JSON: {}", e))?;
                let path = parsed["carry_forward_path"]
                    .as_str()
                    .ok_or("No carry_forward_path in complete response")?;
                if path.ends_with(suffix) {
                    Ok(())
                } else {
                    Err(format!(
                        "carry_forward_path '{}' does not end with '{}'",
                        path, suffix
                    ))
                }
            },
        ),
        check_def(
            "the complete response reflection_path contains {string}",
            &[("complete_response", "JsonValue")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let response = ctx
                    .get::<serde_json::Value>("complete_response")
                    .ok_or("No complete_response")?;
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("No text in response")?;
                if response["result"]["isError"].as_bool().unwrap_or(false) {
                    return Err(format!("Complete tool returned error: {}", text));
                }
                let parsed: serde_json::Value =
                    serde_json::from_str(text).map_err(|e| format!("Invalid JSON: {}", e))?;
                let path = parsed["reflection_path"]
                    .as_str()
                    .ok_or("No reflection_path in complete response")?;
                if path.contains(needle) {
                    Ok(())
                } else {
                    Err(format!(
                        "reflection_path '{}' does not contain '{}'",
                        path, needle
                    ))
                }
            },
        ),
        check_def(
            "the complete response reflection_path file contains {string}",
            &[("complete_response", "JsonValue")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let response = ctx
                    .get::<serde_json::Value>("complete_response")
                    .ok_or("No complete_response")?;
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("No text in response")?;
                if response["result"]["isError"].as_bool().unwrap_or(false) {
                    return Err(format!("Complete tool returned error: {}", text));
                }
                let parsed: serde_json::Value =
                    serde_json::from_str(text).map_err(|e| format!("Invalid JSON: {}", e))?;
                let path = parsed["reflection_path"]
                    .as_str()
                    .ok_or("No reflection_path in complete response")?;
                let content = std::fs::read_to_string(path)
                    .map_err(|e| format!("Failed to read reflection file '{}': {}", path, e))?;
                if content.contains(needle) {
                    Ok(())
                } else {
                    Err(format!(
                        "Reflection file '{}' does not contain '{}'. Content:\n{}",
                        path, needle, content
                    ))
                }
            },
        ),
        check_def(
            "the complete response reflection_path file does not contain {string}",
            &[("complete_response", "JsonValue")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let response = ctx
                    .get::<serde_json::Value>("complete_response")
                    .ok_or("No complete_response")?;
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("No text in response")?;
                if response["result"]["isError"].as_bool().unwrap_or(false) {
                    return Err(format!("Complete tool returned error: {}", text));
                }
                let parsed: serde_json::Value =
                    serde_json::from_str(text).map_err(|e| format!("Invalid JSON: {}", e))?;
                let path = parsed["reflection_path"]
                    .as_str()
                    .ok_or("No reflection_path in complete response")?;
                let content = std::fs::read_to_string(path)
                    .map_err(|e| format!("Failed to read reflection file '{}': {}", path, e))?;
                if !content.contains(needle) {
                    Ok(())
                } else {
                    Err(format!(
                        "Reflection file '{}' unexpectedly contains '{}'. Content:\n{}",
                        path, needle, content
                    ))
                }
            },
        ),

        // ===== Amend tool steps =====

        step_def(
            "an amend tools/call is sent with:",
            &[("mcp_process", "McpProcess")],
            &[
                ("mcp_process", "McpProcess"),
                ("amend_response", "JsonValue"),
                ("hearth_path", "PathBuf"),
            ],
            |mut ctx, params| {
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
                let mut fields: std::collections::HashMap<String, String> =
                    std::collections::HashMap::new();
                for (k, v) in pairs {
                    fields.insert(k, v);
                }
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;

                let mut arguments = serde_json::Map::new();
                for key in &[
                    "artifact_path",
                    "kind",
                    "target_document",
                    "target_id",
                    "op_kind",
                    "body",
                    "new_kind",
                    "anchor",
                    "actor_name",
                    "actor_type",
                    "actor_model",
                    "actor_provider",
                    "actor_sdk_version",
                    "actor_entrypoint",
                ] {
                    if let Some(v) = fields.get(*key) {
                        arguments.insert((*key).to_string(), serde_json::Value::String(v.clone()));
                    }
                }
                if let Some(v) = fields.get("actor_context_window") {
                    if let Ok(n) = v.parse::<i64>() {
                        arguments.insert(
                            "actor_context_window".to_string(),
                            serde_json::Value::Number(n.into()),
                        );
                    }
                }

                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 89,
                    "method": "tools/call",
                    "params": {
                        "name": "amend",
                        "arguments": serde_json::Value::Object(arguments)
                    }
                });
                process.send(&request);
                std::thread::sleep(std::time::Duration::from_millis(3000));
                let response = process.read_response()?;

                let mut out = Context::new();
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                out.set("mcp_process", process);
                out.set("amend_response", response);
                Ok(out)
            },
        ),
        check_def(
            "the amend response op_id is non-empty",
            &[("amend_response", "JsonValue")],
            |ctx, _params| {
                let response = ctx
                    .get::<serde_json::Value>("amend_response")
                    .ok_or("No amend_response")?;
                if response["result"]["isError"].as_bool().unwrap_or(false) {
                    let text = response["result"]["content"][0]["text"]
                        .as_str()
                        .unwrap_or("(no text)");
                    return Err(format!("Amend tool returned error: {}", text));
                }
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("No text in amend response")?;
                let parsed: serde_json::Value =
                    serde_json::from_str(text).map_err(|e| format!("Invalid JSON: {}", e))?;
                let op_id = parsed["op_id"]
                    .as_str()
                    .ok_or("No op_id in amend response")?;
                if op_id.is_empty() {
                    Err("amend response op_id is empty".to_string())
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the amend response has no new_state",
            &[("amend_response", "JsonValue")],
            |ctx, _params| {
                let response = ctx
                    .get::<serde_json::Value>("amend_response")
                    .ok_or("No amend_response")?;
                if response["result"]["isError"].as_bool().unwrap_or(false) {
                    let text = response["result"]["content"][0]["text"]
                        .as_str()
                        .unwrap_or("(no text)");
                    return Err(format!("Amend tool returned error: {}", text));
                }
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("No text in amend response")?;
                let parsed: serde_json::Value =
                    serde_json::from_str(text).map_err(|e| format!("Invalid JSON: {}", e))?;
                if parsed.get("new_state").is_none() {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected new_state to be absent in amend response, but got: {}",
                        parsed["new_state"]
                    ))
                }
            },
        ),
        check_def(
            "the amend response new_state is {string}",
            &[("amend_response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected new_state")?;
                let response = ctx
                    .get::<serde_json::Value>("amend_response")
                    .ok_or("No amend_response")?;
                if response["result"]["isError"].as_bool().unwrap_or(false) {
                    let text = response["result"]["content"][0]["text"]
                        .as_str()
                        .unwrap_or("(no text)");
                    return Err(format!("Amend tool returned error: {}", text));
                }
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("No text in amend response")?;
                let parsed: serde_json::Value =
                    serde_json::from_str(text).map_err(|e| format!("Invalid JSON: {}", e))?;
                let new_state = parsed["new_state"]
                    .as_str()
                    .ok_or("No new_state in amend response")?;
                if new_state == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected new_state '{}', got '{}'",
                        expected, new_state
                    ))
                }
            },
        ),
        check_def(
            "the amend tool inputSchema requires {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let field = params.get_string(0).ok_or("Missing field name")?.to_string();
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let tools = response["result"]["tools"]
                    .as_array()
                    .ok_or("Missing result.tools array")?;
                let amend_tool = tools
                    .iter()
                    .find(|t| t["name"].as_str() == Some("amend"))
                    .ok_or("No amend tool found in tools list")?;
                let required = amend_tool["inputSchema"]["required"]
                    .as_array()
                    .ok_or("Missing inputSchema.required array on amend tool")?;
                let has_field = required.iter().any(|r| r.as_str() == Some(&field));
                if !has_field {
                    return Err(format!(
                        "Field '{}' not in amend inputSchema required: {:?}",
                        field,
                        required.iter().filter_map(|r| r.as_str()).collect::<Vec<_>>()
                    ));
                }
                Ok(())
            },
        ),

        // ===== PersistPlaybook MCP tool steps =====

        step_def(
            "a separate temp owner-home directory for MCP persist_playbook",
            &[("mcp_process", "McpProcess")],
            &[
                ("mcp_process", "McpProcess"),
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("mcp_persist_owner_home", "PathBuf"),
                ("mcp_persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |mut ctx, _params| {
                let process = ctx
                    .take::<McpProcess>("mcp_process")
                    .ok_or("No mcp_process")?;
                let engine_process = ctx.take::<EngineProcess>("engine_process");
                let dir = tempfile::TempDir::new()
                    .map_err(|e| format!("temp owner-home: {}", e))?;
                let path = dir.path().to_path_buf();
                let handle: Arc<Mutex<Option<tempfile::TempDir>>> =
                    Arc::new(Mutex::new(Some(dir)));

                let mut out = Context::new();
                out.set("mcp_process", process);
                if let Some(engine_process) = engine_process {
                    out.set("engine_process", engine_process);
                }
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                out.set("mcp_persist_owner_home", path);
                out.set("mcp_persist_owner_home_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a {string} tools/call is sent for kind {string} with:",
            &[
                ("mcp_process", "McpProcess"),
                ("mcp_persist_owner_home", "PathBuf"),
                ("mcp_persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                ("mcp_process", "McpProcess"),
                ("response", "JsonValue"),
                ("persist_playbook_response", "JsonValue"),
                ("hearth_path", "PathBuf"),
                ("mcp_persist_owner_home", "PathBuf"),
                ("mcp_persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("mcp_persist_owner_home_playbooks_before", "usize"),
                ("mcp_persist_hearth_playbooks_before", "usize"),
            ],
            |mut ctx, params| {
                let tool_name = params.get_string(0).ok_or("Expected tool name")?.to_string();
                if tool_name != "persist_playbook" && tool_name != "persist_playbook" {
                    return Err(format!("Unexpected persist tool name '{}'", tool_name));
                }
                let kind = params.get_string(1).ok_or("Expected kind")?.to_string();
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

                let mut actor_name = String::new();
                let mut actor_type = String::new();
                let mut actor_model = String::new();
                let mut actor_provider = String::new();
                // The persist_playbook RPC behind the shim is the enforcing WRITE
                // boundary, so the default (and the `minimal`/`different_minimal`
                // success + collision fixtures) must be WRITE-boundary conformant:
                // an outcome_predicate plus a hooked non-terminal state that
                // references `intent.md` (auto-carried below). The negative-path
                // fixtures (hook_missing, loader_invalid) fail before/regardless.
                let mut machine_yaml = anvil_test_support::conformant_machine_yaml(&kind);
                // (filename, content) hook files carried alongside the machine via
                // the MCP `hooks` argument. A `machine | hook_bearing` +
                // `hook | intent.md` pair persists a valid hook-bearing playbook;
                // `machine | hook_missing` with NO hook row proves the unknown-hook
                // rejection through the MCP surface.
                let mut hooks: Vec<(String, String)> = Vec::new();
                for (key, value) in pairs {
                    match key.as_str() {
                        "actor_name" => actor_name = value,
                        "actor_type" => actor_type = value,
                        "actor_model" => actor_model = value,
                        "actor_provider" => actor_provider = value,
                        // Conformant (predicate + intent.md hook) so it clears the
                        // enforcing WRITE boundary.
                        "machine" if value == "minimal" => {
                            machine_yaml = anvil_test_support::conformant_machine_yaml(&kind)
                        }
                        // Byte-different conformant sibling — same-kind/different-
                        // content collision under enforcement.
                        "machine" if value == "different_minimal" => {
                            machine_yaml = anvil_test_support::different_conformant_machine_yaml(&kind)
                        }
                        "machine" if value == "loader_invalid" => {
                            machine_yaml = anvil_test_support::loader_invalid_machine_yaml(&kind)
                        }
                        // Predicate-LESS machine — refused at the enforcing WRITE
                        // boundary with playbook_measurement_definition_missing.
                        "machine" if value == "no_predicate" => {
                            machine_yaml = anvil_test_support::minimal_machine_yaml(&kind)
                        }
                        // Predicate present but the non-terminal state is hookless —
                        // refused with playbook_non_terminal_state_hookless.
                        "machine" if value == "predicate_no_hook" => {
                            machine_yaml = anvil_test_support::machine_yaml_predicate_no_hook(&kind)
                        }
                        "machine" if value == "hook_bearing" => {
                            machine_yaml = anvil_test_support::machine_yaml_with_hook(&kind, "intent.md")
                        }
                        "machine" if value == "hook_missing" => {
                            machine_yaml = anvil_test_support::machine_yaml_with_hook(&kind, "missing.md")
                        }
                        "hook" => {
                            hooks.push((value.clone(), format!("Hook body for {}", value)))
                        }
                        // Conformant machine declaring a DIFFERENT kind, so the RPC
                        // clears the WRITE boundary and then fails on kind mismatch.
                        "machine_kind" => {
                            machine_yaml = anvil_test_support::conformant_machine_yaml(&value)
                        }
                        other => {
                            return Err(format!(
                                "Unknown persist_playbook MCP field: '{}'",
                                other
                            ))
                        }
                    }
                }
                // The conformant fixtures reference `intent.md`; carry it so the
                // enforcing loader resolves the reference (unless a scenario
                // supplied its own hook rows). Harmless for the negative paths.
                if !hooks.iter().any(|(name, _)| name == "intent.md") {
                    hooks.push(("intent.md".to_string(), "Hook body for intent.md".to_string()));
                }

                let mut process = ctx
                    .take::<McpProcess>("mcp_process")
                    .ok_or("No mcp_process")?;
                let owner_home = ctx
                    .get::<PathBuf>("mcp_persist_owner_home")
                    .ok_or("No mcp_persist_owner_home")?
                    .clone();
                let handle = ctx
                    .get::<Arc<Mutex<Option<tempfile::TempDir>>>>(
                        "mcp_persist_owner_home_handle",
                    )
                    .ok_or("No mcp_persist_owner_home_handle")?
                    .clone();
                let owner_before = ctx
                    .get::<usize>("mcp_persist_owner_home_playbooks_before")
                    .copied();
                let hearth_before = ctx
                    .get::<usize>("mcp_persist_hearth_playbooks_before")
                    .copied();

                let hook_args: Vec<serde_json::Value> = hooks
                    .iter()
                    .map(|(name, content)| {
                        serde_json::json!({ "name": name, "content": content })
                    })
                    .collect();
                let mut arguments = serde_json::json!({
                    "owner_home": owner_home.to_string_lossy(),
                    "kind": kind,
                    "machine_yaml": machine_yaml,
                    "actor_name": actor_name,
                    "actor_type": actor_type,
                    "actor_model": actor_model,
                    "actor_provider": actor_provider
                });
                // Only attach `hooks` when the scenario carries any, so existing
                // hook-free scenarios send byte-identical arguments (no `hooks` key).
                if !hook_args.is_empty() {
                    arguments["hooks"] = serde_json::Value::Array(hook_args);
                }

                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 91,
                    "method": "tools/call",
                    "params": {
                        "name": tool_name,
                        "arguments": arguments
                    }
                });
                process.send(&request);
                std::thread::sleep(std::time::Duration::from_millis(3000));
                let response = process.read_response()?;

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("response", response.clone());
                out.set("persist_playbook_response", response);
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                out.set("mcp_persist_owner_home", owner_home);
                out.set("mcp_persist_owner_home_handle", handle);
                if let Some(before) = owner_before {
                    out.set("mcp_persist_owner_home_playbooks_before", before);
                }
                if let Some(before) = hearth_before {
                    out.set("mcp_persist_hearth_playbooks_before", before);
                }
                Ok(out)
            },
        ),
        check_def(
            "the persist_playbook response kind is {string}",
            &[("persist_playbook_response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected kind")?;
                let response = ctx
                    .get::<serde_json::Value>("persist_playbook_response")
                    .ok_or("No persist_playbook_response")?;
                if response["result"]["isError"].as_bool().unwrap_or(false) {
                    let text = response["result"]["content"][0]["text"]
                        .as_str()
                        .unwrap_or("(no text)");
                    return Err(format!("persist_playbook returned error: {}", text));
                }
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("No text in persist_playbook response")?;
                let parsed: serde_json::Value =
                    serde_json::from_str(text).map_err(|e| format!("Invalid JSON: {}", e))?;
                let actual = parsed["kind"].as_str().ok_or("No kind in response")?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("Expected kind '{}', got '{}'", expected, actual))
                }
            },
        ),
        check_def(
            "the persist_playbook response written_path is under the owner-home",
            &[
                ("persist_playbook_response", "JsonValue"),
                ("mcp_persist_owner_home", "PathBuf"),
            ],
            |ctx, _params| {
                let owner_home = ctx
                    .get::<PathBuf>("mcp_persist_owner_home")
                    .ok_or("No mcp_persist_owner_home")?;
                let response = ctx
                    .get::<serde_json::Value>("persist_playbook_response")
                    .ok_or("No persist_playbook_response")?;
                if response["result"]["isError"].as_bool().unwrap_or(false) {
                    let text = response["result"]["content"][0]["text"]
                        .as_str()
                        .unwrap_or("(no text)");
                    return Err(format!("persist_playbook returned error: {}", text));
                }
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("No text in persist_playbook response")?;
                let parsed: serde_json::Value =
                    serde_json::from_str(text).map_err(|e| format!("Invalid JSON: {}", e))?;
                let written_path = parsed["written_path"]
                    .as_str()
                    .ok_or("No written_path in response")?;
                if written_path.starts_with(&owner_home.to_string_lossy().into_owned()) {
                    Ok(())
                } else {
                    Err(format!(
                        "written_path '{}' is not under owner-home '{}'",
                        written_path,
                        owner_home.display()
                    ))
                }
            },
        ),
        check_def(
            "a machine.yaml exists at {string} under the MCP persist owner-home",
            &[("mcp_persist_owner_home", "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected rel path")?;
                let owner_home = ctx
                    .get::<PathBuf>("mcp_persist_owner_home")
                    .ok_or("No mcp_persist_owner_home")?;
                let path = owner_home.join(rel);
                if path.is_file() {
                    Ok(())
                } else {
                    Err(format!("Expected file at {}", path.display()))
                }
            },
        ),
        check_def(
            "no machine.yaml exists at {string} under the MCP persist owner-home",
            &[("mcp_persist_owner_home", "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected rel path")?;
                let owner_home = ctx
                    .get::<PathBuf>("mcp_persist_owner_home")
                    .ok_or("No mcp_persist_owner_home")?;
                let path = owner_home.join(rel);
                if path.exists() {
                    Err(format!("Unexpected file exists at {}", path.display()))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "no kind directory exists at {string} under the MCP persist owner-home",
            &[("mcp_persist_owner_home", "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected rel path")?;
                let owner_home = ctx
                    .get::<PathBuf>("mcp_persist_owner_home")
                    .ok_or("No mcp_persist_owner_home")?;
                let path = owner_home.join(rel);
                // Stronger than the narrower "no machine.yaml at <kind>/machine.yaml"
                // assertion: a refused persist must leave NOTHING behind for this
                // kind — not the machine.yaml, and not a leftover kind directory
                // holding a `hooks/` subtree or a stray `intent.md`. If the write
                // boundary ever created the kind dir before failing, `path` (e.g.
                // `playbooks/throwaway_kind`) would exist here and this check would
                // FAIL, where the machine.yaml-path assertion would still pass.
                // `exists()` returns true for a directory (even empty) or any file,
                // so it catches every leftover form.
                if path.exists() {
                    Err(format!(
                        "refused persist left a kind entry behind at {} (the entire kind directory must be absent)",
                        path.display()
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "a fresh registry from the MCP persist owner-home resolves kind {string}",
            &[("mcp_persist_owner_home", "PathBuf")],
            |ctx, params| {
                use anvil_core::domain::playbook::hearth_registry::HearthPlaybookRegistry;
                use anvil_core::domain::playbook::registry::PlaybookRegistry;
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let owner_home = ctx
                    .get::<PathBuf>("mcp_persist_owner_home")
                    .ok_or("No mcp_persist_owner_home")?;
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
        step_def(
            "the MCP persist owner-home playbooks entry count is recorded",
            &[
                ("mcp_persist_owner_home", "PathBuf"),
                ("mcp_persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                ("mcp_process", "McpProcess"),
                ("response", "JsonValue"),
                ("persist_playbook_response", "JsonValue"),
                ("hearth_path", "PathBuf"),
                ("mcp_persist_owner_home", "PathBuf"),
                ("mcp_persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("mcp_persist_owner_home_playbooks_before", "usize"),
            ],
            |mut ctx, _params| {
                let process = ctx
                    .take::<McpProcess>("mcp_process")
                    .ok_or("No mcp_process")?;
                let owner_home = ctx
                    .get::<PathBuf>("mcp_persist_owner_home")
                    .ok_or("No mcp_persist_owner_home")?
                    .clone();
                let handle = ctx
                    .get::<Arc<Mutex<Option<tempfile::TempDir>>>>(
                        "mcp_persist_owner_home_handle",
                    )
                    .ok_or("No mcp_persist_owner_home_handle")?
                    .clone();
                let count = count_dir_entries(&owner_home.join("playbooks"));
                let mut out = Context::new();
                out.set("mcp_process", process);
                if let Some(response) = ctx.get::<serde_json::Value>("response") {
                    out.set("response", response.clone());
                    out.set("persist_playbook_response", response.clone());
                }
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                out.set("mcp_persist_owner_home", owner_home);
                out.set("mcp_persist_owner_home_handle", handle);
                out.set("mcp_persist_owner_home_playbooks_before", count);
                Ok(out)
            },
        ),
        check_def(
            "the MCP persist owner-home playbooks entry count is unchanged",
            &[
                ("mcp_persist_owner_home", "PathBuf"),
                ("mcp_persist_owner_home_playbooks_before", "usize"),
            ],
            |ctx, _params| {
                let owner_home = ctx
                    .get::<PathBuf>("mcp_persist_owner_home")
                    .ok_or("No mcp_persist_owner_home")?;
                let before = ctx
                    .get::<usize>("mcp_persist_owner_home_playbooks_before")
                    .ok_or("No before count")?;
                let after = count_dir_entries(&owner_home.join("playbooks"));
                if after == *before {
                    Ok(())
                } else {
                    Err(format!(
                        "owner-home playbooks entry count changed: before={} after={}",
                        before, after
                    ))
                }
            },
        ),
        step_def(
            "the engine hearth playbooks entry count is recorded for MCP persist_playbook",
            &[("hearth_path", "PathBuf")],
            &[
                ("mcp_process", "McpProcess"),
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("mcp_persist_owner_home", "PathBuf"),
                ("mcp_persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("mcp_persist_hearth_playbooks_before", "usize"),
            ],
            |mut ctx, _params| {
                let process = ctx
                    .take::<McpProcess>("mcp_process")
                    .ok_or("No mcp_process")?;
                let engine_process = ctx.take::<EngineProcess>("engine_process");
                let hearth_path = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let owner_home = ctx
                    .get::<PathBuf>("mcp_persist_owner_home")
                    .ok_or("No mcp_persist_owner_home")?
                    .clone();
                let handle = ctx
                    .get::<Arc<Mutex<Option<tempfile::TempDir>>>>(
                        "mcp_persist_owner_home_handle",
                    )
                    .ok_or("No mcp_persist_owner_home_handle")?
                    .clone();
                let count = count_dir_entries(&hearth_path.join("playbooks"));
                let mut out = Context::new();
                out.set("mcp_process", process);
                if let Some(engine_process) = engine_process {
                    out.set("engine_process", engine_process);
                }
                out.set("hearth_path", hearth_path);
                out.set("mcp_persist_owner_home", owner_home);
                out.set("mcp_persist_owner_home_handle", handle);
                out.set("mcp_persist_hearth_playbooks_before", count);
                Ok(out)
            },
        ),
        check_def(
            "the engine hearth playbooks entry count is unchanged for MCP persist_playbook",
            &[
                ("hearth_path", "PathBuf"),
                ("mcp_persist_hearth_playbooks_before", "usize"),
            ],
            |ctx, _params| {
                let hearth_path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let before = ctx
                    .get::<usize>("mcp_persist_hearth_playbooks_before")
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

        // ===== Machine YAML live-reload: scratch hearth setup and edit steps =====

        // Creates a scratch hearth directory with:
        //   - A track artifact (20260422T0001_live_reload_track) in state "spec"
        //   - The real hearth's playbook files copied under workflows/20260422T0000_track_lifecycle/
        // The scratch hearth is a tmpdir — no restore needed, it is discarded after the test.
        // Outputs hearth_path (the scratch hearth) so subsequent .hearth file and shim steps work.
        step_def(
            "a scratch hearth with a track artifact in state {string} and the real playbook files",
            &[],
            &[("hearth_path", "PathBuf"), ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, params| {
                let _state = params.get_string(0).ok_or("Expected state")?.to_string();

                let (handle, tmp) = retained_temp_dir("anvil-live-reload-hearth-")?;

                // Locate the real hearth via the .hearth file in the workspace root.
                // The test binary runs from the workspace root (the cargo workspace dir).
                // Walk up from the exe path to find the workspace root.
                let real_hearth = {
                    // Try env var first (set by CI or explicit override), then walk up from exe.
                    std::env::var("ANVIL_HEARTH_PATH")
                        .map(PathBuf::from)
                        .unwrap_or_else(|_| {
                            // The binary lives at target/{debug,release}/…; walk up three levels
                            // to the workspace root and read .hearth.
                            let exe = std::env::current_exe().unwrap_or_default();
                            let workspace = exe
                                .ancestors()
                                .find(|p| p.join("Cargo.toml").exists())
                                .unwrap_or(exe.parent().unwrap_or(&exe))
                                .to_path_buf();
                            // Read .hearth file from the workspace root if present.
                            if let Ok(content) = std::fs::read_to_string(workspace.join(".hearth")) {
                                for line in content.lines() {
                                    let trimmed = line.trim();
                                    if let Some(rest) = trimmed.strip_prefix("path:") {
                                        let p = PathBuf::from(rest.trim());
                                        if p.is_absolute() {
                                            return p;
                                        }
                                        return workspace.join(p);
                                    }
                                }
                            }
                            // Fallback: sibling anvil-hearth directory.
                            workspace
                                .parent()
                                .unwrap_or(&workspace)
                                .join("anvil-hearth")
                        })
                };

                // Copy the real playbook machine.yaml into the scratch hearth.
                let workspace_root = PathBuf::from(anvil_test_support::TEST_SUPPORT_DIR)
                    .parent()
                    .ok_or("No parent of CARGO_MANIFEST_DIR")?
                    .to_path_buf();
                let candidates = [
                    real_hearth.join("playbooks").join("track_lifecycle"),
                    real_hearth.join("workflows").join("track_lifecycle"),
                    workspace_root.join("playbooks").join("track_lifecycle"),
                    workspace_root.join("workflows").join("track_lifecycle"),
                ];
                let src_playbook_dir = candidates
                    .iter()
                    .find(|path| path.join("machine.yaml").is_file())
                    .cloned()
                    .ok_or_else(|| {
                        format!(
                            "Failed to find track_lifecycle playbook in candidates: {:?}",
                            candidates
                        )
                    })?;
                let dst_playbook_dir = tmp.join("playbooks").join("20260422T0000_track_lifecycle");
                std::fs::create_dir_all(&dst_playbook_dir)
                    .map_err(|e| format!("Failed to create playbook dir: {}", e))?;
                for entry in std::fs::read_dir(&src_playbook_dir)
                    .map_err(|e| format!("Failed to read playbook dir: {}", e))?
                {
                    let entry = entry.map_err(|e| format!("Dir entry error: {}", e))?;
                    if entry.path().is_file() {
                        let dest = dst_playbook_dir.join(entry.file_name());
                        std::fs::copy(entry.path(), &dest)
                            .map_err(|e| format!("Failed to copy {}: {}", entry.path().display(), e))?;
                    }
                }

                // Copy the hooks/ subdirectory too: the real machine.yaml now
                // declares (spec, doer) / (spec_review, reviewer) hooks (P5), and
                // the loader rejects a machine.yaml whose hook references are not
                // present in hooks/. Without the bodies, the HearthPlaybookRegistry
                // would drop the machine and describe would silently fall back to
                // the compiled seed, defeating the live-reload proof.
                let src_hooks_dir = src_playbook_dir.join("hooks");
                if src_hooks_dir.is_dir() {
                    let dst_hooks_dir = dst_playbook_dir.join("hooks");
                    std::fs::create_dir_all(&dst_hooks_dir)
                        .map_err(|e| format!("Failed to create hooks dir: {}", e))?;
                    for entry in std::fs::read_dir(&src_hooks_dir)
                        .map_err(|e| format!("Failed to read hooks dir: {}", e))?
                    {
                        let entry = entry.map_err(|e| format!("Hooks entry error: {}", e))?;
                        if entry.path().is_file() {
                            std::fs::copy(entry.path(), dst_hooks_dir.join(entry.file_name()))
                                .map_err(|e| format!("Failed to copy hook: {}", e))?;
                        }
                    }
                }

                // Scaffold a minimal track artifact in spec state.
                let track_id = "20260422T0001_live_reload_track";
                let track_dir = tmp.join("tracks").join(track_id);
                std::fs::create_dir_all(&track_dir)
                    .map_err(|e| format!("Failed to create track dir: {}", e))?;

                let status_yaml = concat!(
                    "version: 1\n",
                    "kind: track\n",
                    "state: spec\n",
                    "proposal: 20260411T2021_anvil_workflow_engine\n",
                    "actors:\n",
                    "transitions:\n",
                    "  - to: spec\n",
                    "    at: 2026-04-22T00:00:00Z\n",
                    "    actor: Bootstrap-000000\n",
                    "    role: spec\n",
                    "    approver: mark\n",
                    "    note: scaffold\n",
                );
                std::fs::write(track_dir.join("status.yaml"), status_yaml)
                    .map_err(|e| format!("Failed to write status.yaml: {}", e))?;

                // A minimal spec.md so describe can load the artifact.
                std::fs::write(track_dir.join("spec.md"), "# Live Reload Track\n\nTest fixture.\n")
                    .map_err(|e| format!("Failed to write spec.md: {}", e))?;

                // A minimal tracks.md registry so this scratch hearth satisfies
                // the engine hearth predicate (tracks/ dir + tracks.md), required
                // now that shim-driven RPCs send a non-empty hearth_path and are
                // predicate-checked.
                std::fs::write(
                    tmp.join("tracks.md"),
                    "# Tracks\n\n## spec\n\n- [Live Reload Track](tracks/20260422T0001_live_reload_track/)\n",
                )
                .map_err(|e| format!("Failed to write tracks.md: {}", e))?;

                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),

        // Edits the machine.yaml in the scratch hearth to remove the spec→spec_review
        // transition block. The scratch hearth is a tmpdir; no restore is needed.
        // Outputs hearth_path and mcp_process (pass-through so subsequent steps keep them).
        step_def(
            "the scratch hearth machine.yaml has the spec-to-spec_review transition removed",
            &[("hearth_path", "PathBuf"), ("mcp_process", "McpProcess")],
            &[("hearth_path", "PathBuf"), ("mcp_process", "McpProcess")],
            |mut ctx, _params| {
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let mcp_process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                let machine_yaml_path = hearth
                    .join("playbooks")
                    .join("20260422T0000_track_lifecycle")
                    .join("machine.yaml");

                let content = std::fs::read_to_string(&machine_yaml_path)
                    .map_err(|e| format!("Failed to read machine.yaml: {}", e))?;

                // ADD a new `spec → plan` transition (additive edit) rather than
                // removing `spec → spec_review`. The track machine is a tightly
                // chained graph: removing any forward edge orphans its downstream
                // states, which the registration-time contiguity gate rejects —
                // dropping the whole machine and revealing the seed fallback.
                // An additive edit keeps the machine contiguous and registered
                // while changing describe(spec)'s available_actions, which is the
                // live-reload property this scenario proves. We splice the new
                // edge in directly after the existing spec→spec_review block.
                // The anchor must match the machine.yaml AS SERIALIZED by the
                // real playbook (this scratch hearth is a byte-copy of
                // playbooks/track_lifecycle/machine.yaml). That serialization
                // is a TOP-LEVEL transition list: `- ` at column 0, fields at
                // 2-space indent, `null` (not `~`), and a trailing `hook: null`.
                // The prior anchor used 2-space-indented `- ` + `~` and omitted
                // `hook`, so it drifted out of match when the file was
                // reformatted (2026-07-07 finding). Anchoring through the full
                // block (incl. `hook: null`) also lands `insert_at` cleanly
                // between complete transitions, not mid-block.
                let anchor = concat!(
                    "- from_state: spec\n",
                    "  to_state: spec_review\n",
                    "  required_role: spec\n",
                    "  required_satisfaction: null\n",
                    "  requires_approver: false\n",
                    "  hook: null\n",
                );
                let pos = content.find(anchor).ok_or_else(|| format!(
                    "Could not find spec→spec_review transition block in machine.yaml. Content snippet:\n{}",
                    &content[..content.len().min(500)]
                ))?;
                let insert_at = pos + anchor.len();
                let new_edge = concat!(
                    "- from_state: spec\n",
                    "  to_state: plan\n",
                    "  required_role: spec\n",
                    "  required_satisfaction: null\n",
                    "  requires_approver: false\n",
                    "  hook: null\n",
                );
                let edited = format!(
                    "{}{}{}",
                    &content[..insert_at],
                    new_edge,
                    &content[insert_at..]
                );
                std::fs::write(&machine_yaml_path, &edited)
                    .map_err(|e| format!("Failed to write edited machine.yaml: {}", e))?;

                let mut out = Context::new();
                out.set("hearth_path", hearth);
                out.set("mcp_process", mcp_process);
                Ok(out)
            },
        ),

        // Asserts that the most recent describe response's available_actions includes
        // an action with action == <name>.
        check_def(
            "the describe response includes action {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected action name")?.to_string();
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("Missing content text")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse describe response: {}", e))?;
                let actions = parsed["available_actions"]
                    .as_array()
                    .ok_or("Missing available_actions in describe response")?;
                let found = actions.iter().any(|a| a["action"].as_str() == Some(&expected));
                if found {
                    Ok(())
                } else {
                    Err(format!(
                        "Action '{}' not found in available_actions. Found: {:?}",
                        expected,
                        actions.iter().filter_map(|a| a["action"].as_str()).collect::<Vec<_>>()
                    ))
                }
            },
        ),

        // Asserts that the most recent describe response's available_actions does NOT include
        // an action with action == <name>. Used to verify live-reload removed the transition.
        check_def(
            "the describe response does not include action {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let excluded = params.get_string(0).ok_or("Expected action name")?.to_string();
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("Missing content text")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse describe response: {}", e))?;
                let actions = parsed["available_actions"]
                    .as_array()
                    .ok_or("Missing available_actions in describe response")?;
                let found = actions.iter().any(|a| a["action"].as_str() == Some(&excluded));
                if found {
                    Err(format!(
                        "Action '{}' unexpectedly found in available_actions after machine.yaml edit. Actions: {:?}",
                        excluded,
                        actions.iter().filter_map(|a| a["action"].as_str()).collect::<Vec<_>>()
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        // ===== JSON-RPC error code conformance steps =====
        step_def(
            "an unknown method request is sent",
            &[("mcp_process", "McpProcess")],
            &[("mcp_process", "McpProcess"), ("response", "JsonValue")],
            |mut ctx, _params| {
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 99,
                    "method": "unknown/method",
                    "params": {}
                });
                process.send(&request);
                let response = process.read_response()?;
                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("response", response);
                Ok(out)
            },
        ),
        step_def(
            "a tools/call request with no name field is sent",
            &[("mcp_process", "McpProcess")],
            &[("mcp_process", "McpProcess"), ("response", "JsonValue")],
            |mut ctx, _params| {
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 98,
                    "method": "tools/call",
                    "params": {
                        "arguments": {}
                    }
                });
                process.send(&request);
                let response = process.read_response()?;
                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("response", response);
                Ok(out)
            },
        ),
        check_def(
            "the response error code is {int}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected error code")?;
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let code = response["error"]["code"]
                    .as_i64()
                    .ok_or_else(|| format!(
                        "Missing error.code in response: {}",
                        serde_json::to_string_pretty(response).unwrap_or_default()
                    ))?;
                if code != expected {
                    return Err(format!("Expected error.code {}, got {}", expected, code));
                }
                Ok(())
            },
        ),
        check_def(
            "the response error data code is {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected data code")?;
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let code = response["error"]["data"]["code"]
                    .as_str()
                    .ok_or_else(|| format!(
                        "Missing error.data.code in response: {}",
                        serde_json::to_string_pretty(response).unwrap_or_default()
                    ))?;
                if code != expected {
                    return Err(format!("Expected error.data.code '{}', got '{}'", expected, code));
                }
                Ok(())
            },
        ),
        // ===== Kit-bundled playbook fixture step =====
        step_def(
            "the kit-bundled playbook is copied into the hearth tmpdir",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, _params| {
                let hearth_path = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();

                // Resolve dist/anvil-kit/playbooks/track_lifecycle relative to the
                // workspace root.  The manifest dir of anvil-test-support is
                // <workspace>/anvil-test-support, so the workspace root is one level up.
                let workspace_root = PathBuf::from(anvil_test_support::TEST_SUPPORT_DIR)
                    .parent()
                    .ok_or("No parent of CARGO_MANIFEST_DIR")?
                    .to_path_buf();
                let dist_src = workspace_root.join("dist/anvil-kit/playbooks/track_lifecycle");
                let source_src = workspace_root.join("playbooks/track_lifecycle");
                let src = if dist_src.join("machine.yaml").exists() {
                    dist_src
                } else {
                    source_src
                };

                if !src.join("machine.yaml").exists() {
                    return Err(format!(
                        "track_lifecycle machine.yaml not found at {}. \
                         Run the kit build script or keep source playbooks present.",
                        src.display()
                    ));
                }

                let dest = hearth_path.join("playbooks").join("track_lifecycle");
                std::fs::create_dir_all(&dest)
                    .map_err(|e| format!("Failed to create playbook dest dir: {}", e))?;

                for entry in std::fs::read_dir(&src)
                    .map_err(|e| format!("Failed to read src dir {}: {}", src.display(), e))?
                {
                    let entry = entry.map_err(|e| format!("Entry error: {}", e))?;
                    let ft = entry.file_type().map_err(|e| format!("file_type: {}", e))?;
                    if ft.is_file() {
                        let filename = entry.file_name();
                        std::fs::copy(entry.path(), dest.join(&filename))
                            .map_err(|e| format!("Failed to copy {:?}: {}", filename, e))?;
                    }
                }

                // Copy the hooks/ subdirectory so the engine can serve the
                // migrated (spec, doer) / (spec_review, reviewer) hook bodies
                // end-to-end (M1, hook_content_serving P5). The flat file copy
                // above skips subdirectories.
                let hooks_src = src.join("hooks");
                if hooks_src.is_dir() {
                    let hooks_dest = dest.join("hooks");
                    std::fs::create_dir_all(&hooks_dest)
                        .map_err(|e| format!("Failed to create hooks dest dir: {}", e))?;
                    for entry in std::fs::read_dir(&hooks_src)
                        .map_err(|e| format!("Failed to read hooks src: {}", e))?
                    {
                        let entry = entry.map_err(|e| format!("Entry error: {}", e))?;
                        if entry
                            .file_type()
                            .map_err(|e| format!("file_type: {}", e))?
                            .is_file()
                        {
                            let filename = entry.file_name();
                            std::fs::copy(entry.path(), hooks_dest.join(&filename))
                                .map_err(|e| format!("Failed to copy hook {:?}: {}", filename, e))?;
                        }
                    }
                }

                let mut out = Context::new();
                out.set("hearth_path", hearth_path);
                if let Some(handle) = ctx.get::<RetainedTempDir>("hearth_path_handle") {
                    out.set("hearth_path_handle", Arc::clone(handle));
                }
                Ok(out)
            },
        ),

        // ===== BP4: begin_adoption_status MCP tool steps =====

        /// Send a tools/call for "begin_adoption_status" and store the response.
        /// The table must have columns: actor_name, artifact_path, state.
        step_def(
            "a begin_adoption_status tools/call is sent with:",
            &[("mcp_process", "McpProcess")],
            &[
                ("mcp_process", "McpProcess"),
                ("begin_adoption_status_mcp_response", "JsonValue"),
                ("hearth_path", "PathBuf"),
            ],
            |mut ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let mut fields: std::collections::HashMap<String, String> =
                    std::collections::HashMap::new();
                // Support both header-row and body-row formats used by the step tables.
                if table.headers.len() >= 2 {
                    fields.insert(
                        table.headers[0].trim().to_string(),
                        table.headers[1].trim().to_string(),
                    );
                }
                for row in &table.rows {
                    if row.len() >= 2 {
                        fields.insert(row[0].trim().to_string(), row[1].trim().to_string());
                    }
                }
                let mut process = ctx
                    .take::<McpProcess>("mcp_process")
                    .ok_or("No mcp_process")?;

                let arguments = serde_json::json!({
                    "actor_name": fields.get("actor_name").cloned().unwrap_or_default(),
                    "artifact_path": fields.get("artifact_path").cloned().unwrap_or_default(),
                    "state": fields.get("state").cloned().unwrap_or_default(),
                });

                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 91,
                    "method": "tools/call",
                    "params": {
                        "name": "begin_adoption_status",
                        "arguments": arguments
                    }
                });

                process.send(&request);
                std::thread::sleep(std::time::Duration::from_millis(3000));
                let response = process.read_response()?;

                let mut out = Context::new();
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                out.set("mcp_process", process);
                out.set("begin_adoption_status_mcp_response", response);
                Ok(out)
            },
        ),

        check_def(
            "the begin_adoption_status response has_open_begin is true",
            &[("begin_adoption_status_mcp_response", "JsonValue")],
            |ctx, _params| {
                let response = ctx
                    .get::<serde_json::Value>("begin_adoption_status_mcp_response")
                    .ok_or("No begin_adoption_status_mcp_response")?;
                if response["result"]["isError"].as_bool().unwrap_or(false) {
                    let text = response["result"]["content"][0]["text"]
                        .as_str()
                        .unwrap_or("(no text)");
                    return Err(format!(
                        "begin_adoption_status returned tool error: {}",
                        text
                    ));
                }
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("Missing content text in begin_adoption_status response")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse begin_adoption_status response: {}", e))?;
                let has_open = parsed["has_open_begin"]
                    .as_bool()
                    .ok_or("Missing has_open_begin in response")?;
                if has_open {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected has_open_begin to be true, but it was false. Response: {}",
                        text
                    ))
                }
            },
        ),

        check_def(
            "the begin_adoption_status response has_open_begin is false",
            &[("begin_adoption_status_mcp_response", "JsonValue")],
            |ctx, _params| {
                let response = ctx
                    .get::<serde_json::Value>("begin_adoption_status_mcp_response")
                    .ok_or("No begin_adoption_status_mcp_response")?;
                if response["result"]["isError"].as_bool().unwrap_or(false) {
                    let text = response["result"]["content"][0]["text"]
                        .as_str()
                        .unwrap_or("(no text)");
                    return Err(format!(
                        "begin_adoption_status returned tool error: {}",
                        text
                    ));
                }
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("Missing content text in begin_adoption_status response")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse begin_adoption_status response: {}", e))?;
                let has_open = parsed["has_open_begin"]
                    .as_bool()
                    .ok_or("Missing has_open_begin in response")?;
                if !has_open {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected has_open_begin to be false, but it was true. Response: {}",
                        text
                    ))
                }
            },
        ),

        // ===== BP4: complete response warnings assertion =====

        check_def(
            "the complete response warnings contains {string}",
            &[("complete_response", "JsonValue")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?.to_string();
                let response = ctx
                    .get::<serde_json::Value>("complete_response")
                    .ok_or("No complete_response")?;
                if response["result"]["isError"].as_bool().unwrap_or(false) {
                    let text = response["result"]["content"][0]["text"]
                        .as_str()
                        .unwrap_or("(no text)");
                    return Err(format!("Complete tool returned error: {}", text));
                }
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("No text in complete response")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Invalid JSON in complete response: {}", e))?;
                let warnings = parsed["warnings"]
                    .as_array()
                    .ok_or_else(|| format!(
                        "No 'warnings' array in complete response. Response: {}",
                        text
                    ))?;
                let found = warnings
                    .iter()
                    .any(|w| w.as_str().map(|s| s.contains(&needle)).unwrap_or(false));
                if found {
                    Ok(())
                } else {
                    Err(format!(
                        "No warning containing '{}' found. Warnings: {:?}",
                        needle,
                        warnings.iter().filter_map(|w| w.as_str()).collect::<Vec<_>>()
                    ))
                }
            },
        ),
        step_def(
            "a {string} tools/call is sent with:",
            &[("mcp_process", "McpProcess")],
            &[
                ("mcp_process", "McpProcess"),
                ("response", "JsonValue"),
                ("hearth_path", "PathBuf"),
                ("work_dir", "PathBuf"),
                ("intake_instance_path", "String"),
                ("engine_process", "EngineProcess"),
                ("mcp_persist_owner_home", "PathBuf"),
                ("mcp_persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("mcp_persist_owner_home_playbooks_before", "usize"),
                ("mcp_persist_hearth_playbooks_before", "usize"),
            ],
            |mut ctx, params| {
                let tool_name = params.get_string(0).ok_or("Expected tool name")?.to_string();
                if tool_name != "candidate_playbook_intake" && tool_name != CANDIDATE_PLAYBOOK_INTAKE {
                    return Err(format!("Unexpected candidate intake tool name '{}'", tool_name));
                }
                let table = params.data_table().ok_or("Expected data table")?;
                let field_col = column_index(table, "field")?;
                let value_col = column_index(table, "value")?;
                let mut fields: std::collections::HashMap<String, String> =
                    std::collections::HashMap::new();
                for row in &table.rows {
                    fields.insert(row[field_col].trim().to_string(), row[value_col].trim().to_string());
                }
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                let engine_process = ctx.take::<EngineProcess>("engine_process");
                let target_owner = match fields.get("target_owner").map(String::as_str) {
                    Some("<mcp_persist_owner_home>") => ctx
                        .get::<PathBuf>("mcp_persist_owner_home")
                        .ok_or("No mcp_persist_owner_home for target_owner placeholder")?
                        .to_string_lossy()
                        .into_owned(),
                    Some("<whitespace_padded_tmp_owner_home>") => {
                        " /tmp/anvil-candidate-owner".to_string()
                    }
                    Some(value) => value.to_string(),
                    None => String::new(),
                };

                let evidence: Vec<String> = fields
                    .get("evidence")
                    .map(|value| {
                        value
                            .split(',')
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(ToString::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                let proposed_state_intent = fields
                    .get("proposed_state_intent")
                    .cloned()
                    .unwrap_or_else(|| "Sort the escalation into the right support lane.".to_string());
                let proposed_expected_output = fields
                    .get("proposed_expected_output")
                    .cloned()
                    .unwrap_or_else(|| "A triage note naming the lane and next action.".to_string());
                let intent = fields.get("intent").cloned().unwrap_or_default();

                let arguments = serde_json::json!({
                    "source": fields.get("source").cloned().unwrap_or_default(),
                    "intent": intent.clone(),
                    "at": fields.get("at").cloned().unwrap_or_default(),
                    "evidence": evidence,
                    "route_description": fields
                        .get("route_description")
                        .cloned()
                        .unwrap_or_else(|| default_route_description(&intent)),
                    "route_triggers": route_triggers_from_fields(&fields, &intent),
                    "projection_targets": projection_targets_from_fields(&fields),
                    "proposed_states": [
                        {
                            "state": "triage",
                            "role": "doer",
                            "intent": proposed_state_intent,
                            "expected_output": proposed_expected_output
                        }
                    ],
                    "target_owner": target_owner,
                    "parent_id": fields.get("parent_id").cloned().unwrap_or_default(),
                    "approver": fields.get("approver").cloned().unwrap_or_else(|| "lore".to_string()),
                    "actor_name": fields.get("actor_name").cloned().unwrap_or_default(),
                    "actor_type": fields.get("actor_type").cloned().unwrap_or_default(),
                    "actor_model": fields.get("actor_model").cloned().unwrap_or_default(),
                    "actor_provider": fields.get("actor_provider").cloned().unwrap_or_default(),
                });
                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 72,
                    "method": "tools/call",
                    "params": {
                        "name": tool_name,
                        "arguments": arguments
                    }
                });

                process.send(&request);
                std::thread::sleep(std::time::Duration::from_millis(3000));
                let response = process.read_response()?;
                if response["result"]["isError"].as_bool().unwrap_or(false) {
                    let mut out = Context::new();
                    if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                        out.set("hearth_path", hp.clone());
                    }
                    if let Some(work_dir) = ctx.get::<PathBuf>("work_dir") {
                        out.set("work_dir", work_dir.clone());
                    }
                    out.set("mcp_process", process);
                    out.set("response", response);
                    if let Some(engine_process) = engine_process {
                        out.set("engine_process", engine_process);
                    }
                    return Ok(out);
                }
                let text = response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or_else(|| format!("No result content text in response: {}", response))?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse intake response JSON: {}", e))?;
                let instance_id = parsed["instance_id"]
                    .as_str()
                    .ok_or_else(|| format!("No instance_id in intake response: {}", parsed))?;

                let mut out = Context::new();
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                if let Some(work_dir) = ctx.get::<PathBuf>("work_dir") {
                    out.set("work_dir", work_dir.clone());
                }
                out.set("mcp_process", process);
                out.set("response", response);
                out.set(
                    "intake_instance_path",
                    format!("workflow_generations/{}", instance_id),
                );
                if let Some(engine_process) = engine_process {
                    out.set("engine_process", engine_process);
                }
                if let Some(owner_home) = ctx.get::<PathBuf>("mcp_persist_owner_home") {
                    out.set("mcp_persist_owner_home", owner_home.clone());
                }
                if let Some(handle) =
                    ctx.get::<Arc<Mutex<Option<tempfile::TempDir>>>>("mcp_persist_owner_home_handle")
                {
                    out.set("mcp_persist_owner_home_handle", handle.clone());
                }
                if let Some(before) = ctx.get::<usize>("mcp_persist_owner_home_playbooks_before") {
                    out.set("mcp_persist_owner_home_playbooks_before", *before);
                }
                if let Some(before) = ctx.get::<usize>("mcp_persist_hearth_playbooks_before") {
                    out.set("mcp_persist_hearth_playbooks_before", *before);
                }
                Ok(out)
            },
        ),
        step_def(
            "a candidate_playbook_intake tools/call is sent with candidate fields and proposed states:",
            &[("mcp_process", "McpProcess")],
            &[
                ("mcp_process", "McpProcess"),
                ("response", "JsonValue"),
                ("hearth_path", "PathBuf"),
                ("work_dir", "PathBuf"),
                ("intake_instance_path", "String"),
                ("engine_process", "EngineProcess"),
                ("mcp_persist_owner_home", "PathBuf"),
                ("mcp_persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("mcp_persist_owner_home_playbooks_before", "usize"),
                ("mcp_persist_hearth_playbooks_before", "usize"),
            ],
            |mut ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let field_col = column_index(table, "field")?;
                let value_col = column_index(table, "value")?;
                let state_col = column_index(table, "state")?;
                let role_col = column_index(table, "role")?;
                let intent_col = column_index(table, "intent")?;
                let output_col = column_index(table, "expected_output")?;

                let mut fields: std::collections::HashMap<String, String> =
                    std::collections::HashMap::new();
                let mut proposed_states = Vec::new();
                for row in &table.rows {
                    let field = row.get(field_col).map(|s| s.trim()).unwrap_or("");
                    if !field.is_empty() {
                        fields.insert(
                            field.to_string(),
                            row.get(value_col)
                                .map(|s| s.trim().to_string())
                                .unwrap_or_default(),
                        );
                    }

                    let state = row.get(state_col).map(|s| s.trim()).unwrap_or("");
                    if !state.is_empty() {
                        proposed_states.push(serde_json::json!({
                            "state": state,
                            "role": row.get(role_col).map(|s| s.trim()).unwrap_or(""),
                            "intent": row.get(intent_col).map(|s| s.trim()).unwrap_or(""),
                            "expected_output": row.get(output_col).map(|s| s.trim()).unwrap_or("")
                        }));
                    }
                }
                if proposed_states.is_empty() {
                    return Err("Expected at least one proposed state row".to_string());
                }

                if matches!(
                    fields.get("target_owner").map(String::as_str),
                    Some("<mcp_persist_owner_home>")
                ) {
                    let owner_home = ctx
                        .get::<PathBuf>("mcp_persist_owner_home")
                        .ok_or("No mcp_persist_owner_home for target_owner placeholder")?
                        .to_string_lossy()
                        .into_owned();
                    fields.insert("target_owner".to_string(), owner_home);
                }

                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                let engine_process = ctx.take::<EngineProcess>("engine_process");
                let request = intake_candidate_playbook_request(
                    &fields,
                    "at",
                    serde_json::Value::Array(proposed_states),
                )?;
                process.send(&request);
                std::thread::sleep(std::time::Duration::from_millis(3000));
                let response = process.read_response()?;
                if response["result"]["isError"].as_bool().unwrap_or(false) {
                    let text = response["result"]["content"][0]["text"].as_str().unwrap_or("");
                    return Err(format!("candidate_playbook_intake returned tool error: {}", text));
                }
                let parsed = parse_intake_tool_response(&response)?;
                let instance_id = parsed["instance_id"]
                    .as_str()
                    .ok_or_else(|| format!("No instance_id in intake response: {}", parsed))?;

                let mut out = Context::new();
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                if let Some(work_dir) = ctx.get::<PathBuf>("work_dir") {
                    out.set("work_dir", work_dir.clone());
                }
                out.set("mcp_process", process);
                out.set("response", response);
                out.set(
                    "intake_instance_path",
                    format!("workflow_generations/{}", instance_id),
                );
                if let Some(engine_process) = engine_process {
                    out.set("engine_process", engine_process);
                }
                if let Some(owner_home) = ctx.get::<PathBuf>("mcp_persist_owner_home") {
                    out.set("mcp_persist_owner_home", owner_home.clone());
                }
                if let Some(handle) =
                    ctx.get::<Arc<Mutex<Option<tempfile::TempDir>>>>("mcp_persist_owner_home_handle")
                {
                    out.set("mcp_persist_owner_home_handle", handle.clone());
                }
                if let Some(before) = ctx.get::<usize>("mcp_persist_owner_home_playbooks_before") {
                    out.set("mcp_persist_owner_home_playbooks_before", *before);
                }
                if let Some(before) = ctx.get::<usize>("mcp_persist_hearth_playbooks_before") {
                    out.set("mcp_persist_hearth_playbooks_before", *before);
                }
                Ok(out)
            },
        ),
        step_def(
            "two candidate_playbook_intake tools/calls with the same intent are sent with:",
            &[("mcp_process", "McpProcess")],
            &[
                ("mcp_process", "McpProcess"),
                ("response", "JsonValue"),
                ("hearth_path", "PathBuf"),
                ("intake_instance_ids", "Vec<String>"),
                ("intake_instance_paths", "Vec<String>"),
                ("intake_playbook_names", "Vec<String>"),
            ],
            |mut ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let field_col = column_index(table, "field")?;
                let value_col = column_index(table, "value")?;
                let mut fields: std::collections::HashMap<String, String> =
                    std::collections::HashMap::new();
                for row in &table.rows {
                    fields.insert(row[field_col].trim().to_string(), row[value_col].trim().to_string());
                }
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;

                let mut instance_ids = Vec::new();
                let mut playbook_names = Vec::new();
                let mut last_response = serde_json::Value::Null;
                for at_field in ["first_at", "second_at"] {
                    let request = intake_candidate_playbook_request(&fields, at_field, valid_proposed_states_json())?;
                    process.send(&request);
                    std::thread::sleep(std::time::Duration::from_millis(3000));
                    let response = process.read_response()?;
                    if response["result"]["isError"].as_bool().unwrap_or(false) {
                        let text = response["result"]["content"][0]["text"].as_str().unwrap_or("");
                        return Err(format!("candidate_playbook_intake returned tool error: {}", text));
                    }
                    let parsed = parse_intake_tool_response(&response)?;
                    instance_ids.push(
                        parsed["instance_id"]
                            .as_str()
                            .ok_or_else(|| format!("No instance_id in intake response: {}", parsed))?
                            .to_string(),
                    );
                    playbook_names.push(
                        parsed["playbook_name"]
                            .as_str()
                            .ok_or_else(|| format!("No playbook_name in intake response: {}", parsed))?
                            .to_string(),
                    );
                    last_response = response;
                }

                let instance_paths = instance_ids
                    .iter()
                    .map(|id| format!("workflow_generations/{}", id))
                    .collect::<Vec<_>>();

                let mut out = Context::new();
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                out.set("mcp_process", process);
                out.set("response", last_response);
                out.set("intake_instance_ids", instance_ids);
                out.set("intake_instance_paths", instance_paths);
                out.set("intake_playbook_names", playbook_names);
                Ok(out)
            },
        ),
        step_def(
            "a candidate_playbook_intake tools/call is sent with proposed state field {string} blank",
            &[("mcp_process", "McpProcess")],
            &[("mcp_process", "McpProcess"), ("response", "JsonValue"), ("hearth_path", "PathBuf")],
            |mut ctx, params| {
                let blank_field = params.get_string(0).ok_or("Expected field")?;
                let mut fields = std::collections::HashMap::new();
                fields.insert("source".to_string(), "lore".to_string());
                fields.insert("intent".to_string(), "Review support escalations".to_string());
                fields.insert("at".to_string(), "2026-06-08T00:00:00Z".to_string());
                fields.insert("evidence".to_string(), "obs-1,obs-2".to_string());
                fields.insert(
                    "route_description".to_string(),
                    default_route_description("Review support escalations"),
                );
                fields.insert(
                    "route_triggers".to_string(),
                    "review support playbook,author escalation playbook".to_string(),
                );
                fields.insert("projection_targets".to_string(), "workflows.md".to_string());
                fields.insert("target_owner".to_string(), "/tmp/anvil-candidate-owner".to_string());
                fields.insert("parent_id".to_string(), "20260606T0000_builder_parent_track".to_string());
                fields.insert("actor_name".to_string(), "Intake-Actor-100002".to_string());
                fields.insert("actor_type".to_string(), "agent".to_string());
                fields.insert("actor_model".to_string(), "gpt-5-codex".to_string());
                fields.insert("actor_provider".to_string(), "openai".to_string());

                let mut proposed_state = serde_json::json!({
                    "state": "triage",
                    "role": "doer",
                    "intent": "Sort the escalation into the right support lane.",
                    "expected_output": "A triage note naming the lane and next action."
                });
                proposed_state[blank_field] = serde_json::Value::String(String::new());

                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                let request = intake_candidate_playbook_request(
                    &fields,
                    "at",
                    serde_json::Value::Array(vec![proposed_state]),
                )?;
                process.send(&request);
                std::thread::sleep(std::time::Duration::from_millis(3000));
                let response = process.read_response()?;

                let mut out = Context::new();
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                out.set("mcp_process", process);
                out.set("response", response);
                Ok(out)
            },
        ),
        step_def(
            "a candidate_playbook_intake tools/call is sent with candidate field {string} {string}",
            &[("mcp_process", "McpProcess")],
            &[("mcp_process", "McpProcess"), ("response", "JsonValue"), ("hearth_path", "PathBuf")],
            |mut ctx, params| {
                let candidate_field = params.get_string(0).ok_or("Expected candidate field")?;
                let mode = params.get_string(1).ok_or("Expected field mode")?;
                if mode != "blank" && mode != "missing" {
                    return Err(format!("Expected field mode blank or missing, got {}", mode));
                }

                let mut arguments = serde_json::json!({
                    "source": "lore",
                    "intent": "Review support escalations",
                    "at": "2026-06-08T00:00:00Z",
                    "evidence": ["obs-1", "obs-2"],
                    "route_description": default_route_description("Review support escalations"),
                    "route_triggers": ["review support playbook", "author escalation playbook"],
                    "projection_targets": ["workflows.md"],
                    "proposed_states": valid_proposed_states_json(),
                    "target_owner": "/tmp/anvil-candidate-owner",
                    "parent_id": "20260606T0000_builder_parent_track",
                    "approver": "lore",
                    "actor_name": "Intake-Actor-100003",
                    "actor_type": "agent",
                    "actor_model": "gpt-5-codex",
                    "actor_provider": "openai",
                });

                match mode {
                    "blank" => {
                        arguments[candidate_field] = serde_json::Value::String(String::new());
                    }
                    "missing" => {
                        arguments
                            .as_object_mut()
                            .ok_or("candidate intake arguments are not an object")?
                            .remove(candidate_field);
                    }
                    _ => unreachable!(),
                }

                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 72,
                    "method": "tools/call",
                    "params": {
                        "name": CANDIDATE_PLAYBOOK_INTAKE,
                        "arguments": arguments
                    }
                });

                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                process.send(&request);
                std::thread::sleep(std::time::Duration::from_millis(3000));
                let response = process.read_response()?;

                let mut out = Context::new();
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                out.set("mcp_process", process);
                out.set("response", response);
                Ok(out)
            },
        ),
        check_def(
            "both candidate_playbook_intake responses have playbook_name {string}",
            &[("intake_playbook_names", "Vec<String>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected playbook_name")?;
                let names = ctx
                    .get::<Vec<String>>("intake_playbook_names")
                    .ok_or("No intake_playbook_names")?;
                if names.len() != 2 {
                    return Err(format!("Expected 2 playbook names, got {:?}", names));
                }
                if names.iter().all(|name| name == expected) {
                    Ok(())
                } else {
                    Err(format!("Expected both playbook names '{}', got {:?}", expected, names))
                }
            },
        ),
        check_def(
            "the candidate_playbook_intake responses have distinct instance ids",
            &[("intake_instance_ids", "Vec<String>")],
            |ctx, _params| {
                let ids = ctx
                    .get::<Vec<String>>("intake_instance_ids")
                    .ok_or("No intake_instance_ids")?;
                if ids.len() == 2 && ids[0] != ids[1] {
                    Ok(())
                } else {
                    Err(format!("Expected two distinct instance ids, got {:?}", ids))
                }
            },
        ),
        check_def(
            "both candidate_playbook_intake instances exist under the hearth",
            &[("hearth_path", "PathBuf"), ("intake_instance_paths", "Vec<String>")],
            |ctx, _params| {
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let paths = ctx
                    .get::<Vec<String>>("intake_instance_paths")
                    .ok_or("No intake_instance_paths")?;
                if paths.len() != 2 {
                    return Err(format!("Expected 2 intake instance paths, got {:?}", paths));
                }
                for path in paths {
                    let status_path = hearth.join(path).join("status.yaml");
                    if !status_path.exists() {
                        return Err(format!("Missing intake instance status at {}", status_path.display()));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "the candidate_playbook_intake response has kind {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected kind")?;
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let text = response["result"]["content"][0]["text"].as_str()
                    .ok_or("No text in response")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse response JSON: {}", e))?;
                let kind = parsed["kind"].as_str().ok_or("No kind in response")?;
                if kind == expected {
                    Ok(())
                } else {
                    Err(format!("Expected kind '{}', got '{}'", expected, kind))
                }
            },
        ),
        check_def(
            "the candidate_playbook_intake response has playbook_name {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected playbook_name")?;
                let response = ctx.get::<serde_json::Value>("response").ok_or("No response")?;
                let text = response["result"]["content"][0]["text"].as_str()
                    .ok_or("No text in response")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("Failed to parse response JSON: {}", e))?;
                let playbook_name = parsed["playbook_name"]
                    .as_str()
                    .ok_or("No playbook_name in response")?;
                if playbook_name == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected playbook_name '{}', got '{}'",
                        expected, playbook_name
                    ))
                }
            },
        ),
        step_def(
            "the intake builder is driven to completed through the MCP shim",
            &[("mcp_process", "McpProcess"), ("intake_instance_path", "String")],
            &[
                ("mcp_process", "McpProcess"),
                ("complete_response", "JsonValue"),
                ("response", "JsonValue"),
                ("hearth_path", "PathBuf"),
                ("work_dir", "PathBuf"),
                ("intake_instance_path", "String"),
                ("engine_process", "EngineProcess"),
                ("mcp_persist_owner_home", "PathBuf"),
                ("mcp_persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("mcp_persist_owner_home_playbooks_before", "usize"),
                ("mcp_persist_hearth_playbooks_before", "usize"),
            ],
            |mut ctx, _params| {
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                let artifact_path = ctx
                    .get::<String>("intake_instance_path")
                    .ok_or("No intake_instance_path")?
                    .clone();
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

                let mut last_response = serde_json::Value::Null;
                for (idx, (satisfaction, expected_state)) in hops.iter().enumerate() {
                    let arguments = serde_json::json!({
                        "artifact_path": artifact_path,
                        "actor_name": "Intake-Builder-Driver-100000",
                        "actor_type": "agent",
                        "actor_model": "gpt-5-codex",
                        "actor_provider": "openai",
                        "satisfaction": *satisfaction,
                        "approver": "lore",
                        "note": "",
                        "reflection_notes": ""
                    });
                    let request = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": 1090 + idx,
                        "method": "tools/call",
                        "params": {
                            "name": "complete",
                            "arguments": arguments
                        }
                    });
                    process.send(&request);
                    std::thread::sleep(std::time::Duration::from_millis(3000));
                    let response = process.read_response()?;
                    let is_error = response["result"]["isError"].as_bool().unwrap_or(false);
                    if is_error {
                        last_response = response;
                        if *expected_state == "completed" {
                            break;
                        }
                        let text = last_response["result"]["content"][0]["text"]
                            .as_str()
                            .unwrap_or("(no text)");
                        return Err(format!(
                            "complete tools/call failed before terminal hop (expected '{}'): {}",
                            expected_state, text
                        ));
                    }

                    let text = response["result"]["content"][0]["text"]
                        .as_str()
                        .ok_or_else(|| format!("No content text in complete response: {}", response))?;
                    let parsed: serde_json::Value = serde_json::from_str(text)
                        .map_err(|e| format!("Invalid complete response JSON: {}", e))?;
                    let new_state = parsed["new_state"]
                        .as_str()
                        .ok_or_else(|| format!("No new_state in complete response: {}", parsed))?;
                    if new_state != *expected_state {
                        return Err(format!(
                            "complete hop expected new_state '{}', got '{}'. Response: {}",
                            expected_state, new_state, text
                        ));
                    }
                    last_response = response;
                }

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("complete_response", last_response.clone());
                out.set("response", last_response);
                out.set("intake_instance_path", artifact_path);
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                if let Some(work_dir) = ctx.get::<PathBuf>("work_dir") {
                    out.set("work_dir", work_dir.clone());
                }
                if let Some(engine_process) = ctx.take::<EngineProcess>("engine_process") {
                    out.set("engine_process", engine_process);
                }
                if let Some(owner_home) = ctx.get::<PathBuf>("mcp_persist_owner_home") {
                    out.set("mcp_persist_owner_home", owner_home.clone());
                }
                if let Some(handle) =
                    ctx.get::<Arc<Mutex<Option<tempfile::TempDir>>>>("mcp_persist_owner_home_handle")
                {
                    out.set("mcp_persist_owner_home_handle", handle.clone());
                }
                if let Some(before) = ctx.get::<usize>("mcp_persist_owner_home_playbooks_before") {
                    out.set("mcp_persist_owner_home_playbooks_before", *before);
                }
                if let Some(before) = ctx.get::<usize>("mcp_persist_hearth_playbooks_before") {
                    out.set("mcp_persist_hearth_playbooks_before", *before);
                }
                Ok(out)
            },
        ),
        step_def(
            "the intake builder is driven to the terminal-ready state through the MCP shim",
            &[("mcp_process", "McpProcess"), ("intake_instance_path", "String")],
            &[
                ("mcp_process", "McpProcess"),
                ("complete_response", "JsonValue"),
                ("response", "JsonValue"),
                ("hearth_path", "PathBuf"),
                ("work_dir", "PathBuf"),
                ("intake_instance_path", "String"),
                ("engine_process", "EngineProcess"),
                ("mcp_persist_owner_home", "PathBuf"),
                ("mcp_persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("mcp_persist_owner_home_playbooks_before", "usize"),
                ("mcp_persist_hearth_playbooks_before", "usize"),
            ],
            |mut ctx, _params| {
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                let artifact_path = ctx
                    .get::<String>("intake_instance_path")
                    .ok_or("No intake_instance_path")?
                    .clone();
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
                ];

                let mut last_response = serde_json::Value::Null;
                for (idx, (satisfaction, expected_state)) in hops.iter().enumerate() {
                    let arguments = serde_json::json!({
                        "artifact_path": artifact_path,
                        "actor_name": "Intake-Builder-Driver-100000",
                        "actor_type": "agent",
                        "actor_model": "gpt-5-codex",
                        "actor_provider": "openai",
                        "satisfaction": *satisfaction,
                        "approver": "lore",
                        "note": "",
                        "reflection_notes": ""
                    });
                    let request = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": 1190 + idx,
                        "method": "tools/call",
                        "params": {
                            "name": "complete",
                            "arguments": arguments
                        }
                    });
                    process.send(&request);
                    std::thread::sleep(std::time::Duration::from_millis(3000));
                    let response = process.read_response()?;
                    if response["result"]["isError"].as_bool().unwrap_or(false) {
                        let text = response["result"]["content"][0]["text"]
                            .as_str()
                            .unwrap_or("(no text)");
                        return Err(format!(
                            "complete tools/call failed before terminal-ready state (expected '{}'): {}",
                            expected_state, text
                        ));
                    }

                    let text = response["result"]["content"][0]["text"]
                        .as_str()
                        .ok_or_else(|| format!("No content text in complete response: {}", response))?;
                    let parsed: serde_json::Value = serde_json::from_str(text)
                        .map_err(|e| format!("Invalid complete response JSON: {}", e))?;
                    let new_state = parsed["new_state"]
                        .as_str()
                        .ok_or_else(|| format!("No new_state in complete response: {}", parsed))?;
                    if new_state != *expected_state {
                        return Err(format!(
                            "complete hop expected new_state '{}', got '{}'. Response: {}",
                            expected_state, new_state, text
                        ));
                    }
                    last_response = response;
                }

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("complete_response", last_response.clone());
                out.set("response", last_response);
                out.set("intake_instance_path", artifact_path);
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                if let Some(work_dir) = ctx.get::<PathBuf>("work_dir") {
                    out.set("work_dir", work_dir.clone());
                }
                if let Some(engine_process) = ctx.take::<EngineProcess>("engine_process") {
                    out.set("engine_process", engine_process);
                }
                if let Some(owner_home) = ctx.get::<PathBuf>("mcp_persist_owner_home") {
                    out.set("mcp_persist_owner_home", owner_home.clone());
                }
                if let Some(handle) =
                    ctx.get::<Arc<Mutex<Option<tempfile::TempDir>>>>("mcp_persist_owner_home_handle")
                {
                    out.set("mcp_persist_owner_home_handle", handle.clone());
                }
                if let Some(before) = ctx.get::<usize>("mcp_persist_owner_home_playbooks_before") {
                    out.set("mcp_persist_owner_home_playbooks_before", *before);
                }
                if let Some(before) = ctx.get::<usize>("mcp_persist_hearth_playbooks_before") {
                    out.set("mcp_persist_hearth_playbooks_before", *before);
                }
                Ok(out)
            },
        ),
        step_def(
            "the intake builder artifact directory is made non-writable",
            &[("hearth_path", "PathBuf"), ("intake_instance_path", "String")],
            &[
                ("mcp_process", "McpProcess"),
                ("complete_response", "JsonValue"),
                ("response", "JsonValue"),
                ("hearth_path", "PathBuf"),
                ("work_dir", "PathBuf"),
                ("intake_instance_path", "String"),
                ("engine_process", "EngineProcess"),
                ("mcp_persist_owner_home", "PathBuf"),
                ("mcp_persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |mut ctx, _params| {
                let process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let artifact_path = ctx
                    .get::<String>("intake_instance_path")
                    .ok_or("No intake_instance_path")?
                    .clone();
                let artifact_dir = hearth.join(&artifact_path);
                // The transition log is now one-file-per-event under
                // `<artifact>/transitions/`, so the terminal complete's write
                // targets that subdir, not status.yaml. To exercise the
                // write-failure path (the scenario's intent), make the EXISTING
                // transitions/ subdir non-writable too — otherwise the event
                // write would succeed inside a writable child even with the
                // parent dir locked.
                let lock_non_writable = |dir: &std::path::Path| -> Result<(), String> {
                    let metadata = std::fs::metadata(dir)
                        .map_err(|e| format!("metadata {}: {}", dir.display(), e))?;
                    let mut permissions = metadata.permissions();
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        permissions.set_mode(0o500);
                        std::fs::set_permissions(dir, permissions)
                            .map_err(|e| format!("chmod non-writable {}: {}", dir.display(), e))
                    }
                    #[cfg(not(unix))]
                    {
                        permissions.set_readonly(true);
                        std::fs::set_permissions(dir, permissions)
                            .map_err(|e| format!("mark non-writable {}: {}", dir.display(), e))
                    }
                };
                // Lock the transitions/ subdir first (while the parent is still
                // writable), then the artifact dir itself.
                let transitions_dir = artifact_dir.join("transitions");
                if transitions_dir.exists() {
                    lock_non_writable(&transitions_dir)?;
                }
                lock_non_writable(&artifact_dir)?;

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("hearth_path", hearth);
                out.set("intake_instance_path", artifact_path);
                if let Some(response) = ctx.get::<serde_json::Value>("complete_response") {
                    out.set("complete_response", response.clone());
                    out.set("response", response.clone());
                }
                if let Some(work_dir) = ctx.get::<PathBuf>("work_dir") {
                    out.set("work_dir", work_dir.clone());
                }
                if let Some(engine_process) = ctx.take::<EngineProcess>("engine_process") {
                    out.set("engine_process", engine_process);
                }
                if let Some(owner_home) = ctx.get::<PathBuf>("mcp_persist_owner_home") {
                    out.set("mcp_persist_owner_home", owner_home.clone());
                }
                if let Some(handle) =
                    ctx.get::<Arc<Mutex<Option<tempfile::TempDir>>>>("mcp_persist_owner_home_handle")
                {
                    out.set("mcp_persist_owner_home_handle", handle.clone());
                }
                Ok(out)
            },
        ),
        step_def(
            "the intake builder artifact directory writability is restored",
            &[("hearth_path", "PathBuf"), ("intake_instance_path", "String")],
            &[
                ("mcp_process", "McpProcess"),
                ("complete_response", "JsonValue"),
                ("response", "JsonValue"),
                ("hearth_path", "PathBuf"),
                ("work_dir", "PathBuf"),
                ("intake_instance_path", "String"),
                ("engine_process", "EngineProcess"),
                ("mcp_persist_owner_home", "PathBuf"),
                ("mcp_persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("mcp_persist_machine_yaml_bytes", "Vec<u8>"),
            ],
            |mut ctx, _params| {
                let process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let artifact_path = ctx
                    .get::<String>("intake_instance_path")
                    .ok_or("No intake_instance_path")?
                    .clone();
                let artifact_dir = hearth.join(&artifact_path);
                let restore_writable = |dir: &std::path::Path| -> Result<(), String> {
                    let metadata = std::fs::metadata(dir)
                        .map_err(|e| format!("metadata {}: {}", dir.display(), e))?;
                    let mut permissions = metadata.permissions();
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        permissions.set_mode(0o700);
                        std::fs::set_permissions(dir, permissions)
                            .map_err(|e| format!("chmod writable {}: {}", dir.display(), e))
                    }
                    #[cfg(not(unix))]
                    {
                        permissions.set_readonly(false);
                        std::fs::set_permissions(dir, permissions)
                            .map_err(|e| format!("mark writable {}: {}", dir.display(), e))
                    }
                };
                // Restore the artifact dir first (so its children are reachable),
                // then the transitions/ subdir that the non-writable step also
                // locked (the transition log now lives there).
                restore_writable(&artifact_dir)?;
                let transitions_dir = artifact_dir.join("transitions");
                if transitions_dir.exists() {
                    restore_writable(&transitions_dir)?;
                }

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("hearth_path", hearth);
                out.set("intake_instance_path", artifact_path);
                if let Some(response) = ctx.get::<serde_json::Value>("complete_response") {
                    out.set("complete_response", response.clone());
                    out.set("response", response.clone());
                }
                if let Some(work_dir) = ctx.get::<PathBuf>("work_dir") {
                    out.set("work_dir", work_dir.clone());
                }
                if let Some(engine_process) = ctx.take::<EngineProcess>("engine_process") {
                    out.set("engine_process", engine_process);
                }
                if let Some(owner_home) = ctx.get::<PathBuf>("mcp_persist_owner_home") {
                    out.set("mcp_persist_owner_home", owner_home.clone());
                }
                if let Some(handle) =
                    ctx.get::<Arc<Mutex<Option<tempfile::TempDir>>>>("mcp_persist_owner_home_handle")
                {
                    out.set("mcp_persist_owner_home_handle", handle.clone());
                }
                if let Some(bytes) = ctx.get::<Vec<u8>>("mcp_persist_machine_yaml_bytes") {
                    out.set("mcp_persist_machine_yaml_bytes", bytes.clone());
                }
                Ok(out)
            },
        ),
        step_def(
            "the same intake builder terminal complete is attempted through the MCP shim",
            &[("mcp_process", "McpProcess"), ("intake_instance_path", "String")],
            &[
                ("mcp_process", "McpProcess"),
                ("complete_response", "JsonValue"),
                ("response", "JsonValue"),
                ("hearth_path", "PathBuf"),
                ("work_dir", "PathBuf"),
                ("intake_instance_path", "String"),
                ("engine_process", "EngineProcess"),
                ("mcp_persist_owner_home", "PathBuf"),
                ("mcp_persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("mcp_persist_machine_yaml_bytes", "Vec<u8>"),
            ],
            |mut ctx, _params| {
                let mut process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                let artifact_path = ctx
                    .get::<String>("intake_instance_path")
                    .ok_or("No intake_instance_path")?
                    .clone();
                let arguments = serde_json::json!({
                    "artifact_path": artifact_path,
                    "actor_name": "Intake-Builder-Driver-100000",
                    "actor_type": "agent",
                    "actor_model": "gpt-5-codex",
                    "actor_provider": "openai",
                    "satisfaction": "approved",
                    "approver": "lore",
                    "note": "",
                    "reflection_notes": ""
                });
                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1290,
                    "method": "tools/call",
                    "params": {
                        "name": "complete",
                        "arguments": arguments
                    }
                });
                process.send(&request);
                std::thread::sleep(std::time::Duration::from_millis(3000));
                let response = process.read_response()?;

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("complete_response", response.clone());
                out.set("response", response);
                out.set("intake_instance_path", artifact_path);
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                if let Some(work_dir) = ctx.get::<PathBuf>("work_dir") {
                    out.set("work_dir", work_dir.clone());
                }
                if let Some(engine_process) = ctx.take::<EngineProcess>("engine_process") {
                    out.set("engine_process", engine_process);
                }
                if let Some(owner_home) = ctx.get::<PathBuf>("mcp_persist_owner_home") {
                    out.set("mcp_persist_owner_home", owner_home.clone());
                }
                if let Some(handle) =
                    ctx.get::<Arc<Mutex<Option<tempfile::TempDir>>>>("mcp_persist_owner_home_handle")
                {
                    out.set("mcp_persist_owner_home_handle", handle.clone());
                }
                if let Some(bytes) = ctx.get::<Vec<u8>>("mcp_persist_machine_yaml_bytes") {
                    out.set("mcp_persist_machine_yaml_bytes", bytes.clone());
                }
                Ok(out)
            },
        ),
        step_def(
            "the MCP persist owner-home machine.yaml bytes are recorded for kind {string}",
            &[("mcp_persist_owner_home", "PathBuf")],
            &[
                ("mcp_process", "McpProcess"),
                ("complete_response", "JsonValue"),
                ("response", "JsonValue"),
                ("hearth_path", "PathBuf"),
                ("work_dir", "PathBuf"),
                ("intake_instance_path", "String"),
                ("engine_process", "EngineProcess"),
                ("mcp_persist_owner_home", "PathBuf"),
                ("mcp_persist_owner_home_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("mcp_persist_machine_yaml_bytes", "Vec<u8>"),
            ],
            |mut ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let process = ctx.take::<McpProcess>("mcp_process").ok_or("No mcp_process")?;
                let owner_home = ctx
                    .get::<PathBuf>("mcp_persist_owner_home")
                    .ok_or("No mcp_persist_owner_home")?
                    .clone();
                let path = owner_home.join("playbooks").join(kind).join("machine.yaml");
                let bytes = std::fs::read(&path)
                    .map_err(|e| format!("read persisted machine {}: {}", path.display(), e))?;

                let mut out = Context::new();
                out.set("mcp_process", process);
                out.set("mcp_persist_owner_home", owner_home);
                out.set("mcp_persist_machine_yaml_bytes", bytes);
                if let Some(response) = ctx.get::<serde_json::Value>("complete_response") {
                    out.set("complete_response", response.clone());
                    out.set("response", response.clone());
                }
                if let Some(hp) = ctx.get::<PathBuf>("hearth_path") {
                    out.set("hearth_path", hp.clone());
                }
                if let Some(work_dir) = ctx.get::<PathBuf>("work_dir") {
                    out.set("work_dir", work_dir.clone());
                }
                if let Some(artifact_path) = ctx.get::<String>("intake_instance_path") {
                    out.set("intake_instance_path", artifact_path.clone());
                }
                if let Some(engine_process) = ctx.take::<EngineProcess>("engine_process") {
                    out.set("engine_process", engine_process);
                }
                if let Some(handle) =
                    ctx.get::<Arc<Mutex<Option<tempfile::TempDir>>>>("mcp_persist_owner_home_handle")
                {
                    out.set("mcp_persist_owner_home_handle", handle.clone());
                }
                Ok(out)
            },
        ),
        check_def(
            "the MCP persist owner-home machine.yaml bytes are unchanged for kind {string}",
            &[
                ("mcp_persist_owner_home", "PathBuf"),
                ("mcp_persist_machine_yaml_bytes", "Vec<u8>"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let owner_home = ctx
                    .get::<PathBuf>("mcp_persist_owner_home")
                    .ok_or("No mcp_persist_owner_home")?;
                let before = ctx
                    .get::<Vec<u8>>("mcp_persist_machine_yaml_bytes")
                    .ok_or("No recorded machine bytes")?;
                let path = owner_home.join("playbooks").join(kind).join("machine.yaml");
                let after = std::fs::read(&path)
                    .map_err(|e| format!("read persisted machine {}: {}", path.display(), e))?;
                if &after == before {
                    Ok(())
                } else {
                    Err(format!(
                        "Persisted machine bytes changed for kind '{}' at {}",
                        kind,
                        path.display()
                    ))
                }
            },
        ),
        check_def(
            "the complete response is a tool error containing {string}",
            &[("complete_response", "JsonValue")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected text")?;
                let response = ctx
                    .get::<serde_json::Value>("complete_response")
                    .ok_or("No complete_response")?;
                let is_error = response["result"]["isError"].as_bool().unwrap_or(false);
                if !is_error {
                    return Err(format!(
                        "Expected complete tool error, got: {}",
                        serde_json::to_string_pretty(response).unwrap_or_default()
                    ));
                }
                let text = response["result"]["content"][0]["text"].as_str().unwrap_or("");
                if text.contains(expected) {
                    Ok(())
                } else {
                    Err(format!("Complete error '{}' does not contain '{}'", text, expected))
                }
            },
        ),
        check_def(
            "the generated candidate playbook kind {string} has measurement for state {string} role {string}",
            &[("mcp_persist_owner_home", "PathBuf")],
            |ctx, params| {
                use anvil_core::domain::playbook::hearth_registry::HearthPlaybookRegistry;
                use anvil_core::domain::playbook::registry::PlaybookRegistry;
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let state_name = params.get_string(1).ok_or("Expected state")?;
                let role = params.get_string(2).ok_or("Expected role")?;
                let owner_home = ctx
                    .get::<PathBuf>("mcp_persist_owner_home")
                    .ok_or("No mcp_persist_owner_home")?;
                let registry = HearthPlaybookRegistry::new(owner_home.clone());
                let machine = registry.machine_for(kind).ok_or_else(|| {
                    format!(
                        "fresh registry from owner-home does not resolve kind '{}'",
                        kind
                    )
                })?;
                let state = machine
                    .states
                    .iter()
                    .find(|state| state.name == state_name)
                    .ok_or_else(|| format!("No state '{}' on generated machine", state_name))?;
                let measurement = state.measurement_by_role.get(role).ok_or_else(|| {
                    format!(
                        "No measurement for state '{}' role '{}' on generated machine",
                        state_name, role
                    )
                })?;
                if measurement.intent.trim().is_empty()
                    || measurement.expected_output.trim().is_empty()
                {
                    Err(format!(
                        "Measurement for state '{}' role '{}' was empty: {:?}",
                        state_name, role, measurement
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the driven playbook_generation builder emitted non-degenerate step measurements for each builder step",
            &[("engine_process", "EngineProcess")],
            |ctx, _params| {
                let expected_states = [
                    "gathering",
                    "gathering_review",
                    "analyzing",
                    "analyze_review",
                    "modeling",
                    "model_review",
                    "testing",
                    "test_review",
                    "trial_run",
                    "trial_review",
                    "reflecting",
                    "reflection_review",
                    "evolving",
                    "evolve_review",
                    "completed",
                ];
                let expected: std::collections::BTreeSet<&str> =
                    expected_states.iter().copied().collect();
                let engine = ctx
                    .get::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                std::thread::sleep(std::time::Duration::from_secs(2));
                let lines = engine.stderr_lines();
                let mut counts: std::collections::BTreeMap<String, usize> =
                    std::collections::BTreeMap::new();
                let mut records = Vec::new();
                for line in &lines {
                    let value: serde_json::Value = match serde_json::from_str(line) {
                        Ok(value) => value,
                        Err(_) => continue,
                    };
                    let Some(obj) = value.as_object() else {
                        continue;
                    };
                    // After H1, step_measurement records carry track_id = the
                    // playbook KIND (the cross-instance aggregation key) and
                    // playbook_id = the per-run instance id (NOT the definition
                    // id). Select builder records by track_id == the kind, and
                    // require playbook_id to be a non-empty instance id distinct
                    // from the kind.
                    if obj.get("event_kind").and_then(|v| v.as_str())
                        != Some("step_measurement")
                        || obj.get("track_id").and_then(|v| v.as_str())
                            != Some(anvil_test_support::builder::BUILDER_KIND)
                    {
                        continue;
                    }
                    let to_state = obj.get("to_state").and_then(|v| v.as_str()).unwrap_or("");
                    let track_id = obj.get("track_id").and_then(|v| v.as_str()).unwrap_or("");
                    let playbook_id = obj
                        .get("playbook_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let intent = obj.get("intent").and_then(|v| v.as_str()).unwrap_or("");
                    let expected_output = obj
                        .get("expected_output")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if to_state.is_empty()
                        || track_id.is_empty()
                        || playbook_id.is_empty()
                        || playbook_id == track_id
                        || intent.is_empty()
                        || expected_output.is_empty()
                    {
                        continue;
                    }
                    *counts.entry(to_state.to_string()).or_insert(0) += 1;
                    records.push(line.clone());
                }

                let actual: std::collections::BTreeSet<&str> =
                    counts.keys().map(String::as_str).collect();
                if actual != expected {
                    return Err(format!(
                        "Builder step measurement states mismatch. Expected {:?}, got {:?}.\nRecords:\n{}",
                        expected,
                        actual,
                        records.join("\n")
                    ));
                }
                let duplicates: Vec<_> = counts
                    .iter()
                    .filter(|(_, count)| **count != 1)
                    .map(|(state, count)| format!("{}={}", state, count))
                    .collect();
                if !duplicates.is_empty() {
                    return Err(format!(
                        "Expected exactly one non-degenerate builder step measurement per state, got duplicate counts {:?}.\nRecords:\n{}",
                        duplicates,
                        records.join("\n")
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the persisted daily_recap generated playbook is loader-valid with three measured diamonds and no hooks",
            &[("mcp_persist_owner_home", "PathBuf")],
            |ctx, _params| {
                use anvil_core::domain::playbook::hearth_registry::HearthPlaybookRegistry;
                use anvil_core::domain::playbook::loader::load_from_yaml;
                use anvil_core::domain::playbook::registry::PlaybookRegistry;

                let owner_home = ctx
                    .get::<PathBuf>("mcp_persist_owner_home")
                    .ok_or("No mcp_persist_owner_home")?;
                let path = owner_home
                    .join("playbooks")
                    .join("daily_recap")
                    .join("machine.yaml");
                let yaml = std::fs::read_to_string(&path)
                    .map_err(|e| format!("read persisted machine {}: {}", path.display(), e))?;
                let machine = load_from_yaml("daily_recap", &yaml, &[])
                    .map_err(|e| format!("load_from_yaml failed for {}: {}", path.display(), e))?;

                if machine.kind != "daily_recap"
                    || machine.directory != "daily_recaps"
                    || machine.registry != "daily_recaps.md"
                {
                    return Err(format!(
                        "Unexpected daily_recap metadata: kind={} directory={} registry={}",
                        machine.kind, machine.directory, machine.registry
                    ));
                }

                let expected_states = [
                    "gathering",
                    "gathering_review",
                    "gathering_revision",
                    "synthesizing",
                    "synthesizing_review",
                    "synthesizing_revision",
                    "reporting",
                    "reporting_review",
                    "reporting_revision",
                    "outcome_reflection_review",
                    "completed",
                ];
                let actual_states: Vec<&str> =
                    machine.states.iter().map(|state| state.name.as_str()).collect();
                if actual_states != expected_states {
                    return Err(format!(
                        "Unexpected daily_recap states. Expected {:?}, got {:?}",
                        expected_states, actual_states
                    ));
                }

                for state in &machine.states {
                    if state.hook.is_some() || !state.hooks_by_role.is_empty() {
                        return Err(format!("State '{}' has hook references", state.name));
                    }
                    let should_be_review = state.name.ends_with("_review");
                    if state.is_review_gate != should_be_review {
                        return Err(format!(
                            "State '{}' review gate expected {}, got {}",
                            state.name, should_be_review, state.is_review_gate
                        ));
                    }
                    let should_be_terminal = state.name == "completed";
                    if state.is_terminal != should_be_terminal {
                        return Err(format!(
                            "State '{}' terminal expected {}, got {}",
                            state.name, should_be_terminal, state.is_terminal
                        ));
                    }
                }

                for working_state in ["gathering", "synthesizing", "reporting"] {
                    let state = machine
                        .states
                        .iter()
                        .find(|state| state.name == working_state)
                        .ok_or_else(|| format!("Missing state '{}'", working_state))?;
                    let measurement = state.measurement_by_role.get("doer").ok_or_else(|| {
                        format!("State '{}' missing doer measurement", working_state)
                    })?;
                    if measurement.intent.trim().is_empty()
                        || measurement.expected_output.trim().is_empty()
                    {
                        return Err(format!(
                            "State '{}' has empty doer measurement: {:?}",
                            working_state, measurement
                        ));
                    }
                }

                for transition in &machine.transitions {
                    if transition.hook.is_some() {
                        return Err(format!(
                            "Transition '{} -> {}' has a hook reference",
                            transition.from_state, transition.to_state
                        ));
                    }
                    let from_review = transition.from_state.ends_with("_review");
                    if from_review {
                        let satisfaction = transition.required_satisfaction.as_deref().ok_or_else(|| {
                            format!(
                                "Review transition '{} -> {}' missing required_satisfaction",
                                transition.from_state, transition.to_state
                            )
                        })?;
                        let expected = if transition.to_state.ends_with("_revision") {
                            ["needs_revision"].as_slice()
                        } else {
                            ["satisfied"].as_slice()
                        };
                        if satisfaction != expected {
                            return Err(format!(
                                "Review transition '{} -> {}' expected satisfaction {:?}, got {:?}",
                                transition.from_state, transition.to_state, expected, satisfaction
                            ));
                        }
                    } else if transition.required_satisfaction.is_some() {
                        return Err(format!(
                            "Non-review transition '{} -> {}' unexpectedly has required_satisfaction {:?}",
                            transition.from_state,
                            transition.to_state,
                            transition.required_satisfaction
                        ));
                    }
                }

                let registry = HearthPlaybookRegistry::new(owner_home.clone());
                if !registry.invalid_artifacts().is_empty() {
                    return Err(format!(
                        "fresh registry from owner-home has load errors: {:?}",
                        registry.invalid_artifacts()
                    ));
                }
                registry.machine_for("daily_recap").ok_or_else(|| {
                    "fresh registry from owner-home does not resolve kind 'daily_recap'".to_string()
                })?;
                Ok(())
            },
        ),
        check_def(
            "the hearth status.yaml for the intake instance contains {string}",
            &[("hearth_path", "PathBuf"), ("intake_instance_path", "String")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let artifact_path = ctx
                    .get::<String>("intake_instance_path")
                    .ok_or("No intake_instance_path")?;
                let status_path = hearth.join(artifact_path).join("status.yaml");
                let content = std::fs::read_to_string(&status_path)
                    .map_err(|e| format!("Failed to read {}: {}", status_path.display(), e))?;
                if content.contains(needle) {
                    Ok(())
                } else {
                    Err(format!(
                        "status.yaml at {} does not contain '{}'. Content:\n{}",
                        status_path.display(), needle, content
                    ))
                }
            },
        ),
        check_def(
            "the intake instance status state is not {string}",
            &[("hearth_path", "PathBuf"), ("intake_instance_path", "String")],
            |ctx, params| {
                let forbidden = params.get_string(0).ok_or("Expected forbidden state")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let artifact_path = ctx
                    .get::<String>("intake_instance_path")
                    .ok_or("No intake_instance_path")?;
                let dir = hearth.join(artifact_path);
                let status_path = dir.join("status.yaml");
                let content = std::fs::read_to_string(&status_path)
                    .map_err(|e| format!("Failed to read {}: {}", status_path.display(), e))?;
                let status: anvil_core::domain::status::FullStatusYaml =
                    serde_yaml::from_str(&content)
                        .map_err(|e| format!("Failed to parse {}: {}", status_path.display(), e))?;
                // Resolve through the folding seam, not the stale raw `state:`.
                let state = anvil_core::domain::transition_log::resolve_state_with_events(
                    &status, &dir,
                )
                .map_err(|e| format!("unreadable transition evidence: {e}"))?
                .ok_or_else(|| format!("No resolvable state for {}", dir.display()))?;
                if state == forbidden.as_ref() as &str {
                    Err(format!(
                        "Expected intake instance state not to be '{}', but status.yaml at {} was:\n{}",
                        forbidden,
                        status_path.display(),
                        content
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "no relative playbook directory {string} exists under the MCP engine working directory",
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
        check_def(
            "the generation context for the intake instance contains {string}",
            &[("hearth_path", "PathBuf"), ("intake_instance_path", "String")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let artifact_path = ctx
                    .get::<String>("intake_instance_path")
                    .ok_or("No intake_instance_path")?;
                let context_path = hearth.join(artifact_path).join("generation-context.json");
                let content = std::fs::read_to_string(&context_path)
                    .map_err(|e| format!("Failed to read {}: {}", context_path.display(), e))?;
                if content.contains(needle) {
                    Ok(())
                } else {
                    Err(format!(
                        "generation context at {} does not contain '{}'. Content:\n{}",
                        context_path.display(), needle, content
                    ))
                }
            },
        ),
        check_def(
            "the intake step measurement is stamped with the instance transition time and not candidate time {string}",
            &[("hearth_path", "PathBuf"), ("intake_instance_path", "String"), ("engine_process", "EngineProcess")],
            |ctx, params| {
                let candidate_at = params.get_string(0).ok_or("Expected candidate time")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let artifact_path = ctx
                    .get::<String>("intake_instance_path")
                    .ok_or("No intake_instance_path")?;
                let dir = hearth.join(artifact_path);
                let status_path = dir.join("status.yaml");
                let status_content = std::fs::read_to_string(&status_path)
                    .map_err(|e| format!("Failed to read {}: {}", status_path.display(), e))?;
                // The transition log is now the per-file event store; read the
                // latest folded transition's `at` rather than the legacy array.
                let status: anvil_core::domain::status::FullStatusYaml =
                    serde_yaml::from_str(&status_content)
                        .map_err(|e| format!("Failed to parse {}: {}", status_path.display(), e))?;
                let history =
                    anvil_core::domain::transition_log::resolve_transitions_with_events(&status, &dir)
                        .map_err(|e| format!("unreadable transition evidence: {e}"))?;
                let transition_at = history
                    .last()
                    .and_then(|t| t.at.clone())
                    .ok_or_else(|| {
                        format!(
                            "No transition at timestamp found for {} (event store + legacy array empty)",
                            dir.display()
                        )
                    })?;
                let transition_at = transition_at.as_str();
                if transition_at == candidate_at.as_ref() as &str {
                    return Err(format!(
                        "Test setup expected distinct transition and candidate times, both were {}",
                        candidate_at
                    ));
                }

                let engine = ctx
                    .get::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                // track_id on the step_measurement is now the playbook KIND (not
                // the run/instance id) — the kind-aggregation semantics. Resolve
                // the instance's kind from its status.yaml and match on that.
                let track_id = status
                    .kind
                    .clone()
                    .ok_or("intake instance status.yaml has no kind")?;
                let track_id = track_id.as_str();

                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                let mut matching_records = Vec::new();
                loop {
                    let lines = engine.stderr_lines();
                    for line in &lines {
                        let value: serde_json::Value = match serde_json::from_str(line) {
                            Ok(value) => value,
                            Err(_) => continue,
                        };
                        let obj = match value.as_object() {
                            Some(obj) => obj,
                            None => continue,
                        };
                        let is_intake_measurement = obj
                            .get("event_kind")
                            .and_then(|v| v.as_str())
                            == Some("step_measurement")
                            && obj.get("track_id").and_then(|v| v.as_str()) == Some(track_id)
                            && obj.get("to_state").and_then(|v| v.as_str()) == Some("gathering")
                            && obj.get("role").and_then(|v| v.as_str()) == Some("doer");
                        if !is_intake_measurement {
                            continue;
                        }

                        matching_records.push(line.clone());
                        let measurement_at = obj
                            .get("at")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| format!("step_measurement has no string at field: {}", line))?;
                        if measurement_at == candidate_at {
                            return Err(format!(
                                "intake step_measurement used candidate time {} instead of engine transition time {}. Record: {}",
                                candidate_at, transition_at, line
                            ));
                        }
                        if measurement_at != transition_at {
                            return Err(format!(
                                "intake step_measurement at {} did not equal transition time {}. Record: {}",
                                measurement_at, transition_at, line
                            ));
                        }
                    }
                    if !matching_records.is_empty() {
                        return Ok(());
                    }
                    if std::time::Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
                let last_seen = engine.stderr_lines();
                Err(format!(
                    "No intake step_measurement found for track_id {}.\nCaptured stderr lines ({}):\n{}",
                    track_id,
                    last_seen.len(),
                    last_seen.join("\n")
                ))
            },
        ),
    ]
}

fn valid_proposed_states_json() -> serde_json::Value {
    serde_json::json!([
        {
            "state": "triage",
            "role": "doer",
            "intent": "Sort the escalation into the right support lane.",
            "expected_output": "A triage note naming the lane and next action."
        }
    ])
}

fn default_route_description(intent: &str) -> String {
    let normalized = if intent.trim().is_empty() {
        "candidate"
    } else {
        intent.trim()
    };
    format!(
        "Route here when the user asks to AUTHOR {} playbooks. NOT for running a {} instance.",
        normalized, normalized
    )
}

fn route_triggers_from_fields(
    fields: &std::collections::HashMap<String, String>,
    intent: &str,
) -> Vec<String> {
    fields
        .get("route_triggers")
        .map(|value| comma_list(value))
        .filter(|values| !values.is_empty())
        .unwrap_or_else(|| {
            let normalized = if intent.trim().is_empty() {
                "candidate"
            } else {
                intent.trim()
            };
            vec![
                format!("create {} playbook", normalized),
                format!("author {} playbook", normalized),
            ]
        })
}

fn projection_targets_from_fields(
    fields: &std::collections::HashMap<String, String>,
) -> Vec<String> {
    fields
        .get("projection_targets")
        .map(|value| comma_list(value))
        .filter(|values| !values.is_empty())
        .unwrap_or_else(|| vec!["workflows.md".to_string()])
}

fn comma_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn intake_candidate_playbook_request(
    fields: &std::collections::HashMap<String, String>,
    at_field: &str,
    proposed_states: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let evidence: Vec<String> = fields
        .get("evidence")
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default();
    let at = fields
        .get(at_field)
        .or_else(|| fields.get("at"))
        .ok_or_else(|| format!("Missing '{}' value", at_field))?;
    let intent = fields.get("intent").cloned().unwrap_or_default();

    let arguments = serde_json::json!({
        "source": fields.get("source").cloned().unwrap_or_default(),
        "intent": intent.clone(),
        "at": at,
        "evidence": evidence,
        "route_description": fields
            .get("route_description")
            .cloned()
            .unwrap_or_else(|| default_route_description(&intent)),
        "route_triggers": route_triggers_from_fields(fields, &intent),
        "projection_targets": projection_targets_from_fields(fields),
        "proposed_states": proposed_states,
        "target_owner": fields.get("target_owner").cloned().unwrap_or_default(),
        "parent_id": fields.get("parent_id").cloned().unwrap_or_default(),
        "approver": fields.get("approver").cloned().unwrap_or_else(|| "lore".to_string()),
        "actor_name": fields.get("actor_name").cloned().unwrap_or_default(),
        "actor_type": fields.get("actor_type").cloned().unwrap_or_default(),
        "actor_model": fields.get("actor_model").cloned().unwrap_or_default(),
        "actor_provider": fields.get("actor_provider").cloned().unwrap_or_default(),
    });

    Ok(serde_json::json!({
        "jsonrpc": "2.0",
        "id": 72,
        "method": "tools/call",
        "params": {
            "name": CANDIDATE_PLAYBOOK_INTAKE,
            "arguments": arguments
        }
    }))
}

fn parse_intake_tool_response(response: &serde_json::Value) -> Result<serde_json::Value, String> {
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .ok_or_else(|| format!("No result content text in response: {}", response))?;
    serde_json::from_str(text).map_err(|e| format!("Failed to parse intake response JSON: {}", e))
}
