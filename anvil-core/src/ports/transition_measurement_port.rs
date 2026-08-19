//! Append-only durable sink for redacted per-transition measurement records.
//!
//! A [`TransitionMeasurementRecord`] carries only machine-derived public labels
//! and hashed/labelled join keys. It never carries raw actor identity, artifact
//! paths, conversation ids, project roots, notes, prose, tokens, or model
//! payloads.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionMeasurementRecord {
    /// Record discriminator. Always `"transition_measurement"`.
    pub kind: String,
    /// Public playbook kind label, e.g. `"track"` or `"lore_query"`.
    pub artifact_kind: String,
    /// The state the transition left. Empty for begin-created initial state.
    pub from_state: String,
    /// The state the transition entered.
    pub to_state: String,
    /// Machine role label for the transition.
    pub role: String,
    /// Review verdict only when the source state is a review gate.
    pub satisfaction: Option<String>,
    /// Outcome label for the transition emit path; successful emits use `"ok"`.
    pub outcome: String,
    /// Whether the transition succeeded.
    pub success: bool,
    /// ISO-8601 timestamp stamped by the engine at transition time.
    pub at: String,
    /// Salted, non-reversible hash of the surface conversation/session id.
    pub conversation_hash: Option<String>,
    /// Basename/registered label of the caller's project root.
    pub project_label: Option<String>,
    /// Opaque per-run artifact instance id tying transition records to a playbook run.
    pub playbook_run_id: Option<String>,
    /// Whether an evidence CLAIM accompanied the call that produced this
    /// transition — and NOTHING about whether an artifact exists. The claim is
    /// opaque; this port never dereferences it.
    ///
    /// Named for what it is. An earlier draft called it `evidence_status`, which
    /// invited exactly the misreading that started this track: `absent` was read
    /// as "no artifact", when measured on the live fleet it was 73 unclaimed
    /// against 8 genuine abandonments — a ~9:1 false-positive rate.
    ///
    /// `claimed | self_described | unclaimed | pending | not_applicable`.
    /// Optional on read so records written before this field decode unchanged.
    pub claimed_evidence_status: Option<String>,
    /// What the artifact of record looked like at transition time:
    /// `placeholder | substantive | missing | not_applicable`.
    ///
    /// The reader's predicate is a CONJUNCTION — unclaimed AND placeholder — and
    /// a claim-only field cannot express its second half. Shipping just the
    /// claim would have let the consumer either reject every unclaimed
    /// transition (re-creating the 73 false positives) or none.
    ///
    /// Never the string `absent`: that value is in the downstream scorer's
    /// FORBIDDEN_VALUES and would fail its frozen validation on every record.
    pub artifact_assessment: Option<String>,
}

pub const TRANSITION_MEASUREMENT_KIND: &str = "transition_measurement";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionMeasurementError {
    IoError { message: String },
    MalformedRecord { message: String },
}

impl fmt::Display for TransitionMeasurementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransitionMeasurementError::IoError { message } => write!(f, "I/O error: {}", message),
            TransitionMeasurementError::MalformedRecord { message } => {
                write!(f, "Malformed transition-measurement record: {}", message)
            }
        }
    }
}

impl std::error::Error for TransitionMeasurementError {}

pub trait TransitionMeasurementWritePort: Send + Sync {
    fn append_transition_measurement(
        &self,
        record: &TransitionMeasurementRecord,
    ) -> Result<(), TransitionMeasurementError>;
}

pub trait TransitionMeasurementReadPort: Send + Sync {
    fn read_transition_measurements(
        &self,
    ) -> Result<Vec<TransitionMeasurementRecord>, TransitionMeasurementError>;
}
