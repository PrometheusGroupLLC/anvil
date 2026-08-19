//! PlaybookRunFidelity read-side fold over the universal activity-log sink.
//!
//! Where [`crate::domain::activity_summary`] measures whether work *reaches* and
//! *begins* a playbook (coverage + conversion, Layer 1), this fold measures
//! whether a begun playbook is *driven and closed properly* — the FIDELITY axis
//! (Layer 3). It is purely derived from the durable, redacted activity-log record
//! stream the engine already writes (one record per command turn); no new
//! instrumentation. Every signal comes from the existing per-transition fields:
//! `from_state`, `to_state`, `actor_hash`, `at`, `workflow_kind`, and
//! `playbook_run_id`.
//!
//! The four fidelity signals (spec Layer 3):
//!
//!   - **Completion rate** per kind: of the instances that were begun, the
//!     fraction that reached a TERMINAL state (per the registry's per-kind
//!     terminal predicate — `completed` is non-terminal for the track seed;
//!     `abandoned`/`superseded` are terminal). Reports `begun`, `terminal`, rate.
//!   - **Dangling instances**: begun + >=1 transition but NO terminal transition
//!     — the begun-and-abandoned set (C1, the completion tail dropping). Total +
//!     per-kind breakdown.
//!   - **Revision-cycle depth**: count of transitions INTO a `*_revision` state —
//!     a POSITIVE signal (reviews catching things). Per kind + total.
//!   - **Review authenticity**: for each review-gate exit (a transition FROM a
//!     `*_review` state), paired with its matching enter (the transition INTO
//!     that same `*_review` state for the same instance, immediately prior by
//!     `at`), whether the exiting actor differs from the entering actor
//!     (delegated/authentic) or matches (self-review, the rubber-stamp signature
//!     C2), plus the elapsed seconds in the review state.
//!
//! Pure function over an already-read record vector — no filesystem, no ports.
//! The engine reads the sink via the read port and hands the vector here. An
//! empty input yields a zeroed result (never an error): a fresh hearth has no
//! activity yet.
//!
//! ## Skips (additive / backward-compatible, spec R5)
//!
//! Records lacking a `playbook_run_id` (older records, commands with no
//! instance) are SKIPPED from every per-instance measure — they cannot be
//! attributed to an instance, so they contribute nothing rather than erroring. A
//! "transition" is a record with a non-empty `to_state`; non-transition turns
//! (route/catalog/describe) carry no `to_state` and so never count toward begun.

use crate::domain::activity_summary::LabelCount;
use crate::domain::playbook::registry::PlaybookRegistry;
use crate::domain::route::state_is_terminal;
use crate::ports::activity_log_port::ActivityLogRecord;
use chrono::DateTime;
use std::collections::BTreeMap;

/// Per-kind completion: of the instances of `kind` that were begun, how many
/// reached a terminal transition, and the resulting rate. `completion_rate` is
/// `terminal / begun` (0.0 when `begun` is 0).
#[derive(Debug, Clone, PartialEq)]
pub struct KindCompletion {
    pub kind: String,
    pub begun: u64,
    pub terminal: u64,
    pub completion_rate: f64,
}

/// Per-instance fidelity row derived from the same transition grouping used for
/// aggregate fidelity. `folded_state` is the latest folded transition state,
/// never a cached `status.yaml` field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybookRunInstanceFidelity {
    pub instance_id: String,
    pub kind: String,
    pub folded_state: String,
    pub begun: bool,
    pub transition_count: u64,
    pub reached_terminal: bool,
    pub dangling: bool,
    pub revision_cycles: u64,
}

/// Review-authenticity counters over paired review-gate transitions. A review
/// exit (transition FROM a `*_review` state) is paired with its matching enter
/// (the transition INTO that same `*_review` state for the instance, immediately
/// prior by `at`); `review_exits` counts only PAIRED exits, so it always equals
/// `delegated_exits + self_review_exits`. An exit with no matching enter is
/// skipped (cannot be classified).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReviewAuthenticity {
    pub review_exits: u64,
    /// Paired exits where the entering actor differs from the exiting actor
    /// (author != reviewer) — the delegated/authentic reviews.
    pub delegated_exits: u64,
    /// Paired exits where the entering actor equals the exiting actor (author ==
    /// reviewer) — the self-reviews (the rubber-stamp signature, C2).
    pub self_review_exits: u64,
}

/// The full PlaybookRunFidelity fold result.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaybookRunFidelityResult {
    /// Per-kind completion, ordered ascending by kind.
    pub completion: Vec<KindCompletion>,
    /// Per-instance fidelity rows, ordered ascending by instance id.
    pub instances: Vec<PlaybookRunInstanceFidelity>,
    /// Total count of dangling instances (begun + >=1 transition but NO terminal
    /// transition) across all kinds.
    pub dangling_instances: u64,
    /// Per-kind dangling-instance counts, descending by count then ascending by
    /// kind. Only kinds with >=1 dangling instance appear.
    pub dangling_by_kind: Vec<LabelCount>,
    /// Per-kind revision-cycle counts (transitions whose `to_state` ends in
    /// `_revision`), descending by count then ascending by kind. Only kinds with
    /// >=1 revision cycle appear.
    pub revision_cycles: Vec<LabelCount>,
    /// Total revision cycles across all kinds.
    pub revision_cycles_total: u64,
    /// Review-authenticity counters over paired review-gate transitions.
    pub review: ReviewAuthenticity,
    /// Elapsed-seconds samples between each paired review enter and exit `at`,
    /// in instance-id then chronological order. Only non-negative samples whose
    /// both timestamps parse are collected.
    pub review_elapsed_seconds: Vec<u64>,
}

/// Suffix marking a review-gate state (`spec_review`, `impl_review`, ...).
const REVIEW_SUFFIX: &str = "_review";
/// Suffix marking a revision state (`spec_revision`, `impl_revision`, ...).
use crate::domain::shared_types::is_revision_state;

/// A single per-instance transition: a record with a non-empty `to_state`.
struct Transition {
    from_state: String,
    to_state: String,
    actor_hash: Option<String>,
    at: String,
}

/// Sort a `kind -> count` map into a `Vec<LabelCount>` ordered descending by
/// count, then ascending by kind. Zero-count kinds are dropped.
fn sorted_counts(counts: BTreeMap<String, u64>) -> Vec<LabelCount> {
    let mut out: Vec<LabelCount> = counts
        .into_iter()
        .filter(|(_, count)| *count > 0)
        .map(|(label, count)| LabelCount { label, count })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.label.cmp(&b.label)));
    out
}

/// Fold an activity-log record stream into the playbook-fidelity result.
///
/// Records are grouped by `playbook_run_id` (those lacking it are skipped);
/// each instance's transitions (records with a non-empty `to_state`) are sorted
/// defensively by `at`. The terminal predicate comes from `registry` so it is
/// per-kind correct (never a hardcoded state-name list). An empty input folds to
/// a zeroed result.
pub fn fold_playbook_run_fidelity(
    records: &[ActivityLogRecord],
    registry: &dyn PlaybookRegistry,
) -> PlaybookRunFidelityResult {
    // Group transitions by instance, preserving each instance's resolved kind.
    let mut transitions_by_instance: BTreeMap<String, Vec<Transition>> = BTreeMap::new();
    let mut kind_by_instance: BTreeMap<String, String> = BTreeMap::new();

    for record in records {
        let Some(instance) = record.playbook_run_id.as_ref() else {
            continue;
        };
        if instance.is_empty() {
            continue;
        }
        // The instance's kind is the first non-empty workflow_kind seen for it.
        if !record.artifact_kind.is_empty() {
            kind_by_instance
                .entry(instance.clone())
                .or_insert_with(|| record.artifact_kind.clone());
        }
        // Only transitions (records carrying a destination state) participate.
        if record.to_state.is_empty() {
            continue;
        }
        transitions_by_instance
            .entry(instance.clone())
            .or_default()
            .push(Transition {
                from_state: record.from_state.clone(),
                to_state: record.to_state.clone(),
                actor_hash: record.actor_hash.clone(),
                at: record.at.clone(),
            });
    }

    // Per-kind completion accumulators.
    let mut begun_by_kind: BTreeMap<String, u64> = BTreeMap::new();
    let mut terminal_by_kind: BTreeMap<String, u64> = BTreeMap::new();
    let mut dangling_by_kind: BTreeMap<String, u64> = BTreeMap::new();
    let mut revision_by_kind: BTreeMap<String, u64> = BTreeMap::new();
    let mut dangling_total: u64 = 0;
    let mut revision_total: u64 = 0;
    let mut review = ReviewAuthenticity::default();
    let mut review_elapsed_seconds: Vec<u64> = Vec::new();
    let mut instances: Vec<PlaybookRunInstanceFidelity> = Vec::new();

    for (instance, mut transitions) in transitions_by_instance {
        if transitions.is_empty() {
            continue;
        }
        // Defensive chronological sort — the sink is append-order (already
        // chronological), but pairing review enter/exit relies on it.
        transitions.sort_by(|a, b| a.at.cmp(&b.at));

        let kind = kind_by_instance.get(&instance).cloned().unwrap_or_default();

        // The instance is begun (>=1 transition). Determine whether any
        // transition reached a terminal state for the kind.
        *begun_by_kind.entry(kind.clone()).or_insert(0) += 1;
        let reached_terminal = transitions
            .iter()
            .any(|t| state_is_terminal(registry, &kind, &t.to_state));
        if reached_terminal {
            *terminal_by_kind.entry(kind.clone()).or_insert(0) += 1;
        } else {
            *dangling_by_kind.entry(kind.clone()).or_insert(0) += 1;
            dangling_total += 1;
        }

        // Revision cycles: transitions INTO a `*_revision` state.
        let mut instance_revision_cycles = 0;
        for t in &transitions {
            if is_revision_state(&t.to_state) {
                *revision_by_kind.entry(kind.clone()).or_insert(0) += 1;
                revision_total += 1;
                instance_revision_cycles += 1;
            }
        }

        let folded_state = transitions
            .last()
            .map(|t| t.to_state.clone())
            .unwrap_or_default();
        let transition_count = transitions.len() as u64;
        instances.push(PlaybookRunInstanceFidelity {
            instance_id: instance.clone(),
            kind: kind.clone(),
            folded_state,
            begun: true,
            transition_count,
            reached_terminal,
            dangling: !reached_terminal,
            revision_cycles: instance_revision_cycles,
        });

        // Review authenticity: pair each review exit (FROM a `*_review` state)
        // with the immediately-prior enter (INTO that same `*_review` state).
        for (idx, exit) in transitions.iter().enumerate() {
            if !exit.from_state.ends_with(REVIEW_SUFFIX) {
                continue;
            }
            let review_state = exit.from_state.as_str();
            let Some(enter) = transitions[..idx]
                .iter()
                .rev()
                .find(|t| t.to_state == review_state)
            else {
                continue;
            };
            review.review_exits += 1;
            if enter.actor_hash == exit.actor_hash {
                review.self_review_exits += 1;
            } else {
                review.delegated_exits += 1;
            }
            if let Some(seconds) = elapsed_seconds(&enter.at, &exit.at) {
                review_elapsed_seconds.push(seconds);
            }
        }
    }

    // Build the per-kind completion rows (every begun kind), ascending by kind.
    let completion = begun_by_kind
        .into_iter()
        .map(|(kind, begun)| {
            let terminal = terminal_by_kind.get(&kind).copied().unwrap_or(0);
            let completion_rate = if begun > 0 {
                terminal as f64 / begun as f64
            } else {
                0.0
            };
            KindCompletion {
                kind,
                begun,
                terminal,
                completion_rate,
            }
        })
        .collect();

    PlaybookRunFidelityResult {
        completion,
        instances,
        dangling_instances: dangling_total,
        dangling_by_kind: sorted_counts(dangling_by_kind),
        revision_cycles: sorted_counts(revision_by_kind),
        revision_cycles_total: revision_total,
        review,
        review_elapsed_seconds,
    }
}

/// Non-negative whole seconds between two RFC-3339 timestamps (`enter`→`exit`).
/// Returns `None` when either timestamp fails to parse or the interval is
/// negative (defensive — out-of-order data is dropped rather than counted).
fn elapsed_seconds(enter_at: &str, exit_at: &str) -> Option<u64> {
    let enter = DateTime::parse_from_rfc3339(enter_at).ok()?;
    let exit = DateTime::parse_from_rfc3339(exit_at).ok()?;
    let seconds = (exit - enter).num_seconds();
    if seconds >= 0 {
        Some(seconds as u64)
    } else {
        None
    }
}

/// Cross-hearth merge of multiple per-hearth activity-log streams into a single
/// folded fidelity result. Concatenates every hearth's records, then folds the
/// union with [`fold_playbook_run_fidelity`] — correct ONLY because the salt is
/// per-deployment, so the same actor/instance hashes identically in every hearth.
pub fn fold_playbook_run_fidelity_across_hearths(
    streams: &[Vec<ActivityLogRecord>],
    registry: &dyn PlaybookRegistry,
) -> PlaybookRunFidelityResult {
    let mut all: Vec<ActivityLogRecord> = Vec::new();
    for s in streams {
        all.extend(s.iter().cloned());
    }
    fold_playbook_run_fidelity(&all, registry)
}
