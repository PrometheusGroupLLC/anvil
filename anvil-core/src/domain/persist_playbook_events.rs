//! Domain event emitted by `PersistPlaybookCommandHandler::execute` (track 1a).
//!
//! ## CQRS shape
//!
//! `PersistPlaybookCommandHandler::execute` is a pure function that reads via a
//! `&dyn PlaybookRegistry` (for content-aware duplicate-kind detection),
//! loader-validates the candidate machine, and emits `Vec<PersistPlaybookEvent>`
//! rather than calling the mutation port directly. The engine layer routes each
//! event (BP3):
//!
//! - `PlaybookPersisted` → `ArtifactPort::persist_generated_playbook`.
//!
//! The event is emitted only after request validation and the handler's
//! content-aware fast path pass. A loader-invalid machine or a duplicate already
//! visible during preflight produces no event. A concurrent writer can still win
//! after preflight; the filesystem adapter is therefore the authoritative
//! collision gate and must refuse different or invalid existing bytes before the
//! engine reports success. An identical preexisting machine returns success as
//! an idempotent no-op.
//!
//! ## Exhaustive match required
//!
//! The engine router (BP3) must exhaustively match all variants — no `_ =>`
//! catch-all. Adding a variant without a router arm is a compile error.

use crate::domain::playbook::candidate::GeneratedExemplarFile;

/// A single domain decision emitted by `PersistPlaybookCommandHandler::execute`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersistPlaybookEvent {
    /// A validated playbook machine to persist into `owner_home`. Routes to
    /// `ArtifactPort::persist_generated_playbook`.
    PlaybookPersisted {
        owner_home: String,
        kind: String,
        machine_yaml: String,
        /// Hook files (filename, content) written under `hooks/` beside
        /// `machine.yaml`. Empty = machine.yaml-only persist.
        hooks: Vec<(String, String)>,
        exemplars: Vec<GeneratedExemplarFile>,
    },
}
