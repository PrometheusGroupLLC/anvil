//! One change-record commit: stage the transaction's paths into a PRIVATE
//! index seeded from the parent's tree, commit under the fixed engine
//! identity, and move the ref by compare-and-swap.
//!
//! Everything reaches git through [`crate::git_plumbing`]'s closed subcommand
//! set, so HEAD, the repository's own index and the working tree are untouched
//! by construction rather than by review.
//!
//! Lives in this crate, not in `anvil-core`'s `domain/`, because it shells out.

use crate::git_plumbing::{
    run_plumbing, run_plumbing_text, GitEnv, Plumbing, ANVIL_GIT_DIR, CHANGE_RECORD_REF,
};
use anvil_core::domain::change_record::message::{trailer_value, DECLARED_TRAILER_KEYS};
use std::path::{Path, PathBuf};

/// Why a change-record write refused. Never a swallowed warning: the
/// anti-precedent is a durable write path that warns once and is never read
/// back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeRecordError {
    Git { detail: String },
    Io { detail: String },
    /// A live value matching neither the journal's expectation nor the desired
    /// bytes. NEVER overwritten — reported, in the backlog journal's posture.
    Conflict { detail: String },
    /// The debug-only injected crash point fired. Unreachable in a release
    /// build; see `change_record_journal::crash_after`.
    TestCrash { at: String },
}

impl std::fmt::Display for ChangeRecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChangeRecordError::Git { detail } => write!(f, "git: {}", detail),
            ChangeRecordError::Io { detail } => write!(f, "io: {}", detail),
            ChangeRecordError::Conflict { detail } => write!(f, "conflict: {}", detail),
            ChangeRecordError::TestCrash { at } => write!(f, "test crash after {}", at),
        }
    }
}

pub fn git_err(detail: impl Into<String>) -> ChangeRecordError {
    ChangeRecordError::Git {
        detail: detail.into(),
    }
}

pub fn io_err(detail: impl Into<String>) -> ChangeRecordError {
    ChangeRecordError::Io {
        detail: detail.into(),
    }
}

/// This mechanism's private area, inside `.git/` so it is outside the worktree
/// and the exhaustive path declaration never has to name it.
pub fn anvil_dir(repo: &Path) -> PathBuf {
    repo.join(".git").join(ANVIL_GIT_DIR)
}

fn text(
    repo: &Path,
    plumbing: &Plumbing<'_>,
    env: &GitEnv,
    stdin: Option<&[u8]>,
    what: &str,
) -> Result<String, ChangeRecordError> {
    let Some((code, out)) = run_plumbing_text(repo, plumbing, env, stdin) else {
        return Err(git_err(format!("could not run {}", what)));
    };
    if code != 0 {
        return Err(git_err(format!("{} exited {}", what, code)));
    }
    Ok(out.trim().to_string())
}

/// A ref's commit, or `None` when it does not exist. A missing ref is never an
/// error: an un-imported repository has neither.
pub fn read_ref(repo: &Path, name: &str) -> Result<Option<String>, ChangeRecordError> {
    let Some((code, out)) = run_plumbing_text(
        repo,
        &Plumbing::RevParseVerify(name),
        &GitEnv::default(),
        None,
    ) else {
        return Err(git_err("could not run rev-parse --verify"));
    };
    if code != 0 || out.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(out.trim().to_string()))
}

/// Compare-and-swap. `old` is the tip observed when the parent was chosen, or
/// `None` to assert the ref must not yet exist. Returns whether it landed —
/// a nonzero exit is a LOST RACE, never a force update.
pub fn update_ref_cas(
    repo: &Path,
    name: &str,
    new: &str,
    old: Option<&str>,
) -> Result<bool, ChangeRecordError> {
    let Some((code, _)) = run_plumbing_text(
        repo,
        &Plumbing::UpdateRef {
            name,
            new,
            old: old.unwrap_or(""),
        },
        &GitEnv::default(),
        None,
    ) else {
        return Err(git_err("could not run update-ref"));
    };
    Ok(code == 0)
}

/// One object's headers and message.
pub fn cat_file(repo: &Path, object: &str) -> Result<String, ChangeRecordError> {
    let Some((code, out)) = run_plumbing(
        repo,
        &Plumbing::CatFilePretty(object),
        &GitEnv::default(),
        None,
    ) else {
        return Err(git_err("could not run cat-file -p"));
    };
    if code != 0 {
        return Err(git_err(format!("cat-file -p {} exited {}", object, code)));
    }
    Ok(String::from_utf8_lossy(&out).to_string())
}

/// Write a blob and return its object id.
pub fn hash_blob(repo: &Path, bytes: &[u8]) -> Result<String, ChangeRecordError> {
    text(
        repo,
        &Plumbing::HashObjectStdin,
        &GitEnv::default(),
        Some(bytes),
        "hash-object",
    )
}

/// Executable bit preserved; everything else is a regular blob. A mode this
/// does not recognise would silently change the file on replay.
pub fn blob_mode(path: &Path) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = std::fs::metadata(path) {
            if metadata.permissions().mode() & 0o111 != 0 {
                return "100755".to_string();
            }
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    "100644".to_string()
}

/// Hash every path and return `(mode, object, path)` cacheinfo entries.
pub fn stage_entries(
    repo: &Path,
    paths: &[String],
) -> Result<Vec<(String, String, String)>, ChangeRecordError> {
    let mut entries = Vec::with_capacity(paths.len());
    for relative in paths {
        let absolute = repo.join(relative);
        let bytes = std::fs::read(&absolute)
            .map_err(|e| io_err(format!("read {}: {}", absolute.display(), e)))?;
        let object = hash_blob(repo, &bytes)?;
        entries.push((blob_mode(&absolute), object, relative.clone()));
    }
    Ok(entries)
}

/// Build a tree in `index`, optionally seeded from `base_tree`.
///
/// The index path is load-bearing: it lives under `.git/anvil/`, so the
/// repository's own `.git/index` is never opened and a human with a staged
/// change is untouched.
pub fn build_tree(
    repo: &Path,
    index: &Path,
    base_tree: Option<&str>,
    entries: &[(String, String, String)],
) -> Result<String, ChangeRecordError> {
    if let Some(parent) = index.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| io_err(format!("create {}: {}", parent.display(), e)))?;
    }
    // Start from an empty index every time: this index is ours and carries no
    // state between runs.
    if index.exists() {
        std::fs::remove_file(index)
            .map_err(|e| io_err(format!("remove {}: {}", index.display(), e)))?;
    }
    let env = GitEnv {
        index_file: Some(index.to_path_buf()),
        commit_date: None,
    };
    if let Some(tree) = base_tree {
        text(repo, &Plumbing::ReadTree(tree), &env, None, "read-tree")?;
    }
    // Chunked so a hearth with thousands of governance files cannot overrun
    // the platform argv limit.
    for chunk in entries.chunks(128) {
        text(
            repo,
            &Plumbing::UpdateIndexCacheInfo { entries: chunk },
            &env,
            None,
            "update-index",
        )?;
    }
    let tree = text(repo, &Plumbing::WriteTree, &env, None, "write-tree")?;
    if tree.is_empty() {
        return Err(git_err("write-tree produced no tree"));
    }
    Ok(tree)
}

/// Build a commit object. Identity comes from the environment, never from
/// config, so a hearth owner's real name and email cannot become
/// engine-authored record metadata.
///
/// `date` pins the author and committer timestamps. Passing the transaction's
/// declared `at` is what makes the object REPRODUCIBLE: a rolled-forward
/// commit then hashes to the same object id as the one an uninterrupted run
/// would have written, so a duplicate is not merely detected — it cannot be a
/// different object. Left unset the commit is stamped "now", which is right for
/// a one-shot import and wrong for anything recovery may re-derive.
pub fn commit_tree(
    repo: &Path,
    tree: &str,
    parent: Option<&str>,
    message: &str,
    date: Option<&str>,
) -> Result<String, ChangeRecordError> {
    let env = GitEnv {
        index_file: None,
        commit_date: date.filter(|d| !d.is_empty()).map(str::to_string),
    };
    let commit = text(
        repo,
        &Plumbing::CommitTree {
            tree,
            parent,
            message,
        },
        &env,
        None,
        "commit-tree",
    )?;
    if commit.is_empty() {
        return Err(git_err("commit-tree produced no commit"));
    }
    Ok(commit)
}

/// The tree a commit names.
pub fn tree_of(repo: &Path, commit: &str) -> Result<String, ChangeRecordError> {
    header_of(&cat_file(repo, commit)?, "tree")
        .ok_or_else(|| git_err(format!("commit {} names no tree", commit)))
}

fn header_of(object: &str, key: &str) -> Option<String> {
    object
        .lines()
        .take_while(|line| !line.is_empty())
        .find_map(|line| line.strip_prefix(key).and_then(|rest| rest.strip_prefix(' ')))
        .map(|value| value.trim().to_string())
}

/// How far back the lineage is walked before the search is called a defect
/// rather than a deep history. An interrupted operation's commit is at or
/// within a few of the tip; anything deeper means the journalled parent is not
/// on this lineage at all.
const MAX_LINEAGE_SCAN: usize = 512;

/// The commit carrying `operation_id`, searched from `tip` back to (but not
/// including) `stop_at` — the tip the journal observed. This read IS the
/// idempotency key: it is what makes a second recovery pass add no commit.
pub fn commit_for_operation(
    repo: &Path,
    tip: Option<&str>,
    stop_at: Option<&str>,
    operation_id: &str,
) -> Result<Option<String>, ChangeRecordError> {
    let mut current = tip.map(str::to_string);
    for _ in 0..MAX_LINEAGE_SCAN {
        let Some(sha) = current else {
            return Ok(None);
        };
        if Some(sha.as_str()) == stop_at {
            return Ok(None);
        }
        let object = cat_file(repo, &sha)?;
        if trailer_value(&object, DECLARED_TRAILER_KEYS[0]).as_deref() == Some(operation_id) {
            return Ok(Some(sha));
        }
        current = header_of(&object, "parent");
    }
    Err(git_err(format!(
        "walked {} commits of {} without reaching the journalled parent",
        MAX_LINEAGE_SCAN, CHANGE_RECORD_REF
    )))
}
