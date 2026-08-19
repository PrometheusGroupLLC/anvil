//! Step module for `read_instance_artifact_rpc.feature` (engine seam).
//!
//! Seeds a HERMETIC fixture hearth with a real track directory carrying a
//! handful of files, then exercises the gRPC `ReadInstanceArtifact` RPC and
//! the /ws `read_instance_artifact` JSON-RPC method against it — same
//! connect-send-recv shape as `list_instance_artifacts_rpc`.

use anvil_test_support::engine::EngineProcess;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{async_step_def, check_def, StepDef};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

use anvil_test_support::{retained_temp_dir, RetainedTempDir};

const RIA_GRPC_KEY: &str = "ria_grpc_result";
const RIA_WS_KEY: &str = "ria_ws_response";

/// Resolve "that track directory" AFTER the engine has started — mirrors
/// `list_instance_artifacts_rpc::single_track_dir`: the shared "the engine is
/// started with that hearth" step replaces context with only its OWN
/// declared provides, so any custom key set beforehand (like a stashed
/// instance_dir) is dropped. Every fixture in this module creates exactly ONE
/// directory under `<hearth>/tracks/`.
fn single_track_dir(hearth_path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let tracks = hearth_path.join("tracks");
    let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(&tracks)
        .map_err(|e| format!("read_dir {}: {}", tracks.display(), e))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    if entries.len() != 1 {
        return Err(format!(
            "expected exactly 1 track directory under {}, found {}",
            tracks.display(),
            entries.len()
        ));
    }
    Ok(entries.remove(0))
}

enum GrpcReadArtifact {
    Success(anvil_engine::proto::ReadInstanceArtifactResponse),
    Error { code: String, message: String },
}

fn grpc_read_artifact(
    ctx: &Context,
) -> Result<&anvil_engine::proto::ReadInstanceArtifactResponse, String> {
    match ctx
        .get::<GrpcReadArtifact>(RIA_GRPC_KEY)
        .ok_or("No read_instance_artifact gRPC result")?
    {
        GrpcReadArtifact::Success(resp) => Ok(resp),
        GrpcReadArtifact::Error { code, message } => Err(format!(
            "Expected read_instance_artifact success, got gRPC {}: {}",
            code, message
        )),
    }
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
            "a hearth with a track directory {string} containing an oversized file {string} of {int} bytes",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| async move {
                let track_id = params.get_string(0).ok_or("Expected track directory name")?.to_string();
                let file_name = params.get_string(1).ok_or("Expected file name")?.to_string();
                let byte_count = params.get_int(2).ok_or("Expected byte count")? as usize;

                let (handle, tmp) = retained_temp_dir("anvil-ria-")?;
                std::fs::create_dir_all(tmp.join("tracks"))
                    .map_err(|e| format!("create tracks: {}", e))?;
                std::fs::write(tmp.join("tracks.md"), "# Tracks\n")
                    .map_err(|e| format!("write tracks.md: {}", e))?;

                let instance_dir = tmp.join("tracks").join(&track_id);
                std::fs::create_dir_all(&instance_dir)
                    .map_err(|e| format!("mkdir {}: {}", instance_dir.display(), e))?;
                // Repeating ASCII content so the byte count is exact and the
                // file is valid UTF-8 (a real oversized artifact would be too).
                let content: String = "x".repeat(byte_count);
                std::fs::write(instance_dir.join(&file_name), content)
                    .map_err(|e| format!("write {}: {}", file_name, e))?;

                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set::<RetainedTempDir>("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        async_step_def(
            "the ReadInstanceArtifact RPC is called for that track directory and file {string}",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[(RIA_GRPC_KEY, "GrpcReadArtifact"), ("engine_process", "EngineProcess")],
            |mut ctx, params| async move {
                let name = params.get_string(0).ok_or("Expected file name")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<std::path::PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let instance_dir = single_track_dir(&hearth_path)?;
                let result = call_read_instance_artifact(engine.port, instance_dir.display().to_string(), name).await;
                let mut out = Context::new();
                out.set(RIA_GRPC_KEY, result);
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        async_step_def(
            "a read_instance_artifact JSON-RPC request is sent over /ws for that track directory and file {string}",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[("engine_process", "EngineProcess"), (RIA_WS_KEY, "Value")],
            |mut ctx, params| async move {
                let name = params.get_string(0).ok_or("Expected file name")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<std::path::PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let instance_dir = single_track_dir(&hearth_path)?;
                let request = json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "read_instance_artifact",
                    "params": { "surface": "test-harness", "instance_dir": instance_dir.display().to_string(), "name": name }
                });
                let response = ws_roundtrip(engine.port, &request).await?;
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set::<Value>(RIA_WS_KEY, response);
                Ok(out)
            },
        ),
        check_def(
            "the read_instance_artifact result has content {string}",
            &[(RIA_GRPC_KEY, "GrpcReadArtifact")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected content string")?.replace("\\n", "\n");
                let resp = grpc_read_artifact(&ctx)?;
                if resp.content == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "expected content '{}', got '{}'",
                        expected, resp.content
                    ))
                }
            },
        ),
        check_def(
            "the read_instance_artifact result is not truncated",
            &[(RIA_GRPC_KEY, "GrpcReadArtifact")],
            |ctx, _params| {
                let resp = grpc_read_artifact(&ctx)?;
                if !resp.truncated {
                    Ok(())
                } else {
                    Err("expected truncated=false, got true".to_string())
                }
            },
        ),
        check_def(
            "the read_instance_artifact result is truncated",
            &[(RIA_GRPC_KEY, "GrpcReadArtifact")],
            |ctx, _params| {
                let resp = grpc_read_artifact(&ctx)?;
                if resp.truncated {
                    Ok(())
                } else {
                    Err("expected truncated=true, got false".to_string())
                }
            },
        ),
        check_def(
            "the read_instance_artifact result content has length {int}",
            &[(RIA_GRPC_KEY, "GrpcReadArtifact")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected length")? as usize;
                let resp = grpc_read_artifact(&ctx)?;
                if resp.content.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "expected content length {}, got {}",
                        expected,
                        resp.content.len()
                    ))
                }
            },
        ),
        check_def(
            "the read_instance_artifact result size_bytes is {int}",
            &[(RIA_GRPC_KEY, "GrpcReadArtifact")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected size_bytes")? as u64;
                let resp = grpc_read_artifact(&ctx)?;
                if resp.size_bytes == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "expected size_bytes {}, got {}",
                        expected, resp.size_bytes
                    ))
                }
            },
        ),
        check_def(
            "the read_instance_artifact result has a non-empty error_message",
            &[(RIA_GRPC_KEY, "GrpcReadArtifact")],
            |ctx, _params| {
                let resp = grpc_read_artifact(&ctx)?;
                if !resp.error_message.is_empty() {
                    Ok(())
                } else {
                    Err("expected a non-empty error_message, got empty".to_string())
                }
            },
        ),
        check_def(
            "the ReadInstanceArtifact RPC fails with permission_denied",
            &[(RIA_GRPC_KEY, "GrpcReadArtifact")],
            |ctx, _params| match ctx.get::<GrpcReadArtifact>(RIA_GRPC_KEY).ok_or("No gRPC result")? {
                GrpcReadArtifact::Error { code, .. } if code == "PermissionDenied" => Ok(()),
                GrpcReadArtifact::Error { code, message } => Err(format!(
                    "expected PermissionDenied, got {}: {}",
                    code, message
                )),
                GrpcReadArtifact::Success(_) => {
                    Err("expected PermissionDenied, got a success response".to_string())
                }
            },
        ),
        check_def(
            "the /ws read_instance_artifact result has content {string}",
            &[(RIA_WS_KEY, "Value")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected content string")?.replace("\\n", "\n");
                let result = ws_result(&ctx, RIA_WS_KEY)?;
                let content = result.get("content").and_then(Value::as_str).unwrap_or("");
                if content == expected {
                    Ok(())
                } else {
                    Err(format!("/ws content '{}' != expected '{}'", content, expected))
                }
            },
        ),
        check_def(
            "the /ws read_instance_artifact result is not truncated",
            &[(RIA_WS_KEY, "Value")],
            |ctx, _params| {
                let result = ws_result(&ctx, RIA_WS_KEY)?;
                let truncated = result.get("truncated").and_then(Value::as_bool).unwrap_or(true);
                if !truncated {
                    Ok(())
                } else {
                    Err("/ws expected truncated=false, got true".to_string())
                }
            },
        ),
        check_def(
            "the /ws read_instance_artifact result carries name, size_bytes, modified_at, truncated, and error_message fields",
            &[(RIA_WS_KEY, "Value")],
            |ctx, _params| {
                let result = ws_result(&ctx, RIA_WS_KEY)?;
                for field in ["name", "content", "size_bytes", "modified_at", "truncated", "error_message"] {
                    if result.get(field).is_none() {
                        return Err(format!("/ws read_instance_artifact result is missing field '{}': {}", field, result));
                    }
                }
                Ok(())
            },
        ),
    ]
}

/// Dial the gRPC `ReadInstanceArtifact` RPC at `port` for `instance_dir` /
/// `name`, mapping a transport/status failure into the same
/// `GrpcReadArtifact::Error` shape every other RPC step in this crate uses.
async fn call_read_instance_artifact(
    port: u16,
    instance_dir: String,
    name: String,
) -> GrpcReadArtifact {
    let addr = format!("http://127.0.0.1:{}", port);
    match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
        Ok(mut client) => {
            let request = anvil_test_support::surfaced(anvil_engine::proto::ReadInstanceArtifactRequest {
                instance_dir,
                name,
            });
            match client.read_instance_artifact(request).await {
                Ok(response) => GrpcReadArtifact::Success(response.into_inner()),
                Err(status) => GrpcReadArtifact::Error {
                    code: format!("{:?}", status.code()),
                    message: status.message().to_string(),
                },
            }
        }
        Err(e) => GrpcReadArtifact::Error {
            code: "UNAVAILABLE".to_string(),
            message: format!("Connection failed: {}", e),
        },
    }
}
