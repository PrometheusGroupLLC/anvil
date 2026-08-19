use crate::{carry_retained_temp_dir, retained_temp_dir};
use anvil_core::domain::begin::{BeginCommandHandler, BeginError, BeginOutcome, BeginRequest};
use anvil_core::domain::events::Event;
use anvil_core::domain::playbook::composite_registry::CompositePlaybookRegistry;
use anvil_core::domain::playbook::hearth_registry::HearthPlaybookRegistry;
use anvil_core::domain::playbook::registry::SeedPlaybookRegistry;
use anvil_core::domain::shared_types::{ActorIdentity, RequestContext};
use anvil_core::domain::snapshot::{
    SnapshotCommandHandler, SnapshotError, SnapshotRequest as DomainSnapshotRequest,
};
use anvil_core_hearth::fs_actor_write_adapter::FileSystemActorWriteAdapter;
use anvil_core_hearth::fs_query_adapter::FileSystemQueryAdapter;
use anvil_core_hearth::fs_snapshot_adapter::FileSystemSnapshotAdapter;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{async_step_def, check_def, step_def, StepDef};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
enum SparkBeginRpcResult {
    Success(anvil_engine::proto::BeginResponse),
    Error { code: String, message: String },
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a spark lifecycle hearth",
            &[],
            &[
                ("spark_hearth", "PathBuf"),
                ("spark_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, _params| {
                let (handle, hearth) = retained_temp_dir("anvil-spark-core-")?;
                seed_spark_hearth(&hearth)?;
                let mut out = Context::new();
                out.set("spark_hearth", hearth);
                out.set("spark_hearth_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a hearth seeded with the spark_lifecycle machine",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, _params| {
                let (handle, hearth) = retained_temp_dir("anvil-spark-engine-")?;
                seed_spark_hearth(&hearth)?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "begin captures spark {string}",
            &[("spark_hearth", "PathBuf")],
            &[
                ("spark_hearth", "PathBuf"),
                ("spark_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("spark_begin_outcome", "Result<BeginOutcome, BeginError>"),
            ],
            |ctx, params| {
                let body = params
                    .get_string(0)
                    .ok_or("Expected spark body")?
                    .to_string();
                let hearth = ctx
                    .get::<PathBuf>("spark_hearth")
                    .ok_or("No spark_hearth")?
                    .clone();
                let registry = CompositePlaybookRegistry::new(
                    HearthPlaybookRegistry::new(hearth.clone()),
                    SeedPlaybookRegistry,
                );
                let query = FileSystemQueryAdapter::new(hearth.clone());
                let request = spark_begin_request(body);
                let outcome = BeginCommandHandler::execute(&query, &registry, request);
                if let Ok(outcome) = &outcome {
                    route_projection_only_events(&hearth, &outcome.events)?;
                }
                let mut out = Context::new();
                out.set("spark_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "spark_hearth_handle");
                out.set("spark_begin_outcome", outcome);
                Ok(out)
            },
        ),
        async_step_def(
            "the begin RPC captures spark {string}",
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
            ],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("spark_begin_rpc_result", "SparkBeginRpcResult"),
            ],
            |mut ctx, params| async move {
                let body = params
                    .get_string(0)
                    .ok_or("Expected spark body")?
                    .to_string();
                let engine = ctx
                    .take::<crate::engine::EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let hearth = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let result =
                    match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(
                        addr,
                    )
                    .await
                    {
                        Ok(mut client) => {
                            let request = anvil_engine::proto::BeginRequest {
                                hearth_path: String::new(),
                                artifact_type: "spark".to_string(),
                                parent_id: String::new(),
                                track_name: body,
                                playbook_name: String::new(),
                                target_owner: String::new(),
                                approver: String::new(),
                                actor_name: "Spark-Rpc-Test-000000".to_string(),
                                actor_type: "agent".to_string(),
                                actor_model: "test".to_string(),
                                actor_provider: "test".to_string(),
                                actor_context_window: 0,
                                actor_sdk_version: String::new(),
                                actor_entrypoint: String::new(),
                                identifier: String::new(),
                                session_role: "creator".to_string(),
                                ctx_org: "Foundation".to_string(),
                                ctx_space: String::new(),
                                ctx_role: "read".to_string(),
                                ctx_clearance: "internal".to_string(),
                                ..Default::default()
                            };
                            match client.begin(crate::surfaced(request)).await {
                                Ok(response) => SparkBeginRpcResult::Success(response.into_inner()),
                                Err(status) => SparkBeginRpcResult::Error {
                                    code: grpc_code_name(status.code()),
                                    message: status.message().to_string(),
                                },
                            }
                        }
                        Err(e) => SparkBeginRpcResult::Error {
                            code: "UNAVAILABLE".to_string(),
                            message: format!("Connection failed: {}", e),
                        },
                    };
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                out.set("spark_begin_rpc_result", result);
                Ok(out)
            },
        ),
        check_def(
            "the spark begin result state is {string}",
            &[("spark_begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?;
                let outcome = core_outcome(&ctx)?;
                if outcome.result.state == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected state '{}', got '{}'",
                        expected, outcome.result.state
                    ))
                }
            },
        ),
        check_def(
            "the spark begin result track_path is {string}",
            &[("spark_begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected track_path")?;
                let outcome = core_outcome(&ctx)?;
                if outcome.result.track_path == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected track_path '{}', got '{}'",
                        expected, outcome.result.track_path
                    ))
                }
            },
        ),
        check_def(
            "the spark begin context contains {string}",
            &[("spark_begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected context text")?;
                let outcome = core_outcome(&ctx)?;
                if outcome.result.context_text.contains(needle) {
                    Ok(())
                } else {
                    Err(format!(
                        "Context did not contain '{}': {}",
                        needle, outcome.result.context_text
                    ))
                }
            },
        ),
        check_def(
            "the spark begin RPC response state is {string}",
            &[("spark_begin_rpc_result", "SparkBeginRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?;
                match ctx
                    .get::<SparkBeginRpcResult>("spark_begin_rpc_result")
                    .ok_or("No spark_begin_rpc_result")?
                {
                    SparkBeginRpcResult::Success(resp) if resp.state == expected => Ok(()),
                    SparkBeginRpcResult::Success(resp) => Err(format!(
                        "Expected state '{}', got '{}'",
                        expected, resp.state
                    )),
                    SparkBeginRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the spark begin RPC response track_path is {string}",
            &[("spark_begin_rpc_result", "SparkBeginRpcResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected track_path")?;
                match ctx
                    .get::<SparkBeginRpcResult>("spark_begin_rpc_result")
                    .ok_or("No spark_begin_rpc_result")?
                {
                    SparkBeginRpcResult::Success(resp) if resp.track_path == expected => Ok(()),
                    SparkBeginRpcResult::Success(resp) => Err(format!(
                        "Expected track_path '{}', got '{}'",
                        expected, resp.track_path
                    )),
                    SparkBeginRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the spark source contains {string}",
            &[("spark_hearth", "PathBuf")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected text")?;
                assert_hearth_file_contains(&ctx, "spark_hearth", "sparks/sparks.md", needle)
            },
        ),
        check_def(
            "the spark projection contains {string}",
            &[("spark_hearth", "PathBuf")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected text")?;
                assert_hearth_file_contains(&ctx, "spark_hearth", "projections/sparks.md", needle)
            },
        ),
        check_def(
            "the hearth spark source contains {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected text")?;
                assert_hearth_file_contains(&ctx, "hearth_path", "sparks/sparks.md", needle)
            },
        ),
        check_def(
            "the hearth spark projection contains {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected text")?;
                assert_hearth_file_contains(&ctx, "hearth_path", "projections/sparks.md", needle)
            },
        ),
        check_def(
            "no spark artifact status.yaml exists",
            &[],
            |ctx, _params| {
                let hearth = ctx
                    .get::<PathBuf>("spark_hearth")
                    .or_else(|| ctx.get::<PathBuf>("hearth_path"))
                    .ok_or("No spark hearth path")?;
                let sparks_dir = hearth.join("sparks");
                if !sparks_dir.exists() {
                    return Ok(());
                }
                let top_level_status = sparks_dir.join("status.yaml");
                if top_level_status.exists() {
                    return Err(format!(
                        "Expected no spark status.yaml at {}",
                        top_level_status.display()
                    ));
                }
                let mut offenders = Vec::new();
                for entry in std::fs::read_dir(&sparks_dir)
                    .map_err(|e| format!("Failed to read sparks dir: {}", e))?
                {
                    let entry = entry.map_err(|e| e.to_string())?;
                    let status = entry.path().join("status.yaml");
                    if status.exists() {
                        offenders.push(status.display().to_string());
                    }
                }
                if offenders.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected no spark artifact status.yaml, found {:?}",
                        offenders
                    ))
                }
            },
        ),
    ]
}

fn spark_begin_request(body: String) -> BeginRequest {
    BeginRequest {
        ctx: RequestContext::default_safe(),
        artifact_type: "spark".to_string(),
        track_name: body,
        actor_name: "Spark-Core-Test-000000".to_string(),
        actor_type: "agent".to_string(),
        actor_model: "test".to_string(),
        actor_provider: "test".to_string(),
        session_role: "creator".to_string(),
        ..Default::default()
    }
}

fn route_projection_only_events(hearth: &Path, events: &[Event]) -> Result<(), String> {
    for event in events {
        match event {
            Event::ProjectionOnlySnapshot {
                artifact_path,
                event_type,
                body,
                actor,
            } => {
                let at = unix_timestamp_string();
                append_spark_source_event(hearth, body, actor, &at)?;
                let snapshot = FileSystemSnapshotAdapter::new(hearth.to_path_buf());
                let actor_write = FileSystemActorWriteAdapter::new(hearth.to_path_buf());
                SnapshotCommandHandler::execute(
                    &snapshot,
                    &actor_write,
                    DomainSnapshotRequest {
                        artifact_path: artifact_path.clone(),
                        projection_only: true,
                        event_type: event_type.clone(),
                        at,
                        ..Default::default()
                    },
                )
                .map_err(snapshot_error)?;
            }
            other => return Err(format!("Unexpected begin event for spark: {:?}", other)),
        }
    }
    Ok(())
}

fn append_spark_source_event(
    hearth: &Path,
    body: &str,
    actor: &ActorIdentity,
    at: &str,
) -> Result<(), String> {
    let sparks_dir = hearth.join("sparks");
    std::fs::create_dir_all(&sparks_dir)
        .map_err(|e| format!("Failed to create sparks directory: {}", e))?;
    let path = sparks_dir.join("sparks.md");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let spark_body = body.trim();
    let heading = spark_body
        .lines()
        .next()
        .unwrap_or("untitled spark")
        .trim()
        .replace(['\r', '\n'], " ");
    let id = spark_id(spark_body, actor, at);
    let entry = format!(
        "## spark: {}\nid: {}\nat: {}\nactor: {}\n\n{}\n",
        heading, id, at, actor.name, spark_body
    );
    let combined = if existing.trim().is_empty() {
        entry
    } else if existing.ends_with("\n\n") {
        format!("{}{}", existing, entry)
    } else if existing.ends_with('\n') {
        format!("{}\n{}", existing, entry)
    } else {
        format!("{}\n\n{}", existing, entry)
    };
    anvil_core_hearth::atomic_write::atomic_write(&path, combined.as_bytes())
        .map_err(|e| format!("Failed to append spark event: {}", e))
}

fn spark_id(body: &str, actor: &ActorIdentity, at: &str) -> String {
    let mut hasher = DefaultHasher::new();
    body.hash(&mut hasher);
    actor.name.hash(&mut hasher);
    at.hash(&mut hasher);
    let now = unix_nanos();
    now.hash(&mut hasher);
    format!("spark-{:016x}", hasher.finish())
}

fn seed_spark_hearth(hearth: &Path) -> Result<(), String> {
    std::fs::create_dir_all(hearth).map_err(|e| format!("Failed to create hearth: {}", e))?;
    let projection_dir = hearth.join("projections");
    std::fs::create_dir_all(&projection_dir)
        .map_err(|e| format!("Failed to create projections dir: {}", e))?;
    std::fs::write(
        projection_dir.join("sparks.md"),
        "---\nincremental_count: 0\nbase_snapshot: 2026-06-14T00:00:00Z\nlast_updated: 2026-06-14T00:00:00Z\nafter_event: \"\"\n---\n\n# Anvil - Sparks\n\nUntriaged sparks: 0\nAnnotations: 0\n",
    )
    .map_err(|e| format!("Failed to write projections/sparks.md: {}", e))?;
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("Expected workspace root")?
        .join("playbooks")
        .join("spark_lifecycle");
    let dst = hearth.join("playbooks").join("spark_lifecycle");
    copy_dir_recursive(&src, &dst)
        .map_err(|e| format!("Failed to copy spark_lifecycle playbook: {}", e))?;
    Ok(())
}

fn unix_timestamp_string() -> String {
    unix_nanos().to_string()
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dst_path)?;
        } else {
            std::fs::copy(entry.path(), dst_path)?;
        }
    }
    Ok(())
}

fn core_outcome(ctx: &Context) -> Result<&BeginOutcome, String> {
    ctx.get::<Result<BeginOutcome, BeginError>>("spark_begin_outcome")
        .ok_or_else(|| "No spark_begin_outcome".to_string())?
        .as_ref()
        .map_err(|e| format!("Expected begin success, got error: {}", e))
}

fn assert_hearth_file_contains(
    ctx: &Context,
    hearth_key: &str,
    relative_path: &str,
    needle: &str,
) -> Result<(), String> {
    let hearth = ctx
        .get::<PathBuf>(hearth_key)
        .ok_or_else(|| format!("No {}", hearth_key))?;
    let path = hearth.join(relative_path);
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    if content.contains(needle) {
        Ok(())
    } else {
        Err(format!(
            "{} did not contain '{}':\n{}",
            relative_path, needle, content
        ))
    }
}

fn snapshot_error(error: SnapshotError) -> String {
    format!("Snapshot projection-only route failed: {}", error)
}

fn grpc_code_name(code: tonic::Code) -> String {
    match code {
        tonic::Code::Ok => "OK",
        tonic::Code::Cancelled => "CANCELLED",
        tonic::Code::Unknown => "UNKNOWN",
        tonic::Code::InvalidArgument => "INVALID_ARGUMENT",
        tonic::Code::DeadlineExceeded => "DEADLINE_EXCEEDED",
        tonic::Code::NotFound => "NOT_FOUND",
        tonic::Code::AlreadyExists => "ALREADY_EXISTS",
        tonic::Code::PermissionDenied => "PERMISSION_DENIED",
        tonic::Code::ResourceExhausted => "RESOURCE_EXHAUSTED",
        tonic::Code::FailedPrecondition => "FAILED_PRECONDITION",
        tonic::Code::Aborted => "ABORTED",
        tonic::Code::OutOfRange => "OUT_OF_RANGE",
        tonic::Code::Unimplemented => "UNIMPLEMENTED",
        tonic::Code::Internal => "INTERNAL",
        tonic::Code::Unavailable => "UNAVAILABLE",
        tonic::Code::DataLoss => "DATA_LOSS",
        tonic::Code::Unauthenticated => "UNAUTHENTICATED",
    }
    .to_string()
}
