//! Step module for Phase 5 feature files:
//!   - playbook_driven_describe_track.feature
//!   - playbook_driven_describe_proposal_unchanged.feature
//!   - describe_track_kind_boundary.feature
//!   - describe_fallback_kinds_unchanged.feature
//!   - describe_via_registry_handler.feature (Phase 5 production-consumer feature)
//!
//! Provides steps for:
//!   - Calling `describe::available_actions(registry, kind, state)` directly
//!   - Calling `snapshot::registry_section_for(kind, state)`
//!   - Calling `snapshot::projection_targets_for(kind, state, false, "")`
//!   - Calling `routing::compute_execution_route(SUBJECT_AVAILABLE_ACTION, kind, state, role)`
//!   - Hearth-based mutation-propagates scenario (scratch hearth with edited machine.yaml)
//!   - `DescribeQueryHandler::execute` with a HearthPlaybookRegistry (production-consumer)

use anvil_core::domain::describe::{
    available_actions, AvailableAction, DescribeQueryHandler, DescribeRequest, DescribeResult,
};
use anvil_core::domain::playbook::hearth_registry::HearthPlaybookRegistry;
use anvil_core::domain::playbook::registry::{PlaybookRegistry, SeedPlaybookRegistry};
use anvil_core::domain::playbook::types::{PlaybookMachine, StateDefinition, TransitionDefinition};
use anvil_core::domain::routing::{
    compute_execution_route, SUBJECT_AVAILABLE_ACTION, SUBJECT_FILTERED_ARTIFACT,
};
use anvil_core::domain::snapshot::{projection_targets_for, registry_section_for};
use anvil_core_hearth::test_describe_adapter::TestDescribeAdapter;
use anvil_core::ports::describe_port::InstanceState;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Context key for the available_actions result.
const ACTIONS_KEY: &str = "daa_actions";
/// Context key for registry_section_for result.
const REGISTRY_SECTION_KEY: &str = "daa_registry_section";
/// Context key for projection_targets_for result.
const PROJ_TARGETS_KEY: &str = "daa_proj_targets";
/// Context key for compute_execution_route result.
const SUPPORTED_PLAYBOOK_KEY: &str = "daa_execution_route";
/// Context key: path to scratch hearth for mutation-propagates / handler tests.
const SCRATCH_HEARTH_PATH_KEY: &str = "daa_scratch_hearth_path";
/// Context key: TempDir handle for scratch hearth (kept alive so dir persists).
const SCRATCH_HEARTH_HANDLE_KEY: &str = "daa_scratch_hearth_handle";
/// Context key: the DescribeQueryHandler result (DescribeResult).
const HANDLER_RESULT_KEY: &str = "daa_handler_result";
/// Context key: artifact id used by handler scenarios.
const ARTIFACT_ID_KEY: &str = "daa_artifact_id";
/// Context key: whether the registry for handler tests is empty.
const EMPTY_REGISTRY_KEY: &str = "daa_empty_registry";

pub fn steps() -> Vec<StepDef> {
    vec![
        // ===== When: call available_actions(registry, kind, state) via SeedPlaybookRegistry =====
        // The existing scenarios in playbook_driven_describe_track.feature,
        // playbook_driven_describe_proposal_unchanged.feature, etc. use this step.
        // The step now internally constructs a SeedPlaybookRegistry and passes it through
        // so the call site exercises the new 3-arg signature without changing scenario text.
        step_def(
            "available_actions is called with kind {string} and state {string}",
            &[],
            &[(ACTIONS_KEY, "Vec<AvailableAction>")],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let registry = SeedPlaybookRegistry;
                let actions = available_actions(&registry, &kind, &state);
                let mut out = Context::new();
                out.set(ACTIONS_KEY, actions);
                Ok(out)
            },
        ),
        // ===== Then: actions list has N entries =====
        check_def(
            "the available actions list has {int} entries",
            &[(ACTIONS_KEY, "Vec<AvailableAction>")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let actions = ctx
                    .get::<Vec<AvailableAction>>(ACTIONS_KEY)
                    .ok_or("No actions result")?;
                if actions.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} actions but got {}: {:?}",
                        expected,
                        actions.len(),
                        actions
                            .iter()
                            .map(|a| format!("{} ({})", a.action, a.required_role))
                            .collect::<Vec<_>>()
                    ))
                }
            },
        ),
        // ===== Then: actions list includes action with role =====
        check_def(
            "the available actions list includes action {string} with role {string}",
            &[(ACTIONS_KEY, "Vec<AvailableAction>")],
            |ctx, params| {
                let expected_action = params.get_string(0).ok_or("Expected action")?;
                let expected_role = params.get_string(1).ok_or("Expected role")?;
                let actions = ctx
                    .get::<Vec<AvailableAction>>(ACTIONS_KEY)
                    .ok_or("No actions result")?;
                if actions.iter().any(|a| {
                    a.action == expected_action.as_ref() as &str
                        && a.required_role == expected_role.as_ref() as &str
                }) {
                    Ok(())
                } else {
                    Err(format!(
                        "Action '{}' role '{}' not found in: {:?}",
                        expected_action,
                        expected_role,
                        actions
                            .iter()
                            .map(|a| format!("{} ({})", a.action, a.required_role))
                            .collect::<Vec<_>>()
                    ))
                }
            },
        ),
        // ===== Then: actions list is empty =====
        check_def(
            "the available actions list is empty",
            &[(ACTIONS_KEY, "Vec<AvailableAction>")],
            |ctx, _params| {
                let actions = ctx
                    .get::<Vec<AvailableAction>>(ACTIONS_KEY)
                    .ok_or("No actions result")?;
                if actions.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected empty actions but got {}: {:?}",
                        actions.len(),
                        actions
                            .iter()
                            .map(|a| format!("{} ({})", a.action, a.required_role))
                            .collect::<Vec<_>>()
                    ))
                }
            },
        ),
        // ===== Given: track seed override adds synthetic state with no outgoing transitions =====
        // Used by the (g3) no-exit state scenario which still uses the seed override path.
        step_def(
            "the track seed override adds a synthetic state with no outgoing transitions",
            &[],
            &[],
            |_ctx, _params| {
                // Build a modified seed: add a synthetic state, no transitions for it
                let original = anvil_core::domain::playbook::seeds::track_seed().clone();
                let mut modified = original;
                modified.states.push(StateDefinition {
                    name: "test_holding_state".to_string(),
                    role_filters: vec![],
                    registry_section: String::new(),
                    projection_targets: vec![],
                    is_review_gate: false,
                    is_terminal: false,
                    hook: None,
                    hooks_by_role: std::collections::BTreeMap::new(),
                    measurement_by_role: std::collections::BTreeMap::new(),
                });
                // No transitions added for test_holding_state — zero outgoing by design
                let leaked: &'static PlaybookMachine = Box::leak(Box::new(modified));
                anvil_core::domain::playbook::seeds::set_track_seed_override(leaked);
                Ok(Context::new())
            },
        ),
        // ===== Then/And: track seed override is cleared (teardown) =====
        check_def("the track seed override is cleared", &[], |_ctx, _params| {
            anvil_core::domain::playbook::seeds::clear_track_seed_override();
            Ok(())
        }),
        // ===== Given: scratch hearth with the modified track machine.yaml (mutation-propagates) =====
        // Builds a temp hearth with a track machine.yaml that omits the
        // spec_review→spec_revision transition. This drives the mutation through
        // HearthPlaybookRegistry rather than the thread-local seed override.
        step_def(
            "a hearth with a modified track machine.yaml that omits the spec_review to spec_revision transition",
            &[],
            &[
                (SCRATCH_HEARTH_PATH_KEY, "PathBuf"),
                (SCRATCH_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, _params| {
                let temp_dir = tempfile::TempDir::new()
                    .map_err(|e| format!("Failed to create temp dir: {}", e))?;
                let hearth_path = temp_dir.path().to_path_buf();

                // Build machine.yaml from the compiled-in track seed, removing the
                // spec_review→spec_revision transition. This is the mutation.
                let original = anvil_core::domain::playbook::seeds::track_seed().clone();
                let mut modified = original;
                modified.transitions.retain(|t| {
                    !(t.from_state == "spec_review" && t.to_state == "spec_revision")
                });
                // Removing the only inbound edge orphans `spec_revision`. Drop
                // the orphaned state (and its outbound edge) too so the mutated
                // machine stays contiguous and registers — the registration-time
                // contiguity gate rejects unreachable non-terminal states.
                modified
                    .transitions
                    .retain(|t| t.from_state != "spec_revision" && t.to_state != "spec_revision");
                modified.states.retain(|s| s.name != "spec_revision");

                // Serialize the modified machine to YAML and write it to the scratch hearth.
                let yaml = machine_to_yaml(&modified);
                let playbook_dir = hearth_path.join("playbooks").join("20260422T0000_track_lifecycle");
                std::fs::create_dir_all(&playbook_dir)
                    .map_err(|e| format!("Failed to create playbook dir: {}", e))?;
                std::fs::write(playbook_dir.join("machine.yaml"), &yaml)
                    .map_err(|e| format!("Failed to write machine.yaml: {}", e))?;

                let handle: Arc<Mutex<Option<tempfile::TempDir>>> =
                    Arc::new(Mutex::new(Some(temp_dir)));
                let mut out = Context::new();
                out.set(SCRATCH_HEARTH_PATH_KEY, hearth_path);
                out.set(SCRATCH_HEARTH_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        // ===== When: call available_actions via registry from scratch hearth =====
        step_def(
            "available_actions is called with registry from that hearth, kind {string}, and state {string}",
            &[
                (SCRATCH_HEARTH_PATH_KEY, "PathBuf"),
                (SCRATCH_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (ACTIONS_KEY, "Vec<AvailableAction>"),
                (SCRATCH_HEARTH_PATH_KEY, "PathBuf"),
                (SCRATCH_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let hearth_path = ctx
                    .get::<PathBuf>(SCRATCH_HEARTH_PATH_KEY)
                    .ok_or("No scratch hearth path")?
                    .clone();
                let handle = ctx
                    .get::<Arc<Mutex<Option<tempfile::TempDir>>>>(SCRATCH_HEARTH_HANDLE_KEY)
                    .ok_or("No scratch hearth handle")?
                    .clone();

                let registry = HearthPlaybookRegistry::new(hearth_path.clone());
                let actions = available_actions(&registry, &kind, &state);

                let mut out = Context::new();
                out.set(ACTIONS_KEY, actions);
                out.set(SCRATCH_HEARTH_PATH_KEY, hearth_path);
                out.set(SCRATCH_HEARTH_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        // ===== When: call registry_section_for(kind, state) =====
        step_def(
            "registry_section_for is called with kind {string} and state {string}",
            &[],
            &[(REGISTRY_SECTION_KEY, "String")],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let result = registry_section_for(&kind, &state);
                let mut out = Context::new();
                out.set(
                    REGISTRY_SECTION_KEY,
                    result.unwrap_or("None").to_string(),
                );
                Ok(out)
            },
        ),
        // ===== Then: registry section is =====
        check_def(
            "the registry section is {string}",
            &[(REGISTRY_SECTION_KEY, "String")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected section")?;
                let actual = ctx
                    .get::<String>(REGISTRY_SECTION_KEY)
                    .ok_or("No registry section result")?;
                if actual == expected.as_ref() as &str {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected registry section '{}' but got '{}'",
                        expected, actual
                    ))
                }
            },
        ),
        // ===== When: call projection_targets_for(kind, state) =====
        step_def(
            "projection_targets_for is called with kind {string} and state {string}",
            &[],
            &[(PROJ_TARGETS_KEY, "String")],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                // projection_only=false, event_type="" — non-spark normal transition
                let targets = projection_targets_for(&kind, &state, false, "", false);
                let targets_str: String = targets
                    .iter()
                    .map(|t| format!("{:?}", t))
                    .collect::<Vec<_>>()
                    .join(",");
                let mut out = Context::new();
                out.set(PROJ_TARGETS_KEY, targets_str);
                Ok(out)
            },
        ),
        // ===== Then: projection targets is =====
        check_def(
            "the projection targets is {string}",
            &[(PROJ_TARGETS_KEY, "String")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected targets")?;
                let actual = ctx
                    .get::<String>(PROJ_TARGETS_KEY)
                    .ok_or("No projection targets result")?;
                if actual == expected.as_ref() as &str {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected projection targets '{}' but got '{}'",
                        expected, actual
                    ))
                }
            },
        ),
        // ===== When: call compute_execution_route for available_action =====
        step_def(
            "compute_execution_route is called for available_action kind {string} state {string} role {string}",
            &[],
            &[(SUPPORTED_PLAYBOOK_KEY, "String")],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let role = params.get_string(2).ok_or("Expected role")?.to_string();
                let result = compute_execution_route(SUBJECT_AVAILABLE_ACTION, &kind, &state, &role);
                let mut out = Context::new();
                out.set(SUPPORTED_PLAYBOOK_KEY, result);
                Ok(out)
            },
        ),
        // ===== When: call compute_execution_route for filtered_artifact =====
        step_def(
            "compute_execution_route is called for filtered_artifact kind {string} state {string} role {string}",
            &[],
            &[(SUPPORTED_PLAYBOOK_KEY, "String")],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let role = params.get_string(2).ok_or("Expected role")?.to_string();
                let result =
                    compute_execution_route(SUBJECT_FILTERED_ARTIFACT, &kind, &state, &role);
                let mut out = Context::new();
                out.set(SUPPORTED_PLAYBOOK_KEY, result);
                Ok(out)
            },
        ),
        // ===== Then: supported playbook result is =====
        check_def(
            "the supported playbook result is {string}",
            &[(SUPPORTED_PLAYBOOK_KEY, "String")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected result")?;
                let actual = ctx
                    .get::<String>(SUPPORTED_PLAYBOOK_KEY)
                    .ok_or("No execution_route result")?;
                if actual == expected.as_ref() as &str {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected execution_route '{}' but got '{}'",
                        expected, actual
                    ))
                }
            },
        ),

        // ─── describe_via_registry_handler.feature steps ────────────────────────

        // ===== Given: scratch hearth with the track machine.yaml loaded =====
        // Creates a temp hearth containing the compiled-in track machine.yaml
        // (serialized from the seed). Used by DescribeQueryHandler scenarios.
        step_def(
            "a scratch hearth with the track machine.yaml loaded",
            &[],
            &[
                (SCRATCH_HEARTH_PATH_KEY, "PathBuf"),
                (SCRATCH_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, _params| {
                let temp_dir = tempfile::TempDir::new()
                    .map_err(|e| format!("Failed to create temp dir: {}", e))?;
                let hearth_path = temp_dir.path().to_path_buf();

                let machine = anvil_core::domain::playbook::seeds::track_seed().clone();
                let yaml = machine_to_yaml(&machine);
                let playbook_dir =
                    hearth_path.join("playbooks").join("20260422T0000_track_lifecycle");
                std::fs::create_dir_all(&playbook_dir)
                    .map_err(|e| format!("Failed to create playbook dir: {}", e))?;
                std::fs::write(playbook_dir.join("machine.yaml"), &yaml)
                    .map_err(|e| format!("Failed to write machine.yaml: {}", e))?;

                let handle: Arc<Mutex<Option<tempfile::TempDir>>> =
                    Arc::new(Mutex::new(Some(temp_dir)));
                let mut out = Context::new();
                out.set(SCRATCH_HEARTH_PATH_KEY, hearth_path);
                out.set(SCRATCH_HEARTH_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        // ===== And: HearthPlaybookRegistry pointed at that scratch hearth =====
        // Marks the context so the "When DescribeQueryHandler execute..." step
        // constructs a HearthPlaybookRegistry from the scratch hearth.
        step_def(
            "a HearthPlaybookRegistry pointed at that scratch hearth",
            &[
                (SCRATCH_HEARTH_PATH_KEY, "PathBuf"),
                (SCRATCH_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (SCRATCH_HEARTH_PATH_KEY, "PathBuf"),
                (SCRATCH_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (EMPTY_REGISTRY_KEY, "bool"),
            ],
            |ctx, _params| {
                let hearth_path = ctx
                    .get::<PathBuf>(SCRATCH_HEARTH_PATH_KEY)
                    .ok_or("No scratch hearth path")?
                    .clone();
                let handle = ctx
                    .get::<Arc<Mutex<Option<tempfile::TempDir>>>>(SCRATCH_HEARTH_HANDLE_KEY)
                    .ok_or("No scratch hearth handle")?
                    .clone();
                let mut out = Context::new();
                out.set(SCRATCH_HEARTH_PATH_KEY, hearth_path);
                out.set(SCRATCH_HEARTH_HANDLE_KEY, handle);
                out.set(EMPTY_REGISTRY_KEY, false);
                Ok(out)
            },
        ),
        // ===== And: empty HearthPlaybookRegistry with no playbook artifacts =====
        step_def(
            "an empty HearthPlaybookRegistry with no playbook artifacts",
            &[],
            &[
                (EMPTY_REGISTRY_KEY, "bool"),
                (SCRATCH_HEARTH_PATH_KEY, "PathBuf"),
                (SCRATCH_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, _params| {
                // Create a temp hearth with NO playbooks directory.
                let temp_dir = tempfile::TempDir::new()
                    .map_err(|e| format!("Failed to create temp dir: {}", e))?;
                let hearth_path = temp_dir.path().to_path_buf();
                let handle: Arc<Mutex<Option<tempfile::TempDir>>> =
                    Arc::new(Mutex::new(Some(temp_dir)));
                let mut out = Context::new();
                out.set(EMPTY_REGISTRY_KEY, true);
                out.set(SCRATCH_HEARTH_PATH_KEY, hearth_path);
                out.set(SCRATCH_HEARTH_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        // ===== And: a track artifact in state {string} in the scratch hearth =====
        // Stores a synthetic InstanceState in the context so the When step can
        // construct a TestDescribeAdapter with it. Also stores the artifact ID.
        step_def(
            "a track artifact in state {string} in the scratch hearth",
            &[
                (SCRATCH_HEARTH_PATH_KEY, "PathBuf"),
                (SCRATCH_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (SCRATCH_HEARTH_PATH_KEY, "PathBuf"),
                (SCRATCH_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (ARTIFACT_ID_KEY, "String"),
                ("daa_artifact_state", "String"),
            ],
            |ctx, params| {
                let state = params.get_string(0).ok_or("Expected state")?.to_string();
                let hearth_path = ctx
                    .get::<PathBuf>(SCRATCH_HEARTH_PATH_KEY)
                    .ok_or("No scratch hearth path")?
                    .clone();
                let handle = ctx
                    .get::<Arc<Mutex<Option<tempfile::TempDir>>>>(SCRATCH_HEARTH_HANDLE_KEY)
                    .ok_or("No scratch hearth handle")?
                    .clone();
                // Create a minimal status.yaml so the TestDescribeAdapter can serve the artifact.
                let artifact_id = "test_artifact_001".to_string();
                // Write state to a context key so the When step can build the adapter.
                // We store the artifact_id; the state is embedded in the adapter at When time.
                let mut out = Context::new();
                out.set(SCRATCH_HEARTH_PATH_KEY, hearth_path);
                out.set(SCRATCH_HEARTH_HANDLE_KEY, handle);
                // Store state string under a combined key for the When step.
                out.set("daa_artifact_state", state);
                out.set(ARTIFACT_ID_KEY, artifact_id);
                Ok(out)
            },
        ),
        // ===== And: a track artifact in state {string} in a scratch hearth (empty registry variant) =====
        step_def(
            "a track artifact in state {string} in a scratch hearth",
            &[
                (EMPTY_REGISTRY_KEY, "bool"),
                (SCRATCH_HEARTH_PATH_KEY, "PathBuf"),
                (SCRATCH_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (EMPTY_REGISTRY_KEY, "bool"),
                (SCRATCH_HEARTH_PATH_KEY, "PathBuf"),
                (SCRATCH_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (ARTIFACT_ID_KEY, "String"),
                ("daa_artifact_state", "String"),
            ],
            |ctx, params| {
                let state = params.get_string(0).ok_or("Expected state")?.to_string();
                let hearth_path = ctx
                    .get::<PathBuf>(SCRATCH_HEARTH_PATH_KEY)
                    .ok_or("No scratch hearth path")?
                    .clone();
                let handle = ctx
                    .get::<Arc<Mutex<Option<tempfile::TempDir>>>>(SCRATCH_HEARTH_HANDLE_KEY)
                    .ok_or("No scratch hearth handle")?
                    .clone();
                let empty_registry = *ctx.get::<bool>(EMPTY_REGISTRY_KEY).ok_or("No empty_registry key")?;
                let artifact_id = "test_artifact_001".to_string();
                let mut out = Context::new();
                out.set(EMPTY_REGISTRY_KEY, empty_registry);
                out.set(SCRATCH_HEARTH_PATH_KEY, hearth_path);
                out.set(SCRATCH_HEARTH_HANDLE_KEY, handle);
                out.set("daa_artifact_state", state);
                out.set(ARTIFACT_ID_KEY, artifact_id);
                Ok(out)
            },
        ),
        // ===== When: DescribeQueryHandler execute is called with that registry and artifact =====
        step_def(
            "DescribeQueryHandler execute is called with that registry and artifact",
            &[
                (SCRATCH_HEARTH_PATH_KEY, "PathBuf"),
                (SCRATCH_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (ARTIFACT_ID_KEY, "String"),
                ("daa_artifact_state", "String"),
            ],
            &[
                (HANDLER_RESULT_KEY, "DescribeResult"),
                (SCRATCH_HEARTH_PATH_KEY, "PathBuf"),
                (SCRATCH_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, _params| {
                let hearth_path = ctx
                    .get::<PathBuf>(SCRATCH_HEARTH_PATH_KEY)
                    .ok_or("No scratch hearth path")?
                    .clone();
                let handle = ctx
                    .get::<Arc<Mutex<Option<tempfile::TempDir>>>>(SCRATCH_HEARTH_HANDLE_KEY)
                    .ok_or("No scratch hearth handle")?
                    .clone();
                let artifact_id = ctx
                    .get::<String>(ARTIFACT_ID_KEY)
                    .ok_or("No artifact id")?
                    .clone();
                let state = ctx
                    .get::<String>("daa_artifact_state")
                    .ok_or("No artifact state")?
                    .clone();
                let empty_registry = ctx
                    .get::<bool>(EMPTY_REGISTRY_KEY)
                    .copied()
                    .unwrap_or(false);

                // Build a TestDescribeAdapter with one synthetic artifact.
                let instance = InstanceState {
                    kind: "track".to_string(),
                    state: state.clone(),
                    transition_count: 1,
                    last_transition: None,
                };
                let mut instances = std::collections::HashMap::new();
                instances.insert(artifact_id.clone(), instance);
                let adapter = TestDescribeAdapter::new(instances);

                // Build the registry from the scratch hearth (or empty registry).
                let result = if empty_registry {
                    let empty_hearth = HearthPlaybookRegistry::new(hearth_path.clone());
                    DescribeQueryHandler::execute(
                        &adapter,
                        &empty_hearth,
                        DescribeRequest { identifier: artifact_id },
                    )
                } else {
                    let hearth_registry = HearthPlaybookRegistry::new(hearth_path.clone());
                    DescribeQueryHandler::execute(
                        &adapter,
                        &hearth_registry,
                        DescribeRequest { identifier: artifact_id },
                    )
                };

                let describe_result = result.map_err(|e| format!("DescribeQueryHandler failed: {}", e))?;

                let mut out = Context::new();
                out.set(HANDLER_RESULT_KEY, describe_result);
                out.set(SCRATCH_HEARTH_PATH_KEY, hearth_path);
                out.set(SCRATCH_HEARTH_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        // ===== Then: describe result contains available_actions with action and role =====
        check_def(
            "the describe result contains available_actions with action {string} and role {string}",
            &[(HANDLER_RESULT_KEY, "DescribeResult")],
            |ctx, params| {
                let expected_action = params.get_string(0).ok_or("Expected action")?;
                let expected_role = params.get_string(1).ok_or("Expected role")?;
                let result = ctx
                    .get::<DescribeResult>(HANDLER_RESULT_KEY)
                    .ok_or("No handler result")?;
                match result {
                    DescribeResult::InstanceInfo { available_actions, .. } => {
                        if available_actions.iter().any(|a| {
                            a.action == expected_action.as_ref() as &str
                                && a.required_role == expected_role.as_ref() as &str
                        }) {
                            Ok(())
                        } else {
                            Err(format!(
                                "Action '{}' role '{}' not found in: {:?}",
                                expected_action,
                                expected_role,
                                available_actions
                                    .iter()
                                    .map(|a| format!("{} ({})", a.action, a.required_role))
                                    .collect::<Vec<_>>()
                            ))
                        }
                    }
                    other => Err(format!("Expected InstanceInfo but got {:?}", other)),
                }
            },
        ),
        // ===== Then: describe result available_actions list is empty =====
        check_def(
            "the describe result available_actions list is empty",
            &[(HANDLER_RESULT_KEY, "DescribeResult")],
            |ctx, _params| {
                let result = ctx
                    .get::<DescribeResult>(HANDLER_RESULT_KEY)
                    .ok_or("No handler result")?;
                match result {
                    DescribeResult::InstanceInfo { available_actions, .. } => {
                        if available_actions.is_empty() {
                            Ok(())
                        } else {
                            Err(format!(
                                "Expected empty available_actions but got {}: {:?}",
                                available_actions.len(),
                                available_actions
                                    .iter()
                                    .map(|a| format!("{} ({})", a.action, a.required_role))
                                    .collect::<Vec<_>>()
                            ))
                        }
                    }
                    other => Err(format!("Expected InstanceInfo but got {:?}", other)),
                }
            },
        ),
    ]
}

// ─── Helper: serialize PlaybookMachine to YAML for scratch hearths ─────────────

/// Serialize a `PlaybookMachine` to a YAML string that the loader can parse back.
///
/// This is used by scratch-hearth steps to construct `machine.yaml` content from
/// a `PlaybookMachine` value (e.g., the compiled-in seed, possibly modified for
/// mutation-propagates scenarios). The output format matches the schema that
/// `loader::load_from_yaml` accepts.
fn machine_to_yaml(machine: &PlaybookMachine) -> String {
    let mut yaml = String::new();
    yaml.push_str(&format!("kind: {}\n", machine.kind));
    yaml.push_str(&format!("directory: {}\n", machine.directory));
    yaml.push_str(&format!("registry: {}\n", machine.registry));
    yaml.push_str(&format!(
        "description: \"{}\"\n",
        machine.description.replace('"', "\\\"")
    ));
    // required_fields (Vec<FieldDescriptor> — serialize each as a YAML mapping)
    if machine.required_fields.is_empty() {
        yaml.push_str("required_fields: []\n");
    } else {
        yaml.push_str("required_fields:\n");
        for f in &machine.required_fields {
            yaml.push_str(&format!(
                "  - name: {}\n    field_type: {}\n    description: \"{}\"\n",
                f.name,
                f.field_type,
                f.description.replace('"', "\\\"")
            ));
        }
    }
    // roles
    if machine.roles.is_empty() {
        yaml.push_str("roles: []\n");
    } else {
        yaml.push_str("roles:\n");
        for r in &machine.roles {
            yaml.push_str(&format!("  - {}\n", r));
        }
    }
    // states
    yaml.push_str("states:\n");
    for s in &machine.states {
        yaml.push_str(&format!("  - name: {}\n", s.name));
        yaml.push_str("    role_filters: []\n");
        yaml.push_str(&format!("    registry_section: {}\n", s.registry_section));
        yaml.push_str("    projection_targets: []\n");
        yaml.push_str(&format!("    is_review_gate: {}\n", s.is_review_gate));
        yaml.push_str(&format!("    is_terminal: {}\n", s.is_terminal));
    }
    // transitions
    yaml.push_str("transitions:\n");
    for t in &machine.transitions {
        yaml.push_str(&format!("  - from_state: {}\n", t.from_state));
        yaml.push_str(&format!("    to_state: {}\n", t.to_state));
        yaml.push_str(&format!("    required_role: {}\n", t.required_role));
        // Emit the ACTUAL satisfaction tokens (not a hardcoded `~`) so a machine
        // whose review gates carry `is_review_gate: true` re-loads valid — the
        // loader rejects an is_review_gate exit lacking a required_satisfaction.
        match &t.required_satisfaction {
            Some(tokens) if !tokens.is_empty() => {
                let list = tokens
                    .iter()
                    .map(|s| format!("\"{}\"", s))
                    .collect::<Vec<_>>()
                    .join(", ");
                yaml.push_str(&format!("    required_satisfaction: [{}]\n", list));
            }
            _ => yaml.push_str("    required_satisfaction: ~\n"),
        }
        yaml.push_str(&format!("    requires_approver: {}\n", t.requires_approver));
    }
    yaml
}
