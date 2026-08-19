//! Step module for the K8 MCP end-to-end feature
//! (`anvil-mcp/features/e2e/backlog_item_mcp.feature`).
//!
//! REAL SEAM (plan §3, Task 9). Every step drives a REAL `anvil-mcp` stdio
//! subprocess attached to its OWN real `anvil-engine` process over a real
//! isolated temporary hearth: real `initialize`, real `tools/list`, real
//! `tools/call`, and filesystem assertions against the bytes the engine wrote.
//! Nothing here is mocked and no step reaches around the shim.

use anvil_test_support::engine::{spawn_engine_for_hearth, EngineProcess};
use crate::mcp::OwnedMcpShim;
use anvil_test_support::{retained_temp_dir, RetainedTempDir};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const WORLD: &str = "bi_mcp_world";
const WORLD_TY: &str = "BacklogMcpFixture";

/// The organ the queue scenarios name verbatim.
const ORGAN: &str = "bn_0rgan00001";
/// A second organ, so the cross-organ view genuinely aggregates.
const ORGAN2: &str = "bn_0rgan00002";

#[derive(Clone)]
pub struct BacklogMcpFixture {
    pub hearth: PathBuf,
    _temp: RetainedTempDir,
    _work: RetainedTempDir,
    /// The engine the shim is attached to. Held so it outlives the shim.
    _engine: Arc<EngineProcess>,
    shim: Arc<Mutex<OwnedMcpShim>>,
    state: Arc<Mutex<FixtureState>>,
}

#[derive(Default)]
struct FixtureState {
    next_id: i64,
    item_id: Option<String>,
    last: Option<Attempt>,
}

#[derive(Clone, Debug)]
enum Attempt {
    ToolResult { state: Option<String>, text: String },
    JsonRpcError { message: String },
}

impl BacklogMcpFixture {
    fn new() -> Result<Self, String> {
        let (temp, hearth) = retained_temp_dir("anvil-backlog-mcp")?;
        std::fs::create_dir_all(hearth.join("tracks")).map_err(|e| format!("create tracks: {e}"))?;
        std::fs::write(hearth.join("tracks.md"), "# Tracks\n")
            .map_err(|e| format!("tracks.md: {e}"))?;

        let engine = spawn_engine_for_hearth(&hearth)?;
        let port = engine.port;

        // The shim resolves its hearth from `.hearth` in its cwd.
        let (work_handle, work_dir) = retained_temp_dir("anvil-backlog-mcp-work")?;
        std::fs::write(
            work_dir.join(".hearth"),
            format!("path: {}\n", hearth.display()),
        )
        .map_err(|e| format!("write .hearth: {e}"))?;

        let shim = OwnedMcpShim::start(&work_dir, port)?;

        let fixture = Self {
            hearth,
            _temp: temp,
            _work: work_handle,
            _engine: Arc::new(engine),
            shim: Arc::new(Mutex::new(shim)),
            state: Arc::new(Mutex::new(FixtureState::default())),
        };
        // `begin` refuses without a session, so the journey opens with a real
        // checkin through the same stdio seam.
        fixture.call_tool("checkin", json!({ "role": "creator", "actor_type": "agent", "actor_model": "claude-opus-5", "actor_provider": "anthropic", "actor_name": "Mcpdriver-000001" }))?;
        Ok(fixture)
    }

    fn with_state<T>(&self, f: impl FnOnce(&mut FixtureState) -> T) -> Result<T, String> {
        let mut guard = self.state.lock().map_err(|_| "state mutex".to_string())?;
        Ok(f(&mut guard))
    }

    fn next_id(&self) -> Result<i64, String> {
        self.with_state(|s| {
            s.next_id += 1;
            s.next_id
        })
    }

    /// Send one JSON-RPC request over the real stdio seam.
    fn send(&self, request: &Value) -> Result<Value, String> {
        let mut shim = self.shim.lock().map_err(|_| "shim mutex".to_string())?;
        shim.request(request)
    }

    /// One real `tools/call`. Returns the raw JSON-RPC response.
    fn call_tool(&self, name: &str, arguments: Value) -> Result<Value, String> {
        let id = self.next_id()?;
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        }))
    }

    fn record(&self, attempt: Attempt) -> Result<(), String> {
        self.with_state(|s| s.last = Some(attempt))
    }

    fn last(&self) -> Result<Attempt, String> {
        self.with_state(|s| s.last.clone())?
            .ok_or_else(|| "no backlog MCP tool call was attempted".to_string())
    }

    fn item_id(&self) -> Result<String, String> {
        self.with_state(|s| s.item_id.clone())?
            .ok_or_else(|| "this step needs an item created through the begin tool".to_string())
    }

    /// Classify one raw JSON-RPC response into the outcome the feature asserts
    /// against. Both loud forms — a JSON-RPC `error` object and a tool result
    /// flagged `isError` — are errors; nothing degrades into a fake success.
    fn classify(&self, response: &Value) -> Attempt {
        if let Some(error) = response.get("error") {
            return Attempt::JsonRpcError {
                message: error.to_string(),
            };
        }
        if response["result"]["isError"].as_bool().unwrap_or(false) {
            return Attempt::JsonRpcError {
                message: response["result"]["content"][0]["text"]
                    .as_str()
                    .unwrap_or("(missing error text)")
                    .to_string(),
            };
        }
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| response["result"].to_string());
        let state = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v["state"].as_str().map(|s| s.to_string()));
        Attempt::ToolResult { state, text }
    }

    fn record_response(&self, response: &Value) -> Result<(), String> {
        let attempt = self.classify(response);
        self.record(attempt)
    }

    /// The identity every mutation tool repeats.
    fn identity(&self, role: &str) -> Value {
        json!({
            "actor_name": "Mcpdriver-000001",
            "actor_role": role,
            "actor_type": "agent",
            "actor_model": "claude-opus-5",
            "actor_provider": "anthropic",
            "actor_context_window": 200000,
            "actor_sdk_version": "1.0.0",
            "actor_entrypoint": "brine"
        })
    }
}

fn merge(base: Value, extra: Value) -> Value {
    let mut object = base.as_object().cloned().unwrap_or_default();
    if let Some(more) = extra.as_object() {
        for (k, v) in more {
            object.insert(k.clone(), v.clone());
        }
    }
    Value::Object(object)
}

/// The closed K8 genesis object the shim must accept as an OBJECT.
fn genesis_item(organ: &str, title: &str) -> Value {
    json!({
        "business_node_id": organ,
        "title": title,
        "action_class": "dev",
        "intake": {
            "edge": "spark_triage",
            "evidence_refs": [{ "kind": "spark", "id": "sp_seed01" }]
        },
        "origin_binding": {
            "value_gap_served": { "kind": "temper_measure", "id": "tm_valuegap01" },
            "minting_council_id": null,
            "experiment_id": null,
            "predicted_value": null
        }
    })
}

/// The `fields` object each named `begin` input sends. Every variant is built
/// from the SAME valid genesis object so the only difference is the thing under
/// test.
fn begin_fields_for(input: &str) -> Result<Value, String> {
    let item = genesis_item(ORGAN, "mcp journey candidate");
    let with_origin = |mutate: fn(&mut serde_json::Map<String, Value>)| {
        let mut item = item.clone();
        if let Some(origin) = item["origin_binding"].as_object_mut() {
            mutate(origin);
        }
        json!({ "item": item })
    };
    Ok(match input {
        "valid_candidate" | "no_predictor" => json!({ "item": item }),
        // The shim must refuse a caller that pre-serializes the object instead
        // of passing it structurally.
        "prestringified_item" => json!({ "item": item.to_string() }),
        "council_with_prediction" => with_origin(|origin| {
            origin.insert("minting_council_id".into(), json!("co_seed01"));
            origin.insert("predicted_value".into(), json!(2.5));
        }),
        "experiment_with_prediction" => with_origin(|origin| {
            origin.insert("experiment_id".into(), json!("ex_seed01"));
            origin.insert("predicted_value".into(), json!(2.5));
        }),
        "both_predictor_ids" => with_origin(|origin| {
            origin.insert("minting_council_id".into(), json!("co_seed01"));
            origin.insert("experiment_id".into(), json!("ex_seed01"));
            origin.insert("predicted_value".into(), json!(2.5));
        }),
        "predictor_without_value" => with_origin(|origin| {
            origin.insert("minting_council_id".into(), json!("co_seed01"));
        }),
        "value_without_predictor" => with_origin(|origin| {
            origin.insert("predicted_value".into(), json!(2.5));
        }),
        "forged_engine_owned_key" => {
            let mut item = item.clone();
            item["backlog_item_id"] = json!("bi_forged01");
            json!({ "item": item })
        }
        "sibling_field" => json!({ "item": item, "track_name": "not a K8 field" }),
        other => return Err(format!("unknown begin input '{other}'")),
    })
}

/// The full SHAPE change set the journey applies before its first re-rank.
fn full_changes() -> Value {
    json!({
        "effort_class": "s",
        "playbook_binding": { "playbook_definition_id": "pd_seed01", "route_to_intake": false },
        "rank_inputs": {
            "value_gap_magnitude": {
                "ref": { "kind": "temper_measure", "id": "tm_valuegap01" },
                "magnitude": 4.0
            },
            "nick_weight": 1.0,
            "dependency_readiness": { "status": "ready", "blocker_refs": [] }
        }
    })
}

/// Read the item's state back out of the bytes the ENGINE wrote — the snapshot
/// tool's response carries no state field, so the assertion is a filesystem
/// assertion rather than an echo of the caller's own request.
fn stored_state(hearth: &Path, bi_id: &str) -> Result<String, String> {
    use anvil_core::ports::backlog_item_port::BacklogItemPort;
    anvil_core_hearth::fs_backlog_item_adapter::FileSystemBacklogItemAdapter::new(
        hearth.to_path_buf(),
    )
    .load_item(bi_id)
    .map(|l| l.item.state.as_str().to_string())
    .map_err(|e| format!("load {bi_id}: {e}"))
}

/// Arguments for one of the nine mutation tools, in the journey's context.
fn mutation_arguments(w: &BacklogMcpFixture, tool: &str, role: &str) -> Result<Value, String> {
    let identity = w.identity(role);
    // A domain-rejection scenario names no created item; the request still
    // carries a syntactically valid id so the rejection is the ROLE.
    let bi_id = w
        .with_state(|s| s.item_id.clone())?
        .unwrap_or_else(|| "bi_absent0001".to_string());
    Ok(match tool {
        "backlog_shape_edit" => merge(
            identity,
            json!({ "backlog_item_id": bi_id, "changes": full_changes() }),
        ),
        "backlog_recompute_rank" => merge(identity, json!({ "business_node_id": ORGAN })),
        "backlog_stamp_execution_binding" => merge(
            identity,
            json!({
                "backlog_item_id": bi_id,
                "execution_binding": {
                    "track_id": "tr_seed01",
                    "playbook_definition_id": "pd_seed01",
                    "playbook_run_id": "wf::seed/any-shape 42",
                    "run_id": "lr_abcdefghjkmnpqrstvwxyz0123"
                },
                "outcome_binding": {
                    "success_measure_id": "sm_seed01",
                    "tree_node": "tree/seed",
                    "reading_status": "registered"
                }
            }),
        ),
        "backlog_record_outcome_signoff" => merge(
            identity,
            json!({ "backlog_item_id": bi_id, "approver": "Nick" }),
        ),
        "backlog_propose_reshuffle" => merge(
            identity,
            json!({ "proposed": [{ "backlog_item_id": bi_id, "position": 1 }] }),
        ),
        "backlog_veto_age_out" | "backlog_lift_age_out_veto" => merge(
            identity,
            json!({ "backlog_item_id": bi_id, "approver": "Nick" }),
        ),
        other => return Err(format!("unknown backlog MCP tool '{other}'")),
    })
}

/// Seed the organ context the queue scenarios read: one ranked survivor plus
/// one explicit pre-triage candidate with a null playbook binding, and a second
/// organ so the cross-organ view aggregates more than one. Every write goes
/// through the real shim.
fn seed_queue_context(w: &BacklogMcpFixture) -> Result<(), String> {
    // A pre-triage candidate: routed to intake, never ranked.
    for (organ, title) in [(ORGAN, "pre-triage candidate"), (ORGAN2, "second organ item")] {
        let response = w.call_tool(
            "begin",
            json!({
                "artifact_type": "backlog_item",
                "actor_name": "Mcpdriver-000001",
                "actor_type": "agent",
                "actor_model": "claude-opus-5",
                "actor_provider": "anthropic",
                "fields": { "item": genesis_item(organ, title) }
            }),
        )?;
        if response.get("error").is_some() {
            return Err(format!("seeding {organ} failed: {response}"));
        }
        let bi_id = begin_item_id(&response)?;
        let shaped = w.call_tool(
            "backlog_shape_edit",
            merge(
                w.identity("organ_loop"),
                json!({
                    "backlog_item_id": bi_id,
                    "changes": { "playbook_binding": { "playbook_definition_id": null, "route_to_intake": true } }
                }),
            ),
        )?;
        if shaped.get("error").is_some() {
            return Err(format!("shaping {bi_id} failed: {shaped}"));
        }
    }
    Ok(())
}

fn begin_item_id(response: &Value) -> Result<String, String> {
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .ok_or_else(|| format!("begin returned no tool text: {response}"))?;
    let parsed: Value = serde_json::from_str(text)
        .map_err(|e| format!("begin tool text is not JSON: {e} (raw {text})"))?;
    parsed["track_path"]
        .as_str()
        .and_then(|p| p.rsplit('/').next())
        .filter(|s| s.starts_with("bi_"))
        .map(|s| s.to_string())
        .ok_or_else(|| format!("begin returned an unexpected track_path: {parsed}"))
}

fn world(ctx: &Context) -> Result<BacklogMcpFixture, String> {
    ctx.get::<BacklogMcpFixture>(WORLD)
        .cloned()
        .ok_or_else(|| "backlog MCP fixture missing; start with 'a backlog MCP session'".to_string())
}

fn carry(w: BacklogMcpFixture) -> Context {
    Context::new().with(WORLD, w)
}

fn p(params: &brine_core::step_types::Params, i: usize) -> Result<String, String> {
    params
        .get_string(i)
        .map(|s| s.to_string())
        .ok_or_else(|| format!("missing string parameter {i}"))
}

fn expect_result(w: &BacklogMcpFixture) -> Result<(Option<String>, String), String> {
    match w.last()? {
        Attempt::ToolResult { state, text } => Ok((state, text)),
        Attempt::JsonRpcError { message } => {
            Err(format!("expected a tool result but got JSON-RPC error: {message}"))
        }
    }
}

fn expect_error(w: &BacklogMcpFixture) -> Result<String, String> {
    match w.last()? {
        Attempt::JsonRpcError { message } => Ok(message),
        Attempt::ToolResult { .. } => {
            Err("expected a JSON-RPC error but the tool succeeded".into())
        }
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a backlog MCP session",
            &[],
            &[(WORLD, WORLD_TY)],
            |_ctx, _params| Ok(carry(BacklogMcpFixture::new()?)),
        ),
        step_def(
            "the tool list is requested",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, _params| {
                let w = world(&ctx)?;
                let id = w.next_id()?;
                let response = w.send(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "tools/list",
                    "params": {}
                }))?;
                if let Some(error) = response.get("error") {
                    w.record(Attempt::JsonRpcError {
                        message: error.to_string(),
                    })?;
                } else {
                    // The advertised list itself is the assertion surface.
                    w.record(Attempt::ToolResult {
                        state: None,
                        text: response["result"]["tools"].to_string(),
                    })?;
                }
                Ok(carry(w))
            },
        ),
        step_def(
            "a begin tool call for a backlog item with input {string}",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let w = world(&ctx)?;
                let input = p(params, 0)?;
                let fields = begin_fields_for(&input)?;
                let response = w.call_tool(
                    "begin",
                    json!({
                        "artifact_type": "backlog_item",
                        "actor_name": "Mcpdriver-000001",
                        "actor_type": "agent",
                        "actor_model": "claude-opus-5",
                        "actor_provider": "anthropic",
                        "actor_context_window": 200000,
                        "fields": fields
                    }),
                )?;
                if response.get("error").is_none()
                    && !response["result"]["isError"].as_bool().unwrap_or(false)
                {
                    let bi_id = begin_item_id(&response)?;
                    w.with_state(|s| s.item_id = Some(bi_id))?;
                }
                w.record_response(&response)?;
                Ok(carry(w))
            },
        ),
        step_def(
            "a describe tool call for {string}",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let w = world(&ctx)?;
                let identifier = p(params, 0)?;
                let response = w.call_tool("describe", json!({ "identifier": identifier }))?;
                w.record_response(&response)?;
                Ok(carry(w))
            },
        ),
        step_def(
            "a {string} tool call with role {string}",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let w = world(&ctx)?;
                let tool = p(params, 0)?;
                let role = p(params, 1)?;
                let arguments = mutation_arguments(&w, &tool, &role)?;
                let response = w.call_tool(&tool, arguments)?;
                w.record_response(&response)?;
                Ok(carry(w))
            },
        ),
        step_def(
            "a Snapshot tool call from {string} to {string} with role {string}",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let w = world(&ctx)?;
                let from = p(params, 0)?;
                let to = p(params, 1)?;
                let role = p(params, 2)?;
                let bi_id = w.item_id()?;
                let actual = stored_state(&w.hearth, &bi_id)?;
                if actual != from {
                    return Err(format!(
                        "the scenario declares source state '{from}' but {bi_id} reads '{actual}'"
                    ));
                }
                let response = w.call_tool(
                    "snapshot",
                    json!({
                        "artifact_path": format!("backlog_items/{bi_id}"),
                        "to_state": to,
                        "actor_role": role,
                        // The #5 ATTEND gate demands an engine-verified approver.
                        "approver": "Nick",
                        "actor_name": "Mcpdriver-000001",
                        "actor_type": "agent",
                        "actor_model": "claude-opus-5",
                        "actor_provider": "anthropic",
                        "actor_context_window": 200000
                    }),
                )?;
                match w.classify(&response) {
                    Attempt::JsonRpcError { message } => {
                        w.record(Attempt::JsonRpcError { message })?
                    }
                    Attempt::ToolResult { text, .. } => {
                        // The state comes from the engine-written bytes, not
                        // from the request the fixture just sent.
                        let state = stored_state(&w.hearth, &bi_id)?;
                        w.record(Attempt::ToolResult {
                            state: Some(state),
                            text,
                        })?
                    }
                }
                Ok(carry(w))
            },
        ),
        step_def(
            "a backlog_organ_queue tool call for {string}",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let w = world(&ctx)?;
                let organ = p(params, 0)?;
                seed_queue_context(&w)?;
                let response =
                    w.call_tool("backlog_organ_queue", json!({ "business_node_id": organ }))?;
                w.record_response(&response)?;
                Ok(carry(w))
            },
        ),
        step_def(
            "a backlog_cross_organ_view tool call",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, _params| {
                let w = world(&ctx)?;
                seed_queue_context(&w)?;
                let response = w.call_tool("backlog_cross_organ_view", json!({}))?;
                w.record_response(&response)?;
                Ok(carry(w))
            },
        ),
        // ── Then ─────────────────────────────────────────────────────────────
        check_def(
            "the tool call returns a successful result",
            &[(WORLD, WORLD_TY)],
            |ctx, _params| {
                expect_result(&world(&ctx)?)?;
                Ok(())
            },
        ),
        check_def(
            "the tool result state is {string}",
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let (state, _) = expect_result(&world(&ctx)?)?;
                let want = p(params, 0)?;
                match state {
                    Some(s) if s == want => Ok(()),
                    other => Err(format!("expected state {want:?} but was {other:?}")),
                }
            },
        ),
        check_def(
            "the tool result contains {string}",
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let (_, text) = expect_result(&world(&ctx)?)?;
                let want = p(params, 0)?;
                if text.contains(&want) {
                    Ok(())
                } else {
                    Err(format!("expected tool result containing {want:?} but was {text:?}"))
                }
            },
        ),
        check_def(
            "the tool list advertises {string}",
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let (_, text) = expect_result(&world(&ctx)?)?;
                let want = p(params, 0)?;
                // The name must appear as an advertised tool NAME, not merely
                // somewhere in a description.
                let needle = format!("\"name\":\"{want}\"");
                if text.contains(&needle) {
                    Ok(())
                } else {
                    Err(format!("expected tools/list to advertise {want:?}"))
                }
            },
        ),
        check_def(
            "the tool call returns a JSON-RPC error",
            &[(WORLD, WORLD_TY)],
            |ctx, _params| {
                expect_error(&world(&ctx)?)?;
                Ok(())
            },
        ),
        check_def(
            "the tool call error contains {string}",
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let msg = expect_error(&world(&ctx)?)?;
                let want = p(params, 0)?;
                if msg.contains(&want) {
                    Ok(())
                } else {
                    Err(format!("expected error containing {want:?} but was {msg:?}"))
                }
            },
        ),
    ]
}
