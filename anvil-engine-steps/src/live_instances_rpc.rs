//! Step module for `live_instances_rpc.feature` (engine seam).
//!
//! Seeds a HERMETIC fixture hearth carrying two track instances that exercise the
//! honest live-actor contract:
//!   - `20260707T0000_open_track` — an OPEN (folds to `spec`, non-terminal) track
//!     with an `activity:` begin-marker naming the RAW actor `Atlas-Doer-4211`;
//!   - `20260707T0100_done_track` — a `completed` (terminal) track that MUST be
//!     dropped from the live view even though it carries a begin-marker.
//!
//! Reuses the shared `engine` module's "the engine is started with that hearth"
//! step. Adds the seeding step, the gRPC LiveInstances call + assertions, and the
//! /ws `live_instances` request + assertions (same connect-send-recv shape as
//! `playbook_atlas_rpc`).

use anvil_test_support::engine::EngineProcess;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{async_step_def, check_def, StepDef};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

use anvil_test_support::{retained_temp_dir, RetainedTempDir};

const LI_GRPC_KEY: &str = "li_grpc_result";
const LI_WS_KEY: &str = "li_ws_response";

enum GrpcLive {
    Success(anvil_engine::proto::LiveInstancesResponse),
    Error { code: String, message: String },
}

// ---- fixture status.yaml builders ---------------------------------------

/// An OPEN track: two legacy transitions fold the current state to `spec`
/// (non-terminal), with a begin-marker naming the RAW actor `Atlas-Doer-4211`.
fn open_track_status_yaml() -> &'static str {
    r#"version: 1
kind: track
state: spec
actors: {}
transitions:
  - to: active
    at: "2026-07-07T00:00:00Z"
    actor: Atlas-Doer-4211
    role: creator
  - to: spec
    at: "2026-07-07T00:02:00Z"
    actor: Atlas-Doer-4211
    role: doer
activity:
  - kind: begin
    actor: Atlas-Doer-4211
    state: spec
    at: "2026-07-07T00:02:00Z"
"#
}

/// A COMPLETED track: folds to the terminal `completed` state, so it must be
/// dropped from the live view despite carrying a begin-marker.
fn done_track_status_yaml() -> &'static str {
    r#"version: 1
kind: track
state: completed
actors: {}
transitions:
  - to: active
    at: "2026-07-07T01:00:00Z"
    actor: Bygone-9000
    role: creator
  - to: completed
    at: "2026-07-07T01:30:00Z"
    actor: Bygone-9000
    role: doer
activity:
  - kind: begin
    actor: Bygone-9000
    state: active
    at: "2026-07-07T01:00:00Z"
"#
}

/// A DORMANT open track: non-terminal (`active`) but carries NO `activity:`
/// begin-marker at all — the honest-Live-panel case (Fix B) this feature
/// exists to cover. With no §0 events written either, this instance must be
/// EXCLUDED from the live list and counted in `idle_count`, never rendered as
/// a fake "live agent".
fn dormant_track_status_yaml() -> &'static str {
    r#"version: 1
kind: track
state: active
actors: {}
transitions:
  - to: active
    at: "2026-07-07T03:00:00Z"
    actor: Shelved-Author-1
    role: creator
"#
}

fn write_track(tracks: &std::path::Path, id: &str, status_yaml: &str) -> Result<(), String> {
    let dir = tracks.join(id);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {}", id, e))?;
    std::fs::write(dir.join("status.yaml"), status_yaml)
        .map_err(|e| format!("write {}/status.yaml: {}", id, e))?;
    Ok(())
}

/// Append `count` minimal §0 event lines to
/// `<hearth>/__temper_home__/.temper/step-measurements/<kind>/events.jsonl`
/// for `instance` — the SAME path `step0_activity_index::read_step0_activity_index`
/// reads ("the engine is started with that hearth" sets `ANVIL_TEMPER_HOME` to
/// `<hearth>/__temper_home__`). Only `workflow_id`/`at` are load-bearing for the
/// reader; the rest of the §0 schema is omitted since this fixture exists to
/// exercise the action_count/current_step/artifact_dir WIRING, not the full §0
/// contract (covered by `step_measurement_stream.feature`).
fn write_step0_events(
    hearth: &std::path::Path,
    kind: &str,
    instance: &str,
    count: u32,
) -> Result<(), String> {
    let dir = hearth
        .join("__temper_home__")
        .join(".temper")
        .join("step-measurements")
        .join(kind);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir step0 dir: {}", e))?;
    let mut lines = String::new();
    for i in 0..count {
        lines.push_str(&format!(
            "{{\"workflow_id\":\"{}\",\"track_id\":\"{}\",\"at\":\"2026-07-07T00:0{}:00Z\"}}\n",
            instance, kind, i
        ));
    }
    std::fs::write(dir.join("events.jsonl"), lines).map_err(|e| format!("write events.jsonl: {}", e))?;
    Ok(())
}

// ---- accessors ----------------------------------------------------------

fn grpc_live(ctx: &Context) -> Result<&anvil_engine::proto::LiveInstancesResponse, String> {
    match ctx
        .get::<GrpcLive>(LI_GRPC_KEY)
        .ok_or("No live gRPC result")?
    {
        GrpcLive::Success(resp) => Ok(resp),
        GrpcLive::Error { code, message } => Err(format!(
            "Expected live-instances success, got gRPC {}: {}",
            code, message
        )),
    }
}

fn live_instance<'a>(
    resp: &'a anvil_engine::proto::LiveInstancesResponse,
    key: &str,
) -> Option<&'a anvil_engine::proto::LiveInstance> {
    resp.instances.iter().find(|i| i.instance_id == key)
}

fn ws_result(ctx: &Context, key: &str) -> Result<Value, String> {
    let response = ctx.get::<Value>(key).ok_or("No /ws response in context")?;
    if let Some(error) = response.get("error") {
        return Err(format!("Expected JSON-RPC result, got error: {}", error));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| format!("JSON-RPC response has no result: {}", response))
}

fn ws_instance<'a>(result: &'a Value, key: &str) -> Option<&'a Value> {
    result
        .get("instances")
        .and_then(Value::as_array)?
        .iter()
        .find(|i| i.get("instance_id").and_then(Value::as_str) == Some(key))
}

/// True when `name` looks like a salted SHA-256 hex digest (64 hex chars) rather
/// than a raw actor name.
fn looks_like_hash(name: &str) -> bool {
    name.len() == 64 && name.chars().all(|c| c.is_ascii_hexdigit())
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
            Message::Binary(bytes) => {
                return serde_json::from_slice(&bytes)
                    .map_err(|e| format!("ws binary reply not JSON: {}", e));
            }
            Message::Close(_) => return Err("ws closed before a reply frame".to_string()),
            _ => continue,
        }
    }
    Err("ws stream ended before a reply frame".to_string())
}

pub fn steps() -> Vec<StepDef> {
    vec![
        async_step_def(
            "a live instances engine hearth",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, _params| async move {
                let (handle, tmp) = retained_temp_dir("anvil-live-")?;
                // Engine hearth predicate needs tracks/ + tracks.md.
                let tracks = tmp.join("tracks");
                std::fs::create_dir_all(&tracks).map_err(|e| format!("create tracks: {}", e))?;
                std::fs::write(tmp.join("tracks.md"), "# Tracks\n")
                    .map_err(|e| format!("write tracks.md: {}", e))?;

                // OPEN instance — folds to `spec`, RAW actor Atlas-Doer-4211.
                write_track(
                    &tracks,
                    "20260707T0000_open_track",
                    open_track_status_yaml(),
                )?;
                // COMPLETED instance — terminal, must be dropped.
                write_track(
                    &tracks,
                    "20260707T0100_done_track",
                    done_track_status_yaml(),
                )?;

                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set::<RetainedTempDir>("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        async_step_def(
            "the LiveInstances RPC is called",
            &[("engine_process", "EngineProcess")],
            &[
                (LI_GRPC_KEY, "GrpcLive"),
                ("engine_process", "EngineProcess"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let result =
                    match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(
                        addr,
                    )
                    .await
                    {
                        Ok(mut client) => {
                            let request =
                                anvil_test_support::surfaced(anvil_engine::proto::LiveInstancesRequest {
                                    hearth_path: String::new(),
                                    all_hearths: false,
                                });
                            match client.live_instances(request).await {
                                Ok(response) => GrpcLive::Success(response.into_inner()),
                                Err(status) => GrpcLive::Error {
                                    code: format!("{:?}", status.code()),
                                    message: status.message().to_string(),
                                },
                            }
                        }
                        Err(e) => GrpcLive::Error {
                            code: "UNAVAILABLE".to_string(),
                            message: format!("Connection failed: {}", e),
                        },
                    };
                let mut out = Context::new();
                out.set(LI_GRPC_KEY, result);
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        async_step_def(
            "a live_instances JSON-RPC request is sent over /ws with hearth_path {string}",
            &[("engine_process", "EngineProcess")],
            &[("engine_process", "EngineProcess"), (LI_WS_KEY, "Value")],
            |mut ctx, params| async move {
                let hearth_path = params.get_string(0).unwrap_or_default().to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let request = json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "live_instances",
                    "params": { "surface": "test-harness", "hearth_path": hearth_path }
                });
                let response = ws_roundtrip(engine.port, &request).await?;
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set::<Value>(LI_WS_KEY, response);
                Ok(out)
            },
        ),
        // ---- gRPC assertions --------------------------------------------
        check_def(
            "live instances has an instance {string}",
            &[(LI_GRPC_KEY, "GrpcLive")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected instance id")?;
                let resp = grpc_live(&ctx)?;
                live_instance(resp, key)
                    .map(|_| ())
                    .ok_or_else(|| format!("No live instance for '{}'", key))
            },
        ),
        check_def(
            "live instances has no instance {string}",
            &[(LI_GRPC_KEY, "GrpcLive")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected instance id")?;
                let resp = grpc_live(&ctx)?;
                if live_instance(resp, key).is_some() {
                    Err(format!("Live instance '{}' present, expected dropped", key))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the live instance {string} has kind {string}",
            &[(LI_GRPC_KEY, "GrpcLive")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected instance id")?;
                let kind = params.get_string(1).ok_or("Expected kind")?;
                let resp = grpc_live(&ctx)?;
                let inst = live_instance(resp, key)
                    .ok_or_else(|| format!("No live instance for '{}'", key))?;
                if inst.kind == kind {
                    Ok(())
                } else {
                    Err(format!(
                        "Instance '{}' kind '{}' != expected '{}'",
                        key, inst.kind, kind
                    ))
                }
            },
        ),
        check_def(
            "the live instance {string} has state {string}",
            &[(LI_GRPC_KEY, "GrpcLive")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected instance id")?;
                let state = params.get_string(1).ok_or("Expected state")?;
                let resp = grpc_live(&ctx)?;
                let inst = live_instance(resp, key)
                    .ok_or_else(|| format!("No live instance for '{}'", key))?;
                if inst.state == state {
                    Ok(())
                } else {
                    Err(format!(
                        "Instance '{}' state '{}' != expected '{}'",
                        key, inst.state, state
                    ))
                }
            },
        ),
        check_def(
            "the live instance {string} has actor {string}",
            &[(LI_GRPC_KEY, "GrpcLive")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected instance id")?;
                let actor = params.get_string(1).ok_or("Expected actor")?;
                let resp = grpc_live(&ctx)?;
                let inst = live_instance(resp, key)
                    .ok_or_else(|| format!("No live instance for '{}'", key))?;
                if inst.actor == actor {
                    Ok(())
                } else {
                    Err(format!(
                        "Instance '{}' actor '{}' != expected '{}'",
                        key, inst.actor, actor
                    ))
                }
            },
        ),
        check_def(
            "the live instance {string} actor is a raw name not a hash",
            &[(LI_GRPC_KEY, "GrpcLive")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected instance id")?;
                let resp = grpc_live(&ctx)?;
                let inst = live_instance(resp, key)
                    .ok_or_else(|| format!("No live instance for '{}'", key))?;
                if inst.actor.trim().is_empty() {
                    return Err(format!("Instance '{}' actor is empty", key));
                }
                if looks_like_hash(&inst.actor) {
                    return Err(format!(
                        "Instance '{}' actor '{}' looks like a salted hash, not a raw name",
                        key, inst.actor
                    ));
                }
                Ok(())
            },
        ),
        // ---- /ws assertions ---------------------------------------------
        check_def(
            "the /ws live_instances result has an instance {string}",
            &[(LI_WS_KEY, "Value")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected instance id")?;
                let result = ws_result(&ctx, LI_WS_KEY)?;
                ws_instance(&result, key)
                    .map(|_| ())
                    .ok_or_else(|| format!("No /ws live instance for '{}'", key))
            },
        ),
        check_def(
            "the /ws live_instances result has no instance {string}",
            &[(LI_WS_KEY, "Value")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected instance id")?;
                let result = ws_result(&ctx, LI_WS_KEY)?;
                if ws_instance(&result, key).is_some() {
                    Err(format!(
                        "/ws live instance '{}' present, expected dropped",
                        key
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the /ws live_instances instance {string} has actor {string}",
            &[(LI_WS_KEY, "Value")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected instance id")?;
                let actor = params.get_string(1).ok_or("Expected actor")?;
                let result = ws_result(&ctx, LI_WS_KEY)?;
                let inst = ws_instance(&result, key)
                    .ok_or_else(|| format!("No /ws live instance for '{}'", key))?;
                let actual = inst.get("actor").and_then(Value::as_str).unwrap_or("");
                if actual == actor {
                    Ok(())
                } else {
                    Err(format!(
                        "/ws instance '{}' actor '{}' != expected '{}'",
                        key, actual, actor
                    ))
                }
            },
        ),
        // ---- Fix B: honest live-vs-dormant fixtures + assertions --------
        check_def(
            "a dormant open track {string} exists in that hearth",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let id = params.get_string(0).ok_or("Expected instance id")?.to_string();
                let hearth = ctx
                    .get::<std::path::PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?;
                write_track(&hearth.join("tracks"), &id, dormant_track_status_yaml())
            },
        ),
        check_def(
            "the temper step0 stream for kind {string} has {int} events for instance {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let count = params.get_int(1).ok_or("Expected count")? as u32;
                let instance = params.get_string(2).ok_or("Expected instance")?.to_string();
                let hearth = ctx
                    .get::<std::path::PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?;
                write_step0_events(hearth, &kind, &instance, count)
            },
        ),
        check_def(
            "live instances has idle_count {int}",
            &[(LI_GRPC_KEY, "GrpcLive")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected idle_count")? as u32;
                let resp = grpc_live(&ctx)?;
                if resp.idle_count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "idle_count {} != expected {}",
                        resp.idle_count, expected
                    ))
                }
            },
        ),
        check_def(
            "the live instance {string} has action_count {int}",
            &[(LI_GRPC_KEY, "GrpcLive")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected instance id")?;
                let expected = params.get_int(1).ok_or("Expected action_count")? as u32;
                let resp = grpc_live(&ctx)?;
                let inst = live_instance(resp, key)
                    .ok_or_else(|| format!("No live instance for '{}'", key))?;
                if inst.action_count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "instance '{}' action_count {} != expected {}",
                        key, inst.action_count, expected
                    ))
                }
            },
        ),
        check_def(
            "the live instance {string} has current_step {string}",
            &[(LI_GRPC_KEY, "GrpcLive")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected instance id")?;
                let expected = params.get_string(1).ok_or("Expected current_step")?;
                let resp = grpc_live(&ctx)?;
                let inst = live_instance(resp, key)
                    .ok_or_else(|| format!("No live instance for '{}'", key))?;
                if inst.current_step == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "instance '{}' current_step '{}' != expected '{}'",
                        key, inst.current_step, expected
                    ))
                }
            },
        ),
        check_def(
            "the live instance {string} has artifact_dir ending with {string}",
            &[(LI_GRPC_KEY, "GrpcLive")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected instance id")?;
                let suffix = params.get_string(1).ok_or("Expected artifact_dir suffix")?;
                let resp = grpc_live(&ctx)?;
                let inst = live_instance(resp, key)
                    .ok_or_else(|| format!("No live instance for '{}'", key))?;
                if inst.artifact_dir.ends_with(suffix) {
                    Ok(())
                } else {
                    Err(format!(
                        "instance '{}' artifact_dir '{}' does not end with '{}'",
                        key, inst.artifact_dir, suffix
                    ))
                }
            },
        ),
    ]
}
