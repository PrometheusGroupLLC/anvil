//! Playbook-query interpreter — pure functions over `PlaybookMachine`.
//!
//! Phase 4: minimum surface Phase 5 needs.
//!
//! `outgoing_transitions(machine, from_state)` returns the outgoing transitions
//! from a given state in the order they appear in `machine.transitions`
//! (the loader preserves YAML list order per serde_yaml). Returns an empty
//! vec when the state has no outgoing transitions — including the terminal /
//! "no-exit" state case (R12.3).
//!
//! This module has **no I/O, no global state**. It is a pure function over its
//! `&PlaybookMachine` input (R12.1 purity invariant). The `PlaybookRegistry`
//! port (in `registry.rs`) is used BY consumers to resolve a kind-name to a
//! machine; it is NOT used here. The interpreter stays unaware of the registry.

use crate::domain::playbook::types::{MeasurementSpec, PlaybookMachine, TransitionDefinition};

/// Pure hook selection result for a playbook state/transition declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybookHookSelection {
    pub hook_scope: String,
    pub state: String,
    pub filename: String,
}

/// A single outgoing transition returned by the interpreter.
///
/// Carries the fields the edge-selection consumer (complete) needs:
/// `to_state`, `required_role`, `required_satisfaction`, and `requires_approver`.
/// All are returned verbatim from the underlying `TransitionDefinition`.
/// `required_satisfaction` was added for the unified edge-selector (the keystone
/// engine_drives_any_machine track); it is purely additive — the
/// `playbook_interpreter_outgoing.feature` scenarios assert only `to_state` and
/// `required_role`, so no fixture changes are needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingTransition {
    pub to_state: String,
    pub required_role: String,
    pub required_satisfaction: Option<Vec<String>>,
    pub requires_approver: bool,
}

impl OutgoingTransition {
    fn from_transition(t: &TransitionDefinition) -> Self {
        OutgoingTransition {
            to_state: t.to_state.clone(),
            required_role: t.required_role.clone(),
            required_satisfaction: t.required_satisfaction.clone(),
            requires_approver: t.requires_approver,
        }
    }
}

/// Returns all outgoing transitions from `from_state` in the order they appear
/// in `machine.transitions`.
///
/// Returns an empty `Vec` when:
/// - the state has no outgoing transitions (terminal / "no-exit" state, R12.3)
/// - `from_state` is not declared in `machine.states` (defensive: unknown input)
///
/// The function is a pure scan; it does not validate that `from_state` is a
/// declared state — the defensive empty-vec return covers undeclared inputs.
pub fn outgoing_transitions(
    machine: &PlaybookMachine,
    from_state: &str,
) -> Vec<OutgoingTransition> {
    machine
        .transitions
        .iter()
        .filter(|t| t.from_state == from_state)
        .map(OutgoingTransition::from_transition)
        .collect()
}

/// Return the state-level hook for `state_name`, if the state exists and declares one.
///
/// Pure helper: no filesystem, no registry, no global state, no side effects.
pub fn state_hook(machine: &PlaybookMachine, state_name: &str) -> Option<PlaybookHookSelection> {
    machine
        .states
        .iter()
        .find(|state| state.name == state_name)
        .and_then(|state| {
            state.hook.as_ref().map(|filename| PlaybookHookSelection {
                hook_scope: "state".to_string(),
                state: state.name.clone(),
                filename: filename.clone(),
            })
        })
}

/// Return the step-measurement spec for `(state_name, role)`.
///
/// Returns the `MeasurementSpec` from `state.measurement_by_role[role]` if both
/// the state and the role entry exist; `None` otherwise.
///
/// Pure helper: no filesystem, no registry, no global state, no side effects.
pub fn state_role_measurement<'a>(
    machine: &'a PlaybookMachine,
    state_name: &str,
    role: &str,
) -> Option<&'a MeasurementSpec> {
    machine
        .states
        .iter()
        .find(|state| state.name == state_name)
        .and_then(|state| state.measurement_by_role.get(role))
}

/// Return the role-aware hook for `(state_name, role)`.
///
/// Lookup order:
/// 1. If the state declares `hooks_by_role[role]`, return that with scope `"state_role"`.
/// 2. Otherwise, fall back to the state-level `hook` (scope `"state"`).
/// 3. If neither is declared, return `None`.
///
/// Pure helper: no filesystem, no registry, no global state, no side effects.
pub fn state_role_hook(
    machine: &PlaybookMachine,
    state_name: &str,
    role: &str,
) -> Option<PlaybookHookSelection> {
    machine
        .states
        .iter()
        .find(|state| state.name == state_name)
        .and_then(|state| {
            // Check role-specific hook first.
            if let Some(filename) = state.hooks_by_role.get(role) {
                return Some(PlaybookHookSelection {
                    hook_scope: "state_role".to_string(),
                    state: state.name.clone(),
                    filename: filename.clone(),
                });
            }
            // Fall back to role-agnostic hook.
            state.hook.as_ref().map(|filename| PlaybookHookSelection {
                hook_scope: "state".to_string(),
                state: state.name.clone(),
                filename: filename.clone(),
            })
        })
}
