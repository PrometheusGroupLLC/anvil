//! Playbook integrity fold — the PURE, SHARED measurement-integrity summary of a
//! single loaded `PlaybookMachine`.
//!
//! This is the ONE implementation of the rubber-stamp / unmeasured / rubric-
//! provenance rules. `hearth_lint`'s review-gate ratchet and the atlas surface
//! both call `playbook_integrity` so the definition never drifts between the
//! lint gate and the app.
//!
//! Honesty constraints (from the 2026-07-06 fleet measurement review):
//!   - `rubber_stamp_gates` adopts the FLEET definition — a review-looking state
//!     (`name.ends_with("_review")`) that is `is_review_gate:false` OR has ANY
//!     outgoing transition with `required_satisfaction:null` (incl. a single-edge
//!     gate) is a rubber stamp. On an all-valid live hearth this reduces to the
//!     narrow `is_review_gate:false` check, because the loader guarantees every
//!     exit of an `is_review_gate:true` state carries a non-null satisfaction.
//!   - `grader_declared` reports whether the rubric NAMES a grader — provenance
//!     ONLY. It is never "registered" or "calibrated" (no grader registry exists;
//!     `grader.is_some()` reporting a registered grader would be exactly the
//!     dishonesty this surface exists to expose).

use crate::domain::playbook::types::PlaybookMachine;

/// The honest measurement-integrity summary of a single loaded playbook machine.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Integrity {
    /// Whether the machine loaded. Always `true` for a machine that reaches this
    /// fold (invalid machines never produce a `PlaybookMachine`); the atlas sets
    /// `false` for its invalid arm separately.
    pub loads: bool,
    /// Review-looking states (`*_review`) that are NOT properly gated — the fleet
    /// rubber-stamp definition. State names only (the caller prefixes the kind).
    pub rubber_stamp_gates: Vec<String>,
    /// Non-terminal states carrying no `measurement_by_role` spec.
    pub unmeasured_states: Vec<String>,
    /// Number of anchor exemplars the success rubric declares (0 when no rubric).
    pub anchors_count: usize,
    /// Whether the success rubric NAMES a grader. Provenance only — never
    /// "registered" or "calibrated".
    pub grader_declared: bool,
}

/// Fold a loaded `PlaybookMachine` into its honest integrity summary.
///
/// Pure: no I/O, no global state. A machine that reaches this fold loaded, so
/// `loads` is `true`.
pub fn playbook_integrity(machine: &PlaybookMachine) -> Integrity {
    Integrity {
        loads: true,
        rubber_stamp_gates: rubber_stamp_gates(machine),
        unmeasured_states: unmeasured_states(machine),
        anchors_count: machine
            .success_rubric
            .as_ref()
            .map(|r| r.anchors.len())
            .unwrap_or(0),
        grader_declared: machine
            .success_rubric
            .as_ref()
            .and_then(|r| r.grader.as_ref())
            .is_some(),
    }
}

/// Review-looking states that are NOT properly gated (fleet definition).
///
/// A `*_review` state is a rubber stamp when `is_review_gate == false` OR some
/// outgoing transition from it carries `required_satisfaction: None` (a
/// verdict the loader cannot enforce). Single-edge gates count.
pub fn rubber_stamp_gates(machine: &PlaybookMachine) -> Vec<String> {
    machine
        .states
        .iter()
        .filter(|state| state.name.ends_with("_review"))
        .filter(|state| {
            !state.is_review_gate
                || machine
                    .transitions
                    .iter()
                    .filter(|t| t.from_state == state.name)
                    .any(|t| t.required_satisfaction.is_none())
        })
        .map(|state| state.name.clone())
        .collect()
}

/// Non-terminal states with an empty `measurement_by_role` map.
///
/// A terminal state legitimately has nothing to measure; a non-terminal state
/// with no measurement spec is an unmeasured step.
pub fn unmeasured_states(machine: &PlaybookMachine) -> Vec<String> {
    machine
        .states
        .iter()
        .filter(|state| !state.is_terminal && state.measurement_by_role.is_empty())
        .map(|state| state.name.clone())
        .collect()
}
