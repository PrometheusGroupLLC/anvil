//! Narrow append-only port for the artifact's `activity:` log.
//!
//! The begin-marker write is an append analogous to `ActorWritePort`'s
//! actor upsert, but with a distinct single responsibility: it never
//! touches `actors:`/`transitions:` and never records a state transition.
//! It appends one [`ActivityEntry`] to the target artifact's status.yaml
//! `activity:` block (creating the block if absent). A *new* narrow port —
//! rather than extending `ActorWritePort` — keeps the append-only activity
//! concern from muddying the three-leg actor-upsert contract.
//!
//! Implementations own the atomic read-modify-write of status.yaml; the
//! engine routes `Event::BeginMarkerWritten` to this port under the
//! per-hearth begin lock (no re-lock).

use crate::domain::shared_types::ActivityEntry;
use std::fmt;

/// Errors surfaced by [`ActivityWritePort`]. Adapter-level I/O and parse
/// failures only — callers (the command handlers / engine) own argument
/// validation before invoking the port. Mirrors [`crate::ports::actor_write_port::ActorWriteError`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivityWriteError {
    IoError {
        message: String,
    },
    MalformedStatus {
        artifact_path: String,
        message: String,
    },
    /// The target artifact directory or its `status.yaml` is missing.
    NotFound {
        artifact_path: String,
    },
}

impl fmt::Display for ActivityWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActivityWriteError::IoError { message } => write!(f, "I/O error: {}", message),
            ActivityWriteError::MalformedStatus {
                artifact_path,
                message,
            } => write!(
                f,
                "Malformed status.yaml for '{}': {}",
                artifact_path, message
            ),
            ActivityWriteError::NotFound { artifact_path } => {
                write!(f, "Artifact '{}' not found", artifact_path)
            }
        }
    }
}

impl std::error::Error for ActivityWriteError {}

/// Append-only `activity:` write port. The single method
/// `append_activity` appends one entry to the artifact's `activity:` list
/// (creating the list if absent), preserving every other section of
/// status.yaml unchanged. Implementations must apply the append
/// atomically per call.
pub trait ActivityWritePort: Send + Sync {
    fn append_activity(
        &self,
        artifact_path: &str,
        entry: &ActivityEntry,
    ) -> Result<(), ActivityWriteError>;
}
