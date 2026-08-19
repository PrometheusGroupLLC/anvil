use anvil_core::ports::reflection_write_port::{ReflectionWriteError, ReflectionWritePort};
use std::path::PathBuf;

/// Filesystem implementation of `ReflectionWritePort`.
///
/// Writes reflection files to `<hearth_path>/<artifact_path>/<source_state>_reflection/<filename>`.
/// Uses an atomic write-to-temp + rename pattern to avoid partial files on failure.
///
/// The adapter does NOT guard against an already-existing file at the target path —
/// filenames are uniquely determined by `(transition_at, actor_name)` so collisions
/// are impossible under the state-machine one-transition-at-a-time invariant.
#[derive(Clone)]
pub struct FileSystemReflectionWriteAdapter {
    hearth_path: PathBuf,
}

impl FileSystemReflectionWriteAdapter {
    pub fn new(hearth_path: PathBuf) -> Self {
        Self { hearth_path }
    }
}

impl ReflectionWritePort for FileSystemReflectionWriteAdapter {
    fn write_reflection_file(
        &self,
        artifact_path: &str,
        source_state: &str,
        filename: &str,
        body: &str,
    ) -> Result<String, ReflectionWriteError> {
        // Step 1: compute subdirectory path.
        let subdir = self
            .hearth_path
            .join(artifact_path)
            .join(format!("{}_reflection", source_state));

        // Step 2: create subdirectory (idempotent via create_dir_all).
        // If create_dir_all fails for any reason (permission denied, path
        // component is an existing file, etc.), the error is mapped to IoError
        // and surfaced to the caller — no subsequent write steps are attempted.
        let subdir_str = subdir.display().to_string();
        std::fs::create_dir_all(&subdir).map_err(|e| ReflectionWriteError::IoError {
            path: subdir_str.clone(),
            message: e.to_string(),
        })?;

        // Step 3: full file path and temp path.
        let final_path = subdir.join(filename);
        let final_path_str = final_path.display().to_string();
        let tmp_filename = format!("{}.tmp", filename);
        let tmp_path = subdir.join(&tmp_filename);

        // Step 4: atomic write — write to temp, then rename.
        // Two distinct cleanup obligations:
        // (a) Partial-write cleanup: if write(tmp) fails mid-write, a partial .tmp
        //     file may be left behind. The best-effort unlink below addresses this.
        // (b) Rename infallibility after a complete temp-file write: once write(tmp)
        //     succeeds, rename(tmp, final) is a same-directory same-filesystem
        //     operation, which POSIX guarantees atomic. No partial visible state
        //     is possible at this step.
        std::fs::write(&tmp_path, body).map_err(|e| {
            // Best-effort cleanup of partial .tmp file (obligation a).
            let _ = std::fs::remove_file(&tmp_path);
            ReflectionWriteError::IoError {
                path: final_path_str.clone(),
                message: e.to_string(),
            }
        })?;

        std::fs::rename(&tmp_path, &final_path).map_err(|e| {
            // Best-effort cleanup if rename somehow fails (should not happen
            // on same-directory same-filesystem, but log and ignore).
            let _ = std::fs::remove_file(&tmp_path);
            ReflectionWriteError::IoError {
                path: final_path_str.clone(),
                message: e.to_string(),
            }
        })?;

        // Step 5: return absolute path.
        Ok(final_path_str)
    }
}
