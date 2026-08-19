//! In-memory implementation of `OpLogWritePort` for brine step definitions.
//!
//! Keys op logs by `(artifact_path, target_document)` so the adapter-agnostic
//! port contract can be exercised without a temp hearth (parity scenario).

use anvil_core::domain::amendment::{OpLog, OpLogEntry};
use anvil_core::ports::op_log_write_port::{OpLogWriteError, OpLogWritePort};
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Default)]
pub struct InMemoryOpLogAdapter {
    logs: Mutex<HashMap<(String, String), OpLog>>,
}

impl InMemoryOpLogAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    /// The current op log for `(artifact_path, target_document)` (empty when
    /// none recorded). Convenience reader for step assertions.
    pub fn op_log(&self, artifact_path: &str, target_document: &str) -> OpLog {
        self.logs
            .lock()
            .unwrap()
            .get(&(artifact_path.to_string(), target_document.to_string()))
            .cloned()
            .unwrap_or_default()
    }
}

impl OpLogWritePort for InMemoryOpLogAdapter {
    fn append_op(
        &self,
        artifact_path: &str,
        target_document: &str,
        entry: &OpLogEntry,
    ) -> Result<(), OpLogWriteError> {
        let mut logs = self.logs.lock().unwrap();
        let log = logs
            .entry((artifact_path.to_string(), target_document.to_string()))
            .or_default();
        // push_entry preserves op_id / accepted_at / seq (no re-numbering).
        log.push_entry(entry.clone());
        Ok(())
    }
}
