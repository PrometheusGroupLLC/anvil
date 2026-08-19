//! Step module for playbook_measurement_selection.feature.
//!
//! Mirrors playbook_hook_declaration_selection.rs; exercises the
//! state_role_measurement pure selector over a fixture PlaybookMachine.

use anvil_core::domain::playbook::interpreter::state_role_measurement;
use anvil_core::domain::playbook::types::{
    MeasurementSpec, PlaybookMachine, RouteConfig, StateDefinition,
};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::collections::BTreeMap;

const FIXTURE_MACHINE_KEY: &str = "wms_fixture_machine";
const SELECTED_MEASUREMENT_KEY: &str = "wms_selected_measurement";

fn fixture_machine_with_state_measurements(states: Vec<StateDefinition>) -> PlaybookMachine {
    PlaybookMachine {
        kind: "track".to_string(),
        directory: "tracks".to_string(),
        registry: "tracks.md".to_string(),
        parent_kind: None,
        description: "Measurement selection fixture".to_string(),
        route: RouteConfig::default(),
        required_fields: vec![],
        roles: vec!["doer".to_string(), "reviewer".to_string()],
        states,
        transitions: vec![],
        register: anvil_core::domain::playbook::types::Register::Driven,
        ..Default::default()
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a fixture playbook machine with state measurements:",
            &[],
            &[(FIXTURE_MACHINE_KEY, "PlaybookMachine")],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected a data table")?;

                let state_idx = table
                    .headers
                    .iter()
                    .position(|h| h == "state")
                    .ok_or("Missing column 'state'")?;
                let role_idx = table
                    .headers
                    .iter()
                    .position(|h| h == "role")
                    .ok_or("Missing column 'role'")?;
                let intent_idx = table
                    .headers
                    .iter()
                    .position(|h| h == "intent")
                    .ok_or("Missing column 'intent'")?;
                let expected_output_idx = table
                    .headers
                    .iter()
                    .position(|h| h == "expected_output")
                    .ok_or("Missing column 'expected_output'")?;

                // Build a map: state_name -> BTreeMap<role, MeasurementSpec>
                let mut state_measurements: BTreeMap<String, BTreeMap<String, MeasurementSpec>> =
                    BTreeMap::new();
                let mut state_order: Vec<String> = Vec::new();

                for row in &table.rows {
                    let state_name = row
                        .get(state_idx)
                        .ok_or("Row too short for 'state'")?
                        .trim()
                        .to_string();
                    let role = row
                        .get(role_idx)
                        .ok_or("Row too short for 'role'")?
                        .trim()
                        .to_string();
                    let intent = row
                        .get(intent_idx)
                        .ok_or("Row too short for 'intent'")?
                        .trim()
                        .to_string();
                    let expected_output = row
                        .get(expected_output_idx)
                        .ok_or("Row too short for 'expected_output'")?
                        .trim()
                        .to_string();

                    if !state_order.contains(&state_name) {
                        state_order.push(state_name.clone());
                    }
                    state_measurements.entry(state_name).or_default().insert(
                        role,
                        MeasurementSpec {
                            intent,
                            expected_output,
                            success_criteria: None,
                            evidence_obligation: Vec::new(),
                        },
                    );
                }

                // Also add a "plan" state with no measurements, so the "None for plan" scenario works.
                if !state_order.contains(&"plan".to_string()) {
                    state_order.push("plan".to_string());
                }

                let states = state_order
                    .into_iter()
                    .map(|name| {
                        let measurement_by_role =
                            state_measurements.get(&name).cloned().unwrap_or_default();
                        StateDefinition {
                            name,
                            role_filters: vec![],
                            registry_section: String::new(),
                            projection_targets: vec![],
                            is_review_gate: false,
                            is_terminal: false,
                            hook: None,
                            hooks_by_role: BTreeMap::new(),
                            measurement_by_role,
                        }
                    })
                    .collect();

                let mut out = Context::new();
                out.set(
                    FIXTURE_MACHINE_KEY,
                    fixture_machine_with_state_measurements(states),
                );
                Ok(out)
            },
        ),
        step_def(
            "state_role_measurement is called for state {string} role {string}",
            &[(FIXTURE_MACHINE_KEY, "PlaybookMachine")],
            &[(SELECTED_MEASUREMENT_KEY, "Option<MeasurementSpec>")],
            |ctx, params| {
                let state = params.get_string(0).ok_or("Expected state")?;
                let role = params.get_string(1).ok_or("Expected role")?;
                let machine = ctx
                    .get::<PlaybookMachine>(FIXTURE_MACHINE_KEY)
                    .ok_or("No fixture machine")?;
                let selected = state_role_measurement(machine, &state, &role).cloned();
                let mut out = Context::new();
                out.set(SELECTED_MEASUREMENT_KEY, selected);
                Ok(out)
            },
        ),
        check_def(
            "the selected measurement has intent {string}",
            &[(SELECTED_MEASUREMENT_KEY, "Option<MeasurementSpec>")],
            |ctx, params| {
                let expected_intent = params.get_string(0).ok_or("Expected intent")?;
                let selected = ctx
                    .get::<Option<MeasurementSpec>>(SELECTED_MEASUREMENT_KEY)
                    .ok_or("No selected measurement key")?
                    .as_ref()
                    .ok_or("Expected selected measurement but got None")?;
                if selected.intent == expected_intent {
                    Ok(())
                } else {
                    Err(format!(
                        "intent: expected '{}' got '{}'",
                        expected_intent, selected.intent
                    ))
                }
            },
        ),
        check_def(
            "the selected measurement has expected_output {string}",
            &[(SELECTED_MEASUREMENT_KEY, "Option<MeasurementSpec>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected expected_output")?;
                let selected = ctx
                    .get::<Option<MeasurementSpec>>(SELECTED_MEASUREMENT_KEY)
                    .ok_or("No selected measurement key")?
                    .as_ref()
                    .ok_or("Expected selected measurement but got None")?;
                if selected.expected_output == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "expected_output: expected '{}' got '{}'",
                        expected, selected.expected_output
                    ))
                }
            },
        ),
        check_def(
            "no measurement is selected",
            &[(SELECTED_MEASUREMENT_KEY, "Option<MeasurementSpec>")],
            |ctx, _params| {
                let selected = ctx
                    .get::<Option<MeasurementSpec>>(SELECTED_MEASUREMENT_KEY)
                    .ok_or("No selected measurement key")?;
                if selected.is_some() {
                    Err(format!(
                        "Expected no measurement selection but got {:?}",
                        selected
                    ))
                } else {
                    Ok(())
                }
            },
        ),
    ]
}
