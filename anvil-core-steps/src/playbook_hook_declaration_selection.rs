//! Step module for playbook_hook_declaration_selection.feature.

use anvil_core::domain::playbook::interpreter::{
    state_hook, state_role_hook, PlaybookHookSelection,
};
use anvil_core::domain::playbook::types::{PlaybookMachine, RouteConfig, StateDefinition};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::collections::BTreeMap;

const FIXTURE_MACHINE_KEY: &str = "whds_fixture_machine";
const SELECTED_HOOK_KEY: &str = "whds_selected_hook";

fn fixture_machine_with_state_hooks(states: Vec<StateDefinition>) -> PlaybookMachine {
    PlaybookMachine {
        kind: "track".to_string(),
        directory: "tracks".to_string(),
        registry: "tracks.md".to_string(),
        parent_kind: None,
        description: "Hook selection fixture".to_string(),
        route: RouteConfig::default(),
        required_fields: vec![],
        roles: vec!["doer".to_string()],
        states,
        transitions: vec![],
        register: anvil_core::domain::playbook::types::Register::Driven,
        ..Default::default()
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a fixture playbook machine with state hooks:",
            &[],
            &[(FIXTURE_MACHINE_KEY, "PlaybookMachine")],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected a data table")?;
                let state_idx = table
                    .headers
                    .iter()
                    .position(|h| h == "state")
                    .ok_or("Missing column 'state'")?;
                let hook_idx = table
                    .headers
                    .iter()
                    .position(|h| h == "hook")
                    .ok_or("Missing column 'hook'")?;
                // Optional columns for per-role hooks. If absent, hooks_by_role is empty.
                let doer_hook_idx = table.headers.iter().position(|h| h == "doer_hook");
                let reviewer_hook_idx = table.headers.iter().position(|h| h == "reviewer_hook");

                let states = table
                    .rows
                    .iter()
                    .map(|row| {
                        let name = row
                            .get(state_idx)
                            .ok_or("Row too short for 'state'")?
                            .to_string();
                        let hook = row.get(hook_idx).and_then(|h| {
                            let trimmed = h.trim();
                            if trimmed.is_empty() {
                                None
                            } else {
                                Some(trimmed.to_string())
                            }
                        });

                        let mut hooks_by_role: BTreeMap<String, String> = BTreeMap::new();
                        if let Some(idx) = doer_hook_idx {
                            if let Some(val) = row.get(idx) {
                                let trimmed = val.trim();
                                if !trimmed.is_empty() {
                                    hooks_by_role.insert("doer".to_string(), trimmed.to_string());
                                }
                            }
                        }
                        if let Some(idx) = reviewer_hook_idx {
                            if let Some(val) = row.get(idx) {
                                let trimmed = val.trim();
                                if !trimmed.is_empty() {
                                    hooks_by_role
                                        .insert("reviewer".to_string(), trimmed.to_string());
                                }
                            }
                        }

                        Ok(StateDefinition {
                            name,
                            role_filters: vec![],
                            registry_section: String::new(),
                            projection_targets: vec![],
                            is_review_gate: false,
                            is_terminal: false,
                            hook,
                            hooks_by_role,
                            measurement_by_role: std::collections::BTreeMap::new(),
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;

                let mut out = Context::new();
                out.set(
                    FIXTURE_MACHINE_KEY,
                    fixture_machine_with_state_hooks(states),
                );
                Ok(out)
            },
        ),
        step_def(
            "state_hook is called for state {string}",
            &[(FIXTURE_MACHINE_KEY, "PlaybookMachine")],
            &[(SELECTED_HOOK_KEY, "Option<PlaybookHookSelection>")],
            |ctx, params| {
                let state = params.get_string(0).ok_or("Expected state")?;
                let machine = ctx
                    .get::<PlaybookMachine>(FIXTURE_MACHINE_KEY)
                    .ok_or("No fixture machine")?;
                let selected = state_hook(machine, &state);
                let mut out = Context::new();
                out.set(SELECTED_HOOK_KEY, selected);
                Ok(out)
            },
        ),
        step_def(
            "state_role_hook is called for state {string} role {string}",
            &[(FIXTURE_MACHINE_KEY, "PlaybookMachine")],
            &[(SELECTED_HOOK_KEY, "Option<PlaybookHookSelection>")],
            |ctx, params| {
                let state = params.get_string(0).ok_or("Expected state")?;
                let role = params.get_string(1).ok_or("Expected role")?;
                let machine = ctx
                    .get::<PlaybookMachine>(FIXTURE_MACHINE_KEY)
                    .ok_or("No fixture machine")?;
                let selected = state_role_hook(machine, &state, &role);
                let mut out = Context::new();
                out.set(SELECTED_HOOK_KEY, selected);
                Ok(out)
            },
        ),
        check_def(
            "the selected hook has scope {string} state {string} and filename {string}",
            &[(SELECTED_HOOK_KEY, "Option<PlaybookHookSelection>")],
            |ctx, params| {
                let expected_scope = params.get_string(0).ok_or("Expected scope")?;
                let expected_state = params.get_string(1).ok_or("Expected state")?;
                let expected_filename = params.get_string(2).ok_or("Expected filename")?;
                let selected = ctx
                    .get::<Option<PlaybookHookSelection>>(SELECTED_HOOK_KEY)
                    .ok_or("No selected hook key")?
                    .as_ref()
                    .ok_or("Expected selected hook but got None")?;

                let mut errs = Vec::new();
                if selected.hook_scope != expected_scope {
                    errs.push(format!(
                        "scope: expected '{}' got '{}'",
                        expected_scope, selected.hook_scope
                    ));
                }
                if selected.state != expected_state {
                    errs.push(format!(
                        "state: expected '{}' got '{}'",
                        expected_state, selected.state
                    ));
                }
                if selected.filename != expected_filename {
                    errs.push(format!(
                        "filename: expected '{}' got '{}'",
                        expected_filename, selected.filename
                    ));
                }
                if errs.is_empty() {
                    Ok(())
                } else {
                    Err(errs.join("; "))
                }
            },
        ),
        check_def(
            "no hook is selected",
            &[(SELECTED_HOOK_KEY, "Option<PlaybookHookSelection>")],
            |ctx, _params| {
                let selected = ctx
                    .get::<Option<PlaybookHookSelection>>(SELECTED_HOOK_KEY)
                    .ok_or("No selected hook key")?;
                if selected.is_some() {
                    Err(format!("Expected no hook selection but got {:?}", selected))
                } else {
                    Ok(())
                }
            },
        ),
    ]
}
