use crate::domain::{ArtifactSummary, HearthError};

/// Port for reading artifact state from a hearth.
///
/// The trait operates on domain types, not file paths. Implementations
/// decide how to read artifacts — from the filesystem, from memory
/// (for testing), or from any other source.
pub trait HearthReaderPort {
    /// List all artifacts in the hearth.
    ///
    /// Returns all artifacts regardless of state. Terminal state filtering
    /// is the caller's responsibility (domain layer concern, not adapter concern).
    fn list_artifacts(&self) -> Result<Vec<ArtifactSummary>, HearthError>;

    /// Read the raw YAML text of a playbook artifact's `machine.yaml` file.
    ///
    /// Returns `Ok(Some(yaml_text))` if the file exists and is readable.
    /// Returns `Ok(None)` if the artifact has no `machine.yaml` (e.g., when
    /// the artifact was created with `begin` but its machine has not been
    /// authored yet — valid during early lifecycle states).
    /// Returns `Err` only on unexpected I/O failures.
    ///
    /// `artifact_id` is the directory name of the playbook artifact
    /// (e.g., `"20260420T1000_my_workflow"`). The implementation is
    /// responsible for locating the file under `playbooks/{id}/machine.yaml`.
    ///
    /// Phase 3: called by `CatalogQueryHandler` for each playbook artifact to
    /// validate the machine and collect `invalid_artifacts`.
    fn read_playbook_machine_yaml(&self, artifact_id: &str) -> Result<Option<String>, HearthError>;

    /// List the filenames under a playbook artifact's `hooks/` directory.
    ///
    /// Returns `Ok(vec![])` when the directory does not exist (no hooks defined).
    /// Returns `Ok(filenames)` with only plain filenames (no path components) when
    /// the directory exists. The list is sorted for stable ordering.
    /// Returns `Err` only on unexpected I/O failures.
    ///
    /// `artifact_id` is the directory name of the playbook artifact
    /// (e.g., `"20260420T1000_my_workflow"`). The implementation locates
    /// the directory at `playbooks/{artifact_id}/hooks/`.
    ///
    /// Phase 6: used by `list_playbook_hooks` discoverability feature (R5.6).
    /// The Phase-1 loader receives hook filenames as a `&[String]` argument
    /// (pure function); this port method provides the production call site.
    fn list_playbook_hooks(&self, artifact_id: &str) -> Result<Vec<String>, HearthError>;
}
