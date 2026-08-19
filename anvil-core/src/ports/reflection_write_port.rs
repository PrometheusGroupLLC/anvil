use crate::domain::complete::CompleteError;
use std::fmt;

/// Errors from reflection-file write operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReflectionWriteError {
    /// A general I/O failure. `path` is the attempted file path;
    /// `message` is the underlying error description.
    IoError { path: String, message: String },
}

impl fmt::Display for ReflectionWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReflectionWriteError::IoError { path, message } => {
                write!(f, "reflection write I/O error at '{}': {}", path, message)
            }
        }
    }
}

impl std::error::Error for ReflectionWriteError {}

impl From<ReflectionWriteError> for CompleteError {
    fn from(e: ReflectionWriteError) -> Self {
        match e {
            ReflectionWriteError::IoError { path, message } => {
                CompleteError::ReflectionWriteFailed {
                    path,
                    io_error: message,
                }
            }
        }
    }
}

/// Narrow port for writing per-phase reflection files.
///
/// Separated from `SnapshotPort` (state/registry/projection bookkeeping)
/// and `ArtifactPort` (review-doc header + track scaffolding) because
/// reflection writes are pure durable storage of actor-supplied narrative —
/// they are neither state-machine artefacts nor scaffolding. A dedicated
/// port keeps this concern isolated and provides a narrow extension point
/// if Slice C's resolving track later unifies carry-forward.md with
/// reflection_notes.
///
/// The caller computes `filename` to keep the adapter a pure byte-sink.
pub trait ReflectionWritePort: Send + Sync {
    /// Write a reflection file at `<artifact_path>/<source_state>_reflection/<filename>`.
    ///
    /// `artifact_path` — relative path to the artifact directory under the hearth
    ///   (e.g., `"tracks/20260420T0210_complete_reflection_notes"`).
    /// `source_state` — the state the artifact was in when `complete` was called
    ///   (e.g., `"spec"`, `"spec_review"`).
    /// `filename` — caller-computed filename (e.g., `"20260420T040000Z-Doer-123456.md"`).
    ///   Keeping filename computation in the handler (not the adapter) ensures
    ///   the same instant is used for both the `transition_at` timestamp and
    ///   the compact-basic filename timestamp.
    /// `body` — the fully-rendered file contents (frontmatter + blank line + trimmed
    ///   notes + trailing newline). The adapter writes the bytes as-is.
    ///
    /// Returns the absolute path to the written file on success.
    fn write_reflection_file(
        &self,
        artifact_path: &str,
        source_state: &str,
        filename: &str,
        body: &str,
    ) -> Result<String, ReflectionWriteError>;
}
