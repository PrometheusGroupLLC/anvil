//! Survivor-corrected outcome fold (Q2 evaluation).
//!
//! Decision `playbook_success_rubric_model` Amendment 1 item 11 (survivorship
//! correction, "critical"): the Q2 denominator is BEGUN instances, not completed
//! ones. Dangling/abandoned runs count as outcome-FAILURES. Judging only
//! terminals would give the router survivor-biased fitness.
//!
//! This fold joins two existing sinks per playbook kind:
//!
//!   - the universal activity log gives **begun** (instances with ≥1 transition)
//!     and **terminal** (instances reaching a machine-declared terminal state,
//!     per the registry's per-kind predicate — the SAME dangling detection
//!     `playbook_run_fidelity` uses);
//!   - the playbook-measurement sink gives **outcome-satisfied** (a begun
//!     instance whose terminal measurement recorded `success = true`).
//!
//! The headline `outcome_rate` is `outcome_satisfied / begun`. Because a dangling
//! instance never emits a terminal measurement, it can never be outcome-satisfied
//! — so it is counted as a failure automatically, exactly as the decision
//! requires. The survivor-biased rate (`satisfied / terminal`) is intentionally
//! NOT the headline; a caller wanting to see the bias can compute it from the
//! exposed counts.
//!
//! Pure function over already-read record vectors + a registry for the terminal
//! predicate. Empty input folds to an empty result.

use crate::domain::playbook::registry::PlaybookRegistry;
use crate::domain::route::state_is_terminal;
use crate::ports::activity_log_port::ActivityLogRecord;
use crate::ports::playbook_measurement_port::PlaybookMeasurementRecord;
use std::collections::{BTreeMap, BTreeSet};

/// Per-kind survivor-corrected outcome counts. `outcome_rate` is
/// `outcome_satisfied / begun` (0.0 when `begun` is 0) — the survivor-CORRECTED
/// rate, where dangling (begun-but-never-terminal) instances are failures.
#[derive(Debug, Clone, PartialEq)]
pub struct SurvivorOutcome {
    pub kind: String,
    /// Instances of this kind with ≥1 transition (the corrected denominator).
    pub begun: u64,
    /// Of `begun`, those reaching a machine-declared terminal state.
    pub terminal: u64,
    /// Of `begun`, those whose terminal measurement recorded `success = true`.
    pub outcome_satisfied: u64,
    /// `outcome_satisfied / begun` — the headline, survivor-corrected rate.
    pub outcome_rate: f64,
}

impl SurvivorOutcome {
    /// Begun instances that did not reach a terminal state (dangling) — counted
    /// as outcome-failures. Convenience over `begun - terminal`.
    pub fn dangling(&self) -> u64 {
        self.begun.saturating_sub(self.terminal)
    }
}

/// The full survivor-corrected outcome fold result, ordered ascending by kind.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SurvivorOutcomeResult {
    pub outcomes: Vec<SurvivorOutcome>,
}

impl SurvivorOutcomeResult {
    /// The outcome row for a kind, if present.
    pub fn outcome(&self, kind: &str) -> Option<&SurvivorOutcome> {
        self.outcomes.iter().find(|o| o.kind == kind)
    }
}

/// Fold the activity log + playbook-measurement streams into per-kind
/// survivor-corrected outcome rates.
///
/// `begun`/`terminal` come from the activity log (records lacking a
/// `playbook_run_id` are skipped; a transition is a record with a non-empty
/// `to_state`); the terminal predicate is per-kind via `registry`.
/// `outcome_satisfied` joins the measurement sink by instance id (a begun
/// instance with a `success = true` measurement).
pub fn fold_survivor_outcome(
    activity: &[ActivityLogRecord],
    measurements: &[PlaybookMeasurementRecord],
    registry: &dyn PlaybookRegistry,
) -> SurvivorOutcomeResult {
    // Group transitions by instance, carrying each instance's resolved kind.
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

    // Instances whose terminal measurement recorded success = true.
    let satisfied_instances: BTreeSet<String> = measurements
        .iter()
        .filter(|m| m.success)
        .filter_map(|m| m.playbook_run_id.clone())
        .filter(|id| !id.is_empty())
        .collect();

    let mut begun_by_kind: BTreeMap<String, u64> = BTreeMap::new();
    let mut terminal_by_kind: BTreeMap<String, u64> = BTreeMap::new();
    let mut satisfied_by_kind: BTreeMap<String, u64> = BTreeMap::new();

    for (instance, to_states) in &to_states_by_instance {
        if to_states.is_empty() {
            continue;
        }
        let kind = kind_by_instance.get(instance).cloned().unwrap_or_default();
        *begun_by_kind.entry(kind.clone()).or_insert(0) += 1;

        let reached_terminal = to_states
            .iter()
            .any(|s| state_is_terminal(registry, &kind, s));
        if reached_terminal {
            *terminal_by_kind.entry(kind.clone()).or_insert(0) += 1;
        }
        // Outcome-satisfied is gated on a success measurement — a dangling
        // instance has none, so it is a failure by construction.
        if satisfied_instances.contains(instance) {
            *satisfied_by_kind.entry(kind.clone()).or_insert(0) += 1;
        }
    }

    let outcomes = begun_by_kind
        .into_iter()
        .map(|(kind, begun)| {
            let terminal = terminal_by_kind.get(&kind).copied().unwrap_or(0);
            let outcome_satisfied = satisfied_by_kind.get(&kind).copied().unwrap_or(0);
            let outcome_rate = if begun > 0 {
                outcome_satisfied as f64 / begun as f64
            } else {
                0.0
            };
            SurvivorOutcome {
                kind,
                begun,
                terminal,
                outcome_satisfied,
                outcome_rate,
            }
        })
        .collect();

    SurvivorOutcomeResult { outcomes }
}
