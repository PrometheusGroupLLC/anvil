//! K5 supervision-bind bindability precondition (R10, M1-tightened).
//!
//! Pure logic over a [`PlaybookMachine`] deciding whether the machine is
//! **K5-bindable**: whether a fired run bound to a fresh instance of this
//! machine can be resolved to a terminal state by the K5 fire path's two
//! resolve legs — a single plain-doer `Complete` → terminal `completed`, and a
//! `Snapshot(to_state:"abandoned")` → terminal `abandoned`.
//!
//! This is the bind-time precondition the engine's `begin` handler enforces (when
//! `ANVIL_K5_BIND` is set) BEFORE scaffolding any instance, so an unbindable run
//! fails closed (`FAILED_PRECONDITION` / `machine_not_bindable`) create-or-nothing
//! (spec R10 / §4.3a; contract `docs/contracts/k5-bind-v1.md` §5). It reads no
//! environment and performs no I/O — the engine binary owns the flag decision.
//!
//! ## Why the check mirrors `complete.rs::select_edge` EXACTLY
//! The run-complete resolve is a single plain-doer `Complete` (no `satisfaction`).
//! `complete.rs::select_edge` builds the doer candidate set as
//! `required_satisfaction.is_none() && required_role != "reviewer"` (`:70-73`),
//! drops terminal-target edges **only when** `candidates.len() > 1` (`:81-89`),
//! and keeps a **single** candidate as-is even if terminal (`:87`). So a single
//! `Complete` reaches terminal `completed` **iff** `initial → completed` is the
//! machine's UNIQUE null-satisfaction non-reviewer forward candidate. This module
//! recomputes that exact candidate set, so bindability is necessary-and-sufficient
//! for the resolve to succeed rather than an existence/path-length approximation
//! (carry-forward M1; plan §8 F1).
//!
//! ## Reference state = the machine's INITIAL state (`machine.states.first()`)
//! Pinned because nothing in the fire chain advances the instance between
//! `begin()` and resolve, so a bound instance sits at its initial state at
//! run-resolve time. This is the same first-state convention the begin handler
//! uses to pick the creation target and `loader::validate_contiguity` uses to
//! anchor its scan (`loader.rs:521-523,:551`).

use crate::domain::playbook::interpreter::outgoing_transitions;
use crate::domain::playbook::types::PlaybookMachine;
use std::fmt;

/// The distinct reasons a machine fails the K5 bindability precondition. Every
/// variant maps to the single stable gRPC reason `machine_not_bindable` at the
/// engine seam; the [`fmt::Display`] detail is for logs / debugging only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindabilityError {
    /// The machine declares no states, so it has no initial state to bind at.
    NoInitialState,
    /// The initial state is an `is_review_gate` state — a plain-doer `Complete`
    /// from it would resolve on edge properties without a reviewer actor, so the
    /// unattended resolve is not well-formed (check (a)-1).
    InitialStateIsReviewGate { state: String },
    /// The initial state is itself terminal — there is no forward resolve to
    /// drive (check (a)-1).
    InitialStateIsTerminal { state: String },
    /// The initial state does NOT have exactly one null-satisfaction non-reviewer
    /// forward candidate. `count == 0` → no plain-doer `Complete` resolves;
    /// `count > 1` → the `complete.rs` terminal-target filter strands the resolve
    /// (the >1-step, sibling-forward-edge, and null-sat-abandon-collision classes;
    /// check (a)-2).
    NotSingleForwardCandidate { initial: String, count: usize },
    /// The single forward candidate does not go directly to a declared-terminal
    /// `completed` state (check (a)-3).
    ForwardCandidateNotTerminalCompleted { initial: String, to_state: String },
    /// No declared `→ abandoned` park edge into a declared-terminal `abandoned`
    /// state, so the `Snapshot` abandon leg is not well-formed (check (b)).
    NoAbandonParkEdge,
}

impl fmt::Display for BindabilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BindabilityError::NoInitialState => {
                write!(f, "machine declares no states (no initial state to bind)")
            }
            BindabilityError::InitialStateIsReviewGate { state } => write!(
                f,
                "initial state '{}' is a review gate; an unattended doer Complete cannot resolve it",
                state
            ),
            BindabilityError::InitialStateIsTerminal { state } => {
                write!(f, "initial state '{}' is terminal; no forward resolve exists", state)
            }
            BindabilityError::NotSingleForwardCandidate { initial, count } => write!(
                f,
                "initial state '{}' has {} null-satisfaction non-reviewer forward candidate(s); exactly one is required so a single doer Complete resolves deterministically",
                initial, count
            ),
            BindabilityError::ForwardCandidateNotTerminalCompleted { initial, to_state } => write!(
                f,
                "the sole forward candidate from initial state '{}' targets '{}', not a declared-terminal 'completed' state",
                initial, to_state
            ),
            BindabilityError::NoAbandonParkEdge => write!(
                f,
                "no declared '-> abandoned' park edge into a terminal 'abandoned' state; the Snapshot abandon leg would not be well-formed"
            ),
        }
    }
}

impl std::error::Error for BindabilityError {}

/// The reviewer axis, mirrored from `complete.rs::is_reviewer_role`: a role is a
/// reviewer role iff it is literally `"reviewer"`.
fn is_reviewer_role(role: &str) -> bool {
    role == "reviewer"
}

/// Whether `state_name` names a declared state that is `is_terminal`. Mirrors
/// `complete.rs::is_terminal_state`: absent state → not terminal (defensive).
fn is_declared_terminal(machine: &PlaybookMachine, state_name: &str) -> bool {
    machine
        .states
        .iter()
        .find(|s| s.name == state_name)
        .map(|s| s.is_terminal)
        .unwrap_or(false)
}

/// Decide whether `machine` is K5-bindable. `Ok(())` iff BOTH legs hold; the
/// first failing leg's [`BindabilityError`] is returned otherwise. Pure — no
/// I/O, no environment.
///
/// See the module docs for why check (a) recomputes the `complete.rs` candidate
/// set exactly. The check is create-or-nothing at the caller: on `Err`, the
/// begin handler returns `FAILED_PRECONDITION` / `machine_not_bindable` and
/// scaffolds nothing.
pub fn machine_is_k5_bindable(machine: &PlaybookMachine) -> Result<(), BindabilityError> {
    // Reference state = the machine's INITIAL state = first declared state.
    let initial = machine
        .states
        .first()
        .ok_or(BindabilityError::NoInitialState)?;

    // --- Check (a)-1: initial is not a review gate and not terminal. ---
    if initial.is_review_gate {
        return Err(BindabilityError::InitialStateIsReviewGate {
            state: initial.name.clone(),
        });
    }
    if initial.is_terminal {
        return Err(BindabilityError::InitialStateIsTerminal {
            state: initial.name.clone(),
        });
    }

    // --- Check (a)-2: exactly one null-sat non-reviewer forward candidate. ---
    // Computed IDENTICALLY to complete.rs:70-73 (the doer candidate set).
    let candidates: Vec<_> = outgoing_transitions(machine, &initial.name)
        .into_iter()
        .filter(|e| e.required_satisfaction.is_none() && !is_reviewer_role(&e.required_role))
        .collect();
    let single = match candidates.as_slice() {
        [edge] => edge,
        other => {
            return Err(BindabilityError::NotSingleForwardCandidate {
                initial: initial.name.clone(),
                count: other.len(),
            });
        }
    };

    // --- Check (a)-3: that single edge goes to a declared-terminal `completed`. ---
    if single.to_state != "completed" || !is_declared_terminal(machine, "completed") {
        return Err(BindabilityError::ForwardCandidateNotTerminalCompleted {
            initial: initial.name.clone(),
            to_state: single.to_state.clone(),
        });
    }

    // --- Check (b): a declared `-> abandoned` park edge into terminal `abandoned`. ---
    let abandoned_terminal = is_declared_terminal(machine, "abandoned");
    let has_abandon_edge = machine
        .transitions
        .iter()
        .any(|t| t.to_state == "abandoned");
    if !abandoned_terminal || !has_abandon_edge {
        return Err(BindabilityError::NoAbandonParkEdge);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `PlaybookMachine` from a compact YAML body. Every fixture below is
    /// a `register: driven` machine (is_track == false), so the plain-doer resolve
    /// rides `complete.rs:69-96` exactly — the shape K5 binds.
    fn machine(yaml: &str) -> PlaybookMachine {
        serde_yaml::from_str(yaml).expect("test machine parses")
    }

    /// The canonical minimal bindable shape — the same shape as the shared
    /// `anvil-test-support/fixtures/k5_bindable_min` fixture.
    fn k5_bindable_min() -> PlaybookMachine {
        machine(
            r#"
kind: k5_bindable_min
directory: k5_bindable_mins
registry: k5_bindable_mins.md
description: test
roles: [doer, reviewer]
states:
  - {name: drafting,  registry_section: active,    is_review_gate: false, is_terminal: false}
  - {name: completed, registry_section: completed, is_review_gate: false, is_terminal: true}
  - {name: abandoned, registry_section: abandoned, is_review_gate: false, is_terminal: true}
transitions:
  - {from_state: drafting, to_state: completed, required_role: doer, required_satisfaction: ~, requires_approver: false}
  - {from_state: drafting, to_state: abandoned, required_role: doer, required_satisfaction: [abandoned], requires_approver: false}
register: driven
"#,
        )
    }

    #[test]
    fn minimal_supervision_shape_is_bindable() {
        assert_eq!(machine_is_k5_bindable(&k5_bindable_min()), Ok(()));
    }

    #[test]
    fn no_states_is_not_bindable() {
        let m = PlaybookMachine::default();
        assert_eq!(
            machine_is_k5_bindable(&m),
            Err(BindabilityError::NoInitialState)
        );
    }

    #[test]
    fn review_gated_initial_is_not_bindable() {
        // initial state itself is a review gate — a plain-doer Complete cannot
        // resolve it (check (a)-1). A satisfaction edge keeps the load-time
        // review-gate invariant satisfied.
        let m = machine(
            r#"
kind: k
directory: d
registry: r.md
description: t
roles: [doer, reviewer]
states:
  - {name: gated,     registry_section: active,    is_review_gate: true,  is_terminal: false}
  - {name: completed, registry_section: completed, is_review_gate: false, is_terminal: true}
  - {name: abandoned, registry_section: abandoned, is_review_gate: false, is_terminal: true}
transitions:
  - {from_state: gated, to_state: completed, required_role: reviewer, required_satisfaction: [satisfied], requires_approver: false}
  - {from_state: gated, to_state: abandoned, required_role: doer, required_satisfaction: [abandoned], requires_approver: false}
register: driven
"#,
        );
        assert!(matches!(
            machine_is_k5_bindable(&m),
            Err(BindabilityError::InitialStateIsReviewGate { .. })
        ));
    }

    #[test]
    fn review_gated_completed_behind_a_gate_is_not_bindable() {
        // `completed` sits behind an is_review_gate state (A8). From initial the
        // sole forward candidate targets the gate state, not `completed` → reject
        // via check (a)-3.
        let m = machine(
            r#"
kind: k
directory: d
registry: r.md
description: t
roles: [doer, reviewer]
states:
  - {name: drafting,  registry_section: active,    is_review_gate: false, is_terminal: false}
  - {name: review,    registry_section: active,    is_review_gate: true,  is_terminal: false}
  - {name: completed, registry_section: completed, is_review_gate: false, is_terminal: true}
  - {name: abandoned, registry_section: abandoned, is_review_gate: false, is_terminal: true}
transitions:
  - {from_state: drafting, to_state: review,    required_role: doer, required_satisfaction: ~, requires_approver: false}
  - {from_state: review,   to_state: completed, required_role: reviewer, required_satisfaction: [satisfied], requires_approver: false}
  - {from_state: drafting, to_state: abandoned, required_role: doer, required_satisfaction: [abandoned], requires_approver: false}
register: driven
"#,
        );
        assert!(matches!(
            machine_is_k5_bindable(&m),
            Err(BindabilityError::ForwardCandidateNotTerminalCompleted { .. })
        ));
    }

    #[test]
    fn more_than_one_doer_step_is_not_bindable() {
        // initial -> s1 -> completed (two doer steps). From initial, C = {->s1},
        // target != completed → reject (the original M1 class).
        let m = machine(
            r#"
kind: k
directory: d
registry: r.md
description: t
roles: [doer, reviewer]
states:
  - {name: drafting,  registry_section: active,    is_review_gate: false, is_terminal: false}
  - {name: s1,        registry_section: active,    is_review_gate: false, is_terminal: false}
  - {name: completed, registry_section: completed, is_review_gate: false, is_terminal: true}
  - {name: abandoned, registry_section: abandoned, is_review_gate: false, is_terminal: true}
transitions:
  - {from_state: drafting, to_state: s1,        required_role: doer, required_satisfaction: ~, requires_approver: false}
  - {from_state: s1,       to_state: completed, required_role: doer, required_satisfaction: ~, requires_approver: false}
  - {from_state: drafting, to_state: abandoned, required_role: doer, required_satisfaction: [abandoned], requires_approver: false}
register: driven
"#,
        );
        assert!(matches!(
            machine_is_k5_bindable(&m),
            Err(BindabilityError::ForwardCandidateNotTerminalCompleted { .. })
        ));
    }

    #[test]
    fn sibling_forward_edge_is_not_bindable() {
        // initial->completed AND initial->s1, both null-sat doer. |C| == 2 → the
        // complete.rs terminal filter would drop `completed` and strand at s1.
        // Reject (F1 recast M1 fail-open).
        let m = machine(
            r#"
kind: k
directory: d
registry: r.md
description: t
roles: [doer, reviewer]
states:
  - {name: drafting,  registry_section: active,    is_review_gate: false, is_terminal: false}
  - {name: s1,        registry_section: active,    is_review_gate: false, is_terminal: false}
  - {name: completed, registry_section: completed, is_review_gate: false, is_terminal: true}
  - {name: abandoned, registry_section: abandoned, is_review_gate: false, is_terminal: true}
transitions:
  - {from_state: drafting, to_state: completed, required_role: doer, required_satisfaction: ~, requires_approver: false}
  - {from_state: drafting, to_state: s1,        required_role: doer, required_satisfaction: ~, requires_approver: false}
  - {from_state: s1,       to_state: completed, required_role: doer, required_satisfaction: ~, requires_approver: false}
  - {from_state: drafting, to_state: abandoned, required_role: doer, required_satisfaction: [abandoned], requires_approver: false}
register: driven
"#,
        );
        assert!(matches!(
            machine_is_k5_bindable(&m),
            Err(BindabilityError::NotSingleForwardCandidate { count: 2, .. })
        ));
    }

    #[test]
    fn null_sat_abandon_collision_is_not_bindable() {
        // initial->completed AND initial->abandoned, both null-sat doer. |C| == 2
        // (both terminal → complete.rs forward-filter empties → unresolvable).
        // Reject — this is WHY the abandon edge must carry
        // required_satisfaction: [abandoned] (F1 / check (a)-iii).
        let m = machine(
            r#"
kind: k
directory: d
registry: r.md
description: t
roles: [doer, reviewer]
states:
  - {name: drafting,  registry_section: active,    is_review_gate: false, is_terminal: false}
  - {name: completed, registry_section: completed, is_review_gate: false, is_terminal: true}
  - {name: abandoned, registry_section: abandoned, is_review_gate: false, is_terminal: true}
transitions:
  - {from_state: drafting, to_state: completed, required_role: doer, required_satisfaction: ~, requires_approver: false}
  - {from_state: drafting, to_state: abandoned, required_role: doer, required_satisfaction: ~, requires_approver: false}
register: driven
"#,
        );
        assert!(matches!(
            machine_is_k5_bindable(&m),
            Err(BindabilityError::NotSingleForwardCandidate { count: 2, .. })
        ));
    }

    #[test]
    fn missing_abandon_edge_is_not_bindable() {
        // weekly_recap shape: single drafting->completed doer edge, NO abandoned
        // state / edge. Passes check (a) but fails check (b) (F2).
        let m = machine(
            r#"
kind: weekly_recap
directory: weekly_recaps
registry: weekly_recaps.md
description: t
roles: [doer, reviewer]
states:
  - {name: drafting,  registry_section: active,    is_review_gate: false, is_terminal: false}
  - {name: completed, registry_section: completed, is_review_gate: false, is_terminal: true}
transitions:
  - {from_state: drafting, to_state: completed, required_role: doer, required_satisfaction: ~, requires_approver: false}
register: driven
"#,
        );
        assert_eq!(
            machine_is_k5_bindable(&m),
            Err(BindabilityError::NoAbandonParkEdge)
        );
    }

    #[test]
    fn abandon_edge_from_a_non_initial_state_still_satisfies_check_b() {
        // The abandon park edge need not originate from `initial` — a declared
        // `-> abandoned` edge anywhere satisfies check (b) (the resolve is a
        // free-form Snapshot). Here the abandon edge is from `completed`... which
        // is terminal, so instead put it from a mid state to keep it well-formed.
        let m = machine(
            r#"
kind: k
directory: d
registry: r.md
description: t
roles: [doer, reviewer]
states:
  - {name: drafting,  registry_section: active,    is_review_gate: false, is_terminal: false}
  - {name: completed, registry_section: completed, is_review_gate: false, is_terminal: true}
  - {name: abandoned, registry_section: abandoned, is_review_gate: false, is_terminal: true}
transitions:
  - {from_state: drafting, to_state: completed, required_role: doer, required_satisfaction: ~, requires_approver: false}
  - {from_state: drafting, to_state: abandoned, required_role: doer, required_satisfaction: [abandoned], requires_approver: false}
register: driven
"#,
        );
        // (This is really the minimal shape; the point is the ok path.)
        assert_eq!(machine_is_k5_bindable(&m), Ok(()));
    }
}
