//! Step definitions for `command_surface_seam.feature` — the CQRS command seam.
//!
//! These steps drive the REAL engine over its REAL gRPC wire. Nothing here
//! stands in for the engine, and nothing constructs a seam record itself: every
//! assertion reads the engine's own stderr, which is the channel a person
//! reconstructing a change would actually read.
//!
//! They deliberately do NOT go through `anvil_test_support::surfaced` — that
//! helper is what makes every OTHER scenario in the suite name itself, and a
//! feature about what happens when a surface is absent or wrong has to be able
//! to build a request the helper would never produce.

use anvil_test_support::engine::{BeginRpcResult, EngineProcess};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{async_step_def, check_def, StepDef};
use std::path::PathBuf;

/// Outcome of one raw gRPC command turn: the refusal message, or empty on ok.
const REFUSALS_KEY: &str = "command_seam_refusals";
/// How many raw gRPC calls this scenario made. Asserted alongside the refusals
/// so "every call was refused" can never pass by making no calls at all.
const CALLS_KEY: &str = "command_seam_calls";

type Refusals = Vec<String>;
type CallCount = usize;

const PARENT_ID: &str = "20260411T2021_anvil_workflow_engine";

async fn connect(
    port: u16,
) -> Result<
    anvil_engine::proto::anvil_service_client::AnvilServiceClient<tonic::transport::Channel>,
    String,
> {
    anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(format!(
        "http://127.0.0.1:{}",
        port
    ))
    .await
    .map_err(|e| format!("connect to engine: {e}"))
}

/// Build a request carrying the given surface, or none at all when `surface`
/// is `None`. This is the whole point of the module: the harness's own helper
/// always names a surface, so an absent-surface request has to be built here.
fn request_with_surface<T>(message: T, surface: Option<&str>) -> tonic::Request<T> {
    let mut request = tonic::Request::new(message);
    if let Some(surface) = surface {
        request.metadata_mut().insert(
            anvil_engine::command_seam::SURFACE_METADATA_KEY,
            surface
                .parse::<tonic::metadata::MetadataValue<tonic::metadata::Ascii>>()
                .expect("surface label is ascii"),
        );
    }
    request
}

fn begin_request(name: &str) -> anvil_engine::proto::BeginRequest {
    anvil_engine::proto::BeginRequest {
        artifact_type: "track".to_string(),
        parent_id: PARENT_ID.to_string(),
        track_name: name.to_string(),
        approver: "Seam-Approver".to_string(),
        actor_name: "Command-Seam-000001".to_string(),
        actor_type: "agent".to_string(),
        actor_model: "brine".to_string(),
        actor_provider: "test".to_string(),
        session_role: "creator".to_string(),
        ..Default::default()
    }
}

/// Record one turn's result: the refusal message, or the empty string on ok.
fn push_outcome(ctx: &mut Context, out: &mut Context, message: String) {
    let mut refusals = ctx.get::<Refusals>(REFUSALS_KEY).cloned().unwrap_or_default();
    let calls = ctx.get::<CallCount>(CALLS_KEY).copied().unwrap_or(0) + 1;
    refusals.push(message);
    out.set::<Refusals>(REFUSALS_KEY, refusals);
    out.set::<CallCount>(CALLS_KEY, calls);
}

/// Send one of the four state-changing commands, with or without a surface.
async fn send_command(
    port: u16,
    command: &str,
    surface: Option<&str>,
) -> Result<String, String> {
    let mut client = connect(port).await?;
    let status = match command {
        "begin" => client
            .begin(request_with_surface(
                begin_request("seam gRPC track"),
                surface,
            ))
            .await
            .err(),
        "snapshot" => client
            .snapshot(request_with_surface(
                anvil_engine::proto::SnapshotRequest::default(),
                surface,
            ))
            .await
            .err(),
        "complete" => client
            .complete(request_with_surface(
                anvil_engine::proto::CompleteRequest::default(),
                surface,
            ))
            .await
            .err(),
        "amend" => client
            .amend(request_with_surface(
                anvil_engine::proto::AmendRequest::default(),
                surface,
            ))
            .await
            .err(),
        other => return Err(format!("Unknown command: {other}")),
    };
    Ok(status.map(|s| s.message().to_string()).unwrap_or_default())
}

pub fn steps() -> Vec<StepDef> {
    vec![
        async_step_def(
            "a begin is sent over gRPC naming the surface {string}",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                (REFUSALS_KEY, "Refusals"),
                (CALLS_KEY, "CallCount"),
            ],
            |mut ctx, params| async move {
                let surface = params.get_string(0).ok_or("Expected a surface")?.to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let message = send_command(engine.port, "begin", Some(&surface)).await?;
                let mut out = Context::new();
                push_outcome(&mut ctx, &mut out, message);
                if let Some(hearth) = ctx.get::<PathBuf>("hearth_path").cloned() {
                    out.set::<PathBuf>("hearth_path", hearth);
                }
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        async_step_def(
            "a begin is sent over gRPC naming no surface",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                (REFUSALS_KEY, "Refusals"),
                (CALLS_KEY, "CallCount"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let message = send_command(engine.port, "begin", None).await?;
                let mut out = Context::new();
                push_outcome(&mut ctx, &mut out, message);
                if let Some(hearth) = ctx.get::<PathBuf>("hearth_path").cloned() {
                    out.set::<PathBuf>("hearth_path", hearth);
                }
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        // The same surfaceless send, in the MIDDLE of an open run. It exists
        // separately because a brine Map step retains only the keys it declares:
        // interrupting a run and then asserting what happened to that run needs
        // the run threaded through the interruption. Without it, the only
        // expressible form of "a refused change wrote nothing" is one where
        // there was no run to write about, which proves nothing.
        async_step_def(
            "a complete that names no surface is sent for the open run",
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("begin_result", "BeginRpcResult"),
            ],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("begin_result", "BeginRpcResult"),
                (REFUSALS_KEY, "Refusals"),
                (CALLS_KEY, "CallCount"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let begin = ctx
                    .take::<BeginRpcResult>("begin_result")
                    .ok_or("No begin_result — no run is open")?;
                let message = send_command(engine.port, "complete", None).await?;
                let mut out = Context::new();
                push_outcome(&mut ctx, &mut out, message);
                if let Some(hearth) = ctx.get::<PathBuf>("hearth_path").cloned() {
                    out.set::<PathBuf>("hearth_path", hearth);
                }
                out.set("begin_result", begin);
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        async_step_def(
            "a {string} is sent over gRPC naming no surface",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                (REFUSALS_KEY, "Refusals"),
                (CALLS_KEY, "CallCount"),
            ],
            |mut ctx, params| async move {
                let command = params.get_string(0).ok_or("Expected a command")?.to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let message = send_command(engine.port, &command, None).await?;
                let mut out = Context::new();
                push_outcome(&mut ctx, &mut out, message);
                if let Some(hearth) = ctx.get::<PathBuf>("hearth_path").cloned() {
                    out.set::<PathBuf>("hearth_path", hearth);
                }
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        // `an "amend" …` — Gherkin's article is part of the sentence, so the
        // same step is registered under both articles rather than forcing the
        // feature to say "a amend".
        async_step_def(
            "an {string} is sent over gRPC naming no surface",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                (REFUSALS_KEY, "Refusals"),
                (CALLS_KEY, "CallCount"),
            ],
            |mut ctx, params| async move {
                let command = params.get_string(0).ok_or("Expected a command")?.to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let message = send_command(engine.port, &command, None).await?;
                let mut out = Context::new();
                push_outcome(&mut ctx, &mut out, message);
                if let Some(hearth) = ctx.get::<PathBuf>("hearth_path").cloned() {
                    out.set::<PathBuf>("hearth_path", hearth);
                }
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        check_def(
            "the gRPC call is refused with {string}",
            &[(REFUSALS_KEY, "Refusals")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected a refusal code")?;
                let refusals = ctx
                    .get::<Refusals>(REFUSALS_KEY)
                    .ok_or("No gRPC call was made")?;
                let last = refusals
                    .last()
                    .ok_or("No gRPC call was made — nothing could have been refused")?;
                if last == expected {
                    Ok(())
                } else if last.is_empty() {
                    Err(format!(
                        "The call was SERVED, not refused. Expected refusal {:?}.",
                        expected
                    ))
                } else {
                    Err(format!(
                        "Refused with {:?}, expected {:?}",
                        last, expected
                    ))
                }
            },
        ),
        // Population guard: the count of calls is asserted alongside the
        // verdict, because "every call was refused" is vacuously true of a
        // scenario that made no calls at all.
        check_def(
            "{int} gRPC calls were made and every one was refused with {string}",
            &[(REFUSALS_KEY, "Refusals"), (CALLS_KEY, "CallCount")],
            |ctx, params| {
                let expected_calls = params.get_int(0).ok_or("Expected a call count")? as usize;
                let expected = params.get_string(1).ok_or("Expected a refusal code")?;
                let refusals = ctx
                    .get::<Refusals>(REFUSALS_KEY)
                    .ok_or("No gRPC call was made")?;
                let calls = ctx.get::<CallCount>(CALLS_KEY).copied().unwrap_or(0);
                if calls != expected_calls {
                    return Err(format!(
                        "Declared {} gRPC calls, executed {}",
                        expected_calls, calls
                    ));
                }
                if refusals.len() != expected_calls {
                    return Err(format!(
                        "Recorded {} outcomes for {} calls",
                        refusals.len(),
                        expected_calls
                    ));
                }
                let served: Vec<usize> = refusals
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| m.as_str() != expected)
                    .map(|(i, _)| i)
                    .collect();
                if served.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "{} of {} calls were not refused with {:?}: {:?}",
                        served.len(),
                        calls,
                        expected,
                        refusals
                    ))
                }
            },
        ),
        // The pair is only worth writing if the two halves can be joined. This
        // reads BOTH records off the engine's own stderr and requires the
        // issued id to appear on a settled record for the same command — a
        // check that only counted two records would pass on two unrelated ones.
        check_def(
            "the issued and settled command seam records for {string} carry the same command id",
            &[("engine_process", "EngineProcess")],
            |ctx, params| {
                let command = params.get_string(0).ok_or("Expected a command")?;
                let engine = ctx
                    .get::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                loop {
                    let lines = engine.stderr_lines();
                    let mut issued: Vec<u64> = Vec::new();
                    let mut settled: Vec<u64> = Vec::new();
                    for line in &lines {
                        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
                            continue;
                        };
                        let Some(obj) = value.as_object() else { continue };
                        if obj.get("seam").and_then(|v| v.as_str())
                            != Some(anvil_engine::command_seam::COMMAND_SEAM)
                        {
                            continue;
                        }
                        if obj.get("command").and_then(|v| v.as_str()) != Some(command) {
                            continue;
                        }
                        let Some(id) = obj.get("command_id").and_then(|v| v.as_u64()) else {
                            continue;
                        };
                        match obj.get("phase").and_then(|v| v.as_str()) {
                            Some("issued") => issued.push(id),
                            Some("settled") => settled.push(id),
                            _ => {}
                        }
                    }
                    let paired: Vec<u64> = issued
                        .iter()
                        .copied()
                        .filter(|id| *id != 0 && settled.contains(id))
                        .collect();
                    if !paired.is_empty() {
                        return Ok(());
                    }
                    if std::time::Instant::now() >= deadline {
                        return Err(format!(
                            "No issued id for {:?} was matched by a settled record. \
                             issued ids: {:?}; settled ids: {:?}",
                            command, issued, settled
                        ));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
            },
        ),
        // The refusal is only meaningful if the change really did not happen.
        // Asserting the log alone would pass on an engine that recorded a
        // refusal and then went ahead anyway.
        check_def(
            "the hearth gained no new track",
            &[("hearth_path", "PathBuf")],
            |ctx, _params| {
                let hearth = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let tracks_dir = hearth.join("tracks");
                let count = match std::fs::read_dir(&tracks_dir) {
                    Ok(entries) => entries.flatten().count(),
                    // No tracks directory at all is the strongest possible
                    // form of "no track was created".
                    Err(_) => 0,
                };
                if count == 0 {
                    Ok(())
                } else {
                    Err(format!(
                        "A refused command still created {} track(s) under {}",
                        count,
                        tracks_dir.display()
                    ))
                }
            },
        ),
    ]
}
