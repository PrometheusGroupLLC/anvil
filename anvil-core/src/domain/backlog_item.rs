//! K8 backlog-item (`backlog_item`) domain value model and pure validators.
//!
//! This module realizes the frozen K8 v1.0 D1/D1.2 typed field schema and the
//! pure validators of the anvil realization spec (R4–R28):
//!
//! - the `#[serde(deny_unknown_fields)]` value types (§4.2),
//! - `validate_required_by_state` (R7/R8 — D1.3),
//! - `validate_transition` (R10/R11/R12 — the D2.1 17-row table, expanded to the
//!   29 printed `(from, to, driver_role)` tuples plus the item-local guards),
//! - `done_rule_satisfied` (R22 — D6.3),
//! - `normalize_on_state_entry` (§1.8 — history-silent exit construction / age reset),
//! - the id grammar validators + `bi_` minter (R6),
//! - `BacklogPolicy` + the strict comparator/age-budget parsers (R15/R25),
//! - the pure per-organ rank materializer (`materialize_rank`, R15/R16/R17).
//!
//! It is deliberately free of storage, engine, or wire concerns: those live in
//! the ports/adapters and are wired by later plan tasks. Every function here is a
//! pure function over owned values so it can be exercised by real seams without a
//! mock. `state` is never a second write-authority here — it is validated as a
//! derived mirror of the authoritative ledger (R4).

use crate::domain::shared_types::ActorIdentity;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

// ---------------------------------------------------------------------------
// Closed enums (serde rejects unknown variants at parse time — fail-loud, R5).
// ---------------------------------------------------------------------------

/// `action_class` — the kind of work the item represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionClass {
    Dev,
    Content,
    Consulting,
    Subtraction,
    Incident,
    Experiment,
    Ops,
}

/// `effort_class` — the T-shirt size of the item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffortClass {
    // Ordinal order is smallest-effort-first so `effort_asc` is a plain `Ord`.
    Xs,
    S,
    M,
    L,
    Xl,
}

/// `state` — the lifecycle state (a mirror of the authoritative ledger, R4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Candidate,
    Ready,
    InFlight,
    Done,
    Parked,
    Superseded,
    AgedOut,
}

impl State {
    /// Terminal states have no outgoing edges (R12).
    pub fn is_terminal(self) -> bool {
        matches!(self, State::Done | State::Superseded | State::AgedOut)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            State::Candidate => "candidate",
            State::Ready => "ready",
            State::InFlight => "in_flight",
            State::Done => "done",
            State::Parked => "parked",
            State::Superseded => "superseded",
            State::AgedOut => "aged_out",
        }
    }
}

/// Intake `edge` — the five (D3) genesis entrances.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntakeEdge {
    SparkTriage,
    OrchestratorValueGap,
    ObligationRefresh,
    SentinelIncident,
    NickShape,
}

/// `evidence_ref.kind` — the cross-system join primitive kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Spark,
    LoreNode,
    Decision,
    Council,
    Experiment,
    TemperMeasure,
    TemperHotspot,
    Note,
    BacklogItem,
    Artifact,
}

/// `reading_status` — the outcome-binding reading state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadingStatus {
    Registered,
    Reading,
    UnmeasurableSigned,
}

/// `exit.kind` — always equals the `state` it hangs under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitKind {
    Done,
    Parked,
    Superseded,
    AgedOut,
}

/// `aged_out_reason` — why an item aged out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgedOutReason {
    StaleNoReady,
    StaleNoPickup,
    WakeUnreachable,
}

/// `wake_condition.kind` — event/state-based only, never time-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeKind {
    MeasureThreshold,
    ItemState,
    DependencyReady,
    ExternalEvent,
    Manual,
}

/// `history_entry.kind` — the ten append-only history producers (R13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryKind {
    Created,
    StateChange,
    RankRecomputed,
    RankProposed,
    RankCommitted,
    RankRejected,
    ShapeEdit,
    BindingStamped,
    Signoff,
    Veto,
}

impl HistoryKind {
    /// The exact serialized token. Single-sourced so a scenario naming a
    /// history kind and the persisted bytes can never drift.
    pub fn as_str(self) -> &'static str {
        match self {
            HistoryKind::Created => "created",
            HistoryKind::StateChange => "state_change",
            HistoryKind::RankRecomputed => "rank_recomputed",
            HistoryKind::RankProposed => "rank_proposed",
            HistoryKind::RankCommitted => "rank_committed",
            HistoryKind::RankRejected => "rank_rejected",
            HistoryKind::ShapeEdit => "shape_edited",
            HistoryKind::BindingStamped => "binding_stamped",
            HistoryKind::Signoff => "signoff",
            HistoryKind::Veto => "veto",
        }
    }
}

/// `history_entry.role` / driver role — the D2.1 driver taxonomy (R9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverRole {
    TrackDriver,
    Orchestrator,
    OrganLoop,
    Intake,
    EngineAuto,
    NickShape,
}

impl DriverRole {
    /// The frozen wire name — identical to the serde name and to the role
    /// string the compiled seed's `required_role` rows carry.
    pub fn as_str(self) -> &'static str {
        match self {
            DriverRole::TrackDriver => "track_driver",
            DriverRole::Orchestrator => "orchestrator",
            DriverRole::OrganLoop => "organ_loop",
            DriverRole::Intake => "intake",
            DriverRole::EngineAuto => "engine_auto",
            DriverRole::NickShape => "nick_shape",
        }
    }

    /// Decode one closed driver-role token. This admits every role INCLUDING
    /// the two engine authorities: which rows each may drive is the transition
    /// table's decision, not the transport's, so `engine_auto` reaches the
    /// governed guard that refuses it for the evaluation-only rows and admits
    /// it for row #10 behind a stored reading.
    pub fn from_token(token: &str) -> Result<Self, BacklogItemError> {
        Ok(match token.trim() {
            "track_driver" => DriverRole::TrackDriver,
            "orchestrator" => DriverRole::Orchestrator,
            "organ_loop" => DriverRole::OrganLoop,
            "intake" => DriverRole::Intake,
            "engine_auto" => DriverRole::EngineAuto,
            "nick_shape" => DriverRole::NickShape,
            other => {
                return Err(BacklogItemError::SemanticViolation {
                    detail: format!("`{other}` is not one of the closed K8 driver-role tokens"),
                })
            }
        })
    }
}

/// Free-function alias for [`DriverRole::from_token`], mirroring
/// [`parse_state_token`] at the transport boundary.
pub fn parse_driver_role_token(token: &str) -> Result<DriverRole, BacklogItemError> {
    DriverRole::from_token(token)
}

/// `dependency_readiness.status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyStatus {
    Ready,
    Blocked,
}

// ---------------------------------------------------------------------------
// Value structs — every one carries `#[serde(deny_unknown_fields)]` (R4).
// Optional fields deserialize to `None` when absent; presence is enforced by
// `validate_required_by_state`, never by a reject-if-present rule (§1.2).
// ---------------------------------------------------------------------------

/// The cross-system join primitive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRef {
    pub kind: EvidenceKind,
    pub id: String,
}

/// `intake` — the genesis edge plus its authoritative evidence list (≥1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Intake {
    pub edge: IntakeEdge,
    pub evidence_refs: Vec<EvidenceRef>,
}

/// `value_gap_magnitude` — a Temper measure/hotspot reference plus a magnitude.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValueGapMagnitude {
    // JSON/YAML key is `ref`; serde uses the raw identifier verbatim.
    pub r#ref: EvidenceRef,
    pub magnitude: f64,
}

/// `dependency_readiness`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyReadiness {
    pub status: DependencyStatus,
    #[serde(default)]
    pub blocker_refs: Vec<EvidenceRef>,
}

/// `rank.inputs` — the five ordering inputs plus the mirrored effort class.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RankInputs {
    pub value_gap_magnitude: ValueGapMagnitude,
    pub nick_weight: f64,
    pub dependency_readiness: DependencyReadiness,
    /// Re-rank CYCLE count in `{candidate, ready}` — an event count, never wall-clock (R16).
    pub age: u32,
    /// Mirrored so the explanation is self-contained; must equal the top-level `effort_class`.
    pub effort_class: EffortClass,
}

/// `rank` — the deterministically materialized position plus its inputs/explanation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rank {
    pub position: u32,
    pub inputs: RankInputs,
    pub explanation: String,
}

/// `playbook_binding` — a null id implies `route_to_intake = true` (R19).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaybookBinding {
    pub playbook_definition_id: Option<String>,
    pub route_to_intake: bool,
}

/// `origin_binding` — the value gap served plus the optional predictor/prediction pair.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OriginBinding {
    pub value_gap_served: EvidenceRef,
    #[serde(default)]
    pub minting_council_id: Option<String>,
    #[serde(default)]
    pub experiment_id: Option<String>,
    #[serde(default)]
    pub predicted_value: Option<f64>,
}

/// `execution_binding` — stamped at pickup (#5); `run_id` is a K1a `lr_`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionBinding {
    pub track_id: String,
    pub playbook_definition_id: String,
    pub playbook_run_id: String,
    pub run_id: String,
}

/// `outcome_binding` — the declared success measure plus the reading state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutcomeBinding {
    #[serde(default)]
    pub success_measure_id: Option<String>,
    pub tree_node: String,
    pub reading_status: ReadingStatus,
    pub nick_signoff: bool,
}

/// The declared outcome binding supplied to `stamp_execution_binding` (§4.6).
///
/// `reading_status` here is restricted to `{registered, unmeasurable_signed}` by
/// the stamp op (§1); this type does not itself carry `nick_signoff`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutcomeBindingDecl {
    #[serde(default)]
    pub success_measure_id: Option<String>,
    pub tree_node: String,
    pub reading_status: ReadingStatus,
}

/// `wake_condition` — event/state-based; `ref` omitted only for `kind = manual`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WakeCondition {
    pub kind: WakeKind,
    #[serde(default)]
    pub r#ref: Option<EvidenceRef>,
    pub predicate: String,
}

/// `exit` — required at terminal/parked states; `kind` always equals `state`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exit {
    pub kind: ExitKind,
    #[serde(default)]
    pub wake_condition: Option<WakeCondition>,
    #[serde(default)]
    pub superseded_by: Option<String>,
    #[serde(default)]
    pub aged_out_reason: Option<AgedOutReason>,
}

/// A single append-only `history.yaml` entry (R13/R14). Included here as the
/// typed shape the storage layer appends; this module does not write history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryEntry {
    pub seq: u64,
    pub actor: String,
    pub role: DriverRole,
    pub at: String,
    pub kind: HistoryKind,
    #[serde(default)]
    pub from_state: Option<State>,
    #[serde(default)]
    pub to_state: Option<State>,
    #[serde(default)]
    pub payload: Option<serde_yaml::Value>,
    #[serde(default)]
    pub note: Option<String>,
}

/// The single closed genesis input decoded by `Begin(create_fields["item"])`
/// (§4.1 / plan Task 9). It is the ONLY caller-authored creation surface: every
/// engine-owned field (`backlog_item_id`, `state`, `rank`, `history`,
/// `execution_binding`, `outcome_binding`, `exit`) is absent from this closed
/// shape, so `#[serde(deny_unknown_fields)]` rejects any caller attempt to
/// supply one before a byte is written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BacklogGenesisInput {
    pub business_node_id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    pub action_class: ActionClass,
    #[serde(default)]
    pub effort_class: Option<EffortClass>,
    pub intake: Intake,
    #[serde(default)]
    pub playbook_binding: Option<PlaybookBinding>,
    pub origin_binding: OriginBinding,
}

/// The typed `item.yaml` value (§4.2). `history` is realized out-of-line per R13.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BacklogItem {
    pub backlog_item_id: String,
    pub business_node_id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    pub action_class: ActionClass,
    #[serde(default)]
    pub effort_class: Option<EffortClass>,
    pub state: State,
    pub intake: Intake,
    #[serde(default)]
    pub rank: Option<Rank>,
    #[serde(default)]
    pub playbook_binding: Option<PlaybookBinding>,
    pub origin_binding: OriginBinding,
    #[serde(default)]
    pub execution_binding: Option<ExecutionBinding>,
    #[serde(default)]
    pub outcome_binding: Option<OutcomeBinding>,
    #[serde(default)]
    pub exit: Option<Exit>,
}

// ---------------------------------------------------------------------------
// Errors — fail-loud, no silent fallback.
// ---------------------------------------------------------------------------

/// Every rejection path in this module is a typed, fail-loud error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BacklogItemError {
    /// An opaque id violates its pinned prefix/charset/minimum body length (R6/R7).
    InvalidId { field: String, reason: String },
    /// A field required at `state` is absent (R7/R8 — D1.3).
    MissingRequiredField { state: String, field: String },
    /// The `(from, to)` pair is not one of the 16 D2.1 non-genesis edges (R10).
    IllegalTransition { from: String, to: String },
    /// The `from` state is terminal and has no outgoing edges (R12).
    TerminalNoOutgoing { from: String },
    /// The declared `from` state does not match the item's current mirrored state.
    StateMismatch { declared: String, actual: String },
    /// The driver role is not admissible for this edge (R10/R11).
    WrongRole { edge: u8, role: String },
    /// An item-local edge guard is not satisfied (R11).
    GuardUnsatisfied { edge: u8, detail: String },
    /// The done-rule predicate (R22) does not hold for edge #10.
    DoneRuleUnsatisfied,
    /// A structural/semantic invariant is violated (finite numbers, mirrors, etc.).
    SemanticViolation { detail: String },
    /// A configured policy value (comparator / age budget) is malformed (R15/R25).
    InvalidPolicy { detail: String },
}

impl std::fmt::Display for BacklogItemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BacklogItemError::InvalidId { field, reason } => {
                write!(f, "invalid id for `{field}`: {reason}")
            }
            BacklogItemError::MissingRequiredField { state, field } => {
                write!(f, "field `{field}` is required at state `{state}` but is absent")
            }
            BacklogItemError::IllegalTransition { from, to } => {
                write!(f, "no such transition `{from}` -> `{to}` (not in the D2.1 table)")
            }
            BacklogItemError::TerminalNoOutgoing { from } => {
                write!(f, "state `{from}` is terminal and has no outgoing edges")
            }
            BacklogItemError::StateMismatch { declared, actual } => {
                write!(f, "declared from-state `{declared}` != item state `{actual}`")
            }
            BacklogItemError::WrongRole { edge, role } => {
                write!(f, "role `{role}` may not drive edge #{edge}")
            }
            BacklogItemError::GuardUnsatisfied { edge, detail } => {
                write!(f, "edge #{edge} guard unsatisfied: {detail}")
            }
            BacklogItemError::DoneRuleUnsatisfied => {
                write!(f, "done-rule (D6.3) not satisfied for edge #10")
            }
            BacklogItemError::SemanticViolation { detail } => {
                write!(f, "semantic violation: {detail}")
            }
            BacklogItemError::InvalidPolicy { detail } => {
                write!(f, "invalid backlog policy: {detail}")
            }
        }
    }
}

impl std::error::Error for BacklogItemError {}

// ---------------------------------------------------------------------------
// Id grammar (R6). `bi_` is an anvil-minted id (min body >= 4); `bn_`/`lr_`
// format-validate against the k1a-identity/v1 minimums. All bodies are
// lowercase Crockford base32 (excludes i/l/o/u). Ids are opaque byte strings.
// ---------------------------------------------------------------------------

const CROCKFORD_LOWER: &str = "0123456789abcdefghjkmnpqrstvwxyz";

fn is_crockford_lower(body: &str) -> bool {
    !body.is_empty() && body.chars().all(|c| CROCKFORD_LOWER.contains(c))
}

fn validate_prefixed_id(
    value: &str,
    prefix: &str,
    min_body: usize,
    field: &str,
) -> Result<(), BacklogItemError> {
    let stem = prefix.trim_end_matches('_');
    let body = value.strip_prefix(prefix).ok_or_else(|| BacklogItemError::InvalidId {
        field: field.to_string(),
        reason: format!("missing `{prefix}` prefix [pinned_prefix]"),
    })?;
    if body.len() < min_body {
        return Err(BacklogItemError::InvalidId {
            field: field.to_string(),
            reason: format!("body shorter than {min_body} characters [{stem}_min_body_{min_body}]"),
        });
    }
    if !is_crockford_lower(body) {
        return Err(BacklogItemError::InvalidId {
            field: field.to_string(),
            reason: "body is not lowercase Crockford base32 \
                     [crockford_lowercase crockford_excludes_iluo]"
                .to_string(),
        });
    }
    Ok(())
}

/// Validate an anvil-minted `bi_` id (body >= 4, R6).
pub fn validate_backlog_item_id(value: &str) -> Result<(), BacklogItemError> {
    validate_prefixed_id(value, "bi_", 4, "backlog_item_id")
}

/// Validate a K1a `bn_` business-node id (body >= 10).
pub fn validate_business_node_id(value: &str) -> Result<(), BacklogItemError> {
    validate_prefixed_id(value, "bn_", 10, "business_node_id")
}

/// Validate a K1a `lr_` run id (body >= 26).
pub fn validate_run_id(value: &str) -> Result<(), BacklogItemError> {
    validate_prefixed_id(value, "lr_", 26, "run_id")
}

/// Mint a fresh `bi_` id with a 26-character Crockford base32 body (R6/Task 2).
pub fn mint_backlog_item_id() -> String {
    use rand::Rng;
    let alphabet: Vec<char> = CROCKFORD_LOWER.chars().collect();
    let mut rng = rand::thread_rng();
    let body: String = (0..26)
        .map(|_| alphabet[rng.gen_range(0..alphabet.len())])
        .collect();
    format!("bi_{body}")
}

// ---------------------------------------------------------------------------
// Semantic validation (finite numbers, mirrors, closed shapes, origin invariant).
// ---------------------------------------------------------------------------

/// Enforce the structural/semantic invariants independent of lifecycle state.
///
/// Covers id grammar (R6), finite numeric inputs, `rank.position >= 1`, the
/// effort mirror equality, the value-gap ref kind restriction, the wake-shape
/// `ref`-only-for-non-manual rule, the null-playbook -> route-to-intake
/// invariant, and the two frozen `origin_binding` predictor/prediction rules.
pub fn validate_item_semantics(item: &BacklogItem) -> Result<(), BacklogItemError> {
    validate_backlog_item_id(&item.backlog_item_id)?;
    validate_business_node_id(&item.business_node_id)?;

    if item.intake.evidence_refs.is_empty() {
        return Err(BacklogItemError::SemanticViolation {
            detail: "intake.evidence_refs must be non-empty (>=1) [intake_evidence_nonempty]"
                .to_string(),
        });
    }

    // origin_binding: value-gap ref kind restriction + the two frozen predictor rules.
    validate_value_gap_kind(&item.origin_binding.value_gap_served, "origin_binding.value_gap_served")?;
    validate_origin_predictors(&item.origin_binding)?;

    // rank + inputs invariants.
    if let Some(rank) = &item.rank {
        if rank.position < 1 {
            return Err(BacklogItemError::SemanticViolation {
                detail: "incomplete rank: rank.position must be >= 1 [rank_position_min_1]"
                    .to_string(),
            });
        }
        let inputs = &rank.inputs;
        if !inputs.nick_weight.is_finite() {
            return Err(BacklogItemError::SemanticViolation {
                detail: "rank.inputs.nick_weight must be finite [numeric_finite]".to_string(),
            });
        }
        if !inputs.value_gap_magnitude.magnitude.is_finite() {
            return Err(BacklogItemError::SemanticViolation {
                detail: "rank.inputs.value_gap_magnitude.magnitude must be finite [numeric_finite]"
                    .to_string(),
            });
        }
        validate_value_gap_kind(
            &inputs.value_gap_magnitude.r#ref,
            "rank.inputs.value_gap_magnitude.ref",
        )?;
        // Effort mirror: a committed rank's effort must equal the top-level effort.
        match item.effort_class {
            Some(top) if top == inputs.effort_class => {}
            Some(_) => {
                return Err(BacklogItemError::SemanticViolation {
                    detail: "rank.inputs.effort_class must equal top-level effort_class \
                             [effort_class_mirror_equal]"
                        .to_string(),
                });
            }
            None => {
                return Err(BacklogItemError::SemanticViolation {
                    detail: "a committed rank requires a top-level effort_class mirror \
                             [effort_class_mirror_equal]"
                        .to_string(),
                });
            }
        }
    }

    // playbook_binding: a null playbook id implies route_to_intake = true (R19).
    if let Some(pb) = &item.playbook_binding {
        if pb.playbook_definition_id.is_none() && !pb.route_to_intake {
            return Err(BacklogItemError::SemanticViolation {
                detail: "null playbook_definition_id requires route_to_intake=true \
                         [null_playbook_implies_route]"
                    .to_string(),
            });
        }
    }

    // execution_binding.run_id is a K1a lr_ when present.
    if let Some(eb) = &item.execution_binding {
        validate_run_id(&eb.run_id)?;
    }

    // exit shape: kind must equal state when the exit is present as a canonical exit.
    if let Some(exit) = &item.exit {
        validate_exit_shape(exit)?;
    }

    Ok(())
}

/// Validate a decoded `BacklogGenesisInput` (§4.1). Enforces the candidate-time
/// invariants that are expressible before the engine mints id/state/history:
/// business-node id grammar, a non-empty typed intake evidence list, the origin
/// predictor/value invariants and Temper-only value-gap kind, and the
/// null-playbook implies route-to-intake rule. Enum closure (action / effort /
/// evidence kind / intake edge) is enforced by `deny_unknown_fields` + closed
/// enums at decode time, before this runs.
pub fn validate_genesis_input(input: &BacklogGenesisInput) -> Result<(), BacklogItemError> {
    validate_business_node_id(&input.business_node_id)?;

    if input.intake.evidence_refs.is_empty() {
        return Err(BacklogItemError::SemanticViolation {
            detail: "intake.evidence_refs must be non-empty (>=1) [intake_evidence_nonempty]"
                .to_string(),
        });
    }

    validate_value_gap_kind(
        &input.origin_binding.value_gap_served,
        "origin_binding.value_gap_served",
    )?;
    validate_origin_predictors(&input.origin_binding)?;

    if let Some(pb) = &input.playbook_binding {
        if pb.playbook_definition_id.is_none() && !pb.route_to_intake {
            return Err(BacklogItemError::SemanticViolation {
                detail: "null playbook_definition_id requires route_to_intake=true \
                         [null_playbook_implies_route]"
                    .to_string(),
            });
        }
    }

    Ok(())
}

/// The `reading_status` a `stamp`/genesis-declared outcome may carry initially is
/// closed to `{registered, unmeasurable_signed}` (§1 / NICK-GATE). `reading` is
/// only reachable through the external producer, never declared by a caller.
pub fn validate_initial_reading_status(status: ReadingStatus) -> Result<(), BacklogItemError> {
    match status {
        ReadingStatus::Registered | ReadingStatus::UnmeasurableSigned => Ok(()),
        ReadingStatus::Reading => Err(BacklogItemError::SemanticViolation {
            detail: "initial reading_status must be registered or unmeasurable_signed \
                     [outcome_initial_status_closed]"
                .to_string(),
        }),
    }
}

fn validate_value_gap_kind(r#ref: &EvidenceRef, field: &str) -> Result<(), BacklogItemError> {
    match r#ref.kind {
        EvidenceKind::TemperMeasure | EvidenceKind::TemperHotspot => Ok(()),
        _ => Err(BacklogItemError::SemanticViolation {
            detail: format!(
                "{field} must reference a temper_measure or temper_hotspot [value_gap_temper_only]"
            ),
        }),
    }
}

fn validate_origin_predictors(origin: &OriginBinding) -> Result<(), BacklogItemError> {
    let council = origin.minting_council_id.is_some();
    let experiment = origin.experiment_id.is_some();
    let has_predictor = council || experiment;
    let has_value = origin.predicted_value.is_some();
    match (council, experiment, origin.predicted_value) {
        // Both predictor ids null IFF predicted_value null.
        (false, false, None) => Ok(()),
        // Exactly one predictor present IFF a finite predicted_value present.
        (true, false, Some(v)) | (false, true, Some(v)) if v.is_finite() => Ok(()),
        // Both predictors present always rejects.
        (true, true, _) => Err(BacklogItemError::SemanticViolation {
            detail: "origin_binding may not carry both minting_council_id and experiment_id \
                     [origin_predictors_mutually_exclusive]"
                .to_string(),
        }),
        // A predictor id with no finite prediction.
        _ if has_predictor && !has_value => Err(BacklogItemError::SemanticViolation {
            detail: "origin_binding predictor id requires a finite predicted_value \
                     [origin_predictor_requires_value]"
                .to_string(),
        }),
        // A prediction with no predictor id (or a non-finite prediction).
        _ => Err(BacklogItemError::SemanticViolation {
            detail: "origin_binding predicted_value requires exactly one predictor id \
                     [origin_value_requires_predictor]"
                .to_string(),
        }),
    }
}

fn validate_wake_shape(wake: &WakeCondition) -> Result<(), BacklogItemError> {
    // `ref` is omitted only for kind = manual; every other kind requires it.
    match wake.kind {
        WakeKind::Manual => {
            if wake.r#ref.is_some() {
                return Err(BacklogItemError::SemanticViolation {
                    detail: "wake_condition.ref must be absent for kind=manual".to_string(),
                });
            }
        }
        _ => {
            if wake.r#ref.is_none() {
                return Err(BacklogItemError::SemanticViolation {
                    detail: "wake_condition.ref is required for non-manual kinds".to_string(),
                });
            }
        }
    }
    Ok(())
}

fn validate_exit_shape(exit: &Exit) -> Result<(), BacklogItemError> {
    if let Some(wake) = &exit.wake_condition {
        validate_wake_shape(wake)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Required-by-state matrix (R7/R8 — D1.3). Enforces presence requirements only;
// never a reject-if-present rule (§1.2). `history` is enforced by the reconcile
// layer, not here (§1.3).
// ---------------------------------------------------------------------------

/// Enforce the D1.3 required-by-state matrix for `state` (R7/R8).
pub fn validate_required_by_state(item: &BacklogItem, state: State) -> Result<(), BacklogItemError> {
    // intake.evidence_refs is always required (>=1) across every state.
    if item.intake.evidence_refs.is_empty() {
        return Err(BacklogItemError::MissingRequiredField {
            state: state.as_str().to_string(),
            field: "intake.evidence_refs".to_string(),
        });
    }

    let require = |present: bool, field: &str| -> Result<(), BacklogItemError> {
        if present {
            Ok(())
        } else {
            Err(BacklogItemError::MissingRequiredField {
                state: state.as_str().to_string(),
                field: field.to_string(),
            })
        }
    };

    // effort_class + playbook_binding are required at every state except candidate (§5).
    if state != State::Candidate {
        require(item.effort_class.is_some(), "effort_class")?;
        require(item.playbook_binding.is_some(), "playbook_binding")?;
    }

    // rank is required at ready/in_flight/done/superseded/aged_out; absent/optional at
    // parked; optional at candidate (R7).
    match state {
        State::Ready
        | State::InFlight
        | State::Done
        | State::Superseded
        | State::AgedOut => require(item.rank.is_some(), "rank")?,
        State::Candidate | State::Parked => {}
    }

    // execution_binding + outcome_binding (declared) are required from in_flight, carried
    // through done; source-dependent (tolerated) at parked/superseded/aged_out (§1.4/§1.5/§1.6).
    // The printed matrix row is `outcome_binding.success_measure_id (declared)`,
    // so a binding object that declares no measure does not satisfy it.
    if matches!(state, State::InFlight | State::Done) {
        require(item.execution_binding.is_some(), "execution_binding")?;
        require(item.outcome_binding.is_some(), "outcome_binding")?;
        require(
            declares_success_measure(item),
            "outcome_binding.success_measure_id",
        )?;
    }

    // exit requiredness + kind/subfield shape at the four exit states (R7).
    match state {
        State::Candidate | State::Ready | State::InFlight => { /* exit not required */ }
        State::Done => {
            let exit = require_exit(item, state, ExitKind::Done)?;
            let _ = exit;
        }
        State::Parked => {
            let exit = require_exit(item, state, ExitKind::Parked)?;
            require(exit.wake_condition.is_some(), "exit.wake_condition")?;
        }
        State::Superseded => {
            let exit = require_exit(item, state, ExitKind::Superseded)?;
            require(exit.superseded_by.is_some(), "exit.superseded_by")?;
        }
        State::AgedOut => {
            let exit = require_exit(item, state, ExitKind::AgedOut)?;
            require(exit.aged_out_reason.is_some(), "exit.aged_out_reason")?;
        }
    }

    Ok(())
}

fn require_exit(
    item: &BacklogItem,
    state: State,
    expected_kind: ExitKind,
) -> Result<&Exit, BacklogItemError> {
    let exit = item.exit.as_ref().ok_or_else(|| BacklogItemError::MissingRequiredField {
        state: state.as_str().to_string(),
        field: "exit".to_string(),
    })?;
    if exit.kind != expected_kind {
        return Err(BacklogItemError::SemanticViolation {
            detail: format!(
                "exit.kind must equal state `{}` at state `{}`",
                exit_kind_str(expected_kind),
                state.as_str()
            ),
        });
    }
    Ok(exit)
}

fn exit_kind_str(kind: ExitKind) -> &'static str {
    match kind {
        ExitKind::Done => "done",
        ExitKind::Parked => "parked",
        ExitKind::Superseded => "superseded",
        ExitKind::AgedOut => "aged_out",
    }
}

/// Report EVERY state-local required field missing from `item` for `state`,
/// as machine tokens. Unlike `validate_required_by_state` (which fail-fast
/// rejects on the first absence), this collects the complete missing set so a
/// caller can present all outstanding requirements at once. The `dependency_ready`
/// readiness guard and the declared `success_measure_id` are surfaced here as
/// their own tokens (they are the fields §5's row #1/#5 guards name), never as a
/// generic `rank`/`outcome_binding` absence.
pub fn missing_required_by_state(item: &BacklogItem, state: State) -> Vec<&'static str> {
    let mut missing = Vec::new();

    if item.intake.evidence_refs.is_empty() {
        missing.push("intake_evidence_nonempty");
    }
    if state != State::Candidate {
        if item.effort_class.is_none() {
            missing.push("effort_class");
        }
        if item.playbook_binding.is_none() {
            missing.push("playbook_binding");
        }
    }
    let rank_required = matches!(
        state,
        State::Ready | State::InFlight | State::Done | State::Superseded | State::AgedOut
    );
    if rank_required && item.rank.is_none() {
        missing.push("rank");
    }
    // #1/#14 readiness guard: entering `ready` requires a dependency-ready rank input.
    if state == State::Ready {
        let dep_ready = item
            .rank
            .as_ref()
            .map(|r| r.inputs.dependency_readiness.status == DependencyStatus::Ready)
            .unwrap_or(false);
        if !dep_ready {
            missing.push("dependency_ready");
        }
    }
    if matches!(state, State::InFlight | State::Done) {
        if item.execution_binding.is_none() {
            missing.push("execution_binding");
        }
        let has_measure = item
            .outcome_binding
            .as_ref()
            .map(|o| o.success_measure_id.is_some())
            .unwrap_or(false);
        if !has_measure {
            missing.push("success_measure_id");
        }
    }
    match state {
        State::Candidate | State::Ready | State::InFlight => {}
        State::Done => {
            if exit_of_kind(item, ExitKind::Done).is_none() {
                missing.push("exit");
            }
        }
        State::Parked => match exit_of_kind(item, ExitKind::Parked) {
            Some(exit) if exit.wake_condition.is_some() => {}
            _ => missing.push("wake_condition"),
        },
        State::Superseded => match exit_of_kind(item, ExitKind::Superseded) {
            Some(exit) if exit.superseded_by.is_some() => {}
            _ => missing.push("superseded_by"),
        },
        State::AgedOut => match exit_of_kind(item, ExitKind::AgedOut) {
            Some(exit) if exit.aged_out_reason.is_some() => {}
            _ => missing.push("aged_out_reason"),
        },
    }
    missing
}

/// `O` in the §5 proof: the outcome binding exists AND actually declares its
/// success measure. Binding presence alone is never `O` — §1.4 requires the
/// *declared* `outcome_binding.success_measure_id` to be carried, so an
/// undeclared binding can neither authorize a pickup nor be "carried" by
/// #11/#12.
fn declares_success_measure(item: &BacklogItem) -> bool {
    item.outcome_binding
        .as_ref()
        .map(|o| o.success_measure_id.is_some())
        .unwrap_or(false)
}

fn exit_of_kind(item: &BacklogItem, kind: ExitKind) -> Option<&Exit> {
    item.exit.as_ref().filter(|e| e.kind == kind)
}

/// Enforce the always-required `history` invariant (§1.3): a present, non-empty,
/// gap-free sequence that begins with `created(seq: 0)`. This is the
/// reconcile-layer guard the state-local validator deliberately does NOT carry.
pub fn validate_history(entries: &[HistoryEntry]) -> Result<(), BacklogItemError> {
    if entries.is_empty() {
        return Err(BacklogItemError::SemanticViolation {
            detail: "history must be present and non-empty [history]".to_string(),
        });
    }
    let first = &entries[0];
    if first.seq != 0 || first.kind != HistoryKind::Created {
        return Err(BacklogItemError::SemanticViolation {
            detail: "history must begin with created(seq: 0); a genesis state_change is never recorded [history]".to_string(),
        });
    }
    for (idx, entry) in entries.iter().enumerate() {
        if entry.seq != idx as u64 {
            return Err(BacklogItemError::SemanticViolation {
                detail: format!("history sequence must be contiguous from 0 [history] (at {idx})"),
            });
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Legal-transition table (R10/R11/R12 — D2.1). The 16 non-genesis edges expand
// to the 29 printed `(from, to, role)` tuples; genesis (#0) is the begin mint,
// not a seed row.
// ---------------------------------------------------------------------------

/// One row of the D2.1 table: its edge number, from/to states, and admissible roles.
struct EdgeRow {
    number: u8,
    from: State,
    to: State,
    roles: &'static [DriverRole],
}

const EDGE_TABLE: &[EdgeRow] = &[
    EdgeRow { number: 1, from: State::Candidate, to: State::Ready, roles: &[DriverRole::OrganLoop, DriverRole::Orchestrator] },
    EdgeRow { number: 2, from: State::Candidate, to: State::Parked, roles: &[DriverRole::NickShape, DriverRole::OrganLoop] },
    EdgeRow { number: 3, from: State::Candidate, to: State::Superseded, roles: &[DriverRole::NickShape, DriverRole::Orchestrator] },
    EdgeRow { number: 4, from: State::Candidate, to: State::AgedOut, roles: &[DriverRole::EngineAuto] },
    EdgeRow { number: 5, from: State::Ready, to: State::InFlight, roles: &[DriverRole::TrackDriver] },
    EdgeRow { number: 6, from: State::Ready, to: State::Candidate, roles: &[DriverRole::OrganLoop, DriverRole::Orchestrator, DriverRole::NickShape] },
    EdgeRow { number: 7, from: State::Ready, to: State::Parked, roles: &[DriverRole::NickShape, DriverRole::OrganLoop] },
    EdgeRow { number: 8, from: State::Ready, to: State::Superseded, roles: &[DriverRole::NickShape, DriverRole::Orchestrator] },
    EdgeRow { number: 9, from: State::Ready, to: State::AgedOut, roles: &[DriverRole::EngineAuto] },
    EdgeRow { number: 10, from: State::InFlight, to: State::Done, roles: &[DriverRole::EngineAuto, DriverRole::NickShape] },
    EdgeRow { number: 11, from: State::InFlight, to: State::Parked, roles: &[DriverRole::TrackDriver, DriverRole::NickShape] },
    EdgeRow { number: 12, from: State::InFlight, to: State::Superseded, roles: &[DriverRole::NickShape, DriverRole::Orchestrator] },
    EdgeRow { number: 13, from: State::Parked, to: State::Candidate, roles: &[DriverRole::EngineAuto, DriverRole::NickShape] },
    EdgeRow { number: 14, from: State::Parked, to: State::Ready, roles: &[DriverRole::EngineAuto, DriverRole::NickShape] },
    EdgeRow { number: 15, from: State::Parked, to: State::Superseded, roles: &[DriverRole::NickShape, DriverRole::Orchestrator] },
    EdgeRow { number: 16, from: State::Parked, to: State::AgedOut, roles: &[DriverRole::EngineAuto] },
];

/// The number of printed `(from, to, role)` tuples across the 16 non-genesis edges.
pub const LEGAL_TRANSITION_TUPLE_COUNT: usize = 29;

/// Enumerate every admissible `(from, to, role)` tuple (the seed's coarse gate).
pub fn legal_transition_tuples() -> Vec<(State, State, DriverRole)> {
    let mut out = Vec::new();
    for row in EDGE_TABLE {
        for role in row.roles {
            out.push((row.from, row.to, *role));
        }
    }
    out
}

fn find_edge(from: State, to: State) -> Option<&'static EdgeRow> {
    EDGE_TABLE.iter().find(|r| r.from == from && r.to == to)
}

/// Admit only the D2.1 edges + item-local guards; reject everything else fail-loud
/// (R10/R11/R12). Context-dependent guards (re-rank budget, standing veto, strict
/// top-ready, ATTEND approval, wake reachability) are enforced by the governed
/// preparation seam, not this pure function; the item-local guard fields are
/// enforced here.
pub fn validate_transition(
    item: &BacklogItem,
    from: State,
    to: State,
    driver: DriverRole,
) -> Result<(), BacklogItemError> {
    if from != item.state {
        return Err(BacklogItemError::StateMismatch {
            declared: from.as_str().to_string(),
            actual: item.state.as_str().to_string(),
        });
    }
    if from.is_terminal() {
        return Err(BacklogItemError::TerminalNoOutgoing { from: from.as_str().to_string() });
    }
    let edge = find_edge(from, to).ok_or_else(|| BacklogItemError::IllegalTransition {
        from: from.as_str().to_string(),
        to: to.as_str().to_string(),
    })?;
    if !edge.roles.contains(&driver) {
        return Err(BacklogItemError::WrongRole { edge: edge.number, role: driver_role_str(driver).to_string() });
    }
    check_edge_guard(item, edge)
}

fn driver_role_str(role: DriverRole) -> &'static str {
    role.as_str()
}

fn dependency_ready(item: &BacklogItem) -> bool {
    item.rank
        .as_ref()
        .map(|r| r.inputs.dependency_readiness.status == DependencyStatus::Ready)
        .unwrap_or(false)
}

fn staged_wake(item: &BacklogItem) -> Option<&WakeCondition> {
    item.exit.as_ref().and_then(|e| e.wake_condition.as_ref())
}

fn staged_superseded(item: &BacklogItem) -> bool {
    item.exit.as_ref().and_then(|e| e.superseded_by.as_ref()).is_some()
}

fn staged_reason(item: &BacklogItem) -> Option<AgedOutReason> {
    item.exit.as_ref().and_then(|e| e.aged_out_reason)
}

fn check_edge_guard(item: &BacklogItem, edge: &EdgeRow) -> Result<(), BacklogItemError> {
    let guard = |ok: bool, detail: &str| -> Result<(), BacklogItemError> {
        if ok {
            Ok(())
        } else {
            Err(BacklogItemError::GuardUnsatisfied { edge: edge.number, detail: detail.to_string() })
        }
    };
    match edge.number {
        1 => guard(item.rank.is_some() && dependency_ready(item), "rank populated + dependency ready"),
        2 | 7 => guard(staged_wake(item).is_some(), "wake_condition set"),
        3 | 8 | 15 => guard(staged_superseded(item), "superseded_by set"),
        5 => guard(
            item.execution_binding.is_some() && declares_success_measure(item),
            "execution_binding + declared outcome_binding.success_measure_id",
        ),
        // #10 names WHICH half of the frozen done predicate failed so a caller
        // can never confuse "no stored reading" with "unsigned unmeasurable"
        // with "a track-completion note is not evidence".
        10 => match &item.outcome_binding {
            Some(ob) if done_rule_satisfied(ob) => Ok(()),
            Some(ob) => match ob.reading_status {
                ReadingStatus::Registered => guard(
                    false,
                    "done-rule: no stored reading (reading_status=registered) and no \
                     unmeasurable_signed sign-off; track completion alone is insufficient",
                ),
                ReadingStatus::UnmeasurableSigned => guard(
                    false,
                    "done-rule: unmeasurable_signed requires Nick sign-off \
                     (outcome_binding.nick_signoff)",
                ),
                ReadingStatus::Reading => Err(BacklogItemError::DoneRuleUnsatisfied),
            },
            None => guard(
                false,
                "done-rule: no outcome_binding is bound, so neither a stored reading nor a \
                 sign-off can be evidenced; track completion alone is insufficient",
            ),
        },
        11 => guard(
            staged_wake(item).is_some()
                && item.execution_binding.is_some()
                && declares_success_measure(item),
            "wake_condition set + both pickup bindings carried",
        ),
        12 => guard(
            staged_superseded(item)
                && item.execution_binding.is_some()
                && declares_success_measure(item),
            "superseded_by set + both pickup bindings carried",
        ),
        14 => guard(item.rank.is_some() && dependency_ready(item), "rank retained + dependency ready"),
        4 => guard(staged_reason(item) == Some(AgedOutReason::StaleNoReady), "aged_out_reason=stale_no_ready"),
        9 => guard(staged_reason(item) == Some(AgedOutReason::StaleNoPickup), "aged_out_reason=stale_no_pickup"),
        16 => guard(staged_reason(item) == Some(AgedOutReason::WakeUnreachable), "aged_out_reason=wake_unreachable"),
        // #6 (de-triage) and #13 (wake re-triage) carry no item-local guard beyond
        // tuple+role; their conditions (dependency blocked / SHAPE demotion / wake met)
        // are context-resolved by the governed preparation seam.
        6 | 13 => Ok(()),
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Done-rule (R22 — D6.3).
// ---------------------------------------------------------------------------

/// The frozen done predicate: `reading` OR (`unmeasurable_signed` AND `nick_signoff`).
pub fn done_rule_satisfied(ob: &OutcomeBinding) -> bool {
    match ob.reading_status {
        ReadingStatus::Reading => true,
        ReadingStatus::UnmeasurableSigned => ob.nick_signoff,
        ReadingStatus::Registered => false,
    }
}

// ---------------------------------------------------------------------------
// State-entry normalization (§1.8). History-silent; constructs/clears the exit,
// resets only the target age on entries into {candidate, ready}; never clears
// rank or bindings.
// ---------------------------------------------------------------------------

/// Canonicalize `item` for entry into `target`: set the mirrored state, construct
/// or clear the exit from staged intent, and reset the age on advancement entries.
/// Appends no history and preserves rank (beyond the age reset) and all bindings.
pub fn normalize_on_state_entry(item: &mut BacklogItem, target: State) {
    item.state = target;
    match target {
        State::Candidate | State::Ready => {
            // Entries into candidate/ready are advancements for age accounting (§1.10).
            item.exit = None;
            if let Some(rank) = item.rank.as_mut() {
                rank.inputs.age = 0;
            }
        }
        State::InFlight => {
            item.exit = None;
        }
        State::Done => {
            item.exit = Some(Exit {
                kind: ExitKind::Done,
                wake_condition: None,
                superseded_by: None,
                aged_out_reason: None,
            });
        }
        State::Parked => {
            let wake = item.exit.take().and_then(|e| e.wake_condition);
            item.exit = Some(Exit {
                kind: ExitKind::Parked,
                wake_condition: wake,
                superseded_by: None,
                aged_out_reason: None,
            });
        }
        State::Superseded => {
            let superseded_by = item.exit.take().and_then(|e| e.superseded_by);
            item.exit = Some(Exit {
                kind: ExitKind::Superseded,
                wake_condition: None,
                superseded_by,
                aged_out_reason: None,
            });
        }
        State::AgedOut => {
            let reason = item.exit.take().and_then(|e| e.aged_out_reason);
            item.exit = Some(Exit {
                kind: ExitKind::AgedOut,
                wake_condition: None,
                superseded_by: None,
                aged_out_reason: reason,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Policy (R15/R25): the ordered comparator specification and the age budget,
// with strict parsers. Present malformed configured values are errors, never
// defaults (no silent fallback — §7 policy-parser risk).
// ---------------------------------------------------------------------------

/// One comparator input token, applied in declared order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComparatorToken {
    ValueGapDesc,
    NickWeightDesc,
    AgeDesc,
    DependencyReadyFirst,
    EffortAsc,
}

impl ComparatorToken {
    fn parse(token: &str) -> Option<ComparatorToken> {
        match token {
            "value_gap_desc" => Some(ComparatorToken::ValueGapDesc),
            "nick_weight_desc" => Some(ComparatorToken::NickWeightDesc),
            "age_desc" => Some(ComparatorToken::AgeDesc),
            "dependency_ready_first" => Some(ComparatorToken::DependencyReadyFirst),
            "effort_asc" => Some(ComparatorToken::EffortAsc),
            _ => None,
        }
    }
}

/// The complete set of comparator tokens; every one must appear exactly once.
const ALL_COMPARATOR_TOKENS: [ComparatorToken; 5] = [
    ComparatorToken::ValueGapDesc,
    ComparatorToken::NickWeightDesc,
    ComparatorToken::AgeDesc,
    ComparatorToken::DependencyReadyFirst,
    ComparatorToken::EffortAsc,
];

/// The engine-configured backlog ordering + age-out budget policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacklogPolicy {
    pub age_budget: NonZeroU32,
    pub comparator: Vec<ComparatorToken>,
}

impl Default for BacklogPolicy {
    fn default() -> Self {
        BacklogPolicy {
            age_budget: NonZeroU32::new(3).expect("3 is non-zero"),
            comparator: ALL_COMPARATOR_TOKENS.to_vec(),
        }
    }
}

/// Parse the `ANVIL_BACKLOG_COMPARATOR` grammar: an ASCII comma-separated list of
/// the exact lowercase tokens, each appearing exactly once. Outer and per-token
/// ASCII whitespace is trimmed; empty, unknown, duplicate, or missing tokens reject.
pub fn parse_comparator(raw: &str) -> Result<Vec<ComparatorToken>, BacklogItemError> {
    let trimmed = raw.trim_matches(|c: char| c.is_ascii_whitespace());
    if trimmed.is_empty() {
        return Err(BacklogItemError::InvalidPolicy {
            detail: "comparator (ANVIL_BACKLOG_COMPARATOR) is empty".to_string(),
        });
    }
    let mut tokens = Vec::new();
    for part in trimmed.split(',') {
        let token = part.trim_matches(|c: char| c.is_ascii_whitespace());
        if token.is_empty() {
            return Err(BacklogItemError::InvalidPolicy {
                detail: "empty comparator token".to_string(),
            });
        }
        let parsed = ComparatorToken::parse(token).ok_or_else(|| BacklogItemError::InvalidPolicy {
            detail: format!("unknown comparator token `{token}`"),
        })?;
        if tokens.contains(&parsed) {
            return Err(BacklogItemError::InvalidPolicy {
                detail: format!("duplicate comparator token `{token}`"),
            });
        }
        tokens.push(parsed);
    }
    if tokens.len() != ALL_COMPARATOR_TOKENS.len() {
        return Err(BacklogItemError::InvalidPolicy {
            detail: "comparator must contain every token exactly once".to_string(),
        });
    }
    Ok(tokens)
}

/// Parse the `ANVIL_BACKLOG_AGE_BUDGET` grammar: outer ASCII whitespace trimmed,
/// then only unsigned decimal digits parsing to a `NonZeroU32`. Signs, inner
/// whitespace, zero, and overflow reject.
pub fn parse_age_budget(raw: &str) -> Result<NonZeroU32, BacklogItemError> {
    let trimmed = raw.trim_matches(|c: char| c.is_ascii_whitespace());
    if trimmed.is_empty() || !trimmed.bytes().all(|b| b.is_ascii_digit()) {
        return Err(BacklogItemError::InvalidPolicy {
            detail: format!(
                "age budget (ANVIL_BACKLOG_AGE_BUDGET) must be unsigned decimal digits, got `{raw}`"
            ),
        });
    }
    let value: u32 = trimmed.parse().map_err(|_| BacklogItemError::InvalidPolicy {
        detail: format!("age budget (ANVIL_BACKLOG_AGE_BUDGET) overflows u32: `{trimmed}`"),
    })?;
    NonZeroU32::new(value).ok_or_else(|| BacklogItemError::InvalidPolicy {
        detail: "age budget (ANVIL_BACKLOG_AGE_BUDGET) must be non-zero".to_string(),
    })
}

// ---------------------------------------------------------------------------
// Rank materialization (R15/R16/R17). Pure, total, deterministic per-organ order.
// ---------------------------------------------------------------------------

fn compare_by_policy(a: &BacklogItem, b: &BacklogItem, policy: &BacklogPolicy) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (ai, bi) = match (a.rank.as_ref(), b.rank.as_ref()) {
        (Some(ar), Some(br)) => (&ar.inputs, &br.inputs),
        // Ranked items always carry inputs; if one is missing, fall to the id tie-break.
        _ => return a.backlog_item_id.cmp(&b.backlog_item_id),
    };
    for token in &policy.comparator {
        let ord = match token {
            ComparatorToken::ValueGapDesc => bi
                .value_gap_magnitude
                .magnitude
                .total_cmp(&ai.value_gap_magnitude.magnitude),
            ComparatorToken::NickWeightDesc => bi.nick_weight.total_cmp(&ai.nick_weight),
            ComparatorToken::AgeDesc => bi.age.cmp(&ai.age),
            ComparatorToken::DependencyReadyFirst => {
                dependency_rank(ai).cmp(&dependency_rank(bi))
            }
            ComparatorToken::EffortAsc => ai.effort_class.cmp(&bi.effort_class),
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    // Final total tie-break on the opaque id byte order (R15).
    a.backlog_item_id.cmp(&b.backlog_item_id)
}

fn dependency_rank(inputs: &RankInputs) -> u8 {
    match inputs.dependency_readiness.status {
        DependencyStatus::Ready => 0,
        DependencyStatus::Blocked => 1,
    }
}

/// Deterministically materialize 1-based per-organ positions for the `{candidate,
/// ready}` items that carry complete rank inputs. Items outside those states or
/// without rank inputs are excluded (they belong to the pre-triage partition).
/// Given a fixed comparator + input set the position vector is reproducible.
pub fn materialize_rank(
    items_in_organ: &[BacklogItem],
    policy: &BacklogPolicy,
) -> Vec<(String, u32)> {
    let mut rankable: Vec<&BacklogItem> = items_in_organ
        .iter()
        .filter(|item| matches!(item.state, State::Candidate | State::Ready))
        .filter(|item| item.rank.is_some())
        .collect();
    rankable.sort_by(|a, b| compare_by_policy(a, b, policy));
    rankable
        .into_iter()
        .enumerate()
        .map(|(idx, item)| (item.backlog_item_id.clone(), (idx as u32) + 1))
        .collect()
}

/// The strict same-organ context one preparation is bound to (plan §1 topology,
/// Task 4). A one-item read is NEVER sufficient evidence for a rank-sensitive
/// guard or for #5's top-ready proof, so every such guard reads this instead.
///
/// `context_hash` aggregates every item revision that participated. A prepared
/// capability carries it and the store re-derives it at consume time: if any
/// same-organ item moved, the whole batch is refused.
#[derive(Debug, Clone, PartialEq)]
pub struct BacklogTransitionContext {
    pub business_node_id: String,
    /// Candidate/ready items carrying a complete rank, in resolved comparator
    /// order.
    pub ranked: Vec<BacklogItem>,
    /// Valid candidates with no complete rank input set — the explicit
    /// pre-triage partition (§1.12). Never silently dropped.
    pub unranked_candidate_ids: Vec<String>,
    pub policy: BacklogPolicy,
    /// Content hash of the K8 registry projection at read time.
    pub registry_hash: Option<String>,
    /// One aggregate hash over every participating item revision.
    pub context_hash: String,
}

impl BacklogTransitionContext {
    /// The top ready item id under the resolved comparator, if any. `#5`'s
    /// top-ready proof reads this, never a single item.
    pub fn top_ready(&self) -> Option<&str> {
        self.ranked
            .iter()
            .find(|item| item.state == State::Ready)
            .map(|item| item.backlog_item_id.as_str())
    }
}

// ---------------------------------------------------------------------------
// Governed preparation (§1 write topology, plan Tasks 4 & 6).
// ---------------------------------------------------------------------------

/// Where a prepared transition came from. The `Evaluation` origin is
/// `pub(crate)`-constructible ONLY, so no request mapping, gRPC handler, or MCP
/// tool can forge it: `#4/#9/#13/#14/#16` under `engine_auto` are reachable
/// exclusively from `evaluate_backlog` inside this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransitionOrigin(OriginKind);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OriginKind {
    PublicSnapshot,
    Evaluation,
}

impl TransitionOrigin {
    /// The public gRPC Snapshot origin. Admits the printed human/loop roles and
    /// admits `engine_auto` ONLY for printed row #10 with a stored reading.
    pub fn public_snapshot() -> Self {
        TransitionOrigin(OriginKind::PublicSnapshot)
    }

    /// Constructible only inside this crate — `evaluate_backlog` is its one
    /// caller (plan Task 8).
    pub(crate) fn evaluation() -> Self {
        TransitionOrigin(OriginKind::Evaluation)
    }

    pub fn is_evaluation(self) -> bool {
        self.0 == OriginKind::Evaluation
    }
}

/// The five rows whose `engine_auto` role is evaluation-origin only. Row #10 is
/// deliberately absent: its `engine_auto` path is public and guarded solely by
/// the stored done-rule reading.
pub const EVALUATION_ONLY_ROWS: [u8; 5] = [4, 9, 13, 14, 16];

/// One derived position rewrite produced by an advancement batch (§1.10).
/// Its history append is authored with a server-fixed private `engine_auto`
/// role; a request can never select that authority.
#[derive(Debug, Clone, PartialEq)]
pub struct DerivedRankAppend {
    pub bi_id: String,
    pub position: u32,
    pub explanation: String,
    /// The triggering `state_change` sequence on the advancing item.
    pub triggering_state_change_seq: Option<u64>,
}

/// The consume-once preparation. It owns the expected source revisions, the
/// complete normalized target, the printed-role `state_change`, and — for an
/// advancement — every derived position rewrite, so nothing about the write is
/// recomputed after validation.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedBacklogTransition {
    pub bi_id: String,
    pub edge: u8,
    pub from: State,
    pub to: State,
    pub role: DriverRole,
    pub actor: ActorIdentity,
    pub at: String,
    /// The fully normalized, fully validated target item.
    pub target_item: BacklogItem,
    /// The one reconciled `state_change` this transition appends. Its `seq` is
    /// allocated by the storage layer.
    pub state_change: HistoryEntry,
    pub approver: Option<String>,
    pub note: Option<String>,
    /// #5 only: the fresh, unconsumed `binding_stamped` sequence this pickup
    /// consumes (§1.11).
    pub binding_stamp_seq: Option<u64>,
    /// The strict organ-context hash this capability is bound to, when the row
    /// needed context evidence.
    pub context_hash: Option<String>,
    /// Every same-organ position rewritten by this advancement, in byte-ID
    /// order. Empty for a non-advancement row.
    pub derived_rank: Vec<DerivedRankAppend>,
}

/// Whether `to` is an advancement entry for age accounting (§1.10).
fn is_advancement(edge: u8) -> bool {
    matches!(edge, 1 | 6 | 13 | 14)
}

/// The latest history sequence at which the item entered `ready`.
fn last_ready_entry_seq(history: &[HistoryEntry]) -> Option<u64> {
    history
        .iter()
        .filter(|e| e.kind == HistoryKind::StateChange && e.to_state == Some(State::Ready))
        .map(|e| e.seq)
        .max()
}

/// The latest `binding_stamped` sequence.
fn latest_binding_stamp_seq(history: &[HistoryEntry]) -> Option<u64> {
    history
        .iter()
        .filter(|e| e.kind == HistoryKind::BindingStamped)
        .map(|e| e.seq)
        .max()
}

/// Every `binding_stamp_seq` already consumed by a prior `#5`.
fn consumed_binding_stamps(history: &[HistoryEntry]) -> Vec<u64> {
    history
        .iter()
        .filter(|e| e.kind == HistoryKind::StateChange && e.to_state == Some(State::InFlight))
        .filter_map(|e| {
            e.payload
                .as_ref()
                .and_then(|p| p.get("binding_stamp_seq"))
                .and_then(|v| v.as_u64())
        })
        .collect()
}

/// Whether a standing (set-and-not-lifted) age-out veto exists.
pub fn has_standing_veto(history: &[HistoryEntry]) -> bool {
    let mut standing: Vec<u64> = Vec::new();
    for entry in history.iter().filter(|e| e.kind == HistoryKind::Veto) {
        let action = entry
            .payload
            .as_ref()
            .and_then(|p| p.get("veto_action"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        match action {
            "set" => standing.push(entry.seq),
            "lift" => {
                if let Some(lifted) = entry
                    .payload
                    .as_ref()
                    .and_then(|p| p.get("lifts_seq"))
                    .and_then(|v| v.as_u64())
                {
                    standing.retain(|s| *s != lifted);
                }
            }
            _ => {}
        }
    }
    !standing.is_empty()
}

/// The one governed preparation seam (§1). Every post-genesis K8 lifecycle move
/// — public Snapshot and evaluation alike — goes through here.
///
/// Order is fixed: origin/role authority → exact row/role/guard → NICK-GATE and
/// context guards → state-entry normalization → advancement rematerialization →
/// complete-target validation. Nothing is written; the caller journals the
/// returned value and consumes it exactly once.
#[allow(clippy::too_many_arguments)]
pub fn prepare_backlog_transition(
    source: &BacklogItem,
    history: &[HistoryEntry],
    from: State,
    to: State,
    role: DriverRole,
    actor: &ActorIdentity,
    at: &str,
    origin: TransitionOrigin,
    context: Option<&BacklogTransitionContext>,
    approver: Option<&str>,
) -> Result<PreparedBacklogTransition, BacklogItemError> {
    // A terminal source has NO outgoing edge at all, so it is refused as
    // terminal rather than reported as a missing table row.
    if from.is_terminal() {
        return Err(BacklogItemError::TerminalNoOutgoing {
            from: from.as_str().to_string(),
        });
    }
    let edge = find_edge(from, to)
        .ok_or_else(|| BacklogItemError::IllegalTransition {
            from: from.as_str().to_string(),
            to: to.as_str().to_string(),
        })?
        .number;

    // ── origin authority ────────────────────────────────────────────────────
    if role == DriverRole::EngineAuto
        && !origin.is_evaluation()
        && EVALUATION_ONLY_ROWS.contains(&edge)
    {
        return Err(BacklogItemError::WrongRole {
            edge,
            role: "engine_auto (evaluation-origin only)".to_string(),
        });
    }
    if origin.is_evaluation() && !EVALUATION_ONLY_ROWS.contains(&edge) {
        return Err(BacklogItemError::WrongRole {
            edge,
            role: "evaluation origin may drive only #4/#9/#13/#14/#16".to_string(),
        });
    }

    // ── exact row, role, and item-local guard ───────────────────────────────
    validate_transition(source, from, to, role)?;

    // ── history invariants the state-local validator cannot see (§1.3) ──────
    validate_history(history)?;

    // ── a rank-sensitive edge refuses UNRESOLVED rank-affecting edits ───────
    // (§1.9) rather than advancing on stale inputs. `recompute_rank` is the one
    // producer that resolves them.
    let rank_sensitive = matches!(edge, 1 | 14) || (matches!(edge, 6 | 13) && source.rank.is_some());
    if rank_sensitive {
        let unresolved = unresolved_rank_shape_seqs(history)?;
        if !unresolved.is_empty() {
            return Err(BacklogItemError::GuardUnsatisfied {
                edge,
                detail: format!(
                    "unresolved rank-affecting shape_edit sequences {unresolved:?} must be \
                     consumed by recompute_rank before a rank-sensitive transition"
                ),
            });
        }
    }

    // ── NICK-GATE: a standing veto blocks every age-out row ─────────────────
    if matches!(edge, 4 | 9 | 16) && has_standing_veto(history) {
        return Err(BacklogItemError::GuardUnsatisfied {
            edge,
            detail: "a standing age-out veto is unresolved".to_string(),
        });
    }

    // ── #5: conservative ATTEND gate + strict top-ready + fresh stamp ───────
    let mut binding_stamp_seq = None;
    let mut context_hash = None;
    if edge == 5 {
        let ctx = context.ok_or_else(|| BacklogItemError::GuardUnsatisfied {
            edge,
            detail: "a pickup requires the strict same-organ context; a one-item read is \
                     never sufficient evidence of top-ready"
                .to_string(),
        })?;
        // NICK-GATE (§7): until A4 supplies an autonomy policy every pickup
        // requires an engine-verified nonempty approver. Autonomy is never
        // inferred.
        let approver_value = approver.unwrap_or("").trim();
        if approver_value.is_empty() {
            return Err(BacklogItemError::GuardUnsatisfied {
                edge,
                detail: "ATTEND approval is required for every pickup under this track's \
                         conservative gate"
                    .to_string(),
            });
        }
        if ctx.top_ready() != Some(source.backlog_item_id.as_str()) {
            return Err(BacklogItemError::GuardUnsatisfied {
                edge,
                detail: "the item is not top-ready in its organ under the resolved comparator"
                    .to_string(),
            });
        }
        let stamp = latest_binding_stamp_seq(history).ok_or_else(|| {
            BacklogItemError::GuardUnsatisfied {
                edge,
                detail: "no binding_stamped entry exists".to_string(),
            }
        })?;
        if let Some(ready_seq) = last_ready_entry_seq(history) {
            if stamp < ready_seq {
                return Err(BacklogItemError::GuardUnsatisfied {
                    edge,
                    detail: "the latest binding_stamp_seq predates the current entry into ready; \
                             a wake or re-entry requires a fresh stamp"
                        .to_string(),
                });
            }
        }
        if consumed_binding_stamps(history).contains(&stamp) {
            return Err(BacklogItemError::GuardUnsatisfied {
                edge,
                detail: "that binding_stamp_seq was already consumed by a prior pickup"
                    .to_string(),
            });
        }
        binding_stamp_seq = Some(stamp);
        context_hash = Some(ctx.context_hash.clone());
    }

    // ── #11/#12/#15: source-dependent binding carry (§1.4/§1.5) ─────────────
    let carried_execution = source.execution_binding.clone();
    let carried_outcome = source.outcome_binding.clone();

    // ── normalize target state and exit (history-silent) ────────────────────
    let mut target = source.clone();
    normalize_on_state_entry(&mut target, to);
    // "Carries" means byte-preserves the loaded value; the normalizer never
    // clears bindings, but assert it rather than assume it.
    if target.execution_binding != carried_execution || target.outcome_binding != carried_outcome {
        return Err(BacklogItemError::SemanticViolation {
            detail: "state-entry normalization must byte-preserve both pickup bindings"
                .to_string(),
        });
    }

    // ── advancement: rematerialize the whole same-organ ranked set ──────────
    let mut derived_rank = Vec::new();
    if is_advancement(edge) && target.rank.is_some() {
        let ctx = context.ok_or_else(|| BacklogItemError::GuardUnsatisfied {
            edge,
            detail: "a ranked advancement must be prepared against the strict organ context"
                .to_string(),
        })?;
        let mut hypothetical: Vec<BacklogItem> = ctx
            .ranked
            .iter()
            .filter(|i| i.backlog_item_id != target.backlog_item_id)
            .cloned()
            .collect();
        hypothetical.push(target.clone());
        let positions = materialize_rank(&hypothetical, &ctx.policy);
        for (bi_id, position) in positions {
            let prior = ctx
                .ranked
                .iter()
                .find(|i| i.backlog_item_id == bi_id)
                .and_then(|i| i.rank.as_ref())
                .map(|r| r.position);
            if bi_id == target.backlog_item_id {
                if let Some(rank) = target.rank.as_mut() {
                    rank.position = position;
                    rank.explanation =
                        format!("position {position} under the resolved comparator");
                }
            }
            if prior != Some(position) {
                derived_rank.push(DerivedRankAppend {
                    bi_id: bi_id.clone(),
                    position,
                    explanation: format!("position {position} under the resolved comparator"),
                    triggering_state_change_seq: None,
                });
            }
        }
        derived_rank.sort_by(|a, b| a.bi_id.cmp(&b.bi_id));
        context_hash = Some(ctx.context_hash.clone());
    }

    // ── validate the COMPLETE target only after it is coherent ──────────────
    validate_item_semantics(&target)?;
    validate_required_by_state(&target, to)?;

    let mut payload = serde_yaml::Mapping::new();
    if let Some(seq) = binding_stamp_seq {
        payload.insert(
            serde_yaml::Value::String("binding_stamp_seq".to_string()),
            serde_yaml::Value::Number(seq.into()),
        );
    }

    let state_change = HistoryEntry {
        // Allocated by the storage layer against the strict live tail.
        seq: 0,
        actor: actor.name.clone(),
        role,
        at: at.to_string(),
        kind: HistoryKind::StateChange,
        from_state: Some(from),
        to_state: Some(to),
        payload: if payload.is_empty() {
            None
        } else {
            Some(serde_yaml::Value::Mapping(payload))
        },
        note: None,
    };

    Ok(PreparedBacklogTransition {
        bi_id: source.backlog_item_id.clone(),
        edge,
        from,
        to,
        role,
        actor: actor.clone(),
        at: at.to_string(),
        target_item: target,
        state_change,
        approver: approver.map(|s| s.to_string()),
        note: None,
        binding_stamp_seq,
        context_hash,
        derived_rank,
    })
}


// ---------------------------------------------------------------------------
// Uniform typed mutations, history producers, ranking, queues, and DECIDE
// (plan Task 7). Every named post-genesis operation below is a PURE preparation
// over strictly loaded items: it validates source state and field-group
// authority, applies a closed typed body edit or the closed pending rank
// intent, validates the resulting current-state body WITHOUT changing lifecycle
// state, and returns the ordered item/history deltas the journal commits.
// Rejection precedes every write, and each affected item receives exactly one
// fixed-kind history append per public named operation.
// ---------------------------------------------------------------------------

/// The fixed `shape_edited` payload key holding the closed caller-authored rank
/// intent that `recompute_rank` later consumes (§1.9). There is no sidecar and
/// no partial `Rank`.
const PENDING_KEY: &str = "pending";
/// The fixed `rank_recomputed` payload key naming the consumed `shape_edit`
/// sequences (§1.9).
const CONSUMED_KEY: &str = "consumed_shape_seqs";
/// The fixed DECIDE payload key carrying a proposal identity.
const PROPOSAL_KEY: &str = "proposal_id";
/// The fixed DECIDE payload key carrying the organ-context hash a proposal was
/// authored against, so a stale commit is detectable without a sidecar.
const PROPOSAL_CONTEXT_KEY: &str = "organ_context";
/// The exact reason string an unranked pre-triage candidate carries (§1.12).
pub const UNRANKED_REASON: &str = "rank_not_materialized";

/// One field-level edit. Absence of the whole value means "untouched"; `Clear`
/// means "set to null". There is no third state, so an edit can never silently
/// mean both.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldEdit<T> {
    Set(T),
    Clear,
}

/// The frozen `shape_edit` body. Nothing outside this closed set is editable by
/// a SHAPE caller: lifecycle state, rank position, engine-owned rank age, the
/// effort mirror, and the explanation are all engine-owned.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShapeEditBody {
    pub action_class: Option<ActionClass>,
    pub description: Option<FieldEdit<String>>,
    pub effort_class: Option<FieldEdit<EffortClass>>,
    pub playbook_binding: Option<FieldEdit<PlaybookBinding>>,
    pub value_gap_magnitude: Option<FieldEdit<ValueGapMagnitude>>,
    pub nick_weight: Option<FieldEdit<f64>>,
    pub dependency_readiness: Option<FieldEdit<DependencyReadiness>>,
    pub wake_condition: Option<FieldEdit<WakeCondition>>,
    pub superseded_by: Option<FieldEdit<String>>,
}

impl ShapeEditBody {
    fn is_empty(&self) -> bool {
        self.action_class.is_none()
            && self.description.is_none()
            && self.effort_class.is_none()
            && self.playbook_binding.is_none()
            && self.value_gap_magnitude.is_none()
            && self.nick_weight.is_none()
            && self.dependency_readiness.is_none()
            && self.wake_condition.is_none()
            && self.superseded_by.is_none()
    }
}

/// One proposed DECIDE position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposedPosition {
    pub backlog_item_id: String,
    pub position: u32,
}

/// The nine closed post-genesis operations. None of them changes lifecycle
/// state; a state move is only ever the governed `prepare_backlog_transition`
/// path.
#[derive(Debug, Clone, PartialEq)]
pub enum BacklogMutation {
    ShapeEdit {
        bi_id: String,
        body: ShapeEditBody,
    },
    RecomputeRank {
        business_node_id: String,
    },
    StampExecutionBinding {
        bi_id: String,
        execution_binding: ExecutionBinding,
        outcome_binding: OutcomeBindingDecl,
    },
    RecordOutcomeSignoff {
        bi_id: String,
        approver: String,
    },
    ProposeReshuffle {
        proposed: Vec<ProposedPosition>,
    },
    CommitReshuffle {
        proposal_id: String,
        approver: String,
    },
    RejectReshuffle {
        proposal_id: String,
        approver: String,
    },
    VetoAgeOut {
        bi_id: String,
        approver: String,
    },
    LiftAgeOutVeto {
        bi_id: String,
        approver: String,
    },
}

impl BacklogMutation {
    /// The closed operation token used by the journal manifest, the RPC
    /// response, and the fixed role table.
    pub fn operation(&self) -> &'static str {
        match self {
            BacklogMutation::ShapeEdit { .. } => "shape_edit",
            BacklogMutation::RecomputeRank { .. } => "recompute_rank",
            BacklogMutation::StampExecutionBinding { .. } => "stamp_execution_binding",
            BacklogMutation::RecordOutcomeSignoff { .. } => "record_outcome_signoff",
            BacklogMutation::ProposeReshuffle { .. } => "propose_reshuffle",
            BacklogMutation::CommitReshuffle { .. } => "commit_reshuffle",
            BacklogMutation::RejectReshuffle { .. } => "reject_reshuffle",
            BacklogMutation::VetoAgeOut { .. } => "veto_age_out",
            BacklogMutation::LiftAgeOutVeto { .. } => "lift_age_out_veto",
        }
    }

    /// The frozen fixed role for every operation whose authority is a single
    /// role. `shape_edit` is the one operation with per-field-group authority,
    /// so it returns `None` and is checked field by field.
    pub fn fixed_role(&self) -> Option<DriverRole> {
        match self {
            BacklogMutation::ShapeEdit { .. } => None,
            BacklogMutation::RecomputeRank { .. } => Some(DriverRole::OrganLoop),
            BacklogMutation::StampExecutionBinding { .. } => Some(DriverRole::TrackDriver),
            BacklogMutation::ProposeReshuffle { .. } => Some(DriverRole::Orchestrator),
            BacklogMutation::RecordOutcomeSignoff { .. }
            | BacklogMutation::CommitReshuffle { .. }
            | BacklogMutation::RejectReshuffle { .. }
            | BacklogMutation::VetoAgeOut { .. }
            | BacklogMutation::LiftAgeOutVeto { .. } => Some(DriverRole::NickShape),
        }
    }
}

/// One strictly loaded item plus its strict history, as the pure mutation
/// service sees it. The storage layer supplies these; nothing here reads bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct BacklogItemView {
    pub item: BacklogItem,
    pub history: Vec<HistoryEntry>,
}

/// One prepared history append. `seq` is allocated by the storage layer against
/// the strict live tail, never by a caller.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedHistoryAppend {
    pub bi_id: String,
    pub entry: HistoryEntry,
}

/// One prepared full-item rewrite.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedItemWrite {
    pub bi_id: String,
    pub item: BacklogItem,
}

/// The consume-once prepared mutation. It owns every ordered effect the journal
/// needs, so nothing about the write is recomputed after validation.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedBacklogMutation {
    pub operation: &'static str,
    pub actor: ActorIdentity,
    pub role: DriverRole,
    pub at: String,
    /// Item rewrites in byte-ID order. Empty when the operation writes no body.
    pub items: Vec<PreparedItemWrite>,
    /// History appends in byte-ID order; for DECIDE commit every authorization
    /// entry precedes every position byte (enforced by `decide_commit`).
    pub appends: Vec<PreparedHistoryAppend>,
    /// Every touched item id, byte order.
    pub affected: Vec<String>,
    /// Valid candidates returned as the explicit pre-triage partition (§1.12).
    pub unranked: Vec<String>,
    pub proposal_id: Option<String>,
    /// DECIDE commit is the one frozen ordering special case.
    pub decide_commit: bool,
}

/// The closed pending rank intent a `shape_edit` stages in its fixed history
/// payload. It is never a partial `Rank` and never lands in `item.yaml`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingRankIntent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_gap_magnitude: Option<ValueGapMagnitude>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nick_weight: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency_readiness: Option<DependencyReadiness>,
    /// An effort edit that would stale an existing rank is pending in the same
    /// payload (§1.9) rather than silently invalidating the committed rank.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort_class: Option<EffortClass>,
}

impl PendingRankIntent {
    pub fn is_empty(&self) -> bool {
        self.value_gap_magnitude.is_none()
            && self.nick_weight.is_none()
            && self.dependency_readiness.is_none()
            && self.effort_class.is_none()
    }

    fn absorb(&mut self, later: &PendingRankIntent) {
        if later.value_gap_magnitude.is_some() {
            self.value_gap_magnitude = later.value_gap_magnitude.clone();
        }
        if later.nick_weight.is_some() {
            self.nick_weight = later.nick_weight;
        }
        if later.dependency_readiness.is_some() {
            self.dependency_readiness = later.dependency_readiness.clone();
        }
        if later.effort_class.is_some() {
            self.effort_class = later.effort_class;
        }
    }
}

fn semantic(detail: impl Into<String>) -> BacklogItemError {
    BacklogItemError::SemanticViolation { detail: detail.into() }
}

fn wrong_role(operation: &str, role: DriverRole, detail: &str) -> BacklogItemError {
    semantic(format!(
        "role `{}` may not drive `{operation}`: {detail}",
        role.as_str()
    ))
}

/// The last `rank_recomputed` sequence that NAMES consumed sequences. Every
/// `shape_edit` after it with a nonempty pending intent is unresolved (§1.9).
fn last_consuming_recompute_seq(history: &[HistoryEntry]) -> Option<u64> {
    history
        .iter()
        .filter(|e| e.kind == HistoryKind::RankRecomputed)
        .filter(|e| {
            e.payload
                .as_ref()
                .and_then(|p| p.get(CONSUMED_KEY))
                .is_some()
        })
        .map(|e| e.seq)
        .max()
}

/// Decode one `shape_edit`'s staged pending rank intent.
///
/// A MALFORMED `pending` payload is a hard error, never a silent `None`. If it
/// decayed to `None` the edit would look resolved, `unresolved_rank_shape_seqs`
/// would report nothing outstanding, and a rank-sensitive transition would
/// proceed on the STALE committed rank instead of refusing (§1.9 / §1.12).
fn pending_of(entry: &HistoryEntry) -> Result<Option<PendingRankIntent>, BacklogItemError> {
    let raw = match entry.payload.as_ref().and_then(|p| p.get(PENDING_KEY)) {
        Some(raw) => raw.clone(),
        None => return Ok(None),
    };
    let parsed = serde_yaml::from_value::<PendingRankIntent>(raw).map_err(|e| {
        semantic(format!(
            "history entry seq {} carries a malformed `{PENDING_KEY}` payload: {e} \
             [pending_rank_intent_strict]",
            entry.seq
        ))
    })?;
    Ok(if parsed.is_empty() { None } else { Some(parsed) })
}

/// The unresolved rank-affecting `shape_edit` sequences, oldest first.
pub fn unresolved_rank_shape_seqs(
    history: &[HistoryEntry],
) -> Result<Vec<u64>, BacklogItemError> {
    let cutoff = last_consuming_recompute_seq(history);
    let mut out: Vec<u64> = Vec::new();
    for entry in history
        .iter()
        .filter(|e| e.kind == HistoryKind::ShapeEdit)
        .filter(|e| cutoff.map(|c| e.seq > c).unwrap_or(true))
    {
        if pending_of(entry)?.is_some() {
            out.push(entry.seq);
        }
    }
    out.sort_unstable();
    Ok(out)
}

/// Merge every unresolved pending intent in sequence order (§1.9).
fn resolved_pending(history: &[HistoryEntry]) -> Result<PendingRankIntent, BacklogItemError> {
    let cutoff = last_consuming_recompute_seq(history);
    let mut entries: Vec<&HistoryEntry> = history
        .iter()
        .filter(|e| e.kind == HistoryKind::ShapeEdit)
        .filter(|e| cutoff.map(|c| e.seq > c).unwrap_or(true))
        .collect();
    entries.sort_by_key(|e| e.seq);
    let mut merged = PendingRankIntent::default();
    for entry in entries {
        if let Some(pending) = pending_of(entry)? {
            merged.absorb(&pending);
        }
    }
    Ok(merged)
}

fn history_entry(
    kind: HistoryKind,
    actor: &ActorIdentity,
    role: DriverRole,
    at: &str,
    payload: Option<serde_yaml::Value>,
) -> HistoryEntry {
    HistoryEntry {
        // Allocated by the storage layer against the strict live tail.
        seq: 0,
        actor: actor.name.clone(),
        role,
        at: at.to_string(),
        kind,
        from_state: None,
        to_state: None,
        payload,
        note: None,
    }
}

fn mapping(pairs: Vec<(&str, serde_yaml::Value)>) -> serde_yaml::Value {
    let mut map = serde_yaml::Mapping::new();
    for (key, value) in pairs {
        map.insert(serde_yaml::Value::String(key.to_string()), value);
    }
    serde_yaml::Value::Mapping(map)
}

fn to_yaml<T: Serialize>(value: &T) -> Result<serde_yaml::Value, BacklogItemError> {
    serde_yaml::to_value(value).map_err(|e| semantic(format!("serialize payload: {e}")))
}

// ── shape_edit ──────────────────────────────────────────────────────────────

/// Field-group authority (§Task 7). Enforced independently from the later
/// transition role, so a SHAPE caller can never widen its own edge authority.
fn authorize_shape_group(
    role: DriverRole,
    state: State,
    body: &ShapeEditBody,
) -> Result<(), BacklogItemError> {
    let shape_trio = [DriverRole::NickShape, DriverRole::OrganLoop, DriverRole::Orchestrator];
    let touches_body = body.action_class.is_some()
        || body.description.is_some()
        || body.effort_class.is_some()
        || body.playbook_binding.is_some()
        || body.value_gap_magnitude.is_some()
        || body.nick_weight.is_some();
    if touches_body && !shape_trio.contains(&role) {
        return Err(wrong_role(
            "shape_edit",
            role,
            "action/description/effort/playbook/value-gap/nick-weight admit only \
             {nick_shape, organ_loop, orchestrator}",
        ));
    }
    if let Some(edit) = &body.dependency_readiness {
        let ready = matches!(
            edit,
            FieldEdit::Set(DependencyReadiness { status: DependencyStatus::Ready, .. })
        );
        let allowed: &[DriverRole] = if ready {
            &[DriverRole::OrganLoop, DriverRole::Orchestrator]
        } else {
            &[DriverRole::OrganLoop, DriverRole::Orchestrator, DriverRole::NickShape]
        };
        if !allowed.contains(&role) {
            return Err(wrong_role(
                "shape_edit",
                role,
                "dependency_readiness=ready admits only {organ_loop, orchestrator}",
            ));
        }
    }
    if body.wake_condition.is_some() {
        let allowed: &[DriverRole] = match state {
            State::Candidate | State::Ready => &[DriverRole::NickShape, DriverRole::OrganLoop],
            State::InFlight => &[DriverRole::NickShape, DriverRole::TrackDriver],
            other => {
                return Err(semantic(format!(
                    "a wake_condition edit is not admissible at state `{}`",
                    other.as_str()
                )))
            }
        };
        if !allowed.contains(&role) {
            return Err(wrong_role(
                "shape_edit",
                role,
                "wake authority is source-specific and the orchestrator never writes wake",
            ));
        }
    }
    if body.superseded_by.is_some() {
        if !matches!(
            state,
            State::Candidate | State::Ready | State::InFlight | State::Parked
        ) {
            return Err(semantic(format!(
                "a superseded_by edit is not admissible at state `{}`",
                state.as_str()
            )));
        }
        if !matches!(role, DriverRole::NickShape | DriverRole::Orchestrator) {
            return Err(wrong_role(
                "shape_edit",
                role,
                "superseded_by admits only {nick_shape, orchestrator}; organ-loop and \
                 track-driver never write it",
            ));
        }
    }
    Ok(())
}

/// Stage a parked or superseded exit intent, replacing incompatible staged data.
fn stage_exit_intent(
    item: &mut BacklogItem,
    wake: Option<&FieldEdit<WakeCondition>>,
    superseded: Option<&FieldEdit<String>>,
) {
    if let Some(edit) = wake {
        let value = match edit {
            FieldEdit::Set(w) => Some(w.clone()),
            FieldEdit::Clear => None,
        };
        item.exit = value.map(|w| Exit {
            kind: ExitKind::Parked,
            wake_condition: Some(w),
            superseded_by: None,
            aged_out_reason: None,
        });
    }
    if let Some(edit) = superseded {
        let value = match edit {
            FieldEdit::Set(target) => Some(target.clone()),
            FieldEdit::Clear => None,
        };
        item.exit = value.map(|target| Exit {
            kind: ExitKind::Superseded,
            wake_condition: None,
            superseded_by: Some(target),
            aged_out_reason: None,
        });
    }
}

fn prepare_shape_edit(
    view: &BacklogItemView,
    body: &ShapeEditBody,
    role: DriverRole,
    actor: &ActorIdentity,
    at: &str,
) -> Result<PreparedBacklogMutation, BacklogItemError> {
    if body.is_empty() {
        return Err(semantic("a shape_edit must carry at least one change"));
    }
    if view.item.state.is_terminal() {
        return Err(BacklogItemError::TerminalNoOutgoing {
            from: view.item.state.as_str().to_string(),
        });
    }
    authorize_shape_group(role, view.item.state, body)?;

    let mut item = view.item.clone();
    let mut pending = PendingRankIntent::default();
    let mut applied: Vec<serde_yaml::Value> = Vec::new();
    let mut note = |field: &str, applied: &mut Vec<serde_yaml::Value>| {
        applied.push(serde_yaml::Value::String(field.to_string()));
    };

    if let Some(action) = body.action_class {
        item.action_class = action;
        note("action_class", &mut applied);
    }
    if let Some(edit) = &body.description {
        item.description = match edit {
            FieldEdit::Set(v) => Some(v.clone()),
            FieldEdit::Clear => None,
        };
        note("description", &mut applied);
    }
    if let Some(edit) = &body.playbook_binding {
        item.playbook_binding = match edit {
            FieldEdit::Set(v) => Some(v.clone()),
            FieldEdit::Clear => None,
        };
        note("playbook_binding", &mut applied);
    }
    if let Some(edit) = &body.effort_class {
        match edit {
            FieldEdit::Set(v) if item.rank.is_none() => {
                // No committed rank to stale: the top-level edit lands now.
                item.effort_class = Some(*v);
                note("effort_class", &mut applied);
            }
            FieldEdit::Set(v) => {
                // A committed rank exists, so the effort edit is PENDING until
                // re-rank can update the top level and its mirror together.
                pending.effort_class = Some(*v);
            }
            FieldEdit::Clear if item.rank.is_none() => {
                item.effort_class = None;
                note("effort_class", &mut applied);
            }
            FieldEdit::Clear => {
                return Err(semantic(
                    "clearing effort_class would break the committed rank's effort mirror; \
                     re-rank first",
                ))
            }
        }
    }
    // Rank INPUT edits never write `item.rank`: they stage the closed pending
    // intent that `recompute_rank` consumes (§1.9).
    if let Some(edit) = &body.value_gap_magnitude {
        match edit {
            FieldEdit::Set(v) => pending.value_gap_magnitude = Some(v.clone()),
            FieldEdit::Clear => {
                return Err(semantic("a value_gap_magnitude rank input cannot be cleared"))
            }
        }
    }
    if let Some(edit) = &body.nick_weight {
        match edit {
            FieldEdit::Set(v) => pending.nick_weight = Some(*v),
            FieldEdit::Clear => {
                return Err(semantic("a nick_weight rank input cannot be cleared"))
            }
        }
    }
    if let Some(edit) = &body.dependency_readiness {
        match edit {
            FieldEdit::Set(v) => pending.dependency_readiness = Some(v.clone()),
            FieldEdit::Clear => {
                return Err(semantic("a dependency_readiness rank input cannot be cleared"))
            }
        }
    }
    stage_exit_intent(&mut item, body.wake_condition.as_ref(), body.superseded_by.as_ref());
    if body.wake_condition.is_some() {
        note("wake_condition", &mut applied);
    }
    if body.superseded_by.is_some() {
        note("superseded_by", &mut applied);
    }

    if let Some(pending_value) = pending.value_gap_magnitude.as_ref() {
        if !pending_value.magnitude.is_finite() {
            return Err(semantic(
                "rank.inputs.value_gap_magnitude.magnitude must be finite [numeric_finite]",
            ));
        }
        validate_value_gap_kind(&pending_value.r#ref, "rank.inputs.value_gap_magnitude.ref")?;
    }
    if let Some(weight) = pending.nick_weight {
        if !weight.is_finite() {
            return Err(semantic("rank.inputs.nick_weight must be finite [numeric_finite]"));
        }
    }
    if let Some(wake) = item.exit.as_ref().and_then(|e| e.wake_condition.as_ref()) {
        validate_wake_shape(wake)?;
    }

    // The resulting CURRENT-state body must be valid; lifecycle state is
    // unchanged, so the state-local matrix is re-checked at the same state.
    //
    // §1.8: source-state exit intent staged by `shape_edit` is TOLERATED. A
    // parked item may legitimately stage a superseded intent for a later #15,
    // so the state-local exit requirement is checked against the exit the item
    // already had, never against the staged target intent. The staged value is
    // still shape-validated above; it is simply not asked to satisfy the state
    // it is leaving.
    validate_item_semantics(&item)?;
    let staged_intent = body.wake_condition.is_some() || body.superseded_by.is_some();
    let mut state_local = item.clone();
    if staged_intent {
        state_local.exit = view.item.exit.clone();
    }
    validate_required_by_state(&state_local, item.state)?;
    if item.state != view.item.state {
        return Err(semantic("a shape_edit may never change lifecycle state"));
    }

    let mut pairs: Vec<(&str, serde_yaml::Value)> =
        vec![("fields", serde_yaml::Value::Sequence(applied))];
    if !pending.is_empty() {
        pairs.push((PENDING_KEY, to_yaml(&pending)?));
    }
    let entry = history_entry(HistoryKind::ShapeEdit, actor, role, at, Some(mapping(pairs)));
    let id = item.backlog_item_id.clone();
    Ok(PreparedBacklogMutation {
        operation: "shape_edit",
        actor: actor.clone(),
        role,
        at: at.to_string(),
        items: vec![PreparedItemWrite { bi_id: id.clone(), item }],
        appends: vec![PreparedHistoryAppend { bi_id: id.clone(), entry }],
        affected: vec![id],
        unranked: Vec::new(),
        proposal_id: None,
        decide_commit: false,
    })
}

// ── recompute_rank ──────────────────────────────────────────────────────────

/// The complete resolved input set for one re-rank candidate.
struct ResolvedInputs {
    value_gap: ValueGapMagnitude,
    nick_weight: f64,
    dependency: DependencyReadiness,
    effort: EffortClass,
    consumed: Vec<u64>,
}

/// Resolve one item's re-rank inputs from its committed rank plus its
/// unresolved pending edits. `Ok(None)` means the item is a valid pre-triage
/// candidate: it lacks a complete first-input/effort set and its pending edits
/// are NOT consumed (§1.12).
fn resolve_rank_inputs(view: &BacklogItemView) -> Result<Option<ResolvedInputs>, BacklogItemError> {
    let pending = resolved_pending(&view.history)?;
    let consumed = unresolved_rank_shape_seqs(&view.history)?;
    let committed = view.item.rank.as_ref().map(|r| &r.inputs);

    let value_gap = pending
        .value_gap_magnitude
        .clone()
        .or_else(|| committed.map(|i| i.value_gap_magnitude.clone()));
    let nick_weight = pending.nick_weight.or_else(|| committed.map(|i| i.nick_weight));
    let dependency = pending
        .dependency_readiness
        .clone()
        .or_else(|| committed.map(|i| i.dependency_readiness.clone()));
    let effort = pending
        .effort_class
        .or(view.item.effort_class)
        .or_else(|| committed.map(|i| i.effort_class));

    // §1.10 authority: no orchestrator or Nick role may ever be RECORDED as
    // writing a per-organ position. A ledger that already contains such an
    // entry is an authority leak, and the whole re-rank refuses rather than
    // building on it.
    if let Some(forged) = view
        .history
        .iter()
        .filter(|e| e.kind == HistoryKind::RankRecomputed)
        .find(|e| !matches!(e.role, DriverRole::OrganLoop | DriverRole::EngineAuto))
    {
        return Err(semantic(format!(
            "role `{}` is recorded as writing a per-organ position on `{}` at seq {}; only              organ_loop and the private derived engine_auto authority may",
            forged.role.as_str(),
            view.item.backlog_item_id,
            forged.seq
        )));
    }

    // An already-committed rank must be coherent before anything is recomputed:
    // an incomplete committed rank or an unaccounted effort-mirror mismatch
    // rejects the WHOLE re-rank (§1.9).
    if let Some(rank) = &view.item.rank {
        if rank.position < 1 || rank.explanation.trim().is_empty() {
            return Err(semantic(format!(
                "incomplete rank committed on `{}`: rank is all-or-nothing (position >= 1 \
                 plus an explanation)",
                view.item.backlog_item_id
            )));
        }
        if pending.effort_class.is_none() && view.item.effort_class != Some(rank.inputs.effort_class)
        {
            return Err(semantic(format!(
                "unaccounted effort_class mirror disagreement on `{}` \
                 [effort_class_mirror_equal]",
                view.item.backlog_item_id
            )));
        }
    }

    match (value_gap, nick_weight, dependency, effort) {
        (Some(value_gap), Some(nick_weight), Some(dependency), Some(effort)) => {
            Ok(Some(ResolvedInputs { value_gap, nick_weight, dependency, effort, consumed }))
        }
        _ if view.item.rank.is_some() => Err(semantic(format!(
            "incomplete rank inputs for the already-ranked item `{}`",
            view.item.backlog_item_id
        ))),
        _ => Ok(None),
    }
}

/// Re-rank one organ. Direct callers are `organ_loop`-only; `evaluate_backlog`
/// reuses this with the private `engine_auto` authority.
fn prepare_recompute_rank(
    organ: &str,
    universe: &[BacklogItemView],
    policy: &BacklogPolicy,
    role: DriverRole,
    actor: &ActorIdentity,
    at: &str,
) -> Result<PreparedBacklogMutation, BacklogItemError> {
    validate_business_node_id(organ)?;
    if !matches!(role, DriverRole::OrganLoop | DriverRole::EngineAuto) {
        return Err(wrong_role(
            "recompute_rank",
            role,
            "direct re-rank is organ_loop-only; no orchestrator or Nick role writes per-organ \
             positions",
        ));
    }
    let (items, unranked) = recompute_organ(organ, universe, policy)?;
    let mut appends = Vec::new();
    let mut writes = Vec::new();
    let mut affected = Vec::new();
    for (view, item, consumed) in items {
        let rank = item
            .rank
            .as_ref()
            .ok_or_else(|| semantic("a re-ranked survivor must carry a complete rank"))?;
        let payload = mapping(vec![
            ("position", serde_yaml::Value::Number(rank.position.into())),
            (
                "explanation",
                serde_yaml::Value::String(rank.explanation.clone()),
            ),
            (
                CONSUMED_KEY,
                serde_yaml::Value::Sequence(
                    consumed
                        .iter()
                        .map(|s| serde_yaml::Value::Number((*s).into()))
                        .collect(),
                ),
            ),
        ]);
        appends.push(PreparedHistoryAppend {
            bi_id: view.item.backlog_item_id.clone(),
            entry: history_entry(HistoryKind::RankRecomputed, actor, role, at, Some(payload)),
        });
        affected.push(item.backlog_item_id.clone());
        writes.push(PreparedItemWrite { bi_id: item.backlog_item_id.clone(), item });
    }
    Ok(PreparedBacklogMutation {
        operation: "recompute_rank",
        actor: actor.clone(),
        role,
        at: at.to_string(),
        items: writes,
        appends,
        affected,
        unranked,
        proposal_id: None,
        decide_commit: false,
    })
}

/// The shared organ re-rank kernel: resolve every eligible item's inputs, apply
/// pending effort, advance cycle age, and materialize the complete 1-based
/// per-organ positions. Returns the eligible survivors plus the explicit
/// pre-triage partition.
#[allow(clippy::type_complexity)]
fn recompute_organ<'a>(
    organ: &str,
    universe: &'a [BacklogItemView],
    policy: &BacklogPolicy,
) -> Result<(Vec<(&'a BacklogItemView, BacklogItem, Vec<u64>)>, Vec<String>), BacklogItemError> {
    let mut in_organ: Vec<&BacklogItemView> = universe
        .iter()
        .filter(|v| v.item.business_node_id == organ)
        .filter(|v| matches!(v.item.state, State::Candidate | State::Ready))
        .collect();
    in_organ.sort_by(|a, b| a.item.backlog_item_id.cmp(&b.item.backlog_item_id));

    let mut eligible: Vec<(&BacklogItemView, BacklogItem, Vec<u64>)> = Vec::new();
    let mut unranked: Vec<String> = Vec::new();
    for view in in_organ {
        match resolve_rank_inputs(view)? {
            None => unranked.push(view.item.backlog_item_id.clone()),
            Some(resolved) => {
                let mut item = view.item.clone();
                item.effort_class = Some(resolved.effort);
                let prior_age = view.item.rank.as_ref().map(|r| r.inputs.age).unwrap_or(0);
                item.rank = Some(Rank {
                    // Rewritten by the materializer below before validation.
                    position: 1,
                    inputs: RankInputs {
                        value_gap_magnitude: resolved.value_gap.clone(),
                        nick_weight: resolved.nick_weight,
                        dependency_readiness: resolved.dependency.clone(),
                        age: prior_age.saturating_add(1),
                        effort_class: resolved.effort,
                    },
                    explanation: String::new(),
                });
                eligible.push((view, item, resolved.consumed));
            }
        }
    }

    let hypothetical: Vec<BacklogItem> = eligible.iter().map(|(_, item, _)| item.clone()).collect();
    let positions = materialize_rank(&hypothetical, policy);
    for (bi_id, position) in positions {
        if let Some((_, item, _)) = eligible
            .iter_mut()
            .find(|(_, item, _)| item.backlog_item_id == bi_id)
        {
            if let Some(rank) = item.rank.as_mut() {
                rank.position = position;
                rank.explanation = format!("position {position} under the resolved comparator");
            }
        }
    }
    for (_, item, _) in &eligible {
        validate_item_semantics(item)?;
        validate_required_by_state(item, item.state)?;
    }
    unranked.sort();
    Ok((eligible, unranked))
}

// ── stamp_execution_binding / record_outcome_signoff ────────────────────────

fn prepare_stamp(
    view: &BacklogItemView,
    execution: &ExecutionBinding,
    declared: &OutcomeBindingDecl,
    role: DriverRole,
    actor: &ActorIdentity,
    at: &str,
) -> Result<PreparedBacklogMutation, BacklogItemError> {
    if role != DriverRole::TrackDriver {
        return Err(wrong_role("stamp_execution_binding", role, "stamp is track_driver-only"));
    }
    if view.item.state != State::Ready {
        return Err(semantic(format!(
            "stamp_execution_binding is accepted only on `ready` as the #5 precondition, not \
             `{}`",
            view.item.state.as_str()
        )));
    }
    validate_run_id(&execution.run_id)?;
    validate_initial_reading_status(declared.reading_status)?;
    if declared.success_measure_id.as_deref().map(str::trim).unwrap_or("").is_empty() {
        return Err(semantic(
            "a stamp must declare outcome_binding.success_measure_id",
        ));
    }
    let mut item = view.item.clone();
    item.execution_binding = Some(execution.clone());
    item.outcome_binding = Some(OutcomeBinding {
        success_measure_id: declared.success_measure_id.clone(),
        tree_node: declared.tree_node.clone(),
        reading_status: declared.reading_status,
        nick_signoff: false,
    });
    validate_item_semantics(&item)?;
    validate_required_by_state(&item, item.state)?;

    let payload = mapping(vec![
        ("run_id", serde_yaml::Value::String(execution.run_id.clone())),
        (
            "reading_status",
            to_yaml(&declared.reading_status)?,
        ),
    ]);
    let id = item.backlog_item_id.clone();
    Ok(PreparedBacklogMutation {
        operation: "stamp_execution_binding",
        actor: actor.clone(),
        role,
        at: at.to_string(),
        items: vec![PreparedItemWrite { bi_id: id.clone(), item }],
        appends: vec![PreparedHistoryAppend {
            bi_id: id.clone(),
            entry: history_entry(HistoryKind::BindingStamped, actor, role, at, Some(payload)),
        }],
        affected: vec![id],
        unranked: Vec::new(),
        proposal_id: None,
        decide_commit: false,
    })
}

fn prepare_signoff(
    view: &BacklogItemView,
    approver: &str,
    role: DriverRole,
    actor: &ActorIdentity,
    at: &str,
) -> Result<PreparedBacklogMutation, BacklogItemError> {
    if role != DriverRole::NickShape {
        return Err(wrong_role(
            "record_outcome_signoff",
            role,
            "only nick_shape may sign off an unmeasurable outcome (NICK-GATE)",
        ));
    }
    if view.item.state != State::InFlight {
        return Err(semantic(format!(
            "record_outcome_signoff is in_flight-only, not `{}`",
            view.item.state.as_str()
        )));
    }
    if approver.trim().is_empty() {
        return Err(semantic("record_outcome_signoff requires a nonempty approver"));
    }
    let mut item = view.item.clone();
    let binding = item
        .outcome_binding
        .as_mut()
        .ok_or_else(|| semantic("no outcome_binding is bound, so it cannot be signed off"))?;
    binding.reading_status = ReadingStatus::UnmeasurableSigned;
    binding.nick_signoff = true;
    validate_item_semantics(&item)?;
    validate_required_by_state(&item, item.state)?;

    let payload = mapping(vec![("approver", serde_yaml::Value::String(approver.to_string()))]);
    let id = item.backlog_item_id.clone();
    Ok(PreparedBacklogMutation {
        operation: "record_outcome_signoff",
        actor: actor.clone(),
        role,
        at: at.to_string(),
        items: vec![PreparedItemWrite { bi_id: id.clone(), item }],
        appends: vec![PreparedHistoryAppend {
            bi_id: id.clone(),
            entry: history_entry(HistoryKind::Signoff, actor, role, at, Some(payload)),
        }],
        affected: vec![id],
        unranked: Vec::new(),
        proposal_id: None,
        decide_commit: false,
    })
}

// ── veto_age_out / lift_age_out_veto ────────────────────────────────────────

/// Every set-and-not-lifted veto sequence, oldest first.
fn standing_veto_seqs(history: &[HistoryEntry]) -> Vec<u64> {
    let mut standing: Vec<u64> = Vec::new();
    for entry in history.iter().filter(|e| e.kind == HistoryKind::Veto) {
        let action = entry
            .payload
            .as_ref()
            .and_then(|p| p.get("veto_action"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        match action {
            "set" => standing.push(entry.seq),
            "lift" => {
                if let Some(lifted) = entry
                    .payload
                    .as_ref()
                    .and_then(|p| p.get("lifts_seq"))
                    .and_then(|v| v.as_u64())
                {
                    standing.retain(|s| *s != lifted);
                }
            }
            _ => {}
        }
    }
    standing
}

fn prepare_veto(
    view: &BacklogItemView,
    approver: &str,
    lift: bool,
    role: DriverRole,
    actor: &ActorIdentity,
    at: &str,
) -> Result<PreparedBacklogMutation, BacklogItemError> {
    let operation = if lift { "lift_age_out_veto" } else { "veto_age_out" };
    if role != DriverRole::NickShape {
        return Err(wrong_role(operation, role, "the age-out veto is nick_shape-only (NICK-GATE)"));
    }
    if approver.trim().is_empty() {
        return Err(semantic(format!("{operation} requires a nonempty approver")));
    }
    let standing = standing_veto_seqs(&view.history);
    let payload = if lift {
        match standing.as_slice() {
            [seq] => mapping(vec![
                ("veto_action", serde_yaml::Value::String("lift".to_string())),
                ("lifts_seq", serde_yaml::Value::Number((*seq).into())),
                ("approver", serde_yaml::Value::String(approver.to_string())),
            ]),
            [] => return Err(semantic("no unresolved age-out veto exists to lift")),
            _ => {
                return Err(semantic(
                    "more than one unresolved age-out veto exists; the set is not unique",
                ))
            }
        }
    } else {
        if !matches!(view.item.state, State::Candidate | State::Ready | State::Parked) {
            return Err(semantic(format!(
                "an age-out veto applies only in candidate/ready/parked, not `{}`",
                view.item.state.as_str()
            )));
        }
        if !standing.is_empty() {
            return Err(semantic("an age-out veto is already unresolved on this item"));
        }
        mapping(vec![
            ("veto_action", serde_yaml::Value::String("set".to_string())),
            ("approver", serde_yaml::Value::String(approver.to_string())),
        ])
    };
    let id = view.item.backlog_item_id.clone();
    Ok(PreparedBacklogMutation {
        operation: if lift { "lift_age_out_veto" } else { "veto_age_out" },
        actor: actor.clone(),
        role,
        at: at.to_string(),
        // A veto writes no body: it is a pure history producer.
        items: Vec::new(),
        appends: vec![PreparedHistoryAppend {
            bi_id: id.clone(),
            entry: history_entry(HistoryKind::Veto, actor, role, at, Some(payload)),
        }],
        affected: vec![id],
        unranked: Vec::new(),
        proposal_id: None,
        decide_commit: false,
    })
}

// ── DECIDE: propose / commit / reject ───────────────────────────────────────

/// One unresolved DECIDE proposal recovered from history.
struct OpenProposal {
    positions: Vec<(String, u32)>,
    organ_context: String,
}

fn proposal_entries<'a>(
    universe: &'a [BacklogItemView],
    proposal_id: &str,
) -> Vec<(&'a BacklogItemView, &'a HistoryEntry)> {
    let mut out: Vec<(&BacklogItemView, &HistoryEntry)> = Vec::new();
    for view in universe {
        for entry in &view.history {
            let matches_id = entry
                .payload
                .as_ref()
                .and_then(|p| p.get(PROPOSAL_KEY))
                .and_then(|v| v.as_str())
                == Some(proposal_id);
            if matches_id && entry.kind == HistoryKind::RankProposed {
                out.push((view, entry));
            }
        }
    }
    out.sort_by(|a, b| a.0.item.backlog_item_id.cmp(&b.0.item.backlog_item_id));
    out
}

fn proposal_resolved(universe: &[BacklogItemView], proposal_id: &str) -> bool {
    universe.iter().any(|view| {
        view.history.iter().any(|entry| {
            matches!(entry.kind, HistoryKind::RankCommitted | HistoryKind::RankRejected)
                && entry
                    .payload
                    .as_ref()
                    .and_then(|p| p.get(PROPOSAL_KEY))
                    .and_then(|v| v.as_str())
                    == Some(proposal_id)
        })
    })
}

/// The strict organ-context fingerprint a proposal is bound to: the ranked
/// membership plus each member's committed position, in byte-ID order.
fn organ_context_fingerprint(organ: &str, universe: &[BacklogItemView]) -> String {
    let mut rows: Vec<String> = universe
        .iter()
        .filter(|v| v.item.business_node_id == organ)
        .filter(|v| matches!(v.item.state, State::Candidate | State::Ready))
        .filter_map(|v| {
            v.item
                .rank
                .as_ref()
                .map(|r| format!("{}:{}", v.item.backlog_item_id, r.position))
        })
        .collect();
    rows.sort();
    rows.join(",")
}

fn mint_proposal_id() -> String {
    format!("rp_{}", mint_backlog_item_id().trim_start_matches("bi_"))
}

fn prepare_propose_reshuffle(
    proposed: &[ProposedPosition],
    universe: &[BacklogItemView],
    role: DriverRole,
    actor: &ActorIdentity,
    at: &str,
) -> Result<PreparedBacklogMutation, BacklogItemError> {
    if role != DriverRole::Orchestrator {
        return Err(wrong_role(
            "propose_reshuffle",
            role,
            "DECIDE proposals are orchestrator-only and only Nick may commit them",
        ));
    }
    if proposed.is_empty() {
        return Err(semantic("a reshuffle proposal must name at least one item"));
    }
    let mut ids: Vec<&str> = proposed.iter().map(|p| p.backlog_item_id.as_str()).collect();
    ids.sort_unstable();
    let unique = ids.len();
    ids.dedup();
    if ids.len() != unique {
        return Err(semantic("a reshuffle proposal must name unique item ids"));
    }

    let mut organ: Option<String> = None;
    let mut views: Vec<&BacklogItemView> = Vec::new();
    for entry in proposed {
        let view = universe
            .iter()
            .find(|v| v.item.backlog_item_id == entry.backlog_item_id)
            .ok_or_else(|| {
                semantic(format!("proposed item `{}` does not exist", entry.backlog_item_id))
            })?;
        if !matches!(view.item.state, State::Candidate | State::Ready) {
            return Err(semantic(format!(
                "a reshuffle proposal covers only ranked candidate/ready items, not `{}`",
                view.item.state.as_str()
            )));
        }
        if view.item.rank.is_none() {
            return Err(semantic(format!(
                "proposed item `{}` carries no complete rank",
                entry.backlog_item_id
            )));
        }
        match &organ {
            None => organ = Some(view.item.business_node_id.clone()),
            Some(existing) if existing == &view.item.business_node_id => {}
            Some(_) => {
                return Err(semantic(
                    "a reshuffle proposal may not span organs: positions are per-organ",
                ))
            }
        }
        views.push(view);
    }
    let organ = organ.expect("a nonempty proposal names an organ");

    // The proposal must be a COMPLETE per-organ position set: every ranked
    // member of the authoritative organ, with contiguous 1..n positions.
    let members: Vec<&str> = universe
        .iter()
        .filter(|v| v.item.business_node_id == organ)
        .filter(|v| matches!(v.item.state, State::Candidate | State::Ready))
        .filter(|v| v.item.rank.is_some())
        .map(|v| v.item.backlog_item_id.as_str())
        .collect();
    let mut member_ids: Vec<&str> = members.clone();
    member_ids.sort_unstable();
    let mut named: Vec<&str> = proposed.iter().map(|p| p.backlog_item_id.as_str()).collect();
    named.sort_unstable();
    if member_ids != named {
        return Err(semantic(
            "a reshuffle proposal must cover the complete ranked set of its organ",
        ));
    }
    let mut positions: Vec<u32> = proposed.iter().map(|p| p.position).collect();
    positions.sort_unstable();
    let expected: Vec<u32> = (1..=positions.len() as u32).collect();
    if positions != expected {
        return Err(semantic(
            "proposed positions must be nonzero, unique, and contiguous within the organ",
        ));
    }

    let proposal_id = mint_proposal_id();
    let organ_context = organ_context_fingerprint(&organ, universe);
    let mut appends = Vec::new();
    let mut affected = Vec::new();
    let mut ordered: Vec<&ProposedPosition> = proposed.iter().collect();
    ordered.sort_by(|a, b| a.backlog_item_id.cmp(&b.backlog_item_id));
    for entry in ordered {
        let payload = mapping(vec![
            (PROPOSAL_KEY, serde_yaml::Value::String(proposal_id.clone())),
            ("position", serde_yaml::Value::Number(entry.position.into())),
            (
                PROPOSAL_CONTEXT_KEY,
                serde_yaml::Value::String(organ_context.clone()),
            ),
        ]);
        appends.push(PreparedHistoryAppend {
            bi_id: entry.backlog_item_id.clone(),
            entry: history_entry(HistoryKind::RankProposed, actor, role, at, Some(payload)),
        });
        affected.push(entry.backlog_item_id.clone());
    }
    Ok(PreparedBacklogMutation {
        operation: "propose_reshuffle",
        actor: actor.clone(),
        role,
        at: at.to_string(),
        // A proposal MOVES NO POSITION: no item body is written.
        items: Vec::new(),
        appends,
        affected,
        unranked: Vec::new(),
        proposal_id: Some(proposal_id),
        decide_commit: false,
    })
}

fn open_proposal(
    universe: &[BacklogItemView],
    proposal_id: &str,
) -> Result<OpenProposal, BacklogItemError> {
    let entries = proposal_entries(universe, proposal_id);
    if entries.is_empty() {
        return Err(semantic(format!(
            "no unresolved reshuffle proposal `{proposal_id}` exists"
        )));
    }
    if proposal_resolved(universe, proposal_id) {
        return Err(semantic(format!(
            "reshuffle proposal `{proposal_id}` is already resolved"
        )));
    }
    let mut positions = Vec::new();
    let mut organ_context: Option<String> = None;
    for (view, entry) in &entries {
        let position = entry
            .payload
            .as_ref()
            .and_then(|p| p.get("position"))
            .and_then(|v| v.as_u64())
            .ok_or_else(|| semantic("a rank_proposed entry carries no position"))?;
        let context = entry
            .payload
            .as_ref()
            .and_then(|p| p.get(PROPOSAL_CONTEXT_KEY))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        match &organ_context {
            None => organ_context = Some(context),
            Some(existing) if existing == &context => {}
            Some(_) => {
                return Err(semantic(
                    "a reshuffle proposal's organ context is inconsistent across its items",
                ))
            }
        }
        positions.push((view.item.backlog_item_id.clone(), position as u32));
    }
    positions.sort();
    Ok(OpenProposal {
        positions,
        organ_context: organ_context.unwrap_or_default(),
    })
}

fn prepare_resolve_reshuffle(
    universe: &[BacklogItemView],
    proposal_id: &str,
    approver: &str,
    commit: bool,
    role: DriverRole,
    actor: &ActorIdentity,
    at: &str,
) -> Result<PreparedBacklogMutation, BacklogItemError> {
    let operation = if commit { "commit_reshuffle" } else { "reject_reshuffle" };
    if role != DriverRole::NickShape {
        return Err(wrong_role(
            operation,
            role,
            "only Nick resolves a DECIDE proposal (NICK-GATE)",
        ));
    }
    if approver.trim().is_empty() {
        return Err(semantic(format!("{operation} requires a nonempty approver")));
    }
    let open = open_proposal(universe, proposal_id)?;
    let organ = universe
        .iter()
        .find(|v| v.item.backlog_item_id == open.positions[0].0)
        .map(|v| v.item.business_node_id.clone())
        .ok_or_else(|| semantic("the proposal's organ could not be resolved"))?;
    if organ_context_fingerprint(&organ, universe) != open.organ_context {
        return Err(semantic(format!(
            "reshuffle proposal `{proposal_id}` is stale: its organ moved after the proposal"
        )));
    }

    let kind = if commit { HistoryKind::RankCommitted } else { HistoryKind::RankRejected };
    let mut appends = Vec::new();
    let mut writes = Vec::new();
    let mut affected = Vec::new();
    for (bi_id, position) in &open.positions {
        let payload = mapping(vec![
            (PROPOSAL_KEY, serde_yaml::Value::String(proposal_id.to_string())),
            ("position", serde_yaml::Value::Number((*position).into())),
            ("approver", serde_yaml::Value::String(approver.to_string())),
        ]);
        appends.push(PreparedHistoryAppend {
            bi_id: bi_id.clone(),
            entry: history_entry(kind, actor, role, at, Some(payload)),
        });
        affected.push(bi_id.clone());
        if !commit {
            continue;
        }
        let view = universe
            .iter()
            .find(|v| &v.item.backlog_item_id == bi_id)
            .ok_or_else(|| semantic(format!("proposed item `{bi_id}` disappeared")))?;
        let mut item = view.item.clone();
        let rank = item
            .rank
            .as_mut()
            .ok_or_else(|| semantic(format!("proposed item `{bi_id}` lost its rank")))?;
        rank.position = *position;
        rank.explanation = format!("position {position} approved by DECIDE {proposal_id}");
        validate_item_semantics(&item)?;
        validate_required_by_state(&item, item.state)?;
        writes.push(PreparedItemWrite { bi_id: bi_id.clone(), item });
    }
    Ok(PreparedBacklogMutation {
        operation: if commit { "commit_reshuffle" } else { "reject_reshuffle" },
        actor: actor.clone(),
        role,
        at: at.to_string(),
        items: writes,
        appends,
        affected,
        unranked: Vec::new(),
        proposal_id: Some(proposal_id.to_string()),
        // Commit is the frozen ordering special case: every authorization entry
        // is appended before any approved position byte.
        decide_commit: commit,
    })
}

// ── the one mutation service entry point ────────────────────────────────────

/// Prepare one named post-genesis mutation. `universe` is every strictly loaded
/// item in the resolved hearth: organ-wide operations need the whole organ, and
/// a one-item read is never sufficient evidence for a position.
pub fn prepare_backlog_mutation(
    mutation: &BacklogMutation,
    universe: &[BacklogItemView],
    policy: &BacklogPolicy,
    role: DriverRole,
    actor: &ActorIdentity,
    at: &str,
) -> Result<PreparedBacklogMutation, BacklogItemError> {
    let find = |bi_id: &str| -> Result<&BacklogItemView, BacklogItemError> {
        universe
            .iter()
            .find(|v| v.item.backlog_item_id == bi_id)
            .ok_or_else(|| semantic(format!("backlog item `{bi_id}` does not exist")))
    };
    match mutation {
        BacklogMutation::ShapeEdit { bi_id, body } => {
            prepare_shape_edit(find(bi_id)?, body, role, actor, at)
        }
        BacklogMutation::RecomputeRank { business_node_id } => {
            prepare_recompute_rank(business_node_id, universe, policy, role, actor, at)
        }
        BacklogMutation::StampExecutionBinding {
            bi_id,
            execution_binding,
            outcome_binding,
        } => prepare_stamp(find(bi_id)?, execution_binding, outcome_binding, role, actor, at),
        BacklogMutation::RecordOutcomeSignoff { bi_id, approver } => {
            prepare_signoff(find(bi_id)?, approver, role, actor, at)
        }
        BacklogMutation::VetoAgeOut { bi_id, approver } => {
            prepare_veto(find(bi_id)?, approver, false, role, actor, at)
        }
        BacklogMutation::LiftAgeOutVeto { bi_id, approver } => {
            prepare_veto(find(bi_id)?, approver, true, role, actor, at)
        }
        BacklogMutation::ProposeReshuffle { proposed } => {
            prepare_propose_reshuffle(proposed, universe, role, actor, at)
        }
        BacklogMutation::CommitReshuffle { proposal_id, approver } => {
            prepare_resolve_reshuffle(universe, proposal_id, approver, true, role, actor, at)
        }
        BacklogMutation::RejectReshuffle { proposal_id, approver } => {
            prepare_resolve_reshuffle(universe, proposal_id, approver, false, role, actor, at)
        }
    }
}

// ---------------------------------------------------------------------------
// Queues (plan Task 7). A read never writes, a parked item is filtered off the
// ranked view while its rank stays byte-equal, and a valid rankless candidate is
// surfaced explicitly rather than silently dropped.
// ---------------------------------------------------------------------------

/// One ranked queue row.
#[derive(Debug, Clone, PartialEq)]
pub struct RankedQueueItem {
    pub backlog_item_id: String,
    pub business_node_id: String,
    pub state: State,
    pub position: u32,
    pub title: String,
    pub explanation: String,
    pub route_to_intake: bool,
}

/// One explicit pre-triage row (§1.12).
#[derive(Debug, Clone, PartialEq)]
pub struct UnrankedCandidate {
    pub backlog_item_id: String,
    pub business_node_id: String,
    pub state: State,
    pub title: String,
    pub route_to_intake: bool,
    pub reason: String,
}

/// The two explicit partitions of a queue read.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RankedQueueView {
    pub ranked: Vec<RankedQueueItem>,
    pub unranked_candidates: Vec<UnrankedCandidate>,
}

/// `route_to_intake` projection: an absent binding and a present null id both
/// project `true`; only a present concrete id projects the stored `false`.
pub fn projects_route_to_intake(item: &BacklogItem) -> bool {
    match &item.playbook_binding {
        None => true,
        Some(pb) if pb.playbook_definition_id.is_none() => true,
        Some(pb) => pb.route_to_intake,
    }
}

/// Project one organ's queue. Fails loudly on a rankless `ready` item, a
/// duplicate position, or a broken route invariant.
pub fn organ_queue_view(
    organ: &str,
    universe: &[BacklogItemView],
) -> Result<RankedQueueView, BacklogItemError> {
    validate_business_node_id(organ)?;
    project_queue(
        &universe
            .iter()
            .filter(|v| v.item.business_node_id == organ)
            .collect::<Vec<_>>(),
    )
}

/// Aggregate every organ stably. Never writes.
pub fn cross_organ_queue_view(
    universe: &[BacklogItemView],
) -> Result<RankedQueueView, BacklogItemError> {
    let mut view = project_queue(&universe.iter().collect::<Vec<_>>())?;
    view.ranked.sort_by(|a, b| {
        a.business_node_id
            .cmp(&b.business_node_id)
            .then(a.position.cmp(&b.position))
            .then(a.backlog_item_id.cmp(&b.backlog_item_id))
    });
    view.unranked_candidates.sort_by(|a, b| {
        a.business_node_id
            .cmp(&b.business_node_id)
            .then(a.backlog_item_id.cmp(&b.backlog_item_id))
    });
    Ok(view)
}

fn project_queue(views: &[&BacklogItemView]) -> Result<RankedQueueView, BacklogItemError> {
    let mut ranked: Vec<RankedQueueItem> = Vec::new();
    let mut unranked: Vec<UnrankedCandidate> = Vec::new();
    let mut seen: Vec<(String, u32)> = Vec::new();
    for view in views {
        let item = &view.item;
        // Every valid item is checked, so a corrupt one fails the read loudly
        // rather than silently vanishing from a projection.
        validate_item_semantics(item)?;
        if !matches!(item.state, State::Candidate | State::Ready) {
            continue;
        }
        match &item.rank {
            Some(rank) => {
                if seen
                    .iter()
                    .any(|(organ, pos)| organ == &item.business_node_id && *pos == rank.position)
                {
                    return Err(semantic(format!(
                        "duplicate rank position {} in organ `{}`",
                        rank.position, item.business_node_id
                    )));
                }
                seen.push((item.business_node_id.clone(), rank.position));
                ranked.push(RankedQueueItem {
                    backlog_item_id: item.backlog_item_id.clone(),
                    business_node_id: item.business_node_id.clone(),
                    state: item.state,
                    position: rank.position,
                    title: item.title.clone(),
                    explanation: rank.explanation.clone(),
                    route_to_intake: projects_route_to_intake(item),
                });
            }
            None if item.state == State::Ready => {
                return Err(semantic(format!(
                    "ready item `{}` carries no complete rank; a ready item may never appear \
                     in the pre-triage partition",
                    item.backlog_item_id
                )))
            }
            None => unranked.push(UnrankedCandidate {
                backlog_item_id: item.backlog_item_id.clone(),
                business_node_id: item.business_node_id.clone(),
                state: item.state,
                title: item.title.clone(),
                route_to_intake: projects_route_to_intake(item),
                reason: UNRANKED_REASON.to_string(),
            }),
        }
    }
    ranked.sort_by(|a, b| {
        a.position
            .cmp(&b.position)
            .then(a.backlog_item_id.cmp(&b.backlog_item_id))
    });
    unranked.sort_by(|a, b| a.backlog_item_id.cmp(&b.backlog_item_id));
    Ok(RankedQueueView { ranked, unranked_candidates: unranked })
}


// ---------------------------------------------------------------------------
// The store-backed mutation / queue service (plan Task 7). This is the one
// caller-visible seam every surface (fixture, gRPC handler, MCP tool) uses: it
// strictly loads and reconciles under the store, prepares purely, journals the
// compound write, and consumes the capability exactly once. Rejection precedes
// every item/history write.
// ---------------------------------------------------------------------------

/// The result of one applied mutation.
#[derive(Debug, Clone, PartialEq)]
pub struct BacklogMutationOutcome {
    pub operation: &'static str,
    /// Every touched item id, byte order.
    pub affected: Vec<String>,
    /// The explicit pre-triage partition reported by an organ-wide operation.
    pub unranked: Vec<String>,
    pub proposal_id: Option<String>,
}

fn store_invalid(e: BacklogItemError) -> crate::ports::backlog_item_port::BacklogStoreError {
    crate::ports::backlog_item_port::BacklogStoreError::Invalid { message: e.to_string() }
}

/// Strictly load every published item as the pure services see it. Recovery
/// runs first inside the store, so an interrupted transaction can never be
/// read as live state.
pub fn load_backlog_universe(
    store: &dyn crate::ports::backlog_item_port::BacklogItemPort,
) -> Result<
    (
        Vec<BacklogItemView>,
        Vec<crate::ports::backlog_item_port::LoadedBacklogItem>,
    ),
    crate::ports::backlog_item_port::BacklogStoreError,
> {
    let loaded = store.load_all()?;
    let views = loaded
        .iter()
        .map(|l| BacklogItemView {
            item: l.item.clone(),
            history: l.history.clone(),
        })
        .collect();
    Ok((views, loaded))
}

fn registry_rows(loaded: &[crate::ports::backlog_item_port::LoadedBacklogItem]) -> Vec<crate::domain::backlog_manifest::BacklogRegistryRow> {
    loaded
        .iter()
        .map(|l| crate::domain::backlog_manifest::BacklogRegistryRow {
            id: l.id.clone(),
            title: l.item.title.clone(),
            section: l.item.state.as_str().to_string(),
        })
        .collect()
}

/// Apply one named post-genesis mutation end to end.
#[allow(clippy::too_many_arguments)]
pub fn execute_backlog_mutation(
    store: &dyn crate::ports::backlog_item_port::BacklogItemPort,
    mutation: &BacklogMutation,
    policy: &BacklogPolicy,
    role: DriverRole,
    actor: &ActorIdentity,
    at: &str,
    operation_id: &str,
) -> Result<BacklogMutationOutcome, crate::ports::backlog_item_port::BacklogStoreError> {
    let (views, loaded) = load_backlog_universe(store)?;
    let prepared = prepare_backlog_mutation(mutation, &views, policy, role, actor, at)
        .map_err(store_invalid)?;

    // A no-op preparation (for example an organ whose every candidate is still
    // pre-triage) writes nothing at all rather than journaling an empty
    // transaction.
    if prepared.appends.is_empty() && prepared.items.is_empty() {
        return Ok(BacklogMutationOutcome {
            operation: prepared.operation,
            affected: prepared.affected,
            unranked: prepared.unranked,
            proposal_id: prepared.proposal_id,
        });
    }

    let mut touched: Vec<String> = prepared
        .appends
        .iter()
        .map(|a| a.bi_id.clone())
        .chain(prepared.items.iter().map(|i| i.bi_id.clone()))
        .collect();
    touched.sort();
    touched.dedup();
    let sources: Vec<_> = loaded
        .iter()
        .filter(|l| touched.iter().any(|t| t == &l.id))
        .cloned()
        .collect();

    let registry_old = store.registry_bytes()?;
    let registry_old_hash = registry_old
        .as_ref()
        .map(|b| crate::domain::content_hash::content_hash(b.as_bytes()));
    let commit = crate::domain::backlog_manifest::build_mutation_commit(
        operation_id,
        &prepared,
        &sources,
        &registry_rows(&loaded),
        registry_old_hash,
    )?;
    store.commit(commit)?;
    Ok(BacklogMutationOutcome {
        operation: prepared.operation,
        affected: prepared.affected,
        unranked: prepared.unranked,
        proposal_id: prepared.proposal_id,
    })
}

/// Strictly read one organ's queue. Never writes.
pub fn read_organ_queue(
    store: &dyn crate::ports::backlog_item_port::BacklogItemPort,
    organ: &str,
) -> Result<RankedQueueView, crate::ports::backlog_item_port::BacklogStoreError> {
    let (views, _) = load_backlog_universe(store)?;
    organ_queue_view(organ, &views).map_err(store_invalid)
}

/// Strictly read the aggregated cross-organ view. Never writes.
pub fn read_cross_organ_view(
    store: &dyn crate::ports::backlog_item_port::BacklogItemPort,
) -> Result<RankedQueueView, crate::ports::backlog_item_port::BacklogStoreError> {
    let (views, _) = load_backlog_universe(store)?;
    cross_organ_queue_view(&views).map_err(store_invalid)
}


// ---------------------------------------------------------------------------
// Explicit engine-auto evaluation through the SAME prepared-transition seam
// (plan Task 8). `evaluate_backlog` is the ONLY constructor of the private
// `TransitionOrigin::Evaluation`, so `#4/#9/#13/#14/#16` under `engine_auto` are
// unreachable from any request mapping. Row #10's public `engine_auto` path is
// deliberately untouched: it maps to `PublicSnapshot` and is guarded solely by
// the stored done-rule reading.
// ---------------------------------------------------------------------------

/// The two locally executable wake grammars, and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeVerdict {
    /// The predicate holds right now.
    Satisfied,
    /// The referenced item can never satisfy the predicate again.
    Unreachable,
    /// Persisted but neither auto-satisfied nor auto-unreachable here.
    Undetermined,
}

const STATE_PREDICATE_PREFIX: &str = "state==";
const DEPENDENCY_PREDICATE: &str = "dependency_readiness==ready";

/// Resolve one wake condition against the strictly loaded universe.
///
/// `item_state` requires `ref.kind=backlog_item`, a valid `bi_`, and the exact
/// ASCII `state==<closed state token>`. `dependency_ready` requires the same ref
/// kind/id and the exact ASCII `dependency_readiness==ready`. Any other ref kind
/// or predicate is invalid at shape time, not silently undetermined here.
/// `manual`, `measure_threshold` and `external_event` are persisted but never
/// auto-resolved: Nick wakes those through public #13/#14.
pub fn resolve_wake(
    wake: &WakeCondition,
    universe: &[BacklogItemView],
) -> Result<WakeVerdict, BacklogItemError> {
    match wake.kind {
        WakeKind::Manual | WakeKind::MeasureThreshold | WakeKind::ExternalEvent => {
            Ok(WakeVerdict::Undetermined)
        }
        WakeKind::ItemState | WakeKind::DependencyReady => {
            let reference = wake.r#ref.as_ref().ok_or_else(|| {
                semantic("a locally executable wake_condition requires its ref")
            })?;
            if reference.kind != EvidenceKind::BacklogItem {
                return Err(semantic(format!(
                    "a locally executable wake_condition ref must be kind=backlog_item, not \
                     `{:?}`",
                    reference.kind
                )));
            }
            validate_backlog_item_id(&reference.id)?;
            let referenced = universe
                .iter()
                .find(|v| v.item.backlog_item_id == reference.id)
                .ok_or_else(|| {
                    semantic(format!(
                        "the wake_condition references `{}`, which is not published",
                        reference.id
                    ))
                })?;
            match wake.kind {
                WakeKind::ItemState => {
                    let token = wake
                        .predicate
                        .strip_prefix(STATE_PREDICATE_PREFIX)
                        .ok_or_else(|| {
                            semantic(format!(
                                "an item_state wake predicate must be exactly \
                                 `state==<state>`, not `{}`",
                                wake.predicate
                            ))
                        })?;
                    let wanted = parse_state_token(token)?;
                    if referenced.item.state == wanted {
                        Ok(WakeVerdict::Satisfied)
                    } else if referenced.item.state.is_terminal() {
                        // A terminal referenced item can never move again, so
                        // the equality is PROVABLY unreachable.
                        Ok(WakeVerdict::Unreachable)
                    } else {
                        Ok(WakeVerdict::Undetermined)
                    }
                }
                _ => {
                    if wake.predicate != DEPENDENCY_PREDICATE {
                        return Err(semantic(format!(
                            "a dependency_ready wake predicate must be exactly \
                             `{DEPENDENCY_PREDICATE}`, not `{}`",
                            wake.predicate
                        )));
                    }
                    let ready = referenced
                        .item
                        .rank
                        .as_ref()
                        .map(|r| r.inputs.dependency_readiness.status == DependencyStatus::Ready)
                        .unwrap_or(false);
                    if ready {
                        Ok(WakeVerdict::Satisfied)
                    } else if referenced.item.state.is_terminal() {
                        Ok(WakeVerdict::Unreachable)
                    } else {
                        Ok(WakeVerdict::Undetermined)
                    }
                }
            }
        }
    }
}

/// Decode one closed K8 state token. Public so the transport layer never
/// invents its own parallel token table.
pub fn parse_state_token(token: &str) -> Result<State, BacklogItemError> {
    Ok(match token {
        "candidate" => State::Candidate,
        "ready" => State::Ready,
        "in_flight" => State::InFlight,
        "done" => State::Done,
        "parked" => State::Parked,
        "superseded" => State::Superseded,
        "aged_out" => State::AgedOut,
        other => {
            return Err(semantic(format!(
                "`{other}` is not one of the closed K8 state tokens"
            )))
        }
    })
}

/// One item's complete, ordered contribution to an evaluation batch.
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluationItemPlan {
    pub bi_id: String,
    /// Appends in EXACT order. A surviving re-rank that then ages out records
    /// `rank_recomputed` then `state_change`; an advancement records the
    /// printed-role `state_change` then the private derived `rank_recomputed`.
    pub appends: Vec<HistoryEntry>,
    /// The authoritative transition record, when this item moved state.
    pub moved_to: Option<State>,
    pub role: DriverRole,
    pub final_item: BacklogItem,
}

/// The consume-once evaluation batch.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedBacklogEvaluation {
    pub actor: ActorIdentity,
    pub at: String,
    /// Per-item plans in byte-ID order.
    pub plans: Vec<EvaluationItemPlan>,
    pub recomputed: Vec<String>,
    pub woken: Vec<String>,
    pub aged_out: Vec<String>,
    pub vetoed: Vec<String>,
    pub unranked: Vec<String>,
}

/// The outcome one applied evaluation reports.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BacklogEvaluationOutcome {
    pub recomputed: Vec<String>,
    pub woken: Vec<String>,
    pub aged_out: Vec<String>,
    pub vetoed: Vec<String>,
    pub unranked: Vec<String>,
}

fn derived_rank_history(
    actor: &ActorIdentity,
    at: &str,
    position: u32,
    explanation: &str,
    consumed: &[u64],
    triggering: Option<u64>,
) -> HistoryEntry {
    let mut pairs: Vec<(&str, serde_yaml::Value)> = vec![
        ("position", serde_yaml::Value::Number(position.into())),
        (
            "explanation",
            serde_yaml::Value::String(explanation.to_string()),
        ),
        (
            CONSUMED_KEY,
            serde_yaml::Value::Sequence(
                consumed
                    .iter()
                    .map(|s| serde_yaml::Value::Number((*s).into()))
                    .collect(),
            ),
        ),
    ];
    if let Some(seq) = triggering {
        pairs.push((
            "triggering_state_change_seq",
            serde_yaml::Value::Number(seq.into()),
        ));
    }
    // The AUTHORITY is server-fixed private `engine_auto`; a request can never
    // select it. The initiating identity is retained as trigger attribution.
    history_entry(
        HistoryKind::RankRecomputed,
        actor,
        DriverRole::EngineAuto,
        at,
        Some(mapping(pairs)),
    )
}

/// Purely derive one organ's evaluation batch.
fn prepare_organ_evaluation(
    organ: &str,
    universe: &[BacklogItemView],
    policy: &BacklogPolicy,
    actor: &ActorIdentity,
    at: &str,
    out: &mut PreparedBacklogEvaluation,
) -> Result<(), BacklogItemError> {
    let (survivors, unranked) = recompute_organ(organ, universe, policy)?;
    out.unranked.extend(unranked);

    // ── ranked survivors: age advances, then #4/#9 fires only when the
    //    INCREMENTED age strictly exceeds the budget and no veto stands ──────
    struct Move {
        bi_id: String,
        to: State,
    }
    let budget = policy.age_budget.get();
    let mut moves: Vec<Move> = Vec::new();
    let mut recomputed_items: Vec<(&BacklogItemView, BacklogItem, Vec<u64>)> = survivors;

    for (view, item, _) in recomputed_items.iter() {
        let age = item.rank.as_ref().map(|r| r.inputs.age).unwrap_or(0);
        if age <= budget {
            continue;
        }
        if has_standing_veto(&view.history) {
            // The exemption blocks the EDGE; the ranked age still advanced.
            out.vetoed.push(item.backlog_item_id.clone());
            continue;
        }
        match item.state {
            State::Candidate => moves.push(Move {
                bi_id: item.backlog_item_id.clone(),
                to: State::AgedOut,
            }),
            State::Ready => moves.push(Move {
                bi_id: item.backlog_item_id.clone(),
                to: State::AgedOut,
            }),
            _ => {}
        }
    }

    // ── parked items: only the two local wake grammars resolve here ─────────
    let mut wakes: Vec<(String, State)> = Vec::new();
    let mut parked: Vec<&BacklogItemView> = universe
        .iter()
        .filter(|v| v.item.business_node_id == organ && v.item.state == State::Parked)
        .collect();
    parked.sort_by(|a, b| a.item.backlog_item_id.cmp(&b.item.backlog_item_id));
    for view in parked {
        let wake = match view.item.exit.as_ref().and_then(|e| e.wake_condition.as_ref()) {
            Some(wake) => wake,
            None => continue,
        };
        match resolve_wake(wake, universe)? {
            WakeVerdict::Satisfied => {
                let ranked_and_ready = view.item.rank.is_some()
                    && view
                        .item
                        .rank
                        .as_ref()
                        .map(|r| r.inputs.dependency_readiness.status == DependencyStatus::Ready)
                        .unwrap_or(false);
                let to = if ranked_and_ready { State::Ready } else { State::Candidate };
                wakes.push((view.item.backlog_item_id.clone(), to));
            }
            WakeVerdict::Unreachable => {
                if has_standing_veto(&view.history) {
                    out.vetoed.push(view.item.backlog_item_id.clone());
                    continue;
                }
                wakes.push((view.item.backlog_item_id.clone(), State::AgedOut));
            }
            WakeVerdict::Undetermined => {}
        }
    }

    // ── prepare EVERY derived edge through the shared seam ──────────────────
    let mut plans: Vec<EvaluationItemPlan> = Vec::new();
    for m in &moves {
        let (view, item, consumed) = recomputed_items
            .iter()
            .find(|(_, i, _)| i.backlog_item_id == m.bi_id)
            .expect("a move names one of its own survivors");
        let mut source = item.clone();
        // `aged_out_reason` is supplied ONLY as a state-entry input; the
        // normalizer turns it into the canonical exit and appends no history.
        let reason = match source.state {
            State::Candidate => AgedOutReason::StaleNoReady,
            _ => AgedOutReason::StaleNoPickup,
        };
        source.exit = Some(Exit {
            kind: ExitKind::AgedOut,
            wake_condition: None,
            superseded_by: None,
            aged_out_reason: Some(reason),
        });
        let from = source.state;
        let prepared = prepare_backlog_transition(
            &source,
            &view.history,
            from,
            State::AgedOut,
            DriverRole::EngineAuto,
            actor,
            at,
            TransitionOrigin::evaluation(),
            None,
            None,
        )?;
        let rank = item
            .rank
            .as_ref()
            .expect("a survivor carries a complete rank");
        // rank_recomputed(N) strictly BEFORE the resulting state_change(N+1).
        let mut appends = vec![derived_rank_history(
            actor,
            at,
            rank.position,
            &rank.explanation,
            consumed,
            None,
        )];
        appends.push(prepared.state_change.clone());
        plans.push(EvaluationItemPlan {
            bi_id: m.bi_id.clone(),
            appends,
            moved_to: Some(State::AgedOut),
            role: DriverRole::EngineAuto,
            final_item: prepared.target_item,
        });
        out.aged_out.push(m.bi_id.clone());
    }

    // A ranked advancement is NEVER decided from a one-item read: evaluation
    // supplies the same strict same-organ context the public seam requires.
    let organ_context = BacklogTransitionContext {
        business_node_id: organ.to_string(),
        ranked: universe
            .iter()
            .filter(|v| v.item.business_node_id == organ)
            .filter(|v| matches!(v.item.state, State::Candidate | State::Ready))
            .filter(|v| v.item.rank.is_some())
            .map(|v| v.item.clone())
            .collect(),
        unranked_candidate_ids: out.unranked.clone(),
        policy: policy.clone(),
        registry_hash: None,
        context_hash: String::new(),
    };
    for (bi_id, to) in &wakes {
        let view = universe
            .iter()
            .find(|v| &v.item.backlog_item_id == bi_id)
            .expect("a wake names a loaded item");
        let mut source = view.item.clone();
        if *to == State::AgedOut {
            source.exit = Some(Exit {
                kind: ExitKind::AgedOut,
                wake_condition: None,
                superseded_by: None,
                aged_out_reason: Some(AgedOutReason::WakeUnreachable),
            });
        }
        let prepared = prepare_backlog_transition(
            &source,
            &view.history,
            State::Parked,
            *to,
            DriverRole::EngineAuto,
            actor,
            at,
            TransitionOrigin::evaluation(),
            Some(&organ_context),
            None,
        )?;
        // An ADVANCEMENT records the printed-role state_change first; the
        // private derived rank append follows only when a position exists.
        let prepared_rankless = prepared.target_item.rank.is_none();
        let mut appends = vec![prepared.state_change.clone()];
        if let Some(rank) = prepared.target_item.rank.as_ref() {
            appends.push(derived_rank_history(
                actor,
                at,
                rank.position,
                &rank.explanation,
                &[],
                None,
            ));
        }
        plans.push(EvaluationItemPlan {
            bi_id: bi_id.clone(),
            appends,
            moved_to: Some(*to),
            role: DriverRole::EngineAuto,
            final_item: prepared.target_item,
        });
        if *to == State::AgedOut {
            out.aged_out.push(bi_id.clone());
        } else {
            out.woken.push(bi_id.clone());
            if prepared_rankless {
                // A rankless #13 target stays in the explicit pre-triage
                // partition; no rank is ever fabricated for it (§1.12).
                out.unranked.push(bi_id.clone());
            }
        }
    }

    // ── every remaining survivor records its re-rank alone ──────────────────
    recomputed_items.retain(|(_, item, _)| {
        !plans.iter().any(|p| p.bi_id == item.backlog_item_id)
    });
    for (_, item, consumed) in &recomputed_items {
        let rank = item
            .rank
            .as_ref()
            .expect("a survivor carries a complete rank");
        plans.push(EvaluationItemPlan {
            bi_id: item.backlog_item_id.clone(),
            appends: vec![derived_rank_history(
                actor,
                at,
                rank.position,
                &rank.explanation,
                consumed,
                None,
            )],
            moved_to: None,
            role: DriverRole::EngineAuto,
            final_item: item.clone(),
        });
        out.recomputed.push(item.backlog_item_id.clone());
    }

    // ── one final same-organ materialization, so every position written by
    //    this batch is coherent with every state move it just made ──────────
    let mut resulting: Vec<BacklogItem> = universe
        .iter()
        .filter(|v| v.item.business_node_id == organ)
        .map(|v| {
            plans
                .iter()
                .find(|p| p.bi_id == v.item.backlog_item_id)
                .map(|p| p.final_item.clone())
                .unwrap_or_else(|| v.item.clone())
        })
        .collect();
    resulting.sort_by(|a, b| a.backlog_item_id.cmp(&b.backlog_item_id));
    let final_positions = materialize_rank(&resulting, policy);
    for (bi_id, position) in final_positions {
        let plan = match plans.iter_mut().find(|p| p.bi_id == bi_id) {
            Some(plan) => plan,
            None => continue,
        };
        let explanation = format!("position {position} under the resolved comparator");
        if let Some(rank) = plan.final_item.rank.as_mut() {
            rank.position = position;
            rank.explanation = explanation.clone();
        }
        for entry in plan.appends.iter_mut() {
            if entry.kind != HistoryKind::RankRecomputed {
                continue;
            }
            if let Some(serde_yaml::Value::Mapping(map)) = entry.payload.as_mut() {
                map.insert(
                    serde_yaml::Value::String("position".to_string()),
                    serde_yaml::Value::Number(position.into()),
                );
                map.insert(
                    serde_yaml::Value::String("explanation".to_string()),
                    serde_yaml::Value::String(explanation.clone()),
                );
            }
        }
    }
    for plan in &plans {
        validate_item_semantics(&plan.final_item)?;
        validate_required_by_state(&plan.final_item, plan.final_item.state)?;
    }

    plans.sort_by(|a, b| a.bi_id.cmp(&b.bi_id));
    out.plans.extend(plans);
    Ok(())
}

/// Purely derive the whole evaluation batch. `organ` restricts it to one
/// business node; `None` evaluates every organ.
pub fn prepare_backlog_evaluation(
    organ: Option<&str>,
    universe: &[BacklogItemView],
    policy: &BacklogPolicy,
    actor: &ActorIdentity,
    at: &str,
) -> Result<PreparedBacklogEvaluation, BacklogItemError> {
    let mut organs: Vec<String> = match organ {
        Some(one) => {
            validate_business_node_id(one)?;
            vec![one.to_string()]
        }
        None => {
            let mut all: Vec<String> = universe
                .iter()
                .map(|v| v.item.business_node_id.clone())
                .collect();
            all.sort();
            all.dedup();
            all
        }
    };
    organs.sort();
    let mut out = PreparedBacklogEvaluation {
        actor: actor.clone(),
        at: at.to_string(),
        plans: Vec::new(),
        recomputed: Vec::new(),
        woken: Vec::new(),
        aged_out: Vec::new(),
        vetoed: Vec::new(),
        unranked: Vec::new(),
    };
    for organ in &organs {
        prepare_organ_evaluation(organ, universe, policy, actor, at, &mut out)?;
    }
    out.recomputed.sort();
    out.woken.sort();
    out.aged_out.sort();
    out.vetoed.sort();
    out.unranked.sort();
    Ok(out)
}

/// Run one caller-invoked, lock-held evaluation end to end.
pub fn evaluate_backlog(
    store: &dyn crate::ports::backlog_item_port::BacklogItemPort,
    organ: Option<&str>,
    policy: &BacklogPolicy,
    actor: &ActorIdentity,
    at: &str,
    operation_id: &str,
    hi_res_prefix: &str,
    random_suffix: &str,
) -> Result<BacklogEvaluationOutcome, crate::ports::backlog_item_port::BacklogStoreError> {
    let (views, loaded) = load_backlog_universe(store)?;
    let prepared =
        prepare_backlog_evaluation(organ, &views, policy, actor, at).map_err(store_invalid)?;
    let outcome = BacklogEvaluationOutcome {
        recomputed: prepared.recomputed.clone(),
        woken: prepared.woken.clone(),
        aged_out: prepared.aged_out.clone(),
        vetoed: prepared.vetoed.clone(),
        unranked: prepared.unranked.clone(),
    };
    if prepared.plans.is_empty() {
        return Ok(outcome);
    }
    let sources: Vec<_> = loaded
        .iter()
        .filter(|l| prepared.plans.iter().any(|p| p.bi_id == l.id))
        .cloned()
        .collect();
    let mut rows = registry_rows(&loaded);
    for plan in &prepared.plans {
        if let Some(to) = plan.moved_to {
            if let Some(row) = rows.iter_mut().find(|r| r.id == plan.bi_id) {
                row.section = to.as_str().to_string();
            }
        }
    }
    let registry_old_hash = store
        .registry_bytes()?
        .as_ref()
        .map(|b| crate::domain::content_hash::content_hash(b.as_bytes()));
    let commit = crate::domain::backlog_manifest::build_evaluation_commit(
        operation_id,
        &prepared,
        &sources,
        &rows,
        registry_old_hash,
        hi_res_prefix,
        random_suffix,
    )?;
    store.commit(commit)?;
    Ok(outcome)
}

#[cfg(test)]
mod tests;
