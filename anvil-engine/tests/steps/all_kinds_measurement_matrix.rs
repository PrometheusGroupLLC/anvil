use super::production_engine::{
    spawn_test_server_with_step_measurement_writer, TestAnvilServerHandle,
};
use anvil_core::domain::playbook::registry::{PlaybookRegistry, SeedPlaybookRegistry};
use anvil_core::domain::playbook::types::PlaybookMachine;
use anvil_core_hearth::fs_step_measurement_adapter::FileSystemStepMeasurementAdapter;
use anvil_core::ports::step_measurement_port::{
    StepMeasurementError, StepMeasurementReadPort, StepMeasurementRecord, StepMeasurementWritePort,
};
use anvil_core::ports::transition_measurement_port::TransitionMeasurementReadPort;
use anvil_core::ports::playbook_measurement_port::PlaybookMeasurementReadPort;
use anvil_test_support::{
    async_step_def, check_def, retained_temp_dir, step_def, Context, RetainedTempDir, StepDef,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

const HEARTH_KEY: &str = "matrix_hearth";
const HANDLE_KEY: &str = "matrix_hearth_handle";
const SERVER_KEY: &str = "matrix_server";
const REGISTERED_KEY: &str = "matrix_registered";
const REPORT_KEY: &str = "matrix_report";
const FAILURE_KEY: &str = "matrix_failure";
const PROJECTION_COUNTS_KEY: &str = "matrix_projection_counts";
const PROJECTION_REASON: &str = "projection-only snapshots are outside the measurement boundary";
// K8 mints a backlog_item's genesis transition event through Begin (routing.rs:
// "backlog_item genesis is engine-supported"). Its store then REFUSES any item
// without an authoritative genesis event, so the matrix's generic file-seeding
// cannot construct a measurable instance by design — not an omission, a different
// genesis protocol. Measuring it must drive the real begin path; tracked separately.
const ENGINE_MINTED_REASON: &str =
    "engine-minted genesis: the store refuses a file-seeded instance, so this kind must be \
     measured through its real begin path";

/// The kinds the matrix can actually drive to a measured terminal snapshot.
/// `spark` is projection-only; `backlog_item`'s genesis is engine-minted and its
/// store refuses a file-seeded instance. Both are reported as SKIPPED with an
/// asserted reason. Derived once — the exercise loops and the expected measured
/// count both read this, so they cannot disagree.
fn is_matrix_exercisable(kind: &str) -> bool {
    kind != "spark" && kind != "backlog_item"
}

fn expected_measured_kinds(registered: &BTreeSet<String>) -> usize {
    registered.iter().filter(|k| is_matrix_exercisable(k)).count()
}

#[derive(Clone, Debug, Default)]
struct Coverage {
    step: bool,
    transition: bool,
    workflow: bool,
}

#[derive(Clone, Debug, Default)]
struct CoverageReport {
    registered: BTreeSet<String>,
    covered: BTreeMap<String, Coverage>,
    skipped: BTreeMap<String, String>,
}

impl CoverageReport {
    fn evaluate(&self) -> Result<(), String> {
        let accounted: BTreeSet<String> = self
            .covered
            .keys()
            .chain(self.skipped.keys())
            .cloned()
            .collect();
        let omitted: Vec<String> = self.registered.difference(&accounted).cloned().collect();
        let unexpected: Vec<String> = accounted.difference(&self.registered).cloned().collect();
        let mut failures = Vec::new();
        if !omitted.is_empty() {
            failures.push(format!(
                "silently omitted registered kinds: {}",
                omitted.join(", ")
            ));
        }
        if !unexpected.is_empty() {
            failures.push(format!(
                "unregistered reported kinds: {}",
                unexpected.join(", ")
            ));
        }
        for (kind, coverage) in &self.covered {
            let missing = missing_granularities(coverage);
            if !missing.is_empty() {
                failures.push(format!(
                    "kind '{}' missing granularities: {}",
                    kind,
                    missing.join(", ")
                ));
            }
        }
        for (kind, reason) in &self.skipped {
            if reason.trim().is_empty() {
                failures.push(format!(
                    "kind '{}' skipped without an asserted reason",
                    kind
                ));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }
}

struct HearthStepWriter {
    adapter: FileSystemStepMeasurementAdapter,
}

impl HearthStepWriter {
    fn new(hearth: &Path) -> Self {
        Self {
            adapter: FileSystemStepMeasurementAdapter::new(hearth),
        }
    }
}

impl StepMeasurementWritePort for HearthStepWriter {
    fn append_step_measurement(
        &self,
        record: &StepMeasurementRecord,
    ) -> Result<(), StepMeasurementError> {
        self.adapter.append_step_measurement(record)
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        async_step_def(
            "the live registered-kind measurement matrix",
            &[],
            &[
                (HEARTH_KEY, "PathBuf"),
                (HANDLE_KEY, "RetainedTempDir"),
                (SERVER_KEY, "TestAnvilServerHandle"),
                (REGISTERED_KEY, "BTreeSet<String>"),
            ],
            |_, _| async move {
                let (handle, hearth) = retained_temp_dir("anvil-measurement-matrix-")?;
                let registry = SeedPlaybookRegistry;
                let registered: BTreeSet<String> = registry.kinds().into_iter().collect();
                // PARITY GUARD. The matrix asserts "no registered kind is silently
                // omitted", but `SeedPlaybookRegistry`'s entries are HARD-CODED
                // (registry.rs:690) while the kit actually ships whatever is in
                // `playbooks/*/machine.yaml` (build-kit.sh:340). Those are two
                // hand-maintained copies of one list, so they drift: add a 9th playbook
                // directory and the matrix stays green while never measuring it. Its
                // population would be correct only by coincidence.
                //
                // This is the fourth instance of this exact defect found in one day — the
                // Codex plugin version deadlocked CI, a kit version literal broke the core
                // suite, and a hand-derived 24-row list should have been 26. Each was fixed
                // by deriving rather than duplicating. Here the two lists must stay
                // independent (the seed registry is compiled in; the directory is shipped),
                // so the guard asserts they AGREE instead.
                assert_seed_registry_matches_shipped_playbooks(&registered)?;
                seed_matrix_hearth(&hearth, &registry)?;
                let writer = Arc::new(HearthStepWriter::new(&hearth));
                let server =
                    spawn_test_server_with_step_measurement_writer(hearth.clone(), writer).await?;
                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth);
                out.set::<RetainedTempDir>(HANDLE_KEY, handle);
                out.set(SERVER_KEY, server);
                out.set(REGISTERED_KEY, registered);
                Ok(out)
            },
        ),
        async_step_def(
            "every registered kind is exercised across its measurement boundary",
            &matrix_inputs(),
            &matrix_outputs_with_report(),
            |mut ctx, _| async move {
                let hearth = matrix_hearth(&ctx)?;
                let registered = ctx
                    .get::<BTreeSet<String>>(REGISTERED_KEY)
                    .ok_or("No registered kinds")?
                    .clone();
                let server = take_server(&mut ctx)?;
                let registry = SeedPlaybookRegistry;
                for machine in registry.all_machines() {
                    if machine.projection_only || !is_matrix_exercisable(&machine.kind) {
                        continue;
                    }
                    exercise_terminal_snapshot(server.port(), machine).await?;
                }
                wait_for_step_kinds(&hearth, expected_measured_kinds(&registered))?;
                let mut report = read_report(&hearth, registered)?;
                if let Ok(kind) = std::env::var("ANVIL_MEASUREMENT_MATRIX_MUTATE_KIND") {
                    report.covered.insert(kind, Coverage::default());
                }
                report.evaluate()?;
                let mut out = carry_matrix_context(&mut ctx)?;
                out.set(SERVER_KEY, server);
                out.set(REPORT_KEY, report);
                Ok(out)
            },
        ),
        check_def(
            "the measurement coverage report names every registered kind",
            &[(REPORT_KEY, "CoverageReport")],
            |ctx, _| {
                let report = report(&ctx)?;
                let named: BTreeSet<String> = report
                    .covered
                    .keys()
                    .chain(report.skipped.keys())
                    .cloned()
                    .collect();
                if named == report.registered {
                    Ok(())
                } else {
                    Err(format!(
                        "report names {:?}, registry contains {:?}",
                        named, report.registered
                    ))
                }
            },
        ),
        check_def(
            "every covered kind reports step, transition, and workflow measurement",
            &[(REPORT_KEY, "CoverageReport")],
            |ctx, _| {
                let report = report(&ctx)?;
                let failures: Vec<String> = report
                    .covered
                    .iter()
                    .filter_map(|(kind, coverage)| {
                        let missing = missing_granularities(coverage);
                        (!missing.is_empty()).then(|| format!("{}: {}", kind, missing.join(", ")))
                    })
                    .collect();
                if failures.is_empty() {
                    Ok(())
                } else {
                    Err(format!("coverage gaps: {}", failures.join("; ")))
                }
            },
        ),
        check_def(
            "every skipped kind is named with its asserted exclusion reason",
            &[(REPORT_KEY, "CoverageReport")],
            |ctx, _| {
                let report = report(&ctx)?;
                let spark_ok = report.skipped.get("spark").map(String::as_str)
                    == Some(PROJECTION_REASON);
                let backlog_ok = report.skipped.get("backlog_item").map(String::as_str)
                    == Some(ENGINE_MINTED_REASON);
                if spark_ok && backlog_ok && report.skipped.len() == 2 {
                    Ok(())
                } else {
                    Err(format!("unexpected skip report: {:?}", report.skipped))
                }
            },
        ),
        check_def(
            "no registered kind is silently omitted",
            &[(REPORT_KEY, "CoverageReport")],
            |ctx, _| report(&ctx)?.evaluate(),
        ),
        async_step_def(
            "a projection-only snapshot is exercised",
            &matrix_inputs(),
            &matrix_outputs_with_projection_counts(),
            |mut ctx, _| async move {
                let hearth = matrix_hearth(&ctx)?;
                let before = measurement_counts(&hearth)?;
                let server = take_server(&mut ctx)?;
                call_projection_only_snapshot(server.port()).await?;
                let after = measurement_counts(&hearth)?;
                let delta = (
                    after.0.saturating_sub(before.0),
                    after.1.saturating_sub(before.1),
                    after.2.saturating_sub(before.2),
                );
                let registered = ctx
                    .get::<BTreeSet<String>>(REGISTERED_KEY)
                    .ok_or("No registered kinds")?
                    .clone();
                let mut report = CoverageReport {
                    registered,
                    ..Default::default()
                };
                report
                    .skipped
                    .insert("spark".to_string(), PROJECTION_REASON.to_string());
                report
                    .skipped
                    .insert("backlog_item".to_string(), ENGINE_MINTED_REASON.to_string());
                let mut out = carry_matrix_context(&mut ctx)?;
                out.set(SERVER_KEY, server);
                out.set(PROJECTION_COUNTS_KEY, delta);
                out.set(REPORT_KEY, report);
                Ok(out)
            },
        ),
        check_def(
            "no step, transition, or workflow measurement is emitted",
            &[(PROJECTION_COUNTS_KEY, "(usize,usize,usize)")],
            |ctx, _| match ctx.get::<(usize, usize, usize)>(PROJECTION_COUNTS_KEY) {
                Some((0, 0, 0)) => Ok(()),
                Some(counts) => Err(format!(
                    "projection-only measurement delta was {:?}",
                    counts
                )),
                None => Err("No projection-only measurement counts".to_string()),
            },
        ),
        check_def(
            "the measurement coverage report names {string} with reason {string}",
            &[(REPORT_KEY, "CoverageReport")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let reason = params.get_string(1).ok_or("Expected reason")?;
                match report(&ctx)?.skipped.get(kind) {
                    Some(actual) if actual == reason => Ok(()),
                    actual => Err(format!(
                        "expected skip {} => {:?}, got {:?}",
                        kind, reason, actual
                    )),
                }
            },
        ),
        async_step_def(
            "a completed registered-kind measurement matrix",
            &[],
            &[
                (REPORT_KEY, "CoverageReport"),
                (HEARTH_KEY, "PathBuf"),
                (HANDLE_KEY, "RetainedTempDir"),
            ],
            |_, _| async move {
                let (handle, hearth) = retained_temp_dir("anvil-measurement-mutation-")?;
                let registry = SeedPlaybookRegistry;
                seed_matrix_hearth(&hearth, &registry)?;
                let writer = Arc::new(HearthStepWriter::new(&hearth));
                let server =
                    spawn_test_server_with_step_measurement_writer(hearth.clone(), writer).await?;
                let registered: BTreeSet<String> = registry.kinds().into_iter().collect();
                for machine in registry.all_machines() {
                    if !machine.projection_only && is_matrix_exercisable(&machine.kind) {
                        exercise_terminal_snapshot(server.port(), machine).await?;
                    }
                }
                wait_for_step_kinds(&hearth, expected_measured_kinds(&registered))?;
                let report = read_report(&hearth, registered)?;
                report.evaluate()?;
                drop(server);
                let mut out = Context::new();
                out.set(REPORT_KEY, report);
                out.set(HEARTH_KEY, hearth);
                out.set::<RetainedTempDir>(HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "the matrix observations for registered kind {string} are removed",
            &[(REPORT_KEY, "CoverageReport")],
            &[(REPORT_KEY, "CoverageReport"), (FAILURE_KEY, "String")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let mut report = report(&ctx)?.clone();
                if !report.registered.contains(&kind) {
                    return Err(format!("mutation target '{}' is not registered", kind));
                }
                report.covered.insert(kind, Coverage::default());
                let failure = report
                    .evaluate()
                    .expect_err("mutation must make the coverage matrix fail");
                let mut out = Context::new();
                out.set(REPORT_KEY, report);
                out.set(FAILURE_KEY, failure);
                Ok(out)
            },
        ),
        check_def(
            "the measurement coverage matrix fails naming kind {string}",
            &[(FAILURE_KEY, "String")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let failure = ctx.get::<String>(FAILURE_KEY).ok_or("No matrix failure")?;
                if failure.contains(&format!("kind '{}'", kind)) {
                    Ok(())
                } else {
                    Err(format!("failure did not name '{}': {}", kind, failure))
                }
            },
        ),
        check_def(
            "the failure names missing granularities {string}",
            &[(FAILURE_KEY, "String")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected granularities")?;
                let failure = ctx.get::<String>(FAILURE_KEY).ok_or("No matrix failure")?;
                if failure.contains(expected) {
                    Ok(())
                } else {
                    Err(format!("failure did not name '{}': {}", expected, failure))
                }
            },
        ),
    ]
}

fn matrix_inputs() -> Vec<(&'static str, &'static str)> {
    vec![
        (HEARTH_KEY, "PathBuf"),
        (HANDLE_KEY, "RetainedTempDir"),
        (SERVER_KEY, "TestAnvilServerHandle"),
        (REGISTERED_KEY, "BTreeSet<String>"),
    ]
}

fn matrix_outputs_with_report() -> Vec<(&'static str, &'static str)> {
    let mut outputs = matrix_inputs();
    outputs.push((REPORT_KEY, "CoverageReport"));
    outputs
}

fn matrix_outputs_with_projection_counts() -> Vec<(&'static str, &'static str)> {
    let mut outputs = matrix_outputs_with_report();
    outputs.push((PROJECTION_COUNTS_KEY, "(usize,usize,usize)"));
    outputs
}

fn carry_matrix_context(ctx: &mut Context) -> Result<Context, String> {
    let mut out = Context::new();
    let hearth = ctx.take::<PathBuf>(HEARTH_KEY).ok_or("No matrix hearth")?;
    let handle = ctx
        .take::<RetainedTempDir>(HANDLE_KEY)
        .ok_or("No matrix hearth handle")?;
    let registered = ctx
        .take::<BTreeSet<String>>(REGISTERED_KEY)
        .ok_or("No registered kinds")?;
    out.set(HEARTH_KEY, hearth);
    out.set::<RetainedTempDir>(HANDLE_KEY, handle);
    out.set(REGISTERED_KEY, registered);
    Ok(out)
}

fn take_server(ctx: &mut Context) -> Result<TestAnvilServerHandle, String> {
    ctx.take::<TestAnvilServerHandle>(SERVER_KEY)
        .ok_or_else(|| "No matrix server".to_string())
}

fn matrix_hearth(ctx: &Context) -> Result<PathBuf, String> {
    ctx.get::<PathBuf>(HEARTH_KEY)
        .cloned()
        .ok_or_else(|| "No matrix hearth".to_string())
}

fn report(ctx: &Context) -> Result<&CoverageReport, String> {
    ctx.get::<CoverageReport>(REPORT_KEY)
        .ok_or_else(|| "No measurement coverage report".to_string())
}

fn seed_matrix_hearth(hearth: &Path, registry: &dyn PlaybookRegistry) -> Result<(), String> {
    std::fs::create_dir_all(hearth.join("forge/projections"))
        .map_err(|error| format!("create matrix projections: {}", error))?;
    std::fs::create_dir_all(hearth.join("sparks"))
        .map_err(|error| format!("create sparks: {}", error))?;
    std::fs::write(hearth.join("sparks/sparks.md"), "# Sparks\n")
        .map_err(|error| format!("write sparks source: {}", error))?;
    std::fs::write(hearth.join("forge/projections/sparks.md"), "# Sparks\n")
        .map_err(|error| format!("write sparks projection: {}", error))?;
    for machine in registry.all_machines() {
        if machine.projection_only {
            continue;
        }
        let (from_state, _) = terminal_transition(machine)?;
        let artifact_id = matrix_artifact_id(&machine.kind);
        let artifact_dir = hearth.join(&machine.directory).join(&artifact_id);
        std::fs::create_dir_all(&artifact_dir)
            .map_err(|error| format!("create {} artifact: {}", machine.kind, error))?;
        // K8 keeps backlog items in their own typed store file; a status.yaml-only
        // seed leaves `item.yaml` absent and the snapshot reds FailedPrecondition.

        std::fs::write(
            artifact_dir.join("status.yaml"),
            format!(
                "version: 1\nkind: {}\nstate: {}\ntransitions:\n  - to: {}\n    at: \"2026-07-29T00:00:00Z\"\n    actor: Matrix-Seed-000001\n    role: doer\n",
                machine.kind, from_state, from_state
            ),
        )
        .map_err(|error| format!("write {} status: {}", machine.kind, error))?;
        std::fs::write(
            hearth.join(&machine.registry),
            format!("# {}\n", machine.kind),
        )
        .map_err(|error| format!("write {} registry: {}", machine.kind, error))?;
    }
    Ok(())
}

fn terminal_transition(machine: &PlaybookMachine) -> Result<(String, String), String> {
    machine
        .transitions
        .iter()
        .find(|transition| {
            machine
                .states
                .iter()
                .any(|state| state.name == transition.to_state && state.is_terminal)
        })
        .map(|transition| (transition.from_state.clone(), transition.to_state.clone()))
        .ok_or_else(|| {
            format!(
                "registered in-boundary kind '{}' has no transition into a terminal state",
                machine.kind
            )
        })
}

fn matrix_artifact_id(kind: &str) -> String {
    // K8 pins `backlog_item` ids to a `bi_` prefix with a >=4-char body
    // (`domain::backlog_item::validate_prefixed_id`), so the generic
    // `matrix-<kind>-instance` shape is refused for that kind. The matrix must
    // mint an id the kind's own store accepts, or the scenario fails on id
    // validation before it can measure anything.
    if kind == "backlog_item" {
        return "bi_mtrx0001".to_string();
    }
    format!("matrix-{}-instance", kind)
}

async fn exercise_terminal_snapshot(port: u16, machine: &PlaybookMachine) -> Result<(), String> {
    let (from_state, to_state) = terminal_transition(machine)?;
    let transition = machine
        .transitions
        .iter()
        .find(|transition| transition.from_state == from_state && transition.to_state == to_state)
        .ok_or_else(|| format!("no selected transition for {}", machine.kind))?;
    let artifact_path = format!(
        "{}/{}",
        machine.directory,
        matrix_artifact_id(&machine.kind)
    );
    let mut client = connect(port).await?;
    let request = anvil_engine::proto::SnapshotRequest {
        artifact_path,
        to_state,
        actor_name: "Matrix-Agent-100001".to_string(),
        actor_role: transition.required_role.clone(),
        actor_type: "agent".to_string(),
        actor_model: "brine".to_string(),
        actor_provider: "test".to_string(),
        conversation_id: format!("matrix-{}-conversation", machine.kind),
        project_root: "/tmp/anvil-measurement-matrix".to_string(),
        ..Default::default()
    };
    client
        .snapshot(anvil_test_support::surfaced(request))
        .await
        .map_err(|status| {
            format!(
                "snapshot for registered kind '{}' failed ({:?}): {}",
                machine.kind,
                status.code(),
                status.message()
            )
        })?;
    Ok(())
}

async fn call_projection_only_snapshot(port: u16) -> Result<(), String> {
    let mut client = connect(port).await?;
    let request = anvil_engine::proto::SnapshotRequest {
        artifact_path: "sparks/sparks.md".to_string(),
        projection_only: true,
        event_type: "spark".to_string(),
        note: "matrix projection boundary probe".to_string(),
        ..Default::default()
    };
    client
        .snapshot(anvil_test_support::surfaced(request))
        .await
        .map_err(|status| {
            format!(
                "projection-only snapshot failed ({:?}): {}",
                status.code(),
                status.message()
            )
        })?;
    Ok(())
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
    .map_err(|error| format!("connect to matrix engine: {}", error))
}

fn wait_for_step_kinds(hearth: &Path, expected: usize) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let count = FileSystemStepMeasurementAdapter::new(hearth)
            .read_step_measurements()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|record| record.artifact_kind)
            .collect::<BTreeSet<_>>()
            .len();
        if count >= expected {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for {} measured kinds; observed {}",
                expected, count
            ));
        }
        std::thread::yield_now();
    }
}

fn read_report(hearth: &Path, registered: BTreeSet<String>) -> Result<CoverageReport, String> {
    let steps = FileSystemStepMeasurementAdapter::new(hearth)
        .read_step_measurements()
        .map_err(|error| error.to_string())?;
    let transitions =
        anvil_core_hearth::fs_transition_measurement_adapter::FileSystemTransitionMeasurementAdapter::new(hearth)
            .read_transition_measurements()
            .map_err(|error| error.to_string())?;
    let workflows =
        anvil_core_hearth::fs_playbook_measurement_adapter::FileSystemPlaybookMeasurementAdapter::new(hearth)
            .read_playbook_measurements()
            .map_err(|error| error.to_string())?;
    let mut report = CoverageReport {
        registered,
        ..Default::default()
    };
    for kind in &report.registered {
        if !is_matrix_exercisable(kind) {
            let reason = if kind == "spark" { PROJECTION_REASON } else { ENGINE_MINTED_REASON };
            report.skipped.insert(kind.clone(), reason.to_string());
            continue;
        }
        report.covered.insert(
            kind.clone(),
            Coverage {
                step: steps.iter().any(|record| &record.artifact_kind == kind),
                transition: transitions
                    .iter()
                    .any(|record| &record.artifact_kind == kind),
                workflow: workflows.iter().any(|record| &record.artifact_kind == kind),
            },
        );
    }
    Ok(report)
}

fn measurement_counts(hearth: &Path) -> Result<(usize, usize, usize), String> {
    Ok((
        FileSystemStepMeasurementAdapter::new(hearth)
            .read_step_measurements()
            .map_err(|error| error.to_string())?
            .len(),
        anvil_core_hearth::fs_transition_measurement_adapter::FileSystemTransitionMeasurementAdapter::new(hearth)
            .read_transition_measurements()
            .map_err(|error| error.to_string())?
            .len(),
        anvil_core_hearth::fs_playbook_measurement_adapter::FileSystemPlaybookMeasurementAdapter::new(hearth)
            .read_playbook_measurements()
            .map_err(|error| error.to_string())?
            .len(),
    ))
}

fn missing_granularities(coverage: &Coverage) -> Vec<&'static str> {
    [
        ("step", coverage.step),
        ("transition", coverage.transition),
        ("workflow", coverage.workflow),
    ]
    .into_iter()
    .filter_map(|(name, present)| (!present).then_some(name))
    .collect()
}

/// Fail if the compiled `SeedPlaybookRegistry` and the shipped `playbooks/` directory
/// disagree about which kinds exist.
///
/// `build-kit.sh` copies `playbooks/*/machine.yaml` into the kit and enforcement-loads
/// every one of them at publish time. If the seed registry omits a directory that ships,
/// the matrix silently never measures it; if it names one that does not ship, the matrix
/// measures something the kit does not contain. Either way the matrix's completeness claim
/// is false while its suite is green.
///
/// RED MUTATION: add a directory under `playbooks/` (or delete one) without updating
/// `SeedPlaybookRegistry` — this must fail naming the specific kind.
fn assert_seed_registry_matches_shipped_playbooks(
    registered: &BTreeSet<String>,
) -> Result<(), String> {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("no workspace root above anvil-engine")?
        .to_path_buf();
    let dir = repo.join("playbooks");
    let entries = std::fs::read_dir(&dir)
        .map_err(|e| format!("read {}: {e}", dir.display()))?;

    let mut shipped: BTreeSet<String> = BTreeSet::new();
    for entry in entries.flatten() {
        if !entry.path().join("machine.yaml").is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        // Hearth installs carry a timestamp prefix (`20260422T0000_track_lifecycle`);
        // the repo directory may or may not. Compare on the KIND, not the directory name —
        // that difference already caused one wrong conclusion in this track's review.
        let kind = name
            .split_once('_')
            .filter(|(head, _)| head.len() == 15 && head.contains('T'))
            .map(|(_, tail)| tail.to_string())
            .unwrap_or(name);
        shipped.insert(kind.trim_end_matches("_lifecycle").to_string());
    }

    let normalised: BTreeSet<String> = registered
        .iter()
        .map(|k| k.trim_end_matches("_lifecycle").to_string())
        .collect();

    let missing: Vec<&String> = shipped.difference(&normalised).collect();
    if !missing.is_empty() {
        return Err(format!(
            "playbooks/ ships kinds the seed registry does not register, so the matrix \
             would never measure them: {missing:?} (shipped={shipped:?} registered={normalised:?})"
        ));
    }
    Ok(())
}
