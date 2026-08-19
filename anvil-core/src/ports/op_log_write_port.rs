//! Narrow append-only port for an artifact's structured per-document op log.
//!
//! Mirrors [`crate::ports::activity_write_port::ActivityWritePort`]'s narrow
//! single-responsibility shape: it appends one [`OpLogEntry`] to the target
//! document's op-log file `<artifact_path>/<target_document>.amendments.yaml`
//! and touches nothing else. Unlike the activity port (which line-edits a
//! shared status.yaml to preserve foreign sections), the op-log file is WHOLLY
//! owned by this port, so the adapter re-serializes the whole [`OpLog`] —
//! `#[serde(transparent)]` makes that round-trip a clean YAML array.
//!
//! The engine routes `AmendEvent::OpRecorded` to this port; the entry carries
//! engine-stamped `op_id` / `accepted_at` / `seq` (see `domain::amend`), so the
//! adapter preserves those fields verbatim (`OpLog::push_entry`, no re-numbering).

use crate::domain::amendment::OpLogEntry;
use std::fmt;

/// Errors surfaced by [`OpLogWritePort`]. Adapter-level I/O and parse failures
/// only — callers own argument validation before invoking the port. Mirrors
/// [`crate::ports::activity_write_port::ActivityWriteError`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpLogWriteError {
    IoError {
        message: String,
    },
    /// The existing op-log file exists but cannot be parsed as an [`OpLog`].
    MalformedOpLog {
        artifact_path: String,
        target_document: String,
        message: String,
    },
}

impl fmt::Display for OpLogWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpLogWriteError::IoError { message } => write!(f, "I/O error: {}", message),
            OpLogWriteError::MalformedOpLog {
                artifact_path,
                target_document,
                message,
            } => write!(
                f,
                "Malformed op log for '{}' document '{}': {}",
                artifact_path, target_document, message
            ),
        }
    }
}

impl std::error::Error for OpLogWriteError {}

/// Append-only op-log write port. `append_op` appends one [`OpLogEntry`] to the
/// per-document op-log file (creating it if absent), preserving the entry's
/// `op_id` / `accepted_at` / `seq` verbatim. Implementations apply the append
/// atomically per call.
pub trait OpLogWritePort: Send + Sync {
    fn append_op(
        &self,
        artifact_path: &str,
        target_document: &str,
        entry: &OpLogEntry,
    ) -> Result<(), OpLogWriteError>;
}

/// The op-log file name for a target document: `<target_document>.amendments.yaml`
/// (D-4). Shared by the fs adapter and the QueryPort read so the naming stays
/// single-sourced.
pub fn op_log_file_name(target_document: &str) -> String {
    format!("{}.amendments.yaml", target_document)
}
