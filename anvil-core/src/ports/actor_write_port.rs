//! Uniform actor-write port — owns the three-leg rule for adding,
//! reusing, or extending an actor's configurations entry on a single
//! artifact.
//!
//! Both `BeginCommandHandler` and `SnapshotCommandHandler` depend on
//! this port. The rule is implemented exactly once (per adapter) so the
//! semantics cannot drift between the two callers. See spec R4 of the
//! checkin_backfill_spec_context track.

use crate::domain::shared_types::ActorIdentity;
use std::fmt;

/// Errors surfaced by [`ActorWritePort`]. Adapter-level I/O and parse
/// failures only — no user-facing validation lives here, since callers
/// (the command handlers) own argument validation before invoking the
/// port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorWriteError {
    IoError {
        message: String,
    },
    MalformedStatus {
        artifact_path: String,
        message: String,
    },
    /// The target artifact directory or its `status.yaml` is missing.
    /// Distinct from `IoError` so callers (handlers) can map it to
    /// their own NotFound semantics — gRPC NOT_FOUND for the engine.
    NotFound {
        artifact_path: String,
    },
}

impl fmt::Display for ActorWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActorWriteError::IoError { message } => write!(f, "I/O error: {}", message),
            ActorWriteError::MalformedStatus {
                artifact_path,
                message,
            } => write!(
                f,
                "Malformed status.yaml for '{}': {}",
                artifact_path, message
            ),
            ActorWriteError::NotFound { artifact_path } => {
                write!(f, "Artifact '{}' not found", artifact_path)
            }
        }
    }
}

impl std::error::Error for ActorWriteError {}

/// Uniform actor-write port. The single method `upsert_actor_configuration`
/// implements the three-leg rule:
///
/// 1. **add-if-absent** — actor name absent from the target artifact's
///    actors table → write a new actor record with a single-entry
///    `configurations` list reflecting the call's `actor_*` params.
/// 2. **match-no-op** — actor name present and the call's params match
///    the latest stored configuration → no-op (no mutation).
/// 3. **mismatch-append** — actor name present but the call's params
///    differ from the latest stored configuration → append a new
///    configuration entry (current timestamp + new values). Existing
///    entries are preserved unchanged.
///
/// Implementations must apply the rule atomically per call. The port
/// itself does no input validation — callers (command handlers) are
/// responsible for rejecting empty `actor_name` and empty required
/// runtime params before invoking.
pub trait ActorWritePort: Send + Sync {
    fn upsert_actor_configuration(
        &self,
        artifact_path: &str,
        identity: &ActorIdentity,
    ) -> Result<(), ActorWriteError>;
}
