//! Compiled-in `PlaybookMachine` literal for the `backlog_item` (K8) kind.
//!
//! K8 is a `Free`, parent-less **data-artifact** lifecycle (not a router
//! target). Its state machine is the frozen K8 D2.1 table: candidate is the
//! initial state, `done`/`superseded`/`aged_out` are the three terminals, and
//! the 16 non-genesis edges (#1–#16) expand to exactly **29** seed
//! `TransitionDefinition` rows — one per printed `(from, to, required_role)`
//! tuple. Genesis (#0, `→ candidate`) is the `begin` mint, NOT a seed row, so
//! there is no `from_state: "genesis"` transition here.
//!
//! K8 carries **zero hooks / measurements / projection targets / review gates**
//! and `outcome_predicate = None`: it is a data lifecycle with three terminals,
//! so the compiled seed is the sole source of truth (no on-disk source-tier
//! `machine.yaml`, no served body). The per-edge guards the `PlaybookMachine`
//! model cannot express are added by `domain::backlog_item::validate_transition`
//! and the governed preparation seam, not by these coarse admissibility rows.

use crate::domain::playbook::types::{
    Access, FieldDescriptor, PlaybookMachine, Register, RoleFilter, RouteConfig, StateDefinition,
    TransitionDefinition,
};

/// A hook-less, measurement-less state with its own registry section.
fn state(name: &str, is_terminal: bool) -> StateDefinition {
    StateDefinition {
        name: name.to_string(),
        role_filters: vec![if is_terminal {
            RoleFilter::Terminal
        } else {
            RoleFilter::DoerActionable
        }],
        registry_section: name.to_string(),
        projection_targets: Vec::new(),
        is_review_gate: false,
        is_terminal,
        hook: None,
        hooks_by_role: std::collections::BTreeMap::new(),
        measurement_by_role: std::collections::BTreeMap::new(),
    }
}

/// One `(from, to, role)` admissibility row; guards live in `validate_transition`.
fn edge(from: &str, to: &str, role: &str) -> TransitionDefinition {
    TransitionDefinition {
        from_state: from.to_string(),
        to_state: to.to_string(),
        required_role: role.to_string(),
        required_satisfaction: None,
        requires_approver: false,
        hook: None,
    }
}

pub fn build() -> PlaybookMachine {
    PlaybookMachine {
        kind: "backlog_item".to_string(),
        directory: "backlog_items".to_string(),
        registry: "backlog_items.md".to_string(),
        parent_kind: None,
        parent_required: false,
        description: "K8 backlog item: a ranked, governed unit of prioritized work in an organ queue.".to_string(),
        access: Access::foundation(),
        owner_kit: String::new(),
        visibility: String::new(),
        route: RouteConfig::default(),
        projection_only: false,
        required_fields: vec![FieldDescriptor {
            name: "item".to_string(),
            field_type: "object".to_string(),
            description: "The candidate-required subset of the typed BacklogItem (genesis input).".to_string(),
        }],
        // The exact D2.1 driver-role taxonomy (R9).
        roles: vec![
            "track_driver".to_string(),
            "orchestrator".to_string(),
            "organ_loop".to_string(),
            "intake".to_string(),
            "engine_auto".to_string(),
            "nick_shape".to_string(),
        ],
        // Candidate first (initial state); done/superseded/aged_out terminal.
        states: vec![
            state("candidate", false),
            state("ready", false),
            state("in_flight", false),
            state("done", true),
            state("parked", false),
            state("superseded", true),
            state("aged_out", true),
        ],
        // The 29 printed (from, to, role) tuples across edges #1–#16.
        transitions: vec![
            // #1 candidate -> ready
            edge("candidate", "ready", "organ_loop"),
            edge("candidate", "ready", "orchestrator"),
            // #2 candidate -> parked
            edge("candidate", "parked", "nick_shape"),
            edge("candidate", "parked", "organ_loop"),
            // #3 candidate -> superseded
            edge("candidate", "superseded", "nick_shape"),
            edge("candidate", "superseded", "orchestrator"),
            // #4 candidate -> aged_out
            edge("candidate", "aged_out", "engine_auto"),
            // #5 ready -> in_flight
            edge("ready", "in_flight", "track_driver"),
            // #6 ready -> candidate
            edge("ready", "candidate", "organ_loop"),
            edge("ready", "candidate", "orchestrator"),
            edge("ready", "candidate", "nick_shape"),
            // #7 ready -> parked
            edge("ready", "parked", "nick_shape"),
            edge("ready", "parked", "organ_loop"),
            // #8 ready -> superseded
            edge("ready", "superseded", "nick_shape"),
            edge("ready", "superseded", "orchestrator"),
            // #9 ready -> aged_out
            edge("ready", "aged_out", "engine_auto"),
            // #10 in_flight -> done
            edge("in_flight", "done", "engine_auto"),
            edge("in_flight", "done", "nick_shape"),
            // #11 in_flight -> parked
            edge("in_flight", "parked", "track_driver"),
            edge("in_flight", "parked", "nick_shape"),
            // #12 in_flight -> superseded
            edge("in_flight", "superseded", "nick_shape"),
            edge("in_flight", "superseded", "orchestrator"),
            // #13 parked -> candidate
            edge("parked", "candidate", "engine_auto"),
            edge("parked", "candidate", "nick_shape"),
            // #14 parked -> ready
            edge("parked", "ready", "engine_auto"),
            edge("parked", "ready", "nick_shape"),
            // #15 parked -> superseded
            edge("parked", "superseded", "nick_shape"),
            edge("parked", "superseded", "orchestrator"),
            // #16 parked -> aged_out
            edge("parked", "aged_out", "engine_auto"),
        ],
        register: Register::Free,
        success_rubric: None,
        // K8 is a data-artifact lifecycle with three terminals — no outcome
        // predicate is compiled (the explicit compiled-only source choice).
        outcome_predicate: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::backlog_item::LEGAL_TRANSITION_TUPLE_COUNT;
    use crate::domain::playbook::loader::{validate, validate_contiguity};

    #[test]
    fn seed_has_29_transitions_and_seven_states() {
        let m = build();
        assert_eq!(m.transitions.len(), LEGAL_TRANSITION_TUPLE_COUNT);
        assert_eq!(m.transitions.len(), 29);
        assert_eq!(m.states.len(), 7);
        assert_eq!(m.roles.len(), 6);
    }

    #[test]
    fn seed_is_free_parentless_with_candidate_first() {
        let m = build();
        assert_eq!(m.register, Register::Free);
        assert!(m.parent_kind.is_none());
        assert!(!m.parent_required);
        assert!(m.outcome_predicate.is_none());
        assert_eq!(m.states.first().unwrap().name, "candidate", "candidate must be the initial state");
    }

    #[test]
    fn seed_has_exactly_three_terminals() {
        let m = build();
        let terminals: Vec<&str> = m
            .states
            .iter()
            .filter(|s| s.is_terminal)
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(terminals, vec!["done", "superseded", "aged_out"]);
    }

    #[test]
    fn seed_passes_loader_validation_and_contiguity() {
        let m = build();
        validate(&m, &[]).expect("seed must pass the loader cross-reference + review-gate gate");
        validate_contiguity(&m, "backlog_item")
            .expect("every nonterminal state must progress and reach a terminal");
    }

    #[test]
    fn every_transition_role_is_declared() {
        let m = build();
        for t in &m.transitions {
            assert!(
                m.roles.contains(&t.required_role),
                "transition role `{}` must be in the machine roles",
                t.required_role
            );
            assert!(
                m.states.iter().any(|s| s.name == t.from_state),
                "from_state `{}` must be a declared state",
                t.from_state
            );
            assert!(
                m.states.iter().any(|s| s.name == t.to_state),
                "to_state `{}` must be a declared state",
                t.to_state
            );
        }
    }
}
