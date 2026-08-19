//! Atomic temp+rename whole-file write helper (spec Req 6, crash-safety
//! mechanism).
//!
//! Whole-file replacements of a live file (status.yaml, registry files,
//! projections, the actor block) MUST go through this helper, never an
//! in-place `std::fs::write` on the live path. The helper writes to a
//! `<file>.tmp` sibling and then `std::fs::rename`s it over the target. On a
//! same-directory, same-filesystem rename, POSIX guarantees atomicity, so a
//! crash mid-write cannot leave a torn live file — a reader either sees the
//! whole old file or the whole new file.
//!
//! Modeled on `fs_reflection_write_adapter.rs`'s write-tmp-then-rename pattern,
//! including best-effort cleanup of a partial `.tmp` on failure.
//!
//! # C-d.1 round 8 — this helper AMPLIFIES the read-then-write data-loss class
//!
//! Read this before writing any `read → default-on-failure → derive →
//! atomic_write` chain, because the last step is not the backstop it looks like.
//!
//! `std::fs::write` on a live path needs write permission **on the file**. This
//! helper writes a `<file>.tmp` sibling and renames it, so it needs write
//! permission **on the directory** and none at all on the file. That means:
//!
//! > **a read-then-atomic-write pair loses data at modes where the same pair
//! > using a plain `fs::write` would have refused — including `0000`.**
//!
//! Measured on `append_spark_source_event` at `63df2ff`, before it was fixed:
//!
//! ```text
//! sparks.md 0644 : read ok      atomic_write Ok  -> 3 sparks survive  (control)
//! sparks.md 0200 : read FAILED  atomic_write Ok  -> 1 spark  survives
//! sparks.md 0000 : read FAILED  atomic_write Ok  -> 1 spark  survives
//! ```
//!
//! The file's own mode is irrelevant to the write, so it cannot stop a
//! re-serialize built from a read that failed. **The READ has to refuse.** Every
//! site that re-serializes a whole document it just read is responsible for its
//! own refusal; there is nothing this helper can add that would make a swallowed
//! read safe, and its crash-safety guarantee is orthogonal to that.
//!
//! This is not a defect in the helper — atomicity is exactly what it promises
//! and it delivers it. It is a property of the helper that the sites above it
//! must know, and it went unwritten through seven rounds of this class.

use std::io;
use std::path::Path;

/// Whether the `.tmp` sibling existed at the instant between the temp write
/// and the rename. Used by the Slice A library-seam test to prove the write
/// went through temp+rename (an in-place write would never produce a sibling).
#[derive(Debug, Clone)]
pub struct AtomicObservation {
    pub temp_sibling_existed_mid_write: bool,
}

/// Atomically replace `path` with `contents` via a temp sibling + rename.
pub fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    atomic_write_inner(path, contents, &mut |_| {})
}

/// Like [`atomic_write`], but invokes a caller-provided observer with the temp
/// path at the instant after the temp file is fully written and before the
/// rename. Returns an [`AtomicObservation`] recording whether the temp sibling
/// existed at that instant. This is the deterministic, in-process seam the
/// Slice A barrier test uses — no flaky timing, no test-only API leaks into the
/// production call sites (which use the plain [`atomic_write`]).
pub fn atomic_write_observed(path: &Path, contents: &[u8]) -> io::Result<AtomicObservation> {
    let mut existed = false;
    atomic_write_inner(path, contents, &mut |tmp: &Path| {
        existed = tmp.exists();
    })?;
    Ok(AtomicObservation {
        temp_sibling_existed_mid_write: existed,
    })
}

fn atomic_write_inner(
    path: &Path,
    contents: &[u8],
    mid_write: &mut dyn FnMut(&Path),
) -> io::Result<()> {
    let tmp_path = temp_path_for(path);

    // Write to the temp sibling first. On failure, best-effort clean up the
    // partial temp file so no stray `.tmp` is left behind.
    if let Err(e) = std::fs::write(&tmp_path, contents) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }

    // Mid-write observation point: the temp sibling is fully written but the
    // rename has not yet happened.
    mid_write(&tmp_path);

    // Atomic rename over the live path.
    if let Err(e) = std::fs::rename(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }

    Ok(())
}

/// Compute the `<file>.tmp` sibling path for `path`. The temp file lives in the
/// same directory so the rename is same-filesystem (and therefore atomic).
fn temp_path_for(path: &Path) -> std::path::PathBuf {
    let mut file_name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    file_name.push(".tmp");
    match path.parent() {
        Some(parent) => parent.join(file_name),
        None => std::path::PathBuf::from(file_name),
    }
}
