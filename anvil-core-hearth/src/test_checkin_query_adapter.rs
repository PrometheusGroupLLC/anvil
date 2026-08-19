use anvil_core::domain::checkin::CheckinError;
use anvil_core::domain::{ArtifactSummary, HearthError};
use anvil_core::ports::checkin_query_port::CheckinQueryPort;

/// In-memory test adapter for the reshaped checkin query.
#[derive(Debug, Clone)]
pub struct TestCheckinQueryAdapter {
    artifacts: Vec<ArtifactSummary>,
    word_list: Vec<String>,
}

impl TestCheckinQueryAdapter {
    pub fn new(artifacts: Vec<ArtifactSummary>, word_list: Vec<String>) -> Self {
        Self {
            artifacts,
            word_list,
        }
    }
}

impl CheckinQueryPort for TestCheckinQueryAdapter {
    fn list_artifacts(&self) -> Result<Vec<ArtifactSummary>, HearthError> {
        Ok(self.artifacts.clone())
    }

    fn load_word_list(&self) -> Result<Vec<String>, CheckinError> {
        Ok(self.word_list.clone())
    }
}
