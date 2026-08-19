//! Real-engine steps for `step_measurement_evidence.feature` (T-EEC-2 P2a).
//!
//! The fixture writes one deliberately small playbook to a temporary hearth,
//! then drives begin/snapshot/complete through tonic.  Assertions read the
//! production `step-measurement.jsonl` sink.  Sink reads are deadline-polled so
//! the same scenarios remain valid when P2b moves writes behind a dispatcher.

use anvil_test_support::engine::EngineProcess;
use anvil_test_support::{retained_temp_dir, RetainedTempDir};
use anvil_core::domain::playbook::loader::load_from_yaml;
use anvil_core::domain::playbook_version::machine_content_version;
use anvil_core_hearth::fs_snapshot_adapter::FileSystemSnapshotAdapter;
use anvil_core::ports::snapshot_port::SnapshotPort;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{async_step_def, check_def, step_def, StepDef};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

const KIND: &str = "evidence_probe";
const PLAYBOOK_ID: &str = "p2a_evidence_probe";
const RUN_ID: &str = "p2a-private-artifact-fragment";
const ARTIFACT_PATH: &str = "evidence_runs/p2a-private-artifact-fragment";
const ACTOR: &str = "Evidence-P2a-100000";
const RAW_NOTE: &str = "RAW-P2A-NOTE must never enter the evidence assessment";
const RAW_PROJECT_ROOT: &str = "/private/raw-p2a/private-p2a-workspace";
const JSON_C0_REFERENCE: &str = "opaque:c0:\u{0000}\u{0001}\u{0008}\u{000c}\u{001f}:end";
const CLAIMED_EVIDENCE_GATE_FLAG: &str = "ANVIL_ENFORCE_CLAIMED_EVIDENCE";
const EVIDENCE_KEYS: [&str; 4] = [
    "evidence_status",
    "missing_evidence_classes",
    "claimed_evidence",
    "playbook_version",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Leg {
    Begin,
    Snapshot,
    Complete,
}

#[derive(Debug, Clone)]
enum LifecycleRequest {
    Begin(anvil_engine::proto::BeginRequest),
    Snapshot(anvil_engine::proto::SnapshotRequest),
    Complete(anvil_engine::proto::CompleteRequest),
}

#[derive(Debug, Clone, Default)]
struct RpcOutcome {
    success: bool,
    error: String,
    rows: Vec<Value>,
    raw_sink: Vec<u8>,
}

#[derive(Debug, Clone)]
struct EvidenceFixture {
    leg: Leg,
    request: LifecycleRequest,
    resolved_state: String,
    resolved_role: String,
    machine_version: String,
    privacy_values_present: bool,
    baseline_hearth_files: BTreeMap<PathBuf, Vec<u8>>,
    outcome: RpcOutcome,
}

#[derive(Debug, Clone)]
struct LaneIsolationOutcome {
    lane_x_success: bool,
    lane_x_error: String,
    lane_x_unchanged: bool,
    lane_y_success: bool,
    lane_y_rows: Vec<Value>,
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a P4 claimed-evidence gate hearth with D-privacy decided",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                (
                    "hearth_path_handle",
                    "Arc<Mutex<Option<TempDir>>>",
                ),
            ],
            |_ctx, _params| {
                let (handle, hearth) = retained_temp_dir("anvil-p4-evidence-gate-")?;
                seed_hearth(&hearth)?;
                seed_decided_privacy_decision(&hearth)?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                out.set::<RetainedTempDir>("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a P2a evidence measurement hearth",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                (
                    "hearth_path_handle",
                    "Arc<Mutex<Option<TempDir>>>",
                ),
            ],
            |_ctx, _params| {
                let (handle, hearth) = retained_temp_dir("anvil-p2a-evidence-")?;
                seed_hearth(&hearth)?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                out.set::<RetainedTempDir>("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "the claimed-evidence transition gate is enabled for this request lane",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            |ctx, _params| {
                let path = hearth(&ctx)?.join("engine-flags.env");
                std::fs::write(
                    &path,
                    format!("{}=1\n", CLAIMED_EVIDENCE_GATE_FLAG),
                )
                .map_err(|error| format!("write {}: {}", path.display(), error))?;
                Ok(ctx)
            },
        ),
        step_def(
            "the claimed-evidence transition gate is not configured for this request lane",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            |ctx, _params| {
                let path = hearth(&ctx)?.join("engine-flags.env");
                std::fs::write(&path, "# claimed-evidence gate intentionally absent\n")
                    .map_err(|error| format!("write {}: {}", path.display(), error))?;
                Ok(ctx)
            },
        ),
        step_def(
            "the D-privacy decision is unresolved for this request lane",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            |ctx, _params| {
                seed_privacy_decision(&hearth(&ctx)?, "tension")?;
                Ok(ctx)
            },
        ),
        step_def(
            "request lane X opts into claimed-evidence enforcement while lane Y remains default off",
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_x_path", "PathBuf"),
                ("hearth_y_path", "PathBuf"),
            ],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_x_path", "PathBuf"),
                ("hearth_y_path", "PathBuf"),
                ("p4_lane_x_baseline", "HearthFileMap"),
            ],
            |mut ctx, _params| {
                let lane_x = ctx
                    .get::<PathBuf>("hearth_x_path")
                    .ok_or("Missing request lane X")?
                    .clone();
                let lane_y = ctx
                    .get::<PathBuf>("hearth_y_path")
                    .ok_or("Missing request lane Y")?
                    .clone();
                seed_claimed_evidence_lane(&lane_x, true)?;
                seed_claimed_evidence_lane(&lane_y, false)?;
                let baseline = snapshot_hearth_files(&lane_x)?;
                ctx.set("p4_lane_x_baseline", baseline);
                Ok(ctx)
            },
        ),
        async_step_def(
            "the same unsatisfied evidence transition is sent to both request lanes",
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_x_path", "PathBuf"),
                ("hearth_y_path", "PathBuf"),
                ("p4_lane_x_baseline", "HearthFileMap"),
            ],
            &[
                ("engine_process", "EngineProcess"),
                ("p4_lane_isolation_outcome", "LaneIsolationOutcome"),
            ],
            |mut ctx, _params| async move {
                let engine = take_engine(&mut ctx)?;
                let lane_x = ctx
                    .get::<PathBuf>("hearth_x_path")
                    .ok_or("Missing request lane X")?
                    .clone();
                let lane_y = ctx
                    .get::<PathBuf>("hearth_y_path")
                    .ok_or("Missing request lane Y")?
                    .clone();
                let baseline = ctx
                    .get::<BTreeMap<PathBuf, Vec<u8>>>("p4_lane_x_baseline")
                    .ok_or("Missing request lane X baseline")?
                    .clone();
                let mut client = connect(engine.port).await?;

                let lane_x_result = client
                    .complete(anvil_test_support::surfaced(complete_request_for_hearth(
                        &lane_x,
                        "Evidence-Lane-X-100001",
                    )))
                    .await;
                let (lane_x_success, lane_x_error) = match lane_x_result {
                    Ok(response) => (!response.into_inner().new_state.is_empty(), String::new()),
                    Err(status) => (
                        false,
                        format!("{:?}: {}", status.code(), status.message()),
                    ),
                };

                let lane_y_result = client
                    .complete(anvil_test_support::surfaced(complete_request_for_hearth(
                        &lane_y,
                        "Evidence-Lane-Y-100002",
                    )))
                    .await;
                let lane_y_success = lane_y_result
                    .map(|response| !response.into_inner().new_state.is_empty())
                    .map_err(|status| {
                        format!(
                            "Default-off lane Y complete failed: {:?}: {}",
                            status.code(),
                            status.message()
                        )
                    })?;
                let (lane_y_rows, _) = poll_measurement_sink(&lane_y).await?;
                let lane_x_unchanged = snapshot_hearth_files(&lane_x)? == baseline;

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set(
                    "p4_lane_isolation_outcome",
                    LaneIsolationOutcome {
                        lane_x_success,
                        lane_x_error,
                        lane_x_unchanged,
                        lane_y_success,
                        lane_y_rows,
                    },
                );
                Ok(out)
            },
        ),
        check_def(
            "request lane X is refused before mutation with failed precondition {string}",
            &[("p4_lane_isolation_outcome", "LaneIsolationOutcome")],
            |ctx, params| {
                let code = params.get_string(0).ok_or("Expected refusal code")?;
                let outcome = ctx
                    .get::<LaneIsolationOutcome>("p4_lane_isolation_outcome")
                    .ok_or("Missing lane-isolation outcome")?;
                let expected = format!("FailedPrecondition: {}", code);
                if !outcome.lane_x_success
                    && outcome.lane_x_error == expected
                    && outcome.lane_x_unchanged
                {
                    Ok(())
                } else {
                    Err(format!(
                        "Lane X expected exact refusal '{}' before mutation; success={} error='{}' unchanged={}",
                        expected,
                        outcome.lane_x_success,
                        outcome.lane_x_error,
                        outcome.lane_x_unchanged
                    ))
                }
            },
        ),
        check_def(
            "request lane Y succeeds and records evidence status {string}",
            &[("p4_lane_isolation_outcome", "LaneIsolationOutcome")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected evidence status")?;
                let outcome = ctx
                    .get::<LaneIsolationOutcome>("p4_lane_isolation_outcome")
                    .ok_or("Missing lane-isolation outcome")?;
                let assessment_rows = outcome
                    .lane_y_rows
                    .iter()
                    .filter(|row| row.get("evidence_status").is_some())
                    .collect::<Vec<_>>();
                let actual = assessment_rows
                    .first()
                    .and_then(|row| row.get("evidence_status"))
                    .and_then(Value::as_str);
                if outcome.lane_y_success
                    && assessment_rows.len() == 1
                    && actual == Some(expected)
                {
                    Ok(())
                } else {
                    Err(format!(
                        "Lane Y expected success with one '{}' assessment; success={} rows={}",
                        expected,
                        outcome.lane_y_success,
                        render_rows(&outcome.lane_y_rows)
                    ))
                }
            },
        ),
        step_def(
            "a complete transition claiming {string} as {string}",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &fixture_outputs(),
            |ctx, params| {
                let class = params.get_string(0).ok_or("Expected evidence class")?;
                let reference = params.get_string(1).ok_or("Expected evidence reference")?;
                configure_fixture(
                    ctx,
                    Leg::Complete,
                    false,
                    class,
                    vec![claim(class, reference)],
                )
            },
        ),
        step_def(
            "the transition request contains recognizable raw note, project root, and artifact path text",
            &fixture_inputs(),
            &fixture_outputs(),
            |mut ctx, _params| {
                let engine = take_engine(&mut ctx)?;
                let hearth = hearth(&ctx)?;
                let mut fixture = take_fixture(&mut ctx)?;
                match &mut fixture.request {
                    LifecycleRequest::Complete(request) => {
                        request.note = RAW_NOTE.to_string();
                        request.project_root = RAW_PROJECT_ROOT.to_string();
                        if request.artifact_path != ARTIFACT_PATH {
                            return Err(format!(
                                "Privacy fixture expected artifact path '{}', got '{}'",
                                ARTIFACT_PATH, request.artifact_path
                            ));
                        }
                    }
                    _ => return Err("Raw-request privacy fixture must use complete".to_string()),
                }
                fixture.privacy_values_present = true;
                restore(engine, hearth, fixture)
            },
        ),
        step_def(
            "a complete transition with obligation {string} claiming {string}",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &fixture_outputs(),
            |ctx, params| {
                let obligation = params.get_string(0).ok_or("Expected obligation")?;
                let claimed = params.get_string(1).ok_or("Expected claimed class")?;
                configure_fixture(
                    ctx,
                    Leg::Complete,
                    false,
                    obligation,
                    vec![claim(claimed, &format!("opaque:{}:claim", claimed))],
                )
            },
        ),
        step_def(
            "a complete transition with obligation {string} and no claims",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &fixture_outputs(),
            |ctx, params| {
                let obligation = params.get_string(0).ok_or("Expected obligation")?;
                configure_fixture(ctx, Leg::Complete, false, obligation, Vec::new())
            },
        ),
        step_def(
            "a satisfied complete transition whose selected role {string} requires {string} while reviewer requires {string}",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &fixture_outputs(),
            |ctx, params| {
                let selected_role = params.get_string(0).ok_or("Expected selected role")?;
                let selected_obligation = params
                    .get_string(1)
                    .ok_or("Expected selected-role obligation")?;
                let reviewer_obligation = params
                    .get_string(2)
                    .ok_or("Expected reviewer obligation")?;
                if selected_role != "complete" {
                    return Err(format!(
                        "Complete-role fixture requires selected role 'complete', got '{}'",
                        selected_role
                    ));
                }
                configure_complete_role_fixture(
                    ctx,
                    selected_obligation,
                    reviewer_obligation,
                )
            },
        ),
        step_def(
            "parallel complete edges share a destination, with reviewer {string} for full_revision declared before complete {string} for satisfied",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &fixture_outputs(),
            |ctx, params| {
                let reviewer_obligation = params
                    .get_string(0)
                    .ok_or("Expected reviewer obligation")?;
                let complete_obligation = params
                    .get_string(1)
                    .ok_or("Expected complete obligation")?;
                configure_parallel_complete_role_fixture(
                    ctx,
                    complete_obligation,
                    reviewer_obligation,
                )
            },
        ),
        step_def(
            "a complete transition with an opaque evidence reference containing JSON C0 controls",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &fixture_outputs(),
            |ctx, _params| {
                configure_fixture(
                    ctx,
                    Leg::Complete,
                    false,
                    "artifact_of_consequence",
                    vec![claim("artifact_of_consequence", JSON_C0_REFERENCE)],
                )
            },
        ),
        step_def(
            "a {string} transition whose assessed step requires {string}",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &fixture_outputs(),
            |ctx, params| {
                let leg = parse_leg(params.get_string(0).ok_or("Expected lifecycle leg")?)?;
                let obligation = params.get_string(1).ok_or("Expected obligation")?;
                configure_fixture(ctx, leg, false, obligation, Vec::new())
            },
        ),
        step_def(
            "that transition claims {string}",
            &fixture_inputs(),
            &fixture_outputs(),
            |mut ctx, params| {
                let class = params.get_string(0).ok_or("Expected claimed class")?;
                let engine = take_engine(&mut ctx)?;
                let hearth = hearth(&ctx)?;
                let mut fixture = take_fixture(&mut ctx)?;
                set_claims(
                    &mut fixture.request,
                    vec![claim(class, &format!("opaque:{}:outline", class))],
                );
                restore(engine, hearth, fixture)
            },
        ),
        step_def(
            "a complete transition whose step has no evidence obligation",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &fixture_outputs(),
            |ctx, _params| configure_fixture(ctx, Leg::Complete, false, "", Vec::new()),
        ),
        step_def(
            "a FREE complete transition whose parsed step declares an evidence obligation",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &fixture_outputs(),
            |ctx, _params| {
                configure_fixture(
                    ctx,
                    Leg::Complete,
                    true,
                    "artifact_of_consequence",
                    Vec::new(),
                )
            },
        ),
        async_step_def(
            "the evidence lifecycle transition is sent",
            &fixture_inputs(),
            &fixture_outputs(),
            |mut ctx, _params| async move {
                let engine = take_engine(&mut ctx)?;
                let hearth = hearth(&ctx)?;
                let mut fixture = take_fixture(&mut ctx)?;
                let mut client = connect(engine.port).await?;
                let result = match fixture.request.clone() {
                    LifecycleRequest::Begin(request) => client
                        .begin(anvil_test_support::surfaced(request))
                        .await
                        .map(|response| !response.into_inner().state.is_empty()),
                    LifecycleRequest::Snapshot(request) => client
                        .snapshot(anvil_test_support::surfaced(request))
                        .await
                        .map(|response| response.into_inner().success),
                    LifecycleRequest::Complete(request) => client
                        .complete(anvil_test_support::surfaced(request))
                        .await
                        .map(|response| !response.into_inner().new_state.is_empty()),
                };
                match result {
                    Ok(success) => fixture.outcome.success = success,
                    Err(status) => {
                        fixture.outcome.error =
                            format!("{:?}: {}", status.code(), status.message())
                    }
                }
                match poll_measurement_sink(&hearth).await {
                    Ok((rows, raw_sink)) => {
                        fixture.outcome.rows = rows;
                        fixture.outcome.raw_sink = raw_sink;
                    }
                    Err(error) if fixture.outcome.error.is_empty() => {
                        fixture.outcome.error = error;
                    }
                    Err(_) => {}
                }
                restore(engine, hearth, fixture)
            },
        ),
        check_def(
            "the evidence lifecycle transition succeeds",
            &[("p2a_evidence_fixture", "EvidenceFixture")],
            |ctx, _params| {
                let fixture = fixture(&ctx)?;
                if fixture.outcome.success && fixture.outcome.error.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {:?} transition success, success={} error='{}'",
                        fixture.leg, fixture.outcome.success, fixture.outcome.error
                    ))
                }
            },
        ),
        check_def(
            "the transition is refused with failed precondition {string}",
            &[("p2a_evidence_fixture", "EvidenceFixture")],
            |ctx, params| {
                let code = params.get_string(0).ok_or("Expected refusal code")?;
                let fixture = fixture(&ctx)?;
                let expected = format!("FailedPrecondition: {}", code);
                if !fixture.outcome.success && fixture.outcome.error == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected refusal starting with '{}', success={} error='{}'",
                        expected, fixture.outcome.success, fixture.outcome.error
                    ))
                }
            },
        ),
        check_def(
            "the gated artifact remains in state {string}",
            &[
                ("hearth_path", "PathBuf"),
                ("p2a_evidence_fixture", "EvidenceFixture"),
            ],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected artifact state")?;
                let hearth = hearth(&ctx)?;
                let actual = FileSystemSnapshotAdapter::new(hearth.clone())
                    .read_artifact_state(ARTIFACT_PATH)
                    .map_err(|error| {
                        format!(
                            "fold gated artifact state at {}/{}: {}",
                            hearth.display(),
                            ARTIFACT_PATH,
                            error
                        )
                    })?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected gated artifact state '{}', got '{}' in {}/{}",
                        expected,
                        actual,
                        hearth.display(),
                        ARTIFACT_PATH
                    ))
                }
            },
        ),
        check_def(
            "the gated transition artifacts remain byte-for-byte unchanged",
            &[
                ("hearth_path", "PathBuf"),
                ("p2a_evidence_fixture", "EvidenceFixture"),
            ],
            |ctx, _params| {
                let fixture = fixture(&ctx)?;
                let current = snapshot_hearth_files(&hearth(&ctx)?)?;
                if current == fixture.baseline_hearth_files {
                    return Ok(());
                }
                let changed = fixture
                    .baseline_hearth_files
                    .keys()
                    .chain(current.keys())
                    .filter(|path| {
                        fixture.baseline_hearth_files.get(*path) != current.get(*path)
                    })
                    .collect::<BTreeSet<_>>();
                Err(format!(
                    "Expected refusal before every hearth write; changed paths: {:?}",
                    changed
                ))
            },
        ),
        check_def(
            "no evidence assessment row is emitted",
            &[
                ("hearth_path", "PathBuf"),
                ("p2a_evidence_fixture", "EvidenceFixture"),
            ],
            |ctx, _params| {
                let fixture = fixture(&ctx)?;
                let sink_path = hearth(&ctx)?.join("step-measurement.jsonl");
                let sink = std::fs::read(&sink_path).unwrap_or_default();
                if fixture.outcome.rows.is_empty() && sink.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected no measurement row after refusal; captured={} sink_bytes={}",
                        render_rows(&fixture.outcome.rows),
                        sink.len()
                    ))
                }
            },
        ),
        check_def(
            "exactly one evidence assessment row records status {string}",
            &[("p2a_evidence_fixture", "EvidenceFixture")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected evidence status")?;
                let rows = assessment_rows(fixture(&ctx)?);
                if rows.len() != 1 {
                    return Err(format!(
                        "Expected exactly one evidence assessment row with status '{}', got {}. Lean rows: {}",
                        expected,
                        rows.len(),
                        render_rows(&fixture(&ctx)?.outcome.rows)
                    ));
                }
                let actual = rows[0]
                    .get("evidence_status")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let version = rows[0]
                    .get("playbook_version")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if actual == expected && version == fixture(&ctx)?.machine_version {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected evidence_status '{}' and machine version '{}', got status='{}' version='{}' in {}",
                        expected,
                        fixture(&ctx)?.machine_version,
                        actual,
                        version,
                        rows[0]
                    ))
                }
            },
        ),
        check_def(
            "the evidence row carries the exact registry machine version",
            &[("p2a_evidence_fixture", "EvidenceFixture")],
            |ctx, _params| {
                let fixture = fixture(&ctx)?;
                let row = one_assessment_row(fixture)?;
                let actual = row
                    .get("playbook_version")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if actual == fixture.machine_version {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected exact on-disk machine version '{}', got '{}' in {}",
                        fixture.machine_version, actual, row
                    ))
                }
            },
        ),
        check_def(
            "the evidence row contains only the opaque claim reference, not the raw request text",
            &[("p2a_evidence_fixture", "EvidenceFixture")],
            |ctx, _params| assert_private_claim_row(fixture(&ctx)?),
        ),
        check_def(
            "the evidence row names missing classes {string}",
            &[("p2a_evidence_fixture", "EvidenceFixture")],
            |ctx, params| {
                let expected = csv(params.get_string(0).ok_or("Expected missing classes")?);
                let row = one_assessment_row(fixture(&ctx)?)?;
                let actual = json_string_array(row, "missing_evidence_classes")?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected missing classes {:?}, got {:?} in {}",
                        expected, actual, row
                    ))
                }
            },
        ),
        check_def(
            "the evidence row round-trips the exact opaque JSON C0 reference",
            &[("p2a_evidence_fixture", "EvidenceFixture")],
            |ctx, _params| {
                let row = one_assessment_row(fixture(&ctx)?)?;
                let claims = row
                    .get("claimed_evidence")
                    .and_then(Value::as_array)
                    .ok_or_else(|| format!("Evidence row has no claimed_evidence array: {}", row))?;
                if claims.len() != 1 {
                    return Err(format!("Expected one C0 claim, got {:?}", claims));
                }
                let actual = claims[0]
                    .get("reference")
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!("C0 claim has no string reference: {}", claims[0]))?;
                if actual == JSON_C0_REFERENCE {
                    Ok(())
                } else {
                    Err(format!(
                        "Opaque C0 reference changed: expected {:?}, got {:?}",
                        JSON_C0_REFERENCE, actual
                    ))
                }
            },
        ),
        check_def(
            "the evidence row resolved the {string} state and {string} role",
            &[("p2a_evidence_fixture", "EvidenceFixture")],
            |ctx, params| {
                let expected_state = params.get_string(0).ok_or("Expected state")?;
                let expected_role = params.get_string(1).ok_or("Expected role")?;
                let fixture = fixture(&ctx)?;
                let row = one_assessment_row(fixture)?;
                let state_field = if fixture.leg == Leg::Complete {
                    "from_state"
                } else {
                    "to_state"
                };
                let actual_state = row.get(state_field).and_then(Value::as_str).unwrap_or("");
                let actual_role = row.get("role").and_then(Value::as_str).unwrap_or("");
                if fixture.resolved_state == expected_state
                    && fixture.resolved_role == expected_role
                    && actual_state == expected_state
                    && actual_role == expected_role
                {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {:?} to resolve state='{}' role='{}'; fixture state='{}' role='{}', row {}='{}' role='{}': {}",
                        fixture.leg,
                        expected_state,
                        expected_role,
                        fixture.resolved_state,
                        fixture.resolved_role,
                        state_field,
                        actual_state,
                        actual_role,
                        row
                    ))
                }
            },
        ),
        check_def(
            "the durable step measurement bytes match the legacy row shape",
            &[("p2a_evidence_fixture", "EvidenceFixture")],
            |ctx, _params| assert_legacy_bytes(fixture(&ctx)?),
        ),
        check_def(
            "the durable row contains none of the evidence keys",
            &[("p2a_evidence_fixture", "EvidenceFixture")],
            |ctx, _params| {
                let fixture = fixture(&ctx)?;
                if fixture.outcome.rows.is_empty() {
                    return Err("Expected a durable lean step-measurement row".to_string());
                }
                let present = fixture
                    .outcome
                    .rows
                    .iter()
                    .flat_map(|row| {
                        EVIDENCE_KEYS
                            .iter()
                            .filter(move |key| row.get(**key).is_some())
                    })
                    .copied()
                    .collect::<BTreeSet<_>>();
                if present.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected legacy evidence-neutral rows; found keys {:?} in {}",
                        present,
                        render_rows(&fixture.outcome.rows)
                    ))
                }
            },
        ),
    ]
}

fn fixture_inputs() -> Vec<(&'static str, &'static str)> {
    vec![
        ("engine_process", "EngineProcess"),
        ("hearth_path", "PathBuf"),
        ("p2a_evidence_fixture", "EvidenceFixture"),
    ]
}

fn fixture_outputs() -> Vec<(&'static str, &'static str)> {
    fixture_inputs()
}

fn seed_hearth(hearth: &Path) -> Result<(), String> {
    std::fs::create_dir_all(hearth.join("tracks"))
        .map_err(|error| format!("create tracks fixture: {}", error))?;
    std::fs::write(hearth.join("tracks.md"), "# Tracks\n")
        .map_err(|error| format!("write tracks registry: {}", error))?;
    std::fs::create_dir_all(hearth.join("evidence_runs"))
        .map_err(|error| format!("create evidence run directory: {}", error))?;
    std::fs::write(
        hearth.join("evidence_runs.md"),
        "# Evidence Runs\n\n## spec\n\n## spec_review\n\n## completed\n",
    )
    .map_err(|error| format!("write evidence registry: {}", error))?;
    std::fs::create_dir_all(hearth.join("playbooks").join(PLAYBOOK_ID))
        .map_err(|error| format!("create evidence playbook: {}", error))?;
    write_machine(hearth, false, Leg::Complete, "")?;
    Ok(())
}

fn seed_decided_privacy_decision(hearth: &Path) -> Result<(), String> {
    seed_privacy_decision(hearth, "decided")
}

fn seed_privacy_decision(hearth: &Path, state: &str) -> Result<(), String> {
    let decision = hearth
        .join("decisions")
        .join("20260622T1940_step_measurement_emit_privacy");
    std::fs::create_dir_all(&decision)
        .map_err(|error| format!("create privacy decision fixture: {}", error))?;
    let status_path = decision.join("status.yaml");
    std::fs::write(
        &status_path,
        format!(
            "version: 1\nkind: decision\nstate: {}\ntransitions:\n",
            state
        ),
    )
    .map_err(|error| format!("write {}: {}", status_path.display(), error))
}

fn seed_claimed_evidence_lane(hearth: &Path, enabled: bool) -> Result<(), String> {
    seed_hearth(hearth)?;
    seed_decided_privacy_decision(hearth)?;
    let flags_path = hearth.join("engine-flags.env");
    let flags = if enabled {
        format!("{}=1\n", CLAIMED_EVIDENCE_GATE_FLAG)
    } else {
        "# claimed-evidence gate intentionally absent\n".to_string()
    };
    std::fs::write(&flags_path, flags)
        .map_err(|error| format!("write {}: {}", flags_path.display(), error))?;
    write_machine(
        hearth,
        false,
        Leg::Complete,
        "artifact_of_consequence",
    )?;
    seed_artifact(hearth)
}

fn configure_fixture(
    mut ctx: Context,
    leg: Leg,
    free: bool,
    obligation: &str,
    claims: Vec<anvil_engine::proto::ClaimedEvidence>,
) -> Result<Context, String> {
    let engine = take_engine(&mut ctx)?;
    let hearth = hearth(&ctx)?;
    let machine_version = write_machine(&hearth, free, leg, obligation)?;
    if leg != Leg::Begin {
        seed_artifact(&hearth)?;
    }
    let (request, resolved_state, resolved_role) = request_for(leg, claims);
    let baseline_hearth_files = snapshot_hearth_files(&hearth)?;
    restore(
        engine,
        hearth,
        EvidenceFixture {
            leg,
            request,
            resolved_state,
            resolved_role,
            machine_version,
            privacy_values_present: false,
            baseline_hearth_files,
            outcome: RpcOutcome::default(),
        },
    )
}

fn configure_complete_role_fixture(
    ctx: Context,
    complete_obligation: &str,
    reviewer_obligation: &str,
) -> Result<Context, String> {
    configure_complete_role_fixture_with_edges(ctx, complete_obligation, reviewer_obligation, false)
}

fn configure_parallel_complete_role_fixture(
    ctx: Context,
    complete_obligation: &str,
    reviewer_obligation: &str,
) -> Result<Context, String> {
    configure_complete_role_fixture_with_edges(ctx, complete_obligation, reviewer_obligation, true)
}

fn configure_complete_role_fixture_with_edges(
    mut ctx: Context,
    complete_obligation: &str,
    reviewer_obligation: &str,
    parallel_edges: bool,
) -> Result<Context, String> {
    let engine = take_engine(&mut ctx)?;
    let hearth = hearth(&ctx)?;
    let machine_version = write_complete_role_machine(
        &hearth,
        &csv(complete_obligation),
        &csv(reviewer_obligation),
        parallel_edges,
    )?;
    seed_artifact_in_state(&hearth, "closure_review")?;
    let request = LifecycleRequest::Complete(anvil_engine::proto::CompleteRequest {
        artifact_path: ARTIFACT_PATH.to_string(),
        actor_name: ACTOR.to_string(),
        actor_type: "agent".to_string(),
        actor_model: "brine".to_string(),
        actor_provider: "test".to_string(),
        satisfaction: "satisfied".to_string(),
        ..Default::default()
    });
    let baseline_hearth_files = snapshot_hearth_files(&hearth)?;
    restore(
        engine,
        hearth,
        EvidenceFixture {
            leg: Leg::Complete,
            request,
            resolved_state: "closure_review".to_string(),
            resolved_role: "complete".to_string(),
            machine_version,
            privacy_values_present: false,
            baseline_hearth_files,
            outcome: RpcOutcome::default(),
        },
    )
}

fn snapshot_hearth_files(hearth: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
    fn visit(
        root: &Path,
        directory: &Path,
        files: &mut BTreeMap<PathBuf, Vec<u8>>,
    ) -> Result<(), String> {
        let entries = std::fs::read_dir(directory)
            .map_err(|error| format!("read fixture directory {}: {}", directory.display(), error))?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                format!("read fixture entry under {}: {}", directory.display(), error)
            })?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|error| format!("read file type {}: {}", path.display(), error))?;
            if file_type.is_dir() {
                visit(root, &path, files)?;
            } else if file_type.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|error| format!("relativize {}: {}", path.display(), error))?
                    .to_path_buf();
                let bytes = std::fs::read(&path)
                    .map_err(|error| format!("read fixture file {}: {}", path.display(), error))?;
                files.insert(relative, bytes);
            }
        }
        Ok(())
    }

    let mut files = BTreeMap::new();
    visit(hearth, hearth, &mut files)?;
    Ok(files)
}

fn write_machine(hearth: &Path, free: bool, leg: Leg, obligation: &str) -> Result<String, String> {
    let required = csv(obligation);
    let spec_obligation = if leg == Leg::Snapshot {
        Vec::new()
    } else {
        required.clone()
    };
    let review_obligation = if leg == Leg::Snapshot {
        required
    } else {
        Vec::new()
    };
    let yaml = machine_yaml(free, &spec_obligation, &review_obligation);
    let path = hearth
        .join("playbooks")
        .join(PLAYBOOK_ID)
        .join("machine.yaml");
    std::fs::write(&path, &yaml).map_err(|error| format!("write {}: {}", path.display(), error))?;
    let machine = load_from_yaml(PLAYBOOK_ID, &yaml, &[])
        .map_err(|error| format!("load exact on-disk fixture machine: {}", error))?;
    machine_content_version(&machine)
        .ok_or_else(|| "Exact on-disk fixture machine had no content version".to_string())
}

fn write_complete_role_machine(
    hearth: &Path,
    complete_obligation: &[String],
    reviewer_obligation: &[String],
    parallel_edges: bool,
) -> Result<String, String> {
    let yaml = complete_role_machine_yaml(complete_obligation, reviewer_obligation, parallel_edges);
    let path = hearth
        .join("playbooks")
        .join(PLAYBOOK_ID)
        .join("machine.yaml");
    std::fs::write(&path, &yaml).map_err(|error| format!("write {}: {}", path.display(), error))?;
    let machine = load_from_yaml(PLAYBOOK_ID, &yaml, &[])
        .map_err(|error| format!("load complete-role fixture machine: {}", error))?;
    machine_content_version(&machine)
        .ok_or_else(|| "Complete-role fixture machine had no content version".to_string())
}

fn machine_yaml(free: bool, spec_obligation: &[String], review_obligation: &[String]) -> String {
    let register = if free { "free" } else { "driven" };
    format!(
        r#"kind: evidence_probe
directory: evidence_runs
registry: evidence_runs.md
description: "P2a exact-version evidence assessment probe."
register: {register}
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
        intent: "Produce the probe specification."
        expected_output: "A durable probe specification."
{spec_obligation}  - name: spec_review
    role_filters: []
    registry_section: spec_review
    projection_targets: []
    is_review_gate: true
    is_terminal: false
    measurement_by_role:
      reviewer:
        intent: "Review the probe specification."
        expected_output: "A durable probe review."
{review_obligation}  - name: completed
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
"#,
        register = register,
        spec_obligation = obligation_yaml(spec_obligation),
        review_obligation = obligation_yaml(review_obligation),
    )
}

fn complete_role_machine_yaml(
    complete_obligation: &[String],
    reviewer_obligation: &[String],
    parallel_edges: bool,
) -> String {
    let transitions = if parallel_edges {
        r#"  - from_state: closure_review
    to_state: completed
    required_role: reviewer
    required_satisfaction:
      - full_revision
    requires_approver: false
  - from_state: closure_review
    to_state: completed
    required_role: complete
    required_satisfaction:
      - satisfied
    requires_approver: false
"#
    } else {
        r#"  - from_state: closure_review
    to_state: completed
    required_role: complete
    required_satisfaction:
      - satisfied
    requires_approver: false
"#
    };
    format!(
        r#"kind: evidence_probe
directory: evidence_runs
registry: evidence_runs.md
description: "P2a machine-selected complete-role probe."
register: driven
required_fields: []
roles:
  - doer
  - reviewer
  - complete
states:
  - name: closure_review
    role_filters: []
    registry_section: closure_review
    projection_targets: []
    is_review_gate: true
    is_terminal: false
    measurement_by_role:
      complete:
        intent: "Close the reviewed probe administratively."
        expected_output: "A completed probe transition."
{complete_obligation}      reviewer:
        intent: "Review the probe before closure."
        expected_output: "A probe review verdict."
{reviewer_obligation}  - name: completed
    role_filters: []
    registry_section: completed
    projection_targets: []
    is_review_gate: false
    is_terminal: true
transitions:
{transitions}outcome_predicate:
  terminal_state: completed
"#,
        complete_obligation = obligation_yaml(complete_obligation),
        reviewer_obligation = obligation_yaml(reviewer_obligation),
        transitions = transitions,
    )
}

fn obligation_yaml(classes: &[String]) -> String {
    if classes.is_empty() {
        return String::new();
    }
    let mut yaml = "        evidence_obligation:\n".to_string();
    for class in classes {
        yaml.push_str(&format!("          - {}\n", class));
    }
    yaml
}

fn seed_artifact(hearth: &Path) -> Result<(), String> {
    seed_artifact_in_state(hearth, "spec")
}

fn seed_artifact_in_state(hearth: &Path, state: &str) -> Result<(), String> {
    let directory = hearth.join(ARTIFACT_PATH);
    if directory.exists() {
        std::fs::remove_dir_all(&directory)
            .map_err(|error| format!("reset artifact fixture: {}", error))?;
    }
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("create artifact fixture: {}", error))?;
    std::fs::write(
        directory.join("status.yaml"),
        format!(
            "version: 1\nkind: {kind}\nstate: {state}\ntransitions:\n  - to: {state}\n    at: \"2026-07-19T00:00:00Z\"\n    actor: Seed-P2a-000001\n    role: doer\n",
            kind = KIND,
            state = state,
        ),
    )
    .map_err(|error| format!("write artifact status: {}", error))?;
    std::fs::write(directory.join("spec.md"), "# P2a evidence probe\n")
        .map_err(|error| format!("write probe artifact: {}", error))?;
    Ok(())
}

fn request_for(
    leg: Leg,
    claims: Vec<anvil_engine::proto::ClaimedEvidence>,
) -> (LifecycleRequest, String, String) {
    match leg {
        Leg::Begin => (
            LifecycleRequest::Begin(anvil_engine::proto::BeginRequest {
                artifact_type: KIND.to_string(),
                track_name: "p2a evidence begin".to_string(),
                actor_name: ACTOR.to_string(),
                actor_type: "agent".to_string(),
                actor_model: "brine".to_string(),
                actor_provider: "test".to_string(),
                session_role: "creator".to_string(),
                ctx_org: "Foundation".to_string(),
                ctx_role: "read".to_string(),
                ctx_clearance: "internal".to_string(),
                claimed_evidence: claims,
                ..Default::default()
            }),
            "spec".to_string(),
            "doer".to_string(),
        ),
        Leg::Snapshot => (
            LifecycleRequest::Snapshot(anvil_engine::proto::SnapshotRequest {
                artifact_path: ARTIFACT_PATH.to_string(),
                to_state: "spec_review".to_string(),
                actor_name: ACTOR.to_string(),
                actor_role: "reviewer".to_string(),
                actor_type: "agent".to_string(),
                actor_model: "brine".to_string(),
                actor_provider: "test".to_string(),
                claimed_evidence: claims,
                ..Default::default()
            }),
            "spec_review".to_string(),
            "reviewer".to_string(),
        ),
        Leg::Complete => (
            LifecycleRequest::Complete(anvil_engine::proto::CompleteRequest {
                artifact_path: ARTIFACT_PATH.to_string(),
                actor_name: ACTOR.to_string(),
                actor_type: "agent".to_string(),
                actor_model: "brine".to_string(),
                actor_provider: "test".to_string(),
                claimed_evidence: claims,
                ..Default::default()
            }),
            "spec".to_string(),
            "doer".to_string(),
        ),
    }
}

fn complete_request_for_hearth(
    hearth: &Path,
    actor_name: &str,
) -> anvil_engine::proto::CompleteRequest {
    anvil_engine::proto::CompleteRequest {
        artifact_path: ARTIFACT_PATH.to_string(),
        actor_name: actor_name.to_string(),
        actor_type: "agent".to_string(),
        actor_model: "brine".to_string(),
        actor_provider: "test".to_string(),
        hearth_path: hearth.to_string_lossy().into_owned(),
        ..Default::default()
    }
}

fn set_claims(request: &mut LifecycleRequest, claims: Vec<anvil_engine::proto::ClaimedEvidence>) {
    match request {
        LifecycleRequest::Begin(request) => request.claimed_evidence = claims,
        LifecycleRequest::Snapshot(request) => request.claimed_evidence = claims,
        LifecycleRequest::Complete(request) => request.claimed_evidence = claims,
    }
}

fn claim(class: &str, reference: &str) -> anvil_engine::proto::ClaimedEvidence {
    anvil_engine::proto::ClaimedEvidence {
        class: class.to_string(),
        reference: reference.to_string(),
    }
}

fn parse_leg(raw: &str) -> Result<Leg, String> {
    match raw {
        "begin" => Ok(Leg::Begin),
        "snapshot" => Ok(Leg::Snapshot),
        "complete" => Ok(Leg::Complete),
        _ => Err(format!("Unknown evidence lifecycle leg '{}'", raw)),
    }
}

fn csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect()
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
    .map_err(|error| format!("connect to evidence engine: {}", error))
}

async fn poll_measurement_sink(hearth: &Path) -> Result<(Vec<Value>, Vec<u8>), String> {
    let path = hearth.join("step-measurement.jsonl");
    let mut last_error = String::new();
    for _ in 0..100 {
        match std::fs::read(&path) {
            Ok(bytes) if !bytes.is_empty() => match parse_jsonl(&bytes) {
                Ok(rows) if !rows.is_empty() => return Ok((rows, bytes)),
                Ok(_) => {}
                Err(error) => last_error = error,
            },
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => last_error = format!("read {}: {}", path.display(), error),
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Err(format!(
        "No complete step-measurement row appeared at {} within 2s{}",
        path.display(),
        if last_error.is_empty() {
            String::new()
        } else {
            format!("; last error: {}", last_error)
        }
    ))
}

fn parse_jsonl(bytes: &[u8]) -> Result<Vec<Value>, String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| format!("step-measurement sink is not UTF-8: {}", error))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<Value>(line)
                .map_err(|error| format!("parse step-measurement row '{}': {}", line, error))
        })
        .collect()
}

fn assessment_rows(fixture: &EvidenceFixture) -> Vec<&Value> {
    fixture
        .outcome
        .rows
        .iter()
        .filter(|row| row.get("evidence_status").is_some())
        .collect()
}

fn one_assessment_row(fixture: &EvidenceFixture) -> Result<&Value, String> {
    let rows = assessment_rows(fixture);
    if rows.len() == 1 {
        Ok(rows[0])
    } else {
        Err(format!(
            "Expected exactly one evidence assessment row, got {}. Lean rows: {}",
            rows.len(),
            render_rows(&fixture.outcome.rows)
        ))
    }
}

fn json_string_array(row: &Value, key: &str) -> Result<Vec<String>, String> {
    row.get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("Expected array field '{}' in {}", key, row))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(ToString::to_string)
                .ok_or_else(|| format!("Expected string in '{}': {}", key, value))
        })
        .collect()
}

fn assert_private_claim_row(fixture: &EvidenceFixture) -> Result<(), String> {
    if !fixture.privacy_values_present {
        return Err("Privacy request values were not installed".to_string());
    }
    let row = one_assessment_row(fixture)?;
    let raw = serde_json::to_string(row)
        .map_err(|error| format!("render evidence assessment row: {}", error))?;
    let claims = row
        .get("claimed_evidence")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("Evidence row has no claimed_evidence array: {}", row))?;
    if claims.len() != 1 {
        return Err(format!("Expected one opaque claim, got {:?}", claims));
    }
    let claim = claims[0]
        .as_object()
        .ok_or_else(|| format!("Claim is not an object: {}", claims[0]))?;
    let keys = claim.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected_keys = ["class", "reference"].into_iter().collect::<BTreeSet<_>>();
    let expected_reference = "artifact:test-run:sha-9173";
    let class_ok = claim.get("class").and_then(Value::as_str) == Some("artifact_of_consequence");
    let reference_ok = claim.get("reference").and_then(Value::as_str) == Some(expected_reference);
    let leaked = [RAW_NOTE, RAW_PROJECT_ROOT, ARTIFACT_PATH]
        .into_iter()
        .filter(|value| raw.contains(value))
        .collect::<Vec<_>>();
    if keys == expected_keys
        && class_ok
        && reference_ok
        && raw.contains(expected_reference)
        && leaked.is_empty()
    {
        Ok(())
    } else {
        Err(format!(
            "Evidence privacy failure: keys={:?}, class_ok={}, reference_ok={}, leaked={:?}, row={}",
            keys, class_ok, reference_ok, leaked, row
        ))
    }
}

fn assert_legacy_bytes(fixture: &EvidenceFixture) -> Result<(), String> {
    if fixture.outcome.rows.len() != 1 {
        return Err(format!(
            "Expected exactly one legacy lean row, got {}: {}",
            fixture.outcome.rows.len(),
            render_rows(&fixture.outcome.rows)
        ));
    }
    let row = &fixture.outcome.rows[0];
    if EVIDENCE_KEYS.iter().any(|key| row.get(*key).is_some()) {
        return Err(format!("Legacy row contains evidence keys: {}", row));
    }
    let at = row
        .get("at")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Legacy row has no at: {}", row))?;
    let actor_hash = row
        .get("actor_hash")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Legacy row has no salted actor_hash: {}", row))?;
    // The correlation keys are part of the lean row's shape (they were added by
    // the correlation-key foundation, after this pin was first written) and
    // their VALUES are nondeterministic — a salted hash and a derived label —
    // exactly like `at` and `actor_hash` above, so they are read from the row
    // rather than hard-coded. What this pin still asserts byte-for-byte is the
    // KEY SET, its ORDER, and the absence of every evidence key: an added,
    // dropped, renamed or reordered field reds it. Their values are asserted by
    // the correlation-keys scenario in the same suite.
    let correlation = ["conversation_hash", "project_label"]
        .iter()
        .filter_map(|key| {
            row.get(*key)
                .and_then(Value::as_str)
                .map(|value| format!(",\"{}\":\"{}\"", key, value))
        })
        .collect::<String>();
    let expected = format!(
        "{{\"kind\":\"step_measurement\",\"from_state\":\"spec\",\"to_state\":\"spec_review\",\"role\":\"doer\",\"intent_present\":true,\"expected_output_present\":true,\"at\":\"{}\",\"artifact_kind\":\"{}\",\"actor_hash\":\"{}\"{},\"playbook_run_id\":\"{}\"}}\n",
        at, KIND, actor_hash, correlation, RUN_ID
    )
    .into_bytes();
    if fixture.outcome.raw_sink == expected {
        Ok(())
    } else {
        Err(format!(
            "Legacy bytes changed. Expected '{}', got '{}'",
            String::from_utf8_lossy(&expected),
            String::from_utf8_lossy(&fixture.outcome.raw_sink)
        ))
    }
}

fn render_rows(rows: &[Value]) -> String {
    rows.iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

fn take_engine(ctx: &mut Context) -> Result<EngineProcess, String> {
    ctx.take::<EngineProcess>("engine_process")
        .ok_or_else(|| "No engine_process".to_string())
}

fn hearth(ctx: &Context) -> Result<PathBuf, String> {
    ctx.get::<PathBuf>("hearth_path")
        .cloned()
        .ok_or_else(|| "No hearth_path".to_string())
}

fn take_fixture(ctx: &mut Context) -> Result<EvidenceFixture, String> {
    ctx.take::<EvidenceFixture>("p2a_evidence_fixture")
        .ok_or_else(|| "No P2a evidence fixture".to_string())
}

fn fixture(ctx: &Context) -> Result<&EvidenceFixture, String> {
    ctx.get::<EvidenceFixture>("p2a_evidence_fixture")
        .ok_or_else(|| "No P2a evidence fixture".to_string())
}

fn restore(
    engine: EngineProcess,
    hearth: PathBuf,
    fixture: EvidenceFixture,
) -> Result<Context, String> {
    let mut out = Context::new();
    out.set("engine_process", engine);
    out.set("hearth_path", hearth);
    out.set("p2a_evidence_fixture", fixture);
    Ok(out)
}
