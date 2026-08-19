//! Step module for `playbook_next_step_generator.feature` and
//! `playbook_contiguity_validation.feature`.
//!
//! Proves the machine-derived next_step generator is generic (exercised against
//! the registered proposal/decision seeds and a lore_query-style fixture, not
//! track_lifecycle) and that the contiguity validator rejects non-flowing
//! machines while accepting well-formed machines and every compiled-in seed.

use anvil_core::domain::playbook::loader::validate_contiguity;
use anvil_core::domain::playbook::next_step::next_step_for;
use anvil_core::domain::playbook::seeds::{decision_seed, proposal_seed, track_seed};
use anvil_core::domain::playbook::types::{
    PlaybookMachine, RouteConfig, StateDefinition, TransitionDefinition,
};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::collections::BTreeMap;

const MACHINE_KEY: &str = "wns_machine";
const NEXT_STEP_KEY: &str = "wns_next_step";
const CONTIGUITY_RESULT_KEY: &str = "wns_contiguity_result";

fn plain_state(name: &str, is_terminal: bool) -> StateDefinition {
    StateDefinition {
        name: name.to_string(),
        role_filters: vec![],
        registry_section: String::new(),
        projection_targets: vec![],
        is_review_gate: false,
        is_terminal,
        hook: None,
        hooks_by_role: BTreeMap::new(),
        measurement_by_role: BTreeMap::new(),
    }
}

fn plain_transition(from: &str, to: &str) -> TransitionDefinition {
    TransitionDefinition {
        from_state: from.to_string(),
        to_state: to.to_string(),
        required_role: "doer".to_string(),
        required_satisfaction: None,
        requires_approver: false,
        hook: None,
    }
}

fn machine_with(
    states: Vec<StateDefinition>,
    transitions: Vec<TransitionDefinition>,
) -> PlaybookMachine {
    PlaybookMachine {
        kind: "fixture".to_string(),
        directory: "fixtures".to_string(),
        registry: "fixtures.md".to_string(),
        parent_kind: None,
        description: "Fixture machine".to_string(),
        route: RouteConfig::default(),
        required_fields: vec![],
        roles: vec!["doer".to_string(), "reviewer".to_string()],
        states,
        transitions,
        ..Default::default()
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ===== next_step generator: Given machines =====
        step_def(
            "the proposal seed machine",
            &[],
            &[(MACHINE_KEY, "PlaybookMachine")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set(MACHINE_KEY, proposal_seed().clone());
                Ok(out)
            },
        ),
        step_def(
            "the decision seed machine",
            &[],
            &[(MACHINE_KEY, "PlaybookMachine")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set(MACHINE_KEY, decision_seed().clone());
                Ok(out)
            },
        ),
        step_def(
            "a fixture machine {string} with doer flow {string} to {string} and terminal {string}",
            &[],
            &[(MACHINE_KEY, "PlaybookMachine")],
            |_ctx, params| {
                let _kind = params.get_string(0).ok_or("kind")?.to_string();
                let from = params.get_string(1).ok_or("from")?.to_string();
                let to = params.get_string(2).ok_or("to")?.to_string();
                let terminal = params.get_string(3).ok_or("terminal")?.to_string();
                // states: from (doer), to (== terminal here).
                let mut states = vec![plain_state(&from, false)];
                if to != from {
                    states.push(plain_state(&to, to == terminal));
                }
                let transitions = vec![plain_transition(&from, &to)];
                let mut out = Context::new();
                out.set(MACHINE_KEY, machine_with(states, transitions));
                Ok(out)
            },
        ),
        // ===== next_step generator: When =====
        step_def(
            "next_step_for is computed for state {string}",
            &[(MACHINE_KEY, "PlaybookMachine")],
            &[(NEXT_STEP_KEY, "String")],
            |ctx, params| {
                let state = params.get_string(0).ok_or("state")?.to_string();
                let machine = ctx
                    .get::<PlaybookMachine>(MACHINE_KEY)
                    .ok_or("No machine")?;
                let next = next_step_for(machine, &state);
                let mut out = Context::new();
                out.set(NEXT_STEP_KEY, next);
                Ok(out)
            },
        ),
        // ===== next_step generator: Then =====
        check_def(
            "the next_step is non-empty",
            &[(NEXT_STEP_KEY, "String")],
            |ctx, _params| {
                let next = ctx.get::<String>(NEXT_STEP_KEY).ok_or("No next_step")?;
                if next.trim().is_empty() {
                    Err("next_step is empty".to_string())
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the next_step contains {string}",
            &[(NEXT_STEP_KEY, "String")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("needle")?;
                let next = ctx.get::<String>(NEXT_STEP_KEY).ok_or("No next_step")?;
                if next.contains(needle) {
                    Ok(())
                } else {
                    Err(format!("next_step does not contain '{}': {}", needle, next))
                }
            },
        ),
        check_def(
            "the next_step does not contain {string}",
            &[(NEXT_STEP_KEY, "String")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("needle")?;
                let next = ctx.get::<String>(NEXT_STEP_KEY).ok_or("No next_step")?;
                if next.contains(needle) {
                    Err(format!(
                        "next_step unexpectedly contains '{}': {}",
                        needle, next
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        // ===== contiguity: Given fixtures =====
        step_def(
            "a contiguity fixture machine with states:",
            &[],
            &[(MACHINE_KEY, "PlaybookMachine")],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected states table")?;
                let name_idx = table
                    .headers
                    .iter()
                    .position(|h| h == "name")
                    .ok_or("Missing 'name' column")?;
                let term_idx = table
                    .headers
                    .iter()
                    .position(|h| h == "is_terminal")
                    .ok_or("Missing 'is_terminal' column")?;
                let states: Vec<StateDefinition> = table
                    .rows
                    .iter()
                    .map(|row| {
                        let name = row.get(name_idx).cloned().unwrap_or_default();
                        let is_terminal = row.get(term_idx).map(|v| v == "true").unwrap_or(false);
                        plain_state(&name, is_terminal)
                    })
                    .collect();
                let mut out = Context::new();
                out.set(MACHINE_KEY, machine_with(states, vec![]));
                Ok(out)
            },
        ),
        step_def(
            "contiguity fixture transitions:",
            &[(MACHINE_KEY, "PlaybookMachine")],
            &[(MACHINE_KEY, "PlaybookMachine")],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected transitions table")?;
                let from_idx = table
                    .headers
                    .iter()
                    .position(|h| h == "from_state")
                    .ok_or("Missing 'from_state' column")?;
                let to_idx = table
                    .headers
                    .iter()
                    .position(|h| h == "to_state")
                    .ok_or("Missing 'to_state' column")?;
                let transitions: Vec<TransitionDefinition> = table
                    .rows
                    .iter()
                    .map(|row| {
                        let from = row.get(from_idx).cloned().unwrap_or_default();
                        let to = row.get(to_idx).cloned().unwrap_or_default();
                        plain_transition(&from, &to)
                    })
                    .collect();
                let mut machine = ctx
                    .get::<PlaybookMachine>(MACHINE_KEY)
                    .ok_or("No machine")?
                    .clone();
                machine.transitions = transitions;
                let mut out = Context::new();
                out.set(MACHINE_KEY, machine);
                Ok(out)
            },
        ),
        step_def(
            "the contiguity track seed",
            &[],
            &[(MACHINE_KEY, "PlaybookMachine")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set(MACHINE_KEY, track_seed().clone());
                Ok(out)
            },
        ),
        step_def(
            "the contiguity proposal seed",
            &[],
            &[(MACHINE_KEY, "PlaybookMachine")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set(MACHINE_KEY, proposal_seed().clone());
                Ok(out)
            },
        ),
        // ===== contiguity: When =====
        step_def(
            "validate_contiguity is called on the fixture",
            &[(MACHINE_KEY, "PlaybookMachine")],
            &[(CONTIGUITY_RESULT_KEY, "String")],
            |ctx, _params| run_contiguity(&ctx),
        ),
        step_def(
            "validate_contiguity is called on the seed",
            &[(MACHINE_KEY, "PlaybookMachine")],
            &[(CONTIGUITY_RESULT_KEY, "String")],
            |ctx, _params| run_contiguity(&ctx),
        ),
        // ===== contiguity: Then =====
        check_def(
            "the contiguity result is Ok",
            &[(CONTIGUITY_RESULT_KEY, "String")],
            |ctx, _params| {
                let result = ctx
                    .get::<String>(CONTIGUITY_RESULT_KEY)
                    .ok_or("No contiguity result")?;
                if result == "Ok" {
                    Ok(())
                } else {
                    Err(format!("Expected Ok, got {}", result))
                }
            },
        ),
        check_def(
            "the contiguity result is an error with code {string}",
            &[(CONTIGUITY_RESULT_KEY, "String")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("code")?;
                let result = ctx
                    .get::<String>(CONTIGUITY_RESULT_KEY)
                    .ok_or("No contiguity result")?;
                if result.contains(expected) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected error code '{}', got {}",
                        expected, result
                    ))
                }
            },
        ),
    ]
}

fn run_contiguity(ctx: &Context) -> Result<Context, String> {
    let machine = ctx
        .get::<PlaybookMachine>(MACHINE_KEY)
        .ok_or("No machine")?;
    let result = validate_contiguity(machine, &machine.kind);
    let mut out = Context::new();
    match result {
        Ok(()) => out.set(CONTIGUITY_RESULT_KEY, "Ok".to_string()),
        Err(e) => out.set(CONTIGUITY_RESULT_KEY, format!("Err: {}", e.code())),
    }
    Ok(out)
}
