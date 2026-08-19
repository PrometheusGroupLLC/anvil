//! Step module for `hook_manifest_rpc.feature` (engine seam).
//!
//! Seeds a real engine hearth with a playbook machine that declares
//! `hooks_by_role` AND writes the referenced hook BODIES under `hooks/`, then
//! reuses the shared `engine` module's "the engine is started with that hearth"
//! step. This module adds the seeding step, the gRPC HookManifest call + its
//! assertions, and the /ws `hook_manifest` request + its assertions. The /ws
//! request/transport reuses the same connect-send-recv shape as the existing
//! `ws_bridge` module (one frame round-trip on `/ws`).

use anvil_test_support::engine::EngineProcess;
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{async_step_def, check_def, StepDef};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use tokio_tungstenite::tungstenite::Message;

use anvil_test_support::{retained_temp_dir, RetainedTempDir};

const GRPC_RESULT_KEY: &str = "hmr_grpc_result";
const WS_RESPONSE_KEY: &str = "hmr_ws_response";

enum GrpcResult {
    Success(anvil_engine::proto::HookManifestResponse),
    Error { code: String, message: String },
}

fn column_index(table: &DataTable, name: &str) -> Result<usize, String> {
    table
        .headers
        .iter()
        .position(|h| h == name)
        .ok_or_else(|| format!("Missing '{}' column in data table", name))
}

/// Build a driven machine declaring `hooks_by_role` for each (state, role) row,
/// so the engine's registry resolves it and the fold enumerates its hooks.
fn machine_yaml_with_hooks(
    kind: &str,
    by_state: &BTreeMap<String, BTreeMap<String, String>>,
) -> String {
    let mut states = String::new();
    for (state, hooks) in by_state {
        // This fixture exercises hook serving only; transitions are empty, so
        // every state is marked terminal to satisfy the registration-time
        // contiguity gate (a non-terminal state must have an outgoing edge).
        states.push_str(&format!(
            "  - name: {state}\n    role_filters: []\n    registry_section: {state}\n    projection_targets: []\n    is_review_gate: false\n    is_terminal: true\n    hooks_by_role:\n",
            state = state
        ));
        for (role, filename) in hooks {
            states.push_str(&format!("      {}: {}\n", role, filename));
        }
    }
    format!(
        r#"kind: {kind}
route:
  triggers:
    - "{kind}"
directory: {kind}s
registry: {kind}s.md
description: "{kind} with hooks"
required_fields: []
roles:
  - doer
  - reviewer
states:
{states}transitions: []
"#,
        kind = kind,
        states = states
    )
}

fn entry_for<'a>(
    resp: &'a anvil_engine::proto::HookManifestResponse,
    kind: &str,
    state: &str,
    role: &str,
) -> Option<&'a anvil_engine::proto::ResolvedHook> {
    resp.hooks
        .iter()
        .find(|h| h.artifact_kind == kind && h.state == state && h.role == role)
}

fn ws_result(ctx: &Context) -> Result<Value, String> {
    let response = ctx
        .get::<Value>(WS_RESPONSE_KEY)
        .ok_or("No hmr_ws_response in context")?;
    if let Some(error) = response.get("error") {
        return Err(format!("Expected JSON-RPC result, got error: {}", error));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| format!("JSON-RPC response has no result: {}", response))
}

fn ws_hook<'a>(result: &'a Value, kind: &str, state: &str, role: &str) -> Option<&'a Value> {
    result
        .get("hooks")
        .and_then(Value::as_array)?
        .iter()
        .find(|h| {
            h.get("artifact_kind").and_then(Value::as_str) == Some(kind)
                && h.get("state").and_then(Value::as_str) == Some(state)
                && h.get("role").and_then(Value::as_str) == Some(role)
        })
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
            "a hook manifest engine hearth with playbook {string} hooks:",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| async move {
                let kind = params.get_string(0).ok_or("Expected playbook kind")?.to_string();
                let table = params.data_table().ok_or("Expected data table")?;
                let state_col = column_index(table, "state")?;
                let role_col = column_index(table, "role")?;
                let file_col = column_index(table, "filename")?;
                let body_col = column_index(table, "body")?;

                let (handle, tmp) = retained_temp_dir("anvil-hmr-")?;
                // Engine hearth predicate needs tracks/ + tracks.md.
                std::fs::create_dir_all(tmp.join("tracks"))
                    .map_err(|e| format!("create tracks: {}", e))?;
                std::fs::write(tmp.join("tracks.md"), "# Tracks\n")
                    .map_err(|e| format!("write tracks.md: {}", e))?;

                // Group rows into states; write each hook body under hooks/.
                let wf_id = format!("{}_dir", kind);
                let wf_dir = tmp.join("playbooks").join(&wf_id);
                let hooks_dir = wf_dir.join("hooks");
                std::fs::create_dir_all(&hooks_dir)
                    .map_err(|e| format!("create hooks dir: {}", e))?;

                let mut by_state: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
                for row in &table.rows {
                    let state = row[state_col].trim().to_string();
                    let role = row[role_col].trim().to_string();
                    let filename = row[file_col].trim().to_string();
                    let body = row[body_col].trim().to_string();
                    std::fs::write(hooks_dir.join(&filename), body)
                        .map_err(|e| format!("write hook body {}: {}", filename, e))?;
                    by_state
                        .entry(state)
                        .or_default()
                        .insert(role, filename);
                }

                std::fs::write(
                    wf_dir.join("machine.yaml"),
                    machine_yaml_with_hooks(&kind, &by_state),
                )
                .map_err(|e| format!("write machine.yaml: {}", e))?;
                // status.yaml keeps the kind registry-resolvable.
                std::fs::write(
                    wf_dir.join("status.yaml"),
                    "version: 1\nkind: playbook\nstate: active\nactors: {}\ntransitions: []\n",
                )
                .map_err(|e| format!("write status.yaml: {}", e))?;

                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set::<RetainedTempDir>("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        async_step_def(
            "the HookManifest RPC is called",
            &[("engine_process", "EngineProcess")],
            &[
                (GRPC_RESULT_KEY, "GrpcResult"),
                ("engine_process", "EngineProcess"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let port = engine.port;
                let addr = format!("http://127.0.0.1:{}", port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = anvil_test_support::surfaced(anvil_engine::proto::HookManifestRequest {
                            hearth_path: String::new(),
                        });
                        match client.hook_manifest(request).await {
                            Ok(response) => GrpcResult::Success(response.into_inner()),
                            Err(status) => GrpcResult::Error {
                                code: format!("{:?}", status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => GrpcResult::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };
                let mut out = Context::new();
                out.set(GRPC_RESULT_KEY, result);
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        check_def(
            "the hook manifest RPC gate_query is {string}",
            &[(GRPC_RESULT_KEY, "GrpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected gate_query")?;
                let result = ctx.get::<GrpcResult>(GRPC_RESULT_KEY).ok_or("No grpc result")?;
                match result {
                    GrpcResult::Success(resp) => {
                        if resp.gate_query == expected {
                            Ok(())
                        } else {
                            Err(format!(
                                "Expected gate_query '{}', got '{}'",
                                expected, resp.gate_query
                            ))
                        }
                    }
                    GrpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the hook manifest RPC has a hook for artifact_kind {string} state {string} role {string} with gate {string}",
            &[(GRPC_RESULT_KEY, "GrpcResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let state = params.get_string(1).ok_or("Expected state")?;
                let role = params.get_string(2).ok_or("Expected role")?;
                let gate = params.get_string(3).ok_or("Expected gate")?;
                let result = ctx.get::<GrpcResult>(GRPC_RESULT_KEY).ok_or("No grpc result")?;
                match result {
                    GrpcResult::Success(resp) => {
                        let hook = entry_for(resp, kind, state, role).ok_or_else(|| {
                            format!("No hook for ({}, {}, {})", kind, state, role)
                        })?;
                        if hook.gate == gate {
                            Ok(())
                        } else {
                            Err(format!(
                                "({}, {}, {}): expected gate '{}', got '{}'",
                                kind, state, role, gate, hook.gate
                            ))
                        }
                    }
                    GrpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the hook manifest RPC hook for artifact_kind {string} state {string} role {string} body contains {string}",
            &[(GRPC_RESULT_KEY, "GrpcResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let state = params.get_string(1).ok_or("Expected state")?;
                let role = params.get_string(2).ok_or("Expected role")?;
                let needle = params.get_string(3).ok_or("Expected substring")?;
                let result = ctx.get::<GrpcResult>(GRPC_RESULT_KEY).ok_or("No grpc result")?;
                match result {
                    GrpcResult::Success(resp) => {
                        let hook = entry_for(resp, kind, state, role).ok_or_else(|| {
                            format!("No hook for ({}, {}, {})", kind, state, role)
                        })?;
                        if hook.body.contains(needle) {
                            Ok(())
                        } else {
                            Err(format!(
                                "({}, {}, {}) body does not contain '{}': {:?}",
                                kind, state, role, needle, hook.body
                            ))
                        }
                    }
                    GrpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        async_step_def(
            "a hook_manifest JSON-RPC request is sent over /ws with hearth_path {string}",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                (WS_RESPONSE_KEY, "Value"),
            ],
            |mut ctx, params| async move {
                let hearth_path = params.get_string(0).unwrap_or_default().to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let request = json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "hook_manifest",
                    "params": { "surface": "test-harness", "hearth_path": hearth_path }
                });
                let response = ws_roundtrip(engine.port, &request).await?;
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set::<Value>(WS_RESPONSE_KEY, response);
                Ok(out)
            },
        ),
        check_def(
            "the /ws hook_manifest result gate_query is {string}",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected gate_query")?;
                let result = ws_result(&ctx)?;
                let actual = result
                    .get("gate_query")
                    .and_then(Value::as_str)
                    .ok_or("result.gate_query missing or not a string")?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("Expected gate_query '{}', got '{}'", expected, actual))
                }
            },
        ),
        check_def(
            "the /ws hook_manifest result has a hook for artifact_kind {string} state {string} role {string} with gate {string}",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let state = params.get_string(1).ok_or("Expected state")?;
                let role = params.get_string(2).ok_or("Expected role")?;
                let gate = params.get_string(3).ok_or("Expected gate")?;
                let result = ws_result(&ctx)?;
                let hook = ws_hook(&result, kind, state, role)
                    .ok_or_else(|| format!("No hook for ({}, {}, {})", kind, state, role))?;
                let actual = hook.get("gate").and_then(Value::as_str).unwrap_or("");
                if actual == gate {
                    Ok(())
                } else {
                    Err(format!(
                        "({}, {}, {}): expected gate '{}', got '{}'",
                        kind, state, role, gate, actual
                    ))
                }
            },
        ),
        check_def(
            "the /ws hook_manifest result hook for artifact_kind {string} state {string} role {string} body contains {string}",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let state = params.get_string(1).ok_or("Expected state")?;
                let role = params.get_string(2).ok_or("Expected role")?;
                let needle = params.get_string(3).ok_or("Expected substring")?;
                let result = ws_result(&ctx)?;
                let hook = ws_hook(&result, kind, state, role)
                    .ok_or_else(|| format!("No hook for ({}, {}, {})", kind, state, role))?;
                let body = hook.get("body").and_then(Value::as_str).unwrap_or("");
                if body.contains(needle) {
                    Ok(())
                } else {
                    Err(format!(
                        "({}, {}, {}) body does not contain '{}': {:?}",
                        kind, state, role, needle, body
                    ))
                }
            },
        ),
    ]
}
