use anvil_core::domain::checkin::CheckinError;
use anvil_core::domain::{ArtifactSummary, HearthError};
use crate::fs_hearth_reader::FileSystemHearthReader;
use anvil_core::ports::checkin_query_port::CheckinQueryPort;
use anvil_core::ports::hearth_reader::HearthReaderPort;
use std::path::PathBuf;

/// Filesystem implementation of CheckinQueryPort.
/// Reuses FileSystemHearthReader for artifact listing and the word list
/// loading logic from FileSystemCheckinAdapter.
#[derive(Debug, Clone)]
pub struct FileSystemCheckinQueryAdapter {
    hearth_path: PathBuf,
}

impl FileSystemCheckinQueryAdapter {
    pub fn new(hearth_path: PathBuf) -> Self {
        Self { hearth_path }
    }
}

impl CheckinQueryPort for FileSystemCheckinQueryAdapter {
    fn list_artifacts(&self) -> Result<Vec<ArtifactSummary>, HearthError> {
        let reader = FileSystemHearthReader::new(self.hearth_path.clone());
        reader.list_artifacts()
    }

    fn load_word_list(&self) -> Result<Vec<String>, CheckinError> {
        let dict_path = PathBuf::from("/usr/share/dict/words");
        let content = std::fs::read_to_string(&dict_path).map_err(|e| CheckinError::IoError {
            message: format!("Failed to read {}: {}", dict_path.display(), e),
        })?;
        let words: Vec<String> = content
            .lines()
            .filter(|w| {
                let len = w.len();
                (4..=8).contains(&len)
                    && w.starts_with(|c: char| c.is_ascii_uppercase())
                    && w[1..].chars().all(|c| c.is_ascii_lowercase())
            })
            .map(|w| w.to_string())
            .collect();
        if words.is_empty() {
            return Err(CheckinError::IoError {
                message: "No suitable words found in dictionary".to_string(),
            });
        }
        Ok(words)
    }
}
