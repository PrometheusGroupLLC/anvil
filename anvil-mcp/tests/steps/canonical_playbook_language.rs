//! In-crate step definitions for `features/canonical_playbook_language_mcp.feature`
//! (track `retire_the_workflow_term`, task A.3).
//!
//! Every assertion here reads the REAL stdio MCP server's responses — the shim
//! binary is spawned against a real `anvil-engine` over a real hearth, and the
//! evidence is `tools/list`, a real `anvil_orchestrate` resume response, a real
//! `amend` call carrying the canonical `kind: playbook` wire value, and a real
//! `amend` call carrying the RETIRED `kind: workflow` value that must now be
//! refused. Reading `src/main.rs` (or grepping source) is never acceptance
//! evidence and is not done here.

use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The conversation whose open playbook run the resume leg must find.
const RESUME_CONVERSATION: &str = "a3-mcp-canonical-language";
/// The open (begun, non-terminal) run seeded for that conversation.
const RESUME_ARTIFACT: &str = "20260601T0900_open_playbook_run";
/// A second, independent artifact so the amend calls cannot perturb the
/// open-run marker the resume leg depends on.
const AMEND_ARTIFACT: &str = "20260601T0901_canonical_amendment_target";

/// The canonical amendment-kind wire value for a playbook definition.
const CANONICAL_AMENDMENT_KIND: &str = "playbook";
/// The retired value it replaces. The real server must refuse it: one name.
const RETIRED_AMENDMENT_KIND: &str = "workflow";

/// Retired wire identifiers this surface must no longer advertise or accept.
/// Each is checked by NAME against the advertised argument set and the
/// advertised prose, so a survivor is reported as itself rather than as a bare
/// substring hit.
const RETIRED_WIRE_TOKENS: &[&str] = &[
    "workflow_generation",
    "workflow_hint",
    "workflow_name",
    "workflow_unknown_hook_reference",
    "available_workflow_kinds",
    "supported_workflow",
    "workflow_instance_id",
    "workflow_kind",
];

// ---------------------------------------------------------------------------
// Fixture: a real hearth, a real engine, a real stdio shim.
// ---------------------------------------------------------------------------

struct A3Fixture {
    _hearth_dir: tempfile::TempDir,
    hearth: PathBuf,
    engine: Child,
    shim: Child,
    next_id: i64,
}

impl Drop for A3Fixture {
    fn drop(&mut self) {
        let _ = self.shim.kill();
        let _ = self.shim.wait();
        let _ = self.engine.kill();
        let _ = self.engine.wait();
    }
}

impl A3Fixture {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        {
            let stdin = self.shim.stdin.as_mut().ok_or("no shim stdin")?;
            let line = serde_json::to_string(&request).map_err(|e| e.to_string())?;
            writeln!(stdin, "{}", line).map_err(|e| format!("write shim stdin: {}", e))?;
            stdin.flush().map_err(|e| format!("flush shim stdin: {}", e))?;
        }
        let stdout = self.shim.stdout.as_mut().ok_or("no shim stdout")?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| format!("read shim stdout: {}", e))?;
        if line.trim().is_empty() {
            return Err(format!(
                "no response to {} (shim stdout closed or empty)",
                method
            ));
        }
        serde_json::from_str(&line)
            .map_err(|e| format!("parse {} response: {} (raw: {})", method, e, line.trim()))
    }

    fn tools_call(&mut self, name: &str, arguments: Value) -> Result<Value, String> {
        self.call(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        )
    }
}

fn free_port() -> Result<u16, String> {
    let listener =
        TcpListener::bind("127.0.0.1:0").map_err(|e| format!("bind ephemeral port: {}", e))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("local_addr: {}", e))?
        .port();
    drop(listener);
    Ok(port)
}

fn wait_for_port(port: u16) -> bool {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn repo_root() -> PathBuf {
    // anvil/anvil-mcp -> anvil
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("anvil-mcp has a parent directory")
        .to_path_buf()
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else if file_type.is_file() {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// A `status.yaml` for a track artifact. When `conversation_id` is supplied the
/// artifact carries an OPEN begin marker for that conversation, which is what
/// makes the engine's resume pre-check fire.
fn track_status_yaml(state: &str, conversation_id: Option<&str>) -> String {
    let mut yaml = format!(
        "version: 1\nkind: track\nstate: {state}\ntransitions:\n  - to: {state}\n    at: \"2026-06-01T00:00:00Z\"\n    actor: Seeder-000000\n    role: doer\n",
        state = state
    );
    if let Some(conversation_id) = conversation_id {
        yaml.push_str(&format!(
            "activity:\n  - kind: begin\n    actor: Beginner-111111\n    state: {state}\n    at: \"2026-06-01T10:00:00Z\"\n    conversation_id: \"{conversation_id}\"\n",
            state = state,
            conversation_id = conversation_id
        ));
    }
    yaml
}

fn build_hearth() -> Result<(tempfile::TempDir, PathBuf), String> {
    let dir = tempfile::Builder::new()
        .prefix("anvil-a3-mcp-hearth-")
        .tempdir()
        .map_err(|e| format!("temp hearth: {}", e))?;
    let hearth = dir.path().to_path_buf();
    let write = |rel: &str, body: &str| -> Result<(), String> {
        let path = hearth.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {}", rel, e))?;
        }
        std::fs::write(&path, body).map_err(|e| format!("write {}: {}", rel, e))
    };

    write(".hearth", "path: .\n")?;
    write("tracks.md", "# Tracks\n")?;
    write(
        "proposals.md",
        "# Proposals\n\n## active\n\n- [seed](proposals/seedprop/)\n",
    )?;
    write("projections/execution.md", "# Execution\n")?;
    write(
        "proposals/seedprop/status.yaml",
        "version: 1\nkind: proposal\nstate: active\nactors: {}\ntransitions:\n  - to: active\n    at: \"2026-06-01T00:00:00Z\"\n    actor: Seeder-000000\n    role: author\n",
    )?;
    write("proposals/seedprop/proposal.md", "# Seed proposal\n")?;

    // The real shipped playbook definitions — the engine resolves `track` (and
    // its hooks) from these, so the resume response is produced by the same
    // registry production uses.
    copy_dir_all(&repo_root().join("playbooks"), &hearth.join("playbooks"))
        .map_err(|e| format!("copy playbooks: {}", e))?;

    write(
        &format!("tracks/{}/status.yaml", RESUME_ARTIFACT),
        &track_status_yaml("spec", Some(RESUME_CONVERSATION)),
    )?;
    write(
        &format!("tracks/{}/spec.md", RESUME_ARTIFACT),
        "# Open playbook run\n",
    )?;
    write(
        &format!("tracks/{}/status.yaml", AMEND_ARTIFACT),
        &track_status_yaml("spec", None),
    )?;
    write(
        &format!("tracks/{}/spec.md", AMEND_ARTIFACT),
        "# Legacy amendment target\n",
    )?;

    Ok((dir, hearth))
}

fn start_fixture() -> Result<A3Fixture, String> {
    let (dir, hearth) = build_hearth()?;
    let port = free_port()?;

    let engine_binary = anvil_test_support::harness::binary_path("anvil-engine");
    let engine = Command::new(&engine_binary)
        .env("ANVIL_RENDEZVOUS_DISABLE", "1")
        .env(
            "ANVIL_TELEMETRY_DIR",
            std::env::temp_dir().join("anvil-brine-telemetry"),
        )
        .arg("--hearth")
        .arg(&hearth)
        .arg("--port")
        .arg(port.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn {}: {}", engine_binary.display(), e))?;

    if !wait_for_port(port) {
        return Err(format!(
            "anvil-engine never accepted connections on 127.0.0.1:{}",
            port
        ));
    }

    let shim_binary = anvil_test_support::harness::binary_path("anvil-mcp");
    let shim = Command::new(&shim_binary)
        .current_dir(&hearth)
        .env("ANVIL_ENGINE_PORT", port.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn {}: {}", shim_binary.display(), e))?;

    let mut fixture = A3Fixture {
        _hearth_dir: dir,
        hearth,
        engine,
        shim,
        next_id: 0,
    };

    fixture.call(
        "initialize",
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "brine-a3", "version": "0.1.0" }
        }),
    )?;

    Ok(fixture)
}

// ---------------------------------------------------------------------------
// Evidence readers
// ---------------------------------------------------------------------------

fn evidence<'a>(ctx: &'a Context) -> Result<&'a Value, String> {
    ctx.get::<Value>("a3_evidence")
        .ok_or_else(|| "no a3_evidence in context (the When step did not run)".to_string())
}

fn tool<'a>(evidence: &'a Value, name: &str) -> Result<&'a Value, String> {
    evidence["tools_list"]["result"]["tools"]
        .as_array()
        .ok_or("tools/list carried no result.tools array")?
        .iter()
        .find(|t| t["name"].as_str() == Some(name))
        .ok_or_else(|| format!("tools/list advertised no `{}` tool", name))
}

fn property_description<'a>(tool: &'a Value, property: &str) -> Result<&'a str, String> {
    tool["inputSchema"]["properties"][property]["description"]
        .as_str()
        .ok_or_else(|| {
            format!(
                "tool `{}` advertises no description for property `{}`",
                tool["name"].as_str().unwrap_or("?"),
                property
            )
        })
}

/// Every advertised prose string: each tool description plus each of its
/// argument descriptions, labeled by where it came from.
fn advertised_prose(evidence: &Value) -> Result<Vec<(String, String)>, String> {
    let tools = evidence["tools_list"]["result"]["tools"]
        .as_array()
        .ok_or("tools/list carried no result.tools array")?;
    let mut prose = Vec::new();
    for t in tools {
        let name = t["name"].as_str().unwrap_or("?").to_string();
        if let Some(desc) = t["description"].as_str() {
            prose.push((format!("{} (tool description)", name), desc.to_string()));
        }
        if let Some(props) = t["inputSchema"]["properties"].as_object() {
            for (property, schema) in props {
                if let Some(desc) = schema["description"].as_str() {
                    prose.push((format!("{}.{}", name, property), desc.to_string()));
                }
                // one nesting level (e.g. persist_playbook.hooks.items)
                if let Some(desc) = schema["items"]["description"].as_str() {
                    prose.push((format!("{}.{}.items", name, property), desc.to_string()));
                }
                if let Some(item_props) = schema["items"]["properties"].as_object() {
                    for (inner, inner_schema) in item_props {
                        if let Some(desc) = inner_schema["description"].as_str() {
                            prose.push((
                                format!("{}.{}.items.{}", name, property, inner),
                                desc.to_string(),
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(prose)
}

/// Every occurrence of `workflow` in `text`, with surrounding context. There
/// is no carve-out: the wire moved with the prose, so any occurrence on an
/// agent-served string is a violation.
fn workflow_occurrences(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut offenders = Vec::new();
    let mut search_from = 0usize;
    while let Some(rel) = lower[search_from..].find("workflow") {
        let at = search_from + rel;
        let end = (at + 60).min(text.len());
        let start = at.saturating_sub(40);
        offenders.push(text[start..end].to_string());
        search_from = at + "workflow".len();
    }
    offenders
}

/// Every advertised argument NAME on every tool, labeled by its tool.
fn advertised_argument_names(evidence: &Value) -> Result<Vec<(String, String)>, String> {
    let tools = evidence["tools_list"]["result"]["tools"]
        .as_array()
        .ok_or("tools/list carried no result.tools array")?;
    let mut names = Vec::new();
    for t in tools {
        let tool_name = t["name"].as_str().unwrap_or("?").to_string();
        names.push(("<tool name>".to_string(), tool_name.clone()));
        if let Some(props) = t["inputSchema"]["properties"].as_object() {
            for (property, schema) in props {
                names.push((tool_name.clone(), property.clone()));
                if let Some(inner_props) = schema["items"]["properties"].as_object() {
                    for inner in inner_props.keys() {
                        names.push((
                            format!("{}.{}.items", tool_name, property),
                            inner.clone(),
                        ));
                    }
                }
            }
        }
    }
    Ok(names)
}

/// Every JSON object key reachable in `value`, with the dotted path to it.
fn collect_keys(value: &Value, path: &str, out: &mut Vec<(String, String)>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                out.push((format!("{}.{}", path, key), key.clone()));
                collect_keys(child, &format!("{}.{}", path, key), out);
            }
        }
        Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                collect_keys(child, &format!("{}[{}]", path, i), out);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Steps
// ---------------------------------------------------------------------------

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a playbook run is already open for the conversation",
            &[],
            &[("a3_fixture", "A3Fixture")],
            |_ctx, _params| {
                let fixture = start_fixture()?;
                let mut out = Context::new();
                out.set("a3_fixture", fixture);
                Ok(out)
            },
        ),
        step_def(
            "the agent lists the Anvil tools and asks to resume",
            &[("a3_fixture", "A3Fixture")],
            &[("a3_evidence", "JsonValue")],
            |mut ctx, _params| {
                let mut fixture = ctx
                    .take::<A3Fixture>("a3_fixture")
                    .ok_or("no a3_fixture in context (the Given step did not run)")?;

                let tools_list = fixture.call("tools/list", json!({}))?;

                // A real resume: a continuation token on the conversation that
                // already has the open run.
                let resume = fixture.tools_call(
                    "anvil_orchestrate",
                    json!({
                        "message": "go",
                        "surface": "claude-code",
                        "conversation_id": RESUME_CONVERSATION,
                        "scope": { "parent_id": "seedprop" },
                        "actor_name": "Resumer-000001",
                        "actor_type": "agent",
                        "actor_model": "claude-opus-5",
                        "actor_provider": "anthropic",
                    }),
                )?;

                // A real call carrying the CANONICAL `kind: playbook` wire value.
                let canonical_amend = fixture.tools_call(
                    "amend",
                    json!({
                        "artifact_path": format!("tracks/{}", AMEND_ARTIFACT),
                        "kind": CANONICAL_AMENDMENT_KIND,
                        "target_document": "spec",
                        "target_id": "CANON-1",
                        "op_kind": "add",
                        "new_kind": "authoring_decision",
                        "body": "The canonical amendment kind is the only callable one.",
                        "actor_name": "Canon-Caller-424242",
                        "actor_type": "agent",
                        "actor_model": "claude-opus-5",
                        "actor_provider": "anthropic",
                    }),
                )?;

                let amendments_path = fixture
                    .hearth
                    .join(format!("tracks/{}/spec.amendments.yaml", AMEND_ARTIFACT));
                let amendments_on_disk = std::fs::read_to_string(&amendments_path).ok();

                // A real call carrying the RETIRED `kind: workflow` value. One
                // name means the server refuses this outright.
                let retired_amend = fixture.tools_call(
                    "amend",
                    json!({
                        "artifact_path": format!("tracks/{}", AMEND_ARTIFACT),
                        "kind": RETIRED_AMENDMENT_KIND,
                        "target_document": "spec",
                        "target_id": "RETIRED-1",
                        "op_kind": "add",
                        "new_kind": "authoring_decision",
                        "body": "The retired amendment kind must not be callable.",
                        "actor_name": "Retired-Caller-424243",
                        "actor_type": "agent",
                        "actor_model": "claude-opus-5",
                        "actor_provider": "anthropic",
                    }),
                )?;

                let after_retired_on_disk = std::fs::read_to_string(&amendments_path).ok();

                let mut out = Context::new();
                out.set(
                    "a3_evidence",
                    json!({
                        "tools_list": tools_list,
                        "resume": resume,
                        "canonical_amend": canonical_amend,
                        "canonical_amend_on_disk": amendments_on_disk,
                        "retired_amend": retired_amend,
                        "after_retired_on_disk": after_retired_on_disk,
                    }),
                );
                Ok(out)
            },
        ),
        check_def(
            "tool guidance calls the definition a playbook",
            &[("a3_evidence", "JsonValue")],
            |ctx, _params| {
                let evidence = evidence(&ctx)?;

                let begin = tool(evidence, "begin")?;
                let target_owner = property_description(begin, "target_owner")?;
                if !target_owner.contains("generated playbook") {
                    return Err(format!(
                        "begin.target_owner still does not call the generated definition a playbook: {}",
                        target_owner
                    ));
                }

                let orchestrate = tool(evidence, "anvil_orchestrate")?;
                let track_name = property_description(orchestrate, "track_name")?;
                if !track_name.contains("playbooks whose machine requires name") {
                    return Err(format!(
                        "anvil_orchestrate.track_name still does not call the definitions playbooks: {}",
                        track_name
                    ));
                }

                // The advertised prose as a whole: no `workflow` anywhere.
                // There is no carve-out — the wire moved with the prose.
                let mut violations: Vec<String> = Vec::new();
                for (origin, text) in advertised_prose(evidence)? {
                    for offender in workflow_occurrences(&text) {
                        violations.push(format!("{}: …{}…", origin, offender));
                    }
                }
                if !violations.is_empty() {
                    return Err(format!(
                        "advertised MCP prose still calls a definition a workflow in {} place(s):\n{}",
                        violations.len(),
                        violations.join("\n")
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "calls the open execution a playbook run",
            &[("a3_evidence", "JsonValue")],
            |ctx, _params| {
                let evidence = evidence(&ctx)?;
                let guidance = resume_guidance(evidence)?;
                let expected_prefix = format!("Playbook run {} (track) is already in progress", RESUME_ARTIFACT);
                if !guidance.starts_with(&expected_prefix) {
                    return Err(format!(
                        "resume guidance does not call the open execution a playbook run.\nexpected prefix: {}\nactual: {}",
                        expected_prefix, guidance
                    ));
                }
                if guidance.to_lowercase().contains("workflow") {
                    return Err(format!(
                        "resume guidance still calls the open execution a workflow: {}",
                        guidance
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the guidance still identifies the correct continuation action",
            &[("a3_evidence", "JsonValue")],
            |ctx, _params| {
                let evidence = evidence(&ctx)?;
                let resume = resume_result(evidence)?;
                let advance = resume["resume_advance_action"].as_str().unwrap_or("");
                if advance.trim().is_empty() {
                    return Err(format!(
                        "resume response carried no advance action: {}",
                        resume
                    ));
                }
                let guidance = resume_guidance(evidence)?;
                if !guidance.contains(&format!("Continue via {}", advance)) {
                    return Err(format!(
                        "resume guidance does not name the continuation action `{}`: {}",
                        advance, guidance
                    ));
                }
                if !guidance.contains(RESUME_ARTIFACT) {
                    return Err(format!(
                        "resume guidance does not name the open run `{}`: {}",
                        RESUME_ARTIFACT, guidance
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "no advertised argument or wire value says workflow",
            &[("a3_evidence", "JsonValue")],
            |ctx, _params| {
                let evidence = evidence(&ctx)?;
                let mut violations: Vec<String> = Vec::new();

                // (a) advertised NAMES: tool names, argument names, nested
                // item-property names.
                for (owner, name) in advertised_argument_names(evidence)? {
                    if name.to_lowercase().contains("workflow") {
                        violations.push(format!("advertised name {}: `{}`", owner, name));
                    }
                }

                // (b) advertised PROSE, with no carve-out.
                for (origin, text) in advertised_prose(evidence)? {
                    for offender in workflow_occurrences(&text) {
                        violations.push(format!("advertised prose {}: …{}…", origin, offender));
                    }
                }

                // (c) response KEYS on a real resume payload.
                let resume = resume_result(evidence)?;
                let mut keys = Vec::new();
                collect_keys(&resume, "resume", &mut keys);
                for (path, key) in keys {
                    if key.to_lowercase().contains("workflow") {
                        violations.push(format!("response key {}", path));
                    }
                }

                // (d) each retired identifier, named, so a survivor reports as
                // itself rather than as an anonymous substring.
                let advertised = serde_json::to_string(&evidence["tools_list"])
                    .map_err(|e| e.to_string())?;
                for retired in RETIRED_WIRE_TOKENS {
                    if advertised.contains(retired) {
                        violations.push(format!(
                            "retired wire identifier `{}` is still advertised by tools/list",
                            retired
                        ));
                    }
                }

                if !violations.is_empty() {
                    return Err(format!(
                        "the MCP surface still says workflow in {} place(s):\n{}",
                        violations.len(),
                        violations.join("\n")
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the canonical amendment kind is accepted and the retired one is refused",
            &[("a3_evidence", "JsonValue")],
            |ctx, _params| {
                let evidence = evidence(&ctx)?;

                // (a) ADVERTISED — the amend tool offers `playbook` and does
                // not offer `workflow`.
                let amend = tool(evidence, "amend")?;
                let kind_desc = property_description(amend, "kind")?;
                if !kind_desc.contains(CANONICAL_AMENDMENT_KIND) {
                    return Err(format!(
                        "amend.kind does not advertise the canonical `{}` value: {}",
                        CANONICAL_AMENDMENT_KIND, kind_desc
                    ));
                }

                // The persist error code moved with it.
                let persist = tool(evidence, "persist_playbook")?;
                let hooks_desc = property_description(persist, "hooks")?;
                if !hooks_desc.contains("playbook_unknown_hook_reference") {
                    return Err(format!(
                        "persist_playbook.hooks does not advertise the canonical error code: {}",
                        hooks_desc
                    ));
                }

                // (b) ACCEPTED — a real amend call with `kind: playbook` was
                // accepted by the real server and recorded on disk.
                let response = &evidence["canonical_amend"];
                if response["result"]["isError"].as_bool() == Some(true)
                    || response.get("error").is_some()
                {
                    return Err(format!(
                        "a real amend call with the canonical `kind: {}` was rejected: {}",
                        CANONICAL_AMENDMENT_KIND, response
                    ));
                }
                let payload = tool_result_json(response)?;
                let op_id = payload["op_id"].as_str().unwrap_or("");
                if op_id.trim().is_empty() {
                    return Err(format!(
                        "a real amend call with the canonical `kind: {}` returned no op_id: {}",
                        CANONICAL_AMENDMENT_KIND, payload
                    ));
                }
                match evidence["canonical_amend_on_disk"].as_str() {
                    Some(on_disk) if on_disk.contains("CANON-1") => {}
                    Some(on_disk) => {
                        return Err(format!(
                            "the canonical-kind amendment was not recorded on disk: {}",
                            on_disk
                        ))
                    }
                    None => {
                        return Err(
                            "the canonical-kind amendment wrote no spec.amendments.yaml"
                                .to_string(),
                        )
                    }
                }

                // (c) REFUSED — the retired value is not a second spelling of
                // the same thing. It errors, and it writes nothing.
                let retired = &evidence["retired_amend"];
                let refused = retired["result"]["isError"].as_bool() == Some(true)
                    || retired.get("error").is_some();
                if !refused {
                    return Err(format!(
                        "a real amend call with the RETIRED `kind: {}` was accepted — the old \
                         name is still live beside the new one: {}",
                        RETIRED_AMENDMENT_KIND, retired
                    ));
                }
                if let Some(on_disk) = evidence["after_retired_on_disk"].as_str() {
                    if on_disk.contains("RETIRED-1") {
                        return Err(format!(
                            "the retired-kind amendment was recorded on disk anyway: {}",
                            on_disk
                        ));
                    }
                }
                Ok(())
            },
        ),
    ]
}

/// The JSON payload the shim packs into an MCP tool result's text content.
fn tool_result_json(response: &Value) -> Result<Value, String> {
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .ok_or_else(|| format!("tool response carried no text content: {}", response))?;
    serde_json::from_str(text).map_err(|e| format!("parse tool result text `{}`: {}", text, e))
}

fn resume_result(evidence: &Value) -> Result<Value, String> {
    let payload = tool_result_json(&evidence["resume"])?;
    if payload["outcome"].as_str() != Some("resume") {
        return Err(format!(
            "anvil_orchestrate did not resume the open run (outcome {:?}): {}",
            payload["outcome"], payload
        ));
    }
    Ok(payload)
}

fn resume_guidance(evidence: &Value) -> Result<String, String> {
    let resume = resume_result(evidence)?;
    Ok(resume["guidance"].as_str().unwrap_or("").to_string())
}
