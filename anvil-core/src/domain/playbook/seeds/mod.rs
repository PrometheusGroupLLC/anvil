//! Compiled-in `PlaybookMachine` seeds for built-in playbook kinds.
//!
//! Two kinds have compiled-in seeds after Phase 3:
//! - `track`: the existing track lifecycle represented as data (R12.4). This
//!   seed drives Phase 5's `describe::available_actions` consumer migration.
//! - `playbook`: the playbook kind's own lifecycle (R3, R4). This seed is
//!   the source of truth for `describe(playbook)` and `begin(artifact_type:
//!   "playbook", ...)`.
//!
//! Both seeds are `&'static PlaybookMachine` values lazily initialized by
//! `std::sync::OnceLock` (no new deps — MSRV-safe, crate uses only
//! `chrono`, `rand`, `serde`, `serde_yaml`).
//!
//! The public surface is intentionally thin: only `track_seed()` and
//! `playbook_seed()` are public. The seed literals live in the private
//! sub-modules `track` and `playbook`.

mod backlog_item;
mod decision;
mod initiative;
mod learning;
mod milestone;
mod playbook;
mod proposal;
mod spark;
mod track;

use super::types::PlaybookMachine;
use std::sync::OnceLock;

static TRACK_SEED: OnceLock<PlaybookMachine> = OnceLock::new();
static PLAYBOOK_SEED: OnceLock<PlaybookMachine> = OnceLock::new();
static DECISION_SEED: OnceLock<PlaybookMachine> = OnceLock::new();
static INITIATIVE_SEED: OnceLock<PlaybookMachine> = OnceLock::new();
static LEARNING_SEED: OnceLock<PlaybookMachine> = OnceLock::new();
static MILESTONE_SEED: OnceLock<PlaybookMachine> = OnceLock::new();
static PROPOSAL_SEED: OnceLock<PlaybookMachine> = OnceLock::new();
static SPARK_SEED: OnceLock<PlaybookMachine> = OnceLock::new();
static BACKLOG_ITEM_SEED: OnceLock<PlaybookMachine> = OnceLock::new();

// ---- Test-only seed override (feature = "test-support") --------------------
//
// The mutation-propagates scenario in Phase 5 (R12 AC) requires injecting a
// modified seed so `describe::available_actions` returns a shortened list
// WITHOUT any change to describe.rs. The override lives in a thread-local so
// parallel scenario runs do not stomp each other.
//
// Enabled by the `test-support` Cargo feature (activated by `anvil-test-support`
// which depends on this crate with `features = ["test-support"]`). Never
// enabled in production builds.
//
// Usage (step module):
//   set_track_seed_override(Box::leak(Box::new(modified_seed)));
//   ... assert describe output ...
//   clear_track_seed_override();
//
// The `Box::leak` gives a `&'static` reference satisfying the return type of
// `track_seed()`. Leaked memory is negligible (test scenarios only) and
// consistent with the compiled-in seed's own `'static` lifetime.
#[cfg(feature = "test-support")]
use std::cell::Cell;

#[cfg(feature = "test-support")]
thread_local! {
    static TRACK_SEED_OVERRIDE: Cell<Option<&'static PlaybookMachine>> = const { Cell::new(None) };
}

/// Override the track seed for the current test thread.
/// Call `clear_track_seed_override()` at scenario teardown.
/// Only available when the `test-support` Cargo feature is active.
#[cfg(feature = "test-support")]
pub fn set_track_seed_override(seed: &'static PlaybookMachine) {
    TRACK_SEED_OVERRIDE.with(|o| o.set(Some(seed)));
}

/// Clear the track seed override for the current test thread.
/// Only available when the `test-support` Cargo feature is active.
#[cfg(feature = "test-support")]
pub fn clear_track_seed_override() {
    TRACK_SEED_OVERRIDE.with(|o| o.set(None));
}

/// The compiled-in `PlaybookMachine` for the `track` kind.
///
/// This seed mirrors the current `describe::available_actions("track", _)`
/// match (lines 92–107 of describe.rs) on the interpreter-visible axis:
/// `from_state`, `to_state`, `required_role` on every transition.
/// Other state fields are zero-valued; see `track.rs` for the comment bands.
///
/// Phase 5 consumer: `describe::available_actions` for the track kind.
pub fn track_seed() -> &'static PlaybookMachine {
    #[cfg(feature = "test-support")]
    {
        let overridden = TRACK_SEED_OVERRIDE.with(|o| o.get());
        if let Some(seed) = overridden {
            return seed;
        }
    }
    TRACK_SEED.get_or_init(track::build)
}

/// The compiled-in `PlaybookMachine` for the `playbook` kind.
///
/// This is the `playbook` kind's own lifecycle per R3.1 (11 states) and
/// R3.2 (14 transitions). It is the source of truth for `describe(playbook)`,
/// `begin(artifact_type: "playbook", ...)`, and the routing flip in Phase 3.
pub fn playbook_seed() -> &'static PlaybookMachine {
    PLAYBOOK_SEED.get_or_init(playbook::build)
}

/// The compiled-in `PlaybookMachine` for the `decision` kind.
///
/// Decision is a free artifact (`register: free`): it resolves for
/// `begin(artifact_type: "decision")` and `describe`, but is excluded from
/// route candidates.
pub fn decision_seed() -> &'static PlaybookMachine {
    DECISION_SEED.get_or_init(decision::build)
}

/// The compiled-in `PlaybookMachine` for the `initiative` kind.
///
/// Initiative is a free artifact (`register: free`): it resolves for
/// `begin(artifact_type: "initiative")` and `describe`, but is excluded from
/// route candidates.
pub fn initiative_seed() -> &'static PlaybookMachine {
    INITIATIVE_SEED.get_or_init(initiative::build)
}

/// The compiled-in `PlaybookMachine` for the `learning` kind.
///
/// Learning is a free artifact (`register: free`): it resolves for
/// `begin(artifact_type: "learning")` and `describe`, but is excluded from
/// route candidates.
pub fn learning_seed() -> &'static PlaybookMachine {
    LEARNING_SEED.get_or_init(learning::build)
}

/// The compiled-in `PlaybookMachine` for the `milestone` kind.
///
/// Milestone is a free artifact (`register: free`): it resolves for
/// `begin(artifact_type: "milestone")` and `describe`, but is excluded from
/// route candidates.
pub fn milestone_seed() -> &'static PlaybookMachine {
    MILESTONE_SEED.get_or_init(milestone::build)
}

/// The compiled-in `PlaybookMachine` for the `proposal` kind.
///
/// Proposal is a free artifact (`register: free`): it resolves for
/// `begin(artifact_type: "proposal")` and `describe`, but is excluded from
/// route candidates.
pub fn proposal_seed() -> &'static PlaybookMachine {
    PROPOSAL_SEED.get_or_init(proposal::build)
}

/// The compiled-in `PlaybookMachine` for the `spark` kind.
///
/// Spark is a free, projection-only kind: it resolves for
/// `begin(artifact_type: "spark")` and `describe`, but is excluded from
/// route candidates and never scaffolds an artifact directory.
pub fn spark_seed() -> &'static PlaybookMachine {
    SPARK_SEED.get_or_init(spark::build)
}

/// The compiled-in `PlaybookMachine` for the `backlog_item` (K8) kind.
///
/// Backlog item is a free, parent-less data-artifact lifecycle: it is minted by
/// `begin(artifact_type: "backlog_item")` and moved through the frozen D2.1
/// table by the governed transition seam, but is excluded from route candidates
/// and carries no hooks/measurements/source body. Wiring this accessor into the
/// live `machine_for`/`all_machines` registry and the first-class discovery
/// seams is the remaining half of plan Task 3.
pub fn backlog_item_seed() -> &'static PlaybookMachine {
    BACKLOG_ITEM_SEED.get_or_init(backlog_item::build)
}
