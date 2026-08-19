//! Schema types for the playbook artifact kind.
//!
//! A `PlaybookMachine` is the in-memory representation of a `machine.yaml`
//! file. The loader in `loader.rs` produces values of these types; the
//! interpreter in `interpreter.rs` (Phase 4) queries them.
//!
//! All structs carry `#[serde(deny_unknown_fields)]` to enforce R7.3's
//! sequential-phase-only guarantee — unknown keys (e.g., event-trigger
//! fields like `timeout_advance`) are rejected rather than silently ignored.

use serde::{Deserialize, Serialize};

/// The canonical set of quality dimensions a [`SuccessRubric`] may weight.
///
/// Locked by decision `playbook_success_rubric_model` (2026-07-01): a small
/// SHARED vocabulary makes cross-playbook quality scores comparable — the
/// router experiments require comparability, so free-form per-playbook
/// dimensions were rejected. Each playbook SELECTS from and WEIGHTS this set.
///
/// Identifiers are snake_case to match the YAML/serde convention used
/// everywhere else in the schema. A rubric referencing any dimension NOT in
/// this list is rejected at load time (see `loader::validate_with_id`).
///
/// `parent_alignment` (added for the PROPOSAL-ALIGNMENT measurement arm):
/// whether an artifact (spec/plan) advances, stays within scope of, and
/// avoids the out-of-scope of its parent proposal's declared intent.
/// Alignment is orthogonal to intrinsic quality — a well-written artifact
/// that drifts from its proposal still scores LOW on this dimension. Unlike
/// the other dimensions here, `parent_alignment` is graded PER-INSTANCE
/// (once per track, against that track's resolved parent proposal), not
/// per-doer-step — a track has exactly one parent to align with, not one
/// per state.
pub const QUALITY_DIMENSIONS: [&str; 9] = [
    "correctness",
    "clarity_structure",
    "pattern_alignment",
    "research_rigor",
    "forethought",
    "extensibility",
    "security",
    "faithfulness_to_real_process",
    "parent_alignment",
];

/// Whether `dimension` is one of the canonical [`QUALITY_DIMENSIONS`].
///
/// The single validation helper the loader and any future rubric author use to
/// keep the vocabulary comparable across playbooks.
pub fn is_valid_quality_dimension(dimension: &str) -> bool {
    QUALITY_DIMENSIONS.contains(&dimension)
}

/// The role-filter classifications a state may carry.
/// These are used by consumers to bucket states into role-appropriate views.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RoleFilter {
    DoerActionable,
    ReviewPending,
    ReviewAwaiting,
    CreatorParent,
    Terminal,
}

/// A field required for artifact creation under this playbook.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FieldDescriptor {
    /// Field name (e.g., `"playbook_name"`).
    pub name: String,
    /// Field type hint (e.g., `"string"`, `"artifact_id"`, `"actor_name"`).
    #[serde(rename = "field_type")]
    pub field_type: String,
    /// Short description surfaced via `available_types`.
    pub description: String,
}

/// Step-measurement spec for a (state, role) pair.
///
/// Carries the intent (what the actor should accomplish) and expected_output
/// (what artifact proves completion). Used by `state_role_measurement` to
/// look up the spec for a given (state, role) pair.
///
/// `#[serde(deny_unknown_fields)]` matches `FieldDescriptor` — unknown keys
/// in a machine.yaml `measurement_by_role` entry are rejected rather than
/// silently ignored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct MeasurementSpec {
    /// What the actor must accomplish in this (state, role) step.
    pub intent: String,
    /// The artifact or observable that proves the step is complete.
    pub expected_output: String,
    /// Optional per-step success signal: the criteria (or grader hint) that
    /// distinguish a *good* completion of this (state, role) step from one that
    /// merely produced the expected output. Populated later by the generator's
    /// measurement-authoring phase; declared here so specs can carry it.
    ///
    /// `#[serde(default)]` is mandatory — `MeasurementSpec` carries
    /// `#[serde(deny_unknown_fields)]`, so every existing `measurement_by_role`
    /// entry (which lacks `success_criteria`) must continue to parse.
    #[serde(default)]
    pub success_criteria: Option<String>,
    /// The evidence obligation for this (state, role) step: the set of required
    /// [`EvidenceClass`] values this step's execution must claim. Each entry
    /// means "claim ≥1 evidence item of that class OR STRONGER" (v1 fixes
    /// min-count at 1 per required class; upward substitution is defined in
    /// `evidence_obligation::obligation_satisfied`). The `Vec` is interpreted as
    /// a set — order and duplicates are semantically irrelevant. Names classes
    /// only (no per-artifact references — that is T-EEC-2's claimed-evidence
    /// channel). Empty ⇒ no obligation. Added parser-first (T-EEC-1): the field
    /// parses and persists before anything enforces it.
    ///
    /// `#[serde(default)]` is mandatory — `MeasurementSpec` carries
    /// `#[serde(deny_unknown_fields)]`, so every existing `measurement_by_role`
    /// entry (which lacks `evidence_obligation`) must continue to parse.
    /// `skip_serializing_if = "Vec::is_empty"` is added DELIBERATELY (unlike
    /// `success_criteria`) so an absent/empty obligation is omitted entirely
    /// from serialization — the R2 byte-identity-when-absent guarantee, which
    /// keeps every undeclared fleet machine's content-hash `playbook_version`
    /// unchanged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_obligation: Vec<EvidenceClass>,
}

/// The admissible-evidence class a rubric dimension is judged against.
///
/// Decision `playbook_success_rubric_model` Amendment 1 item 13: each dimension
/// declares its admissible evidence (artifacts-of-consequence > verifiable
/// citations > self-description); the judge is constrained to it, and
/// self-description-only dimensions are second-class BY CONSTRUCTION. A
/// dimension that does not declare an evidence class therefore defaults to the
/// weakest tier ([`EvidenceClass::SelfDescription`]) so the absence of a
/// declaration is treated conservatively rather than optimistically.
///
/// Identifiers are snake_case to match the YAML/serde convention. An unknown
/// value is rejected at parse time by serde (the enum admits no other variant),
/// surfacing as a `workflow_yaml_parse_error`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceClass {
    /// The strongest evidence: a produced artifact whose existence/quality is
    /// itself the proof (a passing test, a merged change, a published topic).
    ArtifactOfConsequence,
    /// A checkable citation to an external fact (a file path + line, a URL).
    VerifiableCitation,
    /// The weakest evidence: the actor's own description of what they did. The
    /// default — a dimension that declares nothing is second-class by design.
    #[default]
    SelfDescription,
}

/// A single weighted dimension within a [`SuccessRubric`].
///
/// `dimension` MUST be one of [`QUALITY_DIMENSIONS`]; `weight` is a relative
/// integer weight (well-formed = strictly positive). Integer weights keep the
/// whole schema type-graph `Eq`-derivable (floats do not), and relative
/// weights are all the weighted-overall computation needs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct RubricDimension {
    /// Quality dimension identifier; validated against [`QUALITY_DIMENSIONS`].
    pub dimension: String,
    /// Relative weight of this dimension in the weighted overall score.
    pub weight: u32,
    /// The admissible-evidence class the judge is constrained to for this
    /// dimension. `#[serde(default)]` is mandatory — `RubricDimension` carries
    /// `#[serde(deny_unknown_fields)]`, so every existing rubric dimension
    /// (which lacks `evidence_class`) must continue to parse; the default is the
    /// weakest tier ([`EvidenceClass::SelfDescription`]).
    #[serde(default)]
    pub evidence_class: EvidenceClass,
}

/// A reference to an exemplar instance the rubric's judge scores relative to.
///
/// Decision `playbook_success_rubric_model` Amendment 1 item 12 (anchored
/// scoring): absolute 0..1 scores are noisy, so each rubric ships 1–2 reference
/// exemplars (the Phase-2 gate's good/mediocre pairs seed these) and the judge
/// scores relative to them. `instance` is an opaque instance reference; `band`
/// is the exemplar's quality band (e.g. `"good"`, `"mediocre"`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct AnchorRef {
    /// Opaque reference to the exemplar instance.
    pub instance: String,
    /// The exemplar's quality band, e.g. `"good"` or `"mediocre"`.
    pub band: String,
}

/// The playbook-level success rubric declared on a [`PlaybookMachine`].
///
/// Additive + optional (decision `playbook_success_rubric_model`): a machine
/// may declare which quality dimensions matter for it and how heavily, name the
/// grader that produces the leading score, and DECLARE (defined now, consumed
/// in Phase 5) the lagging-signal identifiers. Machines without a rubric are
/// unaffected — the whole field is `#[serde(default)]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct SuccessRubric {
    /// Weighted quality dimensions selected from [`QUALITY_DIMENSIONS`].
    #[serde(default)]
    pub dimensions: Vec<RubricDimension>,
    /// Reference to the grader that produces the leading quality score (e.g. an
    /// LLM rubric-judge id or a deterministic-proxy name). The grader itself is
    /// a later track; here we carry only the reference.
    #[serde(default)]
    pub grader: Option<String>,
    /// Declared lagging-signal identifiers (churn/revert/adoption/…). Defined
    /// now, WIRED last (Phase 5); "reached terminal" is never "done well."
    #[serde(default)]
    pub lagging_signals: Vec<String>,
    /// Exemplar instance references the judge scores relative to (anchored
    /// scoring). `#[serde(default)]` empty — `SuccessRubric` carries
    /// `#[serde(deny_unknown_fields)]`, so every existing rubric (which lacks
    /// `anchors`) must continue to parse.
    #[serde(default)]
    pub anchors: Vec<AnchorRef>,
}

/// A checkable-fact descriptor declared on a [`PlaybookMachine`], distinct from
/// the quality [`SuccessRubric`].
///
/// Decision `playbook_success_rubric_model` Amendment 1 item 6: every playbook
/// declares BOTH an `outcome_predicate` (an externally checkable FACT — did the
/// world-change happen) and the `success_rubric` (HOW WELL). Predicates are
/// near-Goodhart-proof and are the router experiments' primary fitness input.
/// They version implicitly with the machine — changing one is a visible variant
/// event.
///
/// v1 is intentionally SMALL (no DSL): a `terminal_state` naming the machine
/// state whose reaching is the checkable fact, plus an optional named/described
/// `check` a later fold or judge can evaluate. Richer predicates land later.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct OutcomePredicate {
    /// The machine state whose reaching is the checkable outcome fact. Validated
    /// at load time to be one of the machine's declared states.
    pub terminal_state: String,
    /// An optional named or described checkable fact the fold/judge can later
    /// evaluate (e.g. `"documents reconciled"`, `"behavior on main + brine-green"`).
    /// `#[serde(default)]` — `OutcomePredicate` carries `#[serde(deny_unknown_fields)]`.
    #[serde(default)]
    pub check: Option<String>,
}

/// A single state in the playbook state machine.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StateDefinition {
    /// State name (e.g., `"spec"`, `"spec_review"`).
    pub name: String,
    /// Role-based filter classifications for this state.
    #[serde(default)]
    pub role_filters: Vec<RoleFilter>,
    /// Registry section where artifacts in this state are listed.
    pub registry_section: String,
    /// Projection files to update on transitions into this state.
    #[serde(default)]
    pub projection_targets: Vec<String>,
    /// Whether this is a review gate (enables `complete(satisfaction:...)` semantics).
    pub is_review_gate: bool,
    /// Whether this state is terminal (no further transitions expected).
    pub is_terminal: bool,
    /// Optional hook file under `hooks/` served on `begin` for this state.
    /// The engine discovers but does not yet serve this file (Phase 6 wires serving).
    #[serde(default)]
    pub hook: Option<String>,
    /// Per-role hook files under `hooks/` served on `begin` for this state.
    /// Maps role name (e.g. `"doer"`, `"reviewer"`) to a filename under `hooks/`.
    /// `state_role_hook` reads this map first and falls back to `hook` when the
    /// role key is absent.  `#[serde(default)]` is mandatory because
    /// `StateDefinition` carries `#[serde(deny_unknown_fields)]`; omitting it
    /// causes every existing machine.yaml (which lacks `hooks_by_role`) to fail
    /// parsing.
    #[serde(default)]
    pub hooks_by_role: std::collections::BTreeMap<String, String>,
    /// Per-role step-measurement specs for this state.
    /// Maps role name (e.g. `"doer"`, `"reviewer"`) to a `MeasurementSpec`.
    /// `state_role_measurement` looks up the spec for the given role.
    /// `#[serde(default)]` is mandatory — `StateDefinition` carries
    /// `#[serde(deny_unknown_fields)]` so every existing machine.yaml (which
    /// lacks `measurement_by_role`) must parse without error.
    #[serde(default)]
    pub measurement_by_role: std::collections::BTreeMap<String, MeasurementSpec>,
}

/// A transition between two states.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TransitionDefinition {
    /// The source state.
    pub from_state: String,
    /// The target state.
    pub to_state: String,
    /// The role required to execute this transition.
    pub required_role: String,
    /// Legal satisfaction values for review-gate transitions. Null for non-review-gate.
    #[serde(default)]
    pub required_satisfaction: Option<Vec<String>>,
    /// Whether an approver must be named in the snapshot call.
    pub requires_approver: bool,
    /// Optional hook file under `hooks/` served on `begin` for this transition,
    /// if different from the target state's hook.
    #[serde(default)]
    pub hook: Option<String>,
}

/// A machine's *register*: whether it is a driven playbook the router routes
/// to, or a free generative kind invocable out-of-band.
///
/// Driven playbooks (track/playbook/proposal/knowledge_lifecycle/…) are
/// routed-to candidates. Free kinds (spark/decision/learning) are invocable
/// anytime out-of-band and are excluded from router candidates
/// (decision `driven-workflows-vs-free-generative-types`). Defaults to
/// `Driven` so existing authored machines need no change.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Register {
    /// A playbook the router routes inputs into (the default).
    #[default]
    Driven,
    /// A free generative kind, invocable out-of-band; never a route target.
    Free,
}

/// Minimum requester role required to access a playbook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Read-only access; the default minimum role.
    #[default]
    Read,
    /// Write access.
    Write,
    /// Administrative access.
    Admin,
}

/// Declared sensitivity classification for a playbook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    /// Public information.
    Public,
    /// Internal information; the default sensitivity.
    #[default]
    Internal,
    /// Confidential information.
    Confidential,
    /// Protected health information.
    Phi,
}

/// Playbook access declaration read from `machine.yaml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Access {
    /// Owning org. An empty string or the value `"Foundation"` means wildcard.
    #[serde(default)]
    pub org: String,
    /// Minimum role required to access this playbook.
    #[serde(default)]
    pub min_role: Role,
    /// Declared sensitivity for this playbook.
    #[serde(default)]
    pub sensitivity: Sensitivity,
    /// Optional space constraint.
    #[serde(default)]
    pub space: Option<String>,
}

impl Access {
    /// Default applied when an authored `machine.yaml` omits the whole access block.
    pub fn foundation() -> Self {
        Self {
            org: "Foundation".to_string(),
            min_role: Role::Read,
            sensitivity: Sensitivity::Internal,
            space: None,
        }
    }
}

/// Machine-owned route matching hints.
///
/// Empty triggers mean the machine never auto-resolves from user input. This
/// block is intentionally declarative only; route resolution is implemented in
/// the router layer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct RouteConfig {
    /// LLM-routing-oriented description used only for route candidate selection.
    #[serde(default)]
    pub description: Option<String>,
    /// Exact trigger phrases this machine owns.
    #[serde(default)]
    pub triggers: Vec<String>,
}

/// The in-memory representation of a playbook artifact's `machine.yaml`.
///
/// Produced by `loader::load_from_yaml`; consumed by `interpreter::outgoing_transitions`
/// and future per-consumer migration functions.
///
/// Fields marked `// populated when the per-consumer migration lands` are
/// intentionally left at zero-values in compiled-in seeds for this track.
/// Populate them only in the migration track that actually consumes them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct PlaybookMachine {
    /// The artifact kind this playbook governs (e.g., `"track"`, `"proposal"`).
    pub kind: String,
    /// The plural directory name under `forge/` (e.g., `"tracks"`).
    pub directory: String,
    /// The registry file name (e.g., `"tracks.md"`).
    pub registry: String,
    /// The artifact kind that must exist as a parent for instances of this kind.
    /// `None` if no parent is required.
    #[serde(default)]
    pub parent_kind: Option<String>,
    /// Whether begin-create requires a parent when `parent_kind` is declared.
    ///
    /// Existing machines default to requiring their declared parent. Machines
    /// that use parent_kind only as optional provenance opt out explicitly.
    #[serde(default = "default_parent_required")]
    pub parent_required: bool,
    /// One-line description surfaced via `available_types`.
    pub description: String,
    /// Access tags for playbook candidate scoping.
    #[serde(default = "Access::foundation")]
    pub access: Access,
    /// Kit that owns this playbook declaration.
    #[serde(default)]
    pub owner_kit: String,
    /// Declared visibility tier for this playbook.
    #[serde(default)]
    pub visibility: String,
    /// Machine-owned route trigger declarations.
    #[serde(default)]
    pub route: RouteConfig,
    /// Whether this machine writes projection-only events instead of
    /// scaffolding artifact directories and status.yaml files.
    ///
    /// Existing authored machines omit this key; they must continue to
    /// deserialize as normal scaffolded playbooks.
    #[serde(default)]
    pub projection_only: bool,
    /// Fields required by `begin(artifact_type:...)` creation calls.
    #[serde(default)]
    pub required_fields: Vec<FieldDescriptor>,
    /// Role names legal for this playbook (minimum `["doer", "reviewer"]`).
    pub roles: Vec<String>,
    /// State definitions for this lifecycle.
    pub states: Vec<StateDefinition>,
    /// Transition definitions for this lifecycle.
    pub transitions: Vec<TransitionDefinition>,
    /// Whether this machine is a driven playbook (routed-to) or a free
    /// generative kind (invocable out-of-band, never a route target).
    ///
    /// `#[serde(default)]` is mandatory — the struct is `deny_unknown_fields`
    /// and existing authored `machine.yaml` files omit this key; they default
    /// to `Driven`. Free kinds opt in with `register: free`.
    #[serde(default)]
    pub register: Register,
    /// Optional playbook-level success rubric: which quality dimensions matter
    /// for this playbook, how they are weighted, the grader reference, and the
    /// declared lagging signals.
    ///
    /// `#[serde(default)]` is mandatory — `PlaybookMachine` carries
    /// `#[serde(deny_unknown_fields)]`, so every existing `machine.yaml` (which
    /// lacks `success_rubric`) must continue to parse. Validated (dimensions ∈
    /// vocab, weights well-formed) in `loader::validate_with_id`.
    #[serde(default)]
    pub success_rubric: Option<SuccessRubric>,
    /// Optional checkable-fact outcome predicate — did the world-change this
    /// playbook exists to produce actually happen (distinct from HOW WELL, which
    /// is `success_rubric`). Versioned implicitly with the machine.
    ///
    /// `#[serde(default)]` is mandatory — `PlaybookMachine` carries
    /// `#[serde(deny_unknown_fields)]`, so every existing `machine.yaml` (which
    /// lacks `outcome_predicate`) must continue to parse. Validated
    /// (`terminal_state` ∈ machine states) in `loader::validate_with_id`.
    #[serde(default)]
    pub outcome_predicate: Option<OutcomePredicate>,
}

fn default_parent_required() -> bool {
    true
}

impl PlaybookMachine {
    /// Whether this machine is a driven playbook (a router candidate).
    ///
    /// Free kinds (`register: free`) return `false` and are excluded from the
    /// router's candidate set.
    pub fn is_driven(&self) -> bool {
        self.register == Register::Driven
    }
}
