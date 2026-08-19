use crate::domain::amendment::OpLog;
use crate::domain::begin::BeginError;
use crate::domain::shared_types::{ActivityEntry, ActivityLog, RegistryEntry};
use crate::domain::status::FullStatusYaml;
use std::fmt;

/// Existing artifact matched by the routed-turn idempotency key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginTurnArtifact {
    pub artifact_path: String,
    pub state: String,
}

/// Errors from query operations. The `From<QueryError> for BeginError`
/// impl provides the mapping Phase 2 uses when propagating read failures
/// into the begin handler's error type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    /// The artifact does not exist in the hearth.
    NotFound { artifact_id: String },
    /// The artifact's status.yaml exists but cannot be parsed.
    MalformedStatus {
        artifact_id: String,
        message: String,
    },
    /// A general I/O failure (file missing, permission error, etc.).
    IoError { message: String },
    /// `check_projection_row_unique` found zero rows for the given
    /// track name in the given section.
    ProjectionRowNotFound { message: String },
    /// `check_projection_row_unique` found two or more rows — projection
    /// has a duplicate-name collision.
    ProjectionRowAmbiguous { message: String },
    /// The artifact's transition evidence could not be read under the STRICT
    /// (fail-closed) adoption policy: an unreadable `transitions/` directory,
    /// an unreadable event file, or an unparseable event. Surfaced ONLY by
    /// `read_transitions_strict` — the lenient `read_transitions` deliberately
    /// tolerates this damage. Adoption maps it to
    /// `BeginError::AdoptionEvidenceUnreadable` and fails closed.
    AdoptionEvidenceUnreadable { artifact_id: String, detail: String },
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueryError::NotFound { artifact_id } => {
                write!(f, "Artifact '{}' not found in hearth", artifact_id)
            }
            QueryError::MalformedStatus {
                artifact_id,
                message,
            } => {
                write!(
                    f,
                    "Malformed status.yaml for '{}': {}",
                    artifact_id, message
                )
            }
            QueryError::IoError { message } => write!(f, "I/O error: {}", message),
            QueryError::ProjectionRowNotFound { message } => {
                write!(f, "Projection row not found: {}", message)
            }
            QueryError::ProjectionRowAmbiguous { message } => {
                write!(f, "Projection row ambiguous: {}", message)
            }
            QueryError::AdoptionEvidenceUnreadable {
                artifact_id,
                detail,
            } => {
                write!(
                    f,
                    "Adoption evidence unreadable for '{}': {}",
                    artifact_id, detail
                )
            }
        }
    }
}

impl std::error::Error for QueryError {}

/// Maps a `QueryError` into a `BeginError`.
///
/// `ProjectionRowNotFound` and `ProjectionRowAmbiguous` both map to
/// `BeginError::IoError` with the message preserved verbatim — matching
/// the current `fs_begin_adapter.rs:437-449` output so the existing
/// `begin_error_projection_row_ambiguous.feature` assertions survive
/// unchanged after Phase 3.
impl From<QueryError> for BeginError {
    fn from(e: QueryError) -> Self {
        match e {
            QueryError::NotFound { artifact_id } => BeginError::NotFound {
                identifier: artifact_id,
            },
            QueryError::MalformedStatus {
                artifact_id,
                message,
            } => BeginError::MalformedStatus {
                artifact_id,
                message,
            },
            QueryError::IoError { message } => BeginError::IoError { message },
            QueryError::ProjectionRowNotFound { message } => BeginError::IoError { message },
            QueryError::ProjectionRowAmbiguous { message } => BeginError::IoError { message },
            QueryError::AdoptionEvidenceUnreadable {
                artifact_id,
                detail,
            } => BeginError::AdoptionEvidenceUnreadable {
                identifier: artifact_id,
                detail,
            },
        }
    }
}

/// Read-only port for the begin handler's data needs.
///
/// This port is the query side of the CQRS split: the begin handler
/// reads through `QueryPort` and emits `Event`s rather than calling
/// mutation methods directly. Phase 2 wires the pure handler to
/// `FileSystemQueryAdapter` in the engine layer.
pub trait QueryPort: Send + Sync {
    /// Enumerate the hearth's artifacts as `(id, kind)` pairs.
    ///
    /// Resume-aware routing (Phase 2): the route handler bridges a
    /// continuation message to the conversation's open playbook by scanning the
    /// active artifacts' begin markers. This is the single enumeration seam that
    /// lookup uses — the handler calls `list_artifacts` and then per-artifact
    /// reads (`read_activity_entries`, `read_artifact_state`), never a raw
    /// filesystem scan in the handler itself.
    ///
    /// Returns every artifact the adapter can enumerate, regardless of state;
    /// terminal/openness filtering is the caller's (domain) concern. The default
    /// implementation returns an empty list so in-memory / synthetic adapters
    /// that do not model enumeration need not implement it; the filesystem
    /// adapter overrides it with a real scan.
    fn list_artifacts(&self) -> Result<Vec<(String, String)>, QueryError> {
        Ok(Vec::new())
    }

    /// The artifact kind ("track", "proposal", etc.) for the given id.
    fn read_artifact_kind(&self, artifact_id: &str) -> Result<String, QueryError>;

    /// The current `state:` field in the artifact's status.yaml.
    fn read_artifact_state(&self, artifact_id: &str) -> Result<String, QueryError>;

    /// The full parsed status.yaml for the artifact.
    fn read_artifact_status(&self, artifact_id: &str) -> Result<FullStatusYaml, QueryError>;

    /// Find an existing artifact of `kind` created from `origin_turn`.
    /// Returns `None` when no artifact carries that durable turn key.
    fn find_artifact_by_kind_origin_turn(
        &self,
        kind: &str,
        origin_turn: &str,
    ) -> Result<Option<OriginTurnArtifact>, QueryError>;

    /// The artifact's `activity:` begin-marker log. Returns an empty vec
    /// when the `activity:` key is absent. Used by the begin-adoption
    /// soft-warn (BP2) and the `BeginAdoptionStatus` query (BP3). The
    /// matching transitions come from `read_artifact_status`.
    fn read_activity_entries(&self, artifact_id: &str) -> Result<Vec<ActivityEntry>, QueryError>;

    /// The artifact's `activity:` log WITH degradation diagnostics — the
    /// parsed begin-markers plus a count of malformed entries dropped during
    /// the per-entry-tolerant parse. A `dropped > 0` log is DEGRADED: a dropped
    /// entry may have been an OPEN begin marker, so begin-adoption consumers
    /// (the runtime gate, the `BeginAdoptionStatus` query) must treat it
    /// conservatively rather than as a clean, truly-empty log — otherwise a
    /// dropped open marker silently permits a DUPLICATE begin.
    ///
    /// The default implementation wraps [`Self::read_activity_entries`] as a
    /// never-degraded log — correct for synthetic in-memory/test adapters whose
    /// entries are constructed (never parsed, so nothing can be dropped). The
    /// filesystem adapter overrides it to surface the real drop count read from
    /// the parsed `status.yaml`.
    fn read_activity_log(&self, artifact_id: &str) -> Result<ActivityLog, QueryError> {
        Ok(ActivityLog::new(self.read_activity_entries(artifact_id)?))
    }

    /// The artifact's ordered transition history (oldest → newest). For the
    /// filesystem adapter this FOLDS the per-file transition event store
    /// (`<artifact>/transitions/`) merged with any legacy `status.yaml` array;
    /// the begin-adoption soft-warn and `BeginAdoptionStatus` query route here
    /// so they see the creation event (and every subsequent event), not just
    /// the legacy array. The default implementation folds only the parsed
    /// status (no event directory) — correct for in-memory / synthetic
    /// adapters that have no filesystem event store.
    fn read_transitions(
        &self,
        artifact_id: &str,
    ) -> Result<Vec<crate::domain::status::StatusTransition>, QueryError> {
        let status = self.read_artifact_status(artifact_id)?;
        Ok(crate::domain::transition_log::resolve_transitions(&status))
    }

    /// The artifact's ordered transition history read under a STRICT
    /// (fail-closed) policy — the ADOPTION-only read.
    ///
    /// Identical to [`Self::read_transitions`] EXCEPT that damaged transition
    /// evidence (an unreadable `transitions/` directory or event file, or an
    /// unparseable event) is surfaced as
    /// `QueryError::AdoptionEvidenceUnreadable` rather than silently skipped.
    /// Adoption resets an artifact to its initial state, so it must NEVER
    /// proceed on evidence it cannot fully read: a damaged newest event could
    /// otherwise hide a governing transition and let the reset clobber
    /// engine-governed work. Non-adoption consumers (status folds, soft-warn
    /// detection) keep the lenient read — they deliberately tolerate a corrupt
    /// sibling.
    ///
    /// The default implementation delegates to the lenient read — correct for
    /// in-memory / synthetic adapters that have no on-disk event store to
    /// damage. The filesystem adapter overrides it with a strict on-disk read.
    fn read_transitions_strict(
        &self,
        artifact_id: &str,
    ) -> Result<Vec<crate::domain::status::StatusTransition>, QueryError> {
        self.read_transitions(artifact_id)
    }

    /// The text of a file inside an artifact directory.
    /// `track_path` is relative to the hearth root; `filename` is the
    /// leaf name (e.g., "spec.md").
    fn read_artifact_text(&self, track_path: &str, filename: &str) -> Result<String, QueryError>;

    /// The full contents of `<artifact_path>/carry-forward.md` when present
    /// (Slice C). Returns `Ok(None)` when the file is absent — absence is not
    /// an error (spec R4.2). Returns the raw file bytes including frontmatter
    /// and header; findings extraction happens in the begin handler, not here.
    /// Read fresh from disk on every call (spec R4.5 — no cache), so a manual
    /// correction to the file is reflected in the next `begin`.
    ///
    /// Defaulted to `Ok(None)` so in-memory / synthetic adapters that have no
    /// carry-forward file need not implement it; the filesystem adapter
    /// overrides it.
    fn read_carry_forward_if_present(
        &self,
        _artifact_path: &str,
    ) -> Result<Option<String>, QueryError> {
        Ok(None)
    }

    /// The structured per-document op log for an artifact, read from
    /// `<artifact_path>/<target_document>.amendments.yaml`. Returns an empty
    /// `OpLog` when the file is absent (mirroring `read_activity_entries`'
    /// empty-collection-when-absent convention). The amendment handler replays
    /// this log in `ordered()` order to validate a candidate op (BP2).
    fn read_op_log(&self, artifact_path: &str, target_document: &str) -> Result<OpLog, QueryError>;

    /// The text of a hearth context file.
    /// `relative_path` is relative to the hearth root
    /// (e.g., "context/spec-writing.md").
    fn read_context_file(&self, relative_path: &str) -> Result<String, QueryError>;

    /// The body of a named hook file belonging to a specific playbook.
    /// `playbook_id` is the playbook's directory name under `{hearth}/playbooks/`
    /// (e.g., "20260422T0000_track_lifecycle"). `filename` is the leaf hook
    /// file name (e.g., "spec-writing.md"). Path traversal components
    /// (`/`, `..`) in `filename` are rejected with an `IoError`.
    fn read_playbook_hook_body(
        &self,
        playbook_id: &str,
        filename: &str,
    ) -> Result<String, QueryError>;

    /// The registry entry for an existing artifact. Parses the markdown
    /// entry whose first-link href ends with `/{artifact_id}/` and
    /// returns the parsed names.
    fn read_registry_entry(
        &self,
        registry_file: &str,
        artifact_id: &str,
    ) -> Result<RegistryEntry, QueryError>;

    /// Verify that exactly one projection row matches the given track
    /// name in the given section. Returns `Ok(())` on exact match;
    /// `ProjectionRowNotFound` (0 rows) or `ProjectionRowAmbiguous`
    /// (≥2 rows) otherwise.
    ///
    /// Message text is byte-identical to the current
    /// `fs_begin_adapter.rs:437-449` output so feature assertions
    /// survive unchanged.
    ///
    /// # Forward compatibility note
    ///
    /// This method has no production callers after Amendment 1 removed the
    /// projection-row pre-flight check from `handle_review`. It is kept
    /// because any future handler that emits `ReviewTransition` (or any
    /// other transition that moves a projection row) will need this check
    /// to guard against ambiguous rows before committing the mutation.
    /// `query_port_fs_fidelity.feature` preserves the error-message
    /// contract so it does not silently drift.
    fn check_projection_row_unique(
        &self,
        projection_file: &str,
        track_name: &str,
        from_section: &str,
    ) -> Result<(), QueryError>;
}
