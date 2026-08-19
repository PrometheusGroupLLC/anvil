//! Domain events emitted by `CompleteCommandHandler::execute`.
//!
//! ## CQRS shape
//!
//! `CompleteCommandHandler::execute` (post Phase 2 conversion) is a pure
//! function that reads via `QueryPort` and emits `Vec<CompleteEvent>` rather
//! than calling mutation methods directly. The engine layer routes each event:
//!
//! - `TransitionRecorded` → `SnapshotCommandHandler::execute` (handles
//!   actor seeding + transition append + registry move + projection move
//!   atomically, same as `BeginEvent::ReviewTransition`).
//! - `ActorUpserted` → `ActorWritePort::upsert_actor_configuration`.
//! - `ReflectionWritten` → `ReflectionWritePort::write_reflection_file`.
//!   Emitted only when `reflection_notes` is non-empty.
//!
//! ## Exhaustive match required
//!
//! The engine router (`anvil-engine/src/main.rs`) must exhaustively match
//! all variants — no `_ =>` catch-all. Adding a variant without a
//! corresponding router arm is a compile error.
//!
//! ## Parity with `Event` (begin's variant set)
//!
//! `CompleteEvent` is kept separate from `Event` so new `complete`-specific
//! variants (Slice B `full_revision`, Slice C `address_in_next_step`) can
//! extend without touching `begin`'s variant set. They are parallel types,
//! not a single unified enum.
//!
//! ## Variant granularity — coarse-grained (plan D1 option (a))
//!
//! Registry and projection side effects are not separate top-level variants.
//! `TransitionRecorded` carries all state-bookkeeping intent; the engine router
//! dispatches it to `SnapshotCommandHandler` which atomically composes
//! transition + registry + projection writes. This matches `BeginEvent::ReviewTransition`'s
//! pattern exactly.
//!
//! ## Write ordering (plan D4)
//!
//! Event emission order: `ActorUpserted` → optionally `ReflectionWritten` → `TransitionRecorded`.
//! The engine router iterates `outcome.events` in order, preserving the
//! current handler's write ordering (actor → reflection → status.yaml → registry → projection).

use crate::domain::shared_types::ActorIdentity;

/// A single domain decision emitted by `CompleteCommandHandler::execute`.
///
/// The engine layer pattern-matches on variants to dispatch each event
/// to the correct infrastructure concern. Exhaustive match is required —
/// add match arms when adding variants.
#[derive(Debug, Clone)]
pub enum CompleteEvent {
    /// A state-bookkeeping transition: move the artifact to a new state,
    /// record the transition in status.yaml, move the registry entry,
    /// and move the projection row. The engine router dispatches this to
    /// `SnapshotCommandHandler`, which atomically composes these three writes.
    ///
    /// Actor identity is NOT embedded here (contrast with `BeginEvent::ReviewTransition`).
    /// The actor upsert is a separate `ActorUpserted` event emitted before this one,
    /// matching the current handler's write ordering (actor first, then transition).
    TransitionRecorded {
        artifact_path: String,
        to_state: String,
        at: String,
        role: String,
        approver: Option<String>,
        note: Option<String>,
        /// The actor name for the transition record in status.yaml.
        actor_name: String,
        /// The reviewer satisfaction recorded on the transition metadata.
        /// `Some("address_in_next_step")` distinguishes a carry-forward
        /// acceptance from an outright (`satisfied`) acceptance in the audit
        /// trail (Slice C, R2.2). `None` for every other path — the YAML
        /// emitter omits the line, keeping existing status.yaml byte-identical.
        satisfaction: Option<String>,
    },
    /// A carry-forward file write: the reviewer's verbatim findings, rendered
    /// per the Slice C file-format lock, written to `<track_dir>/carry-forward.md`.
    /// Routes to `SnapshotPort::write_carry_forward`. Emitted only on the
    /// `complete(satisfaction: "address_in_next_step")` path. The engine
    /// records the returned absolute path into `CompleteResult.carry_forward_path`.
    CarryForwardWritten { artifact_path: String, body: String },
    /// An actor upsert: seed or update the actors table entry for this actor.
    /// Routes to `ActorWritePort::upsert_actor_configuration`.
    ActorUpserted {
        artifact_path: String,
        identity: ActorIdentity,
    },
    /// A reflection file write: create a per-actor reflection notes file
    /// in the `<source_state>_reflection/` subdirectory.
    /// Routes to `ReflectionWritePort::write_reflection_file`.
    /// Emitted only when `reflection_notes` is non-empty.
    ReflectionWritten {
        artifact_path: String,
        source_state: String,
        filename: String,
        body: String,
    },
}

impl CompleteEvent {
    /// Returns the `to_state` field if this is a `TransitionRecorded` event,
    /// or `None` otherwise. Convenience accessor for step assertions.
    pub fn transition_to_state(&self) -> Option<&str> {
        match self {
            CompleteEvent::TransitionRecorded { to_state, .. } => Some(to_state.as_str()),
            _ => None,
        }
    }

    /// Returns `true` if this is a `TransitionRecorded` variant.
    pub fn is_transition_recorded(&self) -> bool {
        matches!(self, CompleteEvent::TransitionRecorded { .. })
    }

    /// Returns `true` if this is an `ActorUpserted` variant.
    pub fn is_actor_upserted(&self) -> bool {
        matches!(self, CompleteEvent::ActorUpserted { .. })
    }

    /// Returns `true` if this is a `ReflectionWritten` variant.
    pub fn is_reflection_written(&self) -> bool {
        matches!(self, CompleteEvent::ReflectionWritten { .. })
    }

    /// Returns `true` if this is a `CarryForwardWritten` variant.
    pub fn is_carry_forward_written(&self) -> bool {
        matches!(self, CompleteEvent::CarryForwardWritten { .. })
    }
}
