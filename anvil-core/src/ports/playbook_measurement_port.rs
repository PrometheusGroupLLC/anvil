//! Append-only durable sink for redacted whole-playbook measurement records.
//!
//! A [`PlaybookMeasurementRecord`] is emitted only when a playbook instance
//! enters a machine-declared terminal state. It carries only public playbook
//! labels and hashed/labelled join keys; it never carries raw actor identity,
//! artifact paths, conversation ids, project roots, notes, prose, tokens, or
//! model payloads.

use std::fmt;

/// Whether a quality vector was produced at completion (leading) or from
/// outcome-over-time signals (lagging). Terminal emit is `Leading` by
/// construction; lagging recalibration lands in a later phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QualitySignal {
    /// Judged at completion (structured review + judge + deterministic proxies).
    #[default]
    Leading,
    /// Outcome over time (churn/revert/adoption/downstream quality).
    Lagging,
}

impl QualitySignal {
    /// Stable snake_case label used in the durable sink.
    pub fn as_str(&self) -> &'static str {
        match self {
            QualitySignal::Leading => "leading",
            QualitySignal::Lagging => "lagging",
        }
    }
}

/// One graded quality dimension score within a [`PlaybookMeasurementRecord`]'s
/// quality vector. `score` is conventionally 0.0..=1.0. Empty until a grader is
/// wired — the record SHAPE is this phase's deliverable, not the scoring.
#[derive(Debug, Clone, PartialEq)]
pub struct DimensionScore {
    /// Quality dimension identifier (from the shared vocabulary).
    pub dimension: String,
    /// Graded score for this dimension.
    pub score: f64,
}

// NOTE: `Eq` is intentionally NOT derived below — the quality-vector fields
// carry `f64` scores. `PartialEq` is sufficient for the tests/comparisons this
// record participates in (it is never a hash-map key nor a set member).
#[derive(Debug, Clone, PartialEq)]
pub struct PlaybookMeasurementRecord {
    /// Record discriminator. Always `"playbook_measurement"`.
    pub kind: String,
    /// Public playbook kind label, e.g. `"track"` or `"lore_query"`.
    pub artifact_kind: String,
    /// Opaque per-run artifact instance id tying the playbook record to one run.
    pub playbook_run_id: Option<String>,
    /// Machine-declared terminal state reached by this playbook instance.
    pub terminal_state: String,
    /// Whether the entered state was declared terminal by the playbook machine.
    pub terminal_reached: bool,
    /// Outcome label for the terminal emit path; successful terminal emits use
    /// `"terminal_reached"`.
    pub outcome: String,
    /// The completion FLOOR: whether the playbook instance reached a terminal
    /// state. This is NEVER the quality measure — "reached terminal" only means
    /// it finished, not that it finished *well*. The quality vector below
    /// carries the "done well" signal. Kept distinct by design.
    pub success: bool,
    /// Per-dimension leading-quality scores. EMPTY in this phase — no grader is
    /// wired yet; the record shape is the deliverable, scoring lands later.
    pub quality_dimension_scores: Vec<DimensionScore>,
    /// Weighted overall leading-quality score. `None` until a grader is wired.
    pub quality_overall: Option<f64>,
    /// Identifier of the grader/source that produced the scores. `None` for now.
    pub quality_grader: Option<String>,
    /// Whether this quality vector is a leading or lagging signal. Terminal
    /// emit is `Leading` by construction.
    pub quality_signal: QualitySignal,
    /// ISO-8601 timestamp stamped by the engine at transition time.
    pub at: String,
    /// Salted, non-reversible hash of the surface conversation/session id.
    pub conversation_hash: Option<String>,
    /// Basename/registered label of the caller's project root.
    pub project_label: Option<String>,
    /// Content-derived version of the playbook (PlaybookMachine) that produced
    /// this instance — temper's experiment unit (`playbook_success_rubric_model`
    /// Amendment 1 addendum). A stable content hash of the machine definition,
    /// so it changes exactly when the machine changes and can't be forgotten.
    /// `None` when the machine was not resolvable at emit (fail-open).
    ///
    /// Additive + backward-compatible: this record is hand-serialized (no serde
    /// derive), so the `#[serde(default)]` discipline is realized in the fs
    /// adapter — the field is emitted only when `Some`, and the read path
    /// defaults it to `None` for older lines that lack it.
    pub playbook_version: Option<String>,
}

pub const PLAYBOOK_MEASUREMENT_KIND: &str = "playbook_measurement";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaybookMeasurementError {
    IoError { message: String },
    MalformedRecord { message: String },
}

impl fmt::Display for PlaybookMeasurementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlaybookMeasurementError::IoError { message } => write!(f, "I/O error: {}", message),
            PlaybookMeasurementError::MalformedRecord { message } => {
                write!(f, "Malformed playbook-measurement record: {}", message)
            }
        }
    }
}

impl std::error::Error for PlaybookMeasurementError {}

pub trait PlaybookMeasurementWritePort: Send + Sync {
    fn append_playbook_measurement(
        &self,
        record: &PlaybookMeasurementRecord,
    ) -> Result<(), PlaybookMeasurementError>;
}

pub trait PlaybookMeasurementReadPort: Send + Sync {
    fn read_playbook_measurements(
        &self,
    ) -> Result<Vec<PlaybookMeasurementRecord>, PlaybookMeasurementError>;
}
