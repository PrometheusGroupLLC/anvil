use anvil_core::domain::begin::{BeginCommandHandler, BeginError, BeginOutcome, BeginRequest};
use anvil_core::domain::describe::{DescribeQueryHandler, DescribeRequest, DescribeResult};
use anvil_core::domain::playbook::registry::{
    driven_candidates, resolve_route, SeedPlaybookRegistry,
};
use anvil_core::domain::playbook::seeds::initiative_seed;
use anvil_core::domain::routing::{
    compute_execution_route, SUBJECT_AVAILABLE_ACTION, SUBJECT_AVAILABLE_TYPE,
};
use anvil_core::domain::shared_types::RequestContext;
use anvil_core_hearth::in_memory_query_adapter::InMemoryQueryAdapter;
use anvil_core_hearth::test_describe_adapter::TestDescribeAdapter;
use anvil_core::ports::describe_port::InstanceState;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::collections::HashMap;

const BEGIN_RESULT_KEY: &str = "initiative_begin_result";
const DESCRIBE_RESULT_KEY: &str = "initiative_describe_result";
const ROUTE_CANDIDATES_KEY: &str = "initiative_route_candidates";
const ROUTE_SELECTED_KEY: &str = "initiative_route_selected";
const PLAYBOOK_KEY: &str = "initiative_execution_route";

fn begin_request() -> BeginRequest {
    BeginRequest {
        ctx: RequestContext::default_safe(),
        artifact_type: "initiative".to_string(),
        track_name: "Engine Initiative".to_string(),
        approver: "Human".to_string(),
        actor_name: "Initiative-Test-000000".to_string(),
        actor_type: "agent".to_string(),
        actor_model: "test".to_string(),
        actor_provider: "test".to_string(),
        actor_context_window: 0,
        actor_sdk_version: String::new(),
        actor_entrypoint: String::new(),
        ..Default::default()
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "begin create initiative is executed with a draft hook",
            &[],
            &[(BEGIN_RESULT_KEY, "Result<BeginOutcome, BeginError>")],
            |_ctx, _params| {
                let mut query = InMemoryQueryAdapter::new();
                query.with_playbook_hook_body(
                    "initiative_lifecycle",
                    "draft.md",
                    "INITIATIVE DRAFT HOOK",
                );
                let registry = SeedPlaybookRegistry;
                let result = BeginCommandHandler::execute(&query, &registry, begin_request());
                let mut out = Context::new();
                out.set(BEGIN_RESULT_KEY, result);
                Ok(out)
            },
        ),
        check_def(
            "the initiative begin result is state {string}",
            &[(BEGIN_RESULT_KEY, "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?;
                match ctx
                    .get::<Result<BeginOutcome, BeginError>>(BEGIN_RESULT_KEY)
                    .ok_or("No begin result")?
                {
                    Ok(outcome) if outcome.result.state == expected => Ok(()),
                    Ok(outcome) => Err(format!(
                        "Expected state '{}', got '{}'",
                        expected, outcome.result.state
                    )),
                    Err(e) => Err(format!("Expected begin success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the initiative begin context contains {string}",
            &[(BEGIN_RESULT_KEY, "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected text")?;
                match ctx
                    .get::<Result<BeginOutcome, BeginError>>(BEGIN_RESULT_KEY)
                    .ok_or("No begin result")?
                {
                    Ok(outcome) if outcome.result.context_text.contains(needle) => Ok(()),
                    Ok(outcome) => Err(format!(
                        "context_text did not contain '{}': {}",
                        needle, outcome.result.context_text
                    )),
                    Err(e) => Err(format!("Expected begin success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the initiative begin event scaffolds files:",
            &[(BEGIN_RESULT_KEY, "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let expected = params
                    .data_table()
                    .ok_or("Expected table")?
                    .rows
                    .iter()
                    .filter_map(|row| row.first().cloned())
                    .collect::<Vec<_>>();
                let outcome = ctx
                    .get::<Result<BeginOutcome, BeginError>>(BEGIN_RESULT_KEY)
                    .ok_or("No begin result")?
                    .as_ref()
                    .map_err(|e| format!("Expected begin success, got error: {}", e))?;
                let mut actual = Vec::new();
                for event in &outcome.events {
                    if let anvil_core::domain::events::Event::ArtifactCreation {
                        scaffold_files,
                        ..
                    } = event
                    {
                        actual = scaffold_files.iter().map(|(name, _)| name.clone()).collect();
                    }
                }
                for filename in expected {
                    if !actual.contains(&filename) {
                        return Err(format!(
                            "Expected scaffold file '{}' in {:?}",
                            filename, actual
                        ));
                    }
                }
                Ok(())
            },
        ),
        step_def(
            "describe is called for an initiative in state {string}",
            &[],
            &[(DESCRIBE_RESULT_KEY, "DescribeResult")],
            |_ctx, params| {
                let state = params.get_string(0).ok_or("Expected state")?.to_string();
                let mut instances = HashMap::new();
                instances.insert(
                    "initiatives/example".to_string(),
                    InstanceState {
                        kind: "initiative".to_string(),
                        state,
                        transition_count: 1,
                        last_transition: None,
                    },
                );
                let adapter = TestDescribeAdapter::new(instances);
                let registry = SeedPlaybookRegistry;
                let result = DescribeQueryHandler::execute(
                    &adapter,
                    &registry,
                    DescribeRequest {
                        identifier: "initiatives/example".to_string(),
                    },
                )
                .map_err(|e| e.to_string())?;
                let mut out = Context::new();
                out.set(DESCRIBE_RESULT_KEY, result);
                Ok(out)
            },
        ),
        check_def(
            "the initiative describe actions include {string} with execution_route {string}",
            &[(DESCRIBE_RESULT_KEY, "DescribeResult")],
            |ctx, params| {
                let action = params.get_string(0).ok_or("Expected action")?;
                let playbook = params.get_string(1).ok_or("Expected playbook")?;
                let result = ctx
                    .get::<DescribeResult>(DESCRIBE_RESULT_KEY)
                    .ok_or("No describe result")?;
                match result {
                    DescribeResult::InstanceInfo {
                        available_actions, ..
                    } => {
                        let found = available_actions
                            .iter()
                            .any(|a| a.action == action && a.execution_route == playbook);
                        if found {
                            Ok(())
                        } else {
                            Err(format!(
                                "No action '{}' with playbook '{}' in {:?}",
                                action, playbook, available_actions
                            ))
                        }
                    }
                    other => Err(format!("Expected instance info, got {:?}", other)),
                }
            },
        ),
        step_def(
            "execution_route is computed for initiative available_type",
            &[],
            &[(PLAYBOOK_KEY, "String")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set(
                    PLAYBOOK_KEY,
                    compute_execution_route(
                        SUBJECT_AVAILABLE_TYPE,
                        "initiative",
                        "",
                        "creator",
                    ),
                );
                Ok(out)
            },
        ),
        step_def(
            "execution_route is computed for initiative action state {string} role {string}",
            &[],
            &[(PLAYBOOK_KEY, "String")],
            |_ctx, params| {
                let state = params.get_string(0).ok_or("Expected state")?;
                let role = params.get_string(1).ok_or("Expected role")?;
                let mut out = Context::new();
                out.set(
                    PLAYBOOK_KEY,
                    compute_execution_route(
                        SUBJECT_AVAILABLE_ACTION,
                        "initiative",
                        state,
                        role,
                    ),
                );
                Ok(out)
            },
        ),
        check_def(
            "the initiative execution_route is {string}",
            &[(PLAYBOOK_KEY, "String")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected playbook")?;
                let actual = ctx.get::<String>(PLAYBOOK_KEY).ok_or("No playbook")?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("Expected playbook '{}', got '{}'", expected, actual))
                }
            },
        ),
        step_def(
            "initiative seed route candidates are selected",
            &[],
            &[(ROUTE_CANDIDATES_KEY, "Vec<String>")],
            |_ctx, _params| {
                let registry = SeedPlaybookRegistry;
                let candidates = driven_candidates(&registry)
                    .into_iter()
                    .map(|m| m.kind.clone())
                    .collect::<Vec<_>>();
                let mut out = Context::new();
                out.set(ROUTE_CANDIDATES_KEY, candidates);
                Ok(out)
            },
        ),
        check_def(
            "the initiative route candidates do not include initiative",
            &[(ROUTE_CANDIDATES_KEY, "Vec<String>")],
            |ctx, _params| {
                let candidates = ctx
                    .get::<Vec<String>>(ROUTE_CANDIDATES_KEY)
                    .ok_or("No candidates")?;
                if candidates.iter().any(|kind| kind == "initiative") {
                    Err(format!("initiative unexpectedly present in {:?}", candidates))
                } else {
                    Ok(())
                }
            },
        ),
        step_def(
            "route resolution is called for initiative-like input",
            &[],
            &[(ROUTE_SELECTED_KEY, "String")],
            |_ctx, _params| {
                let registry = SeedPlaybookRegistry;
                let result = resolve_route(
                    &registry,
                    &RequestContext::default_safe(),
                    "create an initiative for engine lifecycle",
                );
                let mut out = Context::new();
                out.set(ROUTE_SELECTED_KEY, result.selected_kind.unwrap_or_default());
                Ok(out)
            },
        ),
        check_def(
            "route selects no initiative kind",
            &[(ROUTE_SELECTED_KEY, "String")],
            |ctx, _params| {
                let selected = ctx
                    .get::<String>(ROUTE_SELECTED_KEY)
                    .ok_or("No selected kind")?;
                if selected.is_empty() {
                    Ok(())
                } else {
                    Err(format!("Expected no selected kind, got '{}'", selected))
                }
            },
        ),
        check_def("the initiative seed is register free", &[], |_ctx, _params| {
            if initiative_seed().is_driven() {
                Err("initiative seed is unexpectedly driven".to_string())
            } else {
                Ok(())
            }
        }),
        check_def(
            "the initiative seed has transition {string} to {string} role {string}",
            &[],
            |_ctx, params| {
                let from = params.get_string(0).ok_or("Expected from")?;
                let to = params.get_string(1).ok_or("Expected to")?;
                let role = params.get_string(2).ok_or("Expected role")?;
                assert_transition(from, to, role, None, false)
            },
        ),
        check_def(
            "the initiative seed has transition {string} to {string} role {string} requiring approver",
            &[],
            |_ctx, params| {
                let from = params.get_string(0).ok_or("Expected from")?;
                let to = params.get_string(1).ok_or("Expected to")?;
                let role = params.get_string(2).ok_or("Expected role")?;
                assert_transition(from, to, role, None, true)
            },
        ),
        check_def(
            "the initiative seed has review transition {string} to {string} role {string} satisfaction {string}",
            &[],
            |_ctx, params| {
                let from = params.get_string(0).ok_or("Expected from")?;
                let to = params.get_string(1).ok_or("Expected to")?;
                let role = params.get_string(2).ok_or("Expected role")?;
                let satisfaction = params.get_string(3).ok_or("Expected satisfaction")?;
                assert_transition(from, to, role, Some(satisfaction), role == "reviewer")
            },
        ),
    ]
}

fn assert_transition(
    from: &str,
    to: &str,
    role: &str,
    satisfaction: Option<&str>,
    requires_approver: bool,
) -> Result<(), String> {
    let found = initiative_seed().transitions.iter().any(|t| {
        t.from_state == from
            && t.to_state == to
            && t.required_role == role
            && t.requires_approver == requires_approver
            && match (&t.required_satisfaction, satisfaction) {
                (None, None) => true,
                (Some(values), Some(expected)) => values.iter().any(|value| value == expected),
                _ => false,
            }
    });
    if found {
        Ok(())
    } else {
        Err(format!(
            "Missing transition {} -> {} role {} satisfaction {:?} requires_approver {}",
            from, to, role, satisfaction, requires_approver
        ))
    }
}
