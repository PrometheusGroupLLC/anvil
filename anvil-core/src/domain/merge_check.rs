//! The completion merge check: a claim that cites code must cite code that is
//! ON `origin/main`.
//!
//! ## The defect this closes
//!
//! `completed` is the engine's strongest assertion: this work shipped. Until
//! now nothing tied that assertion to the code. Measured on the live
//! `foundry-business-hearth` re-theme program: `T-RETHEME-LORE` was recorded
//! `completed` against an implementation that exists only on an unmerged
//! branch — lore's `origin/main` carries ZERO of the tokens the track claims to
//! have introduced. Three sibling tracks were in the same position, and a
//! fourth was `completed` against a branch that introduced none of the work at
//! all. Every one of those completions was accepted by an engine that never
//! looked.
//!
//! ## The rule
//!
//! When a lifecycle call that lands an artifact in `completed` presents
//! claimed evidence that CITES CODE, each cited commit must be an ancestor of
//! that repository's `origin/main`, and each cited path must exist. Anything
//! else refuses the transition BEFORE the first write.
//!
//! ## What this module is, and is not
//!
//! This module is PURE: it classifies opaque references and turns resolutions
//! into a refusal message. It runs no git, touches no filesystem. The
//! resolution itself is a port ([`crate::ports::code_evidence_port`]) so the
//! rule is testable without a repository and the adapter is swappable.
//!
//! ## What the classifier EXCLUDES (say it out loud)
//!
//! A reference is only checkable if it *names* something checkable. These are
//! deliberately NOT code claims and pass untouched:
//!
//! - prose / self-description (`"reviewed the diff by hand"`),
//! - a URL (anything containing `://`) — an external citation, not a local ref,
//! - a bare token with no `/` and no alphabetic file extension (`v0.4.3`,
//!   `foundry-app`) — nothing here identifies a file,
//! - a completion presenting NO claims at all: a document-only track cites no
//!   code, and refusing it would punish honesty. Making a claim MANDATORY is a
//!   different lever (the playbook's `evidence_obligation` plus the P4 lane
//!   gate); this module never invents one.
//!
//! And the check only arms on the transition INTO `completed`. A track parked
//! at `impl_revision`, or moving `spec → spec_review`, is not asserting that
//! anything shipped — its code is SUPPOSED to be on a branch — so gating it
//! would refuse honest in-flight work.

use std::fmt;

/// A claimed-evidence reference, classified for the completion merge check.
///
/// The `repo` qualifier exists because the claim and the code live in
/// DIFFERENT repositories: the `T-RETHEME-LORE` record sits in
/// `foundry-business-hearth` while the code it cites is lore's. An unqualified
/// reference therefore means "in this hearth's own repository" and nothing
/// else — a cross-repo claim must say `<repo>@<ref>` or it cannot be resolved,
/// and an unresolvable claim REFUSES rather than passes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeClaim {
    /// `commit:<rev>` or `commit:<repo>@<rev>`.
    Commit {
        repo: Option<String>,
        rev: String,
    },
    /// A repository path: `<path>`, `<path>:<line>`, `file:<path>`, or
    /// `<repo>@<path>`. The `:<line>` suffix is stripped — a citation names a
    /// file, and the line is a courtesy to the reader.
    Path {
        repo: Option<String>,
        path: String,
    },
    /// Not a code citation. Passes.
    NotCode,
}

impl CodeClaim {
    /// Does this claim require resolution? `NotCode` does not.
    pub fn is_code(&self) -> bool {
        !matches!(self, CodeClaim::NotCode)
    }
}

/// Split a leading `<repo>@` qualifier off a reference body.
///
/// The qualifier is only recognised when the `@` precedes any `/` and the
/// prefix looks like a repository directory name. That keeps `@` inside a path
/// (or inside a rev-spec such as `main@{1}`) from being mistaken for a
/// qualifier.
fn split_repo(body: &str) -> (Option<String>, &str) {
    let Some(at) = body.find('@') else {
        return (None, body);
    };
    if let Some(slash) = body.find('/') {
        if slash < at {
            return (None, body);
        }
    }
    let (name, rest) = body.split_at(at);
    let rest = &rest[1..];
    let plausible = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    if plausible && !rest.is_empty() {
        (Some(name.to_string()), rest)
    } else {
        (None, body)
    }
}

/// Strip a trailing `:<line>` (and `:<line>:<column>`) suffix from a path
/// citation. `anvil-engine/src/main.rs:467` cites `anvil-engine/src/main.rs`.
fn strip_line_suffix(path: &str) -> &str {
    let mut out = path;
    for _ in 0..2 {
        let Some(colon) = out.rfind(':') else { break };
        let (head, tail) = out.split_at(colon);
        let digits = &tail[1..];
        if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) && !head.is_empty() {
            out = head;
        } else {
            break;
        }
    }
    out
}

/// Does this look like a filename — a dot followed by a purely ALPHABETIC
/// extension of 1..=5 characters?
///
/// Alphabetic on purpose. `v0.4.3` and `0.1.324` have a trailing dot group too,
/// and treating a version string as a missing file would refuse honest
/// completions. Measured against the references already on the live
/// `step-measurement.jsonl`, this admits `.rs`, `.md`, `.tsx`, `.yaml`, `.json`
/// and rejects every version literal present.
fn looks_like_filename(candidate: &str) -> bool {
    let Some(dot) = candidate.rfind('.') else {
        return false;
    };
    let ext = &candidate[dot + 1..];
    !ext.is_empty()
        && ext.len() <= 5
        && ext.chars().all(|c| c.is_ascii_alphabetic())
        && dot > 0
}

/// Classify one opaque claimed-evidence reference.
///
/// Total and pure. Anything not positively recognised as a commit or a path is
/// [`CodeClaim::NotCode`] — the classifier never guesses a claim INTO
/// checkability, because a wrong guess refuses an honest completion.
pub fn classify_reference(reference: &str) -> CodeClaim {
    let trimmed = reference.trim();
    if trimmed.is_empty() {
        return CodeClaim::NotCode;
    }
    if let Some(rest) = trimmed.strip_prefix("commit:") {
        let (repo, rev) = split_repo(rest.trim());
        return CodeClaim::Commit {
            repo,
            rev: rev.trim().to_string(),
        };
    }
    // A URL is an external citation. It names no local object, so there is
    // nothing this check could resolve — and pretending otherwise would be a
    // check that cannot fail.
    if trimmed.contains("://") {
        return CodeClaim::NotCode;
    }
    let body = trimmed.strip_prefix("file:").unwrap_or(trimmed).trim();
    let (repo, body) = split_repo(body);
    let path = strip_line_suffix(body).trim();
    if path.is_empty() {
        return CodeClaim::NotCode;
    }
    if path.contains('/') || looks_like_filename(path) {
        CodeClaim::Path {
            repo,
            path: path.to_string(),
        }
    } else {
        CodeClaim::NotCode
    }
}

/// What the resolver found. Every variant that is not `Merged`/`PathPresent`/
/// `NotApplicable` REFUSES: an unresolvable claim is not a passing claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimResolution {
    /// Not a code claim — nothing to resolve.
    NotApplicable,
    /// The commit exists and is an ancestor of that repo's `origin/main`.
    CommitMerged {
        repo_label: String,
        resolved_sha: String,
    },
    /// The commit exists but `origin/main` does not contain it. `branches`
    /// names the refs that DO, because "where is it then?" is the first
    /// question a refused completion raises.
    CommitNotMerged {
        repo_label: String,
        resolved_sha: String,
        origin_main_sha: String,
        branches: Vec<String>,
    },
    /// No object of that name in that repository.
    CommitUnknown {
        repo_label: String,
        rev: String,
    },
    /// The repository has no `origin/main`, so ancestry has no referent. This
    /// refuses rather than passing: a check with no baseline is not a check.
    MissingOriginMain {
        repo_label: String,
    },
    /// The cited path exists.
    PathPresent {
        repo_label: String,
        resolved: String,
    },
    /// The cited path exists nowhere the resolver looked. `searched` lists
    /// every candidate so the refusal is actionable.
    PathMissing {
        repo_label: String,
        searched: Vec<String>,
    },
    /// A `<repo>@` qualifier named a repository the resolver could not locate.
    RepoUnresolved {
        repo_name: String,
        searched: Vec<String>,
    },
    /// The resolver itself could not run (no git, hearth outside a work tree).
    VerifierUnavailable {
        detail: String,
    },
}

impl ClaimResolution {
    /// Does this resolution refuse the completion?
    pub fn refuses(&self) -> bool {
        !matches!(
            self,
            ClaimResolution::NotApplicable
                | ClaimResolution::CommitMerged { .. }
                | ClaimResolution::PathPresent { .. }
        )
    }

    /// One line naming the SHA, the repository, and — for the unmerged case —
    /// the branch the commit IS on.
    pub fn detail(&self) -> String {
        match self {
            ClaimResolution::NotApplicable => "not a code claim".to_string(),
            ClaimResolution::CommitMerged {
                repo_label,
                resolved_sha,
            } => format!("{} is on {}'s origin/main", resolved_sha, repo_label),
            ClaimResolution::CommitNotMerged {
                repo_label,
                resolved_sha,
                origin_main_sha,
                branches,
            } => {
                let where_it_is = if branches.is_empty() {
                    "no branch in that repository contains it".to_string()
                } else {
                    format!("it is on: {}", branches.join(", "))
                };
                format!(
                    "commit {} is NOT an ancestor of {}'s origin/main (origin/main = {}); {}",
                    resolved_sha, repo_label, origin_main_sha, where_it_is
                )
            }
            ClaimResolution::CommitUnknown { repo_label, rev } => format!(
                "commit '{}' does not exist in {} — qualify it as commit:<repo>@<sha> if it \
                 belongs to another repository",
                rev, repo_label
            ),
            ClaimResolution::MissingOriginMain { repo_label } => format!(
                "{} has no origin/main, so ancestry cannot be established — fetch the remote \
                 before completing",
                repo_label
            ),
            ClaimResolution::PathPresent {
                repo_label,
                resolved,
            } => format!("{} exists in {}", resolved, repo_label),
            ClaimResolution::PathMissing {
                repo_label,
                searched,
            } => format!(
                "path does not exist in {} — looked at: {}",
                repo_label,
                searched.join(", ")
            ),
            ClaimResolution::RepoUnresolved {
                repo_name,
                searched,
            } => format!(
                "repository '{}' could not be located — searched: {}; set ANVIL_CODE_REPO_ROOTS \
                 to the directories that hold it",
                repo_name,
                searched.join(", ")
            ),
            ClaimResolution::VerifierUnavailable { detail } => {
                format!("the merge check could not run: {}", detail)
            }
        }
    }
}

/// One refused claim, kept whole so the message can name the reference the
/// caller actually typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusedClaim {
    pub reference: String,
    pub resolution: ClaimResolution,
}

/// The refusal returned to the caller when at least one code claim fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeCheckRefusal {
    pub refused: Vec<RefusedClaim>,
    /// How many claims were classified as code and therefore resolved. Printed
    /// so a reader can tell a refusal apart from a check that scanned nothing.
    pub code_claims_examined: usize,
    /// How many claims were present in total.
    pub claims_present: usize,
}

/// The stable status token every merge-check refusal carries.
pub const MERGE_CHECK_REFUSED: &str = "claimed_evidence_not_on_origin_main";

impl fmt::Display for MergeCheckRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: this completion cites code that is not on origin/main. \
             {} of {} claim(s) name code; {} refused.",
            MERGE_CHECK_REFUSED,
            self.code_claims_examined,
            self.claims_present,
            self.refused.len()
        )?;
        for item in &self.refused {
            write!(f, " [{} -> {}]", item.reference, item.resolution.detail())?;
        }
        Ok(())
    }
}

/// Apply the rule to a completion's claims and their resolutions.
///
/// `items` pairs each presented reference with what the resolver found for it.
/// Empty input passes: a completion presenting no claims cites no code.
pub fn merge_check_verdict(
    items: &[(String, ClaimResolution)],
) -> Result<(), MergeCheckRefusal> {
    let code_claims_examined = items
        .iter()
        .filter(|(_, resolution)| !matches!(resolution, ClaimResolution::NotApplicable))
        .count();
    let refused: Vec<RefusedClaim> = items
        .iter()
        .filter(|(_, resolution)| resolution.refuses())
        .map(|(reference, resolution)| RefusedClaim {
            reference: reference.clone(),
            resolution: resolution.clone(),
        })
        .collect();
    if refused.is_empty() {
        Ok(())
    } else {
        Err(MergeCheckRefusal {
            refused,
            code_claims_examined,
            claims_present: items.len(),
        })
    }
}

/// The single state whose entry arms the merge check.
///
/// Named here rather than inlined at the call sites so the two lifecycle legs
/// that can reach it (`complete` and the free-form `snapshot`) cannot drift
/// apart — a gate one leg enforces and the other does not is a gate with a
/// door beside it.
pub const COMPLETION_STATE: &str = "completed";

/// Does entering `to_state` arm the merge check?
///
/// `completed` and nothing else. Say what that EXCLUDES: the other machines'
/// terminal states — `abandoned`, `superseded`, decision's `resolved`/
/// `retired`, learning's `established` — are not claims that code shipped, and
/// the universal exits in particular are how work is stood down. Extending the
/// gate to them would refuse the very transition used to admit that something
/// did NOT land. Across the seed playbooks, `completed` is reached by
/// `track_lifecycle` (from `reflection_review` and `amend_review`),
/// `milestone_lifecycle`, and `proposal_lifecycle` — all three of which do
/// assert delivery.
pub fn arms_merge_check(to_state: &str) -> bool {
    to_state == COMPLETION_STATE
}
