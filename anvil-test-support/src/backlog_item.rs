//! Step module for the K8 `backlog_item` core features
//! (`anvil-core/features/backlog_item/*.feature`).
//!
//! The `@schema` slice (Task 2) is wired to the REAL value model in
//! `anvil_core::domain::backlog_item`: named inputs are decoded through the
//! actual `#[serde(deny_unknown_fields)]` types and run through the actual
//! semantic / required-by-state / history validators. No test double backs any
//! outcome — an input that the real validator wrongly accepts records a success
//! and turns its scenario red. The later-task steps (`@storage`, `@genesis`,
//! `@snapshot`, `@mutations`, `@evaluate`, `@transport`) remain routed to
//! [`BacklogFixture::unimplemented`] until their owning task lands them; that
//! keeps their still-unimplemented slices Red for the intended reason.

use crate::{retained_temp_dir, RetainedTempDir};
use anvil_core::domain::backlog_item as bi;
use anvil_core::domain::backlog_item::{
    ActionClass, BacklogGenesisInput, BacklogItem, DependencyReadiness, DependencyStatus,
    DriverRole, EffortClass, EvidenceKind, EvidenceRef, Exit, ExitKind, HistoryEntry, HistoryKind,
    Intake, IntakeEdge, OriginBinding, OutcomeBinding, ExecutionBinding, PlaybookBinding, Rank,
    RankInputs, ReadingStatus, State, ValueGapMagnitude, WakeCondition, WakeKind,
};
use anvil_core::domain::backlog_item::legal_transition_tuples;
use anvil_core::domain::playbook::loader::validate_contiguity;
use anvil_core::domain::playbook::registry::{driven_candidates, PlaybookRegistry, SeedPlaybookRegistry};
use anvil_core::domain::playbook::seeds::backlog_item_seed;
use anvil_core::domain::playbook::types::Register;
use anvil_core::domain::routing::{compute_execution_route, SUBJECT_AVAILABLE_TYPE};
use anvil_core::domain::catalog::CatalogQueryHandler;
use anvil_core::domain::snapshot::{registry_file_for, registry_section_for};
use anvil_core::domain::{available_artifact_types, describe, ArtifactType};
use anvil_core_hearth::fs_hearth_reader::FileSystemHearthReader;
use anvil_core_hearth::fs_query_adapter::FileSystemQueryAdapter;
use anvil_core_hearth::fs_snapshot_adapter::FileSystemSnapshotAdapter;
use anvil_core_hearth::fs_transition_event_adapter::FileSystemTransitionEventAdapter;
use anvil_core::ports::query_port::QueryPort;
use anvil_core::ports::snapshot_port::SnapshotPort;
use anvil_core::ports::transition_event_write_port::{
    TransitionEventWritePort, TransitionRecord,
};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Per-property pass/fail report from one registration-surface inspection.
type RegistrationReport = BTreeMap<String, Result<(), String>>;

const WORLD: &str = "bi_world";
const WORLD_TY: &str = "BacklogFixture";

/// Tokens `missing_required_by_state` reports that are NOT printed K8 matrix
/// rows: they are edge guards (#1/#14 readiness) surfaced by the reporter for
/// caller convenience. Every other reported token is a matrix field and must
/// agree with the strict `validate_required_by_state`.
const GUARD_ONLY_TOKENS: &[&str] = &["dependency_ready"];

/// The one named fixture builder for K8 backlog scenarios (§3). Holds a real
/// temporary hearth, an optional seeded item + history threaded from a `Given`
/// to a `When`, and the last attempted-operation outcome.
#[derive(Clone)]
pub struct BacklogFixture {
    pub hearth: PathBuf,
    _temp: RetainedTempDir,
    last: Arc<Mutex<Option<Attempt>>>,
    item: Arc<Mutex<Option<BacklogItem>>>,
    history: Arc<Mutex<Option<Vec<HistoryEntry>>>>,
    reg: Arc<Mutex<Option<RegistrationReport>>>,
    /// Set when the scenario seeded a REAL on-disk item through the production
    /// store. A disk-backed fixture never falls back to the pure round-trip:
    /// the reload step asserts against the real strict load.
    disk: Arc<Mutex<Option<String>>>,
    /// Set when the scenario seeded a REAL lifecycle provenance through the
    /// production genesis / mutation / governed-transition operations.
    prov: Arc<Mutex<Option<Provenance>>>,
    /// Evidence a later assertion needs that a pass/fail outcome cannot carry.
    evidence: Arc<Mutex<Evidence>>,
}

/// The outcome of one attempted backlog operation.
#[derive(Clone, Debug)]
enum Attempt {
    /// RED-phase placeholder for a seam owned by a later task.
    Unimplemented { seam: String },
    Succeeded { state: Option<String>, history: Vec<String> },
    Rejected { reason: String },
}

impl BacklogFixture {
    fn new() -> Result<Self, String> {
        let (temp, hearth) = retained_temp_dir("anvil-backlog-core")?;
        std::fs::create_dir_all(&hearth).map_err(|e| format!("create hearth: {e}"))?;
        Ok(Self {
            hearth,
            _temp: temp,
            last: Arc::new(Mutex::new(None)),
            item: Arc::new(Mutex::new(None)),
            history: Arc::new(Mutex::new(None)),
            reg: Arc::new(Mutex::new(None)),
            disk: Arc::new(Mutex::new(None)),
            prov: Arc::new(Mutex::new(None)),
            evidence: Arc::new(Mutex::new(Evidence::default())),
        })
    }

    fn set_disk(&self, bi_id: &str) {
        *self.disk.lock().expect("backlog disk mutex") = Some(bi_id.to_string());
    }

    fn disk_id(&self) -> Option<String> {
        self.disk.lock().expect("backlog disk mutex").clone()
    }

    fn set_prov(&self, prov: Provenance) {
        *self.prov.lock().expect("backlog prov mutex") = Some(prov);
    }

    fn prov(&self) -> Option<Provenance> {
        self.prov.lock().expect("backlog prov mutex").clone()
    }

    fn evidence(&self) -> Evidence {
        self.evidence.lock().expect("backlog evidence mutex").clone()
    }

    fn with_evidence(&self, edit: impl FnOnce(&mut Evidence)) {
        edit(&mut self.evidence.lock().expect("backlog evidence mutex"));
    }

    fn set_registration(&self, map: RegistrationReport) {
        *self.reg.lock().expect("backlog reg mutex") = Some(map);
    }

    fn registration_property(&self, prop: &str) -> Result<(), String> {
        let guard = self.reg.lock().expect("backlog reg mutex");
        let map = guard
            .as_ref()
            .ok_or_else(|| "registration surfaces were not inspected".to_string())?;
        match map.get(prop) {
            Some(result) => result.clone(),
            None => Err(format!("unknown registration property `{prop}`")),
        }
    }

    fn record(&self, attempt: Attempt) {
        *self.last.lock().expect("backlog outcome mutex") = Some(attempt);
    }

    fn set_item(&self, item: BacklogItem, history: Vec<HistoryEntry>) {
        *self.item.lock().expect("backlog item mutex") = Some(item);
        *self.history.lock().expect("backlog history mutex") = Some(history);
    }

    fn item(&self) -> Result<BacklogItem, String> {
        self.item
            .lock()
            .expect("backlog item mutex")
            .clone()
            .ok_or_else(|| "no source backlog item was seeded".to_string())
    }

    fn history(&self) -> Vec<HistoryEntry> {
        self.history
            .lock()
            .expect("backlog history mutex")
            .clone()
            .unwrap_or_default()
    }

    fn unimplemented(&self, seam: &str) {
        self.record(Attempt::Unimplemented { seam: seam.to_string() });
    }

    fn last(&self) -> Result<Attempt, String> {
        self.last
            .lock()
            .expect("backlog outcome mutex")
            .clone()
            .ok_or_else(|| "no backlog operation was attempted".to_string())
    }
}

fn world(ctx: &Context) -> Result<BacklogFixture, String> {
    ctx.get::<BacklogFixture>(WORLD)
        .cloned()
        .ok_or_else(|| "backlog fixture missing; start with 'a backlog fixture'".to_string())
}

fn carry(w: BacklogFixture) -> Context {
    Context::new().with(WORLD, w)
}

fn p(params: &brine_core::step_types::Params, i: usize) -> Result<String, String> {
    params
        .get_string(i)
        .map(|s| s.to_string())
        .ok_or_else(|| format!("missing string parameter {i}"))
}

// ── Assertion helpers over the recorded outcome ──────────────────────────────

fn expect_success(w: &BacklogFixture) -> Result<(Option<String>, Vec<String>), String> {
    match w.last()? {
        Attempt::Succeeded { state, history } => Ok((state, history)),
        Attempt::Rejected { reason } => Err(format!("expected success but was rejected: {reason}")),
        Attempt::Unimplemented { seam } => {
            Err(format!("expected success but seam is unimplemented: {seam}"))
        }
    }
}

fn expect_rejected(w: &BacklogFixture) -> Result<String, String> {
    match w.last()? {
        Attempt::Rejected { reason } => Ok(reason),
        Attempt::Succeeded { .. } => Err("expected a rejection but the operation succeeded".into()),
        Attempt::Unimplemented { seam } => {
            Err(format!("expected a rejection but seam is unimplemented: {seam}"))
        }
    }
}

// ── Concrete builders over the REAL value types ──────────────────────────────

fn evid(kind: EvidenceKind, id: &str) -> EvidenceRef {
    EvidenceRef { kind, id: id.to_string() }
}

fn temper_value_gap() -> EvidenceRef {
    evid(EvidenceKind::TemperMeasure, "tm_valuegap01")
}

fn created_history() -> Vec<HistoryEntry> {
    vec![HistoryEntry {
        seq: 0,
        actor: "intake-actor".to_string(),
        role: DriverRole::Intake,
        at: "2026-07-25T00:00:00Z".to_string(),
        kind: HistoryKind::Created,
        from_state: None,
        to_state: Some(State::Candidate),
        payload: None,
        note: None,
    }]
}

fn base_intake() -> Intake {
    Intake {
        edge: IntakeEdge::SparkTriage,
        evidence_refs: vec![evid(EvidenceKind::Spark, "sp_seed01")],
    }
}

fn base_origin() -> OriginBinding {
    OriginBinding {
        value_gap_served: temper_value_gap(),
        minting_council_id: None,
        experiment_id: None,
        predicted_value: None,
    }
}

fn base_playbook() -> PlaybookBinding {
    PlaybookBinding {
        playbook_definition_id: Some("pd_seed01".to_string()),
        route_to_intake: false,
    }
}

fn full_rank(effort: EffortClass, dep: DependencyStatus) -> Rank {
    Rank {
        position: 1,
        inputs: RankInputs {
            value_gap_magnitude: ValueGapMagnitude { r#ref: temper_value_gap(), magnitude: 4.0 },
            nick_weight: 1.0,
            dependency_readiness: DependencyReadiness { status: dep, blocker_refs: vec![] },
            age: 0,
            effort_class: effort,
        },
        explanation: "seeded rank explanation".to_string(),
    }
}

fn exec_binding() -> ExecutionBinding {
    ExecutionBinding {
        track_id: "tr_seed01".to_string(),
        playbook_definition_id: "pd_seed01".to_string(),
        playbook_run_id: "wf::seed/any-shape 42".to_string(),
        run_id: "lr_abcdefghjkmnpqrstvwxyz0123".to_string(),
    }
}

fn outcome_reading() -> OutcomeBinding {
    OutcomeBinding {
        success_measure_id: Some("sm_seed01".to_string()),
        tree_node: "tree/seed".to_string(),
        reading_status: ReadingStatus::Reading,
        nick_signoff: false,
    }
}

/// Seed a MINIMAL, store-valid `item.yaml` for `bi_id` at `<hearth>/backlog_items/<bi_id>/`.
/// The K8 store keeps backlog items in their own typed file, so a generic
/// status.yaml-only seed (what the all-kinds measurement matrix writes for every
/// other kind) leaves `item.yaml` absent and the snapshot fails FailedPrecondition.
pub fn seed_minimal_backlog_item(hearth: &std::path::Path, bi_id: &str) -> Result<(), String> {
    let mut item = base_item();
    item.backlog_item_id = bi_id.to_string();
    let dir = hearth.join("backlog_items").join(bi_id);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create backlog dir: {e}"))?;
    let yaml = serde_yaml::to_string(&item).map_err(|e| format!("serialize item: {e}"))?;
    std::fs::write(dir.join("item.yaml"), yaml).map_err(|e| format!("write item.yaml: {e}"))?;
    // R13 realizes history out-of-line; the store requires the file to exist even
    // when the item has no history yet.
    // The store requires history PRESENT AND NON-EMPTY, so seed the creation entry.
    let history = vec![HistoryEntry {
        seq: 0,
        actor: "matrix-seed".to_string(),
        role: DriverRole::Intake,
        at: "2026-07-25T00:00:00Z".to_string(),
        kind: HistoryKind::Created,
        from_state: None,
        to_state: Some(State::Candidate),
        payload: None,
        note: None,
    }];
    let hy = serde_yaml::to_string(&history).map_err(|e| format!("serialize history: {e}"))?;
    std::fs::write(dir.join("history.yaml"), hy).map_err(|e| format!("write history.yaml: {e}"))
}

pub fn base_item() -> BacklogItem {
    BacklogItem {
        backlog_item_id: "bi_2468ace0".to_string(),
        business_node_id: "bn_abcdef0123".to_string(),
        title: "seed item".to_string(),
        description: None,
        action_class: ActionClass::Dev,
        effort_class: Some(EffortClass::S),
        state: State::Candidate,
        intake: base_intake(),
        rank: None,
        playbook_binding: Some(base_playbook()),
        origin_binding: base_origin(),
        execution_binding: None,
        outcome_binding: None,
        exit: None,
    }
}

/// A bare candidate: only the always-required fields (A). Missing E / P / Rk so
/// that a required-by-state check against any later target surfaces the full
/// missing set.
fn bare_candidate() -> BacklogItem {
    BacklogItem {
        effort_class: None,
        playbook_binding: None,
        ..base_item()
    }
}

/// A candidate fully shaped so that a required-by-state check against `target`
/// finds nothing missing.
fn complete_for(target: State) -> BacklogItem {
    let mut item = base_item();
    item.rank = Some(full_rank(EffortClass::S, DependencyStatus::Ready));
    match target {
        State::Candidate => {}
        State::Ready => {}
        State::InFlight => {
            item.execution_binding = Some(exec_binding());
            item.outcome_binding = Some(outcome_reading());
        }
        State::Done => {
            item.execution_binding = Some(exec_binding());
            item.outcome_binding = Some(outcome_reading());
            item.exit = Some(Exit {
                kind: ExitKind::Done,
                wake_condition: None,
                superseded_by: None,
                aged_out_reason: None,
            });
        }
        State::Parked => {
            item.exit = Some(Exit {
                kind: ExitKind::Parked,
                wake_condition: Some(WakeCondition {
                    kind: WakeKind::ItemState,
                    r#ref: Some(evid(EvidenceKind::BacklogItem, "bi_dep0")),
                    predicate: "item_state(bi_dep0)=done".to_string(),
                }),
                superseded_by: None,
                aged_out_reason: None,
            });
        }
        State::Superseded => {
            item.exit = Some(Exit {
                kind: ExitKind::Superseded,
                wake_condition: None,
                superseded_by: Some("bi_super01".to_string()),
                aged_out_reason: None,
            });
        }
        State::AgedOut => {}
    }
    item
}

fn parse_state(s: &str) -> Result<State, String> {
    Ok(match s {
        "candidate" => State::Candidate,
        "ready" => State::Ready,
        "in_flight" => State::InFlight,
        "done" => State::Done,
        "parked" => State::Parked,
        "superseded" => State::Superseded,
        "aged_out" => State::AgedOut,
        other => return Err(format!("unknown state token: {other}")),
    })
}

// ── Named-input dispatch for the field-schema slice ──────────────────────────

/// Validate a named field-schema input through the real value model. `Ok(())`
/// means the real validators accepted it; `Err(reason)` carries a reason string
/// containing the machine rule token the scenario asserts.
fn validate_named_input(name: &str) -> Result<(), String> {
    // Group 1 — item-layer inputs decoded/validated as a full `BacklogItem`.
    match name {
        "uppercase_bi_id" => {
            let mut item = base_item();
            item.backlog_item_id = "bi_ABCDEF".to_string();
            return run_item_semantics(&item);
        }
        "excluded_letter_bn_id" => {
            let mut item = base_item();
            item.business_node_id = "bn_iiiiiiiiii".to_string();
            return run_item_semantics(&item);
        }
        "short_bn_body" => {
            let mut item = base_item();
            item.business_node_id = "bn_abc".to_string();
            return run_item_semantics(&item);
        }
        "short_lr_body" => {
            let mut item = base_item();
            let mut eb = exec_binding();
            eb.run_id = "lr_abc".to_string();
            item.execution_binding = Some(eb);
            return run_item_semantics(&item);
        }
        "non_finite_magnitude" => {
            let mut item = base_item();
            let mut rank = full_rank(EffortClass::S, DependencyStatus::Ready);
            rank.inputs.value_gap_magnitude.magnitude = f64::INFINITY;
            item.rank = Some(rank);
            return run_item_semantics(&item);
        }
        "rank_position_zero" => {
            let mut item = base_item();
            let mut rank = full_rank(EffortClass::S, DependencyStatus::Ready);
            rank.position = 0;
            item.rank = Some(rank);
            return run_item_semantics(&item);
        }
        "effort_mirror_mismatch" => {
            let mut item = base_item();
            item.effort_class = Some(EffortClass::S);
            item.rank = Some(full_rank(EffortClass::M, DependencyStatus::Ready));
            return run_item_semantics(&item);
        }
        "opaque_playbook_run_id" => {
            let mut item = base_item();
            item.execution_binding = Some(exec_binding());
            return run_item_semantics(&item);
        }
        "lp_prefixed_id" => {
            return bi::validate_backlog_item_id("lp_abcdef01")
                .map_err(|e| format!("[pinned_prefix] {e}"));
        }
        "bad_initial_reading_status" => {
            return bi::validate_initial_reading_status(ReadingStatus::Reading)
                .map_err(|e| e.to_string());
        }
        _ => {}
    }

    // Group 2 — genesis-input-layer inputs decoded through `BacklogGenesisInput`.
    let mut v = base_genesis_json();
    let expect_serde_token: Option<&str> = match name {
        "valid_candidate" | "origin_both_null" => None,
        "unknown_key" => {
            v["totally_unknown"] = serde_json::json!(true);
            Some("deny_unknown_fields")
        }
        "caller_supplied_id" => {
            v["backlog_item_id"] = serde_json::json!("bi_2468ace0");
            Some("engine_owned_id")
        }
        "caller_supplied_state" => {
            v["state"] = serde_json::json!("candidate");
            Some("engine_owned_state")
        }
        "caller_supplied_rank" => {
            v["rank"] = serde_json::json!({"position": 1});
            Some("engine_owned_rank")
        }
        "caller_supplied_history" => {
            v["history"] = serde_json::json!([]);
            Some("engine_owned_history")
        }
        "bad_action_class" => {
            v["action_class"] = serde_json::json!("nonsense_action");
            Some("closed_action_enum")
        }
        "bad_effort_class" => {
            v["effort_class"] = serde_json::json!("gigantic");
            Some("closed_effort_enum")
        }
        "bad_evidence_kind" => {
            v["intake"]["evidence_refs"][0]["kind"] = serde_json::json!("mystery_kind");
            Some("closed_evidence_kind")
        }
        "empty_evidence_refs" => {
            v["intake"]["evidence_refs"] = serde_json::json!([]);
            None
        }
        "non_temper_value_gap_ref" => {
            v["origin_binding"]["value_gap_served"]["kind"] = serde_json::json!("spark");
            None
        }
        "both_predictor_ids" => {
            v["origin_binding"]["minting_council_id"] = serde_json::json!("co_seed01");
            v["origin_binding"]["experiment_id"] = serde_json::json!("ex_seed01");
            v["origin_binding"]["predicted_value"] = serde_json::json!(1.5);
            None
        }
        "predictor_without_value" => {
            v["origin_binding"]["minting_council_id"] = serde_json::json!("co_seed01");
            None
        }
        "value_without_predictor" => {
            v["origin_binding"]["predicted_value"] = serde_json::json!(1.5);
            None
        }
        "null_playbook_without_route" => {
            v["playbook_binding"] =
                serde_json::json!({"playbook_definition_id": null, "route_to_intake": false});
            None
        }
        "origin_council_finite" => {
            v["origin_binding"]["minting_council_id"] = serde_json::json!("co_seed01");
            v["origin_binding"]["predicted_value"] = serde_json::json!(1.5);
            None
        }
        "origin_experiment_finite" => {
            v["origin_binding"]["experiment_id"] = serde_json::json!("ex_seed01");
            v["origin_binding"]["predicted_value"] = serde_json::json!(1.5);
            None
        }
        other => return Err(format!("unknown named input: {other}")),
    };

    match serde_json::from_value::<BacklogGenesisInput>(v) {
        Ok(input) => {
            // A wrongly-ACCEPTED input must record the acceptance, never a
            // rejection: `Then the backlog field rule "<rule>" holds` calls
            // `expect_rejected`, so recording success here is exactly what
            // turns a deleted guarantee into a RED scenario. Returning an
            // `Err` that merely narrates the acceptance (the previous shape)
            // made the row pass under either outcome.
            let _ = expect_serde_token;
            bi::validate_genesis_input(&input).map_err(|e| e.to_string())
        }
        // The rejection text is the PRODUCTION serde error, unadorned. The
        // rule token is not spliced in: a fixture-authored token proves
        // nothing, and `the backlog field rule {string} holds` now asserts
        // against the offending key/variant that production itself names.
        Err(e) => Err(format!("{e}")),
    }
}

/// The substring the PRODUCTION rejection must carry for a serde-layer field
/// rule. Keyed by the rule token the feature row names, valued by the exact
/// offending key/variant that `run_field_rule` plants — so the `Then` can only
/// pass when the closed genesis shape itself refused that specific input.
fn serde_rule_evidence(rule: &str) -> Option<&'static str> {
    Some(match rule {
        "deny_unknown_fields" => "unknown field `totally_unknown`",
        "engine_owned_id" => "unknown field `backlog_item_id`",
        "engine_owned_state" => "unknown field `state`",
        "engine_owned_rank" => "unknown field `rank`",
        "engine_owned_history" => "unknown field `history`",
        "closed_action_enum" => "unknown variant `nonsense_action`",
        "closed_effort_enum" => "unknown variant `gigantic`",
        "closed_evidence_kind" => "unknown variant `mystery_kind`",
        _ => return None,
    })
}

fn run_item_semantics(item: &BacklogItem) -> Result<(), String> {
    bi::validate_item_semantics(item).map_err(|e| e.to_string())
}

fn base_genesis_json() -> serde_json::Value {
    serde_json::json!({
        "business_node_id": "bn_abcdef0123",
        "title": "seed item",
        "action_class": "dev",
        "intake": {
            "edge": "spark_triage",
            "evidence_refs": [{"kind": "spark", "id": "sp_seed01"}]
        },
        "origin_binding": {
            "value_gap_served": {"kind": "temper_measure", "id": "tm_valuegap01"}
        }
    })
}

// ── Source-item provenance seeding ───────────────────────────────────────────

fn seed_source(state: &str, prov: &str) -> Result<(BacklogItem, Vec<HistoryEntry>), String> {
    let history = created_history();
    let item = match (state, prov) {
        ("candidate", "bare") => bare_candidate(),
        ("candidate", "empty_history") => {
            // Deliberately empty history — the reconcile-layer history guard, not
            // the state-local validator, must reject this.
            return Ok((bare_candidate(), Vec::new()));
        }
        ("ready", "ready_shaped_no_binding") => {
            // Rank + effort + playbook + a superseded exit, but NO pickup bindings:
            // the superseded target tolerates absent bindings at the state-local
            // validator.
            let mut item = complete_for(State::Ready);
            item.state = State::Ready;
            item.exit = Some(Exit {
                kind: ExitKind::Superseded,
                wake_condition: None,
                superseded_by: Some("bi_super01".to_string()),
                aged_out_reason: None,
            });
            item
        }
        ("in_flight", "d1_3_full") => d1_3_full_item(),
        ("in_flight", "bound_without_declared_measure") => {
            // An outcome binding OBJECT is present, but it declares no success
            // measure. The printed K8 matrix marks
            // `outcome_binding.success_measure_id (declared)` as R at in_flight
            // and done, so binding presence alone must not satisfy it.
            let mut item = complete_for(State::Done);
            item.state = State::InFlight;
            item.outcome_binding = Some(OutcomeBinding {
                success_measure_id: None,
                ..outcome_reading()
            });
            item
        }
        ("parked", "interleaved_history") => {
            let mut item = complete_for(State::Parked);
            item.state = State::Parked;
            return Ok((item, interleaved_history()));
        }
        _ => {
            return Err(format!(
                "unseeded source provenance (state={state}, provenance={prov}) — owned by a \
                 later task slice"
            ))
        }
    };
    Ok((item, history))
}

/// A D1.3-covering item: every optional field populated at least once.
fn d1_3_full_item() -> BacklogItem {
    let mut item = base_item();
    item.state = State::InFlight;
    item.description = Some("full description".to_string());
    item.effort_class = Some(EffortClass::M);
    item.rank = Some(full_rank(EffortClass::M, DependencyStatus::Ready));
    item.playbook_binding = Some(PlaybookBinding {
        playbook_definition_id: None,
        route_to_intake: true,
    });
    item.origin_binding = OriginBinding {
        value_gap_served: temper_value_gap(),
        minting_council_id: Some("co_seed01".to_string()),
        experiment_id: None,
        predicted_value: Some(2.5),
    };
    item.execution_binding = Some(exec_binding());
    item.outcome_binding = Some(outcome_reading());
    item
}

fn interleaved_history() -> Vec<HistoryEntry> {
    let mut h = created_history();
    h.push(HistoryEntry {
        seq: 1,
        actor: "organ-loop".to_string(),
        role: DriverRole::OrganLoop,
        at: "2026-07-25T00:01:00Z".to_string(),
        kind: HistoryKind::ShapeEdit,
        from_state: None,
        to_state: None,
        payload: None,
        note: Some("shaped".to_string()),
    });
    h.push(HistoryEntry {
        seq: 2,
        actor: "nick".to_string(),
        role: DriverRole::NickShape,
        at: "2026-07-25T00:02:00Z".to_string(),
        kind: HistoryKind::StateChange,
        from_state: Some(State::Candidate),
        to_state: Some(State::Parked),
        payload: None,
        note: None,
    });
    h
}

// ── First-class registration inspection (Task 3) ─────────────────────────────

/// Inspect every first-class registration surface against the REAL domain
/// functions and return a per-property pass/fail map. Nothing is mocked: each
/// property calls the same production function the engine/MCP surfaces call.
fn inspect_registration() -> RegistrationReport {
    let mut m = RegistrationReport::new();
    let seed = backlog_item_seed();
    let registry = SeedPlaybookRegistry;

    let check = |cond: bool, msg: &str| -> Result<(), String> {
        if cond {
            Ok(())
        } else {
            Err(msg.to_string())
        }
    };

    // ArtifactType keystone: serde name + directory + registry file.
    let serde_name = serde_json::to_value(ArtifactType::BacklogItem)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string));
    m.insert(
        "first_class_artifact_type".to_string(),
        check(
            serde_json::from_value::<ArtifactType>(serde_json::json!("backlog_item"))
                .map(|t| t == ArtifactType::BacklogItem)
                .unwrap_or(false),
            "`backlog_item` does not decode to ArtifactType::BacklogItem",
        ),
    );
    m.insert(
        "serde_name_backlog_item".to_string(),
        check(
            serde_name.as_deref() == Some("backlog_item")
                && ArtifactType::BacklogItem.as_str() == "backlog_item",
            "serde/as_str name is not `backlog_item`",
        ),
    );
    m.insert(
        "directory_backlog_items".to_string(),
        check(
            ArtifactType::BacklogItem.directory_name() == "backlog_items",
            "directory_name is not `backlog_items`",
        ),
    );
    m.insert(
        "registry_backlog_items_md".to_string(),
        check(
            ArtifactType::BacklogItem.registry_file() == "backlog_items.md",
            "registry_file is not `backlog_items.md`",
        ),
    );

    // Catalog creation type present (and the six→seven baseline).
    let types = available_artifact_types();
    m.insert(
        "catalog_type_present".to_string(),
        check(
            types.iter().any(|t| t.name == "backlog_item"),
            "available_artifact_types() is missing `backlog_item`",
        ),
    );
    m.insert(
        "seven_type_workflow_baseline".to_string(),
        check(
            types.len() == 7,
            "available_artifact_types() is not seven entries",
        ),
    );

    // Describe: type present + required field `item`.
    m.insert(
        "describe_type_present".to_string(),
        check(
            describe::known_types().contains(&"backlog_item"),
            "describe known_types() is missing `backlog_item`",
        ),
    );
    m.insert(
        "describe_required_field_item".to_string(),
        check(
            describe::required_fields_for_type("backlog_item") == vec!["item".to_string()],
            "describe required field is not exactly [item]",
        ),
    );

    // Checkin creation routing: engine-supported.
    m.insert(
        "checkin_creation_supported".to_string(),
        check(
            compute_execution_route(SUBJECT_AVAILABLE_TYPE, "backlog_item", "", "creator")
                == "engine",
            "backlog_item creation is not engine-supported",
        ),
    );

    // Bare-ID directory lookup (domain-level): type + seed agree on the dir.
    m.insert(
        "bare_id_directory_lookup".to_string(),
        check(
            seed.directory == "backlog_items"
                && ArtifactType::BacklogItem.directory_name() == "backlog_items",
            "bare-id directory lookup does not resolve `backlog_items`",
        ),
    );

    // Seed resolves as a Free machine and is excluded from driven candidates.
    m.insert(
        "register_free".to_string(),
        check(seed.register == Register::Free, "seed register is not Free"),
    );
    m.insert(
        "excluded_from_driven_candidates".to_string(),
        check(
            registry.machine_for("backlog_item").is_some()
                && !driven_candidates(&registry)
                    .iter()
                    .any(|c| c.kind == "backlog_item"),
            "backlog_item is not excluded from driven candidates",
        ),
    );

    // Terminal / non-terminal state semantics from the compiled seed.
    let terminal = |name: &str| -> bool {
        seed.states
            .iter()
            .find(|s| s.name == name)
            .map(|s| s.is_terminal)
            .unwrap_or(false)
    };
    for (prop, name) in [
        ("terminal_done", "done"),
        ("terminal_superseded", "superseded"),
        ("terminal_aged_out", "aged_out"),
    ] {
        m.insert(
            prop.to_string(),
            check(terminal(name), "state is not terminal"),
        );
    }
    for (prop, name) in [
        ("nonterminal_candidate", "candidate"),
        ("nonterminal_ready", "ready"),
        ("nonterminal_in_flight", "in_flight"),
        ("nonterminal_parked", "parked"),
    ] {
        let present = seed.states.iter().any(|s| s.name == name);
        m.insert(
            prop.to_string(),
            check(present && !terminal(name), "state is terminal or absent"),
        );
    }

    // Seed shape: 29 transitions, no genesis row, contiguity to a terminal.
    m.insert(
        "twenty_nine_seed_transitions".to_string(),
        check(seed.transitions.len() == 29, "seed does not have 29 transitions"),
    );
    m.insert(
        "no_genesis_seed_row".to_string(),
        check(
            !seed
                .transitions
                .iter()
                .any(|t| t.from_state == "genesis"),
            "seed carries a genesis transition row",
        ),
    );
    m.insert(
        "contiguity_all_states_reach_terminal".to_string(),
        validate_contiguity(seed, "backlog_item").map_err(|e| format!("{e:?}")),
    );

    // The seed's admissibility rows and the validator's edge table are two
    // independent copies of the same frozen 17-row table. Compare them tuple by
    // tuple — an equal COUNT is not agreement, and a divergence would let the
    // compiled machine admit a `(from, to, role)` the validator refuses (or the
    // reverse).
    let seed_tuples: BTreeSet<(String, String, String)> = seed
        .transitions
        .iter()
        .map(|t| {
            (
                t.from_state.clone(),
                t.to_state.clone(),
                t.required_role.clone(),
            )
        })
        .collect();
    let table_tuples: BTreeSet<(String, String, String)> = legal_transition_tuples()
        .into_iter()
        .map(|(from, to, role)| {
            (
                from.as_str().to_string(),
                to.as_str().to_string(),
                role.as_str().to_string(),
            )
        })
        .collect();
    m.insert(
        "seed_matches_legal_transition_table".to_string(),
        if seed_tuples == table_tuples && seed_tuples.len() == 29 {
            Ok(())
        } else {
            let only_seed: Vec<_> = seed_tuples.difference(&table_tuples).collect();
            let only_table: Vec<_> = table_tuples.difference(&seed_tuples).collect();
            Err(format!(
                "seed and validator tables disagree — seed-only {only_seed:?}, validator-only {only_table:?}"
            ))
        },
    );

    // Registry routing: every one of the seven states maps to its own section
    // of `backlog_items.md` (the free-kind registry the snapshot writer moves
    // rows within).
    let sections: Vec<Option<&'static str>> = K8_STATES
        .iter()
        .map(|s| registry_section_for("backlog_item", s))
        .collect();
    m.insert(
        "registry_sections_all_seven".to_string(),
        check(
            sections.iter().all(|s| s.is_some())
                && sections
                    .iter()
                    .zip(K8_STATES)
                    .all(|(section, state)| section.map(|s| s == *state).unwrap_or(false)),
            "registry_section_for(backlog_item, …) does not resolve all seven K8 states",
        ),
    );
    m.insert(
        "registry_file_backlog_items_md".to_string(),
        check(
            registry_file_for("backlog_item") == "backlog_items.md",
            "registry_file_for(backlog_item) is not `backlog_items.md`",
        ),
    );

    m
}

/// The seven K8 states, candidate first.
const K8_STATES: &[&str] = &[
    "candidate",
    "ready",
    "in_flight",
    "parked",
    "done",
    "superseded",
    "aged_out",
];

/// Seed a REAL hearth with one `backlog_items/<id>/status.yaml` per K8 state
/// plus one active and one terminal track, so the catalog scan and the bare-ID
/// directory lookups run against real bytes on disk.
fn seed_real_registration_hearth(hearth: &std::path::Path) -> Result<(), String> {
    let write = |dir: &std::path::Path, kind: &str, state: &str| -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        std::fs::write(
            dir.join("status.yaml"),
            format!("version: 1\nkind: {kind}\nstate: {state}\n"),
        )
        .map_err(|e| format!("write status.yaml in {}: {e}", dir.display()))
    };
    for state in K8_STATES {
        write(
            &hearth.join("backlog_items").join(format!("bi_{state}")),
            "backlog_item",
            state,
        )?;
    }
    write(
        &hearth.join("tracks").join("track_active"),
        "track",
        "implementing",
    )?;
    write(
        &hearth.join("tracks").join("track_done"),
        "track",
        "completed",
    )?;
    Ok(())
}

/// Inspect the registration surfaces that only a REAL hearth can answer: the
/// kind-aware catalog terminal filter (through `FileSystemHearthReader` +
/// `CatalogQueryHandler`, the exact pair the engine's Catalog handler uses) and
/// the three hardcoded bare-ID directory lookups.
fn inspect_real_hearth_registration(
    hearth: &std::path::Path,
) -> RegistrationReport {
    let mut m = RegistrationReport::new();
    let check = |cond: bool, msg: &str| -> Result<(), String> {
        if cond {
            Ok(())
        } else {
            Err(msg.to_string())
        }
    };

    // --- Catalog: kind-aware terminal filtering over the real scan -----------
    let reader = FileSystemHearthReader::new(hearth.to_path_buf());
    let active: Result<Vec<String>, String> = CatalogQueryHandler::execute(&reader)
        .map(|catalog| catalog.active_artifacts.into_iter().map(|a| a.id).collect())
        .map_err(|e| format!("catalog scan failed: {e}"));
    let listed = |id: &str| -> Result<bool, String> {
        active.as_ref().map(|ids| ids.iter().any(|a| a == id)).map_err(Clone::clone)
    };
    for state in ["done", "superseded", "aged_out"] {
        let hidden = listed(&format!("bi_{state}")).map(|present| !present);
        m.insert(
            format!("catalog_hides_{state}"),
            hidden.and_then(|ok| check(ok, "a terminal K8 item is still listed as active")),
        );
    }
    for state in ["candidate", "ready", "in_flight", "parked"] {
        let present = listed(&format!("bi_{state}"));
        m.insert(
            format!("catalog_lists_{state}"),
            present.and_then(|ok| check(ok, "a live K8 item is missing from the active catalog")),
        );
    }
    m.insert(
        "catalog_unrelated_kind_terminals_intact".to_string(),
        listed("track_active").and_then(|active_listed| {
            listed("track_done").and_then(|done_listed| {
                check(
                    active_listed && !done_listed,
                    "K8 terminal semantics leaked into an unrelated kind",
                )
            })
        }),
    );

    // --- Bare-ID directory lookup at the three hardcoded adapters -----------
    let bare = "bi_candidate";
    let snapshot = FileSystemSnapshotAdapter::new(hearth.to_path_buf());
    m.insert(
        "snapshot_adapter_resolves_bare_id".to_string(),
        match snapshot.read_artifact_kind(bare) {
            Ok(kind) if kind == "backlog_item" => Ok(()),
            Ok(kind) => Err(format!("snapshot adapter resolved kind `{kind}`")),
            Err(e) => Err(format!("snapshot adapter rejected the bare id: {e:?}")),
        },
    );

    let query = FileSystemQueryAdapter::new(hearth.to_path_buf());
    m.insert(
        "query_adapter_resolves_bare_id".to_string(),
        match query.read_artifact_kind(bare) {
            Ok(kind) if kind == "backlog_item" => Ok(()),
            Ok(kind) => Err(format!("query adapter resolved kind `{kind}`")),
            Err(e) => Err(format!("query adapter rejected the bare id: {e:?}")),
        },
    );

    // The transition-event adapter resolves by writing: a bare id must land the
    // event inside `backlog_items/<id>/transitions/`, never in a stray
    // top-level `<hearth>/<id>/` directory.
    let events = FileSystemTransitionEventAdapter::new(hearth.to_path_buf());
    let record = TransitionRecord {
        to: "ready".to_string(),
        at: "2026-07-25T00:00:00Z".to_string(),
        actor: "registration-probe".to_string(),
        role: "organ_loop".to_string(),
        approver: None,
        note: None,
        satisfaction: None,
        event_type: None,
    };
    m.insert(
        "transition_adapter_resolves_bare_id".to_string(),
        match events.append_transition_event(bare, &record) {
            Err(e) => Err(format!("transition adapter rejected the bare id: {e:?}")),
            Ok(()) => {
                let inside = hearth
                    .join("backlog_items")
                    .join(bare)
                    .join("transitions")
                    .read_dir()
                    .map(|d| d.flatten().count())
                    .unwrap_or(0);
                let stray = hearth.join(bare).exists();
                check(
                    inside == 1 && !stray,
                    "the bare-id transition event did not land under backlog_items/<id>/transitions",
                )
            }
        },
    );

    m
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ── Given: fixture + seeded provenance ───────────────────────────────
        step_def(
            "a backlog fixture",
            &[],
            &[(WORLD, WORLD_TY)],
            |_ctx, _params| Ok(carry(BacklogFixture::new()?)),
        ),
        step_def(
            "a candidate backlog item shaped for row {string}",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let w = world(&ctx)?;
                let row = p(params, 0)?;
                let target = match row.as_str() {
                    "1" => State::Ready,
                    "5" => State::InFlight,
                    "7" => State::Parked,
                    "10" => State::Done,
                    _ => State::Candidate,
                };
                w.set_item(complete_for(target), created_history());
                Ok(carry(w))
            },
        ),
        step_def(
            "a source backlog item in state {string} from provenance {string}",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let w = world(&ctx)?;
                let state = p(params, 0)?;
                let prov = p(params, 1)?;
                // Pure value-model provenances stay in memory; every Task 4
                // storage provenance builds a REAL hearth through the
                // production store.
                match seed_source(&state, &prov) {
                    Ok((item, history)) => w.set_item(item, history),
                    Err(seam) => match seed_storage_provenance(&w.hearth, &prov)? {
                        Some(id) => {
                            if prov == "actor_status_rendered" {
                                let (live, staged, expected) =
                                    capture_staged_status(&w.hearth, &id)?;
                                w.with_evidence(|e| {
                                    e.status_live_before = Some(live);
                                    e.status_staged = Some(staged);
                                    e.status_expected = Some(expected);
                                });
                            }
                            w.set_disk(&id)
                        }
                        None => match seed_lifecycle_provenance(&w.hearth, &state, &prov)? {
                            Some(built) => {
                                w.with_evidence(|e| {
                                    e.bindings_before =
                                        binding_bytes(&w.hearth, &built.target).ok();
                                    e.rank_before = store_at(&w.hearth)
                                        .load_item(&built.target)
                                        .ok()
                                        .and_then(|l| serde_yaml::to_string(&l.item.rank).ok());
                                });
                                w.set_prov(built);
                            }
                            None => w.unimplemented(&seam),
                        },
                    },
                }
                Ok(carry(w))
            },
        ),
        step_def(
            "an external Temper reading is seeded for the in-flight item",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, _params| {
                let w = world(&ctx)?;
                let prov = w
                    .prov()
                    .ok_or_else(|| "no in-flight item was seeded".to_string())?;
                seed_external_temper_reading(&w.hearth, &prov.target)?;
                Ok(carry(w))
            },
        ),
        step_def(
            "a standing age-out veto is set",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, _params| {
                let w = world(&ctx)?;
                let prov = w
                    .prov()
                    .ok_or_else(|| "a veto needs a seeded lifecycle item".to_string())?;
                mutate(
                    &w.hearth,
                    BacklogMutation::VetoAgeOut {
                        bi_id: prov.target.clone(),
                        approver: "Nick".to_string(),
                    },
                    DriverRole::NickShape,
                    "Nick-000001",
                )?;
                Ok(carry(w))
            },
        ),
        // ── When: attempts that record an outcome ────────────────────────────
        step_def(
            "backlog genesis is attempted with input {string}",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let w = world(&ctx)?;
                let input = p(params, 0)?;
                match attempt_genesis(&w.hearth, &input) {
                    Ok(id) => {
                        w.set_disk(&id);
                        let history = FileSystemBacklogItemAdapter::new(w.hearth.clone())
                            .load_item(&id)
                            .map_err(|e| format!("reload after genesis: {e}"))?
                            .history
                            .iter()
                            .map(|h| h.kind.as_str().to_string())
                            .collect();
                        w.record(Attempt::Succeeded {
                            state: Some("candidate".to_string()),
                            history,
                        });
                    }
                    Err(reason) => w.record(Attempt::Rejected { reason }),
                }
                Ok(carry(w))
            },
        ),
        step_def(
            "a backlog transition from {string} to {string} is attempted with role {string}",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let w = world(&ctx)?;
                let from = parse_state(&p(params, 0)?)?;
                let to = parse_state(&p(params, 1)?)?;
                let role = parse_role(&p(params, 2)?)?;
                if let Some(prov) = w.prov() {
                    if role == DriverRole::EngineAuto && to != State::Done {
                        // #4/#9/#13/#14/#16 under engine_auto are reachable ONLY
                        // from the private Evaluation origin. Row #10 is the
                        // frozen exception: it stays on the public Snapshot path
                        // and is guarded solely by the stored reading.
                        match run_evaluation(&w.hearth) {
                            Ok(unranked) => {
                                w.with_evidence(|e| e.unranked = unranked);
                                let state = item_state(&w.hearth, &prov.target)?;
                                let history = history_kinds(&w.hearth, &prov.target)?;
                                w.record(Attempt::Succeeded {
                                    state: Some(state),
                                    history,
                                });
                            }
                            Err(reason) => w.record(Attempt::Rejected { reason }),
                        }
                        return Ok(carry(w));
                    }
                    // The REAL governed move: strict load, prepare against live
                    // revisions, and consume the capability exactly once.
                    match govern(&w.hearth, &prov.target, from, to, role, prov.approver.as_deref())
                    {
                        Ok(()) => {
                            let state = item_state(&w.hearth, &prov.target)?;
                            let history = history_kinds(&w.hearth, &prov.target)?;
                            w.with_evidence(|e| {
                                e.bindings_after = binding_bytes(&w.hearth, &prov.target).ok();
                            });
                            w.record(Attempt::Succeeded { state: Some(state), history });
                        }
                        Err(reason) => w.record(Attempt::Rejected { reason }),
                    }
                    return Ok(carry(w));
                }
                match w.disk_id() {
                    Some(id) => match attempt_stale_consume(&w.hearth, &id) {
                        Ok(()) => w.record(Attempt::Succeeded { state: None, history: vec![] }),
                        Err(reason) => w.record(Attempt::Rejected { reason }),
                    },
                    None => w.unimplemented(
                        "prepare_backlog_transition + commit_backlog_transition (Task 6)",
                    ),
                }
                Ok(carry(w))
            },
        ),
        step_def(
            "a raw Snapshot backlog transition from {string} to {string} is attempted with role {string}",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let w = world(&ctx)?;
                let from = parse_state(&p(params, 0)?)?;
                let to = parse_state(&p(params, 1)?)?;
                let role = parse_role(&p(params, 2)?)?;
                let prov = w
                    .prov()
                    .ok_or_else(|| "no lifecycle provenance was seeded".to_string())?;
                // ALWAYS the public origin: this is the forged-authority probe,
                // so it may never fall back to the evaluation service.
                match govern(&w.hearth, &prov.target, from, to, role, prov.approver.as_deref()) {
                    Ok(()) => {
                        let state = item_state(&w.hearth, &prov.target)?;
                        let history = history_kinds(&w.hearth, &prov.target)?;
                        w.record(Attempt::Succeeded { state: Some(state), history });
                    }
                    Err(reason) => w.record(Attempt::Rejected { reason }),
                }
                Ok(carry(w))
            },
        ),
        step_def(
            "the backlog operation {string} is attempted with role {string}",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let w = world(&ctx)?;
                let op = p(params, 0)?;
                let role = parse_role(&p(params, 1)?)?;
                match w.prov() {
                    Some(prov) => {
                        match attempt_named_mutation(&w.hearth, &prov, &op, role) {
                            Ok((state, history, unranked, proposal_id)) => {
                                w.with_evidence(|e| e.unranked = unranked);
                                if let Some(id) = proposal_id {
                                    // A proposal minted in a `Given` is the one
                                    // the later `When` resolves.
                                    let mut next = prov.clone();
                                    next.proposal_id = Some(id);
                                    w.set_prov(next);
                                }
                                w.record(Attempt::Succeeded { state, history });
                            }
                            Err(reason) => w.record(Attempt::Rejected { reason }),
                        }
                    }
                    None => w.unimplemented(&format!(
                        "no lifecycle provenance was seeded for backlog operation '{op}'"
                    )),
                }
                Ok(carry(w))
            },
        ),
        step_def(
            "backlog evaluation is run",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, _params| {
                let w = world(&ctx)?;
                let prov = w
                    .prov()
                    .ok_or_else(|| "no lifecycle provenance was seeded".to_string())?;
                match run_evaluation(&w.hearth) {
                    Ok(unranked) => {
                        w.with_evidence(|e| e.unranked = unranked);
                        let state = item_state(&w.hearth, &prov.target)?;
                        let history = history_kinds(&w.hearth, &prov.target)?;
                        w.record(Attempt::Succeeded { state: Some(state), history });
                    }
                    Err(reason) => w.record(Attempt::Rejected { reason }),
                }
                Ok(carry(w))
            },
        ),
        step_def(
            "the organ queue is read for {string}",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let w = world(&ctx)?;
                let bn = p(params, 0)?;
                match read_organ_queue(&store_at(&w.hearth), &bn) {
                    Ok(view) => {
                        w.with_evidence(|e| {
                            e.ranked_len = view.ranked.len();
                            e.unranked_len = view.unranked_candidates.len();
                            e.unranked = view
                                .unranked_candidates
                                .iter()
                                .map(|u| u.backlog_item_id.clone())
                                .collect();
                            e.ranked_ids =
                                view.ranked.iter().map(|r| r.backlog_item_id.clone()).collect();
                        });
                        // A queue READ never writes: the ranked partition may
                        // never contain a parked item, and a retained parked
                        // rank must stay byte-equal across the read.
                        w.record(Attempt::Succeeded { state: None, history: vec![] });
                    }
                    Err(reason) => w.record(Attempt::Rejected { reason: reason.to_string() }),
                }
                Ok(carry(w))
            },
        ),
        step_def(
            "the backlog registration surfaces are inspected",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, _params| {
                let w = world(&ctx)?;
                let mut surfaces = inspect_registration();
                surfaces.extend(inspect_real_hearth_registration(&w.hearth));
                w.set_registration(surfaces);
                Ok(carry(w))
            },
        ),
        step_def(
            "a real hearth holding one backlog item in every K8 state and two tracks",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, _params| {
                let w = world(&ctx)?;
                seed_real_registration_hearth(&w.hearth)?;
                Ok(carry(w))
            },
        ),
        step_def(
            "the backlog field schema is validated against input {string}",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let w = world(&ctx)?;
                let input = p(params, 0)?;
                match validate_named_input(&input) {
                    Ok(()) => w.record(Attempt::Succeeded { state: None, history: vec![] }),
                    Err(reason) => w.record(Attempt::Rejected { reason }),
                }
                Ok(carry(w))
            },
        ),
        step_def(
            "required-by-state validation is run for target {string}",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let w = world(&ctx)?;
                let target = parse_state(&p(params, 0)?)?;
                let item = w.item()?;
                // The always-required history is enforced by the reconcile layer,
                // not the state-local validator (§1.3).
                if let Err(e) = bi::validate_history(&w.history()) {
                    w.record(Attempt::Rejected { reason: e.to_string() });
                    return Ok(carry(w));
                }
                // The operation under test is the plan-named state-local
                // validator. `missing_required_by_state` is only the reporting
                // superset, so it may never be the thing that decides: if the
                // strict validator admits a target while the reporter still
                // names a MATRIX field, the two have diverged and this step
                // fails loud rather than presenting the reporter's answer.
                let strict = bi::validate_required_by_state(&item, target);
                let missing = bi::missing_required_by_state(&item, target);
                if strict.is_ok() {
                    let matrix_missing: Vec<&str> = missing
                        .iter()
                        .copied()
                        .filter(|t| !GUARD_ONLY_TOKENS.contains(t))
                        .collect();
                    if !matrix_missing.is_empty() {
                        return Err(format!(
                            "validate_required_by_state admitted target {:?} while \
                             missing_required_by_state still reports matrix field(s) {:?} — \
                             the strict validator and its reporter have diverged",
                            target.as_str(),
                            matrix_missing
                        ));
                    }
                }
                match strict {
                    Ok(()) => w.record(Attempt::Succeeded {
                        state: Some(target.as_str().to_string()),
                        history: vec![],
                    }),
                    Err(e) => w.record(Attempt::Rejected {
                        // Carry the reporter's complete set alongside the
                        // fail-fast reason so a scenario may name any missing
                        // field, not just the first one rejected.
                        reason: format!(
                            "{e} (missing required-by-state fields: {})",
                            missing.join(", ")
                        ),
                    }),
                }
                Ok(carry(w))
            },
        ),
        step_def(
            "the backlog item is serialized and reloaded",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, _params| {
                let w = world(&ctx)?;
                if w.disk_id().is_some() || w.prov().is_some() {
                    // Disk-backed: the operation under test is the REAL strict
                    // load (recover-then-reconcile), never a pure round-trip.
                    match real_reload(&w.hearth) {
                        Ok(()) => w.record(Attempt::Succeeded { state: None, history: vec![] }),
                        Err(reason) => w.record(Attempt::Rejected { reason }),
                    }
                    return Ok(carry(w));
                }
                let item = w.item()?;
                let history = w.history();
                match roundtrip(&item, &history) {
                    Ok(()) => w.record(Attempt::Succeeded {
                        state: Some(item.state.as_str().to_string()),
                        history: vec![],
                    }),
                    Err(reason) => w.record(Attempt::Rejected { reason }),
                }
                Ok(carry(w))
            },
        ),
        step_def(
            "the backlog hearth is first read through {string}",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let w = world(&ctx)?;
                let surface = p(params, 0)?;
                match first_read_surface(&w.hearth, &surface) {
                    Ok(()) => w.record(Attempt::Succeeded { state: None, history: vec![] }),
                    Err(reason) => w.record(Attempt::Rejected { reason }),
                }
                Ok(carry(w))
            },
        ),
        step_def(
            "the backlog policy is parsed with comparator {string} and age budget {string}",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let w = world(&ctx)?;
                let comparator = p(params, 0)?;
                let budget = p(params, 1)?;
                let comp = bi::parse_comparator(&comparator);
                let age = bi::parse_age_budget(&budget);
                match (comp, age) {
                    (Ok(_), Ok(_)) => w.record(Attempt::Succeeded { state: None, history: vec![] }),
                    (Err(e), _) | (_, Err(e)) => {
                        w.record(Attempt::Rejected { reason: e.to_string() })
                    }
                }
                Ok(carry(w))
            },
        ),
        step_def(
            "the cross-organ view is read",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, _params| {
                let w = world(&ctx)?;
                match read_cross_organ_view(&store_at(&w.hearth)) {
                    Ok(view) => {
                        w.with_evidence(|e| {
                            e.ranked_len = view.ranked.len();
                            e.unranked_len = view.unranked_candidates.len();
                            e.unranked = view
                                .unranked_candidates
                                .iter()
                                .map(|u| u.backlog_item_id.clone())
                                .collect();
                            e.ranked_ids =
                                view.ranked.iter().map(|r| r.backlog_item_id.clone()).collect();
                        });
                        w.record(Attempt::Succeeded { state: None, history: vec![] });
                    }
                    Err(reason) => w.record(Attempt::Rejected { reason: reason.to_string() }),
                }
                Ok(carry(w))
            },
        ),
        // ── Then: assertions that demand implemented behavior ────────────────
        check_def(
            "the backlog operation succeeds",
            &[(WORLD, WORLD_TY)],
            |ctx, _params| {
                expect_success(&world(&ctx)?)?;
                Ok(())
            },
        ),
        check_def(
            "the resulting backlog state is {string}",
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let (state, _) = expect_success(&world(&ctx)?)?;
                let want = p(params, 0)?;
                match state {
                    Some(s) if s == want => Ok(()),
                    other => Err(format!("expected state {want:?} but was {other:?}")),
                }
            },
        ),
        check_def(
            "the backlog history kinds are {string}",
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let (_, history) = expect_success(&world(&ctx)?)?;
                let want: Vec<String> = p(params, 0)?
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if history == want {
                    Ok(())
                } else {
                    Err(format!("expected history {want:?} but was {history:?}"))
                }
            },
        ),
        check_def(
            "the backlog operation is rejected",
            &[(WORLD, WORLD_TY)],
            |ctx, _params| {
                expect_rejected(&world(&ctx)?)?;
                Ok(())
            },
        ),
        check_def(
            "the backlog operation is rejected because {string}",
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let reason = expect_rejected(&world(&ctx)?)?;
                let want = p(params, 0)?;
                if reason.contains(&want) {
                    Ok(())
                } else {
                    Err(format!("expected rejection reason containing {want:?} but was {reason:?}"))
                }
            },
        ),
        check_def(
            "the backlog registration property {string} is satisfied",
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let want = p(params, 0)?;
                world(&ctx)?
                    .registration_property(&want)
                    .map_err(|e| format!("registration property {want:?} unproven: {e}"))
            },
        ),
        check_def(
            "the backlog field rule {string} holds",
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let want = p(params, 0)?;
                let reason = expect_rejected(&world(&ctx)?)
                    .map_err(|e| format!("field rule {want:?} unproven: {e}"))?;
                // Serde-layer rules are proven by the offending key/variant the
                // closed genesis shape itself names — never by a token the
                // fixture spliced into its own message.
                let needle = serde_rule_evidence(&want).unwrap_or(want.as_str());
                if reason.contains(needle) {
                    Ok(())
                } else {
                    Err(format!(
                        "expected field rule {want:?} to be refused by production text \
                         containing {needle:?} but the recorded reason was {reason:?}"
                    ))
                }
            },
        ),
        check_def(
            "the item is reported in the unranked partition",
            &[(WORLD, WORLD_TY)],
            |ctx, _params| {
                let w = world(&ctx)?;
                expect_success(&w)?;
                let prov = w
                    .prov()
                    .ok_or_else(|| "no lifecycle provenance was seeded".to_string())?;
                let evidence = w.evidence();
                // A parked item is not a pre-triage CANDIDATE: it is filtered
                // off the ranked view entirely while its rank stays byte-equal.
                // Assert exactly that instead of pretending it is unranked.
                let loaded = store_at(&w.hearth)
                    .load_item(&prov.target)
                    .map_err(|e| e.to_string())?;
                if loaded.item.state == State::Parked {
                    if evidence.ranked_ids.iter().any(|id| id == &prov.target) {
                        return Err(format!(
                            "parked item `{}` leaked into the ranked queue",
                            prov.target
                        ));
                    }
                    let retained = serde_yaml::to_string(&loaded.item.rank)
                        .map_err(|e| format!("serialize retained rank: {e}"))?;
                    match &evidence.rank_before {
                        Some(before) if before == &retained => return Ok(()),
                        Some(before) => {
                            return Err(format!(
                                "the parked rank was not byte-equal across the read:\nbefore:                                  {before}\nafter: {retained}"
                            ))
                        }
                        None => return Err("the parked rank was never captured".to_string()),
                    }
                }
                if evidence.unranked.iter().any(|id| id == &prov.target) {
                    Ok(())
                } else {
                    Err(format!(
                        "expected `{}` in the explicit pre-triage partition but it reported {:?}",
                        prov.target, evidence.unranked
                    ))
                }
            },
        ),
        check_def(
            "the backlog item round-trips byte-stably",
            &[(WORLD, WORLD_TY)],
            |ctx, _params| {
                expect_success(&world(&ctx)?)?;
                Ok(())
            },
        ),
        check_def(
            "the carried pickup bindings are byte-preserved",
            &[(WORLD, WORLD_TY)],
            |ctx, _params| {
                let w = world(&ctx)?;
                expect_success(&w)
                    .map_err(|e| format!("binding byte-preservation unproven: {e}"))?;
                let evidence = w.evidence();
                let before = evidence
                    .bindings_before
                    .ok_or_else(|| "the source bindings were never captured".to_string())?;
                let after = evidence
                    .bindings_after
                    .ok_or_else(|| "the target bindings were never captured".to_string())?;
                if before != after {
                    return Err(format!(
                        "the pickup bindings were not byte-preserved:\nbefore: {before}\nafter:                          {after}"
                    ));
                }
                if before.trim().is_empty() || before.contains("null") && !before.contains("run_id")
                {
                    return Err(
                        "no pickup bindings were carried, so preservation proves nothing".into(),
                    );
                }
                Ok(())
            },
        ),
        check_def(
            "the rendered actor status is byte-exact",
            &[(WORLD, WORLD_TY)],
            |ctx, _params| {
                let w = world(&ctx)?;
                expect_success(&w)
                    .map_err(|e| format!("strict actor-status rendering unproven: {e}"))?;
                let evidence = w.evidence();
                let live = evidence.status_live_before.ok_or_else(|| {
                    "the live status.yaml bytes were never captured".to_string()
                })?;
                let staged = evidence.status_staged.ok_or_else(|| {
                    "the prepared journal staged no status.yaml bytes".to_string()
                })?;
                let expected = evidence.status_expected.ok_or_else(|| {
                    "the strict rendering was never computed".to_string()
                })?;
                if staged != expected {
                    return Err(format!(
                        "the prepared journal did not stage the strict rendering.\n\
                         staged:\n{staged}\n---\nstrict-rendered:\n{expected}"
                    ));
                }
                // A staged value byte-equal to the live document would make the
                // comparison vacuous: the upsert must actually change something.
                if staged == live {
                    return Err(
                        "the staged status.yaml equals the live bytes, so byte-exactness \
                         proves nothing"
                            .to_string(),
                    );
                }
                if !staged.contains("Fixture-000002") {
                    return Err(format!(
                        "the staged status.yaml does not carry the advancing actor:\n{staged}"
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the cross-organ view aggregates both partitions",
            &[(WORLD, WORLD_TY)],
            |ctx, _params| {
                let w = world(&ctx)?;
                expect_success(&w)
                    .map_err(|e| format!("cross-organ aggregation unproven: {e}"))?;
                let evidence = w.evidence();
                if evidence.ranked_len < 2 {
                    return Err(format!(
                        "expected the aggregate to span more than one organ's ranked items but                          it held {}",
                        evidence.ranked_len
                    ));
                }
                if evidence.unranked_len == 0 {
                    return Err(
                        "expected the explicit pre-triage partition to be aggregated too".into(),
                    );
                }
                Ok(())
            },
        ),
        check_def(
            "no backlog residue remains under {string}",
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let w = world(&ctx)?;
                let sub = p(params, 0)?;
                let dir = w.hearth.join(&sub);
                // A rejected operation leaves NO transaction residue: no
                // journal, no staging, no `.tmp` sibling. The seeded source
                // item itself is expected to still be there — what must not
                // survive is any trace of the refused write.
                let mut residue: Vec<String> = Vec::new();
                if let Ok(entries) = std::fs::read_dir(dir.join(".transactions")) {
                    for entry in entries.flatten() {
                        residue.push(entry.path().display().to_string());
                    }
                }
                fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
                    if let Ok(entries) = std::fs::read_dir(dir) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.is_dir() {
                                walk(&path, out);
                            } else if path
                                .file_name()
                                .map(|n| n.to_string_lossy().ends_with(".tmp"))
                                .unwrap_or(false)
                            {
                                out.push(path.display().to_string());
                            }
                        }
                    }
                }
                walk(&dir, &mut residue);
                if residue.is_empty() {
                    Ok(())
                } else {
                    Err(format!("unexpected transaction residue: {residue:?}"))
                }
            },
        ),
    ]
}

/// Byte-stable YAML round-trip of an item plus its out-of-line history.
fn roundtrip(item: &BacklogItem, history: &[HistoryEntry]) -> Result<(), String> {
    let item_yaml = serde_yaml::to_string(item).map_err(|e| format!("serialize item: {e}"))?;
    let reloaded: BacklogItem =
        serde_yaml::from_str(&item_yaml).map_err(|e| format!("reload item: {e}"))?;
    if &reloaded != item {
        return Err("item did not round-trip byte-stably".to_string());
    }
    let hist_yaml =
        serde_yaml::to_string(history).map_err(|e| format!("serialize history: {e}"))?;
    let reloaded_hist: Vec<HistoryEntry> =
        serde_yaml::from_str(&hist_yaml).map_err(|e| format!("reload history: {e}"))?;
    if reloaded_hist != history {
        return Err("history did not round-trip byte-stably".to_string());
    }
    Ok(())
}

// ── Real on-disk storage fixtures (plan Task 4, §3 real-seam law) ────────────
//
// Every provenance below builds a REAL hearth through the PRODUCTION store:
// `build_genesis_commit` + `FileSystemBacklogItemAdapter::create_genesis` for
// the item, `prepare_k8_transition` + `commit_backlog_transition` for a
// governed move, and the real `ANVIL_TEST_BACKLOG_CRASH_AFTER` crash points for
// interruption. No `TestSnapshotAdapter`, no mock registry, no direct state
// edit stands in for an operation under test — a corruption fixture edits bytes
// only to CORRUPT them, which is the thing being detected.

use anvil_core::domain::snapshot::{commit_k8_transition, prepare_k8_transition};
use anvil_core::domain::backlog_item::TransitionOrigin;
use anvil_core::domain::shared_types::ActorIdentity;
use anvil_core::domain::backlog_manifest::{
    build_genesis_commit, render_backlog_registry, BacklogRegistryRow,
};
use anvil_core_hearth::fs_backlog_item_adapter::FileSystemBacklogItemAdapter;
use anvil_core::ports::backlog_item_port::BacklogItemPort;

fn fixture_actor(name: &str) -> ActorIdentity {
    ActorIdentity {
        name: name.to_string(),
        actor_type: "agent".to_string(),
        model: "claude-opus-5".to_string(),
        provider: "anthropic".to_string(),
        context_window: 200_000,
        sdk_version: "1.0.0".to_string(),
        entrypoint: "brine".to_string(),
        registered_at: "2026-07-25T00:00:00Z".to_string(),
    }
}

fn genesis_status_bytes(bi_id: &str) -> String {
    format!("version: 1\nkind: backlog_item\nstate: candidate\nid: {bi_id}\n")
}

/// Publish one item through the REAL genesis path and return its id.
fn publish_item(hearth: &std::path::Path, item: &BacklogItem) -> Result<String, String> {
    let store = FileSystemBacklogItemAdapter::new(hearth.to_path_buf());
    let actor = fixture_actor("Fixture-000001");
    let created = HistoryEntry {
        seq: 0,
        actor: actor.name.clone(),
        role: DriverRole::Intake,
        at: "2026-07-25T00:00:00Z".to_string(),
        kind: HistoryKind::Created,
        from_state: None,
        to_state: Some(State::Candidate),
        payload: None,
        note: None,
    };
    let mut rows: Vec<BacklogRegistryRow> = store
        .load_all()
        .map_err(|e| format!("load_all: {e}"))?
        .iter()
        .map(|l| BacklogRegistryRow {
            id: l.id.clone(),
            title: l.item.title.clone(),
            section: l.item.state.as_str().to_string(),
        })
        .collect();
    rows.push(BacklogRegistryRow {
        id: item.backlog_item_id.clone(),
        title: item.title.clone(),
        section: item.state.as_str().to_string(),
    });
    let old_hash = match std::fs::read(hearth.join("backlog_items.md")) {
        Ok(bytes) => Some(anvil_core::domain::content_hash::content_hash(&bytes)),
        Err(_) => None,
    };
    let commit = build_genesis_commit(
        &format!("genesis-{}", item.backlog_item_id),
        item,
        &created,
        &actor,
        &genesis_status_bytes(&item.backlog_item_id),
        &rows,
        old_hash,
        "20260725T000000000000",
        &item.backlog_item_id,
    )
    .map_err(|e| format!("build genesis: {e}"))?;
    store
        .create_genesis(commit)
        .map_err(|e| format!("create_genesis: {e}"))?;
    Ok(item.backlog_item_id.clone())
}

/// A ready-shaped candidate that can legally take row #1.
fn advanceable_candidate(id: &str) -> BacklogItem {
    let mut item = base_item();
    item.backlog_item_id = id.to_string();
    item.title = format!("advanceable {id}");
    item.rank = Some(full_rank(EffortClass::S, DependencyStatus::Ready));
    item
}

fn item_dir(hearth: &std::path::Path, bi_id: &str) -> std::path::PathBuf {
    hearth.join("backlog_items").join(bi_id)
}

/// Run one REAL governed `candidate -> ready` transition, optionally with a
/// crash point armed. Returns `Ok(())` when it committed, `Err(reason)` when the
/// production path refused or the armed crash fired.
fn real_advance(
    hearth: &std::path::Path,
    bi_id: &str,
    crash_after: Option<&str>,
) -> Result<(), String> {
    let adapter = FileSystemSnapshotAdapter::new(hearth.to_path_buf());
    let actor = fixture_actor("Fixture-000002");
    let (_, commit) = prepare_k8_transition(
        &adapter,
        bi_id,
        State::Candidate,
        State::Ready,
        DriverRole::OrganLoop,
        &actor,
        "2026-07-25T00:10:00Z",
        None,
        TransitionOrigin::public_snapshot(),
        &format!("advance-{bi_id}"),
        "20260725T001000000000",
        "advance",
    )
    .map_err(|e| format!("prepare: {e}"))?;

    if let Some(token) = crash_after {
        std::env::set_var("ANVIL_TEST_MODE", "1");
        std::env::set_var("ANVIL_TEST_BACKLOG_CRASH_AFTER", token);
    }
    let outcome = commit_k8_transition(&adapter, commit).map_err(|e| format!("commit: {e}"));
    if crash_after.is_some() {
        std::env::remove_var("ANVIL_TEST_BACKLOG_CRASH_AFTER");
        std::env::remove_var("ANVIL_TEST_MODE");
    }
    outcome
}

/// Seed a real on-disk provenance. `Ok(Some(id))` when this provenance is
/// storage-backed; `Ok(None)` when it is not one of Task 4's.
fn seed_storage_provenance(
    hearth: &std::path::Path,
    prov: &str,
) -> Result<Option<String>, String> {
    let id = "bi_str00001";
    match prov {
        // ── strict corruption ───────────────────────────────────────────────
        "duplicate_id" => {
            publish_item(hearth, &advanceable_candidate(id))?;
            let twin = item_dir(hearth, "bi_str00002");
            copy_tree(&item_dir(hearth, id), &twin)?;
            // The twin still declares the ORIGINAL id — the same
            // backlog_item_id published under two directories.
            Ok(Some(id.to_string()))
        }
        "missing_history" => {
            publish_item(hearth, &advanceable_candidate(id))?;
            std::fs::remove_file(item_dir(hearth, id).join("history.yaml"))
                .map_err(|e| format!("remove history: {e}"))?;
            Ok(Some(id.to_string()))
        }
        "noncontiguous_history" => {
            publish_item(hearth, &advanceable_candidate(id))?;
            let mut h = created_history();
            let mut gapped = h.remove(0);
            gapped.seq = 0;
            let mut second = gapped.clone();
            second.seq = 2;
            second.kind = HistoryKind::ShapeEdit;
            second.to_state = None;
            write_history(hearth, id, &[gapped, second])?;
            Ok(Some(id.to_string()))
        }
        "genesis_state_change" => {
            publish_item(hearth, &advanceable_candidate(id))?;
            let mut h = created_history();
            h[0].kind = HistoryKind::StateChange;
            write_history(hearth, id, &h)?;
            Ok(Some(id.to_string()))
        }
        "ledger_mirror_mismatch" => {
            publish_item(hearth, &advanceable_candidate(id))?;
            let mut item = advanceable_candidate(id);
            item.state = State::Ready;
            write_item(hearth, id, &item)?;
            Ok(Some(id.to_string()))
        }
        "duplicate_event" => {
            publish_item(hearth, &advanceable_candidate(id))?;
            let dir = item_dir(hearth, id).join("transitions");
            let first = std::fs::read_dir(&dir)
                .map_err(|e| format!("read transitions: {e}"))?
                .flatten()
                .next()
                .ok_or_else(|| "no genesis event was written".to_string())?
                .path();
            let bytes = std::fs::read(&first).map_err(|e| format!("read event: {e}"))?;
            std::fs::write(dir.join("zzz_duplicate.yaml"), bytes)
                .map_err(|e| format!("write duplicate event: {e}"))?;
            Ok(Some(id.to_string()))
        }
        "conflicting_history_seq" => {
            publish_item(hearth, &advanceable_candidate(id))?;
            let mut h = created_history();
            let mut dup = h[0].clone();
            dup.seq = 0;
            dup.kind = HistoryKind::ShapeEdit;
            dup.to_state = None;
            h.push(dup);
            write_history(hearth, id, &h)?;
            Ok(Some(id.to_string()))
        }
        "state_change_ledger_skew" => {
            publish_item(hearth, &advanceable_candidate(id))?;
            real_advance(hearth, id, None)?;
            // Rewrite the ledger event's target so the mirrored state_change and
            // the authoritative event disagree.
            let dir = item_dir(hearth, id).join("transitions");
            for entry in std::fs::read_dir(&dir).map_err(|e| format!("{e}"))?.flatten() {
                let raw = std::fs::read_to_string(entry.path()).map_err(|e| format!("{e}"))?;
                if raw.contains("to: ready") {
                    std::fs::write(entry.path(), raw.replace("to: ready", "to: parked"))
                        .map_err(|e| format!("{e}"))?;
                }
            }
            Ok(Some(id.to_string()))
        }
        "malformed_status_yaml" => {
            publish_item(hearth, &advanceable_candidate(id))?;
            std::fs::write(item_dir(hearth, id).join("status.yaml"), "kind: [unclosed\n")
                .map_err(|e| format!("{e}"))?;
            Ok(Some(id.to_string()))
        }

        // ── interruption + per-effect crash, through the REAL crash points ──
        "interrupted_prepared" => {
            publish_item(hearth, &advanceable_candidate(id))?;
            expect_crash(real_advance(hearth, id, Some("prepared")), "prepared")?;
            Ok(Some(id.to_string()))
        }
        "interrupted_applying" => {
            publish_item(hearth, &advanceable_candidate(id))?;
            expect_crash(real_advance(hearth, id, Some("applying")), "applying")?;
            Ok(Some(id.to_string()))
        }
        "interrupted_committed" => {
            publish_item(hearth, &advanceable_candidate(id))?;
            expect_crash(real_advance(hearth, id, Some("committed")), "committed")?;
            Ok(Some(id.to_string()))
        }
        "actor_status_rendered" => {
            publish_item(hearth, &advanceable_candidate(id))?;
            // Crash the REAL governed advance the instant its journal reaches
            // `prepared` — before a single live byte moves — so the staged
            // status.yaml bytes survive on disk and can be compared to the
            // strict rendering the plan says they must equal.
            expect_crash(real_advance(hearth, id, Some("prepared")), "prepared")?;
            Ok(Some(id.to_string()))
        }
        "stale_prepared_capability" => {
            publish_item(hearth, &advanceable_candidate(id))?;
            Ok(Some(id.to_string()))
        }
        other => {
            if let Some(effect) = other.strip_prefix("crash_") {
                let token = crash_token_for(hearth, id, effect)?;
                if effect == "after_publish" {
                    // `Publish` is a genesis-only effect, so this crash point
                    // can only be exercised by crashing GENESIS itself.
                    std::env::set_var("ANVIL_TEST_MODE", "1");
                    std::env::set_var("ANVIL_TEST_BACKLOG_CRASH_AFTER", &token);
                    let outcome = publish_item(hearth, &advanceable_candidate(id)).map(|_| ());
                    std::env::remove_var("ANVIL_TEST_BACKLOG_CRASH_AFTER");
                    std::env::remove_var("ANVIL_TEST_MODE");
                    expect_crash(outcome, &token)?;
                    return Ok(Some(id.to_string()));
                }
                publish_item(hearth, &advanceable_candidate(id))?;
                expect_crash(real_advance(hearth, id, Some(&token)), &token)?;
                return Ok(Some(id.to_string()));
            }
            if let Some(class) = other.strip_prefix("conflict_") {
                publish_item(hearth, &advanceable_candidate(id))?;
                seed_third_value_conflict(hearth, id, class)?;
                return Ok(Some(id.to_string()));
            }
            if other == "interrupted_third_value_conflict" {
                publish_item(hearth, &advanceable_candidate(id))?;
                seed_third_value_conflict(hearth, id, "item")?;
                return Ok(Some(id.to_string()));
            }
            Ok(None)
        }
    }
}

fn expect_crash(outcome: Result<(), String>, token: &str) -> Result<(), String> {
    match outcome {
        Err(reason) if reason.contains("test crash point") => Ok(()),
        Err(other) => Err(format!(
            "expected the armed crash point '{token}' to fire, but the store failed with: \
             {other}"
        )),
        Ok(()) => Err(format!(
            "expected the armed crash point '{token}' to fire, but the transaction committed"
        )),
    }
}

/// The exact crash token for a per-effect point, derived from the live journal
/// so `after_history` / `after_event` name the real sequence and filename.
fn crash_token_for(
    hearth: &std::path::Path,
    id: &str,
    effect: &str,
) -> Result<String, String> {
    Ok(match effect {
        "after_publish" => format!("after_publish:{id}"),
        "after_status" => format!("after_status:{id}"),
        "after_item" => format!("after_item:{id}"),
        "after_history" => format!("after_history:{id}:1"),
        "after_event" => {
            // The event filename is preallocated deterministically by the
            // fixture's own prefix/suffix, mirroring the production allocation.
            let _ = hearth;
            format!("after_event:{id}:20260725T001000000000_Fixture-000002_advance.yaml")
        }
        "after_registry" => "after_registry".to_string(),
        "after_cleanup" => "after_cleanup".to_string(),
        other => return Err(format!("unknown crash effect '{other}'")),
    })
}

/// Interrupt a transaction, then put a THIRD value at one target so recovery
/// must refuse rather than roll forward over it.
fn seed_third_value_conflict(
    hearth: &std::path::Path,
    id: &str,
    class: &str,
) -> Result<(), String> {
    expect_crash(real_advance(hearth, id, Some("applying")), "applying")?;
    let dir = item_dir(hearth, id);
    match class {
        "status" => std::fs::write(dir.join("status.yaml"), "version: 1\nkind: backlog_item\nstate: candidate\nthird: value\n")
            .map_err(|e| format!("{e}"))?,
        "item" => {
            let mut item = advanceable_candidate(id);
            item.title = "a third value nobody prepared".to_string();
            write_item(hearth, id, &item)?
        }
        "history" => {
            let mut h = created_history();
            h[0].note = Some("a third value nobody prepared".to_string());
            write_history(hearth, id, &h)?
        }
        "event" => std::fs::write(
            dir.join("transitions")
                .join("20260725T001000000000_Fixture-000002_advance.yaml"),
            "to: parked\nat: \"2026-07-25T00:10:00Z\"\nactor: Interloper\nrole: nick_shape\n",
        )
        .map_err(|e| format!("{e}"))?,
        "registry" => std::fs::write(
            hearth.join("backlog_items.md"),
            "# Backlog Items\n\n## a third value nobody prepared\n",
        )
        .map_err(|e| format!("{e}"))?,
        "publish" => {
            // Replace the published directory's item with an unrelated id so the
            // journal's target is neither its expected old value nor desired.
            let mut item = advanceable_candidate(id);
            item.title = "a third value at the published target".to_string();
            item.description = Some("publish-class third value".to_string());
            write_item(hearth, id, &item)?
        }
        other => return Err(format!("unknown conflict target class '{other}'")),
    }
    Ok(())
}

fn write_item(
    hearth: &std::path::Path,
    id: &str,
    item: &BacklogItem,
) -> Result<(), String> {
    let yaml = serde_yaml::to_string(item).map_err(|e| format!("{e}"))?;
    std::fs::write(item_dir(hearth, id).join("item.yaml"), yaml).map_err(|e| format!("{e}"))
}

fn write_history(
    hearth: &std::path::Path,
    id: &str,
    entries: &[HistoryEntry],
) -> Result<(), String> {
    let yaml = serde_yaml::to_string(entries).map_err(|e| format!("{e}"))?;
    std::fs::write(item_dir(hearth, id).join("history.yaml"), yaml).map_err(|e| format!("{e}"))
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("{e}"))?;
    for entry in std::fs::read_dir(from).map_err(|e| format!("{e}"))?.flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target).map_err(|e| format!("{e}"))?;
        }
    }
    Ok(())
}

/// The REAL strict reload: recover under the store, then strictly load every
/// published item. This is the operation under test for every `@storage`
/// corruption / recovery / conflict scenario.
fn real_reload(hearth: &std::path::Path) -> Result<(), String> {
    FileSystemBacklogItemAdapter::new(hearth.to_path_buf())
        .load_all()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Read the hearth for the first time through a first-class scan surface.
fn first_read_surface(hearth: &std::path::Path, surface: &str) -> Result<(), String> {
    match surface {
        "catalog" => {
            let reader = FileSystemHearthReader::new(hearth.to_path_buf());
            CatalogQueryHandler::execute(&reader)
                .map(|_| ())
                .map_err(|e| format!("catalog: {e}"))
        }
        "checkin" => {
            use anvil_core_hearth::fs_checkin_query_adapter::FileSystemCheckinQueryAdapter;
            use anvil_core::ports::checkin_query_port::CheckinQueryPort;
            FileSystemCheckinQueryAdapter::new(hearth.to_path_buf())
                .list_artifacts()
                .map(|_| ())
                .map_err(|e| format!("checkin: {e}"))
        }
        "describe" => {
            use anvil_core_hearth::fs_describe_adapter::FileSystemDescribeAdapter;
            use anvil_core::ports::describe_port::DescribePort;
            let adapter = FileSystemDescribeAdapter::new(hearth.to_path_buf());
            match adapter.read_instance("bi_str00001") {
                Ok(_) => Ok(()),
                Err(e) => Err(format!("describe: {e}")),
            }
        }
        other => Err(format!("unknown first-read surface '{other}'")),
    }
}

/// Prepare a capability, move the item underneath it, then attempt the consume.
/// The store must refuse without writing a byte.
fn attempt_stale_consume(hearth: &std::path::Path, bi_id: &str) -> Result<(), String> {
    let adapter = FileSystemSnapshotAdapter::new(hearth.to_path_buf());
    let actor = fixture_actor("Fixture-000003");
    let (_, commit) = prepare_k8_transition(
        &adapter,
        bi_id,
        State::Candidate,
        State::Ready,
        DriverRole::OrganLoop,
        &actor,
        "2026-07-25T00:20:00Z",
        None,
        TransitionOrigin::public_snapshot(),
        &format!("stale-{bi_id}"),
        "20260725T002000000000",
        "stale",
    )
    .map_err(|e| format!("prepare: {e}"))?;

    // The item moves AFTER the capability was prepared.
    let mut moved = advanceable_candidate(bi_id);
    moved.title = "moved after preparation".to_string();
    write_item(hearth, bi_id, &moved)?;

    commit_k8_transition(&adapter, commit)
        .map(|_| ())
        .map_err(|e| e.to_string())
}


/// The named genesis inputs the `@storage` / `@genesis` scenarios use, decoded
/// and published through the REAL `build_backlog_genesis` + strict store.
pub fn genesis_input_json(name: &str) -> Result<String, String> {
    let valid = r#"{
      "business_node_id": "bn_abcdef0123",
      "title": "a genuinely new candidate",
      "action_class": "dev",
      "intake": {
        "edge": "spark_triage",
        "evidence_refs": [{"kind": "spark", "id": "sp_seed01"}]
      },
      "origin_binding": {
        "value_gap_served": {"kind": "temper_measure", "id": "tm_valuegap01"},
        "minting_council_id": null,
        "experiment_id": null,
        "predicted_value": null
      }
    }"#;
    Ok(match name {
        "valid_candidate" | "reread_after_restart" => valid.to_string(),
        "caller_supplied_id" => valid.replacen('{', "{\"backlog_item_id\": \"bi_forged01\",", 1),
        "caller_supplied_state" => valid.replacen('{', "{\"state\": \"ready\",", 1),
        "caller_supplied_rank" => valid.replacen('{', "{\"rank\": {\"position\": 1},", 1),
        "caller_supplied_history" => valid.replacen('{', "{\"history\": [],", 1),
        "unknown_key" => valid.replacen('{', "{\"totally_unknown\": true,", 1),
        "missing_evidence" => valid.replace(r#"[{"kind": "spark", "id": "sp_seed01"}]"#, "[]"),
        "missing_origin" => {
            let start = valid.find("\"origin_binding\"").ok_or("origin_binding not present")?;
            let mut out = valid[..start].trim_end().trim_end_matches(',').to_string();
            out.push_str("\n    }");
            out
        }
        "both_predictor_ids" => valid
            .replace("\"minting_council_id\": null", "\"minting_council_id\": \"co_seed01\"")
            .replace("\"experiment_id\": null", "\"experiment_id\": \"ex_seed01\"")
            .replace("\"predicted_value\": null", "\"predicted_value\": 2.5"),
        "predictor_without_value" => valid
            .replace("\"minting_council_id\": null", "\"minting_council_id\": \"co_seed01\""),
        "invalid_bn_id" => valid.replace("bn_abcdef0123", "bn_ILLEGAL"),
        other => return Err(format!("unknown genesis input '{other}'")),
    })
}

/// Run the REAL typed genesis: the pure `build_backlog_genesis` decoder /
/// validator / minter followed by the strict store's exact-ID publication.
fn attempt_genesis(hearth: &std::path::Path, input: &str) -> Result<String, String> {
    use anvil_core::domain::begin::{build_backlog_genesis, render_backlog_genesis_status};
    use anvil_core_hearth::fs_transition_event_adapter::{hi_res_prefix, short_random_id};
    let json = genesis_input_json(input)?;
    let actor = fixture_actor("Genesis-000001");
    let at = "2026-07-25T00:00:00Z";
    let (item, created) =
        build_backlog_genesis(&json, &actor, at).map_err(|e| format!("{e}"))?;
    let store = FileSystemBacklogItemAdapter::new(hearth.to_path_buf());
    let mut rows: Vec<BacklogRegistryRow> = store
        .load_all()
        .map_err(|e| format!("load_all: {e}"))?
        .iter()
        .map(|l| BacklogRegistryRow {
            id: l.id.clone(),
            title: l.item.title.clone(),
            section: l.item.state.as_str().to_string(),
        })
        .collect();
    rows.push(BacklogRegistryRow {
        id: item.backlog_item_id.clone(),
        title: item.title.clone(),
        section: item.state.as_str().to_string(),
    });
    let old_hash = std::fs::read(hearth.join("backlog_items.md"))
        .ok()
        .map(|b| anvil_core::domain::content_hash::content_hash(&b));
    let commit = anvil_core::domain::backlog_manifest::build_genesis_commit(
        &format!("genesis-{}", item.backlog_item_id),
        &item,
        &created,
        &actor,
        &render_backlog_genesis_status(&item, &actor),
        &rows,
        old_hash,
        &hi_res_prefix(),
        &short_random_id(),
    )
    .map_err(|e| format!("{e}"))?;
    store.create_genesis(commit).map_err(|e| format!("{e}"))?;
    Ok(item.backlog_item_id)
}

// ── Real lifecycle provenances (plan Task 7) ────────────────────────────────
//
// Every provenance below is built by REAL production operations against a real
// temporary hearth: `build_genesis_commit` + `create_genesis` publishes an item,
// `execute_backlog_mutation` runs each named typed mutation through the strict
// store's journaled compound writer, and `prepare_k8_transition` +
// `commit_k8_transition` runs each governed lifecycle move. There is no interim
// guard-seeded fixture, no `TestSnapshotAdapter`, and no direct state edit
// standing in for an operation under test.

use anvil_core::domain::backlog_item::{
    execute_backlog_mutation, read_cross_organ_view, read_organ_queue, BacklogMutation,
    BacklogPolicy, FieldEdit, OutcomeBindingDecl, ProposedPosition, ShapeEditBody,
};
use anvil_core_hearth::fs_transition_event_adapter::{hi_res_prefix, short_random_id};

/// The organ every lifecycle provenance lives in (the queue scenarios name it).
const ORGAN: &str = "bn_0rgan00001";
/// A second organ, for cross-organ aggregation and cross-organ refusal.
const ORGAN2: &str = "bn_0rgan00002";

/// What a lifecycle scenario's `When` step operates on.
#[derive(Clone, Debug, Default)]
struct Provenance {
    target: String,
    organ: String,
    /// The ATTEND approver a `#5` pickup presents. `None` is the explicit
    /// no-approval case, never an oversight.
    approver: Option<String>,
    proposal_id: Option<String>,
    /// The ids a DECIDE proposal covers, in the order the `When` proposes them.
    proposal_targets: Vec<String>,
}

/// Evidence a later assertion needs that the pass/fail outcome cannot carry.
#[derive(Clone, Debug, Default)]
struct Evidence {
    unranked: Vec<String>,
    ranked_ids: Vec<String>,
    /// The exact retained rank bytes captured before a read, so a parked item's
    /// byte-equality is provable rather than asserted.
    rank_before: Option<String>,
    ranked_len: usize,
    unranked_len: usize,
    bindings_before: Option<String>,
    bindings_after: Option<String>,
    /// The live `status.yaml` bytes at the moment the transaction was prepared.
    status_live_before: Option<String>,
    /// The exact bytes the PREPARED journal staged for `status.yaml`.
    status_staged: Option<String>,
    /// The exact bytes the strict actor upsert THEN the `state:` header
    /// projection yield for those live bytes, that actor and that target state
    /// — both legs computed independently here. A K8 advance stages ONE
    /// status.yaml effect carrying both, so the header and the transition event
    /// commit or roll back together.
    status_expected: Option<String>,
}

/// Read the prepared (not yet applied) journal and pull out the exact
/// `status.yaml` bytes it staged for `bi_id`, alongside the live bytes it was
/// rendered from and the strict rendering those bytes must equal.
fn capture_staged_status(
    hearth: &std::path::Path,
    bi_id: &str,
) -> Result<(String, String, String), String> {
    use anvil_core::domain::actor_configuration::{
        render_upserted_actor_configuration, MalformedPolicy,
    };
    use anvil_core::ports::backlog_item_port::{
        BacklogEffect, BacklogJournalManifest, BACKLOG_TRANSACTIONS_DIR,
    };

    let live = std::fs::read_to_string(item_dir(hearth, bi_id).join("status.yaml"))
        .map_err(|e| format!("read live status.yaml: {e}"))?;

    let root = hearth.join("backlog_items").join(BACKLOG_TRANSACTIONS_DIR);
    let mut staged: Option<String> = None;
    for entry in std::fs::read_dir(&root)
        .map_err(|e| format!("read journal root {}: {e}", root.display()))?
        .flatten()
    {
        let manifest_path = entry.path().join("manifest.yaml");
        if !manifest_path.is_file() {
            continue;
        }
        let raw = std::fs::read_to_string(&manifest_path)
            .map_err(|e| format!("read {}: {e}", manifest_path.display()))?;
        let manifest: BacklogJournalManifest =
            serde_yaml::from_str(&raw).map_err(|e| format!("parse journal manifest: {e}"))?;
        for effect in &manifest.effects {
            if let BacklogEffect::Status {
                bi_id: effect_id,
                desired,
                ..
            } = effect
            {
                if effect_id == bi_id {
                    staged = Some(desired.clone());
                }
            }
        }
    }
    let staged = staged.ok_or_else(|| {
        format!(
            "the prepared journal staged NO status.yaml effect for '{bi_id}' — the governed \
             advance never rendered the actor into status.yaml"
        )
    })?;

    let after_actor = render_upserted_actor_configuration(
        &live,
        &fixture_actor("Fixture-000002"),
        MalformedPolicy::Strict,
    )
    .map_err(|e| format!("strict rendering refused the live status.yaml: {e}"))?
    .ok_or_else(|| {
        "the strict rendering was a no-op, so byte-exactness proves nothing".to_string()
    })?;
    // `real_advance` drives candidate -> ready, so the staged bytes must carry
    // the re-projected header as well as the actor upsert.
    let expected = anvil_core::domain::status_header::set_state_header(&after_actor, "ready");

    Ok((live, staged, expected))
}

fn parse_role(raw: &str) -> Result<DriverRole, String> {
    Ok(match raw {
        "nick_shape" => DriverRole::NickShape,
        "organ_loop" => DriverRole::OrganLoop,
        "orchestrator" => DriverRole::Orchestrator,
        "track_driver" => DriverRole::TrackDriver,
        "engine_auto" => DriverRole::EngineAuto,
        "intake" => DriverRole::Intake,
        other => return Err(format!("unknown driver role token: {other}")),
    })
}

fn store_at(hearth: &std::path::Path) -> FileSystemBacklogItemAdapter {
    FileSystemBacklogItemAdapter::new(hearth.to_path_buf())
}

/// A monotonically increasing fixture clock, so history ordering and event
/// filenames are deterministic within one scenario.
fn tick() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    format!("2026-07-25T{:02}:{:02}:{:02}Z", 1 + n / 3600, (n / 60) % 60, n % 60)
}

/// Run one named mutation through the REAL store-backed service.
fn mutate(
    hearth: &std::path::Path,
    mutation: BacklogMutation,
    role: DriverRole,
    actor: &str,
) -> Result<anvil_core::domain::backlog_item::BacklogMutationOutcome, String> {
    let store = store_at(hearth);
    let op = mutation.operation();
    execute_backlog_mutation(
        &store,
        &mutation,
        &BacklogPolicy::default(),
        role,
        &fixture_actor(actor),
        &tick(),
        &format!("{op}-{}", short_random_id()),
    )
    .map_err(|e| e.to_string())
}

/// Run one REAL governed lifecycle move through the prepared/compound seam.
fn govern(
    hearth: &std::path::Path,
    bi_id: &str,
    from: State,
    to: State,
    role: DriverRole,
    approver: Option<&str>,
) -> Result<(), String> {
    let adapter = FileSystemSnapshotAdapter::new(hearth.to_path_buf());
    let at = tick();
    let (_, commit) = prepare_k8_transition(
        &adapter,
        bi_id,
        from,
        to,
        role,
        &fixture_actor("Fixture-000010"),
        &at,
        approver,
        TransitionOrigin::public_snapshot(),
        &format!("advance-{bi_id}-{}", short_random_id()),
        &hi_res_prefix(),
        &short_random_id(),
    )
    .map_err(|e| e.to_string())?;
    commit_k8_transition(&adapter, commit).map_err(|e| e.to_string())
}

/// Publish a bare candidate carrying only the always-required fields.
fn mint_bare(
    hearth: &std::path::Path,
    id: &str,
    organ: &str,
    title: &str,
) -> Result<String, String> {
    let mut item = base_item();
    item.backlog_item_id = id.to_string();
    item.business_node_id = organ.to_string();
    item.title = title.to_string();
    item.effort_class = None;
    item.playbook_binding = None;
    item.rank = None;
    publish_item(hearth, &item)
}

fn shape_effort_and_playbook() -> ShapeEditBody {
    ShapeEditBody {
        effort_class: Some(FieldEdit::Set(EffortClass::S)),
        playbook_binding: Some(FieldEdit::Set(base_playbook())),
        ..Default::default()
    }
}

fn shape_rank_inputs(magnitude: f64, weight: f64, dep: DependencyStatus) -> ShapeEditBody {
    ShapeEditBody {
        value_gap_magnitude: Some(FieldEdit::Set(ValueGapMagnitude {
            r#ref: temper_value_gap(),
            magnitude,
        })),
        nick_weight: Some(FieldEdit::Set(weight)),
        dependency_readiness: Some(FieldEdit::Set(DependencyReadiness {
            status: dep,
            blocker_refs: vec![],
        })),
        ..Default::default()
    }
}

fn shape_wake() -> ShapeEditBody {
    ShapeEditBody {
        wake_condition: Some(FieldEdit::Set(WakeCondition {
            kind: WakeKind::ItemState,
            r#ref: Some(evid(EvidenceKind::BacklogItem, "bi_wake000001")),
            predicate: "state==done".to_string(),
        })),
        ..Default::default()
    }
}

fn shape_supersede() -> ShapeEditBody {
    ShapeEditBody {
        superseded_by: Some(FieldEdit::Set("bi_super0001".to_string())),
        ..Default::default()
    }
}

fn shape(
    hearth: &std::path::Path,
    id: &str,
    body: ShapeEditBody,
    role: DriverRole,
) -> Result<(), String> {
    mutate(
        hearth,
        BacklogMutation::ShapeEdit { bi_id: id.to_string(), body },
        role,
        "Shaper-000001",
    )
    .map(|_| ())
}

fn recompute(hearth: &std::path::Path, organ: &str) -> Result<Vec<String>, String> {
    mutate(
        hearth,
        BacklogMutation::RecomputeRank { business_node_id: organ.to_string() },
        DriverRole::OrganLoop,
        "OrganLoop-000001",
    )
    .map(|o| o.unranked)
}

fn stamp(hearth: &std::path::Path, id: &str) -> Result<(), String> {
    mutate(
        hearth,
        BacklogMutation::StampExecutionBinding {
            bi_id: id.to_string(),
            execution_binding: exec_binding(),
            outcome_binding: OutcomeBindingDecl {
                success_measure_id: Some("sm_seed01".to_string()),
                tree_node: "tree/seed".to_string(),
                reading_status: ReadingStatus::Registered,
            },
        },
        DriverRole::TrackDriver,
        "TrackDriver-000001",
    )
    .map(|_| ())
}

fn signoff(hearth: &std::path::Path, id: &str) -> Result<(), String> {
    mutate(
        hearth,
        BacklogMutation::RecordOutcomeSignoff {
            bi_id: id.to_string(),
            approver: "Nick".to_string(),
        },
        DriverRole::NickShape,
        "Nick-000001",
    )
    .map(|_| ())
}

/// A candidate shaped by ONE real `shape_edit` carrying effort, playbook and all
/// three caller-authored rank inputs. Its rank is still unmaterialized: only
/// `recompute_rank` creates a complete rank.
fn shaped_candidate(
    hearth: &std::path::Path,
    id: &str,
    organ: &str,
    magnitude: f64,
    dep: DependencyStatus,
) -> Result<String, String> {
    mint_bare(hearth, id, organ, &format!("shaped {id}"))?;
    let mut body = shape_effort_and_playbook();
    let inputs = shape_rank_inputs(magnitude, 1.0, dep);
    body.value_gap_magnitude = inputs.value_gap_magnitude;
    body.nick_weight = inputs.nick_weight;
    body.dependency_readiness = inputs.dependency_readiness;
    shape(hearth, id, body, DriverRole::OrganLoop)?;
    Ok(id.to_string())
}

/// Publish an already-shaped, already-ranked candidate through the real exact-ID
/// genesis commit. Used where a scenario asserts the history a NAMED PRODUCER
/// appends and the shaping steps would only be provenance noise.
fn publish_shaped(hearth: &std::path::Path, id: &str, organ: &str) -> Result<String, String> {
    publish_shaped_aged(hearth, id, organ, 0)
}

/// The same, with a committed re-rank age already accumulated, so an aging
/// scenario does not have to spend one history entry per cycle to reach its
/// budget.
fn publish_shaped_aged(
    hearth: &std::path::Path,
    id: &str,
    organ: &str,
    age: u32,
) -> Result<String, String> {
    let mut item = base_item();
    item.backlog_item_id = id.to_string();
    item.business_node_id = organ.to_string();
    item.title = format!("pre-shaped {id}");
    let mut rank = full_rank(EffortClass::S, DependencyStatus::Ready);
    rank.inputs.age = age;
    item.rank = Some(rank);
    publish_item(hearth, &item)
}

/// A ranked candidate: one shape_edit carrying effort, playbook and all three
/// caller-authored rank inputs, then the organ re-rank that materializes rank.
fn ranked_candidate(
    hearth: &std::path::Path,
    id: &str,
    organ: &str,
    magnitude: f64,
    dep: DependencyStatus,
) -> Result<String, String> {
    mint_bare(hearth, id, organ, &format!("ranked {id}"))?;
    let mut body = shape_effort_and_playbook();
    let inputs = shape_rank_inputs(magnitude, 1.0, dep);
    body.value_gap_magnitude = inputs.value_gap_magnitude;
    body.nick_weight = inputs.nick_weight;
    body.dependency_readiness = inputs.dependency_readiness;
    shape(hearth, id, body, DriverRole::OrganLoop)?;
    recompute(hearth, organ)?;
    Ok(id.to_string())
}

/// A ranked, dependency-ready `ready` item reached through the governed #1.
fn ready_item(hearth: &std::path::Path, id: &str, organ: &str, magnitude: f64) -> Result<String, String> {
    ranked_candidate(hearth, id, organ, magnitude, DependencyStatus::Ready)?;
    govern(hearth, id, State::Candidate, State::Ready, DriverRole::OrganLoop, None)?;
    Ok(id.to_string())
}

/// The same `ready` item from a pre-shaped publication, so a named-producer
/// scenario sees `created, state_change` and nothing else before its operation.
fn shaped_ready(hearth: &std::path::Path, id: &str, organ: &str) -> Result<String, String> {
    publish_shaped(hearth, id, organ)?;
    govern(hearth, id, State::Candidate, State::Ready, DriverRole::OrganLoop, None)?;
    Ok(id.to_string())
}

/// A stamped, approved, top-ready item already moved into `in_flight`.
fn in_flight_item(hearth: &std::path::Path, id: &str, organ: &str) -> Result<String, String> {
    shaped_ready(hearth, id, organ)?;
    stamp(hearth, id)?;
    govern(hearth, id, State::Ready, State::InFlight, DriverRole::TrackDriver, Some("Nick"))?;
    Ok(id.to_string())
}

/// A `#7`-origin parked item: ranked at ready, wake staged, then parked.
fn parked_from_7(hearth: &std::path::Path, id: &str, organ: &str) -> Result<String, String> {
    ready_item(hearth, id, organ, 9.0)?;
    shape(hearth, id, shape_wake(), DriverRole::OrganLoop)?;
    govern(hearth, id, State::Ready, State::Parked, DriverRole::OrganLoop, None)?;
    Ok(id.to_string())
}

/// A `#11`-origin parked item: both pickup bindings carried through the park.
fn parked_from_11(hearth: &std::path::Path, id: &str, organ: &str) -> Result<String, String> {
    in_flight_item(hearth, id, organ)?;
    shape(hearth, id, shape_wake(), DriverRole::TrackDriver)?;
    govern(hearth, id, State::InFlight, State::Parked, DriverRole::TrackDriver, None)?;
    Ok(id.to_string())
}

/// A `#2`-origin parked item that never acquired rank.
fn parked_from_2_rankless(hearth: &std::path::Path, id: &str, organ: &str) -> Result<String, String> {
    mint_bare(hearth, id, organ, "rankless parked")?;
    shape(hearth, id, shape_effort_and_playbook(), DriverRole::OrganLoop)?;
    shape(hearth, id, shape_wake(), DriverRole::OrganLoop)?;
    govern(hearth, id, State::Candidate, State::Parked, DriverRole::OrganLoop, None)?;
    Ok(id.to_string())
}

/// The exact YAML bytes of an item's two pickup bindings, for byte-preservation
/// evidence across a transition.
fn binding_bytes(hearth: &std::path::Path, id: &str) -> Result<String, String> {
    let loaded = store_at(hearth).load_item(id).map_err(|e| e.to_string())?;
    let exec = serde_yaml::to_string(&loaded.item.execution_binding)
        .map_err(|e| format!("serialize execution_binding: {e}"))?;
    let outcome = serde_yaml::to_string(&loaded.item.outcome_binding)
        .map_err(|e| format!("serialize outcome_binding: {e}"))?;
    Ok(format!("{exec}\u{1}{outcome}"))
}

fn history_kinds(hearth: &std::path::Path, id: &str) -> Result<Vec<String>, String> {
    Ok(store_at(hearth)
        .load_item(id)
        .map_err(|e| e.to_string())?
        .history
        .iter()
        .map(|h| h.kind.as_str().to_string())
        .collect())
}

fn item_state(hearth: &std::path::Path, id: &str) -> Result<String, String> {
    Ok(store_at(hearth)
        .load_item(id)
        .map_err(|e| e.to_string())?
        .item
        .state
        .as_str()
        .to_string())
}

/// Rewrite one item's bytes to CORRUPT them. Used only where the thing under
/// test is the strict reader's refusal, never to stand in for an operation.
fn corrupt_item(
    hearth: &std::path::Path,
    id: &str,
    mutate_item: impl FnOnce(&mut BacklogItem),
) -> Result<(), String> {
    let mut item = store_at(hearth)
        .load_item(id)
        .map_err(|e| e.to_string())?
        .item;
    mutate_item(&mut item);
    write_item(hearth, id, &item)
}

/// Build one lifecycle provenance with real operations. `Ok(None)` when the
/// name is not a lifecycle provenance.
fn seed_lifecycle_provenance(
    hearth: &std::path::Path,
    state: &str,
    prov: &str,
) -> Result<Option<Provenance>, String> {
    let target = "bi_target0001";
    let mut p = Provenance {
        target: target.to_string(),
        organ: ORGAN.to_string(),
        approver: Some("Nick".to_string()),
        ..Default::default()
    };

    // The printed rows, seeded from their real §5 source provenance.
    if let Some(row) = prov.strip_prefix("row_") {
        match row {
            "1" => {
                ranked_candidate(hearth, target, ORGAN, 9.0, DependencyStatus::Ready)?;
            }
            "2" => {
                mint_bare(hearth, target, ORGAN, "candidate to park")?;
                shape(hearth, target, shape_effort_and_playbook(), DriverRole::OrganLoop)?;
                shape(hearth, target, shape_wake(), DriverRole::OrganLoop)?;
            }
            "3" => {
                ranked_candidate(hearth, target, ORGAN, 9.0, DependencyStatus::Ready)?;
                shape(hearth, target, shape_supersede(), DriverRole::NickShape)?;
            }
            // Evaluation-origin rows. Their aged-out reason is a state-entry
            // input only `evaluate_backlog` may supply, and their wake is one of
            // the two locally executable grammars, so they are seeded by the
            // evaluation seeder below. Role and origin authority are both
            // checked before any guard, which is what the @snapshot refusal
            // scenarios prove against the very same real sources.
            "4" | "9" | "13" | "14" | "16" | "10" => {
                return seed_evaluation_provenance(hearth, prov);
            }
            "5" => {
                ready_item(hearth, target, ORGAN, 9.0)?;
                stamp(hearth, target)?;
            }
            "6" => {
                ready_item(hearth, target, ORGAN, 9.0)?;
            }
            "7" => {
                ready_item(hearth, target, ORGAN, 9.0)?;
                shape(hearth, target, shape_wake(), DriverRole::OrganLoop)?;
            }
            "8" => {
                ready_item(hearth, target, ORGAN, 9.0)?;
                shape(hearth, target, shape_supersede(), DriverRole::NickShape)?;
            }
            "11" => {
                in_flight_item(hearth, target, ORGAN)?;
                shape(hearth, target, shape_wake(), DriverRole::TrackDriver)?;
            }
            "12" => {
                in_flight_item(hearth, target, ORGAN)?;
                shape(hearth, target, shape_supersede(), DriverRole::NickShape)?;
            }
            "15" => {
                parked_from_7(hearth, target, ORGAN)?;
                shape(hearth, target, shape_supersede(), DriverRole::NickShape)?;
            }
            other => return Err(format!("unknown printed row provenance `row_{other}`")),
        }
        return Ok(Some(p));
    }

    match prov {
        // ── off-table / terminal sources ────────────────────────────────────
        "off_table" => match state {
            "candidate" => {
                ranked_candidate(hearth, target, ORGAN, 9.0, DependencyStatus::Ready)?;
            }
            "ready" => {
                ready_item(hearth, target, ORGAN, 9.0)?;
            }
            "in_flight" => {
                in_flight_item(hearth, target, ORGAN)?;
            }
            "parked" => {
                parked_from_7(hearth, target, ORGAN)?;
            }
            other => return Err(format!("no off-table source for state `{other}`")),
        },
        "terminal" => match state {
            "done" => {
                in_flight_item(hearth, target, ORGAN)?;
                signoff(hearth, target)?;
                govern(hearth, target, State::InFlight, State::Done, DriverRole::NickShape, None)?;
            }
            "superseded" => {
                ready_item(hearth, target, ORGAN, 9.0)?;
                shape(hearth, target, shape_supersede(), DriverRole::NickShape)?;
                govern(
                    hearth,
                    target,
                    State::Ready,
                    State::Superseded,
                    DriverRole::NickShape,
                    None,
                )?;
            }
            "aged_out" => {
                // The ONLY writer of an aged-out item is evaluation.
                publish_shaped_aged(hearth, target, ORGAN, 3)?;
                run_evaluation(hearth)?;
            }
            other => return Err(format!("state `{other}` is not terminal")),
        },

        // ── unmet-guard sources ─────────────────────────────────────────────
        "unranked" => {
            mint_bare(hearth, target, ORGAN, "unranked candidate")?;
            shape(hearth, target, shape_effort_and_playbook(), DriverRole::OrganLoop)?;
        }
        "no_attend_approval" => {
            ready_item(hearth, target, ORGAN, 9.0)?;
            stamp(hearth, target)?;
            p.approver = None;
        }
        "stale_binding_stamp" => {
            // Stamped while ready, then parked and woken WITHOUT a fresh stamp:
            // the retained binding is provenance, never authorization.
            ready_item(hearth, target, ORGAN, 9.0)?;
            stamp(hearth, target)?;
            shape(hearth, target, shape_wake(), DriverRole::OrganLoop)?;
            govern(hearth, target, State::Ready, State::Parked, DriverRole::OrganLoop, None)?;
            govern(hearth, target, State::Parked, State::Ready, DriverRole::NickShape, None)?;
        }
        "no_superseded_by" => {
            ranked_candidate(hearth, target, ORGAN, 9.0, DependencyStatus::Ready)?;
        }
        "unusable_actors_status" => {
            // A ranked candidate whose live `status.yaml` is PARSEABLE YAML but
            // whose `actors` key cannot hold an actor block. The strict loader
            // accepts it (it is YAML), so only the K8 write path's
            // `MalformedPolicy::Strict` stands between this document and an
            // add-if-absent leg that would rewrite the `actors:` line and
            // silently discard what it held.
            ranked_candidate(hearth, target, ORGAN, 9.0, DependencyStatus::Ready)?;
            std::fs::write(item_dir(hearth, target).join("status.yaml"), "actors: 7\n")
                .map_err(|e| format!("plant unusable status.yaml: {e}"))?;
        }
        "no_wake_condition" => {
            mint_bare(hearth, target, ORGAN, "candidate without wake")?;
            shape(hearth, target, shape_effort_and_playbook(), DriverRole::OrganLoop)?;
        }
        "missing_binding" => {
            // An in_flight item whose pickup bindings were stripped: the #11/#12
            // carry guard, not the state-local validator, must refuse.
            in_flight_item(hearth, target, ORGAN)?;
            shape(hearth, target, shape_wake(), DriverRole::TrackDriver)?;
            corrupt_item(hearth, target, |item| {
                item.execution_binding = None;
                item.outcome_binding = None;
            })?;
        }

        // ── done-rule sources ───────────────────────────────────────────────
        "bound" | "registered_only" => {
            in_flight_item(hearth, target, ORGAN)?;
        }
        "unsigned_unmeasurable" => {
            in_flight_item(hearth, target, ORGAN)?;
            corrupt_item(hearth, target, |item| {
                if let Some(ob) = item.outcome_binding.as_mut() {
                    ob.reading_status = ReadingStatus::UnmeasurableSigned;
                    ob.nick_signoff = false;
                }
            })?;
        }
        "track_completion_note" => {
            in_flight_item(hearth, target, ORGAN)?;
        }

        // ── #5 stamp-freshness provenances ──────────────────────────────────
        "approved_top_ready_stamped" => {
            ready_item(hearth, target, ORGAN, 9.0)?;
            stamp(hearth, target)?;
        }
        "restamped_after_consumed" => {
            in_flight_item(hearth, target, ORGAN)?;
            shape(hearth, target, shape_wake(), DriverRole::TrackDriver)?;
            govern(hearth, target, State::InFlight, State::Parked, DriverRole::TrackDriver, None)?;
            govern(hearth, target, State::Parked, State::Ready, DriverRole::NickShape, None)?;
            stamp(hearth, target)?;
        }
        "parked_from_11_woken_14_no_restamp" => {
            parked_from_11(hearth, target, ORGAN)?;
            govern(hearth, target, State::Parked, State::Ready, DriverRole::NickShape, None)?;
        }
        "parked_from_11_woken_14_restamped" => {
            parked_from_11(hearth, target, ORGAN)?;
            govern(hearth, target, State::Parked, State::Ready, DriverRole::NickShape, None)?;
            stamp(hearth, target)?;
        }
        "parked_from_11_woken_13_then_1_no_restamp" => {
            parked_from_11(hearth, target, ORGAN)?;
            govern(hearth, target, State::Parked, State::Candidate, DriverRole::NickShape, None)?;
            govern(hearth, target, State::Candidate, State::Ready, DriverRole::OrganLoop, None)?;
        }
        "parked_from_11_woken_13_then_1_restamped" => {
            parked_from_11(hearth, target, ORGAN)?;
            govern(hearth, target, State::Parked, State::Candidate, DriverRole::NickShape, None)?;
            govern(hearth, target, State::Candidate, State::Ready, DriverRole::OrganLoop, None)?;
            stamp(hearth, target)?;
        }

        // ── binding-carry provenances ───────────────────────────────────────
        "bound_with_wake_intent" => {
            in_flight_item(hearth, target, ORGAN)?;
            shape(hearth, target, shape_wake(), DriverRole::TrackDriver)?;
        }
        "bound_with_supersede_intent" => {
            in_flight_item(hearth, target, ORGAN)?;
            shape(hearth, target, shape_supersede(), DriverRole::NickShape)?;
        }
        "parked_from_7_ranked" => {
            parked_from_7(hearth, target, ORGAN)?;
            shape(hearth, target, shape_supersede(), DriverRole::NickShape)?;
        }
        "parked_from_11_ranked" => {
            parked_from_11(hearth, target, ORGAN)?;
            shape(hearth, target, shape_supersede(), DriverRole::NickShape)?;
        }
        "parked_from_2_rankless" => {
            parked_from_2_rankless(hearth, target, ORGAN)?;
            shape(hearth, target, shape_supersede(), DriverRole::NickShape)?;
        }

        // ── rank materialization / queue provenances ────────────────────────
        "rankless" => {
            mint_bare(hearth, target, ORGAN, "rankless candidate")?;
        }
        "all_inputs_and_effort" => {
            mint_bare(hearth, target, ORGAN, "two-edit candidate")?;
            shape(hearth, target, shape_effort_and_playbook(), DriverRole::OrganLoop)?;
            shape(
                hearth,
                target,
                shape_rank_inputs(4.0, 1.0, DependencyStatus::Ready),
                DriverRole::OrganLoop,
            )?;
        }
        "pre_triage_plus_survivor" => {
            mint_bare(hearth, target, ORGAN, "pre-triage candidate")?;
            shaped_candidate(hearth, "bi_zbbb000001", ORGAN, 4.0, DependencyStatus::Ready)?;
        }
        // ── named-producer provenances (one entry per public operation) ─────
        "shapeable" => {
            mint_bare(hearth, target, ORGAN, "shapeable candidate")?;
        }
        "all_inputs_effort" => {
            shaped_candidate(hearth, target, ORGAN, 4.0, DependencyStatus::Ready)?;
        }
        "ready_shaped" => {
            shaped_ready(hearth, target, ORGAN)?;
        }
        "incomplete_committed_rank" => {
            ranked_candidate(hearth, target, ORGAN, 4.0, DependencyStatus::Ready)?;
            corrupt_item(hearth, target, |item| {
                if let Some(rank) = item.rank.as_mut() {
                    rank.explanation = String::new();
                }
            })?;
        }
        "effort_mirror_mismatch" => {
            ranked_candidate(hearth, target, ORGAN, 4.0, DependencyStatus::Ready)?;
            corrupt_item(hearth, target, |item| {
                item.effort_class = Some(EffortClass::L);
            })?;
        }
        "orchestrator_position" => {
            // A ledger that already RECORDS the orchestrator as a per-organ
            // position writer. Only a corruption can produce it — the mutation
            // service never mints one — and the re-rank must refuse to build on
            // that forged authority (§1.10).
            ranked_candidate(hearth, target, ORGAN, 4.0, DependencyStatus::Ready)?;
            let mut history = store_at(hearth)
                .load_item(target)
                .map_err(|e| e.to_string())?
                .history;
            if let Some(entry) = history
                .iter_mut()
                .find(|e| e.kind == HistoryKind::RankRecomputed)
            {
                entry.role = DriverRole::Orchestrator;
            }
            write_history(hearth, target, &history)?;
        }
        "ranked_organ" | "open_proposal" | "resolved_proposal" | "stale_proposal"
        | "commit_crashed_after_authorization" => {
            // Two equally weighted candidates shaped first and re-ranked ONCE,
            // so each carries exactly `created, shape_edited, rank_recomputed`
            // and an advancement (which resets the target's age) genuinely
            // REORDERS the organ.
            shaped_candidate(hearth, target, ORGAN, 4.0, DependencyStatus::Ready)?;
            shaped_candidate(hearth, "bi_zbbb000001", ORGAN, 4.0, DependencyStatus::Ready)?;
            recompute(hearth, ORGAN)?;
            p.proposal_targets = vec![target.to_string(), "bi_zbbb000001".to_string()];
            if prov != "ranked_organ" {
                let proposal = propose(hearth, &p.proposal_targets)?;
                p.proposal_id = Some(proposal.clone());
                match prov {
                    "resolved_proposal" => {
                        resolve_proposal(hearth, &proposal, true, None)?;
                    }
                    "stale_proposal" => {
                        // A NEW ranked member joins the organ after the
                        // proposal, so its authoritative context moved.
                        ranked_candidate(hearth, "bi_zccc000001", ORGAN, 4.0, DependencyStatus::Ready)?;
                    }
                    "commit_crashed_after_authorization" => {
                        crash_decide_commit(hearth, &proposal, &p.proposal_targets)?;
                    }
                    _ => {}
                }
            }
        }
        "unresolved_rank_edit" => {
            ranked_candidate(hearth, target, ORGAN, 4.0, DependencyStatus::Ready)?;
            // A rank-affecting edit that no re-rank has consumed yet.
            shape(
                hearth,
                target,
                shape_rank_inputs(7.0, 2.0, DependencyStatus::Ready),
                DriverRole::OrganLoop,
            )?;
        }
        "malformed_pending_payload" => {
            ranked_candidate(hearth, target, ORGAN, 4.0, DependencyStatus::Ready)?;
            // A REAL rank-affecting shape_edit, then its staged `pending`
            // payload is corrupted on disk to a type-wrong value. The strict
            // history load still accepts it (payload is an untyped value), so
            // this is exactly the shape that used to decay to "nothing
            // unresolved" and let a rank-sensitive edge advance on stale rank.
            shape(
                hearth,
                target,
                shape_rank_inputs(7.0, 2.0, DependencyStatus::Ready),
                DriverRole::OrganLoop,
            )?;
            corrupt_pending_payload(hearth, target)?;
        }
        "cross_organ_proposal" => {
            ranked_candidate(hearth, target, ORGAN, 4.0, DependencyStatus::Ready)?;
            ranked_candidate(hearth, "bi_zddd000001", ORGAN2, 4.0, DependencyStatus::Ready)?;
            p.proposal_targets = vec![target.to_string(), "bi_zddd000001".to_string()];
        }
        "two_organ_ranked_and_pre_triage" => {
            ranked_candidate(hearth, target, ORGAN, 4.0, DependencyStatus::Ready)?;
            ranked_candidate(hearth, "bi_zddd000001", ORGAN2, 4.0, DependencyStatus::Ready)?;
            mint_bare(hearth, "bi_zeee000001", ORGAN, "pre-triage candidate")?;
        }
        _ => return seed_evaluation_provenance(hearth, prov),
    }
    Ok(Some(p))
}

/// Rewrite the LAST `shape_edit`'s `pending` payload to a type-wrong value.
///
/// `HistoryEntry.payload` is an untyped `serde_yaml::Value`, so this survives
/// the strict history load and only fails when `PendingRankIntent` is decoded —
/// which is precisely the seam that must refuse rather than silently report
/// "no unresolved edits".
fn corrupt_pending_payload(hearth: &std::path::Path, id: &str) -> Result<(), String> {
    let path = item_dir(hearth, id).join("history.yaml");
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("read history: {e}"))?;
    let mut entries: Vec<HistoryEntry> =
        serde_yaml::from_str(&raw).map_err(|e| format!("parse history: {e}"))?;
    let entry = entries
        .iter_mut()
        .filter(|e| e.kind == HistoryKind::ShapeEdit)
        .next_back()
        .ok_or("no shape_edit entry to corrupt")?;
    let payload = entry
        .payload
        .as_mut()
        .ok_or("shape_edit entry carries no payload")?;
    let map = payload
        .as_mapping_mut()
        .ok_or("shape_edit payload is not a mapping")?;
    let mut bad = serde_yaml::Mapping::new();
    bad.insert(
        serde_yaml::Value::String("nick_weight".to_string()),
        serde_yaml::Value::String("high".to_string()),
    );
    map.insert(
        serde_yaml::Value::String("pending".to_string()),
        serde_yaml::Value::Mapping(bad),
    );
    write_history(hearth, id, &entries)
}

/// Propose a reshuffle that reverses the named items' positions.
fn propose(hearth: &std::path::Path, targets: &[String]) -> Result<String, String> {
    let proposed: Vec<ProposedPosition> = targets
        .iter()
        .enumerate()
        .map(|(idx, id)| ProposedPosition {
            backlog_item_id: id.clone(),
            position: (targets.len() - idx) as u32,
        })
        .collect();
    let outcome = mutate(
        hearth,
        BacklogMutation::ProposeReshuffle { proposed },
        DriverRole::Orchestrator,
        "Orchestrator-000001",
    )?;
    outcome
        .proposal_id
        .ok_or_else(|| "propose_reshuffle returned no proposal id".to_string())
}

fn resolve_proposal(
    hearth: &std::path::Path,
    proposal_id: &str,
    commit: bool,
    role: Option<DriverRole>,
) -> Result<(), String> {
    let mutation = if commit {
        BacklogMutation::CommitReshuffle {
            proposal_id: proposal_id.to_string(),
            approver: "Nick".to_string(),
        }
    } else {
        BacklogMutation::RejectReshuffle {
            proposal_id: proposal_id.to_string(),
            approver: "Nick".to_string(),
        }
    };
    mutate(
        hearth,
        mutation,
        role.unwrap_or(DriverRole::NickShape),
        "Nick-000001",
    )
    .map(|_| ())
}

/// Crash a DECIDE commit after every `rank_committed` authorization append but
/// BEFORE any approved position byte, then prove no position file preceded its
/// authorization.
fn crash_decide_commit(
    hearth: &std::path::Path,
    proposal_id: &str,
    targets: &[String],
) -> Result<(), String> {
    let before: Vec<Option<u32>> = targets
        .iter()
        .map(|id| {
            store_at(hearth)
                .load_item(id)
                .ok()
                .and_then(|l| l.item.rank.as_ref().map(|r| r.position))
        })
        .collect();
    let last_history_target = targets.iter().max().expect("a nonempty proposal");
    let seq = store_at(hearth)
        .load_item(last_history_target)
        .map_err(|e| e.to_string())?
        .next_seq();
    std::env::set_var("ANVIL_TEST_MODE", "1");
    std::env::set_var(
        "ANVIL_TEST_BACKLOG_CRASH_AFTER",
        format!("after_history:{last_history_target}:{seq}"),
    );
    let outcome = resolve_proposal(hearth, proposal_id, true, None);
    std::env::remove_var("ANVIL_TEST_BACKLOG_CRASH_AFTER");
    std::env::remove_var("ANVIL_TEST_MODE");
    match outcome {
        Err(reason) if reason.contains("test crash point") => {}
        Err(other) => return Err(format!("expected the DECIDE crash point to fire: {other}")),
        Ok(()) => return Err("expected the DECIDE crash point to fire, but it committed".into()),
    }
    // Instrumentation: not one position byte may have landed yet.
    for (id, prior) in targets.iter().zip(before) {
        let now = raw_item_position(hearth, id)?;
        if now != prior {
            return Err(format!(
                "position for `{id}` moved to {now:?} before its authorization entry \
                 (was {prior:?})"
            ));
        }
    }
    Ok(())
}

/// Read one item's committed position straight off disk, WITHOUT running
/// recovery — recovery is exactly what this instrumentation must observe the
/// absence of.
fn raw_item_position(hearth: &std::path::Path, id: &str) -> Result<Option<u32>, String> {
    let raw = std::fs::read_to_string(item_dir(hearth, id).join("item.yaml"))
        .map_err(|e| format!("read item.yaml for {id}: {e}"))?;
    let item: BacklogItem =
        serde_yaml::from_str(&raw).map_err(|e| format!("decode item.yaml for {id}: {e}"))?;
    Ok(item.rank.as_ref().map(|r| r.position))
}

/// Run one named mutation as the scenario's `When`.
fn attempt_named_mutation(
    hearth: &std::path::Path,
    prov: &Provenance,
    op: &str,
    role: DriverRole,
) -> Result<(Option<String>, Vec<String>, Vec<String>, Option<String>), String> {
    let mutation = match op {
        "shape_edit" => BacklogMutation::ShapeEdit {
            bi_id: prov.target.clone(),
            body: shape_rank_inputs(4.0, 1.0, DependencyStatus::Ready),
        },
        "recompute_rank" => BacklogMutation::RecomputeRank {
            business_node_id: prov.organ.clone(),
        },
        "stamp_execution_binding" => BacklogMutation::StampExecutionBinding {
            bi_id: prov.target.clone(),
            execution_binding: exec_binding(),
            outcome_binding: OutcomeBindingDecl {
                success_measure_id: Some("sm_seed01".to_string()),
                tree_node: "tree/seed".to_string(),
                reading_status: ReadingStatus::Registered,
            },
        },
        "record_outcome_signoff" => BacklogMutation::RecordOutcomeSignoff {
            bi_id: prov.target.clone(),
            approver: "Nick".to_string(),
        },
        "veto_age_out" => BacklogMutation::VetoAgeOut {
            bi_id: prov.target.clone(),
            approver: "Nick".to_string(),
        },
        "lift_age_out_veto" => BacklogMutation::LiftAgeOutVeto {
            bi_id: prov.target.clone(),
            approver: "Nick".to_string(),
        },
        "propose_reshuffle" => {
            let targets = if prov.proposal_targets.is_empty() {
                vec![prov.target.clone()]
            } else {
                prov.proposal_targets.clone()
            };
            BacklogMutation::ProposeReshuffle {
                proposed: targets
                    .iter()
                    .enumerate()
                    .map(|(idx, id)| ProposedPosition {
                        backlog_item_id: id.clone(),
                        position: (targets.len() - idx) as u32,
                    })
                    .collect(),
            }
        }
        "commit_reshuffle" => BacklogMutation::CommitReshuffle {
            proposal_id: prov
                .proposal_id
                .clone()
                .unwrap_or_else(|| "rp_absent00000000000000000".to_string()),
            approver: "Nick".to_string(),
        },
        "reject_reshuffle" => BacklogMutation::RejectReshuffle {
            proposal_id: prov
                .proposal_id
                .clone()
                .unwrap_or_else(|| "rp_absent00000000000000000".to_string()),
            approver: "Nick".to_string(),
        },
        other => return Err(format!("unknown backlog operation `{other}`")),
    };
    let outcome = mutate(hearth, mutation, role, "Caller-000001")?;
    let state = item_state(hearth, &prov.target).ok();
    let history = history_kinds(hearth, &prov.target)?;
    Ok((state, history, outcome.unranked, outcome.proposal_id))
}

// ── Evaluation provenances (plan Task 8) ────────────────────────────────────

use anvil_core::domain::backlog_item::evaluate_backlog;

/// Run one REAL evaluation over every organ. The private `Evaluation` origin is
/// constructed inside the service; no fixture can forge it.
fn run_evaluation(hearth: &std::path::Path) -> Result<Vec<String>, String> {
    evaluate_backlog(
        &store_at(hearth),
        None,
        &BacklogPolicy::default(),
        &fixture_actor("Evaluator-000001"),
        &tick(),
        &format!("evaluate-{}", short_random_id()),
        &hi_res_prefix(),
        &short_random_id(),
    )
    .map(|o| o.unranked)
    .map_err(|e| e.to_string())
}

/// A terminal sentinel a locally executable wake predicate can reference.
fn sentinel(hearth: &std::path::Path, id: &str, terminal: State) -> Result<String, String> {
    if store_at(hearth).load_item(id).is_ok() {
        return Ok(id.to_string());
    }
    match terminal {
        State::Done => {
            in_flight_item(hearth, id, ORGAN2)?;
            signoff(hearth, id)?;
            govern(hearth, id, State::InFlight, State::Done, DriverRole::NickShape, None)?;
        }
        State::Superseded => {
            shaped_ready(hearth, id, ORGAN2)?;
            shape(hearth, id, shape_supersede(), DriverRole::NickShape)?;
            govern(hearth, id, State::Ready, State::Superseded, DriverRole::NickShape, None)?;
        }
        other => return Err(format!("`{}` is not a sentinel terminal", other.as_str())),
    }
    Ok(id.to_string())
}

/// A wake condition naming one of the two locally executable grammars.
fn wake_on(kind: WakeKind, reference: Option<&str>, predicate: &str) -> ShapeEditBody {
    ShapeEditBody {
        wake_condition: Some(FieldEdit::Set(WakeCondition {
            kind,
            r#ref: reference.map(|id| evid(EvidenceKind::BacklogItem, id)),
            predicate: predicate.to_string(),
        })),
        ..Default::default()
    }
}

/// A satisfied `item_state` wake: the referenced sentinel is already done.
fn wake_met(hearth: &std::path::Path) -> Result<ShapeEditBody, String> {
    let id = sentinel(hearth, "bi_wake000001", State::Done)?;
    Ok(wake_on(WakeKind::ItemState, Some(&id), "state==done"))
}

/// A provably unreachable `item_state` wake: the referenced sentinel is
/// terminal in a DIFFERENT state, so the equality can never hold again.
fn wake_unreachable(hearth: &std::path::Path) -> Result<ShapeEditBody, String> {
    let id = sentinel(hearth, "bi_wake000002", State::Superseded)?;
    Ok(wake_on(WakeKind::ItemState, Some(&id), "state==done"))
}

/// Advance an item's re-rank age by running the organ re-rank `n` times.
fn age_by(hearth: &std::path::Path, organ: &str, n: u32) -> Result<(), String> {
    for _ in 0..n {
        recompute(hearth, organ)?;
    }
    Ok(())
}

/// A `#7`-origin parked item whose wake is `body`.
fn parked_from_7_with(
    hearth: &std::path::Path,
    id: &str,
    organ: &str,
    body: ShapeEditBody,
) -> Result<String, String> {
    ready_item(hearth, id, organ, 9.0)?;
    shape(hearth, id, body, DriverRole::OrganLoop)?;
    govern(hearth, id, State::Ready, State::Parked, DriverRole::OrganLoop, None)?;
    Ok(id.to_string())
}

/// A `#11`-origin parked item whose wake is `body`.
fn parked_from_11_with(
    hearth: &std::path::Path,
    id: &str,
    organ: &str,
    body: ShapeEditBody,
) -> Result<String, String> {
    in_flight_item(hearth, id, organ)?;
    shape(hearth, id, body, DriverRole::TrackDriver)?;
    govern(hearth, id, State::InFlight, State::Parked, DriverRole::TrackDriver, None)?;
    Ok(id.to_string())
}

/// A `#2`-origin rankless parked item whose wake is `body`.
fn parked_from_2_with(
    hearth: &std::path::Path,
    id: &str,
    organ: &str,
    body: ShapeEditBody,
) -> Result<String, String> {
    mint_bare(hearth, id, organ, "rankless parked")?;
    shape(hearth, id, shape_effort_and_playbook(), DriverRole::OrganLoop)?;
    shape(hearth, id, body, DriverRole::OrganLoop)?;
    govern(hearth, id, State::Candidate, State::Parked, DriverRole::OrganLoop, None)?;
    Ok(id.to_string())
}

/// The ONE explicitly labeled out-of-scope producer fixture (§3): Temper's
/// `registered -> reading` transition is not built by this track, so the
/// scenario that proves the engine-auto done path seeds the stored reading
/// directly and never performs the done transition itself.
pub fn seed_external_temper_reading(hearth: &std::path::Path, id: &str) -> Result<(), String> {
    corrupt_item(hearth, id, |item| {
        if let Some(ob) = item.outcome_binding.as_mut() {
            ob.reading_status = ReadingStatus::Reading;
        }
    })
}

/// Build one evaluation-facing provenance. `Ok(None)` when the name is not one.
fn seed_evaluation_provenance(
    hearth: &std::path::Path,
    prov: &str,
) -> Result<Option<Provenance>, String> {
    let target = "bi_target0001";
    let p = Provenance {
        target: target.to_string(),
        organ: ORGAN.to_string(),
        approver: Some("Nick".to_string()),
        ..Default::default()
    };
    // The default budget is 3, and every re-rank advances a survivor's age by
    // one, so an item is "over budget" once the evaluation's OWN increment
    // takes it strictly past 3.
    match prov {
        // The default budget is 3, so an item already carrying age 3 ages out on
        // the very next evaluation, whose OWN increment takes it to 4 > 3.
        "shaped_over_budget" | "row_4" => {
            publish_shaped_aged(hearth, target, ORGAN, 3)?;
            shape(
                hearth,
                target,
                shape_rank_inputs(4.0, 1.0, DependencyStatus::Ready),
                DriverRole::OrganLoop,
            )?;
        }
        "shaped_at_budget" => {
            publish_shaped_aged(hearth, target, ORGAN, 2)?;
            shape(
                hearth,
                target,
                shape_rank_inputs(4.0, 1.0, DependencyStatus::Ready),
                DriverRole::OrganLoop,
            )?;
        }
        "ready_over_budget" | "row_9" => {
            ready_item(hearth, target, ORGAN, 9.0)?;
            age_by(hearth, ORGAN, 3)?;
        }
        "pre_triage_over_budget" => {
            mint_bare(hearth, target, ORGAN, "pre-triage candidate")?;
        }
        "parked_from_2_wake_met" | "row_13" => {
            let body = wake_met(hearth)?;
            parked_from_2_with(hearth, target, ORGAN, body)?;
        }
        "parked_from_7_wake_met" | "row_14" => {
            let body = wake_met(hearth)?;
            parked_from_7_with(hearth, target, ORGAN, body)?;
        }
        "parked_from_7_wake_unreachable" | "row_16" => {
            let body = wake_unreachable(hearth)?;
            parked_from_7_with(hearth, target, ORGAN, body)?;
        }
        "parked_from_11_wake_unreachable" => {
            let body = wake_unreachable(hearth)?;
            parked_from_11_with(hearth, target, ORGAN, body)?;
        }
        "parked_from_2_wake_unreachable" => {
            let body = wake_unreachable(hearth)?;
            parked_from_2_with(hearth, target, ORGAN, body)?;
        }
        "parked_from_7_manual_wake" => {
            parked_from_7_with(
                hearth,
                target,
                ORGAN,
                wake_on(WakeKind::Manual, None, "nick wakes it"),
            )?;
        }
        "parked_from_7_external_wake" => {
            let sentinel = sentinel(hearth, "bi_wake000001", State::Done)?;
            parked_from_7_with(
                hearth,
                target,
                ORGAN,
                wake_on(WakeKind::ExternalEvent, Some(&sentinel), "an external signal"),
            )?;
        }
        "row_10" => {
            in_flight_item(hearth, target, ORGAN)?;
            seed_external_temper_reading(hearth, target)?;
        }
        other => return Ok(None),
    }
    Ok(Some(p))
}
