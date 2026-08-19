//! Hearth discovery for cross-hearth (`all_hearths`) read-folds.
//!
//! The engine's `all_hearths` queries fold the durable activity sinks of EVERY
//! reachable project hearth. A permitted root, however, is frequently a PARENT
//! directory (e.g. `~/Development`) that itself holds NO sink — the real data
//! lives in sub-hearths beneath it (`foundry-hearth/`, `kiln-hearth/`, …).
//!
//! `discover_hearths` turns the set of permitted roots + the explicit default
//! hearth into the concrete set of hearth directories to fold:
//!
//! - Each permitted root that is ITSELF a genuine hearth is included.
//! - Each immediate (1-level) non-symlink subdirectory of each root that is a
//!   genuine hearth is included. Only one level is scanned — no deep walk — and
//!   symlinked entries are never followed.
//! - The explicit default hearth (the engine's `--hearth`) is always included.
//! - The final set is deduplicated (a hearth reachable both as `--hearth` and
//!   under a root appears once) and returned in a stable (sorted) order.
//!
//! A directory is a genuine hearth ONLY when it holds the artifact store
//! DIRECTLY — a real `tracks/` dir AND a real `tracks.md` registry (the same
//! predicate the engine's `resolve_hearth` gate uses). A project/code dir that
//! merely POINTS at a hearth (a `.hearth` pointer file, or a `forge/`→hearth
//! symlink) is NOT a hearth: counting it would double-fold the hearth it points
//! at. Bare durable sinks (`activity-log.jsonl`, …) alone do not qualify a dir.
//!
//! This is purely an ENUMERATION concern for read-folding. It does NOT grant or
//! widen write authority — the engine's `resolve_hearth` permitted-root gate is
//! unchanged.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// A directory IS a hearth only when it holds the artifact store DIRECTLY —
/// the SAME structural predicate the engine's `resolve_hearth` (and
/// `canonicalize_startup_hearth`) gate on: a real `tracks/` directory AND a real
/// `tracks.md` registry file present AS REAL ENTRIES (not via a symlink).
///
/// This intentionally REJECTS:
///
/// - A project/code dir that merely POINTS at a hearth via a `.hearth` pointer
///   file (the hearth path lives INSIDE that file; the project dir is not itself
///   a hearth — its `.hearth` target is discovered at its own real path).
/// - A project dir whose `forge/` is a SYMLINK to its hearth — following that
///   symlink would double-fold the same data under the project's path; the
///   hearth is discovered at its own real location instead. We require the
///   `tracks/` entry to be a real directory, not a symlink.
/// - A bare durable sink (`activity-log.jsonl`, …) sitting in an otherwise
///   non-hearth dir: a sink alone does not make a hearth.
///
/// Net effect: discovery returns only genuine `*-hearth` directories, excluding
/// code repos, transient git worktrees, and parent dirs.
pub fn looks_like_hearth(dir: &Path) -> bool {
    if !dir.is_dir() {
        return false;
    }
    // `tracks/` must be a REAL directory (not a symlink) — symlink_metadata does
    // NOT traverse the link, so a `tracks` symlink reads as a symlink and fails
    // the `is_dir()` check, keeping a project's symlinked hearth from being
    // pulled in under the project's path.
    let tracks = dir.join("tracks");
    let has_real_tracks_dir = std::fs::symlink_metadata(&tracks)
        .map(|m| m.file_type().is_dir())
        .unwrap_or(false);
    let has_registry = std::fs::symlink_metadata(dir.join("tracks.md"))
        .map(|m| m.file_type().is_file())
        .unwrap_or(false);
    has_real_tracks_dir && has_registry
}

/// Discover the concrete set of hearth directories to fold for an `all_hearths`
/// query, given the permitted roots and the optional explicit default hearth.
///
/// See the module docs for the discovery rule. The returned vector is sorted and
/// deduplicated.
pub fn discover_hearths(
    permitted_roots: &[PathBuf],
    explicit_hearth: Option<&Path>,
) -> Vec<PathBuf> {
    let mut found: BTreeSet<PathBuf> = BTreeSet::new();

    for root in permitted_roots {
        if !root.is_dir() {
            continue;
        }
        // The root itself may be a hearth.
        if looks_like_hearth(root) {
            found.insert(root.clone());
        }
        // Scan immediate subdirectories only (1 level) — never deep-walk, and
        // never FOLLOW a symlinked entry: a code repo's `forge/`→hearth symlink
        // must not pull the hearth in under the project's path (it is discovered
        // at its own real location). Skip any entry that is itself a symlink.
        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries.flatten() {
                let is_symlink = entry.file_type().map(|ft| ft.is_symlink()).unwrap_or(false);
                if is_symlink {
                    continue;
                }
                let path = entry.path();
                if looks_like_hearth(&path) {
                    found.insert(path);
                }
            }
        }
    }

    // The explicit `--hearth` is always folded, even if it lives outside every
    // scanned root's immediate children. Dedup handles overlap.
    if let Some(hearth) = explicit_hearth {
        if hearth.is_dir() {
            found.insert(hearth.to_path_buf());
        }
    }

    found.into_iter().collect()
}
