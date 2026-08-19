//! Append-only writer for the full §0 `StepMeasurementEvent` stream consumed by
//! temper.
//!
//! This is the temper-CONSUMED stream — distinct from the lean booleans-only
//! hearth audit sink (`fs_step_measurement_adapter`). Temper storage is rooted
//! at `~/.temper/` and cannot see hearth paths, so the stream is written at
//! `<temper_home>/.temper/step-measurements/<workflow_kind>/events.jsonl`, one
//! JSON object per line, **append-only** (never rewritten), partitioned by
//! `workflow_kind` (= `track_id`) which gives Scorecard B its per-kind funnel
//! directory for free.
//!
//! ## §0 schema (per line)
//!
//! Required: `playbook_id` (instance id), `track_id` (= workflow_kind),
//! `from_state`, `to_state`, `role`, `actor` (the actor id, NOT a hash), `at`
//! (RFC3339), `intent` (TEXT), `expected_output` (TEXT), `model`. Optional:
//! `tokens`, `duration_ms` (emitted only when the runtime surfaces usage AND the
//! redaction policy permits it). A stable `event_id` =
//! `(playbook_id, from_state→to_state, at, <sub-second/path component>)` lets the
//! sink dedupe replayed/retried transitions without false-deduping same-second
//! retries.
//!
//! ## Privacy
//!
//! The rich record ships ONLY when the `step-measurement-emit-privacy` decision
//! is decided (the gate is the caller's concern). Even then, the redaction
//! policy may omit fields (e.g. `tokens`) — the writer omits redacted fields
//! from the line rather than blanking them.

/// A full §0 step-measurement event destined for the temper-consumed stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step0Event {
    /// Per-instance lifecycle id (the run id). Orders a single run's events.
    pub playbook_id: String,
    /// The playbook KIND (e.g. "track", "lore_query"). Equal to `track_id`; the
    /// per-kind path partition and Scorecard B aggregation key.
    pub track_id: String,
    /// State transitioned FROM. Empty for begin (no prior state).
    pub from_state: String,
    /// State transitioned TO.
    pub to_state: String,
    /// Role label (doer | reviewer | creator | ...).
    pub role: String,
    /// RAW actor id (NOT a salted hash). Permitted by the privacy decision.
    pub actor: String,
    /// The step's declared intent PROSE (machine-author content).
    pub intent: String,
    /// The step's declared expected-output PROSE (machine-author content).
    pub expected_output: String,
    /// RFC3339 transition timestamp.
    pub at: String,
    /// Model id; temper prices usage against it.
    pub model: String,
    /// Token usage — emitted only when the runtime surfaces it AND the policy
    /// permits it. `None` ⇒ the `tokens` key is OMITTED from the line.
    pub tokens: Option<u64>,
    /// Wall-clock duration — emitted only when the runtime surfaces it.
    /// `None` ⇒ the `duration_ms` key is OMITTED from the line.
    pub duration_ms: Option<u64>,
    /// A sub-second/uniqueness component appended to the dedupe identity so a
    /// same-second retry does not false-dedupe. Carried on the line as part of
    /// `event_id`.
    pub event_seq: String,
}

impl Step0Event {
    /// Stable event identity the sink dedupes on:
    /// `<playbook_id>|<from_state>-><to_state>|<at>|<event_seq>`.
    pub fn event_id(&self) -> String {
        format!(
            "{}|{}->{}|{}|{}",
            self.playbook_id, self.from_state, self.to_state, self.at, self.event_seq
        )
    }
}

/// Errors surfaced by the §0 stream writer. Adapter-level I/O only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step0StreamError {
    IoError { message: String },
}

impl std::fmt::Display for Step0StreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Step0StreamError::IoError { message } => write!(f, "I/O error: {}", message),
        }
    }
}

impl std::error::Error for Step0StreamError {}

/// Append-only §0 stream write port. Appends one event line to the per-kind
/// `events.jsonl`, creating the partition directory + file on first write, and
/// deduping on the event's stable identity (a line whose `event_id` already
/// exists in the file is a no-op).
pub trait Step0StreamWritePort: Send + Sync {
    fn append_step0_event(&self, event: &Step0Event) -> Result<(), Step0StreamError>;
}
