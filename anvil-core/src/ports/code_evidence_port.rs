//! Resolution seam for the completion merge check.
//!
//! The rule ("cited code must be on `origin/main`") is pure and lives in
//! [`crate::domain::merge_check`]. Resolving a rev-spec or a path against a
//! real repository is I/O, so it sits behind this one narrow port. The
//! production adapter shells out to `git`
//! (`anvil_core_hearth::git_code_evidence_adapter`); tests substitute a table.

use crate::domain::merge_check::{ClaimResolution, CodeClaim};

/// Resolve a classified code claim against the world.
///
/// Implementations MUST NOT return a passing resolution when they could not
/// determine the answer. An unlocatable repository, an absent `origin/main`, or
/// a `git` that will not run each has its own refusing variant — silence is the
/// one thing this port may never do.
pub trait CodeEvidencePort {
    fn resolve(&self, claim: &CodeClaim) -> ClaimResolution;
}
