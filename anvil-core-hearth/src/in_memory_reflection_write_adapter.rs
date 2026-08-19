use anvil_core::ports::reflection_write_port::{ReflectionWriteError, ReflectionWritePort};
use std::collections::HashMap;
use std::sync::Mutex;

/// In-memory implementation of `ReflectionWritePort` for use in domain-seam
/// brine step definitions and pure-domain feature tests that don't need
/// the filesystem.
///
/// Records written files in a thread-safe map keyed on
/// `(artifact_path, source_state, filename) -> body` for assertion visibility.
///
/// Note on adapter taxonomy (see `anvil-test-support/src/reflection_write.rs`):
/// This is a production-shape in-memory adapter that lives in `anvil-core-hearth/src/`
/// alongside the other in-memory adapters. Fault-injecting adapters (used for
/// write-failure testing) live in `anvil-test-support/` and are test infrastructure
/// for step definitions, not production code.
#[derive(Debug, Default)]
pub struct InMemoryReflectionWriteAdapter {
    /// Records written files as (artifact_path, source_state, filename) -> body.
    pub written_files: Mutex<HashMap<(String, String, String), String>>,
}

impl InMemoryReflectionWriteAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the body written for a specific file, if any.
    pub fn get_body(
        &self,
        artifact_path: &str,
        source_state: &str,
        filename: &str,
    ) -> Option<String> {
        let key = (
            artifact_path.to_string(),
            source_state.to_string(),
            filename.to_string(),
        );
        self.written_files.lock().unwrap().get(&key).cloned()
    }

    /// Return the total number of files written.
    pub fn write_count(&self) -> usize {
        self.written_files.lock().unwrap().len()
    }
}

impl ReflectionWritePort for InMemoryReflectionWriteAdapter {
    fn write_reflection_file(
        &self,
        artifact_path: &str,
        source_state: &str,
        filename: &str,
        body: &str,
    ) -> Result<String, ReflectionWriteError> {
        let key = (
            artifact_path.to_string(),
            source_state.to_string(),
            filename.to_string(),
        );
        self.written_files
            .lock()
            .unwrap()
            .insert(key, body.to_string());
        // Return an absolute-style path so callers can pattern-match on it.
        Ok(format!(
            "{}/{}_reflection/{}",
            artifact_path, source_state, filename
        ))
    }
}
