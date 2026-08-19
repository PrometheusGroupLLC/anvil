//! The production resolver for the completion merge check: real `git`, real
//! repositories, no network.
//!
//! ## Where the expected value comes from, and whether the surface can hold it
//!
//! The ancestry answer comes from `git merge-base --is-ancestor <sha>
//! origin/main` run INSIDE the repository that owns the commit — the same
//! command a human would run, on the same refs. It is not re-derived from a
//! branch tip, an ahead-count, or a version number, all of which can be true
//! while the code is absent.
//!
//! The surface can hold the value only if the resolver is pointed at the RIGHT
//! repository. That is the whole reason `<repo>@<ref>` qualification exists: on
//! the live program the claim record lives in `foundry-business-hearth` and the
//! code lives in `lore`, `kiln`, `temper`, `foundry`. A resolver that silently
//! answered from the hearth's own repository would report "unknown commit" for
//! every honest cross-repo claim and — worse — could report *merged* for a SHA
//! that happens to exist locally. So: unqualified means the hearth's own
//! repository and nothing else, qualified means look it up, and a repository
//! that cannot be located REFUSES.
//!
//! ## No network
//!
//! This never fetches. A stale local `origin/main` can therefore produce a
//! FALSE REFUSAL — never a false pass — and the refusal prints the
//! `origin/main` SHA it compared against so the reader can see the staleness
//! immediately and fetch.

use anvil_core::domain::merge_check::{ClaimResolution, CodeClaim};
// `run_git`, `git_toplevel` and `rev_parse` were declared HERE and were moved to
// `git_plumbing` when the change record needed the same seam. Moved, not copied:
// one git seam for the crate, so a change to how this process shells out to git
// is a change in one file.
use crate::git_plumbing::{git_toplevel, rev_parse, run_git};
use anvil_core::ports::code_evidence_port::CodeEvidencePort;
use std::path::{Component, Path, PathBuf};

/// Environment variable naming the directories that hold code repositories,
/// colon-separated. When set (and non-empty) it REPLACES the default search
/// path rather than extending it, so an operator can state exactly where
/// repositories live.
pub const CODE_REPO_ROOTS_ENV: &str = "ANVIL_CODE_REPO_ROOTS";

/// Git-backed [`CodeEvidencePort`].
pub struct GitCodeEvidenceAdapter {
    hearth: PathBuf,
    /// Directories searched for a `<repo>@` qualifier.
    roots: Vec<PathBuf>,
}

impl GitCodeEvidenceAdapter {
    /// Build the adapter for one hearth.
    ///
    /// `read_env` is injected so the search path is exercisable without
    /// mutating process environment in a test.
    pub fn new(hearth: PathBuf, read_env: impl Fn(&str) -> Option<String>) -> Self {
        let configured: Vec<PathBuf> = read_env(CODE_REPO_ROOTS_ENV)
            .map(|raw| {
                raw.split(':')
                    .map(str::trim)
                    .filter(|entry| !entry.is_empty())
                    .map(PathBuf::from)
                    .collect()
            })
            .unwrap_or_default();
        let roots = if configured.is_empty() {
            // Default: the directory that holds the hearth's own repository.
            // Sibling checkouts are the observed layout (`~/Development/<repo>`
            // beside `~/Development/<repo>-hearth`). This is a DEFAULT, not a
            // fallback: when a repository is not found here the claim refuses
            // and the message names every directory searched.
            git_toplevel(&hearth)
                .as_deref()
                .and_then(Path::parent)
                .map(Path::to_path_buf)
                .into_iter()
                .collect()
        } else {
            configured
        };
        Self { hearth, roots }
    }

    /// Locate the repository a claim refers to. `None` qualifier means the
    /// hearth's own work tree.
    fn locate(&self, repo: Option<&str>) -> Result<PathBuf, ClaimResolution> {
        match repo {
            None => git_toplevel(&self.hearth).ok_or_else(|| {
                ClaimResolution::VerifierUnavailable {
                    detail: format!(
                        "{} is not inside a git work tree, so an unqualified claim has no \
                         repository to resolve against",
                        self.hearth.display()
                    ),
                }
            }),
            Some(name) => {
                let searched: Vec<String> = self
                    .roots
                    .iter()
                    .map(|root| root.join(name).display().to_string())
                    .collect();
                for root in &self.roots {
                    let candidate = root.join(name);
                    if candidate.join(".git").exists() {
                        return git_toplevel(&candidate).ok_or_else(|| {
                            ClaimResolution::VerifierUnavailable {
                                detail: format!(
                                    "{} has a .git entry but git would not resolve its work tree",
                                    candidate.display()
                                ),
                            }
                        });
                    }
                }
                Err(ClaimResolution::RepoUnresolved {
                    repo_name: name.to_string(),
                    searched,
                })
            }
        }
    }

    fn resolve_commit(&self, repo: Option<&str>, rev: &str) -> ClaimResolution {
        let repo_dir = match self.locate(repo) {
            Ok(dir) => dir,
            Err(refusal) => return refusal,
        };
        let repo_label = repo_dir.display().to_string();
        if rev.is_empty() {
            return ClaimResolution::CommitUnknown {
                repo_label,
                rev: String::new(),
            };
        }
        let Some(resolved_sha) = rev_parse(&repo_dir, &format!("{}^{{commit}}", rev)) else {
            return ClaimResolution::CommitUnknown {
                repo_label,
                rev: rev.to_string(),
            };
        };
        let Some(origin_main_sha) = rev_parse(&repo_dir, "origin/main^{commit}") else {
            return ClaimResolution::MissingOriginMain { repo_label };
        };
        match run_git(
            &repo_dir,
            &[
                "merge-base",
                "--is-ancestor",
                &resolved_sha,
                &origin_main_sha,
            ],
        ) {
            Some((0, _)) => ClaimResolution::CommitMerged {
                repo_label,
                resolved_sha,
            },
            Some((_, _)) => ClaimResolution::CommitNotMerged {
                branches: containing_branches(&repo_dir, &resolved_sha),
                repo_label,
                resolved_sha,
                origin_main_sha,
            },
            None => ClaimResolution::VerifierUnavailable {
                detail: format!("could not run git in {}", repo_label),
            },
        }
    }

    fn resolve_path(&self, repo: Option<&str>, path: &str) -> ClaimResolution {
        let repo_dir = match self.locate(repo) {
            Ok(dir) => dir,
            Err(refusal) => return refusal,
        };
        let repo_label = repo_dir.display().to_string();
        let relative = Path::new(path);
        // An absolute path, or one that climbs out with `..`, is not a citation
        // INTO the repository. Admitting it would let `/etc/hosts` satisfy the
        // check — an existence test that anything can pass is not a check.
        let escapes = relative.is_absolute()
            || relative
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_)));
        if escapes {
            return ClaimResolution::PathMissing {
                repo_label,
                searched: vec![format!(
                    "{} (rejected: a citation must be a path INSIDE the repository)",
                    path
                )],
            };
        }
        // Unqualified claims are checked against the hearth first — a track
        // citing `tracks/<id>/spec.md` means the hearth's copy — and then
        // against the repository root, which is the same directory whenever the
        // hearth IS the repository.
        let mut candidates: Vec<PathBuf> = Vec::new();
        if repo.is_none() {
            candidates.push(self.hearth.join(relative));
        }
        let in_repo = repo_dir.join(relative);
        if !candidates.contains(&in_repo) {
            candidates.push(in_repo);
        }
        for candidate in &candidates {
            if candidate.exists() {
                return ClaimResolution::PathPresent {
                    repo_label,
                    resolved: candidate.display().to_string(),
                };
            }
        }
        ClaimResolution::PathMissing {
            repo_label,
            searched: candidates
                .iter()
                .map(|c| c.display().to_string())
                .collect(),
        }
    }
}

impl CodeEvidencePort for GitCodeEvidenceAdapter {
    fn resolve(&self, claim: &CodeClaim) -> ClaimResolution {
        match claim {
            CodeClaim::NotCode => ClaimResolution::NotApplicable,
            CodeClaim::Commit { repo, rev } => self.resolve_commit(repo.as_deref(), rev),
            CodeClaim::Path { repo, path } => self.resolve_path(repo.as_deref(), path),
        }
    }
}

/// Which refs DO contain the commit. Capped, because the answer is for a human
/// reading a refusal, and an unbounded list is noise rather than evidence.
fn containing_branches(dir: &Path, sha: &str) -> Vec<String> {
    let Some((0, out)) = run_git(
        dir,
        &[
            "branch",
            "--all",
            "--contains",
            sha,
            "--format=%(refname:short)",
        ],
    ) else {
        return Vec::new();
    };
    out.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(8)
        .map(ToString::to_string)
        .collect()
}
