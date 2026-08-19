//! Step module for `actor_activity_rpc.feature` (engine seam).
//!
//! Seeds a real engine hearth's artifact directories with status.yaml files
//! carrying `activity:` begin-markers (the RAW actor names), reuses the shared
//! `engine` module's "the engine is started with that hearth" / "both permitted
//! hearths" steps, then drives BOTH the gRPC `ActorActivity` RPC and the `/ws`
//! JSON-RPC method, asserting identical folded data. The /ws assertions reuse
//! the WS roundtrip helper pattern from the usage-timeseries module.
//!
//! LOCAL-ONLY: this exercises the local-dashboard query that returns raw actor
//! names. The data never leaves the machine — it is served over loopback /ws and
//! the on-machine gRPC surface only.

use anvil_test_support::engine::EngineProcess;
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{async_step_def, check_def, step_def, StepDef};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

use anvil_test_support::{retained_temp_dir, RetainedTempDir};

const RESULT_KEY: &str = "aa_rpc_result";
const WS_RESPONSE_KEY: &str = "aa_rpc_ws_response";

fn column_index(table: &DataTable, name: &str) -> Result<usize, String> {
    table
        .headers
        .iter()
        .position(|h| h == name)
        .ok_or_else(|| format!("Missing '{}' column in data table", name))
}

/// Build an engine hearth and seed its artifact directories with status.yaml
/// files carrying `activity:` begin-markers. Rows sharing an artifact_id are
/// appended to that artifact's activity log. Columns:
/// artifact_id, artifact_kind, actor, state, at.
fn build_actor_hearth(table: &DataTable) -> Result<(RetainedTempDir, std::path::PathBuf), String> {
    let id_col = column_index(table, "artifact_id")?;
    let kind_col = column_index(table, "artifact_kind")?;
    let actor_col = column_index(table, "actor")?;
    let state_col = column_index(table, "state")?;
    let at_col = column_index(table, "at")?;

    let (handle, tmp) = retained_temp_dir("anvil-aa-rpc-")?;
    // Engine hearth predicate needs tracks/ + tracks.md.
    std::fs::create_dir_all(tmp.join("tracks")).map_err(|e| format!("create tracks: {}", e))?;
    std::fs::write(tmp.join("tracks.md"), "# Tracks\n")
        .map_err(|e| format!("write tracks.md: {}", e))?;

    // artifact_id -> (kind, dir, accumulated activity YAML rows)
    use std::collections::BTreeMap;
    let mut artifacts: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    for row in &table.rows {
        let id = row[id_col].trim().to_string();
        let kind = row[kind_col].trim().to_string();
        let actor = row[actor_col].trim().to_string();
        let state = row[state_col].trim().to_string();
        let at = row[at_col].trim().to_string();
        let marker = format!(
            "  - kind: begin\n    actor: {}\n    state: {}\n    at: {}\n",
            actor, state, at
        );
        artifacts
            .entry(id)
            .or_insert_with(|| (kind, Vec::new()))
            .1
            .push(marker);
    }

    for (id, (kind, markers)) in artifacts {
        // The directory layout is <kind>s/<id>/status.yaml (e.g. tracks/track-alpha).
        let dir = tmp.join(format!("{}s", kind)).join(&id);
        std::fs::create_dir_all(&dir).map_err(|e| format!("create artifact dir: {}", e))?;
        let status = format!(
            "version: 1\nkind: {}\nstate: active\nactors: {{}}\ntransitions: []\nactivity:\n{}",
            kind,
            markers.join("")
        );
        std::fs::write(dir.join("status.yaml"), status)
            .map_err(|e| format!("write status.yaml: {}", e))?;
    }
    Ok((handle, tmp))
}

async fn ws_roundtrip(port: u16, request: &Value) -> Result<Value, String> {
    let url = format!("ws://127.0.0.1:{}/ws", port);
    let (mut socket, _resp) = tokio_tungstenite::connect_async(&url)
        .await
        .map_err(|e| format!("ws connect to {} failed: {}", url, e))?;
    socket
        .send(Message::Text(request.to_string()))
        .await
        .map_err(|e| format!("ws send failed: {}", e))?;
    while let Some(frame) = socket.next().await {
        match frame.map_err(|e| format!("ws recv failed: {}", e))? {
            Message::Text(text) => {
                return serde_json::from_str(&text)
                    .map_err(|e| format!("ws reply not JSON: {} (raw: {})", e, text));
            }
            Message::Close(_) => return Err("ws closed before a reply frame".to_string()),
            _ => continue,
        }
    }
    Err("ws stream ended before a reply frame".to_string())
}

fn aa_to_json(resp: &anvil_engine::proto::ActorActivityResponse) -> Value {
    let actors: Vec<Value> = resp
        .actors
        .iter()
        .map(|a| {
            json!({
                "actor": a.actor,
                "begin_count": a.begin_count,
                "last_active": a.last_active,
                "artifact_kinds": a.artifact_kinds,
            })
        })
        .collect();
    json!({
        "resolved_hearth": resp.resolved_hearth,
        "hearths_included": resp.hearths_included,
        "actors": actors,
    })
}

fn ws_result(ctx: &Context) -> Result<Value, String> {
    let response = ctx
        .get::<Value>(WS_RESPONSE_KEY)
        .ok_or("No aa_rpc_ws_response in context")?;
    if let Some(error) = response.get("error") {
        return Err(format!("Expected JSON-RPC result, got error: {}", error));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| format!("JSON-RPC response has no result: {}", response))
}

fn find_actor<'a>(result: &'a Value, actor: &str) -> Result<&'a Value, String> {
    result
        .get("actors")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("result.actors is not an array: {}", result))?
        .iter()
        .find(|a| a.get("actor").and_then(Value::as_str) == Some(actor))
        .ok_or_else(|| format!("No actor entry for '{}'", actor))
}

fn assert_begin_count(result: &Value, actor: &str, expected: u64) -> Result<(), String> {
    let entry = find_actor(result, actor)?;
    let actual = entry
        .get("begin_count")
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("actor '{}' begin_count not a u64", actor))?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "actor '{}': expected begin count {}, got {}",
            actor, expected, actual
        ))
    }
}

fn assert_last_active(result: &Value, actor: &str, expected: &str) -> Result<(), String> {
    let entry = find_actor(result, actor)?;
    let actual = entry
        .get("last_active")
        .and_then(Value::as_str)
        .unwrap_or("");
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "actor '{}': expected last active '{}', got '{}'",
            actor, expected, actual
        ))
    }
}

fn assert_artifact_kinds(result: &Value, actor: &str, expected: &str) -> Result<(), String> {
    let want: Vec<String> = expected
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let entry = find_actor(result, actor)?;
    let got: Vec<String> = entry
        .get("artifact_kinds")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("actor '{}' artifact_kinds not an array", actor))?
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    if got == want {
        Ok(())
    } else {
        Err(format!(
            "actor '{}': expected playbook kinds {:?}, got {:?}",
            actor, want, got
        ))
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "an actor activity engine hearth with artifacts:",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let (handle, tmp) = build_actor_hearth(table)?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set::<RetainedTempDir>("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "an actor activity primary hearth with artifacts:",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let (handle, tmp) = build_actor_hearth(table)?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set::<RetainedTempDir>("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        // Seed the SECONDARY hearth into uts_rpc_hearth_b so the shared engine
        // "both permitted hearths" step folds BOTH.
        step_def(
            "an actor activity secondary hearth with artifacts:",
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("uts_rpc_hearth_b", "PathBuf"),
                ("uts_rpc_hearth_b_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let (handle, tmp) = build_actor_hearth(table)?;
                let mut out = Context::new();
                if let Some(p) = ctx.get::<std::path::PathBuf>("hearth_path") {
                    out.set("hearth_path", p.clone());
                }
                if let Some(h) = ctx.get::<RetainedTempDir>("hearth_path_handle") {
                    out.set::<RetainedTempDir>("hearth_path_handle", std::sync::Arc::clone(h));
                }
                out.set("uts_rpc_hearth_b", tmp);
                out.set::<RetainedTempDir>("uts_rpc_hearth_b_handle", handle);
                Ok(out)
            },
        ),
        // ===== gRPC drivers =====
        async_step_def(
            "the ActorActivity RPC is called",
            &[("engine_process", "EngineProcess")],
            &[(RESULT_KEY, "Value"), ("engine_process", "EngineProcess")],
            |mut ctx, _params| async move {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let mut client =
                    anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr)
                        .await
                        .map_err(|e| format!("connect failed: {}", e))?;
                let request = anvil_test_support::surfaced(anvil_engine::proto::ActorActivityRequest {
                    hearth_path: String::new(),
                    all_hearths: false,
                });
                let resp = client
                    .actor_activity(request)
                    .await
                    .map_err(|s| {
                        format!("ActorActivity RPC error {:?}: {}", s.code(), s.message())
                    })?
                    .into_inner();
                let mut out = Context::new();
                out.set::<Value>(RESULT_KEY, aa_to_json(&resp));
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        async_step_def(
            "the ActorActivity RPC is called across all hearths",
            &[("engine_process", "EngineProcess")],
            &[(RESULT_KEY, "Value"), ("engine_process", "EngineProcess")],
            |mut ctx, _params| async move {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let mut client =
                    anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr)
                        .await
                        .map_err(|e| format!("connect failed: {}", e))?;
                let request = anvil_test_support::surfaced(anvil_engine::proto::ActorActivityRequest {
                    hearth_path: String::new(),
                    all_hearths: true,
                });
                let resp = client
                    .actor_activity(request)
                    .await
                    .map_err(|s| {
                        format!("ActorActivity RPC error {:?}: {}", s.code(), s.message())
                    })?
                    .into_inner();
                let mut out = Context::new();
                out.set::<Value>(RESULT_KEY, aa_to_json(&resp));
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        // ===== /ws driver =====
        async_step_def(
            "an actor_activity JSON-RPC request is sent over /ws with hearth_path {string}",
            &[("engine_process", "EngineProcess")],
            &[
                (WS_RESPONSE_KEY, "Value"),
                ("engine_process", "EngineProcess"),
            ],
            |mut ctx, params| async move {
                let hearth_path = params.get_string(0).unwrap_or_default().to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let request = json!({
                    "jsonrpc": "2.0",
                    "id": 21,
                    "method": "actor_activity",
                    "params": { "surface": "test-harness", "hearth_path": hearth_path }
                });
                let response = ws_roundtrip(engine.port, &request).await?;
                let mut out = Context::new();
                out.set::<Value>(WS_RESPONSE_KEY, response);
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        // ===== gRPC assertions =====
        check_def(
            "the actor activity RPC entry for actor {string} has begin count {int}",
            &[(RESULT_KEY, "Value")],
            |ctx, params| {
                let result = ctx.get::<Value>(RESULT_KEY).ok_or("No aa result")?;
                assert_begin_count(
                    result,
                    params.get_string(0).ok_or("actor")?,
                    params.get_int(1).ok_or("count")? as u64,
                )
            },
        ),
        check_def(
            "the actor activity RPC entry for actor {string} has last active {string}",
            &[(RESULT_KEY, "Value")],
            |ctx, params| {
                let result = ctx.get::<Value>(RESULT_KEY).ok_or("No aa result")?;
                assert_last_active(
                    result,
                    params.get_string(0).ok_or("actor")?,
                    params.get_string(1).ok_or("last active")?,
                )
            },
        ),
        check_def(
            "the actor activity RPC entry for actor {string} has playbook kinds {string}",
            &[(RESULT_KEY, "Value")],
            |ctx, params| {
                let result = ctx.get::<Value>(RESULT_KEY).ok_or("No aa result")?;
                assert_artifact_kinds(
                    result,
                    params.get_string(0).ok_or("actor")?,
                    params.get_string(1).ok_or("kinds")?,
                )
            },
        ),
        check_def(
            "the actor activity RPC resolved hearth is the all-hearths sentinel",
            &[(RESULT_KEY, "Value")],
            |ctx, _params| {
                let result = ctx.get::<Value>(RESULT_KEY).ok_or("No aa result")?;
                let rh = result
                    .get("resolved_hearth")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if rh == "(all hearths)" {
                    Ok(())
                } else {
                    Err(format!("expected all-hearths sentinel, got '{}'", rh))
                }
            },
        ),
        check_def(
            "the actor activity RPC included {int} hearths",
            &[(RESULT_KEY, "Value")],
            |ctx, params| {
                let result = ctx.get::<Value>(RESULT_KEY).ok_or("No aa result")?;
                let expected = params.get_int(0).ok_or("count")? as usize;
                let actual = result
                    .get("hearths_included")
                    .and_then(Value::as_array)
                    .ok_or("hearths_included not an array")?
                    .len();
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("expected {} hearths, got {}", expected, actual))
                }
            },
        ),
        // ===== /ws assertions =====
        check_def(
            "the /ws actor activity entry for actor {string} has begin count {int}",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let result = ws_result(&ctx)?;
                assert_begin_count(
                    &result,
                    params.get_string(0).ok_or("actor")?,
                    params.get_int(1).ok_or("count")? as u64,
                )
            },
        ),
        check_def(
            "the /ws actor activity begin_count for actor {string} is a JSON number",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let result = ws_result(&ctx)?;
                let actor = params.get_string(0).ok_or("actor")?;
                let entry = find_actor(&result, actor)?;
                if entry.get("begin_count").map(Value::is_number) == Some(true) {
                    Ok(())
                } else {
                    Err(format!(
                        "actor '{}' begin_count is not a JSON number: {:?}",
                        actor,
                        entry.get("begin_count")
                    ))
                }
            },
        ),
    ]
}
