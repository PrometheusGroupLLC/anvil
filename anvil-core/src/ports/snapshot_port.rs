use crate::domain::backlog_item::BacklogTransitionContext;
use crate::domain::shared_types::{ActivityEntry, ActorIdentity, TransitionContent};
use crate::domain::snapshot::SnapshotError;
use crate::domain::status::StatusTransition;
use crate::ports::backlog_item_port::{LoadedBacklogItem, PreparedBacklogCommit};

/// Port for snapshot operations — the engine's authoritative writer for
/// status transitions, registry mutations, and incremental projection
/// updates. Returns `SnapshotError` at every method; the handler
/// converts `Err` into `SnapshotResult.warnings` for non-critical writes
/// per criticality ordering.
pub trait SnapshotPort: Send + Sync {
    /// The artifact kind ("track", "proposal", ...) at the given path.
    fn read_artifact_kind(&self, artifact_path: &str) -> Result<String, SnapshotError>;

    /// The current `state:` field in the artifact's status.yaml.
    fn read_artifact_state(&self, artifact_path: &str) -> Result<String, SnapshotError>;

    /// The names currently present in the actors table for the artifact.
    /// Used to test collision when the handler generates a new actor
    /// name. An empty list is returned when the actors table is absent.
    fn read_artifact_actor_names(&self, artifact_path: &str) -> Result<Vec<String>, SnapshotError>;

    /// The artifact's `activity:` begin-marker log (empty when absent).
    /// A leaf-executor read used by the begin-adoption soft-warn — the
    /// snapshot handler holds only `&dyn SnapshotPort`, so this read seam
    /// belongs here (F-2). Mirrors `QueryPort::read_activity_entries`.
    fn read_activity_entries(
        &self,
        artifact_path: &str,
    ) -> Result<Vec<ActivityEntry>, SnapshotError>;

    /// The artifact's `transitions:` history (empty when absent). Needed
    /// alongside `read_activity_entries` to evaluate the begin-adoption
    /// close-by-comparison predicate and the creating-actor exemption.
    fn read_transitions(&self, artifact_path: &str)
        -> Result<Vec<StatusTransition>, SnapshotError>;

    /// Strictly load one K8 `backlog_item` through the dedicated store,
    /// recovering any interrupted transaction under the held hearth lock first.
    ///
    /// Defaulted to "unsupported" so unrelated adapters need not implement it;
    /// the filesystem adapter delegates to `FileSystemBacklogItemAdapter`. The
    /// default is an ERROR, never an empty value — a K8 read that silently
    /// answered `None` would let a preflight admit a transition it never saw.
    fn read_backlog_item(&self, bi_id: &str) -> Result<LoadedBacklogItem, SnapshotError> {
        Err(SnapshotError::BacklogStore {
            message: format!(
                "read_backlog_item is not supported by this SnapshotPort for '{bi_id}'"
            ),
        })
    }

    /// Every strictly loaded K8 item in the hearth, recovering first. The
    /// registry projection is rebuilt from this complete set, so a partial
    /// answer would silently corrupt it — the default is an error.
    fn read_backlog_items(&self) -> Result<Vec<LoadedBacklogItem>, SnapshotError> {
        Err(SnapshotError::BacklogStore {
            message: "read_backlog_items is not supported by this SnapshotPort".to_string(),
        })
    }

    /// The strict same-organ context a rank-sensitive or `#5` guard must see:
    /// the reconciled candidate/ready set, the resolved comparator/policy, the
    /// registry revision, and one aggregate context hash — all under the held
    /// lock.
    fn read_backlog_transition_context(
        &self,
        business_node_id: &str,
    ) -> Result<BacklogTransitionContext, SnapshotError> {
        Err(SnapshotError::BacklogStore {
            message: format!(
                "read_backlog_transition_context is not supported by this SnapshotPort for \
                 '{business_node_id}'"
            ),
        })
    }

    /// The PRIVILEGED post-genesis K8 state writer. Consuming a prepared
    /// capability is the ONLY way K8 lifecycle bytes move; `append_transition`
    /// rejects an authoritatively resolved `backlog_item`.
    fn commit_backlog_transition(
        &self,
        _prepared: PreparedBacklogCommit,
    ) -> Result<(), SnapshotError> {
        Err(SnapshotError::BacklogStore {
            message: "commit_backlog_transition is not supported by this SnapshotPort".to_string(),
        })
    }

    /// Append a transition to the artifact's status.yaml and sync the
    /// top-level `state:` field to `transition.to`.
    fn append_transition(
        &self,
        artifact_path: &str,
        transition: &TransitionContent,
    ) -> Result<(), SnapshotError>;

    /// Ensure the actor identity is seeded in the artifact's actors
    /// table. Idempotent when the actor already exists with matching
    /// configuration.
    fn seed_actor(&self, artifact_path: &str, actor: &ActorIdentity) -> Result<(), SnapshotError>;

    /// Whether the registry file already contains an entry pointing at
    /// `artifact_id`. Used by the handler to dispatch between
    /// `create_registry_entry` (first state change) and
    /// `move_registry_entry` (subsequent state changes).
    fn registry_entry_exists(
        &self,
        registry_file: &str,
        artifact_id: &str,
    ) -> Result<bool, SnapshotError>;

    /// Build the markdown entry text for a brand-new registry entry by
    /// reading the artifact's frozen document(s). Invoked by the handler
    /// only when `registry_entry_exists` returns `false`. The handler
    /// then hands the text to `create_registry_entry`.
    fn build_registry_entry_text(
        &self,
        artifact_kind: &str,
        artifact_path: &str,
        to_section: &str,
    ) -> Result<String, SnapshotError>;

    /// Insert a freshly-built entry under the target section of the
    /// registry file. The adapter creates the target section header if
    /// absent.
    fn create_registry_entry(
        &self,
        registry_file: &str,
        artifact_id: &str,
        artifact_kind: &str,
        to_section: &str,
        entry_text: &str,
    ) -> Result<(), SnapshotError>;

    /// Move an existing registry entry from whatever section currently
    /// hosts it to `to_section`. Source search scans the whole file;
    /// from_section is not carried (the engine writes to consolidated
    /// destinations regardless of current fine-grained location).
    /// Creates the target section header if absent.
    fn move_registry_entry(
        &self,
        registry_file: &str,
        artifact_id: &str,
        to_section: &str,
    ) -> Result<(), SnapshotError>;

    /// Move a track row in execution.md to `to_section` (title-case-with-
    /// count projection label, e.g., "Spec Review").
    fn move_execution_row(&self, track_name: &str, to_section: &str) -> Result<(), SnapshotError>;

    /// Move an artifact row in intent.md for proposals or milestones to
    /// `to_section`. `kind` is "proposal" or "milestone".
    fn move_intent_row(
        &self,
        artifact_name: &str,
        kind: &str,
        to_section: &str,
    ) -> Result<(), SnapshotError>;

    /// Rebuild the decisions projection (counts + active tensions +
    /// recently resolved) from the decisions registry.
    fn rebuild_decisions_projection(&self) -> Result<(), SnapshotError>;

    /// Rebuild the sparks projection (counts + frontmatter). Narrative
    /// lines are preserved — this method writes only the count line and
    /// frontmatter.
    fn rebuild_sparks_projection(&self) -> Result<(), SnapshotError>;

    /// Fold a playbook_generation transition into the artifact-local
    /// authoring projection (`<artifact>/authoring.md`).
    fn write_authoring_projection(
        &self,
        artifact_path: &str,
        phase_label: &str,
        state: &str,
        at: &str,
        actor: &str,
        role: &str,
    ) -> Result<(), SnapshotError> {
        let _ = (phase_label, state, at, actor, role);
        Err(SnapshotError::IoError {
            message: format!(
                "write_authoring_projection is not supported by this SnapshotPort for '{}'",
                artifact_path
            ),
        })
    }

    /// Whether the artifact's machine declares `projection_targets` on the
    /// given `to_state`. Drives the generic per-artifact projection for
    /// GENERATED kinds the engine does not hardcode. Defaulted to `false`
    /// so existing adapters need not implement it; the filesystem adapter
    /// resolves it from the hearth playbook registry.
    fn state_declares_projection(
        &self,
        _artifact_path: &str,
        _to_state: &str,
    ) -> Result<bool, SnapshotError> {
        Ok(false)
    }

    /// Fold a transition into the generic per-artifact projection
    /// (`<artifact>/projection.md`) for GENERATED kinds. Same shape as
    /// `write_authoring_projection`; defaulted to "not supported" so the
    /// in-memory test adapter need not implement it.
    fn write_artifact_projection(
        &self,
        artifact_path: &str,
        phase_label: &str,
        state: &str,
        at: &str,
        actor: &str,
        role: &str,
    ) -> Result<(), SnapshotError> {
        let _ = (phase_label, state, at, actor, role);
        Err(SnapshotError::IoError {
            message: format!(
                "write_artifact_projection is not supported by this SnapshotPort for '{}'",
                artifact_path
            ),
        })
    }

    /// Generate a `{Word}-{NNNNNN}` actor name not present in
    /// `existing_actor_names`. The implementation retries on collision
    /// up to its own budget and returns an error if the budget is
    /// exhausted.
    fn generate_actor_name(&self, existing_actor_names: &[String])
        -> Result<String, SnapshotError>;

    /// Write the carry-forward file `<artifact_path>/carry-forward.md` (Slice C).
    /// `content` is the fully-rendered file body (frontmatter + header +
    /// verbatim findings); the adapter is a byte-sink and performs no
    /// interpretation. Returns the absolute path written.
    ///
    /// The file is append-only from the engine's perspective for the duration
    /// of the spec → plan transition: if it already exists, the adapter returns
    /// `SnapshotError::IoError` rather than silently overwriting (spec R3.3).
    ///
    /// Defaulted to an `IoError` so the in-memory test adapter need not
    /// implement it; the filesystem adapter overrides it.
    fn write_carry_forward(
        &self,
        artifact_path: &str,
        _content: &str,
    ) -> Result<String, SnapshotError> {
        Err(SnapshotError::IoError {
            message: format!(
                "write_carry_forward is not supported by this SnapshotPort for '{}'",
                artifact_path
            ),
        })
    }
}
