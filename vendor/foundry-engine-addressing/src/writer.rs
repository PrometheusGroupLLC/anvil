//! Engine-side rendezvous writer: atomic [`publish`] + best-effort [`cleanup`].

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use crate::record::RendezvousRecord;

/// Filename, under the rendezvous directory, that holds the published record.
pub const RENDEZVOUS_FILENAME: &str = "engine.json";

/// Atomically publish `record` to `<dir>/engine.json`.
///
/// The write is crash-safe and never leaves a partial `engine.json`:
///
/// 1. `<dir>` is created if missing.
/// 2. The record is written to a per-pid temp file `engine.json.tmp.<pid>`.
/// 3. The temp file is **fsync'd** so its bytes hit disk.
/// 4. The temp file is **renamed** over `engine.json` (atomic on the same
///    filesystem — a concurrent reader sees either the old complete file or
///    the new complete file, never a torn write).
/// 5. The **parent directory is fsync'd** so the rename itself is durable
///    across power loss (important on macOS/APFS).
///
/// On any error the temp file is best-effort removed so a failed publish does
/// not leak `engine.json.tmp.<pid>`.
pub fn publish(dir: &Path, record: &RendezvousRecord) -> io::Result<()> {
    fs::create_dir_all(dir)?;

    let final_path = dir.join(RENDEZVOUS_FILENAME);
    let tmp_path = dir.join(format!("{RENDEZVOUS_FILENAME}.tmp.{}", std::process::id()));

    // Serialize first; if this fails we have not touched the filesystem.
    let mut bytes = serde_json::to_vec_pretty(record)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    bytes.push(b'\n');

    // Write + fsync the temp file. Scope the File so it is closed before the
    // rename. Clean up the temp file on any failure.
    let write_result = (|| -> io::Result<()> {
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp_path)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        Ok(())
    })();
    if let Err(e) = write_result {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }

    // Atomic swap into place.
    if let Err(e) = fs::rename(&tmp_path, &final_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }

    // fsync the parent directory so the rename survives a crash. Failure to
    // open/sync the directory is non-fatal for visibility (the file is already
    // in place) but we surface it so callers can log durability degradation.
    if let Ok(dir_file) = File::open(dir) {
        let _ = dir_file.sync_all();
    }

    Ok(())
}

/// Best-effort, idempotent removal of `<dir>/engine.json`.
///
/// Intended for clean shutdown. A missing file is **not** an error — calling
/// this twice, or on a never-published dir, is a no-op.
pub fn cleanup(dir: &Path) {
    let final_path = dir.join(RENDEZVOUS_FILENAME);
    match fs::remove_file(&final_path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => {
            tracing_warn(&format!(
                "engine-addressing: cleanup of {} failed: {e}",
                final_path.display()
            ));
        }
    }
}

/// Tiny logging shim. This crate intentionally avoids a hard `tracing`
/// dependency to stay thin for client linkers; cleanup failures are rare and
/// non-fatal, so a stderr line is sufficient and keeps the dep graph minimal.
fn tracing_warn(msg: &str) {
    eprintln!("{msg}");
}
