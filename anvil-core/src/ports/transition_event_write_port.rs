//! Narrow append-only port for an artifact's per-event transition log.
//!
//! Mirrors [`crate::ports::op_log_write_port::OpLogWritePort`]'s narrow
//! single-responsibility shape, but takes the per-file directory pattern one
//! step further: every transition is its OWN file under
//! `<artifact_path>/transitions/`, so two concurrent writers NEVER touch the
//! same file (the conflict-free invariant — spark-20260412-001). The op-log
//! port re-serializes a shared array; this port does NOT — there is no shared
//! array, only one-file-per-event.
//!
//! The filename is derived ONLY from the event's own content plus a random
//! component (`<iso8601-timestamp>_<actor>_<short-id>.yaml`) — NEVER from a
//! count of existing siblings. Counting siblings would require reading the
//! directory and reintroduce the merge conflict this port exists to remove.
//!
//! The record is serialized via serde (not line templating), so `"`/`\`
//! escaping in the optional note is structural (spark-20260503-002).

use serde::{Deserialize, Serialize};
use std::fmt;

/// One transition event, the full content persisted to a single event file.
/// Field order is the canonical YAML order. `approver`/`note` are omitted
/// from the serialized form when absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransitionRecord {
    pub to: String,
    pub at: String,
    pub actor: String,
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approver: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Reviewer satisfaction recorded on the transition (Slice C carry-forward).
    /// `Some("address_in_next_step")` distinguishes carry-forward from outright
    /// acceptance; omitted from the serialized form when absent so existing
    /// transition event files stay byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub satisfaction: Option<String>,
    /// Machine-readable event-type discriminator. `Some("adoption")` marks a
    /// governance-adoption reset so the fold surfaces it on `StatusTransition`
    /// and `has_open_begin` can skip it (see `TransitionContent::event_type`).
    /// Omitted from the serialized form when absent so existing transition event
    /// files stay byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_type: Option<String>,
}

/// Errors surfaced by [`TransitionEventWritePort`]. Adapter-level I/O and
/// parse failures only — callers own argument validation before invoking the
/// port. Mirrors [`crate::ports::op_log_write_port::OpLogWriteError`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionEventWriteError {
    IoError { message: String },
}

impl fmt::Display for TransitionEventWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransitionEventWriteError::IoError { message } => {
                write!(f, "I/O error: {}", message)
            }
        }
    }
}

impl std::error::Error for TransitionEventWriteError {}

/// Append-only transition-event write port. `append_transition_event` writes
/// ONE new file under `<artifact_path>/transitions/` (creating the directory
/// if absent) and touches nothing else. Implementations apply the write
/// atomically per call.
pub trait TransitionEventWritePort: Send + Sync {
    fn append_transition_event(
        &self,
        artifact_path: &str,
        record: &TransitionRecord,
    ) -> Result<(), TransitionEventWriteError>;
}

/// The per-artifact transitions event directory name. Single-sourced so the
/// fs write adapter and the fold read path agree.
pub const TRANSITIONS_DIR: &str = "transitions";

/// Derive a conflict-free event filename — NEVER from a sibling count. The
/// name LEADS with a high-resolution, write-time timestamp prefix
/// (`hi_res_prefix`, e.g. nanosecond `Utc::now()`), then the actor, then a
/// random suffix. Two independent writers get distinct names because of the
/// random suffix (R2); the high-res prefix gives a stable causal tiebreak when
/// two records share the same second-granularity `at` field (R4) — so the fold
/// is deterministic even for back-to-back transitions in the same second,
/// WITHOUT reading sibling files.
pub fn event_file_name(
    hi_res_prefix: &str,
    record: &TransitionRecord,
    random_suffix: &str,
) -> String {
    let safe_prefix = sanitize_segment(hi_res_prefix);
    let safe_actor = sanitize_segment(&record.actor);
    format!("{}_{}_{}.yaml", safe_prefix, safe_actor, random_suffix)
}

fn sanitize_segment(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | ' ' => '-',
            other => other,
        })
        .collect()
}
