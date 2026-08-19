use crate::domain::checkin::CheckinError;
use crate::domain::{ArtifactSummary, HearthError};

/// Port for the reshaped checkin query.
/// Lists all artifacts (filtering is domain logic) and loads word list for name generation.
pub trait CheckinQueryPort {
    fn list_artifacts(&self) -> Result<Vec<ArtifactSummary>, HearthError>;
    fn load_word_list(&self) -> Result<Vec<String>, CheckinError>;
}
