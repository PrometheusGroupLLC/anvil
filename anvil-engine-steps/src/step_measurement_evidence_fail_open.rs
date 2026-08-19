//! Real-engine fail-open coverage for the P2b evidence measurement writer.
//!
//! The blocked-sink cases exercise the production filesystem adapter. The
//! parking case declares a subprocess-only coordination seam: when
//! `ANVIL_TEST_STEP_MEASUREMENT_PARK_DIR` is set, the writer creates `parked`
//! before its first durable append, waits for `release`, and creates `drained`
//! after all accepted rows have been delivered. The seam lets the scenario
//! prove RPC/state progress independently of a parked durable writer without
//! relying on sleeps or an in-process mock service.

use anvil_test_support::engine::{spawn_engine_with_test_env, EngineProcess};
use anvil_test_support::{carry_retained_temp_dir, retained_temp_dir, RetainedTempDir};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{async_step_def, check_def, step_def, StepDef};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const KIND: &str = "evidence_fail_open_probe";
const PLAYBOOK_ID: &str = "p2b_evidence_fail_open_probe";
const ARTIFACT_PATH: &str = "evidence_fail_open_runs/p2b-seeded-run";
const ACTOR: &str = "Evidence-P2b-100000";
const PARK_ENV: &str = "ANVIL_TEST_STEP_MEASUREMENT_PARK_DIR";
const PARKED_MARKER: &str = "parked";
const RELEASE_MARKER: &str = "release";
const DRAINED_MARKER: &str = "drained";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Leg {
    Begin,
    Snapshot,
    Complete,
    /// T-ACT-2 P4-S7 nit: routes the `complete` leg through the real
    /// `anvil-hooks complete --claimed-evidence ...` CLI affordance instead of
    /// an in-process tonic client call, proving fail-open holds when a claim
    /// is actually presented through the shipped affordance, not merely
    /// constructed directly against the RPC.
    CompleteCli,
}

#[derive(Debug, Clone, Default)]
struct LifecycleOutcome {
    returned: bool,
    state: String,
    artifact_path: String,
    error: String,
}

#[derive(Debug, Clone, Default)]
struct ParkingProbe {
    begin: LifecycleOutcome,
    snapshot: LifecycleOutcome,
    complete: LifecycleOutcome,
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a P2b obligated playbook hearth prepared for {string}",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                (
                    "hearth_path_handle",
                    "Arc<Mutex<Option<TempDir>>>",
                ),
            ],
            |_ctx, params| {
                let leg = parse_leg(params.get_string(0).ok_or("Expected lifecycle leg")?)?;
                let (handle, hearth) = retained_temp_dir("anvil-p2b-evidence-fail-open-")?;
                seed_hearth(&hearth)?;
                if leg != Leg::Begin {
                    seed_artifact(&hearth)?;
                }
                anvil_test_support::harness::ensure_binary("anvil-hooks");
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                out.set::<RetainedTempDir>("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "the evidence step-measurement sink is blocked by a directory",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                (
                    "hearth_path_handle",
                    "Arc<Mutex<Option<TempDir>>>",
                ),
            ],
            |ctx, _params| {
                let hearth = hearth(&ctx)?;
                let sink = hearth.join("step-measurement.jsonl");
                std::fs::create_dir(&sink)
                    .map_err(|error| format!("block {} with directory: {}", sink.display(), error))?;
                carry_hearth(&ctx, hearth)
            },
        ),
        async_step_def(
            "the P2b {string} lifecycle RPC is called",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                (
                    "hearth_path_handle",
                    "Arc<Mutex<Option<TempDir>>>",
                ),
                ("p2b_lifecycle_outcome", "LifecycleOutcome"),
            ],
            |mut ctx, params| async move {
                let leg = parse_leg(params.get_string(0).ok_or("Expected lifecycle leg")?)?;
                let engine = take_engine(&mut ctx)?;
                let hearth = hearth(&ctx)?;
                let outcome = call_leg(engine.port, &hearth, leg).await;
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                out.set("p2b_lifecycle_outcome", outcome);
                Ok(out)
            },
        ),
        check_def(
            "the P2b lifecycle RPC succeeds in state {string}",
            &[("p2b_lifecycle_outcome", "LifecycleOutcome")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected response state")?;
                assert_outcome(outcome(&ctx)?, expected)
            },
        ),
        check_def(
            "state {string} is visible in the transitioned artifact",
            &[
                ("hearth_path", "PathBuf"),
                ("p2b_lifecycle_outcome", "LifecycleOutcome"),
            ],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected persisted state")?;
                let result = outcome(&ctx)?;
                assert_state(&hearth(&ctx)?, &result.artifact_path, expected)
            },
        ),
        step_def(
            "the engine is started with its evidence step-measurement writer parked",
            &[("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                (
                    "hearth_path_handle",
                    "Arc<Mutex<Option<TempDir>>>",
                ),
                ("p2b_writer_control", "PathBuf"),
                ("p2b_parking_probe", "ParkingProbe"),
            ],
            |ctx, _params| {
                let hearth = hearth(&ctx)?;
                let control = hearth.join("__p2b_step_measurement_writer_control__");
                std::fs::create_dir_all(&control).map_err(|error| {
                    format!("create writer control {}: {}", control.display(), error)
                })?;
                let engine = spawn_engine_with_test_env(&hearth, PARK_ENV, &control)?;
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                out.set("p2b_writer_control", control);
                out.set("p2b_parking_probe", ParkingProbe::default());
                Ok(out)
            },
        ),
        async_step_def(
            "the parking probe begin RPC is called before writer release",
            &parking_inputs(),
            &parking_outputs(),
            |mut ctx, _params| async move {
                let engine = take_engine(&mut ctx)?;
                let hearth = hearth(&ctx)?;
                let control = control(&ctx)?;
                let mut probe = take_probe(&mut ctx)?;
                if control.join(RELEASE_MARKER).exists() {
                    return Err("Parking release marker existed before begin RPC".to_string());
                }
                probe.begin = call_leg_with_timeout(engine.port, &hearth, Leg::Begin).await;
                restore_parking(&ctx, engine, hearth, control, probe)
            },
        ),
        check_def(
            "the begin RPC returns with state {string} visible while the evidence writer is parked",
            &[
                ("hearth_path", "PathBuf"),
                ("p2b_writer_control", "PathBuf"),
                ("p2b_parking_probe", "ParkingProbe"),
            ],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected begin state")?;
                let probe = probe(&ctx)?;
                assert_parked_before_release(&hearth(&ctx)?, &control(&ctx)?)?;
                assert_outcome(&probe.begin, expected)?;
                assert_state(&hearth(&ctx)?, &probe.begin.artifact_path, expected)
            },
        ),
        async_step_def(
            "the parking probe snapshot RPC is called before writer release",
            &parking_inputs(),
            &parking_outputs(),
            |mut ctx, _params| async move {
                let engine = take_engine(&mut ctx)?;
                let hearth = hearth(&ctx)?;
                let control = control(&ctx)?;
                let mut probe = take_probe(&mut ctx)?;
                if control.join(RELEASE_MARKER).exists() {
                    return Err("Parking release marker existed before snapshot RPC".to_string());
                }
                probe.snapshot = call_leg_with_timeout(engine.port, &hearth, Leg::Snapshot).await;
                restore_parking(&ctx, engine, hearth, control, probe)
            },
        ),
        check_def(
            "the snapshot RPC returns with state {string} visible while the evidence writer is parked",
            &[
                ("hearth_path", "PathBuf"),
                ("p2b_writer_control", "PathBuf"),
                ("p2b_parking_probe", "ParkingProbe"),
            ],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected snapshot state")?;
                let probe = probe(&ctx)?;
                assert_parked_before_release(&hearth(&ctx)?, &control(&ctx)?)?;
                assert_outcome(&probe.snapshot, expected)?;
                assert_state(&hearth(&ctx)?, &probe.snapshot.artifact_path, expected)
            },
        ),
        async_step_def(
            "the parking probe complete RPC is called before writer release",
            &parking_inputs(),
            &parking_outputs(),
            |mut ctx, _params| async move {
                let engine = take_engine(&mut ctx)?;
                let hearth = hearth(&ctx)?;
                let control = control(&ctx)?;
                let mut probe = take_probe(&mut ctx)?;
                if !probe.begin.returned || probe.begin.artifact_path.is_empty() {
                    return Err(format!(
                        "Cannot complete parking probe after begin outcome: {:?}",
                        probe.begin
                    ));
                }
                if control.join(RELEASE_MARKER).exists() {
                    return Err("Parking release marker existed before complete RPC".to_string());
                }
                probe.complete =
                    call_complete_with_timeout(engine.port, &probe.begin.artifact_path).await;
                restore_parking(&ctx, engine, hearth, control, probe)
            },
        ),
        check_def(
            "the complete RPC returns with state {string} visible while the evidence writer is parked",
            &[
                ("hearth_path", "PathBuf"),
                ("p2b_writer_control", "PathBuf"),
                ("p2b_parking_probe", "ParkingProbe"),
            ],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected complete state")?;
                let probe = probe(&ctx)?;
                assert_parked_before_release(&hearth(&ctx)?, &control(&ctx)?)?;
                assert_outcome(&probe.complete, expected)?;
                assert_state(&hearth(&ctx)?, &probe.complete.artifact_path, expected)
            },
        ),
        step_def(
            "the parked evidence writer is released",
            &parking_inputs(),
            &parking_outputs(),
            |mut ctx, _params| {
                let engine = take_engine(&mut ctx)?;
                let hearth = hearth(&ctx)?;
                let control = control(&ctx)?;
                let probe = take_probe(&mut ctx)?;
                std::fs::write(control.join(RELEASE_MARKER), b"release\n")
                    .map_err(|error| format!("release parked evidence writer: {}", error))?;
                restore_parking(&ctx, engine, hearth, control, probe)
            },
        ),
        check_def(
            "its delivered evidence rows drain in lifecycle order exactly once",
            &[
                ("hearth_path", "PathBuf"),
                ("p2b_writer_control", "PathBuf"),
                ("p2b_parking_probe", "ParkingProbe"),
            ],
            |ctx, _params| assert_fifo_drain(&hearth(&ctx)?, &control(&ctx)?),
        ),
        check_def(
            "its delivered snapshot evidence row drains exactly once",
            &[
                ("hearth_path", "PathBuf"),
                ("p2b_writer_control", "PathBuf"),
                ("p2b_parking_probe", "ParkingProbe"),
            ],
            |ctx, _params| assert_snapshot_drain(&hearth(&ctx)?, &control(&ctx)?),
        ),
    ]
}

fn parking_inputs() -> Vec<(&'static str, &'static str)> {
    vec![
        ("engine_process", "EngineProcess"),
        ("hearth_path", "PathBuf"),
        ("p2b_writer_control", "PathBuf"),
        ("p2b_parking_probe", "ParkingProbe"),
    ]
}

fn parking_outputs() -> Vec<(&'static str, &'static str)> {
    let mut outputs = parking_inputs();
    outputs.push(("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"));
    outputs
}

fn parse_leg(value: &str) -> Result<Leg, String> {
    match value {
        "begin" => Ok(Leg::Begin),
        "snapshot" => Ok(Leg::Snapshot),
        "complete" => Ok(Leg::Complete),
        "complete_cli" => Ok(Leg::CompleteCli),
        other => Err(format!("Unknown P2b lifecycle leg '{}'", other)),
    }
}

async fn call_leg(port: u16, hearth: &Path, leg: Leg) -> LifecycleOutcome {
    match leg {
        Leg::Begin => call_begin(port).await,
        Leg::Snapshot => call_snapshot(port).await,
        Leg::Complete => call_complete(port, ARTIFACT_PATH).await,
        Leg::CompleteCli => call_complete_via_cli(hearth, port, ARTIFACT_PATH),
    }
}

async fn call_leg_with_timeout(port: u16, hearth: &Path, leg: Leg) -> LifecycleOutcome {
    match tokio::time::timeout(Duration::from_secs(2), call_leg(port, hearth, leg)).await {
        Ok(outcome) => outcome,
        Err(_) => LifecycleOutcome {
            error: format!(
                "{:?} RPC did not return within 2s while the evidence writer was parked",
                leg
            ),
            ..Default::default()
        },
    }
}

async fn call_complete_with_timeout(port: u16, artifact_path: &str) -> LifecycleOutcome {
    match tokio::time::timeout(Duration::from_secs(2), call_complete(port, artifact_path)).await {
        Ok(outcome) => outcome,
        Err(_) => LifecycleOutcome {
            artifact_path: artifact_path.to_string(),
            error: "Complete RPC did not return within 2s while the evidence writer was parked"
                .to_string(),
            ..Default::default()
        },
    }
}

async fn call_begin(port: u16) -> LifecycleOutcome {
    let mut client = match connect(port).await {
        Ok(client) => client,
        Err(error) => return failed(error, ""),
    };
    let request = anvil_engine::proto::BeginRequest {
        artifact_type: KIND.to_string(),
        track_name: "p2b fail-open begin".to_string(),
        actor_name: ACTOR.to_string(),
        actor_type: "agent".to_string(),
        actor_model: "brine".to_string(),
        actor_provider: "test".to_string(),
        session_role: "creator".to_string(),
        ctx_org: "Foundation".to_string(),
        ctx_role: "read".to_string(),
        ctx_clearance: "internal".to_string(),
        claimed_evidence: vec![claim()],
        ..Default::default()
    };
    match client.begin(anvil_test_support::surfaced(request)).await {
        Ok(response) => {
            let response = response.into_inner();
            LifecycleOutcome {
                returned: true,
                state: response.state,
                artifact_path: response.track_path,
                error: String::new(),
            }
        }
        Err(status) => failed(
            format!(
                "Begin RPC failed ({:?}): {}",
                status.code(),
                status.message()
            ),
            "",
        ),
    }
}

async fn call_snapshot(port: u16) -> LifecycleOutcome {
    let mut client = match connect(port).await {
        Ok(client) => client,
        Err(error) => return failed(error, ARTIFACT_PATH),
    };
    let request = anvil_engine::proto::SnapshotRequest {
        artifact_path: ARTIFACT_PATH.to_string(),
        to_state: "spec_review".to_string(),
        actor_name: ACTOR.to_string(),
        actor_role: "reviewer".to_string(),
        actor_type: "agent".to_string(),
        actor_model: "brine".to_string(),
        actor_provider: "test".to_string(),
        claimed_evidence: vec![claim()],
        ..Default::default()
    };
    match client.snapshot(anvil_test_support::surfaced(request)).await {
        Ok(response) => {
            if response.into_inner().success {
                LifecycleOutcome {
                    returned: true,
                    state: "spec_review".to_string(),
                    artifact_path: ARTIFACT_PATH.to_string(),
                    error: String::new(),
                }
            } else {
                failed("Snapshot RPC returned success=false", ARTIFACT_PATH)
            }
        }
        Err(status) => failed(
            format!(
                "Snapshot RPC failed ({:?}): {}",
                status.code(),
                status.message()
            ),
            ARTIFACT_PATH,
        ),
    }
}

async fn call_complete(port: u16, artifact_path: &str) -> LifecycleOutcome {
    let mut client = match connect(port).await {
        Ok(client) => client,
        Err(error) => return failed(error, artifact_path),
    };
    let request = anvil_engine::proto::CompleteRequest {
        artifact_path: artifact_path.to_string(),
        actor_name: ACTOR.to_string(),
        actor_type: "agent".to_string(),
        actor_model: "brine".to_string(),
        actor_provider: "test".to_string(),
        claimed_evidence: vec![claim()],
        ..Default::default()
    };
    match client.complete(anvil_test_support::surfaced(request)).await {
        Ok(response) => {
            let response = response.into_inner();
            LifecycleOutcome {
                returned: true,
                state: response.new_state,
                artifact_path: artifact_path.to_string(),
                error: String::new(),
            }
        }
        Err(status) => failed(
            format!(
                "Complete RPC failed ({:?}): {}",
                status.code(),
                status.message()
            ),
            artifact_path,
        ),
    }
}

/// T-ACT-2 P4-S7 nit: run the REAL `anvil-hooks complete` binary with the
/// shipped `--claimed-evidence` affordance against the blocked-sink hearth,
/// proving fail-open holds when a claim travels through the actual CLI
/// surface (not merely an in-process RPC construction like `call_complete`
/// above).
fn call_complete_via_cli(hearth: &Path, port: u16, artifact_path: &str) -> LifecycleOutcome {
    let bin = anvil_test_support::harness::binary_path("anvil-hooks");
    let output = std::process::Command::new(bin)
        .arg("complete")
        .arg("--artifact-path")
        .arg(artifact_path)
        .arg("--actor-name")
        .arg(ACTOR)
        .arg("--actor-type")
        .arg("agent")
        .arg("--actor-model")
        .arg("brine")
        .arg("--actor-provider")
        .arg("test")
        .arg("--hearth")
        .arg(hearth.to_string_lossy().to_string())
        .arg("--port")
        .arg(port.to_string())
        .arg("--claimed-evidence")
        .arg("artifact_of_consequence:opaque:p2b:cli-affordance")
        .output();
    let output = match output {
        Ok(output) => output,
        Err(error) => {
            return failed(
                format!("spawn anvil-hooks complete: {}", error),
                artifact_path,
            )
        }
    };
    if !output.status.success() {
        return failed(
            format!(
                "anvil-hooks complete exited non-zero (status {:?}): stdout={} stderr={}",
                output.status.code(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
            artifact_path,
        );
    }
    match resolved_state(hearth, artifact_path) {
        Ok(state) => LifecycleOutcome {
            returned: true,
            state,
            artifact_path: artifact_path.to_string(),
            error: String::new(),
        },
        Err(error) => failed(error, artifact_path),
    }
}

/// Read the resolved state of `artifact_path` directly off disk (mirrors
/// `assert_state` below) so `call_complete_via_cli`'s outcome carries a real
/// state without depending on the CLI's stdout text format.
fn resolved_state(hearth: &Path, artifact_path: &str) -> Result<String, String> {
    let artifact_dir = hearth.join(artifact_path);
    let status_path = artifact_dir.join("status.yaml");
    let contents = std::fs::read_to_string(&status_path)
        .map_err(|error| format!("read {}: {}", status_path.display(), error))?;
    let status: anvil_core::domain::status::FullStatusYaml = serde_yaml::from_str(&contents)
        .map_err(|error| format!("parse {}: {}", status_path.display(), error))?;
    anvil_core::domain::transition_log::resolve_state_with_events(&status, &artifact_dir)
        .map_err(|e| format!("unreadable transition evidence: {e}"))?
        .ok_or_else(|| format!("No resolvable state for {}", artifact_dir.display()))
}

fn claim() -> anvil_engine::proto::ClaimedEvidence {
    anvil_engine::proto::ClaimedEvidence {
        class: "artifact_of_consequence".to_string(),
        reference: "opaque:p2b:durable-output".to_string(),
    }
}

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
    .map_err(|error| format!("connect to P2b engine: {}", error))
}

fn failed(error: impl Into<String>, artifact_path: &str) -> LifecycleOutcome {
    LifecycleOutcome {
        artifact_path: artifact_path.to_string(),
        error: error.into(),
        ..Default::default()
    }
}

fn assert_outcome(result: &LifecycleOutcome, expected: &str) -> Result<(), String> {
    if result.returned && result.error.is_empty() && result.state == expected {
        Ok(())
    } else {
        Err(format!(
            "Expected lifecycle RPC to return in state '{}', got returned={} state='{}' error='{}'",
            expected, result.returned, result.state, result.error
        ))
    }
}

fn assert_state(hearth: &Path, artifact_path: &str, expected: &str) -> Result<(), String> {
    if artifact_path.is_empty() {
        return Err("Lifecycle result did not identify a transitioned artifact".to_string());
    }
    let artifact_dir = hearth.join(artifact_path);
    let status_path = artifact_dir.join("status.yaml");
    let contents = std::fs::read_to_string(&status_path)
        .map_err(|error| format!("read {}: {}", status_path.display(), error))?;
    let status: anvil_core::domain::status::FullStatusYaml = serde_yaml::from_str(&contents)
        .map_err(|error| format!("parse {}: {}", status_path.display(), error))?;
    let state =
        anvil_core::domain::transition_log::resolve_state_with_events(&status, &artifact_dir)
            .map_err(|e| format!("unreadable transition evidence: {e}"))?
            .ok_or_else(|| format!("No resolvable state for {}", artifact_dir.display()))?;
    if state == expected {
        Ok(())
    } else {
        Err(format!(
            "Expected persisted state '{}' at {}, got '{}'",
            expected,
            status_path.display(),
            state
        ))
    }
}

fn assert_parked_before_release(hearth: &Path, control: &Path) -> Result<(), String> {
    if control.join(RELEASE_MARKER).exists() {
        return Err("Evidence writer was released before responsiveness was asserted".to_string());
    }
    wait_for_path(&control.join(PARKED_MARKER), Duration::from_secs(2)).map_err(|_| {
        format!(
            "Evidence writer never created the '{}' marker under {}; the subprocess parking seam {} was not honored",
            PARKED_MARKER,
            control.display(),
            PARK_ENV
        )
    })?;
    let sink = hearth.join("step-measurement.jsonl");
    match std::fs::read_to_string(&sink) {
        Ok(contents) if contents.trim().is_empty() => Ok(()),
        Ok(contents) => Err(format!(
            "Evidence writer persisted rows before release at {}: {}",
            sink.display(),
            contents
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "inspect parked evidence sink {}: {}",
            sink.display(),
            error
        )),
    }
}

fn assert_fifo_drain(hearth: &Path, control: &Path) -> Result<(), String> {
    assert_drain(hearth, control, &[("", "spec"), ("spec", "spec_review")])
}

fn assert_snapshot_drain(hearth: &Path, control: &Path) -> Result<(), String> {
    assert_drain(hearth, control, &[("spec", "spec_review")])
}

fn assert_drain(hearth: &Path, control: &Path, expected: &[(&str, &str)]) -> Result<(), String> {
    wait_for_path(&control.join(DRAINED_MARKER), Duration::from_secs(2)).map_err(|_| {
        format!(
            "Evidence writer did not create '{}' after release at {}",
            DRAINED_MARKER,
            control.display()
        )
    })?;
    let sink = hearth.join("step-measurement.jsonl");
    let (contents, rows) = wait_for_rows(&sink, expected.len(), Duration::from_secs(2))?;
    for (index, ((from, to), row)) in expected.iter().zip(rows.iter()).enumerate() {
        let actual_from = row.get("from_state").and_then(Value::as_str).unwrap_or("");
        let actual_to = row.get("to_state").and_then(Value::as_str).unwrap_or("");
        let status = row
            .get("evidence_status")
            .and_then(Value::as_str)
            .unwrap_or("");
        if actual_from != *from || actual_to != *to || status != "present-as-claimed" {
            return Err(format!(
                "Delivered row {} broke FIFO/evidence contract: expected {} -> {} present-as-claimed, got {}",
                index, from, to, row
            ));
        }
    }
    assert_sink_stable(&sink, &contents, Duration::from_millis(250))
}

fn wait_for_rows(
    sink: &Path,
    expected_count: usize,
    timeout: Duration,
) -> Result<(String, Vec<Value>), String> {
    let deadline = Instant::now() + timeout;
    let mut last_observation = "sink was absent".to_string();
    while Instant::now() < deadline {
        match std::fs::read_to_string(sink) {
            Ok(contents) => {
                let lines = contents
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .collect::<Vec<_>>();
                if lines.len() > expected_count {
                    return Err(format!(
                        "Expected exactly {} delivered evidence row(s), got {}: {}",
                        expected_count,
                        lines.len(),
                        contents
                    ));
                }
                if lines.len() == expected_count {
                    match lines
                        .iter()
                        .map(|line| {
                            serde_json::from_str::<Value>(line).map_err(|error| {
                                format!("parse drained evidence row '{}': {}", line, error)
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()
                    {
                        Ok(rows) => return Ok((contents, rows)),
                        Err(error) => last_observation = error,
                    }
                } else {
                    last_observation = format!(
                        "expected {} delivered evidence row(s), observed {}",
                        expected_count,
                        lines.len()
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                last_observation = "sink was absent".to_string();
            }
            Err(error) => {
                return Err(format!("read drained sink {}: {}", sink.display(), error));
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Err(format!(
        "Evidence sink {} did not reach exactly {} parseable row(s) within {:?}: {}",
        sink.display(),
        expected_count,
        timeout,
        last_observation
    ))
}

fn assert_sink_stable(sink: &Path, expected: &str, duration: Duration) -> Result<(), String> {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
        let current = std::fs::read_to_string(sink)
            .map_err(|error| format!("re-read drained sink {}: {}", sink.display(), error))?;
        if current != expected {
            return Err(format!(
                "Evidence sink {} changed after drain; expected stable at-most-once delivery. Before: {} After: {}",
                sink.display(),
                expected,
                current
            ));
        }
    }
    Ok(())
}

fn wait_for_path(path: &Path, timeout: Duration) -> Result<(), ()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Err(())
}

fn seed_hearth(hearth: &Path) -> Result<(), String> {
    std::fs::create_dir_all(hearth.join("evidence_fail_open_runs"))
        .map_err(|error| format!("create P2b artifact root: {}", error))?;
    std::fs::write(
        hearth.join("evidence_fail_open_runs.md"),
        "# Evidence Fail-open Runs\n\n## spec\n\n## spec_review\n\n## completed\n",
    )
    .map_err(|error| format!("write P2b registry: {}", error))?;
    // The `anvil-hooks` CLI (unlike an in-process tonic call) enforces the
    // structural hearth predicate (a `tracks/` dir + `tracks.md` registry)
    // before dispatching. Carry both here — harmless to the bespoke
    // `evidence_fail_open_probe` kind machinery, which never reads them —
    // so the P4-S7 CLI leg (`complete_cli`) resolves as a valid hearth.
    std::fs::create_dir_all(hearth.join("tracks"))
        .map_err(|error| format!("create P2b tracks/ dir: {}", error))?;
    if !hearth.join("tracks.md").exists() {
        std::fs::write(hearth.join("tracks.md"), "# Tracks\n")
            .map_err(|error| format!("write P2b tracks.md: {}", error))?;
    }
    let playbook = hearth.join("playbooks").join(PLAYBOOK_ID);
    std::fs::create_dir_all(&playbook)
        .map_err(|error| format!("create P2b playbook: {}", error))?;
    std::fs::write(playbook.join("machine.yaml"), MACHINE_YAML)
        .map_err(|error| format!("write P2b machine: {}", error))?;
    Ok(())
}

fn seed_artifact(hearth: &Path) -> Result<(), String> {
    let artifact = hearth.join(ARTIFACT_PATH);
    std::fs::create_dir_all(&artifact)
        .map_err(|error| format!("create P2b seeded artifact: {}", error))?;
    std::fs::write(
        artifact.join("status.yaml"),
        format!(
            "version: 1\nkind: {kind}\nstate: spec\ntransitions:\n  - to: spec\n    at: \"2026-07-19T00:00:00Z\"\n    actor: Seed-P2b-000001\n    role: doer\n",
            kind = KIND,
        ),
    )
    .map_err(|error| format!("write P2b seeded status: {}", error))?;
    std::fs::write(artifact.join("spec.md"), "# P2b fail-open probe\n")
        .map_err(|error| format!("write P2b seeded artifact: {}", error))?;
    Ok(())
}

fn hearth(ctx: &Context) -> Result<PathBuf, String> {
    ctx.get::<PathBuf>("hearth_path")
        .cloned()
        .ok_or_else(|| "No P2b hearth_path".to_string())
}

fn control(ctx: &Context) -> Result<PathBuf, String> {
    ctx.get::<PathBuf>("p2b_writer_control")
        .cloned()
        .ok_or_else(|| "No P2b writer control path".to_string())
}

fn outcome(ctx: &Context) -> Result<&LifecycleOutcome, String> {
    ctx.get::<LifecycleOutcome>("p2b_lifecycle_outcome")
        .ok_or_else(|| "No P2b lifecycle outcome".to_string())
}

fn probe(ctx: &Context) -> Result<&ParkingProbe, String> {
    ctx.get::<ParkingProbe>("p2b_parking_probe")
        .ok_or_else(|| "No P2b parking probe".to_string())
}

fn take_probe(ctx: &mut Context) -> Result<ParkingProbe, String> {
    ctx.take::<ParkingProbe>("p2b_parking_probe")
        .ok_or_else(|| "No P2b parking probe".to_string())
}

fn take_engine(ctx: &mut Context) -> Result<EngineProcess, String> {
    ctx.take::<EngineProcess>("engine_process")
        .ok_or_else(|| "No engine_process".to_string())
}

fn carry_hearth(ctx: &Context, hearth: PathBuf) -> Result<Context, String> {
    let mut out = Context::new();
    out.set("hearth_path", hearth);
    carry_retained_temp_dir(ctx, &mut out, "hearth_path_handle");
    Ok(out)
}

fn restore_parking(
    ctx: &Context,
    engine: EngineProcess,
    hearth: PathBuf,
    control: PathBuf,
    probe: ParkingProbe,
) -> Result<Context, String> {
    let mut out = Context::new();
    out.set("engine_process", engine);
    out.set("hearth_path", hearth);
    carry_retained_temp_dir(ctx, &mut out, "hearth_path_handle");
    out.set("p2b_writer_control", control);
    out.set("p2b_parking_probe", probe);
    Ok(out)
}

const MACHINE_YAML: &str = r#"kind: evidence_fail_open_probe
directory: evidence_fail_open_runs
registry: evidence_fail_open_runs.md
description: "P2b evidence writer fail-open probe."
register: driven
required_fields: []
roles:
  - doer
  - reviewer
states:
  - name: spec
    role_filters: []
    registry_section: spec
    projection_targets: []
    is_review_gate: false
    is_terminal: false
    measurement_by_role:
      doer:
        intent: "Produce a durable P2b probe."
        expected_output: "A durable P2b probe artifact."
        evidence_obligation:
          - artifact_of_consequence
  - name: spec_review
    role_filters: []
    registry_section: spec_review
    projection_targets: []
    is_review_gate: true
    is_terminal: false
    measurement_by_role:
      reviewer:
        intent: "Review the durable P2b probe."
        expected_output: "A durable P2b review."
        evidence_obligation:
          - artifact_of_consequence
  - name: completed
    role_filters: []
    registry_section: completed
    projection_targets: []
    is_review_gate: false
    is_terminal: true
transitions:
  - from_state: spec
    to_state: spec_review
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
  - from_state: spec_review
    to_state: completed
    required_role: reviewer
    required_satisfaction:
      - satisfied
    requires_approver: false
outcome_predicate:
  terminal_state: completed
"#;
