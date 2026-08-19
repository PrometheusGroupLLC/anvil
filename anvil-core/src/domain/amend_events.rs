//! Domain events emitted by `AmendCommandHandler::execute` (B5b).
//!
//! ## CQRS shape
//!
//! `AmendCommandHandler::execute` is a pure function that reads via `QueryPort`
//! + a `&dyn PlaybookRegistry`, validates the candidate op against the kind's
//! schema + the replayed op log, and emits `Vec<AmendEvent>` rather than calling
//! mutation ports directly. The engine layer routes each event (BP3):
//!
//! - `OpRecorded`        → `OpLogWritePort::append_op` (always emitted on success).
//! - `ActorUpserted`     → `ActorWritePort::upsert_actor_configuration` (always).
//! - `TransitionRecorded` → `SnapshotPort::append_transition` directly — emitted
//!   ONLY when the artifact's lifecycle-kind machine declares a
//!   `<current_state> → amend` edge and the artifact is in that source state
//!   (BP4: track-in-`completed`). The variant is DEFINED in BP2 but the handler
//!   does not emit it until the track seed gains the edge (BP4).
//!
//! ## Exhaustive match required
//!
//! The engine router (BP3) must exhaustively match all variants — no `_ =>`
//! catch-all. Adding a variant without a router arm is a compile error.

use crate::domain::amendment::OpLogEntry;
use crate::domain::shared_types::ActorIdentity;

/// A single domain decision emitted by `AmendCommandHandler::execute`.
#[derive(Debug, Clone)]
pub enum AmendEvent {
    /// The accepted op + its engine-stamped identity/ordering fields, to be
    /// appended to the per-document op-log file. Routes to `OpLogWritePort`.
    OpRecorded {
        artifact_path: String,
        target_document: String,
        entry: OpLogEntry,
    },
    /// An actor upsert: seed or update the actors table entry for this actor.
    /// Routes to `ActorWritePort::upsert_actor_configuration`. Mirrors complete.
    ActorUpserted {
        artifact_path: String,
        identity: ActorIdentity,
    },
    /// A state-bookkeeping transition into `amend`. Routes to
    /// `SnapshotPort::append_transition` directly (interpreter-validated before
    /// the port write). Emitted only for a machine-declared amend edge (BP4).
    TransitionRecorded {
        artifact_path: String,
        to_state: String,
        at: String,
        role: String,
        actor_name: String,
    },
}

impl AmendEvent {
    /// Returns `true` if this is an `OpRecorded` variant.
    pub fn is_op_recorded(&self) -> bool {
        matches!(self, AmendEvent::OpRecorded { .. })
    }

    /// Returns `true` if this is an `ActorUpserted` variant.
    pub fn is_actor_upserted(&self) -> bool {
        matches!(self, AmendEvent::ActorUpserted { .. })
    }

    /// Returns `true` if this is a `TransitionRecorded` variant.
    pub fn is_transition_recorded(&self) -> bool {
        matches!(self, AmendEvent::TransitionRecorded { .. })
    }
}
