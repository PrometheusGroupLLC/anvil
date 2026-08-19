//! Filesystem implementation of `OpLogWritePort`.
//!
//! The op-log file `<artifact_path>/<target_document>.amendments.yaml` is wholly
//! owned by this port (no foreign sections to preserve), so — unlike the
//! activity adapter's line-level edit — a full re-serialize of the [`OpLog`] is
//! correct and simplest. `#[serde(transparent)]` on `OpLog` makes the round-trip
//! a clean YAML array. The write goes through `atomic_write` (temp + rename).

use anvil_core::domain::amendment::{OpLog, OpLogEntry};
use anvil_core::ports::op_log_write_port::{op_log_file_name, OpLogWriteError, OpLogWritePort};
use std::path::PathBuf;

pub struct FileSystemOpLogAdapter {
    hearth_path: PathBuf,
}

impl FileSystemOpLogAdapter {
    pub fn new(hearth_path: PathBuf) -> Self {
        Self { hearth_path }
    }
}

impl OpLogWritePort for FileSystemOpLogAdapter {
    fn append_op(
        &self,
        artifact_path: &str,
        target_document: &str,
        entry: &OpLogEntry,
    ) -> Result<(), OpLogWriteError> {
        let dir = self.hearth_path.join(artifact_path);
        let file = dir.join(op_log_file_name(target_document));

        // Read the existing log (empty when absent), preserving fields verbatim.
        //
        // ── C-d.1 round 8, H-3: the WRITE twin of the read round 7 fixed ──
        //
        // Round 7 converted `fs_query_adapter::read_op_log`'s `!file.exists()`
        // with the note *"an unreadable log answering `OpLog::new()` is 'this
        // document has never been amended', which is a clean slate over a file
        // that holds the amendments."* The identical predicate on the WRITE
        // side, in the port that OWNS the same file, was left standing — and
        // here the clean slate is not merely returned, it is `atomic_write`ten
        // over the whole amendment history, because the write below is a FULL
        // re-serialize of the log.
        //
        // Its local reachability is the honest `EIO`/`ESTALE` case §40.9(3)
        // declares: `stat` on the file fails only when the directory lacks `x`,
        // and that same mode defeats the temp-sibling write. Converted anyway,
        // for exactly the reason §40.9(3) gives — the errors that carry no such
        // courtesy — and because one round converting the read of a file and
        // leaving the write of the same file is the asymmetry that IS the
        // finding.
        let mut log = if anvil_core::domain::playbook::fs_probe::node_kind(&file).map_err(|e| {
            OpLogWriteError::IoError {
                message: format!(
                    "op_log_uninspectable: {} could not be inspected: {e}. Refusing to write. \
                     Treating it as absent starts a fresh log and re-serializes THAT over the \
                     amendments on disk.",
                    file.display()
                ),
            }
        })? != anvil_core::domain::playbook::fs_probe::NodeKind::Absent
        {
            let content = std::fs::read_to_string(&file).map_err(|e| OpLogWriteError::IoError {
                message: format!("Failed to read op log: {}", e),
            })?;
            serde_yaml::from_str::<OpLog>(&content).map_err(|e| {
                OpLogWriteError::MalformedOpLog {
                    artifact_path: artifact_path.to_string(),
                    target_document: target_document.to_string(),
                    message: format!("Invalid YAML: {}", e),
                }
            })?
        } else {
            OpLog::new()
        };

        // push_entry preserves op_id / accepted_at / seq (no re-numbering).
        log.push_entry(entry.clone());

        let serialized = serde_yaml::to_string(&log).map_err(|e| OpLogWriteError::IoError {
            message: format!("Failed to serialize op log: {}", e),
        })?;

        std::fs::create_dir_all(&dir).map_err(|e| OpLogWriteError::IoError {
            message: format!("Failed to create artifact dir: {}", e),
        })?;

        crate::atomic_write::atomic_write(&file, serialized.as_bytes()).map_err(|e| {
            OpLogWriteError::IoError {
                message: format!("Failed to write op log: {}", e),
            }
        })
    }
}
