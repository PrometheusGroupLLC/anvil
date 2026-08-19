//! Artifact-path containment guard — the single source of truth shared by the
//! MCP shim and the engine's filesystem adapters.
//!
//! `artifact_path` is, by the tool contract, a path UNDER the resolved hearth.
//! Two independent processes handle it: the shim validates it, then the engine
//! (a SEPARATE process) re-resolves it under the same hearth before writing.
//! Because a symlink inside the hearth (e.g. `link -> /proc/self/cwd`) can
//! resolve differently in each process, validation must be bound to use:
//!
//!   * the shim forwards the NORMALIZED, symlink-resolved, hearth-relative form
//!     produced by [`contained_relative_path`] (validated form == used form),
//!     so the engine re-derives the exact same location; and
//!   * every engine fs adapter that joins `artifact_path` under the hearth
//!     re-checks containment via [`escapes_hearth`] before touching disk — the
//!     engine never trusts that the shim already validated (defense in depth).
//!
//! [`is_syntactically_invalid`] is path-independent: it runs BEFORE any hearth
//! resolution so a `..`/drive-relative/rooted path is refused as an invalid
//! artifact path even when no hearth can be resolved (never masquerading as an
//! ambiguous-hearth failure).

use std::path::{Component, Path, PathBuf};

/// Path-independent syntax rejection. `true` when `artifact_path` can never be
/// a legitimate hearth-relative path regardless of any hearth or the on-disk
/// state:
///   * a `..` (`ParentDir`) traversal in any component;
///   * a Windows drive-relative form (`C:tracks\x`) — a `Prefix` with no root;
///   * a rooted-relative form (`\x`) — a leading separator that is not a full
///     absolute path.
///
/// Evaluated before hearth resolution so these are refused as
/// `invalid_artifact_path`, not `ambiguous_hearth`. The `Prefix`/`RootDir`
/// components only parse on Windows, so the same shapes are additionally
/// guarded at the string level for other platforms.
pub fn is_syntactically_invalid(artifact_path: &str) -> bool {
    let trimmed = artifact_path.trim();
    if trimmed.is_empty() {
        return false;
    }
    let raw = Path::new(trimmed);

    // `..` traversal anywhere in the path.
    if raw
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return true;
    }

    // A Windows `Prefix` (drive/UNC) present but the path is NOT absolute is a
    // drive-relative form (`C:tracks`). An absolute prefixed path (`C:\tracks`)
    // is left to the resolves-outside check.
    if raw
        .components()
        .any(|component| matches!(component, Component::Prefix(_)))
        && !raw.is_absolute()
    {
        return true;
    }

    // On non-Windows platforms the shapes above parse as ordinary components,
    // so guard the raw string too:
    //   * a leading backslash is a rooted-relative path (`\x`);
    //   * `<letter>:` not followed by a separator is drive-relative (`C:tracks`).
    if trimmed.starts_with('\\') {
        return true;
    }
    if is_drive_relative(trimmed) {
        return true;
    }

    false
}

/// `C:tracks` (drive-relative) but not `C:\tracks` / `C:/tracks` (absolute).
fn is_drive_relative(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && bytes.get(2).is_none_or(|c| *c != b'/' && *c != b'\\')
}

fn canonicalize(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Canonicalize the longest EXISTING ancestor of `path` and re-append the
/// not-yet-existing remainder. Resolves symlinks in the real on-disk segments
/// (defeating symlink escapes) even when the leaf artifact does not exist yet
/// (a snapshot may be about to create it).
fn canonicalize_existing_prefix(path: &Path) -> PathBuf {
    let mut ancestor = path;
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if ancestor.exists() {
            let mut resolved = canonicalize(ancestor);
            for segment in tail.iter().rev() {
                resolved.push(segment);
            }
            return resolved;
        }
        match (ancestor.parent(), ancestor.file_name()) {
            (Some(parent), Some(name)) => {
                tail.push(name.to_os_string());
                ancestor = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// The NORMALIZED, symlink-resolved, hearth-relative form of `artifact_path`
/// when it is safely contained under `hearth`; `Err(reason)` when it is
/// syntactically invalid or resolves OUTSIDE the hearth subtree.
///
/// This is the "validated form == used form" contract: the shim forwards this
/// exact relative path and the engine re-joins it under the same hearth to
/// reach the identical location — the intermediate symlinks are already
/// resolved away, so no separate process can re-resolve them differently. An
/// empty `artifact_path` yields an empty relative path (no artifact to check).
pub fn contained_relative_path(hearth: &Path, artifact_path: &str) -> Result<PathBuf, String> {
    let trimmed = artifact_path.trim();
    if trimmed.is_empty() {
        return Ok(PathBuf::new());
    }
    if is_syntactically_invalid(trimmed) {
        return Err(format!(
            "artifact_path must be a hearth-relative path with no '..', drive-relative, or rooted forms: {}",
            trimmed
        ));
    }

    let raw = Path::new(trimmed);
    let candidate = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        hearth.join(raw)
    };
    let hearth_canonical = canonicalize(hearth);
    let candidate_canonical = canonicalize_existing_prefix(&candidate);
    match candidate_canonical.strip_prefix(&hearth_canonical) {
        Ok(relative) => Ok(relative.to_path_buf()),
        Err(_) => Err(format!(
            "artifact_path {} resolves outside the resolved hearth {}",
            trimmed,
            hearth_canonical.display()
        )),
    }
}

/// `true` when joining `artifact_path` under `hearth` would escape the hearth
/// subtree (or the path is syntactically invalid). The engine's fs write
/// adapters call this at their join boundary so an escape is refused even if
/// the shim's normalization was bypassed — the engine independently enforces
/// containment (defense in depth).
pub fn escapes_hearth(hearth: &Path, artifact_path: &str) -> bool {
    contained_relative_path(hearth, artifact_path).is_err()
}
