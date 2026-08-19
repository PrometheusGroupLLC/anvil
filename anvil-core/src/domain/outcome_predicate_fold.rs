//! Outcome-predicate fold: per-kind evaluation of whether begun instances
//! reached their machine's DECLARED `outcome_predicate.terminal_state`.
//!
//! Decision `playbook_success_rubric_model` Amendment 1 item 6 declares an
//! `outcome_predicate` on every driven `PlaybookMachine` — a checkable FACT
//! ("did the world-change this playbook exists to produce actually happen")
//! distinct from the `success_rubric`'s HOW WELL. Until this fold, nothing
//! evaluated it: `survivor_outcome::fold_survivor_outcome`'s `terminal` count
//! keys off the REGISTRY's generic `state_is_terminal` predicate (any state
//! flagged `is_terminal: true`), NOT the machine's specifically DECLARED
//! predicate state. A machine can carry several terminal states (e.g. a
//! generated playbook's `completed`, or a hand-authored machine's
//! `abandoned`/`superseded`) while its `outcome_predicate` names exactly ONE
//! of them as the checkable fact. This fold closes that gap by resolving each
//! instance's kind to its machine's declared predicate and checking reach
//! against THAT state specifically.
//!
//! Scope: only the `terminal_state` half of the predicate is evaluated here —
//! deterministically, from activity-log `to_state`s, exactly like
//! `fold_survivor_outcome`'s terminal detection. The free-text `check` field
//! (e.g. `"documents reconciled"`) is NOT evaluated here: it names a fact a
//! JUDGE evaluates in the outcome loop (item 11), not something a
//! deterministic fold over transition records can check. Faking that
//! evaluation here would be worse than leaving it undone.
//!
//! Pure function over an already-read activity record vector + a registry for
//! per-kind predicate resolution. Empty input folds to an empty result.

use crate::domain::playbook::registry::PlaybookRegistry;
use crate::ports::activity_log_port::ActivityLogRecord;
use std::collections::BTreeMap;

/// Per-kind outcome-predicate evaluation.
///
/// `predicate_declared` is `false` when the kind's machine carries no
/// `outcome_predicate` at all (e.g. every generator-authored machine before
/// the generator was taught to author one) — such kinds are UNGRADEABLE by
/// this fold, NOT failures. `predicate_rate` is `0.0` for them by convention
/// and MUST NOT be read as "0% satisfied" without first checking
/// `predicate_declared`.
#[derive(Debug, Clone, PartialEq)]
pub struct OutcomePredicateOutcome {
    pub kind: String,
    /// Instances of this kind with >= 1 transition (the same denominator
    /// `fold_survivor_outcome` uses).
    pub begun: u64,
    /// Whether the kind's machine declares an `outcome_predicate` at all.
    pub predicate_declared: bool,
    /// Of `begun`, those whose activity reached the declared `terminal_state`.
    /// Always 0 when `predicate_declared` is false.
    pub predicate_satisfied: u64,
    /// `predicate_satisfied / begun`, or `0.0` when `predicate_declared` is
    /// false or `begun` is 0. See the struct doc comment for the caveat on
    /// reading this when `predicate_declared` is false.
    pub predicate_rate: f64,
}

/// The full outcome-predicate fold result, ordered ascending by kind.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OutcomePredicateResult {
    pub outcomes: Vec<OutcomePredicateOutcome>,
}

impl OutcomePredicateResult {
    /// The outcome-predicate row for a kind, if present.
    pub fn outcome(&self, kind: &str) -> Option<&OutcomePredicateOutcome> {
        self.outcomes.iter().find(|o| o.kind == kind)
    }
}

/// Fold the activity log into per-kind outcome-predicate evaluation, keyed off
/// each kind's machine-DECLARED `outcome_predicate.terminal_state` (resolved
/// via `registry.machine_for`) rather than the registry's generic per-state
/// `is_terminal` flag `fold_survivor_outcome` uses.
///
/// `begun` groups activity records by `playbook_run_id` exactly like
/// `fold_survivor_outcome` (records lacking an instance id, or with an empty
/// `to_state`, are skipped as non-transitions). For each begun instance, the
/// kind's machine is resolved via `registry.machine_for`; if it declares an
/// `outcome_predicate`, the instance is `predicate_satisfied` when any of its
/// `to_state`s equals the declared `terminal_state`. If the machine declares
/// no predicate (or the kind does not resolve at all), the instance still
/// counts toward `begun` but the kind's row carries `predicate_declared:
/// false` and contributes nothing to `predicate_satisfied`.
pub fn fold_outcome_predicate(
    activity: &[ActivityLogRecord],
    registry: &dyn PlaybookRegistry,
) -> OutcomePredicateResult {
    // Group transitions by instance, carrying each instance's resolved kind —
    // the same shape `fold_survivor_outcome` uses.
    let mut to_states_by_instance: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut kind_by_instance: BTreeMap<String, String> = BTreeMap::new();
    for record in activity {
        let Some(instance) = record.playbook_run_id.as_ref() else {
            continue;
        };
        if instance.is_empty() {
            continue;
        }
        if !record.artifact_kind.is_empty() {
            kind_by_instance
                .entry(instance.clone())
                .or_insert_with(|| record.artifact_kind.clone());
        }
        if record.to_state.is_empty() {
            continue;
        }
        to_states_by_instance
            .entry(instance.clone())
            .or_default()
            .push(record.to_state.clone());
    }

    let mut begun_by_kind: BTreeMap<String, u64> = BTreeMap::new();
    let mut declared_by_kind: BTreeMap<String, bool> = BTreeMap::new();
    let mut satisfied_by_kind: BTreeMap<String, u64> = BTreeMap::new();

    for (instance, to_states) in &to_states_by_instance {
        if to_states.is_empty() {
            continue;
        }
        let kind = kind_by_instance.get(instance).cloned().unwrap_or_default();
        *begun_by_kind.entry(kind.clone()).or_insert(0) += 1;

        let predicate = registry
            .machine_for(&kind)
            .and_then(|machine| machine.outcome_predicate.as_ref());
        declared_by_kind
            .entry(kind.clone())
            .or_insert_with(|| predicate.is_some());

        if let Some(predicate) = predicate {
            if to_states.iter().any(|state| state == &predicate.terminal_state) {
                *satisfied_by_kind.entry(kind.clone()).or_insert(0) += 1;
            }
        }
    }

    let outcomes = begun_by_kind
        .into_iter()
        .map(|(kind, begun)| {
            let predicate_declared = declared_by_kind.get(&kind).copied().unwrap_or(false);
            let predicate_satisfied = satisfied_by_kind.get(&kind).copied().unwrap_or(0);
            let predicate_rate = if predicate_declared && begun > 0 {
                predicate_satisfied as f64 / begun as f64
            } else {
                0.0
            };
            OutcomePredicateOutcome {
                kind,
                begun,
                predicate_declared,
                predicate_satisfied,
                predicate_rate,
            }
        })
        .collect();

    OutcomePredicateResult { outcomes }
}
