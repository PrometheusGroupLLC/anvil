//! Deterministic in-process proof for the P2b non-blocking writer boundary.
//!
//! The real private `AnvilServer` runs over tonic. Only its durable
//! `StepMeasurementWritePort` is replaced with a Condvar-controlled parking
//! writer, so every lifecycle assertion crosses the production RPC handler and
//! dispatcher while durable delivery is paused without timing sleeps.

use super::production_engine::{
    spawn_test_server_with_step_measurement_writer, TestAnvilServerHandle,
};
use anvil_core::ports::step_measurement_port::{
    StepMeasurementError, StepMeasurementRecord, StepMeasurementWritePort,
};
use anvil_test_support::{
    async_step_def, carry_retained_temp_dir, check_def, retained_temp_dir, step_def, Context,
    RetainedTempDir, StepDef,
};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const KIND: &str = "evidence_fail_open_probe";
const PLAYBOOK_ID: &str = "p2b_evidence_fail_open_probe";
const ARTIFACT_PATH: &str = "evidence_fail_open_runs/p2b-in-process-seeded-run";
const ACTOR: &str = "Evidence-P2b-InProcess-100000";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Leg {
    Begin,
    Snapshot,
    Complete,
}

#[derive(Debug, Clone, Default)]
struct LifecycleOutcome {
    returned: bool,
    state: String,
    artifact_path: String,
    error: String,
}

#[derive(Default)]
struct ParkingState {
    append_entered: bool,
    released: bool,
    records: Vec<StepMeasurementRecord>,
}

#[derive(Default)]
struct ParkingWriter {
    state: Mutex<ParkingState>,
    changed: Condvar,
}

impl ParkingWriter {
    fn wait_until_parked(&self, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        let mut state = self
            .state
            .lock()
            .map_err(|_| "parking writer lock was poisoned".to_string())?;
        while !state.append_entered {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("durable evidence writer was never entered".to_string());
            }
            let (next, waited) = self
                .changed
                .wait_timeout(state, remaining)
                .map_err(|_| "parking writer wait was poisoned".to_string())?;
            state = next;
            if waited.timed_out() && !state.append_entered {
                return Err("durable evidence writer was never entered".to_string());
            }
        }
        if state.released {
            return Err("durable evidence writer was released before the assertion".to_string());
        }
        if !state.records.is_empty() {
            return Err("durable evidence was recorded before release".to_string());
        }
        Ok(())
    }

    fn release(&self) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "parking writer lock was poisoned".to_string())?;
        state.released = true;
        self.changed.notify_all();
        Ok(())
    }

    fn one_record(&self, timeout: Duration) -> Result<StepMeasurementRecord, String> {
        let deadline = Instant::now() + timeout;
        let mut state = self
            .state
            .lock()
            .map_err(|_| "parking writer lock was poisoned".to_string())?;
        while state.records.is_empty() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("durable evidence row did not arrive after release".to_string());
            }
            let (next, waited) = self
                .changed
                .wait_timeout(state, remaining)
                .map_err(|_| "parking writer wait was poisoned".to_string())?;
            state = next;
            if waited.timed_out() && state.records.is_empty() {
                return Err("durable evidence row did not arrive after release".to_string());
            }
        }
        if state.records.len() != 1 {
            return Err(format!(
                "expected exactly one delivered evidence row, got {}",
                state.records.len()
            ));
        }

        // Observe a bounded quiet window after the first delivery. A duplicate
        // append notifies this Condvar and fails immediately; timeout with one
        // row establishes the at-most-once side without a fixed sleep.
        let stable_until = Instant::now() + Duration::from_millis(100);
        while state.records.len() == 1 && Instant::now() < stable_until {
            let remaining = stable_until.saturating_duration_since(Instant::now());
            let (next, _) = self
                .changed
                .wait_timeout(state, remaining)
                .map_err(|_| "parking writer stability wait was poisoned".to_string())?;
            state = next;
        }
        if state.records.len() != 1 {
            return Err(format!(
                "expected at-most-once evidence delivery, got {} rows",
                state.records.len()
            ));
        }
        Ok(state.records[0].clone())
    }
}

impl StepMeasurementWritePort for ParkingWriter {
    fn append_step_measurement(
        &self,
        record: &StepMeasurementRecord,
    ) -> Result<(), StepMeasurementError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| StepMeasurementError::IoError {
                message: "parking writer lock was poisoned".to_string(),
            })?;
        state.append_entered = true;
        self.changed.notify_all();
        while !state.released {
            state = self
                .changed
                .wait(state)
                .map_err(|_| StepMeasurementError::IoError {
                    message: "parking writer wait was poisoned".to_string(),
                })?;
        }
        state.records.push(record.clone());
        self.changed.notify_all();
        Ok(())
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        async_step_def(
            "an obligated playbook is ready for a {string} transition with durable evidence delivery paused",
            &[],
            &[
                ("p2b_in_process_server", "TestAnvilServerHandle"),
                ("p2b_in_process_writer", "Arc<ParkingWriter>"),
                ("p2b_in_process_leg", "Leg"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| async move {
                let leg = parse_leg(params.get_string(0).ok_or("Expected lifecycle leg")?)?;
                let (handle, hearth) = retained_temp_dir("anvil-p2b-in-process-")?;
                seed_hearth(&hearth)?;
                if leg != Leg::Begin {
                    seed_artifact(&hearth)?;
                }
                let writer = Arc::new(ParkingWriter::default());
                let server = spawn_test_server_with_step_measurement_writer(
                    hearth.clone(),
                    writer.clone(),
                )
                .await?;

                let mut out = Context::new();
                out.set("p2b_in_process_server", server);
                out.set("p2b_in_process_writer", writer);
                out.set("p2b_in_process_leg", leg);
                out.set("hearth_path", hearth);
                out.set::<RetainedTempDir>("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        async_step_def(
            "the {string} transition is requested before durable evidence delivery resumes",
            &in_process_inputs(),
            &in_process_outputs_with_outcome(),
            |mut ctx, params| async move {
                let requested = parse_leg(params.get_string(0).ok_or("Expected lifecycle leg")?)?;
                let configured = *ctx
                    .get::<Leg>("p2b_in_process_leg")
                    .ok_or("No configured in-process lifecycle leg")?;
                if requested != configured {
                    return Err(format!(
                        "requested {:?} but the fixture was prepared for {:?}",
                        requested, configured
                    ));
                }
                let server = take_server(&mut ctx)?;
                let writer = take_writer(&mut ctx)?;
                let hearth = hearth(&ctx)?;
                let outcome = tokio::time::timeout(
                    Duration::from_secs(2),
                    call_leg(server.port(), requested),
                )
                .await
                .unwrap_or_else(|_| LifecycleOutcome {
                    artifact_path: if requested == Leg::Begin {
                        String::new()
                    } else {
                        ARTIFACT_PATH.to_string()
                    },
                    error: format!("{:?} RPC did not return while delivery was paused", requested),
                    ..Default::default()
                });
                writer.wait_until_parked(Duration::from_secs(2))?;
                restore_context(&ctx, server, writer, hearth, configured, Some(outcome))
            },
        ),
        check_def(
            "the transition returns in state {string} while durable evidence delivery remains paused",
            &[
                ("p2b_in_process_writer", "Arc<ParkingWriter>"),
                ("p2b_in_process_outcome", "LifecycleOutcome"),
            ],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected response state")?;
                writer(&ctx)?.wait_until_parked(Duration::from_secs(2))?;
                assert_outcome(outcome(&ctx)?, expected)
            },
        ),
        check_def(
            "state {string} is durably visible before evidence delivery resumes",
            &[
                ("hearth_path", "PathBuf"),
                ("p2b_in_process_writer", "Arc<ParkingWriter>"),
                ("p2b_in_process_outcome", "LifecycleOutcome"),
            ],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected persisted state")?;
                writer(&ctx)?.wait_until_parked(Duration::from_secs(2))?;
                let result = outcome(&ctx)?;
                assert_state(&hearth(&ctx)?, &result.artifact_path, expected)
            },
        ),
        step_def(
            "durable evidence delivery resumes",
            &in_process_inputs_with_outcome(),
            &in_process_outputs_with_outcome(),
            |mut ctx, _params| {
                let server = take_server(&mut ctx)?;
                let writer = take_writer(&mut ctx)?;
                let hearth = hearth(&ctx)?;
                let leg = *ctx
                    .get::<Leg>("p2b_in_process_leg")
                    .ok_or("No configured in-process lifecycle leg")?;
                let outcome = ctx
                    .take::<LifecycleOutcome>("p2b_in_process_outcome")
                    .ok_or("No in-process lifecycle outcome")?;
                writer.release()?;
                restore_context(&ctx, server, writer, hearth, leg, Some(outcome))
            },
        ),
        check_def(
            "exactly one delivered evidence row records {string} to {string}",
            &[("p2b_in_process_writer", "Arc<ParkingWriter>")],
            |ctx, params| {
                let from = params.get_string(0).ok_or("Expected from state")?;
                let to = params.get_string(1).ok_or("Expected to state")?;
                let row = writer(&ctx)?.one_record(Duration::from_secs(2))?;
                if row.from_state != from || row.to_state != to {
                    return Err(format!(
                        "expected one delivered row {} -> {}, got {} -> {}",
                        from, to, row.from_state, row.to_state
                    ));
                }
                let evidence = row
                    .evidence
                    .ok_or("delivered row had no evidence assessment")?;
                if evidence.assessment.status.as_str() != "present-as-claimed" {
                    return Err(format!(
                        "expected present-as-claimed evidence, got {}",
                        evidence.assessment.status.as_str()
                    ));
                }
                Ok(())
            },
        ),
    ]
}

fn in_process_inputs() -> Vec<(&'static str, &'static str)> {
    vec![
        ("p2b_in_process_server", "TestAnvilServerHandle"),
        ("p2b_in_process_writer", "Arc<ParkingWriter>"),
        ("p2b_in_process_leg", "Leg"),
        ("hearth_path", "PathBuf"),
    ]
}

fn in_process_inputs_with_outcome() -> Vec<(&'static str, &'static str)> {
    let mut values = in_process_inputs();
    values.push(("p2b_in_process_outcome", "LifecycleOutcome"));
    values
}

fn in_process_outputs_with_outcome() -> Vec<(&'static str, &'static str)> {
    let mut values = in_process_inputs_with_outcome();
    values.push(("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"));
    values
}

fn restore_context(
    ctx: &Context,
    server: TestAnvilServerHandle,
    writer: Arc<ParkingWriter>,
    hearth: PathBuf,
    leg: Leg,
    outcome: Option<LifecycleOutcome>,
) -> Result<Context, String> {
    let mut out = Context::new();
    out.set("p2b_in_process_server", server);
    out.set("p2b_in_process_writer", writer);
    out.set("p2b_in_process_leg", leg);
    out.set("hearth_path", hearth);
    carry_retained_temp_dir(ctx, &mut out, "hearth_path_handle");
    if let Some(outcome) = outcome {
        out.set("p2b_in_process_outcome", outcome);
    }
    Ok(out)
}

fn parse_leg(value: &str) -> Result<Leg, String> {
    match value {
        "begin" => Ok(Leg::Begin),
        "snapshot" => Ok(Leg::Snapshot),
        "complete" => Ok(Leg::Complete),
        other => Err(format!("unknown P2b lifecycle leg '{}'", other)),
    }
}

async fn call_leg(port: u16, leg: Leg) -> LifecycleOutcome {
    match leg {
        Leg::Begin => call_begin(port).await,
        Leg::Snapshot => call_snapshot(port).await,
        Leg::Complete => call_complete(port).await,
    }
}

async fn call_begin(port: u16) -> LifecycleOutcome {
    let mut client = match connect(port).await {
        Ok(client) => client,
        Err(error) => return failed(error, ""),
    };
    let request = anvil_engine::proto::BeginRequest {
        artifact_type: KIND.to_string(),
        track_name: "P2b in-process begin".to_string(),
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
                "begin RPC failed ({:?}): {}",
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
                failed("snapshot RPC returned success=false", ARTIFACT_PATH)
            }
        }
        Err(status) => failed(
            format!(
                "snapshot RPC failed ({:?}): {}",
                status.code(),
                status.message()
            ),
            ARTIFACT_PATH,
        ),
    }
}

async fn call_complete(port: u16) -> LifecycleOutcome {
    let mut client = match connect(port).await {
        Ok(client) => client,
        Err(error) => return failed(error, ARTIFACT_PATH),
    };
    let request = anvil_engine::proto::CompleteRequest {
        artifact_path: ARTIFACT_PATH.to_string(),
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
                artifact_path: ARTIFACT_PATH.to_string(),
                error: String::new(),
            }
        }
        Err(status) => failed(
            format!(
                "complete RPC failed ({:?}): {}",
                status.code(),
                status.message()
            ),
            ARTIFACT_PATH,
        ),
    }
}

fn claim() -> anvil_engine::proto::ClaimedEvidence {
    anvil_engine::proto::ClaimedEvidence {
        class: "artifact_of_consequence".to_string(),
        reference: "opaque:p2b:in-process-output".to_string(),
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
    .map_err(|error| format!("connect to in-process P2b engine: {}", error))
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
            "expected lifecycle RPC to return in state '{}', got returned={} state='{}' error='{}'",
            expected, result.returned, result.state, result.error
        ))
    }
}

fn assert_state(hearth: &Path, artifact_path: &str, expected: &str) -> Result<(), String> {
    if artifact_path.is_empty() {
        return Err("lifecycle result did not identify a transitioned artifact".to_string());
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
            .ok_or_else(|| format!("no resolvable state for {}", artifact_dir.display()))?;
    if state == expected {
        Ok(())
    } else {
        Err(format!(
            "expected persisted state '{}' at {}, got '{}'",
            expected,
            status_path.display(),
            state
        ))
    }
}

fn take_server(ctx: &mut Context) -> Result<TestAnvilServerHandle, String> {
    ctx.take::<TestAnvilServerHandle>("p2b_in_process_server")
        .ok_or_else(|| "no in-process P2b server".to_string())
}

fn writer(ctx: &Context) -> Result<&Arc<ParkingWriter>, String> {
    ctx.get::<Arc<ParkingWriter>>("p2b_in_process_writer")
        .ok_or_else(|| "no in-process P2b parking writer".to_string())
}

fn take_writer(ctx: &mut Context) -> Result<Arc<ParkingWriter>, String> {
    ctx.take::<Arc<ParkingWriter>>("p2b_in_process_writer")
        .ok_or_else(|| "no in-process P2b parking writer".to_string())
}

fn hearth(ctx: &Context) -> Result<PathBuf, String> {
    ctx.get::<PathBuf>("hearth_path")
        .cloned()
        .ok_or_else(|| "no in-process P2b hearth".to_string())
}

fn outcome(ctx: &Context) -> Result<&LifecycleOutcome, String> {
    ctx.get::<LifecycleOutcome>("p2b_in_process_outcome")
        .ok_or_else(|| "no in-process P2b lifecycle outcome".to_string())
}

fn seed_hearth(hearth: &Path) -> Result<(), String> {
    std::fs::create_dir_all(hearth.join("evidence_fail_open_runs"))
        .map_err(|error| format!("create P2b artifact root: {}", error))?;
    std::fs::write(
        hearth.join("evidence_fail_open_runs.md"),
        "# Evidence Fail-open Runs\n\n## spec\n\n## spec_review\n\n## completed\n",
    )
    .map_err(|error| format!("write P2b registry: {}", error))?;
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
            "version: 1\nkind: {kind}\nstate: spec\ntransitions:\n  - to: spec\n    at: \"2026-07-19T00:00:00Z\"\n    actor: Seed-P2b-InProcess-000001\n    role: doer\n",
            kind = KIND,
        ),
    )
    .map_err(|error| format!("write P2b seeded status: {}", error))?;
    std::fs::write(
        artifact.join("spec.md"),
        "# P2b in-process fail-open probe\n",
    )
    .map_err(|error| format!("write P2b seeded artifact: {}", error))?;
    Ok(())
}

const MACHINE_YAML: &str = r#"kind: evidence_fail_open_probe
directory: evidence_fail_open_runs
registry: evidence_fail_open_runs.md
description: "P2b in-process evidence writer fail-open probe."
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
