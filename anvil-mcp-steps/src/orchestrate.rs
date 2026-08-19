//! Brine steps for the `anvil_orchestrate` surface→Anvil handoff (foundry-hearth
//! track 20260605T1931_surface_anvil_entrypoint). Each scenario runs its full
//! flow inside one step against the REAL anvil-engine + anvil-mcp binaries over a
//! throwaway fixture hearth (no mocks): build fixture → spawn engine + shim →
//! call `anvil_orchestrate` → capture the response (and, for the advance case,
//! issue the returned next_call and re-read state + engine measurement). The
//! captured result is stored as `orch` (JsonValue) for the check steps.

use anvil_test_support::{retained_temp_dir, RetainedTempDir};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind 0");
    l.local_addr().unwrap().port()
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

fn copy_playbook_fixture(kind: &str, hearth: &Path) -> Result<(), String> {
    let src = Path::new(anvil_test_support::TEST_SUPPORT_DIR)
        .join("fixtures")
        .join(kind);
    let dst = hearth.join("playbooks").join(kind);
    copy_dir_all(&src, &dst).map_err(|e| format!("copy {} playbook fixture: {}", kind, e))
}

/// Build a fixture hearth that the engine can drive `track_lifecycle` over: the
/// seed proposal (active), the registries, and the anvil-repo `workflows/` (whose
/// `track_lifecycle/hooks/spec-writing.md` matches the machine — the foundry
/// anvil-kit copy drifted to `spec-entry.md`).
fn build_fixture() -> Result<(RetainedTempDir, PathBuf), String> {
    let (handle, dir) = retained_temp_dir("anvil-orch-hearth-")?;
    let mk = |rel: &str| dir.join(rel);
    std::fs::create_dir_all(mk("tracks")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(mk("proposals/seedprop")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(mk("projections")).map_err(|e| e.to_string())?;
    std::fs::write(mk(".hearth"), "path: .\n").map_err(|e| e.to_string())?;
    std::fs::write(mk("tracks.md"), "# Tracks\n").map_err(|e| e.to_string())?;
    std::fs::write(
        mk("proposals.md"),
        "# Proposals\n\n## active\n\n- [seed](proposals/seedprop/)\n",
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(mk("projections/execution.md"), "# Execution\n").map_err(|e| e.to_string())?;
    std::fs::write(
        mk("proposals/seedprop/status.yaml"),
        "version: 1\nkind: proposal\nstate: active\nactors: {}\ntransitions:\n  - to: active\n    at: \"2026-06-05T00:00:00Z\"\n    actor: Nick\n    role: author\n",
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(mk("proposals/seedprop/proposal.md"), "# Seed proposal\n")
        .map_err(|e| e.to_string())?;
    // anvil-test-support/.. -> anvil repo root -> workflows/
    let playbooks_src = Path::new(anvil_test_support::TEST_SUPPORT_DIR)
        .parent()
        .ok_or("no parent")?
        .join("playbooks");
    copy_dir_all(&playbooks_src, &mk("playbooks")).map_err(|e| format!("copy playbooks: {}", e))?;
    copy_playbook_fixture("daily_recap", &dir)?;
    copy_playbook_fixture("weekly_recap", &dir)?;
    copy_playbook_fixture("lore_query", &dir)?;
    // Also seed the knowledge_lifecycle machine so anvil_orchestrate can route a
    // `knowledge_lifecycle` hint through the now-generalized begin (the engine
    // resolves it via HearthPlaybookRegistry). The knowledge registry +
    // directory are seeded so the first-complete entry creation has a target.
    std::fs::create_dir_all(mk("knowledge")).map_err(|e| e.to_string())?;
    std::fs::write(mk("knowledge.md"), "# Knowledge\n\n## Ingesting\n")
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(mk("playbooks/knowledge_lifecycle")).map_err(|e| e.to_string())?;
    std::fs::write(
        mk("playbooks/knowledge_lifecycle/machine.yaml"),
        anvil_test_support::query_port::knowledge_lifecycle_machine_yaml(),
    )
    .map_err(|e| e.to_string())?;
    // Seed a register:free machine ("spark") so the begin-mode driven-kind guard
    // has a free candidate to REJECT (it must never appear in the candidate set,
    // and selecting it must be refused).
    std::fs::create_dir_all(mk("playbooks/spark_free")).map_err(|e| e.to_string())?;
    std::fs::write(
        mk("playbooks/spark_free/machine.yaml"),
        FREE_SPARK_MACHINE_YAML,
    )
    .map_err(|e| e.to_string())?;
    // Seed a driven kit-action machine that declares generic required fields
    // (question, requester) outside the builtin set, so the generic-fields
    // missing-loop and begin-with-fields paths can be exercised end-to-end.
    std::fs::create_dir_all(mk("kit_actions")).map_err(|e| e.to_string())?;
    std::fs::write(mk("kit_actions.md"), "# Kit Actions\n").map_err(|e| e.to_string())?;
    std::fs::create_dir_all(mk("playbooks/kit_action/hooks")).map_err(|e| e.to_string())?;
    std::fs::write(
        mk("playbooks/kit_action/machine.yaml"),
        KIT_ACTION_MACHINE_YAML,
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(
        mk("playbooks/kit_action/hooks/answering.md"),
        "# Answering\n\nAnswer {{question}} for {{requester}}.\n",
    )
    .map_err(|e| e.to_string())?;
    // Seed a driven action whose machine.yaml declares the PRE-MIGRATION spelling
    // of the playbook-name field. Persisted machine definitions are owned by the
    // hearth, not by this repo, and the live builder machine
    // (anvil-hearth/playbooks/20260528T2321_workflow_generation/machine.yaml)
    // still declares it. The shim must recognise that declaration and point the
    // caller at the ONE canonical argument, never at a bag key it does not read.
    std::fs::create_dir_all(mk("legacy_named_actions")).map_err(|e| e.to_string())?;
    std::fs::write(mk("legacy_named_actions.md"), "# Legacy Named Actions\n")
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(mk("playbooks/legacy_named_action/hooks"))
        .map_err(|e| e.to_string())?;
    std::fs::write(
        mk("playbooks/legacy_named_action/machine.yaml"),
        LEGACY_NAMED_ACTION_MACHINE_YAML,
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(
        mk("playbooks/legacy_named_action/hooks/drafting.md"),
        "# Drafting\n\nDraft the definition for {{playbook_name}}.\n",
    )
    .map_err(|e| e.to_string())?;
    // route_response_mirrors_begin H3 (MCP route-mode fail-open): a single-trigger
    // driven machine whose initial (state, doer) DECLARES a hook file that is
    // intentionally never written. A route-mode call that resolves to it makes the
    // engine's guidance enrichment read error, so the engine fails open and the
    // shim returns a SUCCESSFUL thin route (empty guidance), never a tool error.
    std::fs::create_dir_all(mk("playbooks/brittle/hooks")).map_err(|e| e.to_string())?;
    std::fs::write(mk("playbooks/brittle/machine.yaml"), BRITTLE_MACHINE_YAML)
        .map_err(|e| e.to_string())?;
    // The hook EXISTS (so the machine loads) but is UNREADABLE (mode 000) so the
    // engine's request-time guidance read fails — exercising the route-mode
    // fail-open (engine returns a thin route; the shim returns a successful
    // advisory route, not a tool error).
    let brittle_hook = mk("playbooks/brittle/hooks/answering.md");
    std::fs::write(&brittle_hook, "# Answering\n\nFirst-step body.\n")
        .map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&brittle_hook, std::fs::Permissions::from_mode(0o000))
            .map_err(|e| e.to_string())?;
    }
    Ok((handle, dir))
}

/// A driven machine with a single unambiguous trigger whose initial (state,
/// doer) declares a hook file that is never created — used to exercise the MCP
/// route-mode guidance-enrichment fail-open (engine returns a thin route).
const BRITTLE_MACHINE_YAML: &str = r#"kind: brittle
directory: brittles
registry: brittles.md
parent_kind: ~
description: "A driven playbook whose declared hook is missing on disk."
register: driven
route:
  triggers: ["do the brittle thing"]
required_fields: []
roles:
  - doer
  - reviewer
states:
  - name: answering
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
  - from_state: answering
    to_state: completed
    required_role: doer
    requires_approver: false
"#;

/// A driven kit-action machine declaring two generic required fields outside
/// the builtin set, with a route trigger so anvil_orchestrate can select it.
const KIT_ACTION_MACHINE_YAML: &str = r#"kind: kit_action
directory: kit_actions
registry: kit_actions.md
parent_kind: ~
description: "A kit-action playbook with generic required fields."
route:
  triggers: ["do a kit action"]
required_fields:
  - name: question
    field_type: string
    description: The natural-language question to answer.
  - name: requester
    field_type: actor_name
    description: Actor that initiated the run.
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

/// A driven machine that declares the pre-migration spelling of the
/// playbook-name field (`workflow_name`), exactly as the live builder machine
/// in anvil-hearth still does. `machine.yaml` files are persisted hearth data
/// this repo reads and does not own, so the read side must recognise the name.
const LEGACY_NAMED_ACTION_MACHINE_YAML: &str = r#"kind: legacy_named_action
directory: legacy_named_actions
registry: legacy_named_actions.md
parent_kind: ~
description: "A driven playbook whose machine declares the pre-migration name field."
route:
  triggers: ["do a legacy named action"]
required_fields:
  - name: workflow_name
    field_type: string
    description: The name of the playbook being defined.
roles:
  - doer
  - reviewer
states:
  - name: drafting
    role_filters:
      - doer_actionable
    registry_section: "Drafting"
    projection_targets: []
    is_review_gate: false
    is_terminal: false
    hook: drafting.md
  - name: completed
    role_filters:
      - terminal
    registry_section: "Completed"
    projection_targets: []
    is_review_gate: false
    is_terminal: true
    hook: ~
transitions:
  - from_state: drafting
    to_state: completed
    required_role: doer
    requires_approver: false
"#;

/// A minimal register:free machine for the driven-kind-guard rejection test.
const FREE_SPARK_MACHINE_YAML: &str = r#"kind: spark
directory: sparks
registry: sparks.md
description: "A free generative kind (never a route target)."
register: free
required_fields: []
roles:
  - doer
states:
  - name: active
    role_filters: []
    registry_section: active
    projection_targets: []
    is_review_gate: false
    is_terminal: false
transitions: []
"#;

fn wait_port(port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

struct Procs {
    engine: Child,
    shim: Child,
    stderr_buf: Arc<Mutex<String>>,
}

impl Drop for Procs {
    fn drop(&mut self) {
        let _ = self.shim.kill();
        let _ = self.shim.wait();
        let _ = self.engine.kill();
        let _ = self.engine.wait();
    }
}

/// Spawn the real engine (stderr captured for the measurement assertion) + the
/// shim pointed at it, and complete the MCP init handshake.
fn spawn(hearth: &Path) -> Result<Procs, String> {
    let engine_bin = anvil_test_support::harness::binary_path("anvil-engine");
    let shim_bin = anvil_test_support::harness::binary_path("anvil-mcp");
    let port = free_port();
    let mut engine = Command::new(&engine_bin)
        .arg("--hearth")
        .arg(hearth)
        .arg("--port")
        .arg(port.to_string())
        .env("ANVIL_RENDEZVOUS_DISABLE", "1")
        // Anvil records fleet telemetry unconditionally; redirect it off the real
        // `~/.anvil` for this hermetic orchestrate scenario.
        .env(
            "ANVIL_TELEMETRY_DIR",
            std::env::temp_dir().join("anvil-brine-telemetry"),
        )
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn engine: {}", e))?;
    let stderr_buf = Arc::new(Mutex::new(String::new()));
    if let Some(err) = engine.stderr.take() {
        let buf = Arc::clone(&stderr_buf);
        std::thread::spawn(move || {
            let mut r = BufReader::new(err);
            let mut chunk = [0u8; 4096];
            loop {
                match r.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if let Ok(mut g) = buf.lock() {
                            g.push_str(&String::from_utf8_lossy(&chunk[..n]));
                        }
                    }
                }
            }
        });
    }
    if !wait_port(port, Duration::from_secs(8)) {
        return Err("engine did not bind its port".to_string());
    }
    let mut shim = Command::new(&shim_bin)
        .current_dir(hearth)
        .env("ANVIL_ENGINE_PORT", port.to_string())
        .env_remove("FOUNDRY_SESSION_TOKEN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn shim: {}", e))?;
    // MCP init handshake.
    {
        let stdin = shim.stdin.as_mut().ok_or("no shim stdin")?;
        writeln!(stdin, "{}", serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"brine","version":"0"}}})).map_err(|e| e.to_string())?;
        writeln!(
            stdin,
            "{}",
            serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )
        .map_err(|e| e.to_string())?;
        stdin.flush().map_err(|e| e.to_string())?;
    }
    // read the initialize response line
    read_line(&mut shim)?;
    Ok(Procs {
        engine,
        shim,
        stderr_buf,
    })
}

fn read_line(shim: &mut Child) -> Result<serde_json::Value, String> {
    let stdout = shim.stdout.as_mut().ok_or("no shim stdout")?;
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|e| e.to_string())?;
    if line.trim().is_empty() {
        return Err("empty shim response".to_string());
    }
    serde_json::from_str(&line)
        .map_err(|e| format!("parse shim response: {} (raw {})", e, line.trim()))
}

fn send(shim: &mut Child, msg: &serde_json::Value) -> Result<(), String> {
    let stdin = shim.stdin.as_mut().ok_or("no shim stdin")?;
    writeln!(stdin, "{}", msg).map_err(|e| e.to_string())?;
    stdin.flush().map_err(|e| e.to_string())
}

fn call(
    shim: &mut Child,
    idn: i64,
    name: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    send(
        shim,
        &serde_json::json!({"jsonrpc":"2.0","id":idn,"method":"tools/call","params":{"name":name,"arguments":args}}),
    )?;
    read_line(shim)
}

/// Parse the MCP tool result envelope: returns (is_error, inner_json_or_text).
fn unwrap_result(resp: &serde_json::Value) -> (bool, serde_json::Value) {
    let is_error = resp["result"]["isError"].as_bool().unwrap_or(false);
    let text = resp["result"]["content"][0]["text"].as_str().unwrap_or("");
    let inner = serde_json::from_str::<serde_json::Value>(text)
        .unwrap_or_else(|_| serde_json::Value::String(text.to_string()));
    (is_error, inner)
}

/// Map a legacy hint to the two-phase `selection` (the artifact_kind).
/// `track_lifecycle` is the playbook_id label for the seed `track` kind; domain
/// hints (e.g. `knowledge_lifecycle`) ARE the kind.
fn hint_to_selection(hint: &str) -> &str {
    match hint {
        "track_lifecycle" => "track",
        other => other,
    }
}

/// Run one handoff. If `hint` is None the request carries no `selection`
/// (route-mode — the routing/no_match test). If `hint` is Some, it is mapped to
/// a `selection` and the call runs begin-mode in one shot. If `advance` is true,
/// also issue the returned next_call and re-read state + scan engine stderr for
/// the per-step measurement.
fn run(
    message: &str,
    hint: Option<&str>,
    surface: &str,
    advance: bool,
) -> Result<serde_json::Value, String> {
    let selection = hint.map(hint_to_selection);
    run_selection(message, selection, surface, advance)
}

/// Run one handoff with an explicit `selection` (or None for route-mode).
fn run_selection(
    message: &str,
    selection: Option<&str>,
    surface: &str,
    advance: bool,
) -> Result<serde_json::Value, String> {
    run_selection_with_ctx(message, selection, surface, advance, None)
}

fn run_selection_with_ctx(
    message: &str,
    selection: Option<&str>,
    surface: &str,
    advance: bool,
    ctx_org: Option<&str>,
) -> Result<serde_json::Value, String> {
    run_selection_with_ctx_and_conversation(message, selection, surface, advance, ctx_org, "")
}

fn run_selection_with_ctx_and_conversation(
    message: &str,
    selection: Option<&str>,
    surface: &str,
    advance: bool,
    ctx_org: Option<&str>,
    conversation_id: &str,
) -> Result<serde_json::Value, String> {
    let (_hearth_handle, hearth) = build_fixture()?;
    let mut p = spawn(&hearth)?;
    let mut args = serde_json::json!({
        "message": message,
        "surface": surface,
        "scope": {"parent_id": "seedprop"},
        "conversation_id": conversation_id,
        "approver": "Nick",
        "actor_name": "Surface-000001",
        "actor_type": "agent",
        "actor_model": "claude-opus-4-8",
        "actor_provider": "anthropic"
    });
    if let Some(s) = selection {
        args["selection"] = serde_json::json!(s);
    }
    if let Some(org) = ctx_org {
        args["ctx"] = serde_json::json!({
            "org": org,
            "space": "",
            "role": "read",
            "clearance": "internal"
        });
    }
    let resp = call(&mut p.shim, 2, "anvil_orchestrate", args)?;
    let (is_error, handoff) = unwrap_result(&resp);
    let mut out =
        serde_json::json!({ "is_error": is_error, "handoff": handoff, "mcp_call_count": 1 });

    if advance && !is_error {
        let next = &handoff["next_call"];
        let tool = next["tool"].as_str().unwrap_or("");
        let nargs = next["arguments"].clone();
        let _ = call(&mut p.shim, 3, tool, nargs)?;
        out["mcp_call_count"] = serde_json::json!(2);
        // re-read state via describe
        let instance = handoff["instance_id"].as_str().unwrap_or("");
        let desc = call(
            &mut p.shim,
            4,
            "describe",
            serde_json::json!({"identifier": instance}),
        )?;
        out["mcp_call_count"] = serde_json::json!(3);
        let (_, dinner) = unwrap_result(&desc);
        out["post_state"] = serde_json::json!(dinner["state"].as_str().unwrap_or(""));
        // engine emits event_kind="step_measurement" on the transition
        std::thread::sleep(Duration::from_millis(200));
        let measured = p
            .stderr_buf
            .lock()
            .map(|g| g.contains("step_measurement"))
            .unwrap_or(false);
        out["measured"] = serde_json::json!(measured);
    }
    let mut out = with_records(out, &p.stderr_buf);
    // route_response_mirrors_begin H4 proof: count any daily_recap instances the
    // route call created (advisory single must create NONE). Read the hearth
    // before it is dropped.
    out["daily_recap_count"] =
        serde_json::json!(count_kind_instances(&hearth, "daily_recaps", "daily_recap"));
    drop(p);
    Ok(out)
}

/// resume_aware_routing C1 — seed an open (begun, non-terminal) artifact carrying
/// `conversation_id` into the fixture hearth BEFORE shipping a continuation token
/// in route-mode, so the MCP route-mode resume branch can resolve it. Writes
/// `{kind}s/{id}/status.yaml` with the kind/state, a creation transition, and an
/// `activity:` begin marker stamping the conversation_id (mirrors the engine-seam
/// `write_open_artifact`). Then ships `message` in route-mode with that
/// conversation_id + ctx org and returns the handoff.
fn run_selection_resume(
    seed_kind: &str,
    seed_id: &str,
    seed_state: &str,
    message: &str,
    conversation_id: &str,
    ctx_org: &str,
) -> Result<serde_json::Value, String> {
    let (_hearth_handle, hearth) = build_fixture()?;
    // Seed the open artifact + begin marker for this conversation.
    let dir = hearth.join(format!("{}s", seed_kind)).join(seed_id);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create seed dir: {}", e))?;
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
    at: "2026-06-01T10:00:00Z"
    conversation_id: "{conversation_id}"
"#,
        kind = seed_kind,
        state = seed_state,
        conversation_id = conversation_id
    );
    std::fs::write(dir.join("status.yaml"), status)
        .map_err(|e| format!("write seed status.yaml: {}", e))?;

    let mut p = spawn(&hearth)?;
    let args = serde_json::json!({
        "message": message,
        "surface": "kiln",
        "scope": {"parent_id": "seedprop"},
        "conversation_id": conversation_id,
        "approver": "Nick",
        "actor_name": "Surface-000001",
        "actor_type": "agent",
        "actor_model": "claude-opus-4-8",
        "actor_provider": "anthropic",
        "ctx": {"org": ctx_org, "space": "", "role": "read", "clearance": "internal"},
    });
    let resp = call(&mut p.shim, 2, "anvil_orchestrate", args)?;
    let (is_error, handoff) = unwrap_result(&resp);
    let out = serde_json::json!({ "is_error": is_error, "handoff": handoff, "mcp_call_count": 1 });
    let out = with_records(out, &p.stderr_buf);
    drop(p);
    Ok(out)
}

fn base_orchestrate_args(message: &str, conversation_id: &str) -> serde_json::Value {
    serde_json::json!({
        "message": message,
        "surface": "kiln",
        "scope": {"parent_id": "seedprop"},
        "conversation_id": conversation_id,
        "approver": "Nick",
        "actor_name": "Surface-000001",
        "actor_type": "agent",
        "actor_model": "claude-opus-4-8",
        "actor_provider": "anthropic"
    })
}

fn routing_records(stderr_buf: &Arc<Mutex<String>>) -> Vec<serde_json::Value> {
    std::thread::sleep(Duration::from_millis(300));
    let stderr = stderr_buf.lock().map(|g| g.clone()).unwrap_or_default();
    stderr
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|v| v["event_kind"].as_str() == Some("routing_decision"))
        .collect()
}

fn read_jsonl(path: &Path) -> Result<Vec<serde_json::Value>, String> {
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

fn with_records(mut out: serde_json::Value, stderr_buf: &Arc<Mutex<String>>) -> serde_json::Value {
    out["routing_records"] = serde_json::Value::Array(routing_records(stderr_buf));
    out
}

fn handoff_artifact_path(handoff: &serde_json::Value) -> &str {
    handoff["next_call"]["arguments"]["artifact_path"]
        .as_str()
        .unwrap_or("")
}

fn count_kind_instances(hearth: &Path, directory: &str, kind: &str) -> usize {
    let dir = hearth.join(directory);
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.path().join("status.yaml").exists())
                .filter(|entry| {
                    let status_path = entry.path().join("status.yaml");
                    let Ok(content) = std::fs::read_to_string(status_path) else {
                        return false;
                    };
                    serde_yaml::from_str::<serde_yaml::Value>(&content)
                        .ok()
                        .and_then(|status| {
                            status
                                .get("kind")
                                .and_then(|value| value.as_str())
                                .map(|status_kind| status_kind == kind)
                        })
                        .unwrap_or(false)
                })
                .count()
        })
        .unwrap_or(0)
}

fn run_auto_resolve_idempotency(
    message: &str,
    conversation_id: &str,
    different_conversation_id: &str,
    ctx_org: &str,
) -> Result<serde_json::Value, String> {
    let (_hearth_handle, hearth) = build_fixture()?;
    let mut p = spawn(&hearth)?;

    let mut first_args = base_orchestrate_args(message, conversation_id);
    first_args["ctx"] = serde_json::json!({
        "org": ctx_org,
        "space": "",
        "role": "read",
        "clearance": "internal"
    });
    let first_resp = call(&mut p.shim, 2, "anvil_orchestrate", first_args)?;
    let (first_is_error, first_handoff) = unwrap_result(&first_resp);

    let mut second_args = base_orchestrate_args(message, conversation_id);
    second_args["ctx"] = serde_json::json!({
        "org": ctx_org,
        "space": "",
        "role": "read",
        "clearance": "internal"
    });
    let second_resp = call(&mut p.shim, 3, "anvil_orchestrate", second_args)?;
    let (second_is_error, second_handoff) = unwrap_result(&second_resp);
    let count_after_repeat = count_kind_instances(&hearth, "daily_recaps", "daily_recap");

    let mut different_args = base_orchestrate_args(message, different_conversation_id);
    different_args["ctx"] = serde_json::json!({
        "org": ctx_org,
        "space": "",
        "role": "read",
        "clearance": "internal"
    });
    let different_resp = call(&mut p.shim, 4, "anvil_orchestrate", different_args)?;
    let (different_is_error, different_handoff) = unwrap_result(&different_resp);
    let count_after_different = count_kind_instances(&hearth, "daily_recaps", "daily_recap");

    let out = serde_json::json!({
        "first_is_error": first_is_error,
        "second_is_error": second_is_error,
        "different_is_error": different_is_error,
        "first_handoff": first_handoff,
        "second_handoff": second_handoff,
        "different_handoff": different_handoff,
        "first_artifact_path": handoff_artifact_path(&first_handoff),
        "second_artifact_path": handoff_artifact_path(&second_handoff),
        "different_artifact_path": handoff_artifact_path(&different_handoff),
        "count_after_repeat": count_after_repeat,
        "count_after_different": count_after_different
    });
    let out = with_records(out, &p.stderr_buf);
    drop(p);
    Ok(out)
}

fn run_route_then_select(
    message: &str,
    selection: &str,
    conversation_id: &str,
    confidence: &str,
    routing_hint: Option<&str>,
    follow_next_call: bool,
) -> Result<serde_json::Value, String> {
    let (_hearth_handle, hearth) = build_fixture()?;
    let mut p = spawn(&hearth)?;
    let mut route_args = base_orchestrate_args(message, conversation_id);
    if let Some(hint) = routing_hint {
        route_args["routing_hint"] = serde_json::json!(hint);
    }
    let route_resp = call(&mut p.shim, 2, "anvil_orchestrate", route_args)?;
    let (route_is_error, route_handoff) = unwrap_result(&route_resp);
    let mut begin_is_error = route_is_error;
    let mut begin_handoff = serde_json::Value::Null;
    if !route_is_error {
        let mut begin_args = if follow_next_call {
            route_handoff["next_call"]["arguments"].clone()
        } else {
            let mut args = base_orchestrate_args(message, conversation_id);
            if let Some(hint) = routing_hint {
                args["routing_hint"] = serde_json::json!(hint);
            }
            args["candidate_set"] =
                route_handoff["next_call"]["arguments"]["candidate_set"].clone();
            args
        };
        begin_args["selection"] = serde_json::json!(selection);
        begin_args["confidence"] = serde_json::json!(confidence);
        let begin_resp = call(&mut p.shim, 3, "anvil_orchestrate", begin_args)?;
        let (is_error, handoff) = unwrap_result(&begin_resp);
        begin_is_error = is_error;
        begin_handoff = handoff;
    }
    let out = serde_json::json!({
        "is_error": route_is_error || begin_is_error,
        "handoff": if begin_handoff.is_null() { route_handoff.clone() } else { begin_handoff.clone() },
        "route_handoff": route_handoff,
        "begin_handoff": begin_handoff
    });
    let out = with_records(out, &p.stderr_buf);
    drop(p);
    Ok(out)
}

fn run_route_only(
    message: &str,
    routing_hint: &str,
    conversation_id: &str,
) -> Result<serde_json::Value, String> {
    let (_hearth_handle, hearth) = build_fixture()?;
    let mut p = spawn(&hearth)?;
    let mut args = base_orchestrate_args(message, conversation_id);
    args["routing_hint"] = serde_json::json!(routing_hint);
    let resp = call(&mut p.shim, 2, "anvil_orchestrate", args)?;
    let (is_error, handoff) = unwrap_result(&resp);
    let out = serde_json::json!({
        "is_error": is_error,
        "handoff": handoff.clone(),
        "route_handoff": handoff
    });
    let out = with_records(out, &p.stderr_buf);
    drop(p);
    Ok(out)
}

fn run_route_correlation(
    message: &str,
    conversation_id: &str,
    surface: &str,
    ctx_org: &str,
) -> Result<serde_json::Value, String> {
    let (_hearth_handle, hearth) = build_fixture()?;
    let mut p = spawn(&hearth)?;
    let mut args = base_orchestrate_args(message, conversation_id);
    args["surface"] = serde_json::json!(surface);
    args["ctx"] = serde_json::json!({
        "org": ctx_org,
        "space": "",
        "role": "read",
        "clearance": "internal"
    });

    let resp = call(&mut p.shim, 2, "anvil_orchestrate", args)?;
    let (is_error, handoff) = unwrap_result(&resp);
    let routing_activity_records = read_jsonl(&hearth.join("routing-activity.jsonl"))?;
    let activity_log_records = read_jsonl(&hearth.join("activity-log.jsonl"))?;
    let out = serde_json::json!({
        "is_error": is_error,
        "handoff": handoff.clone(),
        "route_handoff": handoff,
        "routing_activity_records": routing_activity_records,
        "activity_log_records": activity_log_records
    });
    let out = with_records(out, &p.stderr_buf);
    drop(p);
    Ok(out)
}

fn run_begin_only_utf8(
    selection: &str,
    conversation_id: &str,
    confidence: &str,
) -> Result<serde_json::Value, String> {
    let (_hearth_handle, hearth) = build_fixture()?;
    let mut p = spawn(&hearth)?;
    let message = "Ingest café notes\nsecond line";
    let mut args = base_orchestrate_args(message, conversation_id);
    args["selection"] = serde_json::json!(selection);
    args["confidence"] = serde_json::json!(confidence);
    let resp = call(&mut p.shim, 2, "anvil_orchestrate", args)?;
    let (is_error, handoff) = unwrap_result(&resp);
    let out = serde_json::json!({
        "is_error": is_error,
        "handoff": handoff
    });
    let out = with_records(out, &p.stderr_buf);
    drop(p);
    Ok(out)
}

fn run_track_missing_fields_preflight() -> Result<serde_json::Value, String> {
    let (_hearth_handle, hearth) = build_fixture()?;
    let mut p = spawn(&hearth)?;
    let before = count_kind_instances(&hearth, "tracks", "track");
    let args = serde_json::json!({
        "message": "start a track",
        "surface": "kiln",
        "conversation_id": "turn-c3-required-fields",
        "actor_name": "Surface-000001",
        "actor_type": "agent",
        "actor_model": "claude-opus-4-8",
        "actor_provider": "anthropic"
    });
    let resp = call(&mut p.shim, 2, "anvil_orchestrate", args)?;
    let (is_error, handoff) = unwrap_result(&resp);
    let after = count_kind_instances(&hearth, "tracks", "track");
    let out = serde_json::json!({
        "is_error": is_error,
        "handoff": handoff,
        "track_count_before": before,
        "track_count_after": after,
        "mcp_call_count": 1
    });
    let out = with_records(out, &p.stderr_buf);
    drop(p);
    Ok(out)
}

fn run_begin_track_with_fields(
    track_name: &str,
    parent_id: &str,
    approver: &str,
) -> Result<serde_json::Value, String> {
    let (_hearth_handle, hearth) = build_fixture()?;
    let mut p = spawn(&hearth)?;
    let args = serde_json::json!({
        "selection": "track",
        "track_name": track_name,
        "parent_id": parent_id,
        "approver": approver,
        "surface": "kiln",
        "conversation_id": "turn-c3-begin-track",
        "actor_name": "Surface-000001",
        "actor_type": "agent",
        "actor_model": "claude-opus-4-8",
        "actor_provider": "anthropic"
    });
    let resp = call(&mut p.shim, 2, "anvil_orchestrate", args)?;
    let (is_error, handoff) = unwrap_result(&resp);
    let out = serde_json::json!({ "is_error": is_error, "handoff": handoff, "mcp_call_count": 1 });
    let out = with_records(out, &p.stderr_buf);
    drop(p);
    Ok(out)
}

/// Begin-mode kit_action with NO generic fields supplied: expects the shim to
/// return a missing_required_fields outcome whose next_call pre-keys `fields`.
fn run_begin_kit_action_no_fields() -> Result<serde_json::Value, String> {
    let (_hearth_handle, hearth) = build_fixture()?;
    let mut p = spawn(&hearth)?;
    let args = serde_json::json!({
        "selection": "kit_action",
        "message": "do a kit action",
        "surface": "kiln",
        "conversation_id": "turn-generic-fields-missing",
        "actor_name": "Surface-000001",
        "actor_type": "agent",
        "actor_model": "claude-opus-4-8",
        "actor_provider": "anthropic"
    });
    let resp = call(&mut p.shim, 2, "anvil_orchestrate", args)?;
    let (is_error, handoff) = unwrap_result(&resp);
    let out = serde_json::json!({ "is_error": is_error, "handoff": handoff, "mcp_call_count": 1 });
    let out = with_records(out, &p.stderr_buf);
    drop(p);
    Ok(out)
}

/// Begin-mode kit_action WITH the generic fields supplied in the `fields`
/// argument: expects the playbook to begin (no missing_required_fields).
fn run_begin_kit_action_with_fields(
    question: &str,
    requester: &str,
) -> Result<serde_json::Value, String> {
    let (_hearth_handle, hearth) = build_fixture()?;
    let mut p = spawn(&hearth)?;
    let args = serde_json::json!({
        "selection": "kit_action",
        "message": "do a kit action",
        "surface": "kiln",
        "conversation_id": "turn-generic-fields-supplied",
        "approver": "Nick",
        "fields": { "question": question, "requester": requester },
        "actor_name": "Surface-000001",
        "actor_type": "agent",
        "actor_model": "claude-opus-4-8",
        "actor_provider": "anthropic"
    });
    let resp = call(&mut p.shim, 2, "anvil_orchestrate", args)?;
    let (is_error, handoff) = unwrap_result(&resp);
    let out = serde_json::json!({ "is_error": is_error, "handoff": handoff, "mcp_call_count": 1 });
    let out = with_records(out, &p.stderr_buf);
    drop(p);
    Ok(out)
}

/// Base begin-mode arguments for the machine that declares the pre-migration
/// name field. Nothing that could satisfy the declared field is supplied.
fn legacy_named_action_args(conversation_id: &str) -> serde_json::Value {
    serde_json::json!({
        "selection": "legacy_named_action",
        "message": "do a legacy named action",
        "surface": "kiln",
        "conversation_id": conversation_id,
        "actor_name": "Surface-000001",
        "actor_type": "agent",
        "actor_model": "claude-opus-4-8",
        "actor_provider": "anthropic"
    })
}

/// Begin-mode against the machine declaring the pre-migration name field, with
/// nothing supplied: the shim must report the field missing AND name the
/// argument that satisfies it.
fn run_begin_legacy_named_action_no_fields() -> Result<serde_json::Value, String> {
    let (_hearth_handle, hearth) = build_fixture()?;
    let mut p = spawn(&hearth)?;
    let resp = call(
        &mut p.shim,
        2,
        "anvil_orchestrate",
        legacy_named_action_args("turn-legacy-name-missing"),
    )?;
    let (is_error, handoff) = unwrap_result(&resp);
    let out = serde_json::json!({ "is_error": is_error, "handoff": handoff, "mcp_call_count": 1 });
    let out = with_records(out, &p.stderr_buf);
    drop(p);
    Ok(out)
}

/// The loop test. Call begin-mode with nothing, then OBEY the returned
/// next_call literally — issue exactly the tool and arguments it carries, with
/// every placeholder (`<name>`) and empty string filled in with a real value —
/// and record the second outcome. An agent that does exactly what it is told
/// must make progress; if the second outcome is still `missing_required_fields`
/// the shim is serving an unsatisfiable loop.
fn run_begin_legacy_named_action_obeying_handoff() -> Result<serde_json::Value, String> {
    let (_hearth_handle, hearth) = build_fixture()?;
    let mut p = spawn(&hearth)?;
    let first = call(
        &mut p.shim,
        2,
        "anvil_orchestrate",
        legacy_named_action_args("turn-legacy-name-loop"),
    )?;
    let (first_error, first_handoff) = unwrap_result(&first);

    let next_tool = first_handoff["next_call"]["tool"]
        .as_str()
        .ok_or_else(|| format!("no next_call.tool in handoff: {}", first_handoff))?
        .to_string();
    let next_args = first_handoff["next_call"]["arguments"]
        .as_object()
        .ok_or_else(|| format!("no next_call.arguments in handoff: {}", first_handoff))?
        .clone();

    // Fill in literally, by the shim's own stated convention: a builtin slot is
    // marked with an `<angle-bracket>` placeholder, a generic slot is an empty
    // string inside `fields`. Nothing else is touched and nothing is added that
    // the handoff did not ask for — an echoed-empty optional argument stays
    // empty, exactly as an obedient caller would leave it.
    const SUPPLIED: &str = "a supplied value";
    let mut second_args = serde_json::Map::new();
    for (k, v) in &next_args {
        let filled = match v {
            serde_json::Value::String(s) if s.starts_with('<') && s.ends_with('>') => {
                serde_json::json!(SUPPLIED)
            }
            serde_json::Value::Object(map) if k == "fields" => serde_json::Value::Object(
                map.iter()
                    .map(|(fk, fv)| {
                        let filled = match fv {
                            serde_json::Value::String(s) if s.is_empty() => {
                                serde_json::json!(SUPPLIED)
                            }
                            other => other.clone(),
                        };
                        (fk.clone(), filled)
                    })
                    .collect(),
            ),
            other => other.clone(),
        };
        second_args.insert(k.clone(), filled);
    }
    let second = call(
        &mut p.shim,
        3,
        &next_tool,
        serde_json::Value::Object(second_args.clone()),
    )?;
    let (second_error, second_handoff) = unwrap_result(&second);

    let out = serde_json::json!({
        "is_error": second_error,
        "handoff": second_handoff,
        "first_is_error": first_error,
        "first_handoff": first_handoff,
        "obeyed_arguments": serde_json::Value::Object(second_args),
        "mcp_call_count": 2
    });
    let out = with_records(out, &p.stderr_buf);
    drop(p);
    Ok(out)
}

fn routing_record<'a>(
    o: &'a serde_json::Value,
    turn_id: &str,
    phase: &str,
    selected: Option<&str>,
) -> Vec<&'a serde_json::Value> {
    let Some(records) = o["routing_records"].as_array() else {
        return Vec::new();
    };
    records
        .iter()
        .filter(|r| r["turn_id"].as_str() == Some(turn_id))
        .filter(|r| r["phase"].as_str() == Some(phase))
        .filter(|r| match selected {
            Some(s) => r["selected"].as_str() == Some(s),
            None => true,
        })
        .collect()
}

fn store(out_key: &str, val: serde_json::Value) -> Context {
    let mut out = Context::new();
    out.set(out_key, val);
    out
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a surface ships {string} into anvil_orchestrate with hint {string}",
            &[],
            &[("orch", "JsonValue")],
            |_ctx, params| {
                let message = params.get_string(0).ok_or("message")?.to_string();
                let hint = params.get_string(1).ok_or("hint")?.to_string();
                let out = run(&message, Some(&hint), "claude-desktop", false)?;
                Ok(store("orch", out))
            },
        ),
        step_def(
            "a surface ships {string} into anvil_orchestrate in route-mode",
            &[],
            &[("orch", "JsonValue")],
            |_ctx, params| {
                let message = params.get_string(0).ok_or("message")?.to_string();
                let out = run_selection(&message, None, "kiln", false)?;
                Ok(store("orch", out))
            },
        ),
        step_def(
            "a surface ships {string} into anvil_orchestrate in route-mode with ctx org {string}",
            &[],
            &[("orch", "JsonValue")],
            |_ctx, params| {
                let message = params.get_string(0).ok_or("message")?.to_string();
                let ctx_org = params.get_string(1).ok_or("ctx org")?.to_string();
                let out = run_selection_with_ctx(&message, None, "kiln", false, Some(&ctx_org))?;
                Ok(store("orch", out))
            },
        ),
        step_def(
            "a surface ships {string} into anvil_orchestrate in route-mode with conversation_id {string} and ctx org {string}",
            &[],
            &[("orch", "JsonValue")],
            |_ctx, params| {
                let message = params.get_string(0).ok_or("message")?.to_string();
                let conversation_id = params.get_string(1).ok_or("conversation_id")?.to_string();
                let ctx_org = params.get_string(2).ok_or("ctx org")?.to_string();
                let out = run_selection_with_ctx_and_conversation(
                    &message,
                    None,
                    "kiln",
                    false,
                    Some(&ctx_org),
                    &conversation_id,
                )?;
                Ok(store("orch", out))
            },
        ),
        // resume_aware_routing C1 — seed an open artifact for a conversation so a
        // later continuation token in route-mode resumes it.
        step_def(
            "the route-mode fixture has an open {string} artifact {string} in state {string} begun for conversation {string}",
            &[],
            &[("resume_seed", "JsonValue")],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("kind")?.to_string();
                let id = params.get_string(1).ok_or("id")?.to_string();
                let state = params.get_string(2).ok_or("state")?.to_string();
                let conversation_id = params.get_string(3).ok_or("conversation_id")?.to_string();
                Ok(store(
                    "resume_seed",
                    serde_json::json!({
                        "kind": kind,
                        "id": id,
                        "state": state,
                        "conversation_id": conversation_id,
                    }),
                ))
            },
        ),
        step_def(
            "a surface ships {string} into anvil_orchestrate in route-mode with conversation_id {string} and ctx org {string} resuming the seeded playbook",
            &[("resume_seed", "JsonValue")],
            &[("orch", "JsonValue")],
            |ctx, params| {
                let message = params.get_string(0).ok_or("message")?.to_string();
                let conversation_id = params.get_string(1).ok_or("conversation_id")?.to_string();
                let ctx_org = params.get_string(2).ok_or("ctx org")?.to_string();
                let seed = ctx
                    .get::<serde_json::Value>("resume_seed")
                    .ok_or("no resume_seed (Given step did not run)")?;
                let kind = seed["kind"].as_str().ok_or("seed kind")?;
                let id = seed["id"].as_str().ok_or("seed id")?;
                let state = seed["state"].as_str().ok_or("seed state")?;
                let out =
                    run_selection_resume(kind, id, state, &message, &conversation_id, &ctx_org)?;
                Ok(store("orch", out))
            },
        ),
        step_def(
            "a surface ships {string} into anvil_orchestrate in route-mode with no creation fields",
            &[],
            &[("orch", "JsonValue")],
            |_ctx, params| {
                let message = params.get_string(0).ok_or("message")?.to_string();
                if message != "start a track" {
                    return Err(format!("unsupported no-field route fixture message '{}'", message));
                }
                let out = run_track_missing_fields_preflight()?;
                Ok(store("orch", out))
            },
        ),
        step_def(
            "a surface begins a kit_action through anvil_orchestrate with no generic fields",
            &[],
            &[("orch", "JsonValue")],
            |_ctx, _params| {
                let out = run_begin_kit_action_no_fields()?;
                Ok(store("orch", out))
            },
        ),
        step_def(
            "a surface begins a kit_action through anvil_orchestrate with fields question {string} requester {string}",
            &[],
            &[("orch", "JsonValue")],
            |_ctx, params| {
                let question = params.get_string(0).ok_or("question")?.to_string();
                let requester = params.get_string(1).ok_or("requester")?.to_string();
                let out = run_begin_kit_action_with_fields(&question, &requester)?;
                Ok(store("orch", out))
            },
        ),
        step_def(
            "a surface begins an action whose machine declares the pre-migration name field, supplying nothing",
            &[],
            &[("orch", "JsonValue")],
            |_ctx, _params| {
                let out = run_begin_legacy_named_action_no_fields()?;
                Ok(store("orch", out))
            },
        ),
        step_def(
            "a surface begins an action whose machine declares the pre-migration name field and then obeys the handoff exactly",
            &[],
            &[("orch", "JsonValue")],
            |_ctx, _params| {
                let out = run_begin_legacy_named_action_obeying_handoff()?;
                Ok(store("orch", out))
            },
        ),
        step_def(
            "a surface begins a track through anvil_orchestrate with track_name {string} parent_id {string} approver {string}",
            &[],
            &[("orch", "JsonValue")],
            |_ctx, params| {
                let track_name = params.get_string(0).ok_or("track_name")?.to_string();
                let parent_id = params.get_string(1).ok_or("parent_id")?.to_string();
                let approver = params.get_string(2).ok_or("approver")?.to_string();
                let out = run_begin_track_with_fields(&track_name, &parent_id, &approver)?;
                Ok(store("orch", out))
            },
        ),
        step_def(
            "a surface ships {string} into anvil_orchestrate in route-mode twice with conversation_id {string} and then with different conversation_id {string} and ctx org {string}",
            &[],
            &[("orch_idempotency", "JsonValue")],
            |_ctx, params| {
                let message = params.get_string(0).ok_or("message")?.to_string();
                let conversation_id = params.get_string(1).ok_or("conversation_id")?.to_string();
                let different_conversation_id = params
                    .get_string(2)
                    .ok_or("different conversation_id")?
                    .to_string();
                let ctx_org = params.get_string(3).ok_or("ctx org")?.to_string();
                let out = run_auto_resolve_idempotency(
                    &message,
                    &conversation_id,
                    &different_conversation_id,
                    &ctx_org,
                )?;
                Ok(store("orch_idempotency", out))
            },
        ),
        step_def(
            "a surface ships {string} into anvil_orchestrate selecting {string}",
            &[],
            &[("orch", "JsonValue")],
            |_ctx, params| {
                let message = params.get_string(0).ok_or("message")?.to_string();
                let selection = params.get_string(1).ok_or("selection")?.to_string();
                let out = run_selection(&message, Some(&selection), "kiln", false)?;
                Ok(store("orch", out))
            },
        ),
        step_def(
            "a surface ships {string} into anvil_orchestrate with hint {string} and advances the step",
            &[],
            &[("orch", "JsonValue")],
            |_ctx, params| {
                let message = params.get_string(0).ok_or("message")?.to_string();
                let hint = params.get_string(1).ok_or("hint")?.to_string();
                let out = run(&message, Some(&hint), "claude-desktop", true)?;
                Ok(store("orch", out))
            },
        ),
        step_def(
            "two surfaces {string} and {string} ship {string} into anvil_orchestrate with hint {string}",
            &[],
            &[("orch_a", "JsonValue"), ("orch_b", "JsonValue")],
            |_ctx, params| {
                let sa = params.get_string(0).ok_or("surface a")?.to_string();
                let sb = params.get_string(1).ok_or("surface b")?.to_string();
                let message = params.get_string(2).ok_or("message")?.to_string();
                let hint = params.get_string(3).ok_or("hint")?.to_string();
                let a = run(&message, Some(&hint), &sa, false)?;
                let b = run(&message, Some(&hint), &sb, false)?;
                let mut out = Context::new();
                out.set("orch_a", a);
                out.set("orch_b", b);
                Ok(out)
            },
        ),
        step_def(
            "a surface routes and then selects {string} for {string} with conversation_id {string} and confidence {string}",
            &[],
            &[("orch", "JsonValue")],
            |_ctx, params| {
                let selection = params.get_string(0).ok_or("selection")?.to_string();
                let message = params.get_string(1).ok_or("message")?.to_string();
                let conversation_id = params.get_string(2).ok_or("conversation_id")?.to_string();
                let confidence = params.get_string(3).ok_or("confidence")?.to_string();
                let out = run_route_then_select(
                    &message,
                    &selection,
                    &conversation_id,
                    &confidence,
                    None,
                    true,
                )?;
                Ok(store("orch", out))
            },
        ),
        step_def(
            "a surface selects {string} for a UTF-8 multiline message with conversation_id {string} and confidence {string}",
            &[],
            &[("orch", "JsonValue")],
            |_ctx, params| {
                let selection = params.get_string(0).ok_or("selection")?.to_string();
                let conversation_id = params.get_string(1).ok_or("conversation_id")?.to_string();
                let confidence = params.get_string(2).ok_or("confidence")?.to_string();
                let out = run_begin_only_utf8(&selection, &conversation_id, &confidence)?;
                Ok(store("orch", out))
            },
        ),
        step_def(
            "a surface routes {string} with routing_hint {string} and conversation_id {string}",
            &[],
            &[("orch", "JsonValue")],
            |_ctx, params| {
                let message = params.get_string(0).ok_or("message")?.to_string();
                let routing_hint = params.get_string(1).ok_or("routing_hint")?.to_string();
                let conversation_id = params.get_string(2).ok_or("conversation_id")?.to_string();
                let out = run_route_only(&message, &routing_hint, &conversation_id)?;
                Ok(store("orch", out))
            },
        ),
        step_def(
            "a surface routes {string} into anvil_orchestrate in route-mode with conversation_id {string} surface {string} and ctx org {string}",
            &[],
            &[("orch", "JsonValue")],
            |_ctx, params| {
                let message = params.get_string(0).ok_or("message")?.to_string();
                let conversation_id = params.get_string(1).ok_or("conversation_id")?.to_string();
                let surface = params.get_string(2).ok_or("surface")?.to_string();
                let ctx_org = params.get_string(3).ok_or("ctx org")?.to_string();
                let out = run_route_correlation(&message, &conversation_id, &surface, &ctx_org)?;
                Ok(store("orch", out))
            },
        ),
        step_def(
            "a surface routes with routing_hint {string} and follows next_call selecting {string} for {string} with conversation_id {string} and confidence {string}",
            &[],
            &[("orch", "JsonValue")],
            |_ctx, params| {
                let routing_hint = params.get_string(0).ok_or("routing_hint")?.to_string();
                let selection = params.get_string(1).ok_or("selection")?.to_string();
                let message = params.get_string(2).ok_or("message")?.to_string();
                let conversation_id = params.get_string(3).ok_or("conversation_id")?.to_string();
                let confidence = params.get_string(4).ok_or("confidence")?.to_string();
                let out = run_route_then_select(
                    &message,
                    &selection,
                    &conversation_id,
                    &confidence,
                    Some(&routing_hint),
                    true,
                )?;
                Ok(store("orch", out))
            },
        ),
        step_def(
            "a surface routes with routing_hint {string} and then selects {string} for {string} with conversation_id {string} and confidence {string}",
            &[],
            &[("orch", "JsonValue")],
            |_ctx, params| {
                let routing_hint = params.get_string(0).ok_or("routing_hint")?.to_string();
                let selection = params.get_string(1).ok_or("selection")?.to_string();
                let message = params.get_string(2).ok_or("message")?.to_string();
                let conversation_id = params.get_string(3).ok_or("conversation_id")?.to_string();
                let confidence = params.get_string(4).ok_or("confidence")?.to_string();
                let out = run_route_then_select(
                    &message,
                    &selection,
                    &conversation_id,
                    &confidence,
                    Some(&routing_hint),
                    false,
                )?;
                Ok(store("orch", out))
            },
        ),
        check_def(
            "the handoff routes to playbook {string} at state {string} role {string}",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let want_playbook = params.get_string(0).ok_or("playbook")?;
                let want_state = params.get_string(1).ok_or("state")?;
                let want_role = params.get_string(2).ok_or("role")?;
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let h = &o["handoff"];
                if h["playbook_id"].as_str() != Some(want_playbook) {
                    return Err(format!(
                        "playbook_id: want {}, got {}; handoff={}",
                        want_playbook, h["playbook_id"], h
                    ));
                }
                if h["state"].as_str() != Some(want_state) {
                    return Err(format!("state: want {}, got {}", want_state, h["state"]));
                }
                if h["role"].as_str() != Some(want_role) {
                    return Err(format!("role: want {}, got {}", want_role, h["role"]));
                }
                Ok(())
            },
        ),
        check_def(
            "the handoff carries a non-empty hook context_text",
            &[("orch", "JsonValue")],
            |ctx, _params| {
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let ct = o["handoff"]["context_text"].as_str().unwrap_or("");
                if ct.is_empty() {
                    return Err("context_text is empty".to_string());
                }
                Ok(())
            },
        ),
        check_def(
            "the handoff carries the step intent and expected_output",
            &[("orch", "JsonValue")],
            |ctx, _params| {
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let h = &o["handoff"];
                if h["intent"].as_str().unwrap_or("").is_empty() {
                    return Err("intent is empty".to_string());
                }
                if h["expected_output"].as_str().unwrap_or("").is_empty() {
                    return Err("expected_output is empty".to_string());
                }
                Ok(())
            },
        ),
        check_def(
            "the handoff next_call advances via {string}",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("tool")?;
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let tool = o["handoff"]["next_call"]["tool"].as_str().unwrap_or("");
                if tool != want {
                    return Err(format!("next_call.tool: want {}, got {}", want, tool));
                }
                Ok(())
            },
        ),
        check_def(
            "the handoff next_call artifact_path starts with {string}",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let prefix = params.get_string(0).ok_or("prefix")?.to_string();
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let path = o["handoff"]["next_call"]["arguments"]["artifact_path"]
                    .as_str()
                    .unwrap_or("");
                if path.starts_with(&prefix) {
                    Ok(())
                } else {
                    Err(format!("artifact_path '{}' does not start with '{}'", path, prefix))
                }
            },
        ),
        check_def(
            "the handoff next_call artifact_path contains {string}",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("needle")?.to_string();
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let path = o["handoff"]["next_call"]["arguments"]["artifact_path"]
                    .as_str()
                    .unwrap_or("");
                if path.contains(&needle) {
                    Ok(())
                } else {
                    Err(format!("artifact_path '{}' does not contain '{}'", path, needle))
                }
            },
        ),
        check_def(
            "the handoff is not an error",
            &[("orch", "JsonValue")],
            |ctx, _params| {
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                if o["is_error"].as_bool().unwrap_or(false) {
                    return Err(format!("expected success, got error: {}", o["handoff"]));
                }
                Ok(())
            },
        ),
        check_def(
            "the handoff is an error",
            &[("orch", "JsonValue")],
            |ctx, _params| {
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                if !o["is_error"].as_bool().unwrap_or(false) {
                    return Err(format!("expected an error, got success: {}", o["handoff"]));
                }
                Ok(())
            },
        ),
        check_def(
            "the handoff outcome is {string}",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("outcome")?;
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let got = o["handoff"]["outcome"].as_str().unwrap_or("");
                if got != want.as_ref() as &str {
                    return Err(format!("outcome: want {}, got {}", want, got));
                }
                Ok(())
            },
        ),
        check_def(
            "the handoff outcome is not {string}",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let unwanted = params.get_string(0).ok_or("outcome")?;
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let got = o["handoff"]["outcome"].as_str().unwrap_or("");
                if got == unwanted.as_ref() as &str {
                    return Err(format!("outcome unexpectedly is {}: {}", unwanted, o["handoff"]));
                }
                Ok(())
            },
        ),
        check_def(
            "the handoff next_call carries a fields object pre-keyed with {string}",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let expected: Vec<String> = params
                    .get_string(0)
                    .ok_or("fields")?
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect();
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let fields = o["handoff"]["next_call"]["arguments"]["fields"]
                    .as_object()
                    .ok_or_else(|| {
                        format!(
                            "next_call.arguments.fields is not an object: {}",
                            o["handoff"]["next_call"]["arguments"]
                        )
                    })?;
                for key in &expected {
                    match fields.get(key) {
                        Some(v) if v.is_string() => {}
                        Some(v) => return Err(format!("fields.{} is not a string: {}", key, v)),
                        None => return Err(format!("fields object missing key '{}': {:?}", key, fields)),
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "the handoff next_call carries no fields object",
            &[("orch", "JsonValue")],
            |ctx, _params| {
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let fields = &o["handoff"]["next_call"]["arguments"]["fields"];
                if fields.is_null() {
                    Ok(())
                } else {
                    Err(format!(
                        "next_call.arguments.fields should be absent, got: {}",
                        fields
                    ))
                }
            },
        ),
        check_def(
            "the handoff next_call arguments name {string}",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("argument name")?.to_string();
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let args = o["handoff"]["next_call"]["arguments"]
                    .as_object()
                    .ok_or_else(|| {
                        format!(
                            "next_call.arguments is not an object: {}",
                            o["handoff"]["next_call"]
                        )
                    })?;
                if args.contains_key(&key) {
                    Ok(())
                } else {
                    Err(format!(
                        "next_call.arguments missing '{}': {:?}",
                        key,
                        args.keys().collect::<Vec<_>>()
                    ))
                }
            },
        ),
        // resume_aware_routing C1 — the route-mode resume handoff surfaces the open
        // playbook (NOT an empty candidates response).
        check_def(
            "the handoff resumes artifact {string} of kind {string} in state {string}",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let want_id = params.get_string(0).ok_or("artifact")?;
                let want_kind = params.get_string(1).ok_or("kind")?;
                let want_state = params.get_string(2).ok_or("state")?;
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let h = &o["handoff"];
                let got_id = h["resume_artifact_id"].as_str().unwrap_or("");
                let got_kind = h["resume_kind"].as_str().unwrap_or("");
                let got_state = h["resume_state"].as_str().unwrap_or("");
                if got_id != want_id.as_ref() as &str {
                    return Err(format!("resume_artifact_id: want {}, got {} (handoff: {})", want_id, got_id, h));
                }
                if got_kind != want_kind.as_ref() as &str {
                    return Err(format!("resume_kind: want {}, got {}", want_kind, got_kind));
                }
                if got_state != want_state.as_ref() as &str {
                    return Err(format!("resume_state: want {}, got {}", want_state, got_state));
                }
                Ok(())
            },
        ),
        check_def(
            "the handoff carries a non-empty resume advance action",
            &[("orch", "JsonValue")],
            |ctx, _params| {
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let action = o["handoff"]["resume_advance_action"].as_str().unwrap_or("");
                if action.trim().is_empty() {
                    return Err(format!(
                        "expected a non-empty resume_advance_action, got empty (handoff: {})",
                        o["handoff"]
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the handoff candidates include kind {string}",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("kind")?;
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let cands = o["handoff"]["candidates"].as_array().cloned().unwrap_or_default();
                if cands.iter().any(|c| c["kind"].as_str() == Some(want.as_ref())) {
                    Ok(())
                } else {
                    Err(format!("expected candidate kind '{}' in {:?}", want, cands))
                }
            },
        ),
        check_def(
            "the handoff candidates do not include kind {string}",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("kind")?;
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let cands = o["handoff"]["candidates"].as_array().cloned().unwrap_or_default();
                if cands.iter().any(|c| c["kind"].as_str() == Some(want.as_ref())) {
                    Err(format!("candidate kind '{}' was unexpectedly present in {:?}", want, cands))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the handoff did not auto-begin",
            &[("orch", "JsonValue")],
            |ctx, _params| {
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let h = &o["handoff"];
                if h["mode"].as_str() == Some("route_auto_resolved")
                    || h["outcome"].as_str() == Some("auto_resolved")
                    || h.get("state").is_some()
                {
                    Err(format!("expected candidate handoff without auto-begin, got {}", h))
                } else {
                    Ok(())
                }
            },
        ),
        // route_response_mirrors_begin H4 — ADVISORY-ONLY single: returns guidance
        // + the begin call + required_fields, performs NO begin / NO transition.
        check_def(
            "the handoff is advisory single for kind {string}",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("kind")?;
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let h = &o["handoff"];
                if h["outcome"].as_str() != Some("single") {
                    return Err(format!("expected outcome 'single', got {}", h["outcome"]));
                }
                if h["mode"].as_str() != Some("route_advisory") {
                    return Err(format!("expected mode 'route_advisory', got {}", h["mode"]));
                }
                if h["resolution_outcome"].as_str() != Some("single") {
                    return Err(format!("expected resolution_outcome 'single', got {}", h["resolution_outcome"]));
                }
                if h["selected_kind"].as_str() != Some(want.as_ref()) {
                    return Err(format!("selected_kind: want {}, got {}", want, h["selected_kind"]));
                }
                // Advisory means NO begin happened: no artifact state / path on the
                // handoff (the model still has to call begin via next_call).
                if h.get("state").is_some() || h.get("artifact_path").is_some() {
                    return Err(format!("advisory single must NOT begin, but handoff carries begin output: {}", h));
                }
                Ok(())
            },
        ),
        check_def(
            "the handoff next_call begins kind {string}",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("kind")?;
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let nc = &o["handoff"]["next_call"];
                if nc["tool"].as_str() != Some("anvil_orchestrate") {
                    return Err(format!("expected next_call tool anvil_orchestrate, got {}", nc["tool"]));
                }
                if nc["arguments"]["selection"].as_str() != Some(want.as_ref()) {
                    return Err(format!("next_call selection: want {}, got {}", want, nc["arguments"]["selection"]));
                }
                Ok(())
            },
        ),
        check_def(
            "the handoff carries a non-empty route guidance",
            &[("orch", "JsonValue")],
            |ctx, _params| {
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let g = o["handoff"]["guidance"].as_str().unwrap_or("");
                if g.trim().is_empty() {
                    Err(format!("expected non-empty guidance, got {:?}", o["handoff"]["guidance"]))
                } else {
                    Ok(())
                }
            },
        ),
        // route_response_mirrors_begin H3 (MCP route-mode fail-open): the engine
        // degraded to a thin route, so the advisory single carries empty guidance
        // — but it is STILL a successful advisory route, not a tool error.
        check_def(
            "the handoff carries an empty route guidance",
            &[("orch", "JsonValue")],
            |ctx, _params| {
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let g = o["handoff"]["guidance"].as_str().unwrap_or("");
                if g.is_empty() {
                    Ok(())
                } else {
                    Err(format!("expected empty guidance (fail-open thin route), got {:?}", g))
                }
            },
        ),
        check_def(
            "the anvil_orchestrate route durable records carry conversation_hash and source {string}",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let expected_source = params.get_string(0).ok_or("source")?;
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let routing_records = o["routing_activity_records"]
                    .as_array()
                    .ok_or("routing_activity_records is not an array")?;
                let routing_record = routing_records
                    .iter()
                    .find(|record| record["kind"].as_str() == Some("daily_recap"))
                    .ok_or_else(|| format!("no daily_recap routing-activity record: {}", o["routing_activity_records"]))?;
                let routing_hash = routing_record["conversation_hash"].as_str().unwrap_or("");
                if routing_hash.is_empty() {
                    return Err(format!(
                        "routing-activity record lacks conversation_hash: {}",
                        routing_record
                    ));
                }

                let activity_records = o["activity_log_records"]
                    .as_array()
                    .ok_or("activity_log_records is not an array")?;
                let activity_record = activity_records
                    .iter()
                    .find(|record| record["command"].as_str() == Some("route"))
                    .ok_or_else(|| format!("no route activity-log record: {}", o["activity_log_records"]))?;
                let activity_hash = activity_record["conversation_hash"].as_str().unwrap_or("");
                if activity_hash.is_empty() {
                    return Err(format!(
                        "activity-log route record lacks conversation_hash: {}",
                        activity_record
                    ));
                }
                if activity_record["source"].as_str() != Some(expected_source.as_ref()) {
                    return Err(format!(
                        "activity-log route source: want {}, got {:?}; record={}",
                        expected_source,
                        activity_record.get("source"),
                        activity_record
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "no daily_recap instance was created by the route call",
            &[("orch", "JsonValue")],
            |ctx, _params| {
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let count = o["daily_recap_count"].as_u64().unwrap_or(u64::MAX);
                if count == 0 {
                    Ok(())
                } else {
                    Err(format!("advisory single must NOT begin; found {} daily_recap instance(s)", count))
                }
            },
        ),
        check_def(
            "both handoffs are not errors",
            &[("orch_idempotency", "JsonValue")],
            |ctx, _params| {
                let o = ctx
                    .get::<serde_json::Value>("orch_idempotency")
                    .ok_or("no orch_idempotency")?;
                if o["first_is_error"].as_bool().unwrap_or(true)
                    || o["second_is_error"].as_bool().unwrap_or(true)
                {
                    Err(format!(
                        "expected first and second handoffs not to be errors, got {}",
                        o
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the repeated handoff returns the same artifact_path",
            &[("orch_idempotency", "JsonValue")],
            |ctx, _params| {
                let o = ctx
                    .get::<serde_json::Value>("orch_idempotency")
                    .ok_or("no orch_idempotency")?;
                let first = o["first_artifact_path"].as_str().unwrap_or("");
                let second = o["second_artifact_path"].as_str().unwrap_or("");
                if !first.is_empty() && first == second {
                    Ok(())
                } else {
                    Err(format!(
                        "expected repeated artifact_path to match first; first='{}' second='{}'",
                        first, second
                    ))
                }
            },
        ),
        check_def(
            "exactly one daily_recap instance exists in the hearth",
            &[("orch_idempotency", "JsonValue")],
            |ctx, _params| {
                let o = ctx
                    .get::<serde_json::Value>("orch_idempotency")
                    .ok_or("no orch_idempotency")?;
                let count = o["count_after_repeat"].as_u64().unwrap_or(0);
                if count == 1 {
                    Ok(())
                } else {
                    Err(format!(
                        "expected one daily_recap after repeat, got {}; {}",
                        count, o
                    ))
                }
            },
        ),
        check_def(
            "the different conversation handoff returns a different artifact_path",
            &[("orch_idempotency", "JsonValue")],
            |ctx, _params| {
                let o = ctx
                    .get::<serde_json::Value>("orch_idempotency")
                    .ok_or("no orch_idempotency")?;
                if o["different_is_error"].as_bool().unwrap_or(true) {
                    return Err(format!("different conversation handoff was an error: {}", o));
                }
                let first = o["first_artifact_path"].as_str().unwrap_or("");
                let different = o["different_artifact_path"].as_str().unwrap_or("");
                if !first.is_empty() && !different.is_empty() && first != different {
                    Ok(())
                } else {
                    Err(format!(
                        "expected different conversation artifact_path to differ; first='{}' different='{}'",
                        first, different
                    ))
                }
            },
        ),
        check_def(
            "exactly two daily_recap instances exist in the hearth",
            &[("orch_idempotency", "JsonValue")],
            |ctx, _params| {
                let o = ctx
                    .get::<serde_json::Value>("orch_idempotency")
                    .ok_or("no orch_idempotency")?;
                let count = o["count_after_different"].as_u64().unwrap_or(0);
                if count == 2 {
                    Ok(())
                } else {
                    Err(format!(
                        "expected two daily_recap instances after different conversation, got {}; {}",
                        count, o
                    ))
                }
            },
        ),
        check_def(
            "the handoff next_call re-invokes {string} carrying a selection",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let want_tool = params.get_string(0).ok_or("tool")?;
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let next = &o["handoff"]["next_call"];
                if next["tool"].as_str() != Some(want_tool.as_ref()) {
                    return Err(format!("next_call.tool: want {}, got {}", want_tool, next["tool"]));
                }
                if next["arguments"].get("selection").is_none() {
                    return Err("next_call.arguments missing `selection`".to_string());
                }
                Ok(())
            },
        ),
        check_def(
            "the handoff missing fields are {string}",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let expected = params
                    .get_string(0)
                    .ok_or("expected fields")?
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect::<Vec<_>>();
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let fields = o["handoff"]["missing_fields"]
                    .as_array()
                    .ok_or_else(|| format!("missing_fields is not an array: {}", o["handoff"]))?
                    .iter()
                    .filter_map(|v| v.as_str().map(ToString::to_string))
                    .collect::<Vec<_>>();
                if fields == expected {
                    Ok(())
                } else {
                    Err(format!("missing_fields: want {:?}, got {:?}", expected, fields))
                }
            },
        ),
        check_def(
            "the handoff next_call re-invokes {string} carrying required field placeholders",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let want_tool = params.get_string(0).ok_or("tool")?;
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let next = &o["handoff"]["next_call"];
                if next["tool"].as_str() != Some(want_tool.as_ref()) {
                    return Err(format!("next_call.tool: want {}, got {}", want_tool, next["tool"]));
                }
                let args = &next["arguments"];
                for key in ["track_name", "parent_id", "approver"] {
                    if !args.get(key).and_then(|v| v.as_str()).unwrap_or("").starts_with('<') {
                        return Err(format!("next_call.arguments.{} missing placeholder in {}", key, args));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "no track artifact was created by the handoff",
            &[("orch", "JsonValue")],
            |ctx, _params| {
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let before = o["track_count_before"].as_u64().unwrap_or(u64::MAX);
                let after = o["track_count_after"].as_u64().unwrap_or(u64::MAX);
                if before == after {
                    Ok(())
                } else {
                    Err(format!("track count changed from {} to {}; {}", before, after, o))
                }
            },
        ),
        check_def(
            "the routing_decision route-mode record has turn_id {string} and candidate {string}",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let turn_id = params.get_string(0).ok_or("turn_id")?.to_string();
                let candidate = params.get_string(1).ok_or("candidate")?.to_string();
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let matches = routing_record(o, &turn_id, "route", None);
                if matches.iter().any(|r| {
                    r["candidate_set"]
                        .as_str()
                        .unwrap_or("")
                        .split(',')
                        .any(|kind| kind == candidate)
                }) {
                    Ok(())
                } else {
                    Err(format!(
                        "no route-mode routing_decision for turn_id {} candidate {}; records: {}",
                        turn_id, candidate, o["routing_records"]
                    ))
                }
            },
        ),
        check_def(
            "the routing_decision begin-mode record has turn_id {string} selected {string} confidence {string}",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let turn_id = params.get_string(0).ok_or("turn_id")?.to_string();
                let selected = params.get_string(1).ok_or("selected")?.to_string();
                let confidence = params.get_string(2).ok_or("confidence")?.to_string();
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let matches = routing_record(o, &turn_id, "begin", Some(&selected));
                if matches
                    .iter()
                    .any(|r| r["confidence"].as_str() == Some(confidence.as_str()))
                {
                    Ok(())
                } else {
                    Err(format!(
                        "no begin-mode routing_decision for turn_id {} selected {} confidence {}; records: {}",
                        turn_id, selected, confidence, o["routing_records"]
                    ))
                }
            },
        ),
        check_def(
            "exactly one routing_decision route-mode record has turn_id {string}",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let turn_id = params.get_string(0).ok_or("turn_id")?.to_string();
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let count = routing_record(o, &turn_id, "route", None).len();
                if count == 1 {
                    Ok(())
                } else {
                    Err(format!(
                        "expected exactly one route-mode record for {}, got {}; records: {}",
                        turn_id, count, o["routing_records"]
                    ))
                }
            },
        ),
        check_def(
            "exactly one routing_decision begin-mode record has turn_id {string} selected {string} confidence {string}",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let turn_id = params.get_string(0).ok_or("turn_id")?.to_string();
                let selected = params.get_string(1).ok_or("selected")?.to_string();
                let confidence = params.get_string(2).ok_or("confidence")?.to_string();
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let count = routing_record(o, &turn_id, "begin", Some(&selected))
                    .into_iter()
                    .filter(|r| r["confidence"].as_str() == Some(confidence.as_str()))
                    .count();
                if count == 1 {
                    Ok(())
                } else {
                    Err(format!(
                        "expected exactly one begin-mode record for {} selected {} confidence {}, got {}; records: {}",
                        turn_id, selected, confidence, count, o["routing_records"]
                    ))
                }
            },
        ),
        check_def(
            "exactly one routing_decision route-mode record has turn_id {string} selected {string} resolution_outcome {string}",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let turn_id = params.get_string(0).ok_or("turn_id")?.to_string();
                let selected = params.get_string(1).ok_or("selected")?.to_string();
                let resolution_outcome = params.get_string(2).ok_or("resolution_outcome")?.to_string();
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let count = routing_record(o, &turn_id, "route", Some(&selected))
                    .into_iter()
                    .filter(|r| r["resolution_outcome"].as_str() == Some(resolution_outcome.as_str()))
                    .count();
                if count == 1 {
                    Ok(())
                } else {
                    Err(format!(
                        "expected exactly one route-mode record for {} selected {} resolution_outcome {}, got {}; records: {}",
                        turn_id, selected, resolution_outcome, count, o["routing_records"]
                    ))
                }
            },
        ),
        check_def(
            "exactly one routing_decision begin-mode record has turn_id {string} selected {string}",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let turn_id = params.get_string(0).ok_or("turn_id")?.to_string();
                let selected = params.get_string(1).ok_or("selected")?.to_string();
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let count = routing_record(o, &turn_id, "begin", Some(&selected)).len();
                if count == 1 {
                    Ok(())
                } else {
                    Err(format!(
                        "expected exactly one begin-mode record for {} selected {}, got {}; records: {}",
                        turn_id, selected, count, o["routing_records"]
                    ))
                }
            },
        ),
        check_def(
            "no routing_decision begin-mode record has turn_id {string}",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let turn_id = params.get_string(0).ok_or("turn_id")?.to_string();
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let count = routing_record(o, &turn_id, "begin", None).len();
                if count == 0 {
                    Ok(())
                } else {
                    Err(format!(
                        "expected no begin-mode record for {}, got {}; records: {}",
                        turn_id, count, o["routing_records"]
                    ))
                }
            },
        ),
        check_def(
            "no routing_decision begin-mode record double-counts the route decision for turn_id {string}",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let turn_id = params.get_string(0).ok_or("turn_id")?.to_string();
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let count = routing_record(o, &turn_id, "begin", None)
                    .into_iter()
                    .filter(|r| r["resolution_outcome"].as_str() == Some("single"))
                    .count();
                if count == 0 {
                    Ok(())
                } else {
                    Err(format!(
                        "expected begin-mode records for {} not to carry the route decision resolution_outcome, got {}; records: {}",
                        turn_id, count, o["routing_records"]
                    ))
                }
            },
        ),
        check_def(
            "the handoff next_call arguments include routing_hint {string}",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("routing_hint")?.to_string();
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let got = o["route_handoff"]["next_call"]["arguments"]["routing_hint"]
                    .as_str()
                    .unwrap_or("");
                if got == want {
                    Ok(())
                } else {
                    Err(format!("next_call routing_hint: want {}, got {}", want, got))
                }
            },
        ),
        check_def(
            "the routing_decision route-mode and begin-mode records for turn_id {string} both have input {string}",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let turn_id = params.get_string(0).ok_or("turn_id")?.to_string();
                let input = params.get_string(1).ok_or("input")?.to_string();
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                let has_route = routing_record(o, &turn_id, "route", None)
                    .iter()
                    .any(|r| r["input"].as_str() == Some(input.as_str()));
                let has_begin = routing_record(o, &turn_id, "begin", None)
                    .iter()
                    .any(|r| r["input"].as_str() == Some(input.as_str()));
                if has_route && has_begin {
                    Ok(())
                } else {
                    Err(format!(
                        "expected route and begin records for {} with input {}; records: {}",
                        turn_id, input, o["routing_records"]
                    ))
                }
            },
        ),
        check_def(
            "the handoff is a typed driven-kind rejection error",
            &[("orch", "JsonValue")],
            |ctx, _params| {
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                if !o["is_error"].as_bool().unwrap_or(false) {
                    return Err("expected an MCP tool error".to_string());
                }
                let text = o["handoff"].as_str().unwrap_or("");
                if !text.contains("not a driven candidate") {
                    return Err(format!("expected driven-kind rejection, got: {}", text));
                }
                Ok(())
            },
        ),
        check_def(
            "the playbook advanced to state {string} and the step was measured",
            &[("orch", "JsonValue")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("state")?;
                let o = ctx.get::<serde_json::Value>("orch").ok_or("no orch")?;
                if o["post_state"].as_str() != Some(want) {
                    return Err(format!("post_state: want {}, got {}", want, o["post_state"]));
                }
                if !o["measured"].as_bool().unwrap_or(false) {
                    return Err("engine did not emit a step_measurement record".to_string());
                }
                Ok(())
            },
        ),
        check_def(
            "both surfaces routed identically",
            &[("orch_a", "JsonValue"), ("orch_b", "JsonValue")],
            |ctx, _params| {
                let a = ctx.get::<serde_json::Value>("orch_a").ok_or("no orch_a")?;
                let b = ctx.get::<serde_json::Value>("orch_b").ok_or("no orch_b")?;
                let ha = &a["handoff"];
                let hb = &b["handoff"];
                for k in ["playbook_id", "state", "role", "context_text"] {
                    if ha[k] != hb[k] {
                        return Err(format!("surface-dependent on {}: {} vs {}", k, ha[k], hb[k]));
                    }
                }
                Ok(())
            },
        ),
    ]
}
