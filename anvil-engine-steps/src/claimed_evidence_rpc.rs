//! Step definitions for `claimed_evidence_rpc.feature` (T-EEC-2 P1).
//!
//! These steps observe the P1 carrier at the public engine boundary (prost
//! round-trip plus the shared mapper) and separately drive the real RPC to
//! prove established lifecycle behavior stays intact. P2's obligated-step
//! assessment behavior is covered by `step_measurement_evidence.feature`.

use anvil_test_support::engine::EngineProcess;
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{async_step_def, check_def, step_def, StepDef};
use prost::Message;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const TRACK_PATH: &str = "tracks/20260719T1800_claimed_evidence_rpc_track";
const PARENT_ID: &str = "20260411T2021_anvil_workflow_engine";
const RAW_USER_TEXT: &str = "RAW-USER-TEXT: claimed evidence must not copy this note";
const RAW_PROJECT_ROOT: &str = "/private/claimed-evidence-private-workspace";
const PROJECTION_REFERENCE_ONE: &str = "projection-only:must-not-persist:alpha";
const PROJECTION_REFERENCE_TWO: &str = "projection-only:must-not-persist:beta";
const MAPPER_AT: &str = "2026-07-19T18:00:00Z";
const MEASUREMENT_POLL_TIMEOUT: Duration = Duration::from_secs(2);
const MEASUREMENT_POLL_INTERVAL: Duration = Duration::from_millis(20);
const MEASUREMENT_STABILITY_WINDOW: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExpectedClaim {
    class: String,
    reference: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestKind {
    Complete,
    Begin,
    Snapshot,
}

#[derive(Debug, Clone)]
enum WireRequest {
    Complete(anvil_engine::proto::CompleteRequest),
    Begin(anvil_engine::proto::BeginRequest),
    Snapshot(anvil_engine::proto::SnapshotRequest),
}

#[derive(Debug, Clone, Default)]
struct BoundaryOutcome {
    response_state: String,
    response_artifact_path: String,
    response_bytes: Vec<u8>,
    rpc_error: String,
    status_transition_recorded: bool,
    transition_at: String,
    transition_bytes: Vec<u8>,
    measurement_bytes: Vec<u8>,
    projection_measurement_unchanged: bool,
    projection_persisted_claim: bool,
}

#[derive(Debug, Clone)]
struct BoundaryFixture {
    kind: RequestKind,
    request: WireRequest,
    expected_claims: Vec<ExpectedClaim>,
    mapped_claims: Vec<ExpectedClaim>,
    raw_bytes: Vec<u8>,
    round_trip_bytes: Vec<u8>,
    legacy_wire_baseline: Vec<u8>,
    outcome: BoundaryOutcome,
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a complete lifecycle request with claimed evidence:",
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
            ],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("claimed_evidence_boundary", "BoundaryFixture"),
            ],
            |mut ctx, params| {
                let claims = claims_from_table(
                    params
                        .data_table()
                        .ok_or("Expected claimed evidence table")?,
                )?;
                let request = anvil_engine::proto::CompleteRequest {
                    artifact_path: TRACK_PATH.to_string(),
                    actor_name: "Claimed-Evidence-Complete-100000".to_string(),
                    actor_type: "agent".to_string(),
                    actor_model: "brine".to_string(),
                    actor_provider: "test".to_string(),
                    note: RAW_USER_TEXT.to_string(),
                    project_root: RAW_PROJECT_ROOT.to_string(),
                    ..Default::default()
                };
                carry_fixture_context(
                    &mut ctx,
                    boundary_fixture(
                        RequestKind::Complete,
                        WireRequest::Complete(request),
                        claims,
                    )?,
                )
            },
        ),
        step_def(
            "the complete lifecycle request contains recognizable raw path and user text",
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("claimed_evidence_boundary", "BoundaryFixture"),
            ],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("claimed_evidence_boundary", "BoundaryFixture"),
            ],
            |mut ctx, _params| {
                let fixture = ctx
                    .take::<BoundaryFixture>("claimed_evidence_boundary")
                    .ok_or("No claimed evidence boundary fixture")?;
                match &fixture.request {
                    WireRequest::Complete(request)
                        if request.note == RAW_USER_TEXT
                            && request.project_root == RAW_PROJECT_ROOT => {}
                    WireRequest::Complete(_) => {
                        return Err("Complete fixture lacks recognizable raw request values".into())
                    }
                    _ => return Err("Expected a complete request fixture".into()),
                }
                carry_fixture_context(&mut ctx, fixture)
            },
        ),
        async_step_def(
            "the complete lifecycle request crosses the engine boundary",
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("claimed_evidence_boundary", "BoundaryFixture"),
            ],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("claimed_evidence_boundary", "BoundaryFixture"),
            ],
            |mut ctx, _params| async move {
                let mut fixture = take_fixture(&mut ctx, RequestKind::Complete)?;
                let request = match fixture.request.clone() {
                    WireRequest::Complete(request) => request,
                    _ => return Err("Expected complete request".into()),
                };
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let hearth = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let mut client = connect(engine.port).await?;
                match client.complete(anvil_test_support::surfaced(request)).await {
                    Ok(response) => {
                        let response = response.into_inner();
                        fixture.outcome.response_state = response.new_state.clone();
                        fixture.outcome.response_artifact_path = response.artifact_path.clone();
                        fixture.outcome.transition_at = response.transition_at.clone();
                        fixture.outcome.response_bytes = response.encode_to_vec();
                    }
                    Err(status) => {
                        fixture.outcome.rpc_error =
                            format!("{:?}: {}", status.code(), status.message());
                    }
                }
                fixture.outcome.transition_bytes = transition_dir_record_containing(
                    &hearth.join(TRACK_PATH).join("transitions"),
                    "to: spec_review",
                )?
                .unwrap_or_default();
                fixture.outcome.status_transition_recorded =
                    !fixture.outcome.transition_bytes.is_empty();
                if fixture.outcome.rpc_error.is_empty() {
                    fixture.outcome.measurement_bytes = wait_for_measurement_transition(
                        &hearth.join("step-measurement.jsonl"),
                        "spec_review",
                    )
                    .await?;
                }
                restore_fixture_context(ctx, engine, hearth, fixture)
            },
        ),
        check_def(
            "the complete request carries the claimed evidence in order",
            &[("claimed_evidence_boundary", "BoundaryFixture")],
            |ctx, _params| claims_match(&ctx, RequestKind::Complete),
        ),
        check_def(
            "each claim contains only its class and opaque reference",
            &[("claimed_evidence_boundary", "BoundaryFixture")],
            |ctx, _params| {
                let fixture = fixture(&ctx)?;
                if fixture.mapped_claims == fixture.expected_claims
                    && fixture.mapped_claims.iter().all(|claim| {
                        !claim.class.trim().is_empty() && !claim.reference.trim().is_empty()
                    })
                {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected class/reference-only claims {:?}, got {:?}",
                        fixture.expected_claims, fixture.mapped_claims
                    ))
                }
            },
        ),
        check_def(
            "the mapped claims contain neither the raw path nor the raw user text",
            &[("claimed_evidence_boundary", "BoundaryFixture")],
            |ctx, _params| {
                let fixture = fixture(&ctx)?;
                let mapped = format!("{:?}", fixture.mapped_claims);
                if mapped.contains(RAW_PROJECT_ROOT) || mapped.contains(RAW_USER_TEXT) {
                    Err(format!(
                        "Mapped claims copied raw request material: {}",
                        mapped
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the durable step measurement contains no raw project root, user note, or artifact path",
            &[("claimed_evidence_boundary", "BoundaryFixture")],
            |ctx, _params| {
                let fixture = fixture(&ctx)?;
                let measurement = String::from_utf8_lossy(&fixture.outcome.measurement_bytes);
                let forbidden = [
                    RAW_PROJECT_ROOT.to_string(),
                    RAW_USER_TEXT.to_string(),
                    TRACK_PATH.to_string(),
                ];
                let leaked = forbidden
                    .iter()
                    .filter(|value| measurement.contains(value.as_str()))
                    .cloned()
                    .collect::<Vec<_>>();
                if fixture.outcome.rpc_error.is_empty() && leaked.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "Complete RPC error='{}'; leaked raw request values={:?}; measurement={}",
                        fixture.outcome.rpc_error, leaked, measurement
                    ))
                }
            },
        ),
        step_def(
            "a legacy complete lifecycle request without claimed evidence",
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
            ],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("claimed_evidence_boundary", "BoundaryFixture"),
            ],
            |mut ctx, _params| {
                let request = anvil_engine::proto::CompleteRequest {
                    artifact_path: TRACK_PATH.to_string(),
                    actor_name: "Legacy-Complete-100000".to_string(),
                    actor_type: "agent".to_string(),
                    actor_model: "brine".to_string(),
                    actor_provider: "test".to_string(),
                    ..Default::default()
                };
                let legacy_wire_baseline = legacy_complete_wire_baseline(&request);
                let mut fixture = boundary_fixture(
                    RequestKind::Complete,
                    WireRequest::Complete(request),
                    Vec::new(),
                )?;
                fixture.legacy_wire_baseline = legacy_wire_baseline;
                carry_fixture_context(&mut ctx, fixture)
            },
        ),
        check_def(
            "the legacy complete payload bytes are unchanged",
            &[("claimed_evidence_boundary", "BoundaryFixture")],
            |ctx, _params| {
                let fixture = fixture(&ctx)?;
                if !fixture.legacy_wire_baseline.is_empty()
                    && fixture.raw_bytes == fixture.legacy_wire_baseline
                    && fixture.round_trip_bytes == fixture.legacy_wire_baseline
                {
                    Ok(())
                } else {
                    Err(format!(
                        "Legacy Complete wire changed: manual={:?}, encoded={:?}, round_trip={:?}",
                        fixture.legacy_wire_baseline,
                        fixture.raw_bytes,
                        fixture.round_trip_bytes
                    ))
                }
            },
        ),
        check_def(
            "the complete request carries no claimed evidence",
            &[("claimed_evidence_boundary", "BoundaryFixture")],
            |ctx, _params| {
                let fixture = fixture(&ctx)?;
                if fixture.mapped_claims.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected no mapped claims, got {:?}",
                        fixture.mapped_claims
                    ))
                }
            },
        ),
        check_def(
            "the successful completion behavior is unchanged",
            &[("claimed_evidence_boundary", "BoundaryFixture")],
            |ctx, _params| assert_legacy_complete_behavior(fixture(&ctx)?),
        ),
        step_def(
            "a begin state-entry request with claimed evidence:",
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
            ],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("claimed_evidence_boundary", "BoundaryFixture"),
            ],
            |mut ctx, params| {
                let claims = claims_from_table(
                    params
                        .data_table()
                        .ok_or("Expected claimed evidence table")?,
                )?;
                let request = anvil_engine::proto::BeginRequest {
                    artifact_type: "track".to_string(),
                    parent_id: PARENT_ID.to_string(),
                    track_name: "claimed evidence begin entry".to_string(),
                    approver: "P1-Approver".to_string(),
                    actor_name: "Claimed-Evidence-Begin-100000".to_string(),
                    actor_type: "agent".to_string(),
                    actor_model: "brine".to_string(),
                    actor_provider: "test".to_string(),
                    session_role: "creator".to_string(),
                    ..Default::default()
                };
                carry_fixture_context(
                    &mut ctx,
                    boundary_fixture(RequestKind::Begin, WireRequest::Begin(request), claims)?,
                )
            },
        ),
        async_step_def(
            "the begin state-entry request crosses the engine boundary",
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("claimed_evidence_boundary", "BoundaryFixture"),
            ],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("claimed_evidence_boundary", "BoundaryFixture"),
            ],
            |mut ctx, _params| async move {
                let mut fixture = take_fixture(&mut ctx, RequestKind::Begin)?;
                let request = match fixture.request.clone() {
                    WireRequest::Begin(request) => request,
                    _ => return Err("Expected begin request".into()),
                };
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let hearth = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let mut client = connect(engine.port).await?;
                match client.begin(anvil_test_support::surfaced(request)).await {
                    Ok(response) => {
                        let response = response.into_inner();
                        fixture.outcome.response_state = response.state.clone();
                        fixture.outcome.response_artifact_path = response.track_path.clone();
                        fixture.outcome.response_bytes = response.encode_to_vec();
                    }
                    Err(status) => {
                        fixture.outcome.rpc_error =
                            format!("{:?}: {}", status.code(), status.message())
                    }
                }
                if !fixture.outcome.response_artifact_path.is_empty() {
                    let artifact_dir = artifact_dir(&hearth, &fixture.outcome.response_artifact_path);
                    let status = std::fs::read_to_string(artifact_dir.join("status.yaml"))
                        .unwrap_or_default();
                    fixture.outcome.transition_bytes = transition_dir_record_containing(
                        &artifact_dir.join("transitions"),
                        "to: spec",
                    )?
                    .unwrap_or_default();
                    fixture.outcome.status_transition_recorded = artifact_dir.is_dir()
                        && status.contains("kind: track")
                        && status.contains("state: spec")
                        && !fixture.outcome.transition_bytes.is_empty();
                }
                if fixture.outcome.rpc_error.is_empty() {
                    fixture.outcome.measurement_bytes = wait_for_measurement_transition(
                        &hearth.join("step-measurement.jsonl"),
                        "spec",
                    )
                    .await?;
                }
                restore_fixture_context(ctx, engine, hearth, fixture)
            },
        ),
        check_def(
            "the begin request carries the claimed evidence in order",
            &[("claimed_evidence_boundary", "BoundaryFixture")],
            |ctx, _params| claims_match(&ctx, RequestKind::Begin),
        ),
        step_def(
            "a snapshot state-entry request with claimed evidence:",
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
            ],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("claimed_evidence_boundary", "BoundaryFixture"),
            ],
            |mut ctx, params| {
                let claims = claims_from_table(
                    params
                        .data_table()
                        .ok_or("Expected claimed evidence table")?,
                )?;
                let request = anvil_engine::proto::SnapshotRequest {
                    artifact_path: TRACK_PATH.to_string(),
                    to_state: "spec_review".to_string(),
                    actor_name: "Claimed-Evidence-Snapshot-100000".to_string(),
                    actor_role: "spec".to_string(),
                    actor_type: "agent".to_string(),
                    actor_model: "brine".to_string(),
                    actor_provider: "test".to_string(),
                    ..Default::default()
                };
                carry_fixture_context(
                    &mut ctx,
                    boundary_fixture(
                        RequestKind::Snapshot,
                        WireRequest::Snapshot(request),
                        claims,
                    )?,
                )
            },
        ),
        async_step_def(
            "the snapshot state-entry request crosses the engine boundary",
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("claimed_evidence_boundary", "BoundaryFixture"),
            ],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("claimed_evidence_boundary", "BoundaryFixture"),
            ],
            |mut ctx, _params| async move {
                let mut fixture = take_fixture(&mut ctx, RequestKind::Snapshot)?;
                let request = match fixture.request.clone() {
                    WireRequest::Snapshot(request) => request,
                    _ => return Err("Expected snapshot request".into()),
                };
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let hearth = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let mut client = connect(engine.port).await?;
                match client.snapshot(anvil_test_support::surfaced(request.clone())).await {
                    Ok(response) => {
                        if response.into_inner().success {
                            fixture.outcome.response_state = "spec_review".to_string();
                        } else {
                            fixture.outcome.rpc_error =
                                "Snapshot returned success=false".to_string();
                        }
                    }
                    Err(status) => {
                        fixture.outcome.rpc_error =
                            format!("{:?}: {}", status.code(), status.message())
                    }
                }

                let measurement_path = hearth.join("step-measurement.jsonl");
                let before_projection =
                    wait_for_measurement_transition(&measurement_path, "spec_review").await?;
                let mut projection = request;
                projection.artifact_path = "sparks/sparks.md".to_string();
                projection.to_state.clear();
                projection.projection_only = true;
                projection.event_type = "spark".to_string();
                match client.snapshot(anvil_test_support::surfaced(projection)).await {
                    Ok(response) => {
                        if !response.into_inner().success {
                            fixture.outcome.rpc_error =
                                "Projection-only snapshot returned success=false".to_string();
                        }
                    }
                    Err(status) => {
                        fixture.outcome.rpc_error =
                            format!("Projection-only {:?}: {}", status.code(), status.message())
                    }
                }
                let after_projection = wait_for_stable_measurement_bytes(
                    &measurement_path,
                    &before_projection,
                    MEASUREMENT_STABILITY_WINDOW,
                )
                .await?;
                fixture.outcome.measurement_bytes = after_projection.clone();
                fixture.outcome.projection_measurement_unchanged =
                    before_projection == after_projection;
                fixture.outcome.projection_persisted_claim =
                    tree_contains_claim(&hearth, &fixture.expected_claims)?;
                restore_fixture_context(ctx, engine, hearth, fixture)
            },
        ),
        check_def(
            "the snapshot request carries the claimed evidence in order",
            &[("claimed_evidence_boundary", "BoundaryFixture")],
            |ctx, _params| claims_match(&ctx, RequestKind::Snapshot),
        ),
        check_def(
            "a projection-only snapshot does not assess or persist claimed evidence",
            &[("claimed_evidence_boundary", "BoundaryFixture")],
            |ctx, _params| {
                let fixture = fixture(&ctx)?;
                if fixture.outcome.rpc_error.is_empty()
                    && fixture.outcome.projection_measurement_unchanged
                    && !fixture.outcome.projection_persisted_claim
                {
                    Ok(())
                } else {
                    Err(format!(
                        "Projection-only result error='{}', measurement_unchanged={}, persisted_claim={}",
                        fixture.outcome.rpc_error,
                        fixture.outcome.projection_measurement_unchanged,
                        fixture.outcome.projection_persisted_claim
                    ))
                }
            },
        ),
    ]
}

fn claims_from_table(table: &DataTable) -> Result<Vec<ExpectedClaim>, String> {
    let class_idx = table
        .headers
        .iter()
        .position(|header| header == "class")
        .ok_or("Missing class column")?;
    let reference_idx = table
        .headers
        .iter()
        .position(|header| header == "reference")
        .ok_or("Missing reference column")?;
    table
        .rows
        .iter()
        .map(|row| {
            Ok(ExpectedClaim {
                class: row.get(class_idx).ok_or("Missing class cell")?.to_string(),
                reference: row
                    .get(reference_idx)
                    .ok_or("Missing reference cell")?
                    .to_string(),
            })
        })
        .collect()
}

fn boundary_fixture(
    kind: RequestKind,
    request: WireRequest,
    expected_claims: Vec<ExpectedClaim>,
) -> Result<BoundaryFixture, String> {
    let field_number = match kind {
        RequestKind::Complete => 17,
        RequestKind::Begin => 30,
        RequestKind::Snapshot => 18,
    };
    let mut raw_bytes = encode_request(&request);
    for claim in &expected_claims {
        let mut encoded_claim = Vec::new();
        append_proto_string(&mut encoded_claim, 1, &claim.class);
        append_proto_string(&mut encoded_claim, 2, &claim.reference);
        append_proto_message(&mut raw_bytes, field_number, &encoded_claim);
    }
    let decoded = decode_request(kind, &raw_bytes)?;
    let round_trip_bytes = encode_request(&decoded);
    let mapped_claims = map_decoded_claims(&decoded)?;

    Ok(BoundaryFixture {
        kind,
        request: decoded,
        expected_claims,
        mapped_claims,
        raw_bytes,
        round_trip_bytes,
        legacy_wire_baseline: Vec::new(),
        outcome: BoundaryOutcome::default(),
    })
}

/// Encode the fields that existed before P1 appended `claimed_evidence = 17`.
/// The legacy fixture intentionally populates only tags 1-5, so this independent
/// encoder pins the old wire bytes without asking the newly generated message
/// type to prove itself.
fn legacy_complete_wire_baseline(request: &anvil_engine::proto::CompleteRequest) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (field_number, value) in [
        (1, request.artifact_path.as_str()),
        (2, request.actor_name.as_str()),
        (3, request.actor_type.as_str()),
        (4, request.actor_model.as_str()),
        (5, request.actor_provider.as_str()),
    ] {
        if !value.is_empty() {
            append_proto_string(&mut bytes, field_number, value);
        }
    }
    bytes
}

fn map_decoded_claims(request: &WireRequest) -> Result<Vec<ExpectedClaim>, String> {
    let claims = match request {
        WireRequest::Complete(request) => request.claimed_evidence.clone(),
        WireRequest::Begin(request) => request.claimed_evidence.clone(),
        WireRequest::Snapshot(request) => request.claimed_evidence.clone(),
    };
    anvil_engine::claimed_evidence::claimed_evidence_to_domain(claims)?
        .into_iter()
        .map(|claim| {
            let class = match claim.class {
                anvil_core::domain::playbook::types::EvidenceClass::ArtifactOfConsequence => {
                    "artifact_of_consequence"
                }
                anvil_core::domain::playbook::types::EvidenceClass::VerifiableCitation => {
                    "verifiable_citation"
                }
                anvil_core::domain::playbook::types::EvidenceClass::SelfDescription => {
                    "self_description"
                }
            };
            Ok(ExpectedClaim {
                class: class.to_string(),
                reference: claim.reference,
            })
        })
        .collect()
}

fn encode_request(request: &WireRequest) -> Vec<u8> {
    match request {
        WireRequest::Complete(request) => request.encode_to_vec(),
        WireRequest::Begin(request) => request.encode_to_vec(),
        WireRequest::Snapshot(request) => request.encode_to_vec(),
    }
}

fn decode_request(kind: RequestKind, bytes: &[u8]) -> Result<WireRequest, String> {
    match kind {
        RequestKind::Complete => anvil_engine::proto::CompleteRequest::decode(bytes)
            .map(WireRequest::Complete)
            .map_err(|error| format!("decode complete request: {}", error)),
        RequestKind::Begin => anvil_engine::proto::BeginRequest::decode(bytes)
            .map(WireRequest::Begin)
            .map_err(|error| format!("decode begin request: {}", error)),
        RequestKind::Snapshot => anvil_engine::proto::SnapshotRequest::decode(bytes)
            .map(WireRequest::Snapshot)
            .map_err(|error| format!("decode snapshot request: {}", error)),
    }
}

fn append_proto_string(bytes: &mut Vec<u8>, field_number: u32, value: &str) {
    append_proto_message(bytes, field_number, value.as_bytes());
}

fn append_proto_message(bytes: &mut Vec<u8>, field_number: u32, value: &[u8]) {
    append_proto_varint(bytes, u64::from((field_number << 3) | 2));
    append_proto_varint(bytes, value.len() as u64);
    bytes.extend_from_slice(value);
}

fn append_proto_varint(bytes: &mut Vec<u8>, mut value: u64) {
    loop {
        if value < 0x80 {
            bytes.push(value as u8);
            return;
        }
        bytes.push((value as u8) | 0x80);
        value >>= 7;
    }
}

fn carry_fixture_context(ctx: &mut Context, fixture: BoundaryFixture) -> Result<Context, String> {
    let engine = ctx
        .take::<EngineProcess>("engine_process")
        .ok_or("No engine_process")?;
    let hearth = ctx
        .get::<PathBuf>("hearth_path")
        .ok_or("No hearth_path")?
        .clone();
    restore_fixture_context(Context::new(), engine, hearth, fixture)
}

fn take_fixture(ctx: &mut Context, expected_kind: RequestKind) -> Result<BoundaryFixture, String> {
    let fixture = ctx
        .take::<BoundaryFixture>("claimed_evidence_boundary")
        .ok_or("No claimed evidence boundary fixture")?;
    if fixture.kind == expected_kind {
        Ok(fixture)
    } else {
        Err(format!(
            "Expected {:?} boundary fixture, got {:?}",
            expected_kind, fixture.kind
        ))
    }
}

fn restore_fixture_context(
    _ctx: Context,
    engine: EngineProcess,
    hearth: PathBuf,
    fixture: BoundaryFixture,
) -> Result<Context, String> {
    let mut out = Context::new();
    out.set("engine_process", engine);
    out.set("hearth_path", hearth);
    out.set("claimed_evidence_boundary", fixture);
    Ok(out)
}

fn fixture(ctx: &Context) -> Result<&BoundaryFixture, String> {
    ctx.get::<BoundaryFixture>("claimed_evidence_boundary")
        .ok_or_else(|| "No claimed evidence boundary fixture".to_string())
}

fn claims_match(ctx: &Context, expected_kind: RequestKind) -> Result<(), String> {
    let fixture = fixture(ctx)?;
    if fixture.kind != expected_kind {
        return Err(format!(
            "Expected {:?} fixture, got {:?}",
            expected_kind, fixture.kind
        ));
    }
    if fixture.mapped_claims == fixture.expected_claims
        && fixture.raw_bytes == fixture.round_trip_bytes
        && fixture.outcome.rpc_error.is_empty()
        && !fixture.outcome.response_state.is_empty()
    {
        Ok(())
    } else {
        Err(format!(
            "Expected ordered claims {:?}, got {:?}; wire_preserved={}; response_state='{}'; rpc_error='{}'",
            fixture.expected_claims,
            fixture.mapped_claims,
            fixture.raw_bytes == fixture.round_trip_bytes,
            fixture.outcome.response_state,
            fixture.outcome.rpc_error
        ))
    }
}

fn assert_legacy_complete_behavior(fixture: &BoundaryFixture) -> Result<(), String> {
    let measurement = String::from_utf8(fixture.outcome.measurement_bytes.clone())
        .map_err(|error| format!("step-measurement is not UTF-8: {}", error))?;
    let line = measurement
        .lines()
        .find(|line| !line.trim().is_empty())
        .ok_or("No pre-evidence step-measurement record")?;
    let value = serde_json::from_str::<serde_json::Value>(line)
        .map_err(|error| format!("parse pre-evidence measurement: {}", error))?;
    let keys = value
        .as_object()
        .ok_or("Pre-evidence measurement is not an object")?
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    let allowed = [
        "actor_hash",
        "at",
        "conversation_hash",
        "expected_output_present",
        "from_state",
        "intent_present",
        "kind",
        "project_label",
        "role",
        "to_state",
        "playbook_run_id",
        "artifact_kind",
    ]
    .into_iter()
    .map(ToString::to_string)
    .collect::<BTreeSet<_>>();
    let unexpected = keys.difference(&allowed).cloned().collect::<Vec<_>>();
    if fixture.outcome.rpc_error.is_empty()
        && fixture.outcome.response_state == "spec_review"
        && fixture.outcome.status_transition_recorded
        && unexpected.is_empty()
        && !measurement.contains("\"claimed_evidence\"")
    {
        Ok(())
    } else {
        Err(format!(
            "Legacy completion changed: state='{}', transition={}, rpc_error='{}', unexpected measurement keys={:?}, measurement={}",
            fixture.outcome.response_state,
            fixture.outcome.status_transition_recorded,
            fixture.outcome.rpc_error,
            unexpected,
            measurement
        ))
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
    .map_err(|error| format!("connect claimed evidence RPC: {}", error))
}

fn parse_measurement_rows(bytes: &[u8]) -> Result<Vec<serde_json::Value>, String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| format!("step-measurement sink is not UTF-8: {}", error))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .map_err(|error| format!("parse step-measurement row '{}': {}", line, error))
        })
        .collect()
}

async fn wait_for_measurement_transition(path: &Path, to_state: &str) -> Result<Vec<u8>, String> {
    let deadline = Instant::now() + MEASUREMENT_POLL_TIMEOUT;
    let mut last_observation: String;

    loop {
        match std::fs::read(path) {
            Ok(bytes) => match parse_measurement_rows(&bytes) {
                Ok(rows)
                    if rows.iter().any(|row| {
                        row.get("to_state").and_then(serde_json::Value::as_str) == Some(to_state)
                    }) =>
                {
                    return Ok(bytes)
                }
                Ok(rows) => {
                    last_observation = format!("{} parsed record(s): {:?}", rows.len(), rows);
                }
                Err(error) => last_observation = error,
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                last_observation = "sink was absent".to_string();
            }
            Err(error) => last_observation = format!("read failed: {}", error),
        }

        let now = Instant::now();
        if now >= deadline {
            return Err(format!(
                "step-measurement sink {} did not contain a transition to '{}' within {:?}: {}",
                path.display(),
                to_state,
                MEASUREMENT_POLL_TIMEOUT,
                last_observation
            ));
        }
        tokio::time::sleep(MEASUREMENT_POLL_INTERVAL.min(deadline - now)).await;
    }
}

async fn wait_for_stable_measurement_bytes(
    path: &Path,
    expected: &[u8],
    stability_window: Duration,
) -> Result<Vec<u8>, String> {
    let deadline = Instant::now() + stability_window;
    loop {
        let current = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                return Err(format!(
                    "read step-measurement sink {} during stability check: {}",
                    path.display(),
                    error
                ))
            }
        };
        if current != expected {
            return Err(format!(
                "step-measurement sink {} changed during the {:?} post-projection stability window; before={} after={}",
                path.display(),
                stability_window,
                String::from_utf8_lossy(expected),
                String::from_utf8_lossy(&current)
            ));
        }

        let now = Instant::now();
        if now >= deadline {
            return Ok(current);
        }
        tokio::time::sleep(MEASUREMENT_POLL_INTERVAL.min(deadline - now)).await;
    }
}

fn tree_contains_claim(root: &Path, claims: &[ExpectedClaim]) -> Result<bool, String> {
    fn visit(path: &Path, needles: &[&str]) -> Result<bool, String> {
        for entry in std::fs::read_dir(path)
            .map_err(|error| format!("read durable tree {}: {}", path.display(), error))?
        {
            let entry = entry.map_err(|error| error.to_string())?;
            let file_type = entry.file_type().map_err(|error| error.to_string())?;
            if file_type.is_dir() {
                if visit(&entry.path(), needles)? {
                    return Ok(true);
                }
            } else if file_type.is_file() {
                let bytes = std::fs::read(entry.path()).map_err(|error| error.to_string())?;
                let text = String::from_utf8_lossy(&bytes);
                if needles.iter().any(|needle| text.contains(needle)) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    let needles = claims
        .iter()
        .map(|claim| claim.reference.as_str())
        .collect::<Vec<_>>();
    visit(root, &needles)
}

fn artifact_dir(hearth: &Path, artifact_path: &str) -> PathBuf {
    let path = Path::new(artifact_path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        hearth.join(path)
    }
}

fn transition_dir_record_containing(path: &Path, needle: &str) -> Result<Option<Vec<u8>>, String> {
    let mut entries = std::fs::read_dir(path)
        .map_err(|error| format!("read transition directory {}: {}", path.display(), error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_file()
        {
            let bytes = std::fs::read(entry.path())
                .map_err(|error| format!("read transition event: {}", error))?;
            if String::from_utf8_lossy(&bytes).contains(needle) {
                return Ok(Some(bytes));
            }
        }
    }
    Ok(None)
}
