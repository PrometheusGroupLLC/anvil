//! Domain events for the begin handler's CQRS output.
//!
//! ## CQRS shape
//!
//! The begin handler (`BeginCommandHandler::execute`) is a pure function
//! that reads via `QueryPort` and emits `Vec<Event>` rather than calling
//! mutation methods directly. The engine layer routes each event:
//!
//! - `ReviewTransition` → `SnapshotCommandHandler::execute` (handles
//!   actor seeding + transition append + registry move + projection move
//!   atomically)
//! - `ReviewDocCreated` → `ArtifactPort::create_review_doc`
//! - `ArtifactCreation` → `ArtifactPort::scaffold_artifact_directory` (creates
//!   the machine-derived directory + initial status.yaml + an optional
//!   placeholder doc, e.g. spec.md for a track)
//!   followed by a `SnapshotRequest` dispatched through
//!   `SnapshotCommandHandler::execute` (appends first transition,
//!   creates registry entry, inserts execution projection row). See
//!   Amendment 2 of the core_cqrs_separation track.
//!
//! ## When to add a new event variant
//!
//! Add a variant when the handler makes a **domain decision** that
//! results in a mutation. Pure reads never produce events.
//! `ReviewTransition` covers state-bookkeeping mutations (status.yaml,
//! registry, projection); `ReviewDocCreated` covers artifact-file
//! creation. Future handlers (plan, implement, complete) add their own
//! variants as needed — this enum is explicitly not a closed set.
//!
//! ### `ReviewTransition` vs. `ReviewDocCreated` — the two axes
//!
//! These two variants cover orthogonal mutation concerns:
//!
//! | | `ReviewTransition` | `ReviewDocCreated` |
//! |---|---|---|
//! | **Concern** | State bookkeeping | Artifact-file creation |
//! | **Writes** | `status.yaml` transition, registry entry move, projection row move, actor upsert | `{track_path}/{doc_name}` file (idempotent) |
//! | **Engine route** | `SnapshotCommandHandler::execute` | `ArtifactPort::create_review_doc` |
//! | **Lock** | Acquires `snapshot_lock` | No lock (filesystem write only) |
//! | **Emitted by** | Any path that records a state transition (e.g., future plan/implement handlers) | Any path that creates a review document |
//!
//! A single `begin` call may emit **both** variants (state transition +
//! review doc creation) or only one. The handler emits only what it
//! decides; the engine dispatches each independently.
//!
//! ### `ArtifactCreation` — compound scaffold (any machine kind)
//!
//! `ArtifactCreation` is a single-event shorthand for the entire "create a
//! new artifact" concern: machine-derived directory, initial `status.yaml`,
//! an optional placeholder doc (`spec.md` for a track; none for domain
//! machines), plus the first (initial-state) transition. The engine shim
//! routes this to `ArtifactPort::scaffold_artifact_directory` followed by
//! `SnapshotCommandHandler::execute`. It drives ANY registry-resolved
//! machine — the kind, initial state, directory and registry are all
//! sourced from the resolved `PlaybookMachine`.
//!
//! ### Routing-layer expectations
//!
//! The engine layer in `anvil-engine/src/main.rs` iterates
//! `outcome.events` and dispatches each variant. Adding a new variant
//! **requires** a new match arm there — the compiler enforces exhaustive
//! matching. If a variant does not yet have an engine route (e.g.,
//! emitted by a new domain handler before the shim is updated), the
//! build will fail, which is the correct behavior.
//!
//! ## `ActorSeeded` is not a separate variant
//!
//! Spec R2's `ActorSeeded` concept is realized as actor-identity fields
//! on `ReviewTransition` (the `actor` field) and applied by the snapshot
//! handler's existing upsert leg. Emitting a separate `ActorSeeded`
//! event would break the atomicity of the snapshot handler's
//! "seed actor + append transition" call.

use crate::domain::shared_types::{ActorIdentity, StatusContent};

/// A single domain decision emitted by a begin handler.
///
/// The engine layer pattern-matches on variants to dispatch each event
/// to the correct infrastructure concern. Exhaustive match is required
/// — add match arms when adding variants.
#[derive(Debug, Clone)]
pub enum Event {
    /// A state-bookkeeping transition: move the artifact to a new state,
    /// record the transition in status.yaml, move the registry entry,
    /// and move the projection row. Actor identity is carried here (not
    /// as a separate `ActorSeeded` variant) to preserve the snapshot
    /// handler's atomicity.
    ReviewTransition {
        track_path: String,
        to_state: String,
        actor: ActorIdentity,
        role: String,
        approver: Option<String>,
        note: Option<String>,
    },
    /// A governance ADOPTION reset: an artifact authored OUTSIDE the engine
    /// (no recorded transition history) is taken back to its machine's initial
    /// state so it can be driven through every phase and review gate. Landing
    /// the state is identical to a `ReviewTransition` (the engine routes it
    /// through the same `SnapshotCommandHandler`), but adoption is a STRUCTURALLY
    /// DISTINCT event, not a `ReviewTransition` distinguishable only by its note
    /// prose. Two consumers rely on that distinction:
    /// - the engine stamps the persisted transition with `event_type:
    ///   "adoption"` so `has_open_begin` skips it and never closes the doer's
    ///   freshly-opened begin (adoption leaves an OPEN begin);
    /// - the begin measurement stream omits the spurious transition/playbook
    ///   measurement it would emit for an ordinary lifecycle transition (a reset
    ///   to the initial state is not a doer's forward progress).
    /// The `BeginMarkerWritten` that opens the adopting actor's begin is emitted
    /// as a separate event alongside this one.
    ArtifactAdopted {
        track_path: String,
        to_state: String,
        actor: ActorIdentity,
        role: String,
        note: Option<String>,
    },
    /// A review document was created (or found pre-existing) at the
    /// track directory. The engine layer routes this to `ArtifactPort`.
    ReviewDocCreated {
        track_path: String,
        doc_name: String,
        header: String,
    },
    /// A new track directory, initial status.yaml, and placeholder spec.md
    /// should be scaffolded via `ArtifactPort::scaffold_track_directory`;
    /// the first transition (`spec → spec`) plus registry entry and
    /// execution projection row are then produced by dispatching a
    /// subsequent `SnapshotRequest` with `to_state: "spec"` through the
    /// existing `SnapshotCommandHandler` — no net-new snapshot surface
    /// area. See Amendment 2 of the core_cqrs_separation track.
    /// A new artifact directory + initial status.yaml (and, for kinds that need
    /// initial documents, scaffold files) should be scaffolded via
    /// `ArtifactPort::scaffold_artifact_directory`. Generalized
    /// from the former `TrackCreation` so the engine drives ANY registry-resolved
    /// machine, not just the track seed. The kind / initial state live in
    /// `status` (`status.kind`, `status.state`); `directory` and `registry_file`
    /// are machine-derived placement; `scaffold_files` carries `(filename,
    /// contents)` pairs (track → one `spec.md`, decision → four lifecycle docs);
    /// `creation_role` is the role recorded on the seed transition (`spec` for
    /// track, `doer` for domain machines).
    /// K8 exact-ID genesis (plan Task 5). Structurally DISTINCT from
    /// `ArtifactCreation`: the engine must never route it through the generic
    /// timestamped scaffold, the adoption path, or a candidate -> candidate
    /// Snapshot. It carries the already-validated typed item, the sole
    /// `created(seq: 0)` history entry, the full server-authored actor
    /// identity, and the exact `status.yaml` bytes to publish.
    BacklogItemCreation {
        item: Box<crate::domain::backlog_item::BacklogItem>,
        created: crate::domain::backlog_item::HistoryEntry,
        actor: ActorIdentity,
        status_bytes: String,
        conversation_id: String,
    },
    ArtifactCreation {
        track_name: String,
        parent_id: String,
        display_name: String,
        actor: ActorIdentity,
        approver: String,
        status: StatusContent,
        directory: String,
        registry_file: String,
        scaffold_files: Vec<(String, String)>,
        creation_role: String,
        /// The originating conversation id (resume-aware routing). Carried so the
        /// engine records a durable open-begin marker on the freshly-scaffolded
        /// artifact, bridging the conversation to its open playbook. Empty when
        /// the begin call omits it (no marker conversation_id recorded).
        conversation_id: String,
    },
    /// A projection-only begin event. The engine records the source event
    /// without scaffolding an artifact directory/status.yaml, then dispatches a
    /// projection-only `SnapshotRequest` so the existing projection path updates
    /// the target projection.
    ProjectionOnlySnapshot {
        artifact_path: String,
        event_type: String,
        body: String,
        actor: ActorIdentity,
    },
    /// A begin-marker should be appended to the artifact's `activity:`
    /// log. Modeled on `ReviewDocCreated` (an additive append, NOT a state
    /// transition): the engine routes this to `ActivityWritePort` and stamps
    /// `at` at routing time. It records that `actor` entered the artifact in
    /// `state`. No registry/projection/state writes — strictly an
    /// `activity:` append. `at` is left empty by the pure handler and filled
    /// by the engine (mirrors how `ReviewTransition` defers `at` stamping).
    BeginMarkerWritten {
        artifact_path: String,
        kind: String,
        actor: String,
        state: String,
        at: String,
        /// The originating conversation id (resume-aware routing). Carried onto
        /// the durable `activity:` marker so a later continuation message can
        /// resolve the conversation's open playbook. Empty when the begin call
        /// omits it (back-compat).
        conversation_id: String,
    },
    /// A new playbook directory should be scaffolded via
    /// `ArtifactPort::scaffold_playbook_directory`; the first
    /// transition (`draft → draft`) plus registry entry are then
    /// produced by a subsequent `SnapshotRequest`. Parallel to
    /// `TrackCreation`. Added by track 20260419T1336_workflow_artifact_kind.
    PlaybookCreation {
        playbook_name: String,
        parent_id: String,
        actor: ActorIdentity,
        approver: String,
        status: StatusContent,
    },
}

impl Event {
    /// The target state for a `ReviewTransition` event, or `None` for
    /// other event kinds. Convenience accessor for assertion-step
    /// ergonomics in test step definitions.
    pub fn transition_target(&self) -> Option<&str> {
        match self {
            Event::ReviewTransition { to_state, .. } => Some(to_state.as_str()),
            _ => None,
        }
    }

    /// The target (initial) state for an `ArtifactAdopted` event, or `None`
    /// for other event kinds. Convenience accessor for step assertions over the
    /// governance-adoption reset.
    pub fn adoption_target(&self) -> Option<&str> {
        match self {
            Event::ArtifactAdopted { to_state, .. } => Some(to_state.as_str()),
            _ => None,
        }
    }

    /// The playbook_name from a `PlaybookCreation` event, or `None` for
    /// other event kinds. Convenience accessor for step assertions.
    pub fn playbook_creation_name(&self) -> Option<&str> {
        match self {
            Event::PlaybookCreation { playbook_name, .. } => Some(playbook_name.as_str()),
            _ => None,
        }
    }

    /// The parent_id from a `PlaybookCreation` event, or `None` for
    /// other event kinds. Convenience accessor for step assertions.
    pub fn playbook_creation_parent_id(&self) -> Option<&str> {
        match self {
            Event::PlaybookCreation { parent_id, .. } => Some(parent_id.as_str()),
            _ => None,
        }
    }

    /// The (kind, state, directory, registry_file) from an `ArtifactCreation`
    /// event, or `None` for other event kinds. Convenience accessor for step
    /// assertions over the machine-derived create event.
    pub fn artifact_creation_placement(&self) -> Option<(&str, &str, &str, &str)> {
        match self {
            Event::ArtifactCreation {
                status,
                directory,
                registry_file,
                ..
            } => Some((
                status.kind.as_str(),
                status.state.as_str(),
                directory.as_str(),
                registry_file.as_str(),
            )),
            _ => None,
        }
    }

    /// The `target_owner` carried on an `ArtifactCreation` event's status, or
    /// `None` for other event kinds. Convenience accessor for step assertions
    /// over the recorded owner descriptor (Anvil-lane 1b).
    pub fn artifact_creation_target_owner(&self) -> Option<&str> {
        match self {
            Event::ArtifactCreation { status, .. } => Some(status.target_owner.as_str()),
            _ => None,
        }
    }

    /// The full `StatusContent` carried on an `ArtifactCreation` event, or
    /// `None` for other event kinds. Convenience accessor for step assertions
    /// over the recorded generic field bag (and other status fields).
    pub fn artifact_creation_status(&self) -> Option<&crate::domain::shared_types::StatusContent> {
        match self {
            Event::ArtifactCreation { status, .. } => Some(status),
            _ => None,
        }
    }

    /// The conversation_id carried on an `ArtifactCreation` event, or `None`
    /// for other event kinds. Convenience accessor for step assertions
    /// (resume-aware routing).
    pub fn artifact_creation_conversation_id(&self) -> Option<&str> {
        match self {
            Event::ArtifactCreation {
                conversation_id, ..
            } => Some(conversation_id.as_str()),
            _ => None,
        }
    }

    /// The state from a `BeginMarkerWritten` event, or `None` for other
    /// event kinds. Convenience accessor for step assertions.
    pub fn begin_marker_state(&self) -> Option<&str> {
        match self {
            Event::BeginMarkerWritten { state, .. } => Some(state.as_str()),
            _ => None,
        }
    }

    /// The kind from a `BeginMarkerWritten` event, or `None` for other
    /// event kinds. Convenience accessor for step assertions.
    pub fn begin_marker_kind(&self) -> Option<&str> {
        match self {
            Event::BeginMarkerWritten { kind, .. } => Some(kind.as_str()),
            _ => None,
        }
    }

    /// The conversation_id from a `BeginMarkerWritten` event, or `None` for
    /// other event kinds. Convenience accessor for step assertions
    /// (resume-aware routing).
    pub fn begin_marker_conversation_id(&self) -> Option<&str> {
        match self {
            Event::BeginMarkerWritten {
                conversation_id, ..
            } => Some(conversation_id.as_str()),
            _ => None,
        }
    }
}
