//! Append-only durable sink for redacted per-step measurement records.
//!
//! The engine emits a `step_measurement` *tracing* event on every lifecycle
//! transition (begin/checkin/complete), but that sink is stderr — not
//! queryable. Temper needs a durable, aggregatable source of per-step
//! measurement coverage (was a measurement — intent + expected_output —
//! actually declared for the step the actor just completed?), so this narrow
//! port persists ONE redacted record per completed step and reads them back for
//! aggregation.
//!
//! ## Redaction (Part-3 allowlist)
//!
//! A [`StepMeasurementRecord`] carries ONLY the allowlisted fields — the record
//! `kind` (always `"step_measurement"`), the `from_state`/`to_state` transition
//! labels, the `role` (the selected measurement-role label, such as doer,
//! reviewer, or complete), two BOOLEANS
//! capturing whether an `intent` and an `expected_output` were declared for the
//! step, an ISO-8601 `at` timestamp, and optional hashed/labelled correlation
//! keys. An obligated DRIVEN step may additionally carry the structural
//! assessment, missing class tokens, caller-supplied opaque references, and a
//! content-derived playbook version. It NEVER copies the intent or
//! expected_output PROSE, surface message text, project/artifact paths,
//! identities, tokens, or any other raw request field. The booleans answer "was
//! a measurement declared" without leaking its content. The write is a typed
//! event + variant-name log, never message text.
//!
//! Two single-responsibility traits keep the CQRS split clean: the complete
//! path holds the write port; a Temper-side query (the scorer) reads the file
//! directly and need not hold the read port — but the read port exists so the
//! contract is symmetric and brine-verifiable in-process.

use crate::domain::playbook::evidence_obligation::EvidenceAssessment;
use crate::domain::shared_types::ClaimedEvidence;
use std::fmt;

/// Evidence-only extension carried by an obligated DRIVEN step measurement.
///
/// Keeping the four persisted evidence fields behind one `Option` makes their
/// all-or-nothing contract structural: FREE steps, steps without an obligation,
/// and legacy records cannot accidentally retain claims or a version stamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepEvidenceRecord {
    pub assessment: EvidenceAssessment,
    /// Caller claims in request order. Each reference remains opaque: it is
    /// copied and escaped for storage, never interpreted or dereferenced.
    pub claimed_evidence: Vec<ClaimedEvidence>,
    /// Content-derived version of the exact registry machine whose obligation
    /// was assessed.
    pub playbook_version: String,
}

/// A redacted, append-only per-step measurement record.
///
/// Every field is on the Part-3 allowlist. There is intentionally no field for
/// the intent/expected_output prose, message text, actor identity, paths, or
/// token counts. `intent_present`/`expected_output_present` are BOOLEANS — they
/// record that a measurement was declared for the step, never what it said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepMeasurementRecord {
    /// Record discriminator. Always `"step_measurement"` — the redaction
    /// constant. Kept for back-compat; the real, public playbook kind lives in
    /// `artifact_kind`.
    pub kind: String,
    /// The state the step transitioned FROM. Empty for begin (no prior state).
    pub from_state: String,
    /// The state the step transitioned TO.
    pub to_state: String,
    /// Selected measurement-role label (for example doer, reviewer, complete).
    pub role: String,
    /// Whether an `intent` was declared for this (state, role) step.
    pub intent_present: bool,
    /// Whether an `expected_output` was declared for this (state, role) step.
    pub expected_output_present: bool,
    /// ISO-8601 timestamp stamped by the engine at transition time.
    pub at: String,
    /// The REAL playbook kind (e.g. "track", "lore_query") — the same public
    /// kind the routing-activity sink records. Additive: legacy records lacking
    /// this field read as empty (`String::default()`), and the step-volume fold
    /// falls back to `kind` when this is empty. NOT a privacy leak.
    pub artifact_kind: String,
    /// Salted, non-reversible, truncated SHA-256 of the actor name.
    /// `None` when no salt is configured (fail-safe: never emit a raw or
    /// unsalted actor). Legacy records lacking this field read as `None`.
    pub actor_hash: Option<String>,
    /// Salted, non-reversible hash of the surface conversation/session id.
    /// Missing on legacy/unjoinable records; raw ids are never persisted.
    pub conversation_hash: Option<String>,
    /// Basename/registered label of the caller's project root. Missing on
    /// legacy/unattributed records; full paths are never persisted.
    pub project_label: Option<String>,
    /// Opaque per-run artifact instance id tying begin/step/complete records to
    /// the same playbook run. Missing on legacy records.
    pub playbook_run_id: Option<String>,
    /// Optional, all-or-nothing evidence assessment extension. `None` preserves
    /// the exact legacy serialized row shape.
    pub evidence: Option<StepEvidenceRecord>,
}

/// The canonical `kind` value for every step-measurement record.
pub const STEP_MEASUREMENT_KIND: &str = "step_measurement";

/// Errors surfaced by the step-measurement ports. Adapter-level I/O and parse
/// failures only — callers own argument validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepMeasurementError {
    IoError { message: String },
    MalformedRecord { message: String },
}

impl fmt::Display for StepMeasurementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StepMeasurementError::IoError { message } => write!(f, "I/O error: {}", message),
            StepMeasurementError::MalformedRecord { message } => {
                write!(f, "Malformed step-measurement record: {}", message)
            }
        }
    }
}

impl std::error::Error for StepMeasurementError {}

/// Append-only write port. Appends one redacted record to the durable sink,
/// creating it on first write. Implementations must apply the append atomically
/// per call.
pub trait StepMeasurementWritePort: Send + Sync {
    fn append_step_measurement(
        &self,
        record: &StepMeasurementRecord,
    ) -> Result<(), StepMeasurementError>;
}

/// Read port. Returns every recorded record in append order. A missing sink
/// reads as an empty stream (no error) — coverage is simply zero.
pub trait StepMeasurementReadPort: Send + Sync {
    fn read_step_measurements(&self) -> Result<Vec<StepMeasurementRecord>, StepMeasurementError>;
}
