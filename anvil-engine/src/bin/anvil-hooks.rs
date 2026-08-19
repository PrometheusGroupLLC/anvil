//! `anvil-hooks` — the per-harness hook INSTALLER and runtime gate.
//!
//! Anvil owns the hook contract AND the per-harness adapters (Foundry merely
//! triggers). This binary delivers Anvil kit hooks into any agent harness and
//! enforces the runtime pre-mutation gate.
//!
//! Subcommands:
//!   install   [--harness auto|claude-code|codex|kiln|hermes] [--config-dir <p>] [--hearth <p>] [--with-mcp]
//!   uninstall [--harness ...] [--config-dir <p>] [--with-mcp]
//!   gate-check  (reads the harness's pre-tool JSON payload on stdin; decides allow/block)
//!
//! Install-time computes the installable hook set DIRECTLY from the hearth via
//! anvil-core's `fold_hook_manifest` (no running daemon required). The runtime
//! gate resolves the mutation's enclosing forge artifact and the open-begin
//! status from the on-disk event store — the SAME predicate the engine's
//! `begin_adoption_status` RPC evaluates. General harness calls fail open;
//! Codex's explicitly attributed hard lane fails closed for governed targets.

/// Wrap a command message as a request that names this binary as its surface
/// AND carries the caller's Foundry credential.
///
/// The engine refuses a state-changing call that does not name the program
/// making it, so this is the command line's half of the CQRS command seam.
/// Unlike the MCP shim — which has one `call_engine` every RPC routes through —
/// hooks connects and builds its request separately per subcommand, so this
/// helper exists to keep the four of them from drifting apart. A subcommand
/// that forgets it does not fail quietly: it is refused with `surface_required`.
///
/// The bearer rides here for the same reason: a Foundry-mode engine gates every
/// one of these RPCs, so a per-subcommand copy of the attach step is a per-
/// subcommand opportunity to forget it. An absent bearer is passed through
/// unattached on purpose — the ENGINE decides authorization, and a client that
/// refused on its own behalf would be enforcing a rule that can be edited out.
fn cli_request<T>(
    message: T,
    bearer: &anvil_engine::kit_bearer::KitBearer,
) -> Result<tonic::Request<T>, String> {
    let mut request = tonic::Request::new(message);
    request.metadata_mut().insert(
        anvil_engine::command_seam::SURFACE_METADATA_KEY,
        tonic::metadata::MetadataValue::from_static("cli"),
    );
    anvil_engine::kit_bearer::attach_kit_bearer(request, bearer)
}

/// Append the credential's provenance to an engine refusal that was about
/// authentication.
///
/// `not_authenticated` on its own tells an operator that a credential was
/// missing or bad, but not WHICH credential path this process took — inherited,
/// minted, or never obtained. Those demand different fixes, so the answer is
/// carried in the message rather than left to be guessed.
fn credential_note(status_message: &str, bearer: &anvil_engine::kit_bearer::KitBearer) -> String {
    if status_message.contains("not_authenticated") {
        format!("\n  ({})", bearer.source.describe())
    } else {
        String::new()
    }
}

use anvil_core::domain::hooks::gate_check::{
    decide_with_policy, find_enclosing_artifact_dir, resolve_artifact,
    resolve_begin_status_from_disk, FailurePolicy, Verdict,
};
use anvil_core::domain::hooks::installer::{self, HarnessOutcome};
use anvil_core::domain::hooks::route_turn::{
    extract_conversation_id, extract_transcript_context, extract_transcript_path,
    extract_user_message, format_guidance, format_park_hint, CandidateBrief, InProgressSignal,
    RouteTurnOutcome, RouterVerdict,
};
use anvil_core::domain::hooks::{Harness, InstallSpec, DEFAULT_GATE_COMMAND, DEFAULT_TURN_COMMAND};
use anvil_core::ports::delivery_log_port::{
    project_delivery_record, DeliveryLogWritePort, DeliveryObservation,
};
use anvil_core_hearth::fs_delivery_log_adapter::FileSystemDeliveryLogAdapter;
use anvil_core_hearth::fs_query_adapter::FileSystemQueryAdapter;
use foundry_engine_addressing::{resolve, ResolveOpts};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anvil_engine::abstention_ledger;
use anvil_engine::kiln_router;
use anvil_engine::telemetry;

/// The default engine gRPC endpoint. The daemon is up at turn time; the
/// route-turn call is time-boxed so a slow/missing engine never stalls the prompt.
const DEFAULT_ENGINE_PORT: u16 = 50051;
const DEFAULT_ENGINE_URL: &str = "http://127.0.0.1:50051";
const ENGINE_HEALTH_PATH: &str = "/health";
/// Hard cap on the route-turn gRPC round trip. Fail-open past this.
///
/// 1500 was sized when the engine answered in double-digit ms. Measured against the
/// LIVE engine on the real hearth 2026-07-28: Route takes **2.3-4.8s** — so EVERY
/// interactive turn blew this cap, hit the `Err(_) => None` fail-open arm, and delivered
/// silence. That arm is unlogged, so a total delivery outage looked exactly like a router
/// that declined. This is the actual cause of "anvil delivers nothing"; the candidate-set
/// widening was necessary but could never have been observed through a 1500ms wall.
///
/// Raised to 8000 to clear the measured range with headroom. Override with
/// `ANVIL_ROUTE_TURN_TIMEOUT_MS` (the engine's own latency is the thing to fix; this cap
/// must not silently hide it again).
const ROUTE_TURN_TIMEOUT_MS: u64 = 8000;

/// Effective route-turn cap: `ANVIL_ROUTE_TURN_TIMEOUT_MS` if set and valid, else the
/// constant above.
fn route_turn_timeout_ms() -> u64 {
    std::env::var("ANVIL_ROUTE_TURN_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(ROUTE_TURN_TIMEOUT_MS)
}
/// context_aware_routing: cap on how many bytes of the transcript tail we read
/// (seeked from the end). The hook is time-boxed — a bounded read stays cheap.
const TRANSCRIPT_TAIL_BYTES: u64 = 32 * 1024;
/// context_aware_routing: cap on how many trailing transcript lines we scan.
const TRANSCRIPT_TAIL_LINES: usize = 40;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(subcommand) = args.first().map(String::as_str) else {
        eprintln!("{}", USAGE);
        return ExitCode::from(64); // EX_USAGE
    };

    if matches!(subcommand, "install" | "uninstall")
        && args[1..]
            .iter()
            .any(|argument| matches!(argument.as_str(), "--help" | "-h"))
    {
        println!("{}", USAGE);
        return ExitCode::SUCCESS;
    }

    match subcommand {
        "install" => run_install(&args[1..]),
        "uninstall" => run_uninstall(&args[1..]),
        "gate-check" => run_gate_check(&args[1..]),
        "render-codex-plugin-hooks" => run_render_codex_plugin_hooks(&args[1..]),
        "route-turn" => run_route_turn(&args[1..]),
        "begin" => run_begin(&args[1..]),
        "snapshot" => run_snapshot(&args[1..]),
        "complete" => run_complete(&args[1..]),
        "amend" => run_amend(&args[1..]),
        "artifact-of-record" => run_artifact_of_record(&args[1..]),
        "--help" | "-h" | "help" => {
            println!("{}", USAGE);
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("anvil-hooks: unknown subcommand '{}'\n\n{}", other, USAGE);
            ExitCode::from(64)
        }
    }
}

const USAGE: &str = "\
anvil-hooks — per-harness hook installer + runtime gate

USAGE:
  anvil-hooks install   [--harness auto|claude-code|codex|kiln|hermes] [--config-dir <path>] [--hearth <path>] [--command <cmd>] [--timeout <ms>] [--with-mcp] [--mcp-command <cmd>] [--codex-plugin-managed]
  anvil-hooks uninstall [--harness auto|claude-code|codex|kiln|hermes] [--config-dir <path>] [--with-mcp]
  anvil-hooks gate-check                       # reads the pre-tool JSON payload on stdin
  anvil-hooks render-codex-plugin-hooks [--command <cmd>] [--timeout <ms>]
  anvil-hooks route-turn [--message <text>] [--hearth <path>] [--port <n>] [--source <harness>]   # reads the user-prompt JSON on stdin
  anvil-hooks begin --artifact-type <kind> --actor-name <n> --actor-type <t> --actor-model <m> --actor-provider <p> [--field k=v ...] [--name <n>] [--parent-id <id>] [--approver <a>] [--playbook-name <n>] [--target-owner <o>] [--conversation-id <id>] [--hearth <path>] [--port <n>]   # CREATION mode: mint and begin a NEW artifact via the direct-engine channel (bypasses the MCP server; see CREDENTIALS below for what it still needs); --field (repeatable) supplies machine-declared required fields
  anvil-hooks begin --identifier <id> --actor-name <n> --actor-type <t> --actor-model <m> --actor-provider <p> [--role resumer|reviewer|doer|complete] [--conversation-id <id>] [--hearth <path>] [--port <n>]   # RESUME mode: re-enter the EXISTING artifact <id> and be served its current-state context. Mints nothing
  anvil-hooks snapshot --artifact-path <p> --to-state <s> --actor-name <n> --actor-type <t> --actor-model <m> --actor-provider <p> [--actor-role <r>] [--approver <a>] [--note <n>] [--conversation-id <id>] [--hearth <path>] [--port <n>]   # drive an arbitrary state transition (the general lifecycle-advance verb) via the same direct-engine channel
  anvil-hooks complete --artifact-path <p> --actor-name <n> --actor-type <t> --actor-model <m> --actor-provider <p> [--satisfaction satisfied|full_revision|address_in_next_step] [--findings <text>] [--approver <a>] [--note <n>] [--reflection-notes <text>] [--claimed-evidence <class>:<reference> ...] [--conversation-id <id>] [--hearth <path>] [--port <n>]   # declare the current pass finished (doer/reviewer paths) via the direct-engine channel; --claimed-evidence (repeatable) presents an ordered evidence claim (class one of artifact_of_consequence|verifiable_citation|self_description; split on the first ':' so the reference keeps any internal file:line colon) assessed against the step's obligation — omitting it is byte-identical to the pre-affordance behavior. A completion that lands the artifact in `completed` is MERGE-CHECKED: a `commit:<sha>` reference must be an ancestor of that repository's origin/main and a path reference must exist, or the completion is refused before any write. Cross-repo claims must name the repository — `commit:<repo>@<sha>`, `<repo>@<path>` — because the hearth and the code are different repositories; an unqualified reference resolves against the hearth's own repository only. Repositories are looked for beside the hearth's repository, or in ANVIL_CODE_REPO_ROOTS (colon-separated) when set
  anvil-hooks amend --artifact-path <p> --kind <k> --target-document <d> --actor-name <n> --actor-type <t> --actor-model <m> --actor-provider <p> [--target-id <id>] [--op-kind <k>] [--body <text>] [--new-kind <k>] [--anchor <a>] [--hearth <path>] [--port <n>]   # record a structured amendment op against a frozen document (and drive completed→amend) via the direct-engine channel

NOTES:
  --harness auto (default) installs into every harness whose config dir is detected.
  --config-dir overrides the harness's conventional dir (~/.claude, ~/.codex, ~/.kiln, ~/.hermes).
  By DEFAULT install/uninstall deliver HOOKS ONLY and never touch any harness's MCP
  config (Foundry's wire.rs owns the version-stable anvil-mcp registration).
  --with-mcp (opt-in, default OFF) ALSO registers/unregisters the anvil MCP server
  (stdio) in each harness's MCP config (Claude Code .claude.json mcpServers.anvil,
  Codex [mcp_servers.anvil-mcp]) — the standalone path for running anvil WITHOUT
  Foundry. --mcp-command sets the version-stable MCP command (Foundry:
  ${KIT_ROOT}/mcp/anvil-mcp); absent, the sibling anvil-mcp next to this binary is used.
  gate-check resolves the mutation's enclosing forge artifact and BLOCKS (exit 2)
  a hard-enforced kind that lacks an open begin session. General harness calls fail
  OPEN on errors; --source codex fails CLOSED for malformed governed targets.
  route-turn ships the user's turn into the engine route RPC (recording the routing
  decision) and prints routing guidance to stdout for the harness to inject. It
  ALWAYS fails open (exit 0, no output) on any error and time-boxes the gRPC call.
  begin has TWO modes and exactly one must be selected. --artifact-type CREATES a
  new artifact; --identifier RESUMES the existing one you name. Passing both (or
  --identifier alongside any creation flag) is REFUSED, never silently resolved —
  guessing is what minted duplicate artifacts. In resume mode --role names the
  session role the engine acts under (default resumer, the re-entry case).

CREDENTIALS:
  The lifecycle verbs (begin/snapshot/complete/amend) and route-turn all call
  GATED engine RPCs. What they need depends on the engine they are talking to:
    * STANDALONE engine (no FOUNDRY_SESSION_TOKEN in the ENGINE's own env at
      startup) — no credential is consulted. This channel is fully independent
      of both the MCP server and the broker.
    * FOUNDRY-MODE engine — every gated RPC MUST carry a valid bearer, and
      absence is a REFUSAL, never a downgrade to standalone. This channel is
      independent of the anvil MCP SERVER, but it is NOT broker-independent:
      the broker is what mints bearers. Earlier versions of this help claimed
      otherwise; that claim only appeared true while the engine shipped with
      its session verifier compiled out.
  The credential is resolved in this order:
    1. FOUNDRY_SESSION_TOKEN, if set non-blank (what the Foundry supervisor
       injects into kit processes) — forwarded as-is.
    2. else a ticket minted for `foundry-mcp:anvil-kit` over the broker socket
       named by FOUNDRY_BROKER_SOCKET.
    3. else no bearer is sent, and a Foundry-mode engine answers
       `not_authenticated`. The refusal names which of the two was missing.
  So: invoked by the supervisor, this works with no setup. Invoked from a bare
  shell against a Foundry-mode engine, export FOUNDRY_BROKER_SOCKET (typically
  ~/.foundry/run/broker.sock) so a ticket can be minted.";

/// Parse a `--flag value` argument out of `args`.
fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

/// Collect every repeated `--field key=value` into a map for `BeginRequest.create_fields`.
/// This is what lets `anvil-hooks begin` satisfy ANY machine-declared required field
/// (e.g. `--field question=… --field requester=…` for lore_query) — not just the
/// track-shaped structured flags — so a routed turn can begin in ONE call.
fn collect_fields(args: &[String]) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--field" {
            if let Some(kv) = args.get(i + 1) {
                if let Some((k, v)) = kv.split_once('=') {
                    if !k.trim().is_empty() {
                        map.insert(k.trim().to_string(), v.to_string());
                    }
                }
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    map
}

/// Collect every repeated `--claimed-evidence <class>:<reference>` into an
/// ORDERED `Vec<ClaimedEvidence>` for `CompleteRequest.claimed_evidence`.
///
/// Mirrors `collect_fields` (the repeatable `--field k=v` collector) with three
/// deliberate differences: the separator is the FIRST `:` (via `split_once(':')`)
/// so a `reference` that is itself a `file:line` citation keeps its own internal
/// colon; the result is an ordered `Vec` (not a map) so the caller's order and
/// duplicates reach the engine intact; and only the `class` half is trimmed (the
/// reference is copied byte-for-byte, no path resolution). The `class` token stays
/// opaque here — the engine's already-shipped `claimed_evidence_to_domain` is the
/// single validator (an unknown token surfaces as its `INVALID_ARGUMENT` /
/// `claimed_evidence_class_unknown`). Omitting the flag yields `Vec::new()` —
/// byte-identical to the pre-affordance hardcode.
fn collect_claimed_evidence(args: &[String]) -> Vec<anvil_engine::proto::ClaimedEvidence> {
    let mut claims = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--claimed-evidence" {
            if let Some(kv) = args.get(i + 1) {
                if let Some((class, reference)) = kv.split_once(':') {
                    if !class.trim().is_empty() {
                        claims.push(anvil_engine::proto::ClaimedEvidence {
                            class: class.trim().to_string(),
                            reference: reference.to_string(),
                        });
                    }
                }
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    claims
}

/// Whether a bare `--flag` is present.
fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

/// Resolve `$HOME` for the conventional config-dir probes.
fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn anvil_rendezvous_dir() -> PathBuf {
    home_dir().join(".anvil")
}

fn port_from_url(url: &str) -> Option<u16> {
    let authority = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .split('/')
        .next()?;
    authority.rsplit_once(':')?.1.parse::<u16>().ok()
}

fn resolve_engine_port(args: &[String]) -> u16 {
    // An explicit `--port` is the strongest expression of intent — a caller
    // targeting a specific engine instance (tests, a side-by-side engine, a
    // non-default deployment). It MUST win over rendezvous/health discovery:
    // otherwise a live engine on the default endpoint silently captures the call,
    // ignoring the requested port (the symptom: e2e CLI calls against a test
    // engine get intercepted by a running :50051 and rejected on its hearth roots).
    if let Some(port) = flag(args, "--port").and_then(|s| s.parse::<u16>().ok()) {
        return port;
    }
    // No explicit port: discover a live engine (ANVIL_ENGINE_URL / rendezvous /
    // health-probe), falling back to the default endpoint's port.
    let opts = ResolveOpts::new("ANVIL_ENGINE_URL", anvil_rendezvous_dir())
        .with_default(DEFAULT_ENGINE_URL)
        .with_health_path(ENGINE_HEALTH_PATH);
    let resolved = resolve(&opts)
        .url
        .unwrap_or_else(|| DEFAULT_ENGINE_URL.to_string());
    port_from_url(&resolved).unwrap_or(DEFAULT_ENGINE_PORT)
}

/// Build the [`InstallSpec`] from `--command` / `--timeout` / `--mcp-command`
/// flags (with defaults).
fn install_spec(args: &[String]) -> InstallSpec {
    InstallSpec {
        command: flag(args, "--command")
            .unwrap_or(DEFAULT_GATE_COMMAND)
            .to_string(),
        timeout_ms: flag(args, "--timeout")
            .and_then(|s| s.parse().ok())
            .unwrap_or(5000),
        turn_command: flag(args, "--turn-command")
            .unwrap_or(DEFAULT_TURN_COMMAND)
            .to_string(),
        mcp_command: resolve_mcp_command(args),
    }
}

/// Resolve the VERSION-STABLE anvil MCP server command to register:
///   1. `--mcp-command <cmd>` if supplied (Foundry passes `${KIT_ROOT}/mcp/anvil-mcp`).
///   2. ELSE derive the sibling `anvil-mcp` next to the running `anvil-hooks`
///      binary (`current_exe()` → parent → `anvil-mcp`) — standalone-safe.
///   3. ELSE the bare name `anvil-mcp` (last resort; never a hard-coded dev path).
fn resolve_mcp_command(args: &[String]) -> String {
    if let Some(cmd) = flag(args, "--mcp-command") {
        return cmd.to_string();
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.join("anvil-mcp")))
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "anvil-mcp".to_string())
}

/// Resolve which harness(es) to act on and how to find each one's config dir.
///
/// `--config-dir` pins the dir for a SINGLE named harness (it makes no sense for
/// `auto`, which spans multiple harnesses). Without it, each harness uses its
/// conventional dir under `$HOME`.
fn resolve_targets(
    args: &[String],
) -> Result<(Vec<Harness>, Box<dyn Fn(Harness) -> PathBuf>), String> {
    let harness_arg = flag(args, "--harness").unwrap_or("auto");
    let config_dir = flag(args, "--config-dir").map(PathBuf::from);

    if harness_arg == "auto" {
        if config_dir.is_some() {
            // A single explicit dir can't host all four harnesses' native files
            // sensibly; still honor it as a probe root so tests/fixtures can
            // point auto at one place.
            let dir = config_dir.unwrap();
            return Ok((Harness::all().to_vec(), Box::new(move |h| dir.join(h.id()))));
        }
        let home = home_dir();
        return Ok((
            Harness::all().to_vec(),
            Box::new(move |h| h.default_config_dir(&home)),
        ));
    }

    let harness =
        Harness::parse(harness_arg).ok_or_else(|| format!("unknown harness '{}'", harness_arg))?;
    match config_dir {
        Some(dir) => Ok((vec![harness], Box::new(move |_| dir.clone()))),
        None => {
            let home = home_dir();
            Ok((
                vec![harness],
                Box::new(move |h| h.default_config_dir(&home)),
            ))
        }
    }
}

fn run_install(args: &[String]) -> ExitCode {
    let spec = install_spec(args);
    let (targets, dir_for) = match resolve_targets(args) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("anvil-hooks install: {}", e);
            return ExitCode::from(64);
        }
    };

    let with_mcp = has_flag(args, "--with-mcp");
    let mode = if has_flag(args, "--codex-plugin-managed") {
        installer::InstallMode::CodexPluginManaged
    } else {
        installer::InstallMode::Standalone
    };
    let reports = installer::install_all_with_mode(&targets, dir_for, &spec, with_mcp, mode);
    let mut any_failed = false;
    for report in &reports {
        if matches!(report.outcome, HarnessOutcome::Failed(_)) {
            any_failed = true;
        }
        println!("{}", installer::render_report(report));
    }
    if any_failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn run_uninstall(args: &[String]) -> ExitCode {
    let (targets, dir_for) = match resolve_targets(args) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("anvil-hooks uninstall: {}", e);
            return ExitCode::from(64);
        }
    };

    let with_mcp = has_flag(args, "--with-mcp");
    let reports = installer::uninstall_all(&targets, dir_for, with_mcp);
    let mut any_failed = false;
    for report in &reports {
        if matches!(report.outcome, HarnessOutcome::Failed(_)) {
            any_failed = true;
        }
        println!("{}", installer::render_report(report));
    }
    if any_failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn run_render_codex_plugin_hooks(args: &[String]) -> ExitCode {
    match anvil_core::domain::hooks::codex::render_plugin_hooks(&install_spec(args)) {
        Ok(document) => {
            println!("{document}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("anvil-hooks render-codex-plugin-hooks: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Mutation paths extracted from a pre-tool payload.
///
/// `unresolved_bash` distinguishes a Bash payload whose cwd is known but whose
/// command does not expose a trustworthy literal mutation target. The caller
/// can then fail closed only when that cwd is inside the resolved hearth,
/// preserving outside-hearth commands.
struct MutationTargets {
    paths: Vec<PathBuf>,
    unresolved_bash: bool,
}

/// Resolve the intentionally narrow Bash contract we can inspect without
/// executing or pretending to fully parse a shell. A literal `touch` command is
/// supported because every non-option argument is a mutation target. Expansion,
/// shell metacharacters, parent traversal, and other commands are ambiguous.
fn extract_literal_touch_paths(command: &str, cwd: &Path) -> Option<Vec<PathBuf>> {
    let mut tokens = command.split_ascii_whitespace();
    if tokens.next()? != "touch" {
        return None;
    }
    let mut paths = Vec::new();
    let mut options_done = false;
    for token in tokens {
        if token == "--" && !options_done {
            options_done = true;
            continue;
        }
        if !options_done && token.starts_with('-') {
            continue;
        }
        if token.is_empty()
            || token.contains([
                '$', '*', '?', '{', '}', '`', '\'', '"', ';', '|', '&', '<', '>',
            ])
        {
            return None;
        }
        let path = PathBuf::from(token);
        if path
            .components()
            .any(|component| component == std::path::Component::ParentDir)
        {
            return None;
        }
        paths.push(if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        });
    }
    (!paths.is_empty()).then_some(paths)
}

/// The PreToolUse payload fields the gate cares about: the edited file path and
/// (optionally) the tool name. We parse defensively — harnesses vary — and fall
/// back to flags so the gate is testable and harness-agnostic.
fn extract_mutation_paths(args: &[String], codex_strict: bool) -> Result<MutationTargets, ()> {
    // Explicit flag wins (used by tests + harnesses that pass it directly).
    if let Some(p) = flag(args, "--path") {
        return Ok(MutationTargets {
            paths: vec![PathBuf::from(p)],
            unresolved_bash: false,
        });
    }
    // Otherwise read the harness's JSON payload on stdin (Claude Code PreToolUse).
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() || buf.trim().is_empty() {
        return Err(());
    }
    let json: serde_json::Value = serde_json::from_str(&buf).map_err(|_| ())?;
    if codex_strict {
        let tool = json
            .get("tool_name")
            .or_else(|| json.get("toolName"))
            .and_then(serde_json::Value::as_str)
            .ok_or(())?;
        let cwd = json
            .get("cwd")
            .or_else(|| json.pointer("/tool_input/cwd"))
            .or_else(|| json.pointer("/tool_input/workdir"))
            .and_then(serde_json::Value::as_str)
            .map(PathBuf::from);
        return match tool {
            "apply_patch" => {
                let patch = json
                    .pointer("/tool_input/command")
                    .or_else(|| json.pointer("/tool_input/patch"))
                    .or_else(|| json.pointer("/tool_input/input"))
                    .and_then(serde_json::Value::as_str)
                    .ok_or(())?;
                let base = cwd.or_else(|| std::env::current_dir().ok()).ok_or(())?;
                let paths: Vec<PathBuf> = patch
                    .lines()
                    .filter_map(|line| {
                        [
                            "*** Add File: ",
                            "*** Update File: ",
                            "*** Delete File: ",
                            "*** Move to: ",
                        ]
                            .iter()
                            .find_map(|prefix| line.strip_prefix(prefix))
                    })
                    .map(PathBuf::from)
                    .map(|path| {
                        if path.is_absolute() {
                            path
                        } else {
                            base.join(path)
                        }
                    })
                    .collect();
                if paths.is_empty() {
                    Err(())
                } else {
                    Ok(MutationTargets {
                        paths,
                        unresolved_bash: false,
                    })
                }
            }
            "Bash" => {
                let cwd = cwd.ok_or(())?;
                let command = json
                    .pointer("/tool_input/command")
                    .and_then(serde_json::Value::as_str)
                    .ok_or(())?;
                if let Some(paths) = extract_literal_touch_paths(command, &cwd) {
                    Ok(MutationTargets {
                        paths,
                        unresolved_bash: false,
                    })
                } else {
                    Ok(MutationTargets {
                        paths: vec![cwd],
                        unresolved_bash: true,
                    })
                }
            }
            _ => Err(()),
        };
    }
    // Claude Code shape: { "tool_input": { "file_path": "..." }, ... }.
    let found: Option<String> = [
        json.pointer("/tool_input/file_path"),
        json.pointer("/tool_input/path"),
        json.pointer("/params/file_path"),
        json.get("file_path"),
        json.get("path"),
    ]
    .into_iter()
    .flatten()
    .find_map(|v| v.as_str())
    .map(str::to_string);
    found
        .map(PathBuf::from)
        .map(|path| MutationTargets {
            paths: vec![path],
            unresolved_bash: false,
        })
        .ok_or(())
}

/// Resolve the hearth root for gate-check: `--hearth` flag, else the `.hearth`
/// pointer file in the edited path's directory tree or cwd.
fn resolve_gate_hearth(args: &[String], edited_path: Option<&Path>) -> Option<PathBuf> {
    if let Some(h) = flag(args, "--hearth") {
        return Some(PathBuf::from(h));
    }
    // Walk up from the edited path (or cwd) looking for a `.hearth` pointer.
    let start = edited_path
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())?;
    let mut dir: Option<&Path> = Some(start.as_path());
    while let Some(current) = dir {
        let pointer = current.join(".hearth");
        if pointer.is_file() {
            return read_hearth_pointer(&pointer);
        }
        dir = current.parent();
    }
    None
}

/// Read a `.hearth` pointer file (`path: <hearth-dir>`), resolving relative paths
/// against the pointer's directory.
fn read_hearth_pointer(pointer: &Path) -> Option<PathBuf> {
    let content = std::fs::read_to_string(pointer).ok()?;
    #[derive(serde::Deserialize)]
    struct HearthConfig {
        path: String,
    }
    let cfg: HearthConfig = serde_yaml::from_str(&content).ok()?;
    let base = pointer.parent().unwrap_or_else(|| Path::new("."));
    let resolved = base.join(&cfg.path);
    Some(resolved.canonicalize().unwrap_or(resolved))
}

/// Read the hook GATE POLICY's `hard_enforce` list from the kit manifest, mirror
/// of the engine's `read_hook_gate_policy`. Absent → empty (everything soft).
fn read_hard_enforce(hearth_root: &Path, args: &[String]) -> Vec<String> {
    // Explicit override (tests / Foundry): comma-separated kinds.
    if let Some(raw) = flag(args, "--hard-enforce") {
        return raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    // Probe the kit manifest from the hearth and its ancestors.
    let mut dir: Option<&Path> = Some(hearth_root);
    while let Some(current) = dir {
        let candidate = current.join("foundry-manifest.json");
        if candidate.is_file() {
            if let Some(list) = parse_hard_enforce(&candidate) {
                return list;
            }
        }
        dir = current.parent();
    }
    Vec::new()
}

fn parse_hard_enforce(path: &Path) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let arr = json.pointer("/hooks/hard_enforce")?.as_array()?;
    Some(
        arr.iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_string)
            .collect(),
    )
}

fn run_gate_check(args: &[String]) -> ExitCode {
    let codex_strict = flag(args, "--source") == Some("codex");
    let failure_policy = if codex_strict {
        FailurePolicy::CodexFailClosed
    } else {
        FailurePolicy::FailOpen
    };
    let mutation_targets = match extract_mutation_paths(args, codex_strict) {
        Ok(targets) => targets,
        Err(()) => return exit_for(failure_policy.on_resolution_failure(), None),
    };
    let edited_paths = mutation_targets.paths;
    let actor = flag(args, "--actor").map(str::to_string);
    let internal_deadline = flag(args, "--internal-deadline-ms")
        .and_then(|raw| raw.parse::<u64>().ok())
        .map(std::time::Duration::from_millis);
    let resolution = gate_verdict_with_deadline(
        edited_paths.clone(),
        mutation_targets.unresolved_bash,
        args.to_vec(),
        actor,
        failure_policy,
        internal_deadline,
    );
    if resolution.verdict == Verdict::Block {
        return block(
            resolution
                .blocked_path
                .as_deref()
                .unwrap_or_else(|| Path::new("<internal-deadline>")),
        );
    }
    allow()
}

struct GateResolution {
    verdict: Verdict,
    blocked_path: Option<PathBuf>,
}

impl GateResolution {
    fn allow() -> Self {
        Self {
            verdict: Verdict::Allow,
            blocked_path: None,
        }
    }

    fn block(path: Option<&Path>) -> Self {
        Self {
            verdict: Verdict::Block,
            blocked_path: path.map(Path::to_path_buf),
        }
    }
}

fn resolve_gate_verdicts(
    paths: &[PathBuf],
    unresolved_bash: bool,
    args: &[String],
    actor: Option<&str>,
    failure_policy: FailurePolicy,
) -> GateResolution {
    for path in paths {
        let Some(hearth_root) = resolve_gate_hearth(args, Some(path)) else {
            // A valid outside target does not erase a later governed target.
            continue;
        };
        if unresolved_bash {
            let canonical_hearth = hearth_root
                .canonicalize()
                .unwrap_or_else(|_| hearth_root.clone());
            let cwd = path.canonicalize().unwrap_or_else(|_| path.clone());
            if cwd.starts_with(&canonical_hearth) {
                return GateResolution::block(Some(path));
            }
            continue;
        }
        let hard_enforce = read_hard_enforce(&hearth_root, args);
        let query = FileSystemQueryAdapter::new(hearth_root.clone());
        let artifact = resolve_artifact(path, &hearth_root);
        if artifact.is_none()
            && find_enclosing_artifact_dir(path, &hearth_root).is_some()
            && failure_policy == FailurePolicy::CodexFailClosed
        {
            return GateResolution::block(Some(path));
        }
        let begin_status = artifact
            .as_ref()
            .and_then(|resolved| resolve_begin_status_from_disk(&query, resolved, actor));
        if decide_with_policy(
            artifact.as_ref(),
            &hard_enforce,
            begin_status,
            failure_policy,
        ) == Verdict::Block
        {
            return GateResolution::block(Some(path));
        }
    }
    GateResolution::allow()
}

fn gate_verdict_with_deadline(
    paths: Vec<PathBuf>,
    unresolved_bash: bool,
    args: Vec<String>,
    actor: Option<String>,
    failure_policy: FailurePolicy,
    deadline: Option<std::time::Duration>,
) -> GateResolution {
    let Some(deadline) = deadline else {
        return resolve_gate_verdicts(
            &paths,
            unresolved_bash,
            &args,
            actor.as_deref(),
            failure_policy,
        );
    };

    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        #[cfg(debug_assertions)]
        {
            if let Some(delay) = std::env::var("ANVIL_HOOKS_TEST_RESOLVER_DELAY_MS")
                .ok()
                .and_then(|raw| raw.parse::<u64>().ok())
            {
                std::thread::sleep(std::time::Duration::from_millis(delay));
            }
        }
        let verdict = resolve_gate_verdicts(
            &paths,
            unresolved_bash,
            &args,
            actor.as_deref(),
            failure_policy,
        );
        let _ = sender.send(verdict);
    });

    receiver
        .recv_timeout(deadline)
        .unwrap_or_else(|_| GateResolution {
            verdict: failure_policy.on_resolution_failure(),
            blocked_path: None,
        })
}

fn exit_for(verdict: Verdict, path: Option<&PathBuf>) -> ExitCode {
    match verdict {
        Verdict::Allow => allow(),
        Verdict::Block => block(
            path.map(PathBuf::as_path)
                .unwrap_or_else(|| Path::new("<unknown>")),
        ),
    }
}

/// The route-turn subcommand: ship the user's turn into the engine's `route` RPC
/// (which RECORDS the routing decision via the activity log + routing-activity
/// sink — we do NOT reimplement recording) and print routing guidance to stdout
/// for the harness to inject. ALWAYS exits 0 with no output on any failure
/// (engine down / parse error / timeout) so a slow or missing engine never
/// blocks or delays the user's turn.
/// The Rust/Python boundary for the open-step sweep.
///
/// The sweep lives with the reliability watchers (Python) while the authority
/// for "which file is this state's artifact, and what does an untouched one look
/// like" lives in anvil-core (Rust). Rather than reimplement that in Python —
/// which is how the placeholder identity would drift from the writer that
/// produces it — the sweep shells out to this.
///
/// Emits a SHA-256 rather than the bytes: the sweep hashes the file on disk and
/// compares digests, so the placeholder content is never duplicated on the
/// Python side and cannot fall out of step with the scaffold.
fn run_artifact_of_record(args: &[String]) -> ExitCode {
    let kind = flag(args, "--kind").unwrap_or_default();
    let state = flag(args, "--state").unwrap_or_default();
    // The display name determines the placeholder bytes; a caller that cannot
    // supply it gets the sentinel form, whose prefix/suffix still identify an
    // untouched scaffold.
    let display_name = flag(args, "--display-name").unwrap_or("").to_string();
    match anvil_core::domain::begin::artifact_of_record(kind, state, &display_name) {
        Some((path, bytes)) => {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            let digest = hasher.finalize();
            println!(
                "{{\"path\":\"{}\",\"placeholder_sha256\":\"{}\"}}",
                path,
                digest.iter().map(|b| format!("{b:02x}")).collect::<String>()
            );
            ExitCode::SUCCESS
        }
        None => {
            // Not an error: "this kind/state has no single artifact of record"
            // is a legitimate, common answer, and making it an error would push
            // the sweep into treating an expected case as a failure.
            println!("{{\"applicable\":false}}");
            ExitCode::SUCCESS
        }
    }
}

fn run_route_turn(args: &[String]) -> ExitCode {
    // Fail-open envelope: any None / Err along the way → no output, exit 0.
    if let Some(guidance) = route_turn_guidance(args) {
        if !guidance.is_empty() {
            // Shape the stdout for the originating harness's UserPromptSubmit
            // contract: Kiln injects ONLY a JSON `additionalContext` field (raw
            // text is dropped), every other harness injects raw stdout verbatim.
            let source = match flag(args, "--source") {
                Some(s) if !s.trim().is_empty() => s,
                _ => "(unattributed)",
            };
            println!(
                "{}",
                anvil_core::domain::hooks::route_turn::shape_guidance_for_source(&guidance, source)
            );
        }
    }
    ExitCode::SUCCESS
}

/// `begin` subcommand — begin a playbook through the direct-engine channel (the
/// same gRPC path route-turn uses), bypassing the anvil MCP server.
///
/// # What this channel is and is not independent OF
///
/// This was long documented as the "broker-independent" path that "works when
/// the broker is down". Against a **standalone** engine that is true: the engine
/// consults no verifier and this binary needs no credential. Against a
/// **Foundry-mode** engine it is false, and it was only ever *apparently* true
/// because the shipped engine had its session verifier compiled out — so an
/// uncredentialed call was indistinguishable from an authorized one.
///
/// With the verifier restored, a Foundry-mode engine REQUIRES a bearer on this
/// RPC and the broker is what mints bearers. So the honest statement is:
/// independent of the MCP **server** (this binary dials the engine directly),
/// but dependent on the **broker** whenever the engine runs in Foundry mode.
/// The credential is resolved by `anvil_engine::kit_bearer` — inherited from
/// `FOUNDRY_SESSION_TOKEN` when the supervisor supplied one, otherwise minted
/// from `FOUNDRY_BROKER_SOCKET`, otherwise absent and refused by name.
///
/// The adoption value is undiminished and unchanged: the MCP *tool* channel is
/// the thing that flaps, and `anvil-hooks begin` (Bash-invokable, always on
/// PATH) still routes around it. Unlike route-turn (advisory, fail-silent),
/// begin is an ACTION: it reports success (exit 0 + result) or failure (exit 1
/// + stderr) so the caller knows.
fn run_begin(args: &[String]) -> ExitCode {
    match call_engine_begin(args) {
        // The engine accepted the begin: print the scoped first step to stdout, exit 0.
        Some(Ok(out)) => {
            println!("{}", out);
            ExitCode::SUCCESS
        }
        // The engine ANSWERED and rejected (missing field, bad parent, …): the reason
        // is actionable, but the begin did not happen — stderr + non-zero so a caller
        // (model or script) knows to fix the inputs and retry.
        Some(Err(reason)) => {
            eprintln!("{}", reason);
            ExitCode::from(1)
        }
        // Could not reach the engine. Argument shape is settled before we dial
        // (`resolve_begin_mode`) and identity is the engine's own verdict, so
        // this is now only ever "nothing answered on that port".
        None => {
            eprintln!(
                "anvil-hooks begin: engine unreachable on the requested port (is the anvil engine \
                 running? check --port). No begin was attempted."
            );
            ExitCode::from(1)
        }
    }
}

/// The two shapes a `begin` call can take.
///
/// `begin` was CREATION-ONLY at this seam even though the wire
/// (`BeginRequest.identifier`) and the MCP shim have carried both modes all
/// along. A harness therefore had no way to say "re-enter the artifact I am
/// already driving": calling begin with an existing artifact's name minted a
/// SECOND artifact plus a stray registry line. Resolving the mode BEFORE dialing
/// keeps the two apart, and makes "both at once" a refusal rather than a silent
/// preference — the ambiguity is the defect, so it is not resolved by precedence.
enum BeginMode {
    /// `--artifact-type <kind> [--name …] [--parent-id …] [--field k=v …]`
    Create { artifact_type: String },
    /// `--identifier <existing-id> [--role <r>]`
    Resume { identifier: String, role: String },
}

/// Flags that mean something only when CREATING an artifact (each one is a
/// "Creation mode:" argument in the MCP shim's own begin schema). Any of them
/// carrying a value alongside `--identifier` is the ambiguity this refuses.
const BEGIN_CREATE_ONLY_FLAGS: [&str; 7] = [
    "--artifact-type",
    "--parent-id",
    "--name",
    "--playbook-name",
    "--approver",
    "--target-owner",
    "--field",
];

/// The session role an identifier-mode begin acts under. The engine REQUIRES one
/// (an empty `session_role` on an identifier begin is `SessionRequired`: "No
/// active session. Call checkin first."). The MCP shim takes it from the prior
/// `checkin`; a surface that ships identity directly instead — `anvil_orchestrate`,
/// and this CLI — declares it on the call. `resumer` is the default because
/// re-entering an artifact you already have is what `--identifier` is for;
/// `--role` names any other role the engine recognizes.
const DEFAULT_BEGIN_ROLE: &str = "resumer";

/// Read a `--flag value` that must be non-blank to count as supplied.
fn trimmed_flag(args: &[String], name: &str) -> Option<String> {
    flag(args, name)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// The house refusal line every `begin` rejection shares, whether the engine
/// answered with it or this binary resolved it before dialing. One formatter so
/// a caller (model or script) sees one shape.
fn begin_rejected(reason: &str) -> String {
    format!(
        "✗ anvil begin rejected: {} — fix the inputs and retry `anvil-hooks begin`.",
        reason
    )
}

/// Decide which mode the caller asked for, or refuse. Pure over the argv.
fn resolve_begin_mode(args: &[String]) -> Result<BeginMode, String> {
    match (
        trimmed_flag(args, "--artifact-type"),
        trimmed_flag(args, "--identifier"),
    ) {
        (_, Some(identifier)) => {
            let creation_flags: Vec<&str> = BEGIN_CREATE_ONLY_FLAGS
                .iter()
                .copied()
                .filter(|name| trimmed_flag(args, name).is_some())
                .collect();
            if !creation_flags.is_empty() {
                return Err(format!(
                    "--identifier was passed with creation-mode flag(s) {}. These are two \
                     different modes — --identifier RESUMES the existing artifact you name, \
                     --artifact-type MINTS a new one — so this call is ambiguous. Drop one side",
                    creation_flags.join(", ")
                ));
            }
            Ok(BeginMode::Resume {
                identifier,
                role: trimmed_flag(args, "--role")
                    .unwrap_or_else(|| DEFAULT_BEGIN_ROLE.to_string()),
            })
        }
        (Some(artifact_type), None) => Ok(BeginMode::Create { artifact_type }),
        (None, None) => Err(
            "neither --artifact-type nor --identifier was given. Pass --artifact-type <kind> \
             to CREATE a new artifact, or --identifier <existing-id> to RESUME one you \
             already have"
                .to_string(),
        ),
    }
}

/// The conversation id to stamp on the begin: the caller-supplied `--conversation-id`
/// when non-empty, else `CLAUDE_CODE_SESSION_ID` (the same id the route hook hashes),
/// so the begin's activity record joins the route leg (the adoption-join fix).
fn begin_conversation_id(args: &[String]) -> String {
    if let Some(c) = flag(args, "--conversation-id") {
        if !c.trim().is_empty() {
            return c.trim().to_string();
        }
    }
    std::env::var("CLAUDE_CODE_SESSION_ID")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_default()
}

/// Call the engine's Begin RPC directly (mirrors `call_engine_route`). Three outcomes:
/// `Some(Ok(msg))` = the engine accepted (msg = the scoped first step, or the resumed
/// artifact's current-state context);
/// `Some(Err(reason))` = REJECTED — either the engine's own actionable verdict, or a
/// mode refusal this binary resolved before dialing (both wear `begin_rejected`);
/// `None` = could not reach the engine.
fn call_engine_begin(args: &[String]) -> Option<Result<String, String>> {
    use anvil_engine::proto::anvil_service_client::AnvilServiceClient;
    use anvil_engine::proto::BeginRequest;

    // Mode first: a refusal here is answerable without an engine, and cannot
    // mint anything on the way to being answered.
    let mode = match resolve_begin_mode(args) {
        Ok(mode) => mode,
        Err(reason) => return Some(Err(begin_rejected(&reason))),
    };
    let (artifact_type, identifier, session_role, success_prefix) = match &mode {
        BeginMode::Create { artifact_type } => (
            artifact_type.clone(),
            String::new(),
            // Creation mode carries no session role — the engine's create path
            // does not read one. Unchanged from before `--identifier` existed.
            String::new(),
            format!("✓ Began `{}`", artifact_type),
        ),
        BeginMode::Resume { identifier, role } => (
            String::new(),
            identifier.clone(),
            role.clone(),
            format!("✓ Resumed `{}` as `{}`", identifier, role),
        ),
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    let port = resolve_engine_port(args);
    let hearth = resolve_gate_hearth(args, None);
    let hearth_path = hearth.map(|p| p.display().to_string()).unwrap_or_default();
    let project_root = std::env::current_dir()
        .ok()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let request = BeginRequest {
        hearth_path,
        artifact_type,
        // Resume mode's whole payload: the existing artifact to re-enter, plus the
        // role the engine acts under. Empty in creation mode — the engine's own
        // dispatch requires EXACTLY one of artifact_type/identifier.
        identifier,
        session_role,
        parent_id: flag(args, "--parent-id").unwrap_or("").to_string(),
        track_name: flag(args, "--name").unwrap_or("").to_string(),
        approver: flag(args, "--approver").unwrap_or("").to_string(),
        actor_name: flag(args, "--actor-name").unwrap_or("").to_string(),
        actor_type: flag(args, "--actor-type").unwrap_or("").to_string(),
        actor_model: flag(args, "--actor-model").unwrap_or("").to_string(),
        actor_provider: flag(args, "--actor-provider").unwrap_or("").to_string(),
        playbook_name: flag(args, "--playbook-name").unwrap_or("").to_string(),
        target_owner: flag(args, "--target-owner").unwrap_or("").to_string(),
        conversation_id: begin_conversation_id(args),
        project_root,
        claimed_evidence: Vec::new(),
        // Generic machine-declared fields (`--field k=v`, repeatable) → the engine's
        // required-field check reads these, so domain kinds (lore_query, agent_memory,
        // compile_topic, …) begin in one call without bespoke flags.
        create_fields: collect_fields(args),
        ..Default::default()
    };
    let addr = format!("http://127.0.0.1:{}", port);
    rt.block_on(async move {
        // Resolve the Foundry credential BEFORE dialing. A Foundry-mode engine
        // gates begin, so an uncredentialed dial can only earn not_authenticated.
        let bearer = anvil_engine::kit_bearer::resolve_kit_bearer().await;
        // Connection failure → None (channel down: caller prints the unreachable line + exit 1).
        let mut client = AnvilServiceClient::connect(addr).await.ok()?;
        let authed = match cli_request(request, &bearer) {
            Ok(r) => r,
            Err(e) => return Some(Err(begin_rejected(&e))),
        };
        // The engine ANSWERED — surface its verdict either way. A rejection (e.g.
        // ParentNotFound, MissingRequiredField) is actionable signal the model needs,
        // NOT a silent failure: return it (Err) so the caller prints the real reason.
        let resp = match client.begin(authed).await {
            Ok(r) => r.into_inner(),
            Err(status) => {
                return Some(Err(format!(
                    "{}{}",
                    begin_rejected(status.message()),
                    credential_note(status.message(), &bearer)
                )))
            }
        };
        let mut out = format!("{} → state `{}`", success_prefix, resp.state);
        if !resp.track_path.is_empty() {
            out.push_str(&format!(" at {}", resp.track_path));
        }
        if !resp.intent.is_empty() {
            out.push_str(&format!("\nIntent: {}", resp.intent));
        }
        if !resp.expected_output.is_empty() {
            out.push_str(&format!("\nExpected output: {}", resp.expected_output));
        }
        if !resp.context_text.is_empty() {
            out.push_str(&format!("\n\n{}", resp.context_text));
        }
        if !resp.next_step.is_empty() {
            out.push_str(&format!("\n\nNext: {}", resp.next_step));
        }
        Some(Ok(out))
    })
}

/// Parse an optional integer `--flag value` (e.g. `--actor-context-window`).
fn int_flag(args: &[String], name: &str) -> i64 {
    flag(args, name)
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0)
}

/// Resolve the gRPC port for a lifecycle verb (`--port`, default 50051).
fn resolve_port(args: &[String]) -> u16 {
    flag(args, "--port")
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(DEFAULT_ENGINE_PORT)
}

/// Build a single-threaded tokio runtime for a blocking gRPC call.
fn block_runtime() -> Option<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()
}

/// The unreachable/missing-args line every lifecycle verb shares (mirrors begin).
fn print_lifecycle_unreachable(verb: &str) -> ExitCode {
    eprintln!(
        "anvil-hooks {verb}: engine unreachable on the requested port (is the anvil engine running?), \
         or required args missing (--artifact-path, --actor-name/type/model/provider)."
    );
    ExitCode::from(1)
}

/// Render a lifecycle-verb outcome to stdout/stderr + exit code (mirrors run_begin):
/// `Some(Ok)` = engine accepted (stdout, exit 0); `Some(Err)` = engine answered but
/// REJECTED (stderr, exit 1); `None` = unreachable / missing args before dialing.
fn finish_lifecycle(verb: &str, outcome: Option<Result<String, String>>) -> ExitCode {
    match outcome {
        Some(Ok(out)) => {
            println!("{}", out);
            ExitCode::SUCCESS
        }
        Some(Err(reason)) => {
            eprintln!("{}", reason);
            ExitCode::from(1)
        }
        None => print_lifecycle_unreachable(verb),
    }
}

/// `snapshot` subcommand — drive an arbitrary state transition through the
/// direct-engine channel (the SAME gRPC path `begin` uses), so a worker agent
/// with only shell access can advance a track's lifecycle without the anvil MCP
/// server. Like `begin`, this bypasses the MCP server but NOT the broker: a
/// Foundry-mode engine gates this RPC and the credential comes from
/// `anvil_engine::kit_bearer` (see `run_begin` for why the older
/// "broker-independent" claim was never true against a Foundry-mode engine).
/// This is the general lifecycle-advance verb (`complete` is the spec/review
/// convenience over the same machine).
fn run_snapshot(args: &[String]) -> ExitCode {
    finish_lifecycle("snapshot", call_engine_snapshot(args))
}

/// Call the engine's Snapshot RPC directly (mirrors `call_engine_begin`).
fn call_engine_snapshot(args: &[String]) -> Option<Result<String, String>> {
    use anvil_engine::proto::anvil_service_client::AnvilServiceClient;
    use anvil_engine::proto::SnapshotRequest;

    let artifact_path = flag(args, "--artifact-path")?.to_string();
    if artifact_path.trim().is_empty() {
        return None;
    }
    let to_state = flag(args, "--to-state")?.to_string();
    if to_state.trim().is_empty() {
        return None;
    }
    let rt = block_runtime()?;
    let port = resolve_port(args);
    let hearth_path = resolve_gate_hearth(args, None)
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let project_root = std::env::current_dir()
        .ok()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let request = SnapshotRequest {
        hearth_path,
        artifact_path,
        to_state: to_state.clone(),
        actor_name: flag(args, "--actor-name").unwrap_or("").to_string(),
        actor_role: flag(args, "--actor-role").unwrap_or("").to_string(),
        approver: flag(args, "--approver").unwrap_or("").to_string(),
        note: flag(args, "--note").unwrap_or("").to_string(),
        actor_type: flag(args, "--actor-type").unwrap_or("").to_string(),
        actor_model: flag(args, "--actor-model").unwrap_or("").to_string(),
        actor_provider: flag(args, "--actor-provider").unwrap_or("").to_string(),
        actor_context_window: int_flag(args, "--actor-context-window"),
        actor_sdk_version: flag(args, "--actor-sdk-version").unwrap_or("").to_string(),
        actor_entrypoint: flag(args, "--actor-entrypoint").unwrap_or("").to_string(),
        projection_only: false,
        event_type: String::new(),
        conversation_id: begin_conversation_id(args),
        project_root,
        claimed_evidence: Vec::new(),
    };
    let addr = format!("http://127.0.0.1:{}", port);
    rt.block_on(async move {
        let bearer = anvil_engine::kit_bearer::resolve_kit_bearer().await;
        let mut client = AnvilServiceClient::connect(addr).await.ok()?;
        let authed = match cli_request(request, &bearer) {
            Ok(r) => r,
            Err(e) => return Some(Err(format!("✗ anvil snapshot rejected: {e}"))),
        };
        let resp = match client.snapshot(authed).await {
            Ok(r) => r.into_inner(),
            Err(status) => {
                return Some(Err(format!(
                    "✗ anvil snapshot rejected: {} — fix the inputs and retry `anvil-hooks snapshot`.{}",
                    status.message(),
                    credential_note(status.message(), &bearer)
                )));
            }
        };
        let mut out = format!("✓ Snapshot → state `{}`", to_state);
        if !resp.actor_name.is_empty() {
            out.push_str(&format!(" (actor {})", resp.actor_name));
        }
        if !resp.warnings.is_empty() {
            out.push_str(&format!("\nWarnings: {}", resp.warnings.join("; ")));
        }
        Some(Ok(out))
    })
}

/// `complete` subcommand — declare the current pass finished on a forge artifact
/// through the direct-engine channel. Doer path (no `--satisfaction`) advances
/// spec→spec_review (and equivalent doer edges); reviewer paths
/// (`--satisfaction satisfied|full_revision|address_in_next_step`) drive the
/// review gates. Works without the anvil MCP server; against a Foundry-mode
/// engine it still needs a broker-minted credential (see `run_begin`).
fn run_complete(args: &[String]) -> ExitCode {
    finish_lifecycle("complete", call_engine_complete(args))
}

/// Call the engine's Complete RPC directly (mirrors `call_engine_begin`).
fn call_engine_complete(args: &[String]) -> Option<Result<String, String>> {
    use anvil_engine::proto::anvil_service_client::AnvilServiceClient;
    use anvil_engine::proto::CompleteRequest;

    let artifact_path = flag(args, "--artifact-path")?.to_string();
    if artifact_path.trim().is_empty() {
        return None;
    }
    let rt = block_runtime()?;
    let port = resolve_port(args);
    let hearth_path = resolve_gate_hearth(args, None)
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let project_root = std::env::current_dir()
        .ok()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let request = CompleteRequest {
        artifact_path,
        actor_name: flag(args, "--actor-name").unwrap_or("").to_string(),
        actor_type: flag(args, "--actor-type").unwrap_or("").to_string(),
        actor_model: flag(args, "--actor-model").unwrap_or("").to_string(),
        actor_provider: flag(args, "--actor-provider").unwrap_or("").to_string(),
        actor_context_window: int_flag(args, "--actor-context-window"),
        actor_sdk_version: flag(args, "--actor-sdk-version").unwrap_or("").to_string(),
        actor_entrypoint: flag(args, "--actor-entrypoint").unwrap_or("").to_string(),
        satisfaction: flag(args, "--satisfaction").unwrap_or("").to_string(),
        approver: flag(args, "--approver").unwrap_or("").to_string(),
        note: flag(args, "--note").unwrap_or("").to_string(),
        reflection_notes: flag(args, "--reflection-notes").unwrap_or("").to_string(),
        hearth_path,
        findings: flag(args, "--findings").unwrap_or("").to_string(),
        conversation_id: begin_conversation_id(args),
        project_root,
        claimed_evidence: collect_claimed_evidence(args),
    };
    let addr = format!("http://127.0.0.1:{}", port);
    rt.block_on(async move {
        let bearer = anvil_engine::kit_bearer::resolve_kit_bearer().await;
        let mut client = AnvilServiceClient::connect(addr).await.ok()?;
        let authed = match cli_request(request, &bearer) {
            Ok(r) => r,
            Err(e) => return Some(Err(format!("✗ anvil complete rejected: {e}"))),
        };
        let resp = match client.complete(authed).await {
            Ok(r) => r.into_inner(),
            Err(status) => {
                return Some(Err(format!(
                    "✗ anvil complete rejected: {} — fix the inputs and retry `anvil-hooks complete`.{}",
                    status.message(),
                    credential_note(status.message(), &bearer)
                )));
            }
        };
        let mut out = format!("✓ Completed → state `{}`", resp.new_state);
        if !resp.transition_at.is_empty() {
            out.push_str(&format!(" at {}", resp.transition_at));
        }
        if !resp.resolved_hearth.is_empty() {
            out.push_str(&format!(" [hearth {}]", resp.resolved_hearth));
        }
        if !resp.warnings.is_empty() {
            out.push_str(&format!("\nWarnings: {}", resp.warnings.join("; ")));
        }
        if !resp.carry_forward_path.is_empty() {
            out.push_str(&format!("\nCarry-forward: {}", resp.carry_forward_path));
        }
        if !resp.reflection_path.is_empty() {
            out.push_str(&format!("\nReflection: {}", resp.reflection_path));
        }
        if !resp.next_step.is_empty() {
            out.push_str(&format!("\n\nNext: {}", resp.next_step));
        }
        Some(Ok(out))
    })
}

/// `amend` subcommand — record a structured amendment op against a frozen artifact
/// document (and drive the `completed → amend` transition for a track) through the
/// RELIABLE direct-engine channel.
fn run_amend(args: &[String]) -> ExitCode {
    finish_lifecycle("amend", call_engine_amend(args))
}

/// Call the engine's Amend RPC directly (mirrors `call_engine_begin`).
fn call_engine_amend(args: &[String]) -> Option<Result<String, String>> {
    use anvil_engine::proto::anvil_service_client::AnvilServiceClient;
    use anvil_engine::proto::AmendRequest;

    let artifact_path = flag(args, "--artifact-path")?.to_string();
    if artifact_path.trim().is_empty() {
        return None;
    }
    let target_document = flag(args, "--target-document")?.to_string();
    if target_document.trim().is_empty() {
        return None;
    }
    let rt = block_runtime()?;
    let port = resolve_port(args);
    let hearth_path = resolve_gate_hearth(args, None)
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let request = AmendRequest {
        hearth_path,
        artifact_path,
        kind: flag(args, "--kind").unwrap_or("").to_string(),
        target_document,
        target_id: flag(args, "--target-id").unwrap_or("").to_string(),
        op_kind: flag(args, "--op-kind").unwrap_or("").to_string(),
        body: flag(args, "--body").unwrap_or("").to_string(),
        new_kind: flag(args, "--new-kind").unwrap_or("").to_string(),
        anchor: flag(args, "--anchor").unwrap_or("").to_string(),
        actor_name: flag(args, "--actor-name").unwrap_or("").to_string(),
        actor_type: flag(args, "--actor-type").unwrap_or("").to_string(),
        actor_model: flag(args, "--actor-model").unwrap_or("").to_string(),
        actor_provider: flag(args, "--actor-provider").unwrap_or("").to_string(),
        actor_context_window: int_flag(args, "--actor-context-window"),
        actor_sdk_version: flag(args, "--actor-sdk-version").unwrap_or("").to_string(),
        actor_entrypoint: flag(args, "--actor-entrypoint").unwrap_or("").to_string(),
    };
    let addr = format!("http://127.0.0.1:{}", port);
    rt.block_on(async move {
        let bearer = anvil_engine::kit_bearer::resolve_kit_bearer().await;
        let mut client = AnvilServiceClient::connect(addr).await.ok()?;
        let authed = match cli_request(request, &bearer) {
            Ok(r) => r,
            Err(e) => return Some(Err(format!("✗ anvil amend rejected: {e}"))),
        };
        let resp = match client.amend(authed).await {
            Ok(r) => r.into_inner(),
            Err(status) => {
                return Some(Err(format!(
                    "✗ anvil amend rejected: {} — fix the inputs and retry `anvil-hooks amend`.{}",
                    status.message(),
                    credential_note(status.message(), &bearer)
                )));
            }
        };
        let mut out = format!("✓ Amend recorded (op {})", resp.op_id);
        if !resp.new_state.is_empty() {
            out.push_str(&format!(" → state `{}`", resp.new_state));
        }
        Some(Ok(out))
    })
}

/// The fallible core of route-turn: resolve the message + hearth, call the engine
/// route RPC (time-boxed), and distill the guidance. Returns `None` on any gap so
/// the caller stays silent. NoMatch returns `Some("")` (silent but successful).
fn route_turn_guidance(args: &[String]) -> Option<String> {
    let Some(turn) = route_turn_input(args) else {
        diagnose("exit: route_turn_input returned None (stdin unparseable / no prompt field)");
        return None;
    };
    let message = turn.message;
    if message.trim().is_empty() {
        diagnose("exit: message empty after trim");
        return None;
    }
    let hearth = resolve_gate_hearth(args, None);
    // DURABLE hearth-local flags — merge them into THIS process's env BEFORE reading
    // `ANVIL_ABSTENTION_LEDGER` below. The abstention ledger is consumed HERE (the
    // `anvil-hooks route-turn` process), NOT in the engine, so the engine's own
    // startup `set_var` can never deliver the flag to us — we must merge our own from
    // the SAME hearth the ledger writes to (`<hearth>/engine-flags.env`), otherwise
    // the durable opt-in silently never reaches its consumer. This runs in the
    // single-threaded window (before `call_engine_route` builds any runtime), so the
    // `set_var` inside is sound. Precedence unchanged (real env > file > default);
    // fail-open on a missing hearth/file. Logging is skipped: route-turn is a
    // fail-silent advisory hook with no tracing subscriber.
    let _ = anvil_engine::engine_flags::install_hearth_local_flags(hearth.as_deref());
    let port = resolve_engine_port(args);
    // The originating harness id, written into the installed hook by the
    // per-harness install adapter (e.g. `route-turn --source claude-code`).
    // Absent/blank → ALWAYS tag the turn `(unattributed)` rather than an empty
    // source: an un-tagged turn must be bucketed explicitly, never silently
    // folded into a real harness or hearth. Fail-open is preserved upstream.
    let source = match flag(args, "--source") {
        Some(s) if !s.trim().is_empty() => s,
        _ => "(unattributed)",
    };

    // Extract by SHAPE here; the engine decides what it means. A turn the digest
    // would have cut is disqualified outright, using the flag recorded at cut
    // time rather than an ellipsis guessed at afterwards.
    let prior_proposal = anvil_core::domain::route::extract_prior_proposal(
        &turn.last_assistant_text,
        turn.last_assistant_truncated,
    );
    let Some(decision) = call_engine_route(
        &message,
        &turn.conversation_id,
        hearth.as_deref(),
        port,
        source,
        &turn.recent_context,
        prior_proposal,
    ) else {
        // The engine may have ROUTED AND RECORDED this turn and still land here: the
        // activity row is written engine-side before the reply is decoded client-side.
        // That asymmetry is exactly how a delivery outage hides behind a healthy log.
        diagnose(&format!(
            "exit: call_engine_route returned None (engine unreachable on port {port}, or reply undecodable)              — hearth={hearth:?}"
        ));
        // RECORD IT. This early return is where the 1500ms-cap outage actually lived, and
        // the delivery log did NOT cover it: `record_delivery` sits downstream, so a total
        // outage produced an EMPTY log — indistinguishable from "no turns happened".
        // Measured on the LIVE hearth 2026-07-28: the delivery log held exactly ONE
        // record, and that one was a manual probe, while thousands of real turns timed out
        // unrecorded.
        //
        // An instrument blind to the failure it exists to catch is worse than none: it
        // reports silence as health. Same defect class as everything else in this track.
        //
        // There is no engine answer here, so there is no hash and no kind. Both
        // are passed EMPTY and the projection turns the empty hash into the
        // sentinel — the hook does not name the sentinel itself, because the
        // fold's unjoinable bucket keys off that exact string and a hand-typed
        // copy that drifts turns an unjoinable row into a joinable-looking one.
        record_delivery(hearth.as_deref(), source, "", "", 0, false, 0, "");
        return None;
    };
    // LIVE ROUTER: the engine supplies the access-scoped GRANTED candidate set;
    // Kiln is the only selector. If Kiln cannot select, the hook stays silent.
    // context_aware_routing: the transcript-derived recent context + in-progress
    // signal steer the selector's pick/abstain (see route_with_llm).
    // resume-signal context-awareness: the engine may have surfaced a PARK HINT
    // for an open playbook the agent moved on from. It rides ALONGSIDE the normal
    // turn guidance (not a RouteTurnOutcome), so lift it out before the decision
    // is consumed and append its rendering below — otherwise the affordance would
    // never reach the user (Codex: the hook must intentionally show park_hint).
    let park_hint = decision
        .park_hint
        .as_ref()
        .map(|p| format_park_hint(&p.artifact_id, &p.kind, &p.state, &p.park_action));
    // Abstention ledger (opt-in, local-only): capture the router's turn-relevant
    // matching kinds BEFORE `route_with_llm` consumes `decision`, so an abstention
    // can be recorded with the candidate set the router actually saw.
    let candidate_set: Vec<String> = decision
        .matching_candidates
        .iter()
        .map(|c| c.kind.clone())
        .collect();
    let candidate_count = candidate_set.len();
    // Lift the engine's reason out BEFORE `route_with_llm` consumes the decision —
    // the same reason the park hint is lifted here. A value read after a move is a
    // compile error; a value never read at all is the silent half-chain this phase
    // exists to prevent.
    let resume_source = decision.resume_source.clone();
    // The engine's hash, lifted for the SAME reason and at the SAME point. A
    // value read after a move is a compile error; a value never read at all is
    // the silent half-chain, and this one is the join key itself.
    let conversation_hash = decision.conversation_hash.clone();
    let outcome = route_with_llm(decision, &message, &turn.recent_context, &turn.in_progress);
    // On the FINAL abstention (NoMatch) AND opt-in on, append one conversation-aware
    // record to the durable ledger. Fail-open: never breaks the advisory hook. The
    // enabled check gates record construction so the default-OFF common path is free.
    if abstention_ledger::abstention_ledger_enabled() {
        let record = abstention_ledger::AbstentionRecord {
            message: message.clone(),
            recent_context: turn.recent_context.clone(),
            conversation_id: turn.conversation_id.clone(),
            candidate_set,
            at: abstention_ledger::now_rfc3339(),
            source: source.to_string(),
        };
        abstention_ledger::record_abstention(true, &outcome, hearth.as_deref(), &record);
    }
    let mut guidance = format_guidance(&outcome, &turn.conversation_id);
    if let Some(hint) = park_hint.filter(|h| !h.is_empty()) {
        if guidance.is_empty() {
            guidance = hint;
        } else {
            guidance = format!("{guidance}\n\n{hint}");
        }
    }
    record_delivery(
        hearth.as_deref(),
        source,
        &conversation_hash,
        // WHICH kind this process wrote to stdout — the left-hand leg of the
        // delivered-kind→begin join. One rule, in `anvil-core`, applied to the
        // outcome the hook actually rendered: a menu and a no-match carry no
        // kind, and fabricating one from a menu would make the suggestion
        // denominator larger than the set of turns anything was suggested on.
        anvil_core::domain::hooks::route_turn::guidance_kind_of(&outcome),
        candidate_count,
        !guidance.is_empty(),
        guidance.len(),
        &resume_source,
    );
    // THE NOTICE, and it is deliberately AFTER the record above.
    //
    // Fail-open says a hook never breaks a turn, so "the lexical fallback returns
    // an error" can only mean the reader is TOLD — on the channel the guidance
    // already uses. But the notice is NOT guidance: appended before the record it
    // would flip `guidance_produced` true on every degraded turn and inflate the
    // delivered denominator with turns that suggested nothing, corrupting the very
    // sink this change extends.
    let cause = slot(&LAST_ROUTER_CAUSE);
    if cause.is_empty() {
        return Some(guidance);
    }
    let notice = anvil_core::domain::hooks::router_degradation::degradation_notice(
        &cause,
        slot(&LAST_ABSTAIN) == "fallback_degraded_to_lexical",
    );
    Some(if guidance.is_empty() {
        notice
    } else {
        format!("{guidance}\n\n{notice}")
    })
}

struct RouteTurnDecision {
    /// Resume is not fresh playbook selection; preserve the open-playbook bridge.
    resume_outcome: Option<RouteTurnOutcome>,
    /// Did the ENGINE match anything? The engine signals "this turn has no task" by
    /// abstaining to an EMPTY matching set, and that signal is what keeps the hook silent
    /// on system-reminder / no-task turns. It MUST stay separate from the selection menu
    /// below: widening the menu to the granted set made `matching_candidates` never empty
    /// and silently defeated the no-task gate, which the
    /// `a no-task system-reminder turn leaves the hook silent` scenario caught.
    engine_matched: bool,
    /// The engine's capped, turn-relevant MATCHING set (`response.matching_candidates`,
    /// ≤3) — NOT the full access-scoped granted flood (~12 kinds). This is the only
    /// set the live Kiln router may select from (router_precision fix #2). Empty when
    /// the engine abstained (no_match / no-task), which the hook renders as silence.
    matching_candidates: Vec<CandidateBrief>,
    /// resume-signal context-awareness — the engine's PARK HINT for an open
    /// playbook the agent has moved on from (surfaced once). Rendered ALONGSIDE
    /// the normal turn guidance so the affordance reaches the user, not just the
    /// RPC. `None` on every turn the engine did not surface a park hint.
    park_hint: Option<ParkHintWire>,
    /// continuation_recognition — the engine's reason for the path it took, carried
    /// verbatim to `delivery-log.jsonl`. Set INDEPENDENTLY of the outcome: a
    /// rejection that then routes successfully must record both the delivered
    /// route AND why the contextual path declined, and deriving one from the other
    /// would lose exactly that case. Empty on every non-continuation turn.
    resume_source: String,
    /// The ENGINE's salted conversation hash for this turn, carried verbatim to
    /// `delivery-log.jsonl`. The hook NEVER computes one: 19 `.telemetry-salt`
    /// files holding 6 distinct values exist across the fleet, so a second
    /// hasher lands in a keyspace disjoint from the engine rows this row is
    /// meant to join to — and that failure presents as a low coverage number
    /// with no visible cause, which is the exact defect this track exists to
    /// end. Empty when the engine had no salt or no conversation id; the
    /// projection turns that into the sentinel, never into a raw id.
    conversation_hash: String,
}

/// The distilled park-hint the engine surfaced on this turn (resume-signal
/// context-awareness). Mirrors the proto `ParkHint` message.
struct ParkHintWire {
    artifact_id: String,
    kind: String,
    state: String,
    park_action: String,
}

/// Record an abstention AND make it observable.
///
/// Every branch below already tagged its reason via `telemetry::record_abstention`, but
/// in the `anvil-hooks` process `telemetry::recorder()` is `None`, so all four reasons
/// were discarded. That is what made the 2026-07-27/28 delivery outage undetectable: the
/// hook fell open to silence, exited 0, wrote nothing to either stream, and the engine's
/// own activity row still read `single`. A total delivery outage and a correctly
/// abstaining router were byte-identical at every instrument we had.
///
/// Fail-open keeps the turn fast; it must not keep the turn silent about being silent.
/// `ANVIL_HOOK_DIAGNOSE=1` prints the reason to STDERR — never stdout, which stays
/// reserved for injected guidance so the hook's contract is unchanged when the flag is
/// off (the default).
/// Emit a diagnostic to STDERR when `ANVIL_HOOK_DIAGNOSE` is set. Never stdout: stdout
/// is reserved for injected guidance, so behavior with the flag off is unchanged.
fn diagnose(msg: &str) {
    if std::env::var("ANVIL_HOOK_DIAGNOSE").is_ok_and(|v| !v.trim().is_empty() && v != "0") {
        eprintln!("anvil-hooks: {msg}");
    }
}

/// The last abstention reason for THIS process, so the delivery record below can name
/// why nothing was delivered. `anvil-hooks route-turn` is a short-lived, single-decision
/// CLI process, so one slot is exactly the right cardinality.
static LAST_ABSTAIN: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// The classified cause of THIS process's routing degradation — set only when the
/// router was CALLED and produced no verdict. One slot for the same reason as
/// `LAST_ABSTAIN`: route-turn is a short-lived single-decision process.
///
/// Empty when no call was made, which is what keeps a kill-switched router from
/// nagging every turn: a configuration is not a degradation, and a turn with no
/// attempt has no cause to report.
static LAST_ROUTER_CAUSE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Read a process-local slot, or the empty string.
fn slot(cell: &std::sync::Mutex<Option<String>>) -> String {
    cell.lock().ok().and_then(|s| s.clone()).unwrap_or_default()
}

/// Whether an ABSENT router verdict may degrade to the lexical pick. Default OFF: see
/// the `RouterVerdict::Fallback` arm for why this is measured before it is enabled.
fn degrade_on_fallback() -> bool {
    std::env::var("ANVIL_HOOK_DEGRADE_ON_FALLBACK")
        .is_ok_and(|v| !v.trim().is_empty() && v != "0")
}

/// Record WHY the route produced nothing, when the cause is infrastructure rather
/// than a judgement. Deliberately NOT `abstain()`: that one also files a telemetry
/// abstention, and an engine that was not running did not abstain — folding the two
/// together is how `engine_call_failed` came to mean three different things at once.
///
/// The three causes need different fixes and were indistinguishable in the delivery
/// log: connect-refused says the engine is ABSENT (fix availability), a route error
/// says it answered badly (fix the RPC), and a deadline says it was too SLOW (fix
/// latency, or the cap). The code comment at the classification site guessed the cap
/// was "the common one"; the delivery log disagrees — failures arrive in bursts with
/// zero interleaving against successes, which is the shape of absence, not slowness.
fn record_route_failure(reason: &str) {
    diagnose(&format!("route failed ({reason})"));
    if let Ok(mut slot) = LAST_ABSTAIN.lock() {
        *slot = Some(reason.to_string());
    }
}

fn abstain(reason: &str) {
    telemetry::record_abstention(reason);
    diagnose(&format!("abstained ({reason})"));
    if let Ok(mut slot) = LAST_ABSTAIN.lock() {
        *slot = Some(reason.to_string());
    }
}

/// Append the DELIVERED decision to `<hearth>/delivery-log.jsonl`.
///
/// The activity log records the ENGINE's answer, written engine-side before the hook
/// re-decides via Kiln. When Kiln overrules it — or fails — the engine row still reads
/// `single` and nothing is delivered. That asymmetry is why adoption and coverage
/// numbers described a decision that never reached a model: on 2026-07-28 the engine
/// logged 56 `single` + 15 `candidates` from claude-code while zero guidance was
/// attached to any transcript.
///
/// NAMING, deliberately narrow: the field is `guidance_produced`, NOT `delivered`. This
/// record is written BEFORE `println!` and long before Claude Code consumes stdout, so it
/// cannot witness delivery. It can report true while stdout writing fails, while the
/// harness ignores the output for that event, or while the harness records the output and
/// still does not place it in the next model request.
///
/// An earlier version called this `delivered` and asserted in a comment that it "cannot
/// drift from what the harness got". That was false, and it was the same defect this
/// whole track exists to fix: a name claiming more than the measurement supports. The
/// producer side is all this file can honestly see; the consumer side is
/// `scripts/verify_hook_delivery.sh`.
///
/// Best-effort and fail-open: a hook must never break a turn to record one. That means a
/// failed append is itself unrecorded — a known limit of this sink, not a guarantee.
fn record_delivery(
    hearth: Option<&std::path::Path>,
    source: &str,
    // The ENGINE's hash, verbatim, or empty when there was no answer. NOT a raw
    // conversation id: this sink no longer has a field for one, and the removal
    // is the point — keeping the raw id beside the hash would leave the raw
    // field as the easy join key and the sink in violation of the standing
    // no-raw-identities constraint.
    conversation_hash: &str,
    guidance_kind: &str,
    engine_candidates: usize,
    guidance_produced: bool,
    guidance_bytes: usize,
    resume_source: &str,
) {
    let Some(hearth) = hearth else { return };
    // A recorded reason wins even when we DID deliver: a `fallback_degraded_to_lexical`
    // hit is a delivery the selector never actually judged, and folding it into a plain
    // `guidance_produced` would hide exactly the rate we need in order to know how often
    // the selector is absent. Only an unqualified production reports as bare
    // `guidance_produced`.
    // A rejection names itself, ahead of any abstention reason: "the contextual
    // path declined because the human said no" is a different fact from "no
    // candidate matched", and folding the first into the second is what made the
    // accepted losses uncountable in the first place.
    let recorded = if resume_source == "rejected" {
        Some("continuation_rejected".to_string())
    } else {
        LAST_ABSTAIN.lock().ok().and_then(|slot| slot.clone())
    };
    let reason = match (guidance_produced, recorded) {
        (_, Some(r)) => r,
        (true, None) => "guidance_produced".to_string(),
        // Nothing produced and nothing recorded. The three KNOWN causes now name
        // themselves (engine_unreachable / engine_timeout / engine_rpc_error) via
        // record_route_failure, so anything still landing here is an exit path we have
        // not accounted for — which is worth knowing precisely because it is unlabelled.
        (false, None) => "engine_call_failed_unclassified".to_string(),
    };
    // The RAW project root, read here and never persisted: the projection below
    // reduces it to a basename, and the record has no field for a path to
    // survive into.
    let project_root = std::env::current_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    let observation = DeliveryObservation {
        at: &abstention_ledger::now_rfc3339(),
        source,
        project_root: &project_root,
        engine_conversation_hash: conversation_hash,
        guidance_kind,
        engine_candidates: engine_candidates as u64,
        guidance_produced,
        guidance_bytes: guidance_bytes as u64,
        outcome: &reason,
        // WHY this turn resolved as it did, straight from the engine. Set
        // INDEPENDENTLY of `outcome`: the two answer different questions (what
        // was delivered vs which path decided it), and deriving either from the
        // other is how a rejection that then routes successfully would go
        // uncounted.
        resume_source,
        // WHY the selector was absent. Set INDEPENDENTLY of `outcome`, which
        // answers what the turn PRODUCED: a turn can degrade to the lexical pick
        // and still produce guidance, and that disagreement is the row worth
        // counting.
        router_cause: &slot(&LAST_ROUTER_CAUSE),
    };
    // ONE constructor, in `anvil-core`, and it is the whole redaction contract:
    // the sentinel for an absent engine hash, the basename for the raw root,
    // the empty kind for a non-delivering turn. `DeliveryLogRecord` is
    // `#[non_exhaustive]`, so this crate CANNOT hand-assemble one — the rule
    // that the hook must not bypass the projection is a compile error, not a
    // review convention.
    //
    // The line-atomic single-`write_all` append (the glued-JSON defect observed
    // on live line 110, and already fixed twice in this repository) now lives in
    // the adapter, which is where every other sink keeps it.
    //
    // Best-effort and fail-open, unchanged: the append error is dropped on
    // purpose. A hook must never break a turn to record one.
    let record = project_delivery_record(&observation);
    let _ = FileSystemDeliveryLogAdapter::new(hearth).append_delivery_log(&record);
}

fn route_with_llm(
    decision: RouteTurnDecision,
    message: &str,
    recent_context: &str,
    in_progress: &InProgressSignal,
) -> RouteTurnOutcome {
    // Resume is not fresh playbook selection. Preserve the open-playbook bridge.
    if let Some(outcome) = decision.resume_outcome {
        return outcome;
    }
    // Gate on the ENGINE's answer, never on the menu — the menu may be widened.
    if !decision.engine_matched || decision.matching_candidates.is_empty() {
        abstain("no_candidate");
        return RouteTurnOutcome::NoMatch;
    }

    // The single-tier router: ONE Kiln→Fireworks call, returning the pure verdict +
    // per-call telemetry. Port / model / bearer / timeout are resolved from the
    // environment (and `~/.anvil/router.json`) inside `route`. There is NO fallback
    // backend — a missed/slow/unparseable call fails open to NoMatch.
    let route_outcome = kiln_router::route(
        message,
        recent_context,
        in_progress,
        &decision.matching_candidates,
        "hook",
    );
    // Surface the per-call telemetry (served_by + the attempt's outcome, latency) so
    // hit-rate vs fail-open is observable. Log to STDERR (best-effort) — stdout is
    // reserved for the injected guidance and the hook must stay fail-open.
    log_tier_telemetry(&route_outcome);
    // Unified content-free route-decision rollup (best-effort, opt-in gated): the
    // outcome bucket, the target tier, and the call latency — never the message,
    // the candidates, or any id. Emitted alongside the legacy stderr/jsonl line.
    let decision_outcome = match &route_outcome.verdict {
        RouterVerdict::Pick { .. } => "routed",
        RouterVerdict::Abstain => "abstained",
        RouterVerdict::Fallback => "fallback",
    };
    // An ABSENT verdict from a call that was actually made carries a cause; record
    // it before the verdict is consumed. `attempts` is empty when routing is off,
    // and that emptiness IS the "no notice for a kill switch" rule.
    if matches!(route_outcome.verdict, RouterVerdict::Fallback) {
        if let (Some(attempt), Ok(mut cell)) =
            (route_outcome.attempts.first(), LAST_ROUTER_CAUSE.lock())
        {
            *cell = Some(attempt.outcome.clone());
        }
    }
    let target_tier = route_outcome.served_by.as_deref().unwrap_or("none");
    let latency_ms = route_outcome.attempts.first().map(|a| a.elapsed_ms as f64);
    telemetry::record_route_decision(None, decision_outcome, target_tier, None, latency_ms);
    match route_outcome.verdict {
        RouterVerdict::Pick { kind, why } => {
            let kind = kind.trim();
            match decision
                .matching_candidates
                .iter()
                .find(|candidate| candidate.kind == kind)
            {
                Some(chosen) => RouteTurnOutcome::Single {
                    kind: chosen.kind.clone(),
                    description: chosen.description.clone(),
                    why: why.trim().to_string(),
                    required_fields: chosen.required_fields.clone(),
                    guidance: String::new(),
                },
                None => {
                    abstain("no_match");
                    RouteTurnOutcome::NoMatch
                }
            }
        }
        RouterVerdict::Abstain => {
            abstain("router_abstain");
            RouteTurnOutcome::NoMatch
        }
        // `Fallback` is NOT the router saying "no" — it is the router failing to answer
        // (timeout / transport / unparseable reply). `kiln_router` documents this case as
        // "the lexical `resolution` UNCHANGED (fail-open)", but dropping to `NoMatch`
        // discarded the engine's lexical pick instead, converting an available answer
        // into silence. That is the difference between a selector that declined and a
        // selector that was absent, and the two must not share an outcome.
        //
        // An explicit `Abstain` is still honored — the model said no and we respect it.
        // Only an ABSENT verdict degrades, and only to what the lexical stage already
        // resolved, which is exactly the behavior that shipped before the semantic
        // cutover. A single unambiguous candidate degrades; a multi-candidate set does
        // not, because picking among them is precisely the judgment we just failed to
        // obtain.
        //
        // GATED DEFAULT-OFF, deliberately. Degrading is a BEHAVIOR change whose benefit
        // is unmeasured and whose risk is documented: `Fallback` is overwhelmingly
        // load-induced (Kiln measured at 2.7-6.6s against a 12s cap; a loaded box pushes
        // it past), and falling back to lexical under load is the known over-match mode.
        // Observed directly while building this: three consecutive turns fell back at
        // load ~280 and the same turn abstained cleanly at load ~17.
        //
        // The delivery log now MEASURES how often this fires. Decide with that data, not
        // with this comment. Flip `ANVIL_HOOK_DEGRADE_ON_FALLBACK=1` to evaluate the arm.
        RouterVerdict::Fallback if !degrade_on_fallback() => {
            abstain("fallback");
            RouteTurnOutcome::NoMatch
        }
        RouterVerdict::Fallback => match decision.matching_candidates.as_slice() {
            [only] => {
                abstain("fallback_degraded_to_lexical");
                RouteTurnOutcome::Single {
                    kind: only.kind.clone(),
                    description: only.description.clone(),
                    why: String::new(),
                    required_fields: only.required_fields.clone(),
                    guidance: String::new(),
                }
            }
            _ => {
                abstain("fallback");
                RouteTurnOutcome::NoMatch
            }
        },
    }
}

/// Emit the per-call router telemetry as a single compact JSON line on stderr
/// (`served_by` + the attempt's `{tier, outcome, elapsed_ms}`) AND append the same line
/// to `~/.anvil/router-telemetry.jsonl` for watching. Best-effort and side-effect-free
/// on stdout so the fail-open turn contract is preserved.
///
/// Serialized via a struct (not `json!`) so the field order is declaration order —
/// `serde_json::Value` would otherwise sort keys alphabetically.
fn log_tier_telemetry(outcome: &kiln_router::RouteOutcome) {
    #[derive(serde::Serialize)]
    struct TelemetryRecord<'a> {
        event: &'static str,
        served_by: &'a str,
        attempts: &'a [kiln_router::RouterAttempt],
    }
    let record = TelemetryRecord {
        event: "router_tier_attempts",
        served_by: outcome.served_by.as_deref().unwrap_or("none"),
        attempts: &outcome.attempts,
    };
    if let Ok(line) = serde_json::to_string(&record) {
        eprintln!("{}", line);
        append_tier_telemetry(&line);
    }
}

/// Best-effort append of one telemetry JSON line to the router telemetry log
/// (`ANVIL_ROUTER_TELEMETRY_FILE` when set, else `~/.anvil/router-telemetry.jsonl`),
/// creating the directory if needed. FULLY FAIL-OPEN: any error (no HOME, mkdir/open/
/// write failure) is swallowed so the turn is NEVER blocked or errored. The stderr
/// emit is the primary sink; this durable append is purely additive observability that
/// `scripts/router-telemetry-watch.py` summarizes.
fn append_tier_telemetry(line: &str) {
    use std::io::Write;
    let path = match std::env::var_os("ANVIL_ROUTER_TELEMETRY_FILE") {
        Some(p) if !p.is_empty() => std::path::PathBuf::from(p),
        _ => {
            let Some(home) = std::env::var_os("HOME") else {
                return;
            };
            std::path::PathBuf::from(home)
                .join(".anvil")
                .join("router-telemetry.jsonl")
        }
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{}", line);
    }
}

/// The resolved route-turn input: the user message plus the conversation/session
/// id (resume_aware_routing H2). The conversation id is empty when the harness
/// supplies none — resume simply never fires for that turn (degrade gracefully).
struct RouteTurnInput {
    message: String,
    conversation_id: String,
    /// context_aware_routing: a compact digest of recent turns distilled from the
    /// harness transcript tail. Empty when no transcript is available.
    recent_context: String,
    /// context_aware_routing: the transcript-grounded in-progress playbook signal.
    in_progress: InProgressSignal,
    /// continuation_recognition: the last assistant turn, untruncated, and
    /// whether the digest cap would have cut it — both decided at cut time.
    last_assistant_text: String,
    last_assistant_truncated: bool,
}

/// Resolve the user message + conversation id: the explicit `--message` flag
/// (tests/harnesses that pass it directly) wins, with an optional
/// `--conversation-id` flag alongside it; otherwise parse the harness's
/// user-prompt JSON on stdin (Claude Code UserPromptSubmit / Hermes pre_llm_call
/// shapes), extracting BOTH the message and the conversation/session id. The id
/// is what lets the engine bridge a continuation message ("go") to this
/// conversation's open playbook — without it the engine pre-check always gets
/// empty and never resumes (H2).
fn route_turn_input(args: &[String]) -> Option<RouteTurnInput> {
    if let Some(m) = flag(args, "--message") {
        // context_aware_routing: an explicit `--transcript <path>` lets a harness
        // (or test) point the flag path at a transcript fixture; absent → no
        // context (routing unchanged).
        let context = flag(args, "--transcript")
            .and_then(|p| read_transcript_tail(Path::new(p)))
            .map(|tail| extract_transcript_context(&tail))
            .unwrap_or_default();
        return Some(RouteTurnInput {
            message: m.to_string(),
            conversation_id: flag(args, "--conversation-id")
                .map(|s| s.to_string())
                .unwrap_or_default(),
            recent_context: context.recent_context,
            last_assistant_text: context.last_assistant_text.clone(),
            last_assistant_truncated: context.last_assistant_truncated,
            in_progress: context.in_progress,
        });
    }
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() || buf.trim().is_empty() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_str(&buf).ok()?;
    let message = extract_user_message(&json)?;
    let conversation_id = extract_conversation_id(&json).unwrap_or_default();
    // context_aware_routing: read the (bounded) transcript tail and distill the
    // recent-context digest + in-progress signal. Any gap (no path, unreadable,
    // unparseable) degrades to an empty context so routing is unchanged.
    let context = extract_transcript_path(&json)
        .and_then(|p| read_transcript_tail(Path::new(&p)))
        .map(|tail| extract_transcript_context(&tail))
        .unwrap_or_default();
    Some(RouteTurnInput {
        message,
        conversation_id,
        recent_context: context.recent_context,
        last_assistant_text: context.last_assistant_text.clone(),
        last_assistant_truncated: context.last_assistant_truncated,
        in_progress: context.in_progress,
    })
}

/// Read the tail of a harness transcript file for context_aware_routing: at most
/// the last [`TRANSCRIPT_TAIL_BYTES`] and the last [`TRANSCRIPT_TAIL_LINES`]
/// lines (the hook is time-boxed — a bounded read stays cheap). Seeks from the end
/// so a large transcript never loads in full. `None` on any I/O gap (fail-open).
fn read_transcript_tail(path: &Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(TRANSCRIPT_TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    // When we seeked past the file head, drop the (likely partial) first line.
    let text: &str = if start > 0 {
        text.split_once('\n').map(|(_, rest)| rest).unwrap_or("")
    } else {
        &text
    };
    let lines: Vec<&str> = text.lines().collect();
    let tail_start = lines.len().saturating_sub(TRANSCRIPT_TAIL_LINES);
    Some(lines[tail_start..].join("\n"))
}

/// Dial the engine and call `route`, time-boxed, returning the distilled outcome.
/// Drives a dedicated current-thread tokio runtime so this sync binary can make
/// the async gRPC call. Any transport/timeout/error → `None` (fail open).
fn call_engine_route(
    message: &str,
    conversation_id: &str,
    hearth: Option<&Path>,
    port: u16,
    source: &str,
    // continuation_recognition: what only the hook has. Both are optional and
    // absent on transcript-less surfaces (codex, kiln, hermes), which is what
    // keeps those byte-identical to today.
    recent_context: &str,
    prior_proposal: Option<anvil_core::domain::route::PriorProposal>,
) -> Option<RouteTurnDecision> {
    use anvil_engine::proto::anvil_service_client::AnvilServiceClient;
    use anvil_engine::proto::RouteRequest;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;

    let hearth_path = hearth.map(|p| p.display().to_string()).unwrap_or_default();
    let message = message.to_string();
    // resume_aware_routing H2 — carry the conversation/session id so the engine's
    // resume pre-check can find this conversation's open playbook. Empty when the
    // harness supplied none (resume never fires; normal routing unchanged).
    let conversation_id = conversation_id.to_string();
    let source = source.to_string();
    let project_root = std::env::current_dir()
        .ok()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    let addr = format!("http://127.0.0.1:{}", port);

    rt.block_on(async move {
        let fut = async {
            let bearer = anvil_engine::kit_bearer::resolve_kit_bearer().await;
            let mut client = match AnvilServiceClient::connect(addr.clone()).await {
                Ok(c) => c,
                Err(e) => {
                    diagnose(&format!("connect FAILED to {addr}: {e}"));
                    record_route_failure("engine_unreachable");
                    return None;
                }
            };
            let request = tonic::Request::new(RouteRequest {
                hearth_path,
                message,
                conversation_id,
                source,
                project_root,
                recent_context: recent_context.to_string(),
                prior_proposal: prior_proposal.map(|p| {
                    anvil_engine::proto::PriorProposal {
                        text: p.text,
                        was_truncated: p.was_truncated,
                    }
                }),
                ..Default::default()
            });
            // Route is a gated RPC too. Without a bearer every turn under a
            // Foundry-mode engine takes the fail-open arm and silently delivers
            // no routing guidance at all — indistinguishable from "no match".
            let request = match anvil_engine::kit_bearer::attach_kit_bearer(request, &bearer) {
                Ok(r) => r,
                Err(e) => {
                    diagnose(&format!("route bearer attach FAILED: {e}"));
                    record_route_failure("engine_rpc_error");
                    return None;
                }
            };
            let response = match client.route(request).await {
                Ok(r) => r.into_inner(),
                Err(e) => {
                    diagnose(&format!("route RPC FAILED: code={:?} msg={}", e.code(), e.message()));
                    // tonic reports a dead/refusing server as Unavailable — that is an
                    // ABSENT engine wearing an RPC error's clothes, and counting it as a
                    // bad reply would send us hunting a protocol bug that is not there.
                    record_route_failure(match e.code() {
                        tonic::Code::Unavailable => "engine_unreachable",
                        tonic::Code::DeadlineExceeded => "engine_timeout",
                        _ => "engine_rpc_error",
                    });
                    return None;
                }
            };
            // router_precision fix #2: carry ONLY the engine's capped, turn-relevant
            // MATCHING set to Kiln — not the full access-scoped granted flood (~12
            // kinds). resolve_route already narrowed to the ≤3 relevant candidates
            // (and abstained to an empty matching set on no-task turns), so Kiln
            // selects from that short menu or abstains, instead of being handed a
            // dozen irrelevant options every turn. Each matching kind is looked up in
            // the full candidate list for its metadata (description / required_fields
            // / intent / step_outline / why_fits), preserving the engine's ranking.
            // The pure prompt builder still applies the route-description cap.
            // WIDE-CANDIDATE ARM (`ANVIL_ROUTER_WIDE_CANDIDATES`, default OFF).
            //
            // Winner-take-all lexical narrowing collapses `matching_candidates` to ONE
            // kind, so the router is asked "does this one fit? yes/no" instead of "which
            // of these fits?". Measured 2026-07-28: the turn "please research sources and
            // compile a topic reference for me" was shown ONLY `lore_source_research`
            // (whose brief says NOT for general web research) and correctly abstained —
            // while the same turn shown the full granted set routes to `compile_topic`.
            // A correct abstention over a wrong menu still delivers the user nothing.
            //
            // This arm changes ONLY the menu the router chooses among, not whether it is
            // consulted: the empty-`matching_candidates` gate upstream is untouched, so
            // the variable under test is isolated to candidate breadth.
            //
            // Default OFF because breadth is NOT a free parameter — the V2 arm measured
            // capping briefs as a REAL NEGATIVE (-2.3 hits), so widening must be measured
            // on frozen-H before it ships, not reasoned about.
            // DEFAULT ON since 2026-07-28. Opt out with ANVIL_ROUTER_WIDE_CANDIDATES=0.
            // Measured N=4 seeds x 40 frozen-H rows: +4, +4, +8, +5 hits (mean +5.25)
            // against a control whose hit count was EXACTLY 6 on all four seeds — zero
            // spread — so the worst seed is 4x the control's entire observed variation.
            // Delivery roughly doubles and should-have-routed-but-silent halves (8-15 ->
            // 3-6), at a cost of +0..2 false routes per 40 rows. ~+32 scaled to the
            // >=24/247 bar the V1 arm failed.
            let wide = std::env::var("ANVIL_ROUTER_WIDE_CANDIDATES")
                .map(|v| !(v.trim() == "0" || v.trim().eq_ignore_ascii_case("off")))
                .unwrap_or(true);
            let engine_matched = !response.matching_candidates.is_empty();
            let selection_kinds: Vec<String> = if wide && engine_matched {
                response.candidates.iter().map(|c| c.kind.clone()).collect()
            } else {
                response.matching_candidates.clone()
            };
            let matching_candidates: Vec<CandidateBrief> = selection_kinds
                .iter()
                .filter_map(|kind| {
                    response
                        .candidates
                        .iter()
                        .find(|c| &c.kind == kind)
                        .map(|c| CandidateBrief {
                            kind: c.kind.clone(),
                            description: c.description.trim().to_string(),
                            route_triggers: c.route_triggers.clone(),
                            required_fields: c.required_fields.clone(),
                            intent: c.intent.clone(),
                            step_outline: c.step_outline.clone(),
                            why_fits: c.why_fits.clone(),
                        })
                })
                .collect();
            let resume_outcome = if response.resolution_outcome == "resume"
                && !response.resume_artifact_id.trim().is_empty()
            {
                Some(RouteTurnOutcome::Resume {
                    artifact_id: response.resume_artifact_id.clone(),
                    kind: response.resume_kind.clone(),
                    state: response.resume_state.clone(),
                    guidance: response.resume_guidance.clone(),
                    advance_action: response.resume_advance_action.clone(),
                })
            } else {
                None
            };
            let park_hint = response.park_hint.as_ref().map(|p| ParkHintWire {
                artifact_id: p.artifact_id.clone(),
                kind: p.kind.clone(),
                state: p.state.clone(),
                park_action: p.park_action.clone(),
            });
            Some(RouteTurnDecision {
                resume_outcome,
                engine_matched,
                matching_candidates,
                park_hint,
                resume_source: response.resume_source.clone(),
                conversation_hash: response.conversation_hash.clone(),
            })
        };
        match tokio::time::timeout(std::time::Duration::from_millis(route_turn_timeout_ms()), fut)
            .await
        {
            Ok(outcome) => outcome,
            Err(_) => {
                // The silence that hid the outage. Fail-open still, but never unlogged.
                diagnose(&format!(
                    "route RPC TIMED OUT after {}ms — failing open to silence",
                    route_turn_timeout_ms()
                ));
                record_route_failure("engine_timeout");
                None
            }
        }
    })
}

/// ALLOW signal: exit 0, no output (Claude Code PreToolUse treats this as proceed).
fn allow() -> ExitCode {
    ExitCode::SUCCESS
}

/// BLOCK signal: emit the harness's block decision JSON on stdout + a reason on
/// stderr, and exit 2 (Claude Code PreToolUse: non-zero/decision = refuse).
fn block(edited_path: &Path) -> ExitCode {
    let reason = format!(
        "anvil: editing {} requires an open begin session for this artifact (run begin first).",
        edited_path.display()
    );
    // Claude Code PreToolUse decision shape.
    let decision = serde_json::json!({
        "decision": "block",
        "reason": reason,
    });
    println!("{}", decision);
    eprintln!("{}", reason);
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn flag_extracts_value_and_handles_absence() {
        let args = argv(&["--to-state", "plan_review", "--actor-role", "plan"]);
        assert_eq!(flag(&args, "--to-state"), Some("plan_review"));
        assert_eq!(flag(&args, "--actor-role"), Some("plan"));
        assert_eq!(flag(&args, "--missing"), None);
    }

    #[test]
    fn int_flag_parses_or_defaults_zero() {
        let args = argv(&["--actor-context-window", "200000"]);
        assert_eq!(int_flag(&args, "--actor-context-window"), 200_000);
        // absent → 0; unparsable → 0
        assert_eq!(int_flag(&args, "--nope"), 0);
        let bad = argv(&["--actor-context-window", "notanumber"]);
        assert_eq!(int_flag(&bad, "--actor-context-window"), 0);
    }

    #[test]
    fn resolve_port_prefers_flag_else_default() {
        assert_eq!(resolve_port(&argv(&["--port", "50099"])), 50099);
        assert_eq!(
            resolve_port(&argv(&["--actor-name", "X"])),
            DEFAULT_ENGINE_PORT
        );
        // non-numeric → default (never panics)
        assert_eq!(resolve_port(&argv(&["--port", "abc"])), DEFAULT_ENGINE_PORT);
    }

    #[test]
    fn resolve_engine_port_prefers_explicit_flag_over_discovery() {
        // An explicit --port is the strongest intent and MUST win over
        // rendezvous/health discovery — otherwise a live engine on the default
        // endpoint captures a call meant for another port (the bug that made the
        // anvil-hooks e2e fail whenever a :50051 engine was running). The
        // flag-present path returns before any discovery I/O, so this is
        // deterministic regardless of ANVIL_ENGINE_URL / a running engine.
        assert_eq!(resolve_engine_port(&argv(&["--port", "51777"])), 51777);
        assert_eq!(
            resolve_engine_port(&argv(&["--message", "hi", "--port", "52001"])),
            52001
        );
    }

    #[test]
    fn begin_conversation_id_prefers_explicit_flag() {
        let args = argv(&["--conversation-id", "abc-123"]);
        assert_eq!(begin_conversation_id(&args), "abc-123");
        // blank flag falls through to env (not asserted here to stay deterministic).
        let blank = argv(&["--conversation-id", "   "]);
        // With a blank flag and no env, the id is empty.
        if std::env::var("CLAUDE_CODE_SESSION_ID").is_err() {
            assert_eq!(begin_conversation_id(&blank), "");
        }
    }

    // The lifecycle verbs short-circuit to `None` (→ "engine unreachable / required
    // args missing", exit 1) BEFORE dialing the engine when a required arg is absent.
    // We assert that guard by pointing at a port no engine listens on: a present-args
    // call would attempt a connection (and fail with the unreachable line too), but an
    // ABSENT required arg must return None without ever building the request. Both map
    // to None here; the value of these is locking the required-arg set in place.
    #[test]
    fn snapshot_requires_artifact_path_and_to_state() {
        // Missing --artifact-path → None.
        let a = argv(&["--to-state", "plan", "--port", "59998"]);
        assert!(call_engine_snapshot(&a).is_none());
        // Missing --to-state → None.
        let b = argv(&["--artifact-path", "tracks/x", "--port", "59998"]);
        assert!(call_engine_snapshot(&b).is_none());
    }

    #[test]
    fn complete_requires_artifact_path() {
        let a = argv(&["--actor-name", "X", "--port", "59998"]);
        assert!(call_engine_complete(&a).is_none());
    }

    #[test]
    fn amend_requires_artifact_path_and_target_document() {
        let a = argv(&["--target-document", "spec", "--port", "59998"]);
        assert!(call_engine_amend(&a).is_none());
        let b = argv(&["--artifact-path", "tracks/x", "--port", "59998"]);
        assert!(call_engine_amend(&b).is_none());
    }
}
