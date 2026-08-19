//! Step module for candidate_playbook_generate.feature.

use anvil_core::domain::playbook::candidate::{CandidatePlaybook, ProposedState};
use anvil_core::domain::playbook::generate::{
    criterion_is_falsifiable, generate, generate_enforcing, GenerateError,
};
use anvil_core::domain::playbook::loader::validate_with_id;
use anvil_core::domain::playbook::types::{
    AnchorRef, EvidenceClass, OutcomePredicate, PlaybookMachine, Register, RubricDimension,
    SuccessRubric, TransitionDefinition,
};
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

const CANDIDATE_KEY: &str = "cwg_candidate";
const MACHINE_KEY: &str = "cwg_machine";
const ERROR_KEY: &str = "cwg_error";
const LINT_VERDICT_KEY: &str = "cwg_lint_verdict";

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a candidate playbook with intent {string} and proposed states:",
            &[],
            &[(CANDIDATE_KEY, "CandidatePlaybook")],
            |_ctx, params| {
                let intent = params.get_string(0).ok_or("Expected intent")?.to_string();
                let table = params.data_table().ok_or("Expected proposed states table")?;
                let mut out = Context::new();
                out.set(CANDIDATE_KEY, fixture_candidate(intent, parse_proposed_states(table)?));
                Ok(out)
            },
        ),
        step_def(
            "a candidate playbook with intent {string} and no proposed states",
            &[],
            &[(CANDIDATE_KEY, "CandidatePlaybook")],
            |_ctx, params| {
                let intent = params.get_string(0).ok_or("Expected intent")?.to_string();
                let mut out = Context::new();
                out.set(CANDIDATE_KEY, fixture_candidate(intent, vec![]));
                Ok(out)
            },
        ),
        step_def(
            "a candidate playbook with intent {string} route description {string} and route triggers:",
            &[],
            &[(CANDIDATE_KEY, "CandidatePlaybook")],
            |_ctx, params| {
                let intent = params.get_string(0).ok_or("Expected intent")?.to_string();
                let route_description = params
                    .get_string(1)
                    .ok_or("Expected route description")?
                    .to_string();
                let table = params.data_table().ok_or("Expected route triggers table")?;
                let mut candidate = fixture_candidate(intent, vec![]);
                candidate.route_description = route_description;
                candidate.route_triggers = parse_single_column(table, "trigger")?;
                let mut out = Context::new();
                out.set(CANDIDATE_KEY, candidate);
                Ok(out)
            },
        ),
        step_def(
            "candidate projection targets:",
            &[(CANDIDATE_KEY, "CandidatePlaybook")],
            &[(CANDIDATE_KEY, "CandidatePlaybook")],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected projection targets table")?;
                let mut candidate = ctx
                    .get::<CandidatePlaybook>(CANDIDATE_KEY)
                    .ok_or("No candidate playbook")?
                    .clone();
                candidate.projection_targets = parse_single_column(table, "target")?;
                let mut out = Context::new();
                out.set(CANDIDATE_KEY, candidate);
                Ok(out)
            },
        ),
        step_def(
            "candidate proposed states:",
            &[(CANDIDATE_KEY, "CandidatePlaybook")],
            &[(CANDIDATE_KEY, "CandidatePlaybook")],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected proposed states table")?;
                let mut candidate = ctx
                    .get::<CandidatePlaybook>(CANDIDATE_KEY)
                    .ok_or("No candidate playbook")?
                    .clone();
                candidate.proposed_states = parse_proposed_states(table)?;
                let mut out = Context::new();
                out.set(CANDIDATE_KEY, candidate);
                Ok(out)
            },
        ),
        step_def(
            "the candidate carries a success rubric with anchors:",
            &[(CANDIDATE_KEY, "CandidatePlaybook")],
            &[(CANDIDATE_KEY, "CandidatePlaybook")],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected rubric table")?;
                let mut candidate = ctx
                    .get::<CandidatePlaybook>(CANDIDATE_KEY)
                    .ok_or("No candidate playbook")?
                    .clone();
                let dimension_idx = col(table, "dimension")?;
                let weight_idx = col(table, "weight")?;
                let evidence_idx = col(table, "evidence_class")?;
                let anchor_idx = col(table, "anchor_instance")?;
                let band_idx = col(table, "anchor_band")?;
                let mut dimensions = Vec::new();
                let mut anchors = Vec::new();
                for row in &table.rows {
                    dimensions.push(RubricDimension {
                        dimension: cell(row, dimension_idx, "dimension")?.to_string(),
                        weight: cell(row, weight_idx, "weight")?
                            .parse::<u32>()
                            .map_err(|e| format!("invalid weight: {}", e))?,
                        evidence_class: parse_evidence_class(cell(
                            row,
                            evidence_idx,
                            "evidence_class",
                        )?)?,
                    });
                    anchors.push(AnchorRef {
                        instance: cell(row, anchor_idx, "anchor_instance")?.to_string(),
                        band: cell(row, band_idx, "anchor_band")?.to_string(),
                    });
                }
                candidate.success_rubric = Some(SuccessRubric {
                    dimensions,
                    grader: Some("fixture-grader".to_string()),
                    lagging_signals: vec!["fixture_outcome".to_string()],
                    anchors: Vec::new(),
                });
                candidate.anchors = anchors;
                let mut out = Context::new();
                out.set(CANDIDATE_KEY, candidate);
                Ok(out)
            },
        ),
        step_def(
            "the candidate carries an outcome predicate with terminal_state {string}",
            &[(CANDIDATE_KEY, "CandidatePlaybook")],
            &[(CANDIDATE_KEY, "CandidatePlaybook")],
            |ctx, params| {
                let terminal_state = params
                    .get_string(0)
                    .ok_or("Expected terminal_state")?
                    .to_string();
                let mut candidate = ctx
                    .get::<CandidatePlaybook>(CANDIDATE_KEY)
                    .ok_or("No candidate playbook")?
                    .clone();
                candidate.outcome_predicate = Some(OutcomePredicate {
                    terminal_state,
                    check: None,
                });
                let mut out = Context::new();
                out.set(CANDIDATE_KEY, candidate);
                Ok(out)
            },
        ),
        step_def(
            "the candidate carries an outcome predicate with terminal_state {string} and check {string}",
            &[(CANDIDATE_KEY, "CandidatePlaybook")],
            &[(CANDIDATE_KEY, "CandidatePlaybook")],
            |ctx, params| {
                let terminal_state = params
                    .get_string(0)
                    .ok_or("Expected terminal_state")?
                    .to_string();
                let check = params.get_string(1).ok_or("Expected check")?.to_string();
                let mut candidate = ctx
                    .get::<CandidatePlaybook>(CANDIDATE_KEY)
                    .ok_or("No candidate playbook")?
                    .clone();
                candidate.outcome_predicate = Some(OutcomePredicate {
                    terminal_state,
                    check: Some(check),
                });
                let mut out = Context::new();
                out.set(CANDIDATE_KEY, candidate);
                Ok(out)
            },
        ),
        step_def(
            "a candidate playbook with blank intent and proposed states:",
            &[],
            &[(CANDIDATE_KEY, "CandidatePlaybook")],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected proposed states table")?;
                let mut out = Context::new();
                out.set(
                    CANDIDATE_KEY,
                    fixture_candidate("   ".to_string(), parse_proposed_states(table)?),
                );
                Ok(out)
            },
        ),
        step_def(
            "a candidate playbook with blank route description and proposed states:",
            &[],
            &[(CANDIDATE_KEY, "CandidatePlaybook")],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected proposed states table")?;
                let mut candidate = fixture_candidate(
                    "Blank Route Description".to_string(),
                    parse_proposed_states(table)?,
                );
                candidate.route_description = "   ".to_string();
                let mut out = Context::new();
                out.set(CANDIDATE_KEY, candidate);
                Ok(out)
            },
        ),
        step_def(
            "a candidate playbook with empty route triggers and proposed states:",
            &[],
            &[(CANDIDATE_KEY, "CandidatePlaybook")],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected proposed states table")?;
                let mut candidate = fixture_candidate(
                    "Empty Route Triggers".to_string(),
                    parse_proposed_states(table)?,
                );
                candidate.route_triggers = vec![];
                let mut out = Context::new();
                out.set(CANDIDATE_KEY, candidate);
                Ok(out)
            },
        ),
        step_def(
            "a candidate playbook with empty projection targets and proposed states:",
            &[],
            &[(CANDIDATE_KEY, "CandidatePlaybook")],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected proposed states table")?;
                let mut candidate = fixture_candidate(
                    "Empty Projection Targets".to_string(),
                    parse_proposed_states(table)?,
                );
                candidate.projection_targets = vec![];
                let mut out = Context::new();
                out.set(CANDIDATE_KEY, candidate);
                Ok(out)
            },
        ),
        step_def(
            "the candidate playbook is generated",
            &[(CANDIDATE_KEY, "CandidatePlaybook")],
            &[
                (MACHINE_KEY, "Option<PlaybookMachine>"),
                (ERROR_KEY, "Option<GenerateError>"),
            ],
            |ctx, _params| {
                let candidate = ctx
                    .get::<CandidatePlaybook>(CANDIDATE_KEY)
                    .ok_or("No candidate playbook")?;
                let result = generate(candidate);
                let mut out = Context::new();
                match result {
                    Ok(machine) => {
                        out.set(MACHINE_KEY, Some(machine));
                        out.set(ERROR_KEY, None::<GenerateError>);
                    }
                    Err(error) => {
                        out.set(MACHINE_KEY, None::<PlaybookMachine>);
                        out.set(ERROR_KEY, Some(error));
                    }
                }
                Ok(out)
            },
        ),
        step_def(
            "the candidate playbook is generated with measurement enforcement",
            &[(CANDIDATE_KEY, "CandidatePlaybook")],
            &[
                (MACHINE_KEY, "Option<PlaybookMachine>"),
                (ERROR_KEY, "Option<GenerateError>"),
            ],
            |ctx, _params| {
                let candidate = ctx
                    .get::<CandidatePlaybook>(CANDIDATE_KEY)
                    .ok_or("No candidate playbook")?;
                // Deterministic: exercises the enforcing entry point directly,
                // reading no environment. The ANVIL_ENFORCE_MEASUREMENT_DEFINITION
                // dark-gate lives in the engine binary, not in the library or
                // these tests.
                let result = generate_enforcing(candidate);
                let mut out = Context::new();
                match result {
                    Ok(machine) => {
                        out.set(MACHINE_KEY, Some(machine));
                        out.set(ERROR_KEY, None::<GenerateError>);
                    }
                    Err(error) => {
                        out.set(MACHINE_KEY, None::<PlaybookMachine>);
                        out.set(ERROR_KEY, Some(error));
                    }
                }
                Ok(out)
            },
        ),
        check_def(
            "the generated playbook has kind {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected kind")?;
                let machine = generated_machine(&ctx)?;
                assert_eq("kind", expected, &machine.kind)
            },
        ),
        check_def(
            "the generated playbook has directory {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected directory")?;
                let machine = generated_machine(&ctx)?;
                assert_eq("directory", expected, &machine.directory)
            },
        ),
        check_def(
            "the generated playbook has registry {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected registry")?;
                let machine = generated_machine(&ctx)?;
                assert_eq("registry", expected, &machine.registry)
            },
        ),
        check_def(
            "the generated playbook has route description {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected route description")?;
                let machine = generated_machine(&ctx)?;
                let actual = machine
                    .route
                    .description
                    .as_deref()
                    .ok_or("Generated playbook route description was absent")?;
                assert_eq("route.description", expected, actual)
            },
        ),
        check_def(
            "the generated playbook has route triggers:",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected route triggers table")?;
                let expected = parse_single_column(table, "trigger")?;
                let machine = generated_machine(&ctx)?;
                if machine.route.triggers == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "route.triggers: expected {:?} got {:?}",
                        expected, machine.route.triggers
                    ))
                }
            },
        ),
        check_def(
            "the generated playbook directory and registry derive consistently from kind",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, _params| {
                let machine = generated_machine(&ctx)?;
                let expected_directory = format!("{}s", machine.kind);
                let expected_registry = format!("{}.md", expected_directory);
                assert_eq("directory", &expected_directory, &machine.directory)?;
                assert_eq("registry", &expected_registry, &machine.registry)
            },
        ),
        check_def(
            "the generated playbook has roles:",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected roles table")?;
                let role_idx = col(table, "role")?;
                let expected = table
                    .rows
                    .iter()
                    .map(|row| cell(row, role_idx, "role").map(str::to_string))
                    .collect::<Result<Vec<_>, _>>()?;
                let machine = generated_machine(&ctx)?;
                if machine.roles == expected {
                    Ok(())
                } else {
                    Err(format!("roles: expected {:?} got {:?}", expected, machine.roles))
                }
            },
        ),
        check_def(
            "the generated playbook has states:",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected states table")?;
                let state_idx = col(table, "state")?;
                let registry_idx = col(table, "registry_section")?;
                let review_idx = col(table, "is_review_gate")?;
                let terminal_idx = col(table, "is_terminal")?;
                let machine = generated_machine(&ctx)?;

                if machine.states.len() != table.rows.len() {
                    return Err(format!(
                        "state count: expected {} got {}",
                        table.rows.len(),
                        machine.states.len()
                    ));
                }

                for (expected_row, actual) in table.rows.iter().zip(machine.states.iter()) {
                    assert_eq("state", cell(expected_row, state_idx, "state")?, &actual.name)?;
                    assert_eq(
                        "registry_section",
                        cell(expected_row, registry_idx, "registry_section")?,
                        &actual.registry_section,
                    )?;
                    assert_bool(
                        "is_review_gate",
                        cell(expected_row, review_idx, "is_review_gate")?,
                        actual.is_review_gate,
                    )?;
                    assert_bool(
                        "is_terminal",
                        cell(expected_row, terminal_idx, "is_terminal")?,
                        actual.is_terminal,
                    )?;
                    if !actual.role_filters.is_empty()
                        || !actual.hooks_by_role.is_empty()
                        || actual.hook.is_some()
                    {
                        return Err(format!("state '{}' has non-minimal declarations", actual.name));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "every generated playbook state has projection targets:",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected projection targets table")?;
                let expected = parse_single_column(table, "target")?;
                let machine = generated_machine(&ctx)?;
                for state in &machine.states {
                    if state.projection_targets != expected {
                        return Err(format!(
                            "State '{}' projection_targets expected {:?} got {:?}",
                            state.name, expected, state.projection_targets
                        ));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "the generated playbook has transitions:",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected transitions table")?;
                let expected = parse_expected_transitions(table)?;
                let machine = generated_machine(&ctx)?;
                let actual = machine.transitions.clone();
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "transitions: expected {:#?} got {:#?}",
                        expected, actual
                    ))
                }
            },
        ),
        check_def(
            "the generated playbook measurement for state {string} role {string} has intent {string} and expected_output {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let state_name = params.get_string(0).ok_or("Expected state")?;
                let role = params.get_string(1).ok_or("Expected role")?;
                let expected_intent = params.get_string(2).ok_or("Expected intent")?;
                let expected_output = params.get_string(3).ok_or("Expected expected_output")?;
                let machine = generated_machine(&ctx)?;
                let state = machine
                    .states
                    .iter()
                    .find(|state| state.name == state_name)
                    .ok_or_else(|| format!("No state '{}'", state_name))?;
                let measurement = state
                    .measurement_by_role
                    .get(role)
                    .ok_or_else(|| format!("No measurement for state '{}' role '{}'", state_name, role))?;
                assert_eq("measurement.intent", &expected_intent, &measurement.intent)?;
                assert_eq(
                    "measurement.expected_output",
                    &expected_output,
                    &measurement.expected_output,
                )
            },
        ),
        check_def(
            "the generated playbook has no measurement for state {string} role {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let state_name = params.get_string(0).ok_or("Expected state")?;
                let role = params.get_string(1).ok_or("Expected role")?;
                let machine = generated_machine(&ctx)?;
                let state = machine
                    .states
                    .iter()
                    .find(|state| state.name == state_name)
                    .ok_or_else(|| format!("No state '{}'", state_name))?;
                if state.measurement_by_role.contains_key(role) {
                    Err(format!(
                        "Expected no measurement for state '{}' role '{}'",
                        state_name, role
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the generated playbook measurement for state {string} role {string} is non-empty",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let state_name = params.get_string(0).ok_or("Expected state")?;
                let role = params.get_string(1).ok_or("Expected role")?;
                let machine = generated_machine(&ctx)?;
                let state = machine
                    .states
                    .iter()
                    .find(|state| state.name == state_name)
                    .ok_or_else(|| format!("No state '{}'", state_name))?;
                let measurement = state
                    .measurement_by_role
                    .get(role)
                    .ok_or_else(|| format!("No measurement for state '{}' role '{}'", state_name, role))?;
                if measurement.intent.trim().is_empty()
                    || measurement.expected_output.trim().is_empty()
                {
                    Err(format!(
                        "Measurement for state '{}' role '{}' was empty: {:?}",
                        state_name, role, measurement
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the generated playbook measurement for state {string} role {string} contains {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let state_name = params.get_string(0).ok_or("Expected state")?;
                let role = params.get_string(1).ok_or("Expected role")?;
                let expected = params.get_string(2).ok_or("Expected text")?;
                let machine = generated_machine(&ctx)?;
                let state = machine
                    .states
                    .iter()
                    .find(|state| state.name == state_name)
                    .ok_or_else(|| format!("No state '{}'", state_name))?;
                let measurement = state
                    .measurement_by_role
                    .get(role)
                    .ok_or_else(|| {
                        format!("No measurement for state '{}' role '{}'", state_name, role)
                    })?;
                let actual = format!(
                    "{}\n{}\n{}",
                    measurement.intent,
                    measurement.expected_output,
                    measurement.success_criteria.as_deref().unwrap_or("")
                );
                if actual.contains(expected) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected measurement for state '{}' role '{}' to contain '{}', got {:?}",
                        state_name, role, expected, measurement
                    ))
                }
            },
        ),
        check_def(
            "the generated playbook measurement for state {string} role {string} has success_criteria {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let state_name = params.get_string(0).ok_or("Expected state")?;
                let role = params.get_string(1).ok_or("Expected role")?;
                let expected = params.get_string(2).ok_or("Expected success_criteria")?;
                let machine = generated_machine(&ctx)?;
                let state = machine
                    .states
                    .iter()
                    .find(|state| state.name == state_name)
                    .ok_or_else(|| format!("No state '{}'", state_name))?;
                let measurement = state
                    .measurement_by_role
                    .get(role)
                    .ok_or_else(|| {
                        format!("No measurement for state '{}' role '{}'", state_name, role)
                    })?;
                let actual = measurement.success_criteria.as_deref().ok_or_else(|| {
                    format!(
                        "Measurement for state '{}' role '{}' has no success_criteria",
                        state_name, role
                    )
                })?;
                assert_eq("measurement.success_criteria", &expected, actual)
            },
        ),
        check_def(
            "the generated playbook has a pre-completed outcome reflection review state",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, _params| {
                let machine = generated_machine(&ctx)?;
                let state = machine
                    .states
                    .iter()
                    .find(|state| state.name == "outcome_reflection_review")
                    .ok_or("No outcome_reflection_review state")?;
                if !state.is_review_gate || state.is_terminal {
                    return Err(format!(
                        "outcome_reflection_review should be a nonterminal review gate, got {:?}",
                        state
                    ));
                }
                let completed_idx = machine
                    .states
                    .iter()
                    .position(|state| state.name == "completed")
                    .ok_or("No completed state")?;
                let reflection_idx = machine
                    .states
                    .iter()
                    .position(|state| state.name == "outcome_reflection_review")
                    .ok_or("No outcome_reflection_review state")?;
                if reflection_idx < completed_idx {
                    Ok(())
                } else {
                    Err("outcome_reflection_review did not appear before completed".to_string())
                }
            },
        ),
        check_def(
            "the generated playbook has no measurements on state {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let state_name = params.get_string(0).ok_or("Expected state")?;
                let machine = generated_machine(&ctx)?;
                let state = machine
                    .states
                    .iter()
                    .find(|state| state.name == state_name)
                    .ok_or_else(|| format!("No state '{}'", state_name))?;
                if state.measurement_by_role.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected no measurements on '{}' but got {:?}",
                        state_name, state.measurement_by_role
                    ))
                }
            },
        ),
        check_def(
            "the generated playbook has no hook references",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, _params| {
                let machine = generated_machine(&ctx)?;
                for state in &machine.states {
                    if state.hook.is_some() || !state.hooks_by_role.is_empty() {
                        return Err(format!("State '{}' has hook references", state.name));
                    }
                }
                for transition in &machine.transitions {
                    if transition.hook.is_some() {
                        return Err(format!(
                            "Transition '{} -> {}' has a hook reference",
                            transition.from_state, transition.to_state
                        ));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "the generated playbook success rubric has dimensions:",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected rubric dimensions table")?;
                let machine = generated_machine(&ctx)?;
                let rubric = machine
                    .success_rubric
                    .as_ref()
                    .ok_or("Generated machine has no success_rubric")?;
                let dimension_idx = col(table, "dimension")?;
                let weight_idx = col(table, "weight")?;
                let evidence_idx = col(table, "evidence_class")?;
                if rubric.dimensions.len() != table.rows.len() {
                    return Err(format!(
                        "Expected {} rubric dimensions, got {}",
                        table.rows.len(),
                        rubric.dimensions.len()
                    ));
                }
                for (row, actual) in table.rows.iter().zip(rubric.dimensions.iter()) {
                    assert_eq(
                        "rubric.dimension",
                        cell(row, dimension_idx, "dimension")?,
                        &actual.dimension,
                    )?;
                    let expected_weight = cell(row, weight_idx, "weight")?
                        .parse::<u32>()
                        .map_err(|e| format!("invalid weight: {}", e))?;
                    if actual.weight != expected_weight {
                        return Err(format!(
                            "rubric.weight: expected {} got {}",
                            expected_weight, actual.weight
                        ));
                    }
                    let expected_evidence =
                        parse_evidence_class(cell(row, evidence_idx, "evidence_class")?)?;
                    if actual.evidence_class != expected_evidence {
                        return Err(format!(
                            "rubric.evidence_class: expected {:?} got {:?}",
                            expected_evidence, actual.evidence_class
                        ));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "the generated playbook success rubric has anchors:",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected anchors table")?;
                let expected = parse_anchors(table)?;
                let machine = generated_machine(&ctx)?;
                let actual = &machine
                    .success_rubric
                    .as_ref()
                    .ok_or("Generated machine has no success_rubric")?
                    .anchors;
                if actual == &expected {
                    Ok(())
                } else {
                    Err(format!("Expected anchors {:?}, got {:?}", expected, actual))
                }
            },
        ),
        check_def(
            "the generated playbook has outcome predicate terminal_state {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected terminal_state")?;
                let machine = generated_machine(&ctx)?;
                let predicate = machine
                    .outcome_predicate
                    .as_ref()
                    .ok_or("Generated machine has no outcome_predicate")?;
                assert_eq("outcome_predicate.terminal_state", expected, &predicate.terminal_state)
            },
        ),
        check_def(
            "the generated playbook has outcome predicate check {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected check")?;
                let machine = generated_machine(&ctx)?;
                let predicate = machine
                    .outcome_predicate
                    .as_ref()
                    .ok_or("Generated machine has no outcome_predicate")?;
                let actual = predicate
                    .check
                    .as_deref()
                    .ok_or("Generated machine's outcome_predicate has no check")?;
                assert_eq("outcome_predicate.check", expected, actual)
            },
        ),
        check_def(
            "the generated playbook has no outcome predicate",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, _params| {
                let machine = generated_machine(&ctx)?;
                if machine.outcome_predicate.is_none() {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected no outcome_predicate, got {:?}",
                        machine.outcome_predicate
                    ))
                }
            },
        ),
        check_def(
            "the generated playbook passes loader validation with artifact id {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let artifact_id = params.get_string(0).ok_or("Expected artifact id")?;
                let machine = generated_machine(&ctx)?;
                validate_with_id(machine, &artifact_id, &[])
                    .map_err(|error| format!("Loader validation failed: {}", error))
            },
        ),
        check_def(
            "candidate playbook generation fails with EmptyProposedStates",
            &[(ERROR_KEY, "Option<GenerateError>")],
            |ctx, _params| assert_error(&ctx, GenerateError::EmptyProposedStates),
        ),
        check_def(
            "candidate playbook generation fails with BlankIntent",
            &[(ERROR_KEY, "Option<GenerateError>")],
            |ctx, _params| assert_error(&ctx, GenerateError::BlankIntent),
        ),
        check_def(
            "candidate playbook generation fails with BlankRouteDescription",
            &[(ERROR_KEY, "Option<GenerateError>")],
            |ctx, _params| assert_error(&ctx, GenerateError::BlankRouteDescription),
        ),
        check_def(
            "candidate playbook generation fails with EmptyRouteTriggers",
            &[(ERROR_KEY, "Option<GenerateError>")],
            |ctx, _params| assert_error(&ctx, GenerateError::EmptyRouteTriggers),
        ),
        check_def(
            "candidate playbook generation fails with EmptyProjectionTargets",
            &[(ERROR_KEY, "Option<GenerateError>")],
            |ctx, _params| assert_error(&ctx, GenerateError::EmptyProjectionTargets),
        ),
        check_def(
            "candidate playbook generation failure contains {string}",
            &[(ERROR_KEY, "Option<GenerateError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected message")?;
                let error = ctx
                    .get::<Option<GenerateError>>(ERROR_KEY)
                    .ok_or("No generation error key")?
                    .as_ref()
                    .ok_or("Expected generation to fail, but it succeeded")?;
                let actual = error.to_string();
                if actual.contains(expected) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected generation error to contain '{}', got '{}'",
                        expected, actual
                    ))
                }
            },
        ),
        check_def(
            "candidate playbook generation fails with BlankProposedStateField for field {string} at index {int}",
            &[(ERROR_KEY, "Option<GenerateError>")],
            |ctx, params| {
                let field = params.get_string(0).ok_or("Expected field")?;
                let index = params.get_int(1).ok_or("Expected index")?;
                assert_error(
                    &ctx,
                    GenerateError::BlankProposedStateField {
                        index: index as usize,
                        field: field.to_string(),
                    },
                )
            },
        ),
        check_def(
            "candidate playbook generation fails with ReservedRole for role {string}",
            &[(ERROR_KEY, "Option<GenerateError>")],
            |ctx, params| {
                let role = params.get_string(0).ok_or("Expected reserved role")?;
                assert_error(
                    &ctx,
                    GenerateError::ReservedRole {
                        role: role.to_string(),
                    },
                )
            },
        ),
        check_def(
            "candidate playbook generation fails with StateNameCollision for name {string}",
            &[(ERROR_KEY, "Option<GenerateError>")],
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected collision name")?;
                assert_error(
                    &ctx,
                    GenerateError::StateNameCollision {
                        name: name.to_string(),
                    },
                )
            },
        ),
        check_def(
            "candidate playbook generation fails with VacuousSuccessCriteria for state {string}",
            &[(ERROR_KEY, "Option<GenerateError>")],
            |ctx, params| {
                let state = params.get_string(0).ok_or("Expected state")?;
                assert_error(
                    &ctx,
                    GenerateError::VacuousSuccessCriteria {
                        state: state.to_string(),
                    },
                )
            },
        ),
        check_def(
            "candidate playbook generation fails with MissingOutcomePredicate",
            &[(ERROR_KEY, "Option<GenerateError>")],
            |ctx, _params| assert_error(&ctx, GenerateError::MissingOutcomePredicate),
        ),
        step_def(
            "the falsifiability lint evaluates the criterion {string}",
            &[],
            &[(LINT_VERDICT_KEY, "bool")],
            |_ctx, params| {
                let criterion = params.get_string(0).ok_or("Expected criterion")?;
                let mut out = Context::new();
                out.set(LINT_VERDICT_KEY, criterion_is_falsifiable(criterion));
                Ok(out)
            },
        ),
        check_def(
            "the criterion is judged {string}",
            &[(LINT_VERDICT_KEY, "bool")],
            |ctx, params| {
                let expected = match params.get_string(0).ok_or("Expected verdict")? {
                    "falsifiable" => true,
                    "vacuous" => false,
                    other => return Err(format!("Unknown verdict literal '{}'", other)),
                };
                let actual = ctx
                    .get::<bool>(LINT_VERDICT_KEY)
                    .ok_or("No lint verdict key")?;
                if *actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "criterion_is_falsifiable: expected {} got {}",
                        expected, actual
                    ))
                }
            },
        ),
    ]
}

fn fixture_candidate(intent: String, proposed_states: Vec<ProposedState>) -> CandidatePlaybook {
    CandidatePlaybook {
        source: "lore".to_string(),
        evidence: vec!["obs-1".to_string()],
        intent,
        route_description: "Route here when the user asks to AUTHOR applicant interview intake playbooks. NOT for running an applicant interview intake instance.".to_string(),
        route_triggers: vec![
            "create interview playbook".to_string(),
            "author interview intake".to_string(),
        ],
        projection_targets: vec!["workflows.md".to_string()],
        register: Register::Driven,
        proposed_states,
        success_rubric: None,
        outcome_predicate: None,
        anchors: Vec::new(),
        exemplars: Vec::new(),
        ledger_classification: None,
        none_yet_justification: None,
        at: "2026-06-08T00:00:00Z".to_string(),
    }
}

fn parse_anchors(table: &DataTable) -> Result<Vec<AnchorRef>, String> {
    let instance_idx = col(table, "instance")?;
    let band_idx = col(table, "band")?;
    table
        .rows
        .iter()
        .map(|row| {
            Ok(AnchorRef {
                instance: cell(row, instance_idx, "instance")?.to_string(),
                band: cell(row, band_idx, "band")?.to_string(),
            })
        })
        .collect()
}

fn parse_evidence_class(value: &str) -> Result<EvidenceClass, String> {
    match value {
        "artifact_of_consequence" => Ok(EvidenceClass::ArtifactOfConsequence),
        "verifiable_citation" => Ok(EvidenceClass::VerifiableCitation),
        "self_description" => Ok(EvidenceClass::SelfDescription),
        other => Err(format!("unknown evidence_class '{}'", other)),
    }
}

fn parse_single_column(table: &DataTable, column: &str) -> Result<Vec<String>, String> {
    let idx = col(table, column)?;
    table
        .rows
        .iter()
        .map(|row| cell(row, idx, column).map(str::to_string))
        .collect()
}

fn parse_proposed_states(table: &DataTable) -> Result<Vec<ProposedState>, String> {
    let state_idx = col(table, "state")?;
    let role_idx = col(table, "role")?;
    let intent_idx = col(table, "intent")?;
    let output_idx = col(table, "expected_output")?;
    // Optional: existing scenario tables predate success_criteria and omit
    // the column entirely, which must keep parsing (candidates authored
    // without it are exactly what the generator's VacuousSuccessCriteria gate
    // exists to reject downstream, not a step-parsing failure).
    let criteria_idx = col_opt(table, "success_criteria");

    table
        .rows
        .iter()
        .map(|row| {
            let success_criteria = match criteria_idx {
                Some(idx) => {
                    let value = cell(row, idx, "success_criteria")?;
                    if value.is_empty() {
                        None
                    } else {
                        Some(value.to_string())
                    }
                }
                None => None,
            };
            Ok(ProposedState {
                state: cell(row, state_idx, "state")?.to_string(),
                role: cell(row, role_idx, "role")?.to_string(),
                intent: cell(row, intent_idx, "intent")?.to_string(),
                expected_output: cell(row, output_idx, "expected_output")?.to_string(),
                success_criteria,
                evidence_obligation: Vec::new(),
            })
        })
        .collect()
}

fn parse_expected_transitions(table: &DataTable) -> Result<Vec<TransitionDefinition>, String> {
    let from_idx = col(table, "from_state")?;
    let to_idx = col(table, "to_state")?;
    let role_idx = col(table, "required_role")?;
    let satisfaction_idx = col(table, "required_satisfaction")?;

    table
        .rows
        .iter()
        .map(|row| {
            let satisfaction = cell(row, satisfaction_idx, "required_satisfaction")?;
            Ok(TransitionDefinition {
                from_state: cell(row, from_idx, "from_state")?.to_string(),
                to_state: cell(row, to_idx, "to_state")?.to_string(),
                required_role: cell(row, role_idx, "required_role")?.to_string(),
                required_satisfaction: if satisfaction.is_empty() {
                    None
                } else {
                    Some(vec![satisfaction.to_string()])
                },
                requires_approver: satisfaction == "satisfied",
                hook: None,
            })
        })
        .collect()
}

fn generated_machine(ctx: &Context) -> Result<&PlaybookMachine, String> {
    ctx.get::<Option<PlaybookMachine>>(MACHINE_KEY)
        .ok_or("No generated machine key".to_string())?
        .as_ref()
        .ok_or("Generation did not succeed".to_string())
}

fn assert_error(ctx: &Context, expected: GenerateError) -> Result<(), String> {
    let actual = ctx
        .get::<Option<GenerateError>>(ERROR_KEY)
        .ok_or("No generation error key".to_string())?;
    match actual {
        Some(error) if error == &expected => Ok(()),
        Some(error) => Err(format!("Expected {:?} got {:?}", expected, error)),
        None => Err(format!("Expected {:?} but generation succeeded", expected)),
    }
}

fn col(table: &DataTable, name: &str) -> Result<usize, String> {
    table
        .headers
        .iter()
        .position(|header| header == name)
        .ok_or_else(|| format!("Missing column '{}'", name))
}

fn col_opt(table: &DataTable, name: &str) -> Option<usize> {
    table.headers.iter().position(|header| header == name)
}

fn cell<'a>(row: &'a [String], idx: usize, name: &str) -> Result<&'a str, String> {
    row.get(idx)
        .map(|value| value.trim())
        .ok_or_else(|| format!("Row too short for '{}'", name))
}

fn assert_eq(field: &str, expected: &str, actual: &str) -> Result<(), String> {
    if expected == actual {
        Ok(())
    } else {
        Err(format!(
            "{}: expected '{}' got '{}'",
            field, expected, actual
        ))
    }
}

fn assert_bool(field: &str, expected: &str, actual: bool) -> Result<(), String> {
    match expected {
        "true" if actual => Ok(()),
        "false" if !actual => Ok(()),
        "true" | "false" => Err(format!(
            "{}: expected '{}' got '{}'",
            field, expected, actual
        )),
        _ => Err(format!(
            "{}: expected boolean literal, got '{}'",
            field, expected
        )),
    }
}
