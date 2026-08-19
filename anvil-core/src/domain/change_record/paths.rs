//! The ONE declaration of which hearth paths the change record covers.
//!
//! A pure fold: `&Path` in, [`PathClass`] out. No filesystem access, no
//! subprocess — the per-transaction writer, baseline import and the divergence
//! report all call this same function. A second copy anywhere is a review
//! failure: registry placement already lives in two divergent copies in this
//! codebase (`domain/snapshot.rs` hardcoded vs `anvil-engine/src/main.rs`
//! machine-driven), and this declaration exists so there is not a third.
//!
//! ## Exhaustive, with a counted residual
//!
//! Every path under the hearth root resolves to exactly one class. The `match`
//! has **no catch-all that returns [`PathClass::Recorded`]**: an unrecognised
//! path is [`PathClass::Residual`], which the divergence report counts and
//! names. A residual bucket that is empty on today's hearths is not a
//! formality — it is the only thing that will notice the next directory
//! somebody adds to a hearth root.
//!
//! See `anvil-core/schemas/change-record.md` for the consumed contract.

use std::path::{Component, Path};

/// Which of the five declared categories a hearth-relative path falls into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PathClass {
    /// Governance state: recorded in the commit.
    Recorded,
    /// A durable append-only sink. Not recorded; counted as its own
    /// denominator in the divergence report, never silently absent.
    ExcludedSink,
    /// Human notes and research the engine never writes. Not recorded;
    /// counted.
    HumanContent,
    /// Secrets and engine config. Never recorded in any stage, including
    /// baseline import, and counted WITHOUT ever being read.
    NeverRecorded,
    /// Matched no declared category. Counted and reported.
    Residual,
}

/// Registry-root directories whose artifact subtrees are governance state.
///
/// `backlog_items/` is included WITH its `.transactions/` journal: that journal
/// is the backlog port's own three-phase record, part of the governance tree,
/// not this mechanism's bookkeeping.
const RECORDED_ROOT_DIRS: &[&str] = &[
    "tracks",
    "proposals",
    "decisions",
    "initiatives",
    "learnings",
    "milestones",
    "playbooks",
    "backlog_items",
    "playbook_generations",
    "workflow_generations",
    "projections",
];

/// Root-level registry files.
const RECORDED_ROOT_FILES: &[&str] = &[
    "tracks.md",
    "proposals.md",
    "decisions.md",
    "initiatives.md",
    "learnings.md",
    "milestones.md",
    "workflows.md",
];

/// Durable append-only sink file names. Matched by file name at any depth,
/// because `__unattributed__/activity-log.jsonl` is a real nested sink on the
/// live hearths.
const EXCLUDED_SINK_FILES: &[&str] = &[
    "activity-log.jsonl",
    "routing-activity.jsonl",
    "step-measurement.jsonl",
    "transition-measurement.jsonl",
    "review-verdict.jsonl",
    "playbook-measurement.jsonl",
    "delivery-log.jsonl",
    // This mechanism's own sink. Were it recorded, a transaction's commit
    // would contain the row describing that same commit.
    "change-record.jsonl",
];

/// Root directories that hold only durable sinks.
const EXCLUDED_SINK_DIRS: &[&str] = &["abstentions"];

/// Root directories holding human notes and research.
const HUMAN_CONTENT_DIRS: &[&str] = &[
    "research",
    "coordination",
    "context",
    "tools",
    "experiments",
    "__unattributed__",
];

/// Secrets and engine configuration. Never recorded, never read.
const NEVER_RECORDED_FILES: &[&str] = &[
    // The key that makes every actor_hash and conversation_hash in this system
    // non-reversible. Recording it into the same object store as the hashes
    // would defeat the no-raw-identities contract by construction.
    ".telemetry-salt",
    ".hearth",
    "hearth.yaml",
    "engine-flags.env",
];

/// Directories that are never recorded at any depth. `.git/` additionally
/// holds this mechanism's own journals, which are bookkeeping ABOUT a
/// recording and would otherwise become a residual on every hearth that has
/// ever crashed.
const NEVER_RECORDED_DIRS: &[&str] = &[".git"];

/// Classify one hearth-relative path.
///
/// The argument is relative to the hearth root. An absolute path, or one that
/// climbs out with `..`, is not a path under the hearth at all and is
/// [`PathClass::Residual`] rather than silently admitted.
pub fn classify(hearth_relative: &Path) -> PathClass {
    let components: Vec<&str> = match normalized_components(hearth_relative) {
        Some(components) => components,
        None => return PathClass::Residual,
    };
    let Some((first, rest)) = components.split_first() else {
        return PathClass::Residual;
    };
    let file_name = *components.last().expect("split_first proved non-empty");

    if components.iter().any(|c| NEVER_RECORDED_DIRS.contains(c)) {
        return PathClass::NeverRecorded;
    }
    if rest.is_empty() && NEVER_RECORDED_FILES.contains(first) {
        return PathClass::NeverRecorded;
    }
    if EXCLUDED_SINK_FILES.contains(&file_name) || EXCLUDED_SINK_DIRS.contains(first) {
        return PathClass::ExcludedSink;
    }
    if !rest.is_empty() && RECORDED_ROOT_DIRS.contains(first) {
        return PathClass::Recorded;
    }
    if rest.is_empty() && RECORDED_ROOT_FILES.contains(first) {
        return PathClass::Recorded;
    }
    if HUMAN_CONTENT_DIRS.contains(first) {
        return PathClass::HumanContent;
    }
    // A loose root-level document. The live hearth carries eight of these
    // (BUILD-PHILOSOPHY.md and friends); none is engine-written.
    if rest.is_empty() && first.ends_with(".md") {
        return PathClass::HumanContent;
    }
    PathClass::Residual
}

/// Split into plain string components, refusing anything that is not a
/// relative path under the hearth.
fn normalized_components(path: &Path) -> Option<Vec<&str>> {
    let mut out = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => out.push(part.to_str()?),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if out.is_empty() {
        return None;
    }
    Some(out)
}
