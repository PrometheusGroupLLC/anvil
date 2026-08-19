//! Baseline import: the one-shot, re-runnable act that gives a hearth a named
//! baseline whose tree equals its recorded governance state.
//!
//! Replay from empty is unattainable for state that predates the mechanism, so
//! the honest starting point is a baseline that replays byte-for-byte —
//! [`replay_baseline`] compares BYTES, never a parse, because a trailing
//! newline and a YAML key order are exactly the differences a parse-and-compare
//! would call equal.
//!
//! Lives in `hearth/`, not `domain/`, because it shells out and walks the disk.
//! `domain/` carries zero `Command::new` today and this mechanism does not
//! become the first.
//!
//! Everything here goes through [`crate::git_plumbing`]'s closed
//! subcommand set: `HEAD`, the repository index and the working tree are never
//! touched, and the ref update is a create-only compare-and-swap, so R4.4's
//! idempotence is enforced by git rather than by a pre-check that could race.

use anvil_core::domain::change_record::paths::{classify, PathClass};
use crate::change_record_commit::{
    anvil_dir, build_tree, commit_tree, stage_entries, ChangeRecordError,
};
use crate::git_plumbing::{
    run_plumbing, run_plumbing_text, GitEnv, Plumbing, BASELINE_REF, CHANGE_RECORD_REF,
};
use std::path::{Path, PathBuf};

/// What an import did.
#[derive(Debug, Clone)]
pub struct BaselineImport {
    /// The commit id `refs/anvil/baseline` names.
    pub baseline: String,
    /// How many recorded paths the baseline tree holds.
    pub paths_recorded: usize,
    /// True when a baseline already existed, so this run established none.
    pub already_imported: bool,
}

/// Why an import refused. Every variant names what was found, in the posture of
/// `GitCodeEvidenceAdapter`: a resolver that cannot locate the right repository
/// refuses rather than answering from the wrong one.
#[derive(Debug, Clone)]
pub enum BaselineImportError {
    /// The hearth lies inside a repository it does not own. Committing a user's
    /// hearth into their home-directory dotfiles repo is the same class of
    /// error as answering a merge claim from the wrong checkout.
    ForeignRepository { hearth: PathBuf, enclosing: PathBuf },
    /// A git invocation failed or could not be run at all.
    Git { detail: String },
    /// A filesystem read or write failed.
    Io { detail: String },
}

impl std::fmt::Display for BaselineImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BaselineImportError::ForeignRepository { hearth, enclosing } => write!(
                f,
                "refusing to import {}: it lies inside the repository {}, which anvil does not \
                 own. Initialize a repository for the hearth itself, or import the enclosing \
                 repository's own hearth.",
                hearth.display(),
                enclosing.display()
            ),
            BaselineImportError::Git { detail } => write!(f, "git: {}", detail),
            BaselineImportError::Io { detail } => write!(f, "io: {}", detail),
        }
    }
}

fn git_error(detail: impl Into<String>) -> BaselineImportError {
    BaselineImportError::Git {
        detail: detail.into(),
    }
}

fn io_error(detail: impl Into<String>) -> BaselineImportError {
    BaselineImportError::Io {
        detail: detail.into(),
    }
}

/// Resolve to the real path so a comparison against `rev-parse --show-toplevel`
/// is a comparison of the same thing. On macOS a temp dir is `/var/folders/…`
/// and git answers `/private/var/folders/…`; without this, every hearth would
/// look like it lived in a foreign repository.
fn real_path(path: &Path) -> Result<PathBuf, BaselineImportError> {
    std::fs::canonicalize(path)
        .map_err(|e| io_error(format!("could not resolve {}: {}", path.display(), e)))
}

/// Establish `refs/anvil/baseline` for `hearth` and point
/// `refs/anvil/change-record` at the same commit.
pub fn import_baseline(hearth: &Path) -> Result<BaselineImport, BaselineImportError> {
    let hearth = real_path(hearth)?;
    ensure_own_repository(&hearth)?;

    let recorded = recorded_paths(&hearth)?;
    let commit = write_baseline_commit(&hearth, &recorded)?;

    // Create-only: git's own compare-and-swap is what makes a re-import
    // idempotent. A pre-check would be a check-then-act with a window in it.
    let (code, _) = run_plumbing_text(
        &hearth,
        &Plumbing::UpdateRef {
            name: BASELINE_REF,
            new: &commit,
            old: "",
        },
        &GitEnv::default(),
        None,
    )
    .ok_or_else(|| git_error("could not run update-ref"))?;
    if code != 0 {
        let existing = read_ref(&hearth, BASELINE_REF)?
            .ok_or_else(|| git_error("update-ref refused but no baseline ref is present"))?;
        return Ok(BaselineImport {
            baseline: existing,
            paths_recorded: recorded.len(),
            already_imported: true,
        });
    }

    let (code, _) = run_plumbing_text(
        &hearth,
        &Plumbing::UpdateRef {
            name: CHANGE_RECORD_REF,
            new: &commit,
            old: "",
        },
        &GitEnv::default(),
        None,
    )
    .ok_or_else(|| git_error("could not run update-ref"))?;
    if code != 0 {
        return Err(git_error(format!(
            "the baseline landed at {} but {} could not be created",
            commit, CHANGE_RECORD_REF
        )));
    }

    Ok(BaselineImport {
        baseline: commit,
        paths_recorded: recorded.len(),
        already_imported: false,
    })
}

/// The hearth must own its repository. `init` is allowed HERE and nowhere else,
/// and never inside a repository anvil did not create: a process that creates
/// repositories as a side effect of serving an RPC is a much larger promise
/// than this mechanism makes.
fn ensure_own_repository(hearth: &Path) -> Result<(), BaselineImportError> {
    match toplevel(hearth)? {
        Some(top) if top == hearth => Ok(()),
        Some(enclosing) => Err(BaselineImportError::ForeignRepository {
            hearth: hearth.to_path_buf(),
            enclosing,
        }),
        None => {
            let (code, _) =
                run_plumbing_text(hearth, &Plumbing::Init, &GitEnv::default(), None)
                    .ok_or_else(|| git_error("could not run git init"))?;
            if code != 0 {
                return Err(git_error(format!(
                    "git init in {} exited {}",
                    hearth.display(),
                    code
                )));
            }
            Ok(())
        }
    }
}

fn toplevel(dir: &Path) -> Result<Option<PathBuf>, BaselineImportError> {
    let Some((code, out)) = run_plumbing_text(
        dir,
        &Plumbing::RevParseShowToplevel,
        &GitEnv::default(),
        None,
    ) else {
        return Err(git_error("could not run rev-parse --show-toplevel"));
    };
    if code != 0 || out.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(real_path(Path::new(out.trim()))?))
}

fn read_ref(repo: &Path, name: &str) -> Result<Option<String>, BaselineImportError> {
    let Some((code, out)) = run_plumbing_text(
        repo,
        &Plumbing::RevParseVerify(name),
        &GitEnv::default(),
        None,
    ) else {
        return Err(git_error("could not run rev-parse --verify"));
    };
    if code != 0 || out.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(out.trim().to_string()))
}

/// Every hearth-relative path the recorded set holds, sorted.
///
/// The classification is [`classify`] and nothing else — the writer and the
/// divergence report read the same declaration, so a path can never be recorded
/// by one and reported missing by the other.
pub fn recorded_paths(hearth: &Path) -> Result<Vec<String>, BaselineImportError> {
    let mut out = Vec::new();
    walk_recorded(hearth, Path::new(""), &mut out)?;
    out.sort();
    Ok(out)
}

fn walk_recorded(
    dir: &Path,
    prefix: &Path,
    out: &mut Vec<String>,
) -> Result<(), BaselineImportError> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| io_error(format!("read_dir {}: {}", dir.display(), e)))?;
    for entry in entries {
        let entry =
            entry.map_err(|e| io_error(format!("dir entry under {}: {}", dir.display(), e)))?;
        let relative = prefix.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|e| io_error(format!("file type of {}: {}", relative.display(), e)))?;
        if file_type.is_dir() {
            // `.git/` is never recorded and never descended into: it holds this
            // mechanism's own index and journals, which are bookkeeping ABOUT a
            // recording, not governance state.
            if classify(&relative) == PathClass::NeverRecorded {
                continue;
            }
            walk_recorded(&entry.path(), &relative, out)?;
        } else if classify(&relative) == PathClass::Recorded {
            out.push(to_slash(&relative));
        }
    }
    Ok(())
}

fn to_slash(path: &Path) -> String {
    path.components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect::<Vec<_>>()
        .join("/")
}

/// The private index. Inside `.git/`, so it is outside the worktree and the
/// repository's own `.git/index` is never opened — a human with a staged change
/// is untouched.
fn private_index_path(hearth: &Path) -> PathBuf {
    anvil_dir(hearth).join("import-index")
}

/// Hash, stage, write the tree and commit it — through the SAME primitives the
/// per-transaction writer uses. Two copies of a git write seam is the
/// duplication this mechanism exists not to repeat.
fn write_baseline_commit(
    hearth: &Path,
    recorded: &[String],
) -> Result<String, BaselineImportError> {
    let entries = stage_entries(hearth, recorded).map_err(from_commit_error)?;
    let tree = build_tree(hearth, &private_index_path(hearth), None, &entries)
        .map_err(from_commit_error)?;
    let message = baseline_message(hearth, recorded.len());
    commit_tree(hearth, &tree, None, &message, None).map_err(from_commit_error)
}

fn from_commit_error(error: ChangeRecordError) -> BaselineImportError {
    match error {
        ChangeRecordError::Io { detail } => BaselineImportError::Io { detail },
        other => BaselineImportError::Git {
            detail: other.to_string(),
        },
    }
}

/// The baseline's own message. Subject carries the command only — no title, no
/// path, no prose — and the trailers are drawn from the declared allowlist in
/// `anvil-core/schemas/change-record.md`.
fn baseline_message(hearth: &Path, paths_recorded: usize) -> String {
    format!(
        "anvil: baseline-import\n\nAnvil-Command: baseline-import\nAnvil-Repository-Label: \
         {}\nAnvil-Paths-Recorded: {}\n",
        repository_label(hearth),
        paths_recorded
    )
}

/// Basename only, under `project_label`'s existing rule — never a path.
fn repository_label(repo: &Path) -> String {
    repo.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_string()
}

/// `(object id, hearth-relative path)` for every blob under a ref.
fn tree_entries(repo: &Path, rev: &str) -> Result<Vec<(String, String)>, BaselineImportError> {
    let Some((code, listing)) =
        run_plumbing_text(repo, &Plumbing::LsTreeRecursive(rev), &GitEnv::default(), None)
    else {
        return Err(git_error("could not run ls-tree"));
    };
    if code != 0 {
        return Err(git_error(format!(
            "ls-tree over {} exited {} — is there a baseline?",
            rev, code
        )));
    }
    let mut out = Vec::new();
    for line in listing.lines() {
        let Some((meta, path)) = line.split_once('\t') else {
            continue;
        };
        let Some(object) = meta.split_whitespace().nth(2) else {
            continue;
        };
        out.push((object.to_string(), path.to_string()));
    }
    out.sort();
    Ok(out)
}

/// Every hearth-relative path reachable from the baseline tree, sorted.
pub fn baseline_tree_paths(hearth: &Path) -> Result<Vec<String>, BaselineImportError> {
    let hearth = real_path(hearth)?;
    Ok(tree_entries(&hearth, BASELINE_REF)?
        .into_iter()
        .map(|(_, path)| path)
        .collect())
}

/// Materialise the baseline tree into `into` and return the paths written.
pub fn replay_baseline(hearth: &Path, into: &Path) -> Result<Vec<String>, BaselineImportError> {
    let hearth = real_path(hearth)?;
    let mut written = Vec::new();
    for (object, relative) in tree_entries(&hearth, BASELINE_REF)? {
        let Some((code, bytes)) = run_plumbing(
            &hearth,
            &Plumbing::CatFileBlob(&object),
            &GitEnv::default(),
            None,
        ) else {
            return Err(git_error("could not run cat-file"));
        };
        if code != 0 {
            return Err(git_error(format!(
                "cat-file for {} ({}) exited {}",
                relative, object, code
            )));
        }
        let target = into.join(&relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| io_error(format!("create {}: {}", parent.display(), e)))?;
        }
        std::fs::write(&target, &bytes)
            .map_err(|e| io_error(format!("write {}: {}", target.display(), e)))?;
        written.push(relative);
    }
    written.sort();
    Ok(written)
}
