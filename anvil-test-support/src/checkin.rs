use crate::retained_temp_dir;
use anvil_core::domain::checkin::{
    CheckinError, CheckinQueryHandler, CheckinQueryRequest, CheckinQueryResult,
};
use anvil_core::domain::describe::{
    DescribeError, DescribeQueryHandler, DescribeRequest, DescribeResult,
};
use anvil_core::domain::playbook::composite_registry::CompositePlaybookRegistry;
use anvil_core::domain::playbook::hearth_registry::HearthPlaybookRegistry;
use anvil_core::domain::playbook::registry::SeedPlaybookRegistry;
use anvil_core::domain::{
    available_artifact_types, ArtifactSummary, AvailableArtifactType,
};
use anvil_core_hearth::test_checkin_query_adapter::TestCheckinQueryAdapter;
use anvil_core_hearth::test_describe_adapter::TestDescribeAdapter;
use anvil_core::ports::describe_port::InstanceState;

// Per Phase 4 of the checkin_backfill_spec_context track: shared helpers
// no longer synthesize actor identity centrally. Step bodies that don't
// otherwise expose identity to the scenario inline their own defaults so
// the values are visible at the call site rather than hidden behind a
// helper.
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::collections::HashMap;

fn column_index(table: &DataTable, name: &str) -> Result<usize, String> {
    table
        .headers
        .iter()
        .position(|h| h == name)
        .ok_or_else(|| format!("Missing '{}' column in data table", name))
}


pub fn steps() -> Vec<StepDef> {
    vec![
        // ===== Checkin query (role-based filtering) steps =====
        step_def(
            "a hearth with artifacts for checkin query:",
            &[],
            &[("cq_artifacts", "Vec<ArtifactSummary>")],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let id_col = column_index(table, "id")?;
                let type_col = column_index(table, "type")?;
                let state_col = column_index(table, "state")?;
                let summary_col = column_index(table, "summary")?;

                let mut artifacts = Vec::new();
                for row in &table.rows {
                    artifacts.push(ArtifactSummary {
                        id: row[id_col].trim().to_string(),
                        artifact_type: crate::parse_artifact_type(&row[type_col])?,
                        state: row[state_col].trim().to_string(),
                        summary: row[summary_col].trim().to_string(),
                        execution_route: String::new(),
                    });
                }

                let mut out = Context::new();
                out.set("cq_artifacts", artifacts);
                Ok(out)
            },
        ),
        step_def(
            "the checkin query word list is {string}",
            &[("cq_artifacts", "Vec<ArtifactSummary>")],
            &[("cq_adapter", "TestCheckinQueryAdapter"), ("cq_artifacts", "Vec<ArtifactSummary>")],
            |ctx, params| {
                let words_str = params.get_string(0).ok_or("Expected words")?;
                let words: Vec<String> = words_str.split(',').map(|s| s.trim().to_string()).collect();
                let artifacts = ctx.get::<Vec<ArtifactSummary>>("cq_artifacts")
                    .ok_or("No cq_artifacts")?.clone();
                let adapter = TestCheckinQueryAdapter::new(artifacts.clone(), words);
                let mut out = Context::new();
                out.set("cq_adapter", adapter);
                out.set("cq_artifacts", artifacts);
                Ok(out)
            },
        ),
        step_def(
            "checkin query is executed with role {string}",
            &[("cq_adapter", "TestCheckinQueryAdapter")],
            &[("cq_result", "Result<CheckinQueryResult, CheckinError>")],
            |ctx, params| {
                let role = params.get_string(0).ok_or("Expected role")?.to_string();
                let adapter = ctx.get::<TestCheckinQueryAdapter>("cq_adapter")
                    .ok_or("No cq_adapter")?;
                let request = CheckinQueryRequest {
                    role,
                    actor_type: "agent".to_string(),
                    actor_model: "test-model".to_string(),
                    actor_provider: "test".to_string(),
                };
                let result = CheckinQueryHandler::execute(adapter, request);
                let mut out = Context::new();
                out.set("cq_result", result);
                Ok(out)
            },
        ),
        check_def(
            "the checkin query result has a generated actor name",
            &[("cq_result", "Result<CheckinQueryResult, CheckinError>")],
            |ctx, _params| {
                let result = ctx.get::<Result<CheckinQueryResult, CheckinError>>("cq_result")
                    .ok_or("No cq_result")?;
                match result {
                    Ok(r) => {
                        if !r.actor_name.contains('-') || r.actor_name.len() < 5 {
                            return Err(format!("Actor name '{}' doesn't look generated", r.actor_name));
                        }
                        Ok(())
                    }
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the checkin query filtered artifacts include {string} with state {string}",
            &[("cq_result", "Result<CheckinQueryResult, CheckinError>")],
            |ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?;
                let state = params.get_string(1).ok_or("Expected state")?;
                let result = ctx.get::<Result<CheckinQueryResult, CheckinError>>("cq_result")
                    .ok_or("No cq_result")?;
                match result {
                    Ok(r) => {
                        let found = r.filtered_artifacts.iter().find(|a| a.id == *id);
                        match found {
                            None => Err(format!("Artifact '{}' not in filtered list: {:?}",
                                id, r.filtered_artifacts.iter().map(|a| &a.id).collect::<Vec<_>>())),
                            Some(a) if a.state == *state => Ok(()),
                            Some(a) => Err(format!("Artifact '{}' state: expected '{}', got '{}'", id, state, a.state)),
                        }
                    }
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the checkin query filtered artifacts do not include {string}",
            &[("cq_result", "Result<CheckinQueryResult, CheckinError>")],
            |ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?;
                let result = ctx.get::<Result<CheckinQueryResult, CheckinError>>("cq_result")
                    .ok_or("No cq_result")?;
                match result {
                    Ok(r) => {
                        if r.filtered_artifacts.iter().any(|a| a.id == *id) {
                            Err(format!("Artifact '{}' should not be in filtered list", id))
                        } else {
                            Ok(())
                        }
                    }
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the checkin query result includes available type {string}",
            &[("cq_result", "Result<CheckinQueryResult, CheckinError>")],
            |ctx, params| {
                let type_name = params.get_string(0).ok_or("Expected type")?;
                let result = ctx.get::<Result<CheckinQueryResult, CheckinError>>("cq_result")
                    .ok_or("No cq_result")?;
                match result {
                    Ok(r) => {
                        if r.available_types.iter().any(|t| t.name == *type_name) { Ok(()) }
                        else { Err(format!("Type '{}' not in available types", type_name)) }
                    }
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the checkin query result has no available types",
            &[("cq_result", "Result<CheckinQueryResult, CheckinError>")],
            |ctx, _params| {
                let result = ctx.get::<Result<CheckinQueryResult, CheckinError>>("cq_result")
                    .ok_or("No cq_result")?;
                match result {
                    Ok(r) if r.available_types.is_empty() => Ok(()),
                    Ok(r) => Err(format!("Expected no available types, got {}", r.available_types.len())),
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the checkin query filtered artifacts include {string} with execution_route {string}",
            &[("cq_result", "Result<CheckinQueryResult, CheckinError>")],
            |ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?;
                let expected = params.get_string(1).ok_or("Expected execution_route")?;
                let result = ctx.get::<Result<CheckinQueryResult, CheckinError>>("cq_result")
                    .ok_or("No cq_result")?;
                match result {
                    Ok(r) => match r.filtered_artifacts.iter().find(|a| a.id == *id) {
                        None => Err(format!("Artifact '{}' not found", id)),
                        Some(a) if a.execution_route == *expected => Ok(()),
                        Some(a) => Err(format!(
                            "Artifact '{}' execution_route: expected '{}', got '{}'",
                            id, expected, a.execution_route
                        )),
                    },
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the checkin query available type {string} has execution_route {string}",
            &[("cq_result", "Result<CheckinQueryResult, CheckinError>")],
            |ctx, params| {
                let type_name = params.get_string(0).ok_or("Expected type")?;
                let expected = params.get_string(1).ok_or("Expected execution_route")?;
                let result = ctx.get::<Result<CheckinQueryResult, CheckinError>>("cq_result")
                    .ok_or("No cq_result")?;
                match result {
                    Ok(r) => match r.available_types.iter().find(|t| t.name == *type_name) {
                        None => Err(format!("Type '{}' not in available_types", type_name)),
                        Some(t) if t.execution_route == *expected => Ok(()),
                        Some(t) => Err(format!(
                            "Type '{}' execution_route: expected '{}', got '{}'",
                            type_name, expected, t.execution_route
                        )),
                    },
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the describe instance available actions include {string} with execution_route {string}",
            &[("desc_result", "Result<DescribeResult, DescribeError>")],
            |ctx, params| {
                let action = params.get_string(0).ok_or("Expected action")?;
                let expected = params.get_string(1).ok_or("Expected execution_route")?;
                match ctx.get::<Result<DescribeResult, DescribeError>>("desc_result").ok_or("No result")? {
                    Ok(DescribeResult::InstanceInfo { available_actions, .. }) => {
                        match available_actions.iter().find(|a| a.action == *action) {
                            None => Err(format!("Action '{}' not found", action)),
                            Some(a) if a.execution_route == *expected => Ok(()),
                            Some(a) => Err(format!(
                                "Action '{}' execution_route: expected '{}', got '{}'",
                                action, expected, a.execution_route
                            )),
                        }
                    }
                    _ => Err("Not InstanceInfo".to_string()),
                }
            },
        ),
        check_def(
            "the checkin query returns an UnsupportedRole error for {string}",
            &[("cq_result", "Result<CheckinQueryResult, CheckinError>")],
            |ctx, params| {
                let expected_role = params.get_string(0).ok_or("Expected role")?;
                let result = ctx.get::<Result<CheckinQueryResult, CheckinError>>("cq_result")
                    .ok_or("No cq_result")?;
                match result {
                    Err(CheckinError::UnsupportedRole { role }) if role == expected_role => Ok(()),
                    Err(e) => Err(format!("Expected UnsupportedRole for '{}', got: {}", expected_role, e)),
                    Ok(_) => Err("Expected error, got success".to_string()),
                }
            },
        ),
        // ===== Describe steps =====
        step_def(
            "a describe handler with type schemas",
            &[],
            &[("desc_adapter", "TestDescribeAdapter")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set("desc_adapter", TestDescribeAdapter::new(HashMap::new()));
                Ok(out)
            },
        ),
        step_def(
            "a describe handler with instances:",
            &[],
            &[("desc_adapter", "TestDescribeAdapter")],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let id_col = column_index(table, "id")?;
                let kind_col = column_index(table, "kind")?;
                let state_col = column_index(table, "state")?;
                let trans_col = column_index(table, "transitions")?;
                let mut instances = HashMap::new();
                for row in &table.rows {
                    let id = row[id_col].trim().to_string();
                    instances.insert(id, InstanceState {
                        kind: row[kind_col].trim().to_string(),
                        state: row[state_col].trim().to_string(),
                        transition_count: row[trans_col].trim().parse().map_err(|_| "bad count")?,
                        last_transition: None,
                    });
                }
                let mut out = Context::new();
                out.set("desc_adapter", TestDescribeAdapter::new(instances));
                Ok(out)
            },
        ),
        step_def(
            "describe is called with identifier {string}",
            &[("desc_adapter", "TestDescribeAdapter")],
            &[("desc_result", "Result<DescribeResult, DescribeError>")],
            |ctx, params| {
                let identifier = params.get_string(0).ok_or("Expected identifier")?.to_string();
                let adapter = ctx.get::<TestDescribeAdapter>("desc_adapter").ok_or("No desc_adapter")?;
                let registry = SeedPlaybookRegistry;
                let result = DescribeQueryHandler::execute(adapter, &registry, DescribeRequest { identifier });
                let mut out = Context::new();
                out.set("desc_result", result);
                Ok(out)
            },
        ),
        // BP7 (AC7): describe through a CompositePlaybookRegistry over a hearth
        // that physically holds the knowledge_lifecycle machine.yaml, so the
        // describe seam can discriminate a knowledge (state, role) action as
        // "engine" (machine-derived) on the describe path.
        step_def(
            "describe is called with identifier {string} using the knowledge_lifecycle machine",
            &[("desc_adapter", "TestDescribeAdapter")],
            &[("desc_result", "Result<DescribeResult, DescribeError>")],
            |ctx, params| {
                let identifier = params.get_string(0).ok_or("Expected identifier")?.to_string();
                let adapter = ctx.get::<TestDescribeAdapter>("desc_adapter").ok_or("No desc_adapter")?;
                let (_handle, tmp) = retained_temp_dir("anvil-describe-knowledge-")?;
                let wf_dir = tmp.join("playbooks").join("20260529T0409_knowledge_lifecycle");
                std::fs::create_dir_all(&wf_dir).map_err(|e| format!("mkdir: {}", e))?;
                std::fs::write(
                    wf_dir.join("machine.yaml"),
                    crate::query_port::knowledge_lifecycle_machine_yaml(),
                )
                .map_err(|e| format!("write machine.yaml: {}", e))?;
                let registry = CompositePlaybookRegistry::new(
                    HearthPlaybookRegistry::new(tmp.clone()),
                    SeedPlaybookRegistry,
                );
                let result =
                    DescribeQueryHandler::execute(adapter, &registry, DescribeRequest { identifier });
                let mut out = Context::new();
                out.set("desc_result", result);
                Ok(out)
            },
        ),
        check_def("the describe result is type info", &[("desc_result", "Result<DescribeResult, DescribeError>")], |ctx, _| {
            match ctx.get::<Result<DescribeResult, DescribeError>>("desc_result").ok_or("No result")? {
                Ok(DescribeResult::TypeInfo { .. }) => Ok(()),
                other => Err(format!("Expected TypeInfo, got {:?}", other)),
            }
        }),
        check_def("the describe result is instance info", &[("desc_result", "Result<DescribeResult, DescribeError>")], |ctx, _| {
            match ctx.get::<Result<DescribeResult, DescribeError>>("desc_result").ok_or("No result")? {
                Ok(DescribeResult::InstanceInfo { .. }) => Ok(()),
                other => Err(format!("Expected InstanceInfo, got {:?}", other)),
            }
        }),
        check_def("the describe type name is {string}", &[("desc_result", "Result<DescribeResult, DescribeError>")], |ctx, params| {
            let expected = params.get_string(0).ok_or("Expected name")?;
            match ctx.get::<Result<DescribeResult, DescribeError>>("desc_result").ok_or("No result")? {
                Ok(DescribeResult::TypeInfo { name, .. }) if name == expected => Ok(()),
                Ok(DescribeResult::TypeInfo { name, .. }) => Err(format!("Expected '{}', got '{}'", expected, name)),
                _ => Err("Not TypeInfo".to_string()),
            }
        }),
        check_def("the describe type has required field {string}", &[("desc_result", "Result<DescribeResult, DescribeError>")], |ctx, params| {
            let field = params.get_string(0).ok_or("Expected field")?;
            match ctx.get::<Result<DescribeResult, DescribeError>>("desc_result").ok_or("No result")? {
                Ok(DescribeResult::TypeInfo { required_fields, .. }) => {
                    if required_fields.iter().any(|f| f == field) { Ok(()) }
                    else { Err(format!("Field '{}' not found in {:?}", field, required_fields)) }
                }
                _ => Err("Not TypeInfo".to_string()),
            }
        }),
        check_def("the describe type parent type is {string}", &[("desc_result", "Result<DescribeResult, DescribeError>")], |ctx, params| {
            let expected = params.get_string(0).ok_or("Expected parent")?;
            match ctx.get::<Result<DescribeResult, DescribeError>>("desc_result").ok_or("No result")? {
                Ok(DescribeResult::TypeInfo { parent_type, .. }) if parent_type == expected => Ok(()),
                Ok(DescribeResult::TypeInfo { parent_type, .. }) => Err(format!("Expected '{}', got '{}'", expected, parent_type)),
                _ => Err("Not TypeInfo".to_string()),
            }
        }),
        check_def("the describe type description contains {string}", &[("desc_result", "Result<DescribeResult, DescribeError>")], |ctx, params| {
            let needle = params.get_string(0).ok_or("Expected needle")?;
            match ctx.get::<Result<DescribeResult, DescribeError>>("desc_result").ok_or("No result")? {
                Ok(DescribeResult::TypeInfo { description, .. }) => {
                    if description.contains(needle.as_ref() as &str) {
                        Ok(())
                    } else {
                        Err(format!("Expected description to contain '{}', got '{}'", needle, description))
                    }
                }
                _ => Err("Not TypeInfo".to_string()),
            }
        }),
        check_def("the describe instance state is {string}", &[("desc_result", "Result<DescribeResult, DescribeError>")], |ctx, params| {
            let expected = params.get_string(0).ok_or("Expected state")?;
            match ctx.get::<Result<DescribeResult, DescribeError>>("desc_result").ok_or("No result")? {
                Ok(DescribeResult::InstanceInfo { state, .. }) if state == expected => Ok(()),
                Ok(DescribeResult::InstanceInfo { state, .. }) => Err(format!("Expected '{}', got '{}'", expected, state)),
                _ => Err("Not InstanceInfo".to_string()),
            }
        }),
        check_def("the describe instance transition count is {int}", &[("desc_result", "Result<DescribeResult, DescribeError>")], |ctx, params| {
            let expected = params.get_int(0).ok_or("Expected count")? as usize;
            match ctx.get::<Result<DescribeResult, DescribeError>>("desc_result").ok_or("No result")? {
                Ok(DescribeResult::InstanceInfo { transition_count, .. }) if *transition_count == expected => Ok(()),
                Ok(DescribeResult::InstanceInfo { transition_count, .. }) => Err(format!("Expected {}, got {}", expected, transition_count)),
                _ => Err("Not InstanceInfo".to_string()),
            }
        }),
        check_def("the describe instance available actions include {string} with role {string}", &[("desc_result", "Result<DescribeResult, DescribeError>")], |ctx, params| {
            let action = params.get_string(0).ok_or("Expected action")?;
            let role = params.get_string(1).ok_or("Expected role")?;
            match ctx.get::<Result<DescribeResult, DescribeError>>("desc_result").ok_or("No result")? {
                Ok(DescribeResult::InstanceInfo { available_actions, .. }) => {
                    if available_actions.iter().any(|a| a.action == *action && a.required_role == *role) { Ok(()) }
                    else { Err(format!("Action '{}' role '{}' not found", action, role)) }
                }
                _ => Err("Not InstanceInfo".to_string()),
            }
        }),
        check_def("the describe result is an UnknownIdentifier error", &[("desc_result", "Result<DescribeResult, DescribeError>")], |ctx, _| {
            match ctx.get::<Result<DescribeResult, DescribeError>>("desc_result").ok_or("No result")? {
                Err(DescribeError::UnknownIdentifier { .. }) => Ok(()),
                other => Err(format!("Expected UnknownIdentifier, got {:?}", other)),
            }
        }),

        // ===== available_artifact_types() steps (Phase 6, R11.3) =====

        // When: available_artifact_types is called
        step_def(
            "available_artifact_types is called",
            &[],
            &[("available_types", "Vec<AvailableArtifactType>")],
            |_ctx, _params| {
                let types = available_artifact_types();
                let mut out = Context::new();
                out.set("available_types", types);
                Ok(out)
            },
        ),

        // Then: N artifact types are returned
        check_def(
            "{int} artifact types are returned",
            &[("available_types", "Vec<AvailableArtifactType>")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let types = ctx
                    .get::<Vec<AvailableArtifactType>>("available_types")
                    .ok_or("No available_types")?;
                if types.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} artifact types, got {}: {:?}",
                        expected,
                        types.len(),
                        types.iter().map(|t| &t.name).collect::<Vec<_>>()
                    ))
                }
            },
        ),

        // Then: the artifact type "X" is present
        check_def(
            "the artifact type {string} is present",
            &[("available_types", "Vec<AvailableArtifactType>")],
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected name")?;
                let types = ctx
                    .get::<Vec<AvailableArtifactType>>("available_types")
                    .ok_or("No available_types")?;
                if types.iter().any(|t| t.name == name.as_ref() as &str) {
                    Ok(())
                } else {
                    Err(format!(
                        "Artifact type '{}' not found in: {:?}",
                        name,
                        types.iter().map(|t| &t.name).collect::<Vec<_>>()
                    ))
                }
            },
        ),

        // Then: the artifact type "X" has requires_parent "Y"
        check_def(
            "the artifact type {string} has requires_parent {string}",
            &[("available_types", "Vec<AvailableArtifactType>")],
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected name")?;
                let expected_parent = params.get_string(1).ok_or("Expected parent")?;
                let types = ctx
                    .get::<Vec<AvailableArtifactType>>("available_types")
                    .ok_or("No available_types")?;
                let found = types.iter().find(|t| t.name == name.as_ref() as &str)
                    .ok_or_else(|| format!("Artifact type '{}' not found", name))?;
                if found.requires_parent == expected_parent.as_ref() as &str {
                    Ok(())
                } else {
                    Err(format!(
                        "Type '{}' requires_parent: expected '{}', got '{}'",
                        name, expected_parent, found.requires_parent
                    ))
                }
            },
        ),

        // Then: the artifact type "X" description contains "Y"
        check_def(
            "the artifact type {string} description contains {string}",
            &[("available_types", "Vec<AvailableArtifactType>")],
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected name")?;
                let needle = params.get_string(1).ok_or("Expected needle")?;
                let types = ctx
                    .get::<Vec<AvailableArtifactType>>("available_types")
                    .ok_or("No available_types")?;
                let found = types.iter().find(|t| t.name == name.as_ref() as &str)
                    .ok_or_else(|| format!("Artifact type '{}' not found", name))?;
                if found.description.contains(needle.as_ref() as &str) {
                    Ok(())
                } else {
                    Err(format!(
                        "Type '{}' description '{}' does not contain '{}'",
                        name, found.description, needle
                    ))
                }
            },
        ),
    ]
}
