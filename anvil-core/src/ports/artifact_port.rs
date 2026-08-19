use crate::domain::begin::BeginError;
use crate::domain::playbook::candidate::GeneratedExemplarFile;
use std::fmt;

/// Errors from artifact operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactError {
    /// A general I/O failure.
    IoError { message: String },
    /// A playbook machine with this kind already exists at the target write
    /// path and is not byte-identical to this persist.
    PlaybookDuplicateKind { kind: String, path: String },
    /// A playbook machine exists at the target write path but cannot be read or
    /// parsed as a valid machine, so the writer must fail closed.
    PlaybookExistingMachineInvalid {
        kind: String,
        path: String,
        message: String,
    },
}

impl fmt::Display for ArtifactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArtifactError::IoError { message } => write!(f, "I/O error: {}", message),
            ArtifactError::PlaybookDuplicateKind { kind, path } => write!(
                f,
                "playbook_duplicate_kind_registration: kind '{}' already exists at '{}'",
                kind, path
            ),
            ArtifactError::PlaybookExistingMachineInvalid {
                kind,
                path,
                message,
            } => write!(
                f,
                "playbook_existing_machine_invalid: existing machine for kind '{}' at '{}' could not be loaded: {}",
                kind, path, message
            ),
        }
    }
}

impl std::error::Error for ArtifactError {}

impl From<ArtifactError> for BeginError {
    fn from(e: ArtifactError) -> Self {
        match e {
            ArtifactError::IoError { message } => BeginError::IoError { message },
            other => BeginError::IoError {
                message: other.to_string(),
            },
        }
    }
}

/// Narrow port for artifact file creation concerns.
///
/// Separated from `QueryPort` because artifact creation is a mutation
/// concern routed by the engine layer via `Event::ReviewDocCreated`,
/// while `QueryPort` is strictly read-only. The idempotency check
/// (file exists?) is an I/O read the pure domain handler must not do
/// directly — hence the port contract.
pub trait ArtifactPort: Send + Sync {
    /// Create a review document at the track directory with the given
    /// header text. `track_path` is the absolute path to the track
    /// directory. Returns the absolute path to the created or existing
    /// file. Idempotent — if the file already exists, returns its path
    /// without overwriting.
    fn create_review_doc(
        &self,
        track_path: &str,
        doc_name: &str,
        header: &str,
    ) -> Result<String, ArtifactError>;

    /// Scaffold a new track directory with an initial `status.yaml` and a
    /// placeholder `spec.md` containing `# {display_name}\n`. Returns the
    /// relative track path (e.g., `"tracks/20260420T0145_my_track"`).
    ///
    /// The placeholder `spec.md` exists so downstream snapshot operations
    /// (specifically `build_registry_entry_text`'s `extract_h1` read) can
    /// resolve a display name without the caller needing to inject one
    /// through the snapshot engine. The spec-authoring session overwrites
    /// this file with its own content on first pass.
    fn scaffold_track_directory(
        &self,
        track_name: &str,
        parent_id: &str,
        display_name: &str,
        status_yaml: &str,
    ) -> Result<String, ArtifactError>;

    /// Scaffold a new playbook directory with an initial `status.yaml` and a
    /// placeholder `definition.md`. Returns the relative playbook path
    /// (e.g., `"playbooks/20260420T0145_my_workflow"`).
    ///
    /// Parallel to `scaffold_track_directory`; differs only in the parent-kind
    /// field name (`track:` vs `proposal:`) and the target directory.
    /// Added by track 20260419T1336_workflow_artifact_kind.
    fn scaffold_playbook_directory(
        &self,
        playbook_name: &str,
        parent_id: &str,
        status_yaml: &str,
    ) -> Result<String, ArtifactError>;

    /// Kind-agnostic scaffold: create `<directory>/<timestamp>_<snake(name)>/`
    /// with an initial `status.yaml`, plus every requested scaffold file. If the base
    /// directory already exists, the filesystem implementation appends the
    /// first available numeric suffix (`-2`, `-3`, ...). Returns the relative
    /// artifact path (e.g. `"knowledge/20260601T0000_topic"`).
    ///
    /// This generalizes `scaffold_track_directory` / `scaffold_playbook_directory`
    /// so the engine drives ANY registry-resolved machine. The track create path
    /// flows through this with `directory="tracks"` and a `spec.md` scaffold
    /// file; domain machines may pass an empty slice.
    fn scaffold_artifact_directory(
        &self,
        directory: &str,
        name: &str,
        status_yaml: &str,
        scaffold_files: &[(&str, &str)],
    ) -> Result<String, ArtifactError>;

    /// Persist a generated, ALREADY-VALIDATED playbook machine into an explicit
    /// owner-home, registry-resolvable from there. Writes
    /// `<owner_home>/playbooks/<kind>/machine.yaml` (creating dirs) and — when
    /// `hooks` is `Some` — a `hooks/<name>` file per entry. Writes **NO**
    /// status.yaml (its absence keeps the kind registry-resolvable-but-not-
    /// begin-able, mirroring the candidate-intake KIND_DIRS gap).
    ///
    /// `owner_home` is an absolute path supplied by the caller; this op MUST NOT
    /// resolve under any fixed `self.hearth_path`. Returns the absolute path to
    /// the written `machine.yaml`.
    ///
    /// The pure `PersistPlaybookCommandHandler` loader-validates the requested
    /// machine FIRST and performs a fast-path duplicate decision, but this write
    /// boundary is the authoritative collision gate. The implementation must
    /// create `machine.yaml` exclusively; if the file already exists, identical
    /// bytes are an idempotent success and different or invalid existing bytes
    /// fail closed without overwriting.
    fn persist_generated_playbook(
        &self,
        owner_home: &str,
        kind: &str,
        machine_yaml: &str,
        hooks: Option<&[(String, String)]>,
        exemplars: Option<&[GeneratedExemplarFile]>,
    ) -> Result<String, ArtifactError>;
}
