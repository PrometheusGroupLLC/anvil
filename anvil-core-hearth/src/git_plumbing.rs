//! The one place this crate shells out to `git`.
//!
//! `run_git`, `git_toplevel` and `rev_parse` were private to
//! `git_code_evidence_adapter.rs` — the only subprocess caller in anvil-core.
//! They were MOVED here rather than copied, because two copies of a git seam is
//! the duplication pattern this codebase has already paid for twice. The
//! evidence adapter's read-only calls reach them unchanged.
//!
//! ## The never-touch-HEAD constraint
//!
//! The change record writes only to its own refs while a human commits normally
//! in the same repository. That co-tenancy rests entirely on the engine never
//! touching `HEAD`, the repository index, or the working tree — so the
//! subcommand set is **closed** ([`Plumbing`]), not merely reviewed. A verb
//! outside it is unrepresentable rather than unnoticed, and
//! [`FORBIDDEN_SUBCOMMANDS`] is asserted at the argv boundary so a future
//! variant cannot smuggle one in.
//!
//! Two further properties, both load-bearing:
//!
//! - **`GIT_INDEX_FILE`.** Every `update-index` / `write-tree` runs against a
//!   private index under `<repo>/.git/anvil/`. The repository's own
//!   `.git/index` is never opened, so a human with a staged change is
//!   untouched.
//! - **Identity from the environment.** `commit-tree` sets `GIT_AUTHOR_*` and
//!   `GIT_COMMITTER_*` explicitly. The ambient `user.name` / `user.email` is
//!   never read: a hearth owner's real name and email must not become
//!   engine-authored record metadata.
//!
//! No remote is read or written, ever. There is no network verb in the
//! allowlist and every network verb is in the forbidden list.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The moving tip of the engine-authored lineage.
pub const CHANGE_RECORD_REF: &str = "refs/anvil/change-record";

/// The immutable first commit of that lineage. The EXISTENCE of this ref is
/// the `has a baseline` predicate.
pub const BASELINE_REF: &str = "refs/anvil/baseline";

/// The fixed engine identity. Constant across every commit and every hearth.
pub const ENGINE_IDENTITY_NAME: &str = "anvil";
/// See [`ENGINE_IDENTITY_NAME`].
pub const ENGINE_IDENTITY_EMAIL: &str = "anvil@localhost";

/// Where this mechanism keeps its private index files and journals — inside
/// `.git/`, so it is outside the worktree and the exhaustive path declaration
/// never has to name it as a residual.
pub const ANVIL_GIT_DIR: &str = "anvil";

/// Every subcommand the change record may run. Declared as data alongside the
/// closed [`Plumbing`] enum so the allowlist is readable in one place and
/// assertable at the argv boundary.
///
/// Deliberately NOT a list of every verb this file can produce: the completion
/// merge check reaches [`run_git`] directly with raw args
/// (`merge-base --is-ancestor`, `branch --all --contains`) — a read-only seam
/// that predates the enum and writes no object, ref, index or worktree. This
/// allowlist governs the change record's WRITE path, which is the path that
/// could disturb a human's checkout.
pub const ALLOWED_SUBCOMMANDS: &[&str] = &[
    "init",
    "rev-parse",
    "hash-object",
    "read-tree",
    "update-index",
    "write-tree",
    "commit-tree",
    "update-ref",
    "ls-tree",
    "cat-file",
    "diff-tree",
];

/// Verbs that would touch `HEAD`, the index, the worktree or a network remote.
/// Never merely a comment: [`argv_of`] asserts against this list, so a future
/// [`Plumbing`] variant that renders one of these fails loudly at its first
/// call rather than quietly rewriting somebody's checkout.
pub const FORBIDDEN_SUBCOMMANDS: &[&str] = &[
    "add", "commit", "stash", "reset", "checkout", "switch", "restore", "clean", "merge", "rebase",
    "cherry-pick", "revert", "rm", "mv", "am", "apply", "push", "fetch", "pull", "clone", "remote",
];

/// The closed set of plumbing invocations. A caller cannot express `git commit`
/// because there is no variant for it.
pub enum Plumbing<'a> {
    /// Baseline import only, and never inside a foreign repository.
    Init,
    /// Resolve the work tree root, or `None` when there is no repository.
    RevParseShowToplevel,
    /// Resolve a rev quietly; a missing ref is `None`, never an error.
    RevParseVerify(&'a str),
    /// Write a blob from stdin and print its object id.
    HashObjectStdin,
    /// Stage one entry into the PRIVATE index. `mode` is `100644` / `100755`.
    UpdateIndexCacheInfo {
        entries: &'a [(String, String, String)],
    },
    /// Write the private index out as a tree object.
    WriteTree,
    /// Build a commit object. Identity comes from the environment.
    CommitTree {
        tree: &'a str,
        parent: Option<&'a str>,
        message: &'a str,
    },
    /// Compare-and-swap a ref. `old` is the tip observed when the parent was
    /// chosen, or `""` to assert the ref must not yet exist. Never forced.
    UpdateRef {
        name: &'a str,
        new: &'a str,
        old: &'a str,
    },
    /// Seed the PRIVATE index from an existing tree, so a transaction's tree
    /// is the parent's plus the paths it wrote — never the paths alone.
    ReadTree(&'a str),
    /// `mode SP type SP object TAB path` for every blob under a tree.
    LsTreeRecursive(&'a str),
    /// Raw bytes of one object.
    CatFileBlob(&'a str),
    /// One object rendered by type — the commit headers and message. Used to
    /// read `Anvil-Operation-Id` back off the lineage, which is the recovery
    /// path's idempotency key.
    CatFilePretty(&'a str),
}

impl Plumbing<'_> {
    fn argv(&self) -> Vec<String> {
        let own = |s: &str| s.to_string();
        match self {
            Plumbing::Init => vec![own("init"), own("--quiet")],
            Plumbing::RevParseShowToplevel => vec![own("rev-parse"), own("--show-toplevel")],
            Plumbing::RevParseVerify(rev) => vec![
                own("rev-parse"),
                own("--verify"),
                own("--quiet"),
                own(rev),
            ],
            Plumbing::HashObjectStdin => {
                vec![own("hash-object"), own("-w"), own("--stdin")]
            }
            Plumbing::UpdateIndexCacheInfo { entries } => {
                let mut argv = vec![own("update-index"), own("--add")];
                for (mode, object, path) in entries.iter() {
                    argv.push(own("--cacheinfo"));
                    argv.push(format!("{},{},{}", mode, object, path));
                }
                argv
            }
            Plumbing::WriteTree => vec![own("write-tree")],
            Plumbing::CommitTree {
                tree,
                parent,
                message,
            } => {
                let mut argv = vec![own("commit-tree"), own(tree)];
                if let Some(parent) = parent {
                    argv.push(own("-p"));
                    argv.push(own(parent));
                }
                argv.push(own("-m"));
                argv.push(own(message));
                argv
            }
            Plumbing::UpdateRef { name, new, old } => {
                vec![own("update-ref"), own(name), own(new), own(old)]
            }
            Plumbing::ReadTree(tree) => vec![own("read-tree"), own(tree)],
            Plumbing::LsTreeRecursive(rev) => vec![own("ls-tree"), own("-r"), own(rev)],
            Plumbing::CatFileBlob(object) => vec![own("cat-file"), own("blob"), own(object)],
            Plumbing::CatFilePretty(object) => vec![own("cat-file"), own("-p"), own(object)],
        }
    }
}

/// Render a [`Plumbing`] to argv, refusing anything outside the allowlist.
///
/// A panic here is the right posture: reaching it means a variant renders a
/// verb the never-touch-HEAD constraint forbids, which is a defect in this file
/// and not a runtime condition a caller can handle.
fn argv_of(plumbing: &Plumbing<'_>) -> Vec<String> {
    let argv = plumbing.argv();
    let subcommand = argv
        .first()
        .expect("every Plumbing variant renders a subcommand");
    assert!(
        !FORBIDDEN_SUBCOMMANDS.contains(&subcommand.as_str()),
        "git '{subcommand}' would touch HEAD, the index, the worktree or a remote"
    );
    assert!(
        ALLOWED_SUBCOMMANDS.contains(&subcommand.as_str()),
        "git '{subcommand}' is not in the change record's allowlist"
    );
    for arg in argv.iter().skip(1) {
        assert!(
            arg != "-f" && arg != "--force" && arg != "-D",
            "a forced or destructive git operation is never taken by the change record"
        );
    }
    argv
}

/// How a plumbing call is run: a private index, an identity, and stdin.
#[derive(Default)]
pub struct GitEnv {
    /// `GIT_INDEX_FILE`. Set for every index-touching call so the
    /// repository's own `.git/index` is never opened.
    pub index_file: Option<PathBuf>,
    /// Author and committer date, RFC 3339 or any format git accepts. Set on
    /// `commit-tree` so a commit is reproducible.
    pub commit_date: Option<String>,
}

/// Run one plumbing invocation. Returns `(exit code, stdout bytes)`, or `None`
/// when git could not be executed at all — which every caller turns into a
/// refusal, never a pass.
pub fn run_plumbing(
    dir: &Path,
    plumbing: &Plumbing<'_>,
    env: &GitEnv,
    stdin: Option<&[u8]>,
) -> Option<(i32, Vec<u8>)> {
    let argv = argv_of(plumbing);
    let mut command = Command::new("git");
    command.arg("-C").arg(dir).args(&argv);
    if let Some(index_file) = &env.index_file {
        command.env("GIT_INDEX_FILE", index_file);
    }
    if matches!(plumbing, Plumbing::CommitTree { .. }) {
        command.env("GIT_AUTHOR_NAME", ENGINE_IDENTITY_NAME);
        command.env("GIT_AUTHOR_EMAIL", ENGINE_IDENTITY_EMAIL);
        command.env("GIT_COMMITTER_NAME", ENGINE_IDENTITY_NAME);
        command.env("GIT_COMMITTER_EMAIL", ENGINE_IDENTITY_EMAIL);
        if let Some(date) = &env.commit_date {
            command.env("GIT_AUTHOR_DATE", date);
            command.env("GIT_COMMITTER_DATE", date);
        }
    }
    let output = match stdin {
        None => command.output().ok()?,
        Some(bytes) => {
            command.stdin(Stdio::piped());
            command.stdout(Stdio::piped());
            command.stderr(Stdio::piped());
            let mut child = command.spawn().ok()?;
            child.stdin.take()?.write_all(bytes).ok()?;
            child.wait_with_output().ok()?
        }
    };
    Some((output.status.code().unwrap_or(-1), output.stdout))
}

/// [`run_plumbing`] with stdout trimmed to a `String` — for the calls whose
/// answer is an object id or a path, never file content.
pub fn run_plumbing_text(
    dir: &Path,
    plumbing: &Plumbing<'_>,
    env: &GitEnv,
    stdin: Option<&[u8]>,
) -> Option<(i32, String)> {
    run_plumbing(dir, plumbing, env, stdin).map(|(code, out)| {
        (
            code,
            String::from_utf8_lossy(&out).trim_end_matches('\n').to_string(),
        )
    })
}

/// Run git in `dir`, returning `(exit code, stdout)`. `None` when git could not
/// be executed at all — which the caller turns into a REFUSAL, never a pass.
pub(crate) fn run_git(dir: &Path, args: &[&str]) -> Option<(i32, String)> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    Some((
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).trim().to_string(),
    ))
}

pub(crate) fn git_toplevel(dir: &Path) -> Option<PathBuf> {
    match run_git(dir, &["rev-parse", "--show-toplevel"]) {
        Some((0, out)) if !out.is_empty() => Some(PathBuf::from(out)),
        _ => None,
    }
}

pub(crate) fn rev_parse(dir: &Path, rev: &str) -> Option<String> {
    match run_git(dir, &["rev-parse", "--verify", "--quiet", rev]) {
        Some((0, out)) if !out.is_empty() => Some(out),
        _ => None,
    }
}
