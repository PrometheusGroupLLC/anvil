//! Step module for `hearth_playbook_registry_adapter.feature`.
//!
//! Provides steps for:
//! - Creating a scratch hearth directory with synthetic `machine.yaml` content
//! - Constructing a `HearthPlaybookRegistry` from that directory
//! - Calling `machine_for(kind)` and storing the result
//! - Asserting on load errors exposed via `invalid_artifacts()`
//!
//! Steps use `tempfile::TempDir` for scratch hearth directories so features are
//! hermetic — no real hearth is read or written.

use anvil_core::domain::playbook::hearth_registry::HearthPlaybookRegistry;
use anvil_core::domain::playbook::registry::PlaybookRegistry;
use anvil_core::domain::playbook::types::PlaybookMachine;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Context key: the path to the temp hearth directory (kept alive via TempDir arc).
const TEMP_HEARTH_PATH_KEY: &str = "hwr_temp_hearth_path";
/// Context key: the TempDir handle (kept alive so dir isn't deleted mid-test).
const TEMP_HEARTH_HANDLE_KEY: &str = "hwr_temp_hearth_handle";
/// Context key: the load-error count after construction.
const LOAD_ERROR_COUNT_KEY: &str = "hwr_load_error_count";
/// Context key: the resolved machine (Option<PlaybookMachine>).
const RESOLVED_KEY: &str = "hwr_resolved";
/// Context key: whether construction succeeded.
const CONSTRUCTION_OK_KEY: &str = "hwr_construction_ok";

/// Minimal valid `machine.yaml` for kind "track" — mirrors the Phase 1 hearth artifact.
/// Uses only the fields required by the loader with no hook references.
fn minimal_machine_yaml(kind: &str) -> String {
    format!(
        r#"kind: {kind}
directory: {kind}s
registry: {kind}s.md
description: "Minimal {kind} machine for testing."
required_fields: []
roles:
  - doer
  - reviewer
states:
  - name: active
    role_filters: []
    registry_section: active
    projection_targets: []
    is_review_gate: false
    is_terminal: false
  - name: completed
    role_filters: []
    registry_section: completed
    projection_targets: []
    is_review_gate: false
    is_terminal: true
transitions:
  - from_state: active
    to_state: completed
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
"#,
        kind = kind
    )
}

/// An event/step/queue-driven `machine.yaml` (#34) for the given kind: it uses
/// `anvil_kind` + `trigger` + `steps` + per-state `on:` maps and declares NO
/// standard `transitions:` graph. This is the schema that used to fail to parse
/// and land in `invalid_artifacts`.
fn event_driven_machine_yaml(kind: &str) -> String {
    format!(
        r#"name: {kind}
version: "0.1.0"
anvil_kind: {kind}
description: "Event-driven {kind} machine for testing."
trigger:
  kind: pending_queue
  poll_tool: list_pending_{kind}
mcp_tool_dependencies:
  - tool: list_pending_{kind}
    purpose: discover items awaiting playbook execution
    side_effects: read-only
steps:
  - id: list_pending
    kind: mcp_call
    tool: list_pending_{kind}
    description: Find an item awaiting playbook execution.
    next: finish
  - id: finish
    kind: llm_turn
    description: Complete the work.
    terminal: true
states:
  - name: pending
    is_terminal: false
    on:
      PlaybookStarted: completed
  - name: completed
    is_terminal: true
  - name: failed
    terminal: true
"#,
        kind = kind
    )
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ===== Given: create temp hearth with a valid machine.yaml =====
        step_def(
            "a temp hearth with a valid {string} machine.yaml at {string}",
            &[],
            &[
                (TEMP_HEARTH_PATH_KEY, "PathBuf"),
                (TEMP_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let rel_path = params
                    .get_string(1)
                    .ok_or("Expected relative path")?
                    .to_string();

                let temp_dir = tempfile::TempDir::new()
                    .map_err(|e| format!("Failed to create temp dir: {}", e))?;
                let hearth_path = temp_dir.path().to_path_buf();

                // Create the machine.yaml at the specified relative path.
                let machine_yaml_path = hearth_path.join(&rel_path);
                if let Some(parent) = machine_yaml_path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("Failed to create dirs: {}", e))?;
                }
                std::fs::write(&machine_yaml_path, minimal_machine_yaml(&kind))
                    .map_err(|e| format!("Failed to write machine.yaml: {}", e))?;

                let handle: Arc<Mutex<Option<tempfile::TempDir>>> =
                    Arc::new(Mutex::new(Some(temp_dir)));
                let mut out = Context::new();
                out.set(TEMP_HEARTH_PATH_KEY, hearth_path);
                out.set(TEMP_HEARTH_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        // ===== And: add another machine.yaml to the same temp hearth =====
        step_def(
            "a temp hearth has a valid {string} machine.yaml at {string}",
            &[
                (TEMP_HEARTH_PATH_KEY, "PathBuf"),
                (TEMP_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (TEMP_HEARTH_PATH_KEY, "PathBuf"),
                (TEMP_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let rel_path = params
                    .get_string(1)
                    .ok_or("Expected relative path")?
                    .to_string();

                let hearth_path = ctx
                    .get::<PathBuf>(TEMP_HEARTH_PATH_KEY)
                    .ok_or("No temp hearth path")?
                    .clone();
                let handle = ctx
                    .get::<Arc<Mutex<Option<tempfile::TempDir>>>>(TEMP_HEARTH_HANDLE_KEY)
                    .ok_or("No temp hearth handle")?
                    .clone();

                let machine_yaml_path = hearth_path.join(&rel_path);
                if let Some(parent) = machine_yaml_path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("Failed to create dirs: {}", e))?;
                }
                std::fs::write(&machine_yaml_path, minimal_machine_yaml(&kind))
                    .map_err(|e| format!("Failed to write machine.yaml: {}", e))?;

                let mut out = Context::new();
                out.set(TEMP_HEARTH_PATH_KEY, hearth_path);
                out.set(TEMP_HEARTH_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        // ===== Given: create temp hearth with an EVENT-DRIVEN machine.yaml =====
        step_def(
            "a temp hearth with an event-driven {string} machine.yaml at {string}",
            &[],
            &[
                (TEMP_HEARTH_PATH_KEY, "PathBuf"),
                (TEMP_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let rel_path = params
                    .get_string(1)
                    .ok_or("Expected relative path")?
                    .to_string();

                let temp_dir = tempfile::TempDir::new()
                    .map_err(|e| format!("Failed to create temp dir: {}", e))?;
                let hearth_path = temp_dir.path().to_path_buf();

                let machine_yaml_path = hearth_path.join(&rel_path);
                if let Some(parent) = machine_yaml_path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("Failed to create dirs: {}", e))?;
                }
                std::fs::write(&machine_yaml_path, event_driven_machine_yaml(&kind))
                    .map_err(|e| format!("Failed to write machine.yaml: {}", e))?;

                let handle: Arc<Mutex<Option<tempfile::TempDir>>> =
                    Arc::new(Mutex::new(Some(temp_dir)));
                let mut out = Context::new();
                out.set(TEMP_HEARTH_PATH_KEY, hearth_path);
                out.set(TEMP_HEARTH_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        // ===== Given: create temp hearth with a MALFORMED machine.yaml =====
        step_def(
            "a temp hearth with a malformed machine.yaml at {string}",
            &[],
            &[
                (TEMP_HEARTH_PATH_KEY, "PathBuf"),
                (TEMP_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let rel_path = params
                    .get_string(0)
                    .ok_or("Expected relative path")?
                    .to_string();

                let temp_dir = tempfile::TempDir::new()
                    .map_err(|e| format!("Failed to create temp dir: {}", e))?;
                let hearth_path = temp_dir.path().to_path_buf();

                let machine_yaml_path = hearth_path.join(&rel_path);
                if let Some(parent) = machine_yaml_path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("Failed to create dirs: {}", e))?;
                }
                // Write deliberately malformed YAML (missing required fields).
                std::fs::write(&machine_yaml_path, "this: is: not: valid: yaml: [[[")
                    .map_err(|e| format!("Failed to write malformed machine.yaml: {}", e))?;

                let handle: Arc<Mutex<Option<tempfile::TempDir>>> =
                    Arc::new(Mutex::new(Some(temp_dir)));
                let mut out = Context::new();
                out.set(TEMP_HEARTH_PATH_KEY, hearth_path);
                out.set(TEMP_HEARTH_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        // ===== When: construct registry from temp hearth =====
        step_def(
            "the hearth registry is constructed from the temp hearth",
            &[
                (TEMP_HEARTH_PATH_KEY, "PathBuf"),
                (TEMP_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (LOAD_ERROR_COUNT_KEY, "usize"),
                (CONSTRUCTION_OK_KEY, "bool"),
                (TEMP_HEARTH_PATH_KEY, "PathBuf"),
                (TEMP_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, _params| {
                let hearth_path = ctx
                    .get::<PathBuf>(TEMP_HEARTH_PATH_KEY)
                    .ok_or("No temp hearth path")?
                    .clone();
                let handle = ctx
                    .get::<Arc<Mutex<Option<tempfile::TempDir>>>>(TEMP_HEARTH_HANDLE_KEY)
                    .ok_or("No temp hearth handle")?
                    .clone();

                let registry = HearthPlaybookRegistry::new(hearth_path.clone());
                let error_count = registry.invalid_artifacts().len();
                // Construction always succeeds — store outcome.
                let mut out = Context::new();
                out.set(LOAD_ERROR_COUNT_KEY, error_count);
                out.set(CONSTRUCTION_OK_KEY, true);
                out.set(TEMP_HEARTH_PATH_KEY, hearth_path);
                out.set(TEMP_HEARTH_HANDLE_KEY, handle);
                // We can't store the registry itself directly (not Clone + arbitrary lifetime),
                // so we need to re-construct it when machine_for is called.
                // Store only the metadata here; the registry is re-constructed in machine_for steps.
                Ok(out)
            },
        ),
        // ===== When: resolve kind via hearth registry =====
        step_def(
            "the hearth registry resolves kind {string}",
            &[
                (TEMP_HEARTH_PATH_KEY, "PathBuf"),
                (TEMP_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (RESOLVED_KEY, "Option<PlaybookMachine>"),
                (TEMP_HEARTH_PATH_KEY, "PathBuf"),
                (TEMP_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let hearth_path = ctx
                    .get::<PathBuf>(TEMP_HEARTH_PATH_KEY)
                    .ok_or("No temp hearth path")?
                    .clone();
                let handle = ctx
                    .get::<Arc<Mutex<Option<tempfile::TempDir>>>>(TEMP_HEARTH_HANDLE_KEY)
                    .ok_or("No temp hearth handle")?
                    .clone();

                let registry = HearthPlaybookRegistry::new(hearth_path.clone());
                // Clone the machine so it can be stored in context.
                let resolved: Option<PlaybookMachine> = registry.machine_for(&kind).cloned();

                let mut out = Context::new();
                out.set(RESOLVED_KEY, resolved);
                out.set(TEMP_HEARTH_PATH_KEY, hearth_path);
                out.set(TEMP_HEARTH_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        // ===== Then: resolve playbook id via hearth registry =====
        check_def(
            "the hearth registry resolves playbook id {string} for kind {string}",
            &[
                (TEMP_HEARTH_PATH_KEY, "PathBuf"),
                (TEMP_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let expected_id = params.get_string(0).ok_or("Expected playbook id")?;
                let kind = params.get_string(1).ok_or("Expected kind")?.to_string();
                let hearth_path = ctx
                    .get::<PathBuf>(TEMP_HEARTH_PATH_KEY)
                    .ok_or("No temp hearth path")?
                    .clone();

                let registry = HearthPlaybookRegistry::new(hearth_path);
                match registry.playbook_id_for(&kind) {
                    Some(id) if id == expected_id.as_ref() as &str => Ok(()),
                    Some(id) => Err(format!(
                        "Expected playbook id '{}' but got '{}'",
                        expected_id, id
                    )),
                    None => Err(format!(
                        "Expected playbook id '{}' for kind '{}' but got None",
                        expected_id, kind
                    )),
                }
            },
        ),
        // ===== Then: construction succeeded =====
        check_def(
            "the hearth registry construction succeeds",
            &[(CONSTRUCTION_OK_KEY, "bool")],
            |ctx, _params| {
                let ok = ctx
                    .get::<bool>(CONSTRUCTION_OK_KEY)
                    .ok_or("No construction_ok key")?;
                if *ok {
                    Ok(())
                } else {
                    Err("Expected registry construction to succeed but it failed".to_string())
                }
            },
        ),
        // ===== Then: registry has load errors =====
        check_def(
            "the hearth registry has load errors",
            &[(LOAD_ERROR_COUNT_KEY, "usize")],
            |ctx, _params| {
                let count = ctx
                    .get::<usize>(LOAD_ERROR_COUNT_KEY)
                    .ok_or("No load_error_count key")?;
                if *count > 0 {
                    Ok(())
                } else {
                    Err("Expected load errors but found none".to_string())
                }
            },
        ),
        // ===== Then: registry has NO load errors =====
        check_def(
            "the hearth registry has no load errors",
            &[(LOAD_ERROR_COUNT_KEY, "usize")],
            |ctx, _params| {
                let count = ctx
                    .get::<usize>(LOAD_ERROR_COUNT_KEY)
                    .ok_or("No load_error_count key")?;
                if *count == 0 {
                    Ok(())
                } else {
                    Err(format!("Expected no load errors but found {}", count))
                }
            },
        ),
        // ===== Then: hearth-resolved machine has kind =====
        check_def(
            "the hearth-resolved machine has kind {string}",
            &[(RESOLVED_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let expected_kind = params.get_string(0).ok_or("Expected kind")?;
                let resolved = ctx
                    .get::<Option<PlaybookMachine>>(RESOLVED_KEY)
                    .ok_or("No resolved machine")?;
                match resolved.as_ref() {
                    Some(m) => {
                        if m.kind != expected_kind {
                            Err(format!(
                                "Expected machine with kind '{}' but got '{}'",
                                expected_kind, m.kind
                            ))
                        } else {
                            Ok(())
                        }
                    }
                    None => Err(format!(
                        "Expected machine with kind '{}' but got None",
                        expected_kind
                    )),
                }
            },
        ),
        // ===== Then: hearth-resolved machine is absent =====
        check_def(
            "the hearth-resolved machine is absent",
            &[(RESOLVED_KEY, "Option<PlaybookMachine>")],
            |ctx, _params| {
                let resolved = ctx
                    .get::<Option<PlaybookMachine>>(RESOLVED_KEY)
                    .ok_or("No resolved machine key")?;
                if resolved.is_some() {
                    Err(format!(
                        "Expected None but got Some(machine with kind '{}')",
                        resolved.as_ref().unwrap().kind
                    ))
                } else {
                    Ok(())
                }
            },
        ),
    ]
}
