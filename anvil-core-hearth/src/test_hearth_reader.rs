use anvil_core::domain::{ArtifactSummary, HearthError};
use anvil_core::ports::hearth_reader::HearthReaderPort;

/// In-memory hearth reader for testing.
///
/// A real adapter implementing the real HearthReaderPort with the
/// behavioral property of being deterministic and configurable.
/// Not a mock — a test adapter per the build philosophy.
pub struct TestHearthReader {
    artifacts: Vec<ArtifactSummary>,
}

impl TestHearthReader {
    pub fn new(artifacts: Vec<ArtifactSummary>) -> Self {
        Self { artifacts }
    }
}

impl HearthReaderPort for TestHearthReader {
    fn list_artifacts(&self) -> Result<Vec<ArtifactSummary>, HearthError> {
        Ok(self.artifacts.clone())
    }

    fn read_playbook_machine_yaml(
        &self,
        _artifact_id: &str,
    ) -> Result<Option<String>, HearthError> {
        // In-memory test reader has no on-disk machine.yaml files.
        // Tests that need machine.yaml behavior use engine-level integration tests
        // against the filesystem reader.
        Ok(None)
    }

    fn list_playbook_hooks(&self, _artifact_id: &str) -> Result<Vec<String>, HearthError> {
        // In-memory test reader has no hooks directories.
        // Tests that need hooks discoverability use FileSystemHearthReader.
        Ok(Vec::new())
    }
}
