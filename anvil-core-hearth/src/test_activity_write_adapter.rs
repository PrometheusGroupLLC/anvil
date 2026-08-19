//! In-memory `ActivityWritePort` implementation for tests. Records each
//! appended begin-marker so feature scenarios can drive and assert the
//! append behavior without filesystem I/O.

use anvil_core::domain::shared_types::ActivityEntry;
use anvil_core::ports::activity_write_port::{ActivityWriteError, ActivityWritePort};
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Default)]
pub struct TestActivityWriteAdapter {
    /// Keyed by artifact_path → ordered list of appended entries.
    entries: Mutex<HashMap<String, Vec<ActivityEntry>>>,
}

impl TestActivityWriteAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Cloned snapshot of the entries appended at `artifact_path`.
    pub fn entries_for(&self, artifact_path: &str) -> Vec<ActivityEntry> {
        self.entries
            .lock()
            .unwrap()
            .get(artifact_path)
            .cloned()
            .unwrap_or_default()
    }
}

impl ActivityWritePort for TestActivityWriteAdapter {
    fn append_activity(
        &self,
        artifact_path: &str,
        entry: &ActivityEntry,
    ) -> Result<(), ActivityWriteError> {
        self.entries
            .lock()
            .unwrap()
            .entry(artifact_path.to_string())
            .or_default()
            .push(entry.clone());
        Ok(())
    }
}
