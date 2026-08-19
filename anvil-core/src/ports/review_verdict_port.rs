//! Append-only durable sink for structured review verdicts.
//!
//! A [`ReviewVerdictRecord`] is captured when a playbook instance leaves a
//! review-gate state (the reviewer's `complete` with a satisfaction). It carries
//! the light structured verdict decided in `playbook_success_rubric_model`: the
//! reviewer keeps their honest verdict + prose findings; the ONLY added
//! structure is a dimension tag + severity per finding, and — at the FINAL/E2E
//! gate only — one holistic "intent well accomplished" confidence.
//!
//! It is keyed by the SAME salted `conversation_hash` / `playbook_run_id`
//! the other measurement sinks use, so it joins with the playbook/step
//! measurement records. It carries only public labels + hashed join keys; never
//! raw actor identity, artifact paths, conversation ids, project roots, prose,
//! tokens, or model payloads.
//!
//! This is a NEW dedicated sink (not an overload of the activity log) per the
//! plan. In this phase the engine captures the SHAPE (gate/final-gate labels,
//! satisfaction, join keys); `findings` and `intent_confidence` are populated by
//! later phases (structured review authoring lands with the generator work).

use std::fmt;

/// One dimension-tagged, severity-rated finding from a review verdict.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VerdictFinding {
    /// Quality dimension this finding concerns (from the shared vocabulary).
    pub dimension: String,
    /// Severity label for the finding (e.g. `"blocker"`, `"minor"`).
    pub severity: String,
    /// The phase this finding is ATTRIBUTED to by the FINDER (not the author) —
    /// the step where the defect originated (`"spec"`, `"plan"`, `"impl"`). When
    /// this differs from the phase of the gate that recorded the finding, the
    /// defect ESCAPED its origin gate (phase-containment/escape, decision
    /// Amendment 1 item 8). `None` when the finder did not attribute an origin.
    pub origin_phase: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewVerdictRecord {
    /// Record discriminator. Always `"review_verdict"`.
    pub kind: String,
    /// Public playbook kind label, e.g. `"track"`.
    pub artifact_kind: String,
    /// Opaque per-run artifact instance id joining this verdict to one run and
    /// to the playbook/step measurement records.
    pub playbook_run_id: Option<String>,
    /// The review-gate state the verdict was rendered at.
    pub gate_state: String,
    /// The reviewer's honest verdict label (the satisfaction value).
    pub satisfaction: String,
    /// Whether this is the final/E2E gate (a gate with an outgoing transition
    /// into a terminal state). The holistic confidence is captured only here.
    pub is_final_gate: bool,
    /// Dimension-tagged findings + severity. Empty in this phase (the shape is
    /// the deliverable; structured findings are authored later).
    pub findings: Vec<VerdictFinding>,
    /// Holistic "intent well accomplished" confidence — present ONLY at the
    /// final/E2E gate, `None` otherwise. Empty string in this phase (captured
    /// later); the field's PRESENCE at the final gate is the deliverable.
    pub intent_confidence: Option<String>,
    /// Outcome label for the capture path; successful captures use `"ok"`.
    pub outcome: String,
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

pub const REVIEW_VERDICT_KIND: &str = "review_verdict";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewVerdictError {
    IoError { message: String },
    MalformedRecord { message: String },
}

impl fmt::Display for ReviewVerdictError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReviewVerdictError::IoError { message } => write!(f, "I/O error: {}", message),
            ReviewVerdictError::MalformedRecord { message } => {
                write!(f, "Malformed review-verdict record: {}", message)
            }
        }
    }
}

impl std::error::Error for ReviewVerdictError {}

pub trait ReviewVerdictWritePort: Send + Sync {
    fn append_review_verdict(&self, record: &ReviewVerdictRecord)
        -> Result<(), ReviewVerdictError>;
}

pub trait ReviewVerdictReadPort: Send + Sync {
    fn read_review_verdicts(&self) -> Result<Vec<ReviewVerdictRecord>, ReviewVerdictError>;
}
