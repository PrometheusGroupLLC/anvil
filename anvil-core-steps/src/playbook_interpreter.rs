//! Step module for `playbook_interpreter_outgoing.feature`.
//!
//! Provides steps for:
//! - Building a fixture `PlaybookMachine` from a data table of transitions
//! - Calling `outgoing_transitions(machine, from_state)`
//! - Asserting on result count, to_state, and required_role

use anvil_core::domain::playbook::interpreter::{outgoing_transitions, OutgoingTransition};
use anvil_core::domain::playbook::types::{
    PlaybookMachine, RouteConfig, StateDefinition, TransitionDefinition,
};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

/// Context key for the fixture PlaybookMachine.
const FIXTURE_MACHINE_KEY: &str = "wi_fixture_machine";
/// Context key for the outgoing_transitions result.
const RESULT_KEY: &str = "wi_result";

/// Build a minimal `PlaybookMachine` carrying the given transitions.
/// States are inferred from the from_state/to_state values in the table.
fn build_fixture_machine(transitions: Vec<TransitionDefinition>) -> PlaybookMachine {
    // Collect all unique state names mentioned across from/to fields.
    let mut state_names: Vec<String> = Vec::new();
    for t in &transitions {
        if !state_names.contains(&t.from_state) {
            state_names.push(t.from_state.clone());
        }
        if !state_names.contains(&t.to_state) {
            state_names.push(t.to_state.clone());
        }
    }

    let states: Vec<StateDefinition> = state_names
        .into_iter()
        .map(|name| StateDefinition {
            name,
            role_filters: vec![],
            registry_section: String::new(),
            projection_targets: vec![],
            is_review_gate: false,
            is_terminal: false,
            hook: None,
            hooks_by_role: std::collections::BTreeMap::new(),
            measurement_by_role: std::collections::BTreeMap::new(),
        })
        .collect();

    // Collect unique roles for the `roles` list.
    let mut roles: Vec<String> = Vec::new();
    for t in &transitions {
        if !roles.contains(&t.required_role) {
            roles.push(t.required_role.clone());
        }
    }

    PlaybookMachine {
        kind: "fixture".to_string(),
        directory: "fixtures".to_string(),
        registry: "fixtures.md".to_string(),
        parent_kind: None,
        description: "Fixture machine for interpreter tests".to_string(),
        route: RouteConfig::default(),
        required_fields: vec![],
        roles,
        states,
        transitions,
        register: anvil_core::domain::playbook::types::Register::Driven,
        ..Default::default()
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ===== Given: build fixture machine from data table =====
        step_def(
            "a fixture playbook machine with transitions:",
            &[],
            &[(FIXTURE_MACHINE_KEY, "PlaybookMachine")],
            |_ctx, params| {
                // Parse data table: columns are from_state | to_state | required_role
                let table = params.data_table().ok_or("Expected a data table")?;
                let headers = &table.headers;
                let from_idx = headers
                    .iter()
                    .position(|h| h == "from_state")
                    .ok_or("Missing column 'from_state'")?;
                let to_idx = headers
                    .iter()
                    .position(|h| h == "to_state")
                    .ok_or("Missing column 'to_state'")?;
                let role_idx = headers
                    .iter()
                    .position(|h| h == "required_role")
                    .ok_or("Missing column 'required_role'")?;

                let mut transitions = Vec::new();
                for row in &table.rows {
                    let from_state = row
                        .get(from_idx)
                        .ok_or("Row too short for 'from_state'")?
                        .to_string();
                    let to_state = row
                        .get(to_idx)
                        .ok_or("Row too short for 'to_state'")?
                        .to_string();
                    let required_role = row
                        .get(role_idx)
                        .ok_or("Row too short for 'required_role'")?
                        .to_string();
                    transitions.push(TransitionDefinition {
                        from_state,
                        to_state,
                        required_role,
                        required_satisfaction: None,
                        requires_approver: false,
                        hook: None,
                    });
                }
                let machine = build_fixture_machine(transitions);
                let mut out = Context::new();
                out.set(FIXTURE_MACHINE_KEY, machine);
                Ok(out)
            },
        ),
        // ===== When: call outgoing_transitions =====
        step_def(
            "outgoing_transitions is called for state {string}",
            &[(FIXTURE_MACHINE_KEY, "PlaybookMachine")],
            &[(RESULT_KEY, "Vec<OutgoingTransition>")],
            |ctx, params| {
                let from_state = params
                    .get_string(0)
                    .ok_or("Expected from_state")?
                    .to_string();
                let machine = ctx
                    .get::<PlaybookMachine>(FIXTURE_MACHINE_KEY)
                    .ok_or("No fixture machine")?;
                let result = outgoing_transitions(machine, &from_state);
                let mut out = Context::new();
                out.set(RESULT_KEY, result);
                Ok(out)
            },
        ),
        // ===== Then: result count =====
        check_def(
            "the result contains {int} transitions",
            &[(RESULT_KEY, "Vec<OutgoingTransition>")],
            |ctx, params| {
                let expected_count: usize = params
                    .get_int(0)
                    .ok_or("Expected count")?
                    .try_into()
                    .map_err(|_| "Negative count".to_string())?;
                let result = ctx
                    .get::<Vec<OutgoingTransition>>(RESULT_KEY)
                    .ok_or("No result")?;
                if result.len() != expected_count {
                    Err(format!(
                        "Expected {} transitions but got {}. Transitions: {:?}",
                        expected_count,
                        result.len(),
                        result
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        // ===== Then: result[i] has to_state and required_role =====
        check_def(
            "result transition {int} has to_state {string} and required_role {string}",
            &[(RESULT_KEY, "Vec<OutgoingTransition>")],
            |ctx, params| {
                let idx: usize = params
                    .get_int(0)
                    .ok_or("Expected index")?
                    .try_into()
                    .map_err(|_| "Negative index".to_string())?;
                let expected_to = params.get_string(1).ok_or("Expected to_state")?;
                let expected_role = params.get_string(2).ok_or("Expected required_role")?;
                let result = ctx
                    .get::<Vec<OutgoingTransition>>(RESULT_KEY)
                    .ok_or("No result")?;
                let t = result
                    .get(idx)
                    .ok_or_else(|| format!("No transition at index {}", idx))?;
                let mut errs = Vec::new();
                if t.to_state != expected_to {
                    errs.push(format!(
                        "to_state: expected '{}' got '{}'",
                        expected_to, t.to_state
                    ));
                }
                if t.required_role != expected_role {
                    errs.push(format!(
                        "required_role: expected '{}' got '{}'",
                        expected_role, t.required_role
                    ));
                }
                if errs.is_empty() {
                    Ok(())
                } else {
                    Err(errs.join("; "))
                }
            },
        ),
    ]
}
