//! Step module for `list_instance_artifacts_rpc.feature` (engine seam).
//!
//! Seeds a HERMETIC fixture hearth with a real track directory carrying a
//! handful of files, then exercises the gRPC `ListInstanceArtifacts` RPC and
//! the /ws `list_instance_artifacts` JSON-RPC method against it — same
//! connect-send-recv shape as `playbook_atlas_rpc` / `live_instances_rpc`.

use anvil_test_support::engine::EngineProcess;
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{async_step_def, check_def, StepDef};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

use anvil_test_support::{retained_temp_dir, RetainedTempDir};

const LIA_GRPC_KEY: &str = "lia_grpc_result";
const LIA_WS_KEY: &str = "lia_ws_response";
const OUTSIDE_DIR_KEY: &str = "lia_outside_dir";

/// Resolve "that track directory" AFTER the engine has started: the shared
/// "the engine is started with that hearth" step is a Map step that replaces
/// context with only its OWN declared provides — any custom key a Given step
/// set before it (like a stashed instance_dir PathBuf) is dropped. Every
/// fixture in this module creates exactly ONE directory under
/// `<hearth>/tracks/`, so re-deriving it by listing that directory (instead
/// of threading a context key through the shared step) is both simpler and
/// robust to that context-replacement behavior.
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

enum GrpcListArtifacts {
    Success(anvil_engine::proto::ListInstanceArtifactsResponse),
    Error { code: String, message: String },
}

fn write_files(dir: &std::path::Path, table: &DataTable) -> Result<(), String> {
    let name_col = table
        .headers
        .iter()
        .position(|h| h == "name")
        .ok_or("Missing 'name' column in data table")?;
    let content_col = table
        .headers
        .iter()
        .position(|h| h == "content")
        .ok_or("Missing 'content' column in data table")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("mkdir {}: {}", dir.display(), e))?;
    for row in &table.rows {
        let name = row[name_col].trim();
        // The feature file's DataTable cells carry literal "\n" as two chars
        // (a real Gherkin table cell can't hold a newline) — unescape it so
        // the preview assertions see real line breaks, matching how a real
        // spec.md would read.
        let content = row[content_col].replace("\\n", "\n");
        std::fs::write(dir.join(name), content)
            .map_err(|e| format!("write {}/{}: {}", dir.display(), name, e))?;
    }
    Ok(())
}

fn grpc_list_artifacts(
    ctx: &Context,
) -> Result<&anvil_engine::proto::ListInstanceArtifactsResponse, String> {
    match ctx
        .get::<GrpcListArtifacts>(LIA_GRPC_KEY)
        .ok_or("No list_instance_artifacts gRPC result")?
    {
        GrpcListArtifacts::Success(resp) => Ok(resp),
        GrpcListArtifacts::Error { code, message } => Err(format!(
            "Expected list_instance_artifacts success, got gRPC {}: {}",
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
            "a hearth with a track directory {string} containing files:",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| async move {
                let track_id = params.get_string(0).ok_or("Expected track directory name")?.to_string();
                let table = params.data_table().ok_or("Expected data table")?.clone();

                let (handle, tmp) = retained_temp_dir("anvil-lia-")?;
                std::fs::create_dir_all(tmp.join("tracks"))
                    .map_err(|e| format!("create tracks: {}", e))?;
                std::fs::write(tmp.join("tracks.md"), "# Tracks\n")
                    .map_err(|e| format!("write tracks.md: {}", e))?;

                let instance_dir = tmp.join("tracks").join(&track_id);
                write_files(&instance_dir, &table)?;

                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set::<RetainedTempDir>("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        async_step_def(
            "an unrelated directory outside any permitted root containing a file {string}",
            &[("engine_process", "EngineProcess")],
            &[
                (OUTSIDE_DIR_KEY, "PathBuf"),
                ("lia_outside_dir_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("engine_process", "EngineProcess"),
            ],
            |mut ctx, params| async move {
                let filename = params.get_string(0).ok_or("Expected filename")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let (handle, tmp) = retained_temp_dir("anvil-lia-outside-")?;
                std::fs::write(tmp.join(&filename), "shh")
                    .map_err(|e| format!("write {}: {}", filename, e))?;
                let mut out = Context::new();
                out.set(OUTSIDE_DIR_KEY, tmp);
                out.set::<RetainedTempDir>("lia_outside_dir_handle", handle);
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        async_step_def(
            "the ListInstanceArtifacts RPC is called for that track directory",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[(LIA_GRPC_KEY, "GrpcListArtifacts"), ("engine_process", "EngineProcess")],
            |mut ctx, _params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<std::path::PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let instance_dir = single_track_dir(&hearth_path)?;
                let result = call_list_instance_artifacts(engine.port, instance_dir.display().to_string()).await;
                let mut out = Context::new();
                out.set(LIA_GRPC_KEY, result);
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        async_step_def(
            "the ListInstanceArtifacts RPC is called for the unrelated directory",
            &[("engine_process", "EngineProcess"), (OUTSIDE_DIR_KEY, "PathBuf")],
            &[(LIA_GRPC_KEY, "GrpcListArtifacts"), ("engine_process", "EngineProcess")],
            |mut ctx, _params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let outside_dir = ctx.get::<std::path::PathBuf>(OUTSIDE_DIR_KEY).ok_or("No outside dir")?.clone();
                let result = call_list_instance_artifacts(engine.port, outside_dir.display().to_string()).await;
                let mut out = Context::new();
                out.set(LIA_GRPC_KEY, result);
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        async_step_def(
            "the ListInstanceArtifacts RPC is called for instance_dir {string}",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[(LIA_GRPC_KEY, "GrpcListArtifacts"), ("engine_process", "EngineProcess")],
            |mut ctx, params| async move {
                let raw = params.get_string(0).ok_or("Expected instance_dir")?.to_string();
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<std::path::PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let instance_dir = raw.replace("<hearth>", &hearth_path.display().to_string());
                let result = call_list_instance_artifacts(engine.port, instance_dir).await;
                let mut out = Context::new();
                out.set(LIA_GRPC_KEY, result);
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        async_step_def(
            "a list_instance_artifacts JSON-RPC request is sent over /ws for that track directory",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[("engine_process", "EngineProcess"), (LIA_WS_KEY, "Value")],
            |mut ctx, _params| async move {
                let engine = ctx.take::<EngineProcess>("engine_process").ok_or("No engine_process")?;
                let hearth_path = ctx.get::<std::path::PathBuf>("hearth_path").ok_or("No hearth_path")?.clone();
                let instance_dir = single_track_dir(&hearth_path)?;
                let request = json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "list_instance_artifacts",
                    "params": { "surface": "test-harness", "instance_dir": instance_dir.display().to_string() }
                });
                let response = ws_roundtrip(engine.port, &request).await?;
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set::<Value>(LIA_WS_KEY, response);
                Ok(out)
            },
        ),
        check_def(
            "the list_instance_artifacts result has {int} artifacts",
            &[(LIA_GRPC_KEY, "GrpcListArtifacts")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let resp = grpc_list_artifacts(&ctx)?;
                if resp.artifacts.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "expected {} artifacts, got {} ({:?})",
                        expected,
                        resp.artifacts.len(),
                        resp.artifacts.iter().map(|a| &a.name).collect::<Vec<_>>()
                    ))
                }
            },
        ),
        check_def(
            "the list_instance_artifacts result artifact {string} has preview containing {string}",
            &[(LIA_GRPC_KEY, "GrpcListArtifacts")],
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected artifact name")?;
                let needle = params.get_string(1).ok_or("Expected preview substring")?;
                let resp = grpc_list_artifacts(&ctx)?;
                let artifact = resp
                    .artifacts
                    .iter()
                    .find(|a| a.name == name)
                    .ok_or_else(|| format!("no artifact named '{}'", name))?;
                if artifact.preview.contains(needle) {
                    Ok(())
                } else {
                    Err(format!(
                        "artifact '{}' preview '{}' does not contain '{}'",
                        name, artifact.preview, needle
                    ))
                }
            },
        ),
        check_def(
            "the list_instance_artifacts result artifact {string} has size greater than {int}",
            &[(LIA_GRPC_KEY, "GrpcListArtifacts")],
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected artifact name")?;
                let floor = params.get_int(1).ok_or("Expected size floor")? as u64;
                let resp = grpc_list_artifacts(&ctx)?;
                let artifact = resp
                    .artifacts
                    .iter()
                    .find(|a| a.name == name)
                    .ok_or_else(|| format!("no artifact named '{}'", name))?;
                if artifact.size_bytes > floor {
                    Ok(())
                } else {
                    Err(format!(
                        "artifact '{}' size_bytes {} is not greater than {}",
                        name, artifact.size_bytes, floor
                    ))
                }
            },
        ),
        check_def(
            "the ListInstanceArtifacts RPC fails with permission_denied",
            &[(LIA_GRPC_KEY, "GrpcListArtifacts")],
            |ctx, _params| match ctx.get::<GrpcListArtifacts>(LIA_GRPC_KEY).ok_or("No gRPC result")? {
                GrpcListArtifacts::Error { code, .. } if code == "PermissionDenied" => Ok(()),
                GrpcListArtifacts::Error { code, message } => Err(format!(
                    "expected PermissionDenied, got {}: {}",
                    code, message
                )),
                GrpcListArtifacts::Success(_) => {
                    Err("expected PermissionDenied, got a success response".to_string())
                }
            },
        ),
        check_def(
            "the ListInstanceArtifacts RPC fails with invalid_argument",
            &[(LIA_GRPC_KEY, "GrpcListArtifacts")],
            |ctx, _params| match ctx.get::<GrpcListArtifacts>(LIA_GRPC_KEY).ok_or("No gRPC result")? {
                GrpcListArtifacts::Error { code, .. } if code == "InvalidArgument" => Ok(()),
                GrpcListArtifacts::Error { code, message } => Err(format!(
                    "expected InvalidArgument, got {}: {}",
                    code, message
                )),
                GrpcListArtifacts::Success(_) => {
                    Err("expected InvalidArgument, got a success response".to_string())
                }
            },
        ),
        check_def(
            "the /ws list_instance_artifacts result has {int} artifacts",
            &[(LIA_WS_KEY, "Value")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let result = ws_result(&ctx, LIA_WS_KEY)?;
                let artifacts = result
                    .get("artifacts")
                    .and_then(Value::as_array)
                    .ok_or("/ws result has no artifacts array")?;
                if artifacts.len() == expected {
                    Ok(())
                } else {
                    Err(format!("/ws artifacts len {} != expected {}", artifacts.len(), expected))
                }
            },
        ),
        check_def(
            "the /ws list_instance_artifacts result artifact {string} has preview containing {string}",
            &[(LIA_WS_KEY, "Value")],
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected artifact name")?;
                let needle = params.get_string(1).ok_or("Expected preview substring")?;
                let result = ws_result(&ctx, LIA_WS_KEY)?;
                let artifact = result
                    .get("artifacts")
                    .and_then(Value::as_array)
                    .ok_or("/ws result has no artifacts array")?
                    .iter()
                    .find(|a| a.get("name").and_then(Value::as_str) == Some(name))
                    .ok_or_else(|| format!("no /ws artifact named '{}'", name))?;
                let preview = artifact.get("preview").and_then(Value::as_str).unwrap_or("");
                if preview.contains(needle) {
                    Ok(())
                } else {
                    Err(format!("/ws artifact '{}' preview '{}' does not contain '{}'", name, preview, needle))
                }
            },
        ),
    ]
}

/// Dial the gRPC `ListInstanceArtifacts` RPC at `port` for `instance_dir`,
/// mapping a transport/status failure into the same `GrpcListArtifacts::Error`
/// shape every other RPC step in this crate uses.
async fn call_list_instance_artifacts(port: u16, instance_dir: String) -> GrpcListArtifacts {
    let addr = format!("http://127.0.0.1:{}", port);
    match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
        Ok(mut client) => {
            let request = anvil_test_support::surfaced(anvil_engine::proto::ListInstanceArtifactsRequest {
                instance_dir,
            });
            match client.list_instance_artifacts(request).await {
                Ok(response) => GrpcListArtifacts::Success(response.into_inner()),
                Err(status) => GrpcListArtifacts::Error {
                    code: format!("{:?}", status.code()),
                    message: status.message().to_string(),
                },
            }
        }
        Err(e) => GrpcListArtifacts::Error {
            code: "UNAVAILABLE".to_string(),
            message: format!("Connection failed: {}", e),
        },
    }
}
