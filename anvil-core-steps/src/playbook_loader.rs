//! Step module for playbook loader feature files.
//!
//! Provides steps for seeding YAML content + hook file lists, calling the
//! playbook loader, and asserting on the result (success with field values,
//! or failure with error code + params).

use anvil_core::domain::playbook::load_error::PlaybookLoadError;
use anvil_core::domain::playbook::loader::load_from_yaml;
use anvil_core::domain::playbook::types::{PlaybookMachine, Role, Sensitivity};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

/// Key used for the YAML text stored in context.
const YAML_KEY: &str = "wl_yaml_text";
/// Key used for the artifact id stored in context.
const ARTIFACT_ID_KEY: &str = "wl_artifact_id";
/// Key used for hook filenames stored in context.
const HOOK_FILES_KEY: &str = "wl_hook_files";
/// Key used for the parsed PlaybookMachine stored in context.
const MACHINE_KEY: &str = "wl_machine";
/// Key used for the PlaybookLoadError stored in context.
const ERROR_KEY: &str = "wl_error";

/// Parse a Gherkin list literal like `["a", "b", "c"]` into a Vec<String>.
/// Returns an empty vec for `[]`.
fn parse_string_list(s: &str) -> Result<Vec<String>, String> {
    let s = s.trim();
    if s == "[]" {
        return Ok(vec![]);
    }
    if !s.starts_with('[') || !s.ends_with(']') {
        return Err(format!(
            "Expected a list literal like [\"a\", \"b\"], got: {}",
            s
        ));
    }
    let inner = &s[1..s.len() - 1];
    let items = inner
        .split(',')
        .map(|item| {
            let t = item.trim();
            // Strip surrounding quotes.
            if (t.starts_with('"') && t.ends_with('"'))
                || (t.starts_with('\'') && t.ends_with('\''))
            {
                t[1..t.len() - 1].to_string()
            } else {
                t.to_string()
            }
        })
        .filter(|s| !s.is_empty())
        .collect();
    Ok(items)
}

fn parse_role(s: &str) -> Result<Role, String> {
    match s {
        "read" => Ok(Role::Read),
        "write" => Ok(Role::Write),
        "admin" => Ok(Role::Admin),
        _ => Err(format!("Unknown role '{}'", s)),
    }
}

fn parse_sensitivity(s: &str) -> Result<Sensitivity, String> {
    match s {
        "public" => Ok(Sensitivity::Public),
        "internal" => Ok(Sensitivity::Internal),
        "confidential" => Ok(Sensitivity::Confidential),
        "phi" => Ok(Sensitivity::Phi),
        _ => Err(format!("Unknown sensitivity '{}'", s)),
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ===== Given: seed YAML content =====
        step_def(
            "a playbook machine.yaml with content:",
            &[],
            &[(YAML_KEY, "String")],
            |_ctx, params| {
                let yaml = params.doc_string().ok_or("Expected a doc string")?;
                let mut out = Context::new();
                out.set(YAML_KEY, yaml.to_string());
                Ok(out)
            },
        ),
        // ===== When: parse with no hook files =====
        step_def(
            "the playbook loader parses the file with artifact id {string} and no hook files",
            &[(YAML_KEY, "String")],
            &[
                (ARTIFACT_ID_KEY, "String"),
                (MACHINE_KEY, "Option<PlaybookMachine>"),
                (ERROR_KEY, "Option<PlaybookLoadError>"),
            ],
            |ctx, params| {
                let artifact_id = params.get_string(0).ok_or("Expected artifact id")?.to_string();
                let yaml = ctx.get::<String>(YAML_KEY).ok_or("No yaml text")?;
                let result = load_from_yaml(&artifact_id, yaml, &[]);
                let mut out = Context::new();
                out.set(ARTIFACT_ID_KEY, artifact_id);
                match result {
                    Ok(machine) => {
                        out.set(MACHINE_KEY, Some(machine));
                        out.set(ERROR_KEY, None::<PlaybookLoadError>);
                    }
                    Err(e) => {
                        out.set(MACHINE_KEY, None::<PlaybookMachine>);
                        out.set(ERROR_KEY, Some(e));
                    }
                }
                Ok(out)
            },
        ),
        // ===== When: parse with hook files list =====
        step_def(
            "the playbook loader parses the file with artifact id {string} and hook files {string}",
            &[(YAML_KEY, "String")],
            &[
                (ARTIFACT_ID_KEY, "String"),
                (MACHINE_KEY, "Option<PlaybookMachine>"),
                (ERROR_KEY, "Option<PlaybookLoadError>"),
            ],
            |ctx, params| {
                let artifact_id = params.get_string(0).ok_or("Expected artifact id")?.to_string();
                let hook_files_str = params.get_string(1).ok_or("Expected hook files list")?.to_string();
                let hook_files = parse_string_list(&hook_files_str)?;
                let yaml = ctx.get::<String>(YAML_KEY).ok_or("No yaml text")?;
                let result = load_from_yaml(&artifact_id, yaml, &hook_files);
                let mut out = Context::new();
                out.set(ARTIFACT_ID_KEY, artifact_id);
                match result {
                    Ok(machine) => {
                        out.set(MACHINE_KEY, Some(machine));
                        out.set(ERROR_KEY, None::<PlaybookLoadError>);
                    }
                    Err(e) => {
                        out.set(MACHINE_KEY, None::<PlaybookMachine>);
                        out.set(ERROR_KEY, Some(e));
                    }
                }
                Ok(out)
            },
        ),
        // ===== Then: parse succeeds =====
        check_def(
            "the parse succeeds",
            &[(ERROR_KEY, "Option<PlaybookLoadError>")],
            |ctx, _params| {
                let err = ctx.get::<Option<PlaybookLoadError>>(ERROR_KEY).ok_or("No error key")?;
                if let Some(e) = err {
                    Err(format!("Expected parse success but got error: {}", e))
                } else {
                    Ok(())
                }
            },
        ),
        // ===== Then: loaded playbook kind =====
        check_def(
            "the loaded playbook kind is {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected kind value")?;
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                if machine.kind != expected {
                    Err(format!("Expected kind '{}' but got '{}'", expected, machine.kind))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the loaded playbook directory is {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected directory value")?;
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                if machine.directory != expected {
                    Err(format!("Expected directory '{}' but got '{}'", expected, machine.directory))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the loaded playbook registry is {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected registry value")?;
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                if machine.registry != expected {
                    Err(format!("Expected registry '{}' but got '{}'", expected, machine.registry))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the loaded playbook description is {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected description value")?;
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                if machine.description != expected {
                    Err(format!(
                        "Expected description '{}' but got '{}'",
                        expected, machine.description
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the loaded playbook access is org {string} min_role {string} sensitivity {string} space {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let org = params.get_string(0).ok_or("Expected org")?;
                let min_role = parse_role(&params.get_string(1).ok_or("Expected min_role")?)?;
                let sensitivity =
                    parse_sensitivity(&params.get_string(2).ok_or("Expected sensitivity")?)?;
                let space = params.get_string(3).ok_or("Expected space")?;
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;

                let mut errs = Vec::new();
                if machine.access.org != org {
                    errs.push(format!(
                        "org: expected '{}' got '{}'",
                        org, machine.access.org
                    ));
                }
                if machine.access.min_role != min_role {
                    errs.push(format!(
                        "min_role: expected {:?} got {:?}",
                        min_role, machine.access.min_role
                    ));
                }
                if machine.access.sensitivity != sensitivity {
                    errs.push(format!(
                        "sensitivity: expected {:?} got {:?}",
                        sensitivity, machine.access.sensitivity
                    ));
                }
                match &machine.access.space {
                    Some(actual) if actual == space => {}
                    Some(actual) => {
                        errs.push(format!("space: expected '{}' got '{}'", space, actual));
                    }
                    None => errs.push(format!("space: expected '{}' got None", space)),
                }

                if errs.is_empty() {
                    Ok(())
                } else {
                    Err(errs.join("; "))
                }
            },
        ),
        check_def(
            "the loaded playbook access is org {string} min_role {string} sensitivity {string} with no space",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let org = params.get_string(0).ok_or("Expected org")?;
                let min_role = parse_role(&params.get_string(1).ok_or("Expected min_role")?)?;
                let sensitivity =
                    parse_sensitivity(&params.get_string(2).ok_or("Expected sensitivity")?)?;
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;

                let mut errs = Vec::new();
                if machine.access.org != org {
                    errs.push(format!(
                        "org: expected '{}' got '{}'",
                        org, machine.access.org
                    ));
                }
                if machine.access.min_role != min_role {
                    errs.push(format!(
                        "min_role: expected {:?} got {:?}",
                        min_role, machine.access.min_role
                    ));
                }
                if machine.access.sensitivity != sensitivity {
                    errs.push(format!(
                        "sensitivity: expected {:?} got {:?}",
                        sensitivity, machine.access.sensitivity
                    ));
                }
                if let Some(space) = &machine.access.space {
                    errs.push(format!("space: expected None got '{}'", space));
                }

                if errs.is_empty() {
                    Ok(())
                } else {
                    Err(errs.join("; "))
                }
            },
        ),
        check_def(
            "the loaded playbook parent_kind is {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected parent_kind value")?;
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                match &machine.parent_kind {
                    Some(pk) if pk == &expected => Ok(()),
                    Some(pk) => Err(format!("Expected parent_kind '{}' but got '{}'", expected, pk)),
                    None => Err(format!("Expected parent_kind '{}' but got None", expected)),
                }
            },
        ),
        check_def(
            "the loaded playbook parent_kind is absent",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, _params| {
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                if machine.parent_kind.is_some() {
                    Err(format!(
                        "Expected parent_kind to be absent but got '{}'",
                        machine.parent_kind.as_deref().unwrap_or("")
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the loaded playbook route triggers are {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let expected_raw = params.get_string(0).ok_or("Expected triggers list")?;
                let expected = parse_string_list(&expected_raw)?;
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                if machine.route.triggers == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected route triggers {:?} but got {:?}",
                        expected, machine.route.triggers
                    ))
                }
            },
        ),
        check_def(
            "the loaded playbook route triggers are:",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected a data table")?;
                let trigger_idx = table
                    .headers
                    .iter()
                    .position(|h| h == "trigger")
                    .ok_or("Missing column 'trigger'")?;
                let expected = table
                    .rows
                    .iter()
                    .map(|row| {
                        row.get(trigger_idx)
                            .ok_or("Row too short for 'trigger'")
                            .map(|value| value.to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                if machine.route.triggers == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected route triggers {:?} but got {:?}",
                        expected, machine.route.triggers
                    ))
                }
            },
        ),
        check_def(
            "the loaded playbook route description is {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected route description")?;
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                match machine.route.description.as_deref() {
                    Some(actual) if actual == expected.as_ref() as &str => Ok(()),
                    Some(actual) => Err(format!(
                        "Expected route description '{}' but got '{}'",
                        expected, actual
                    )),
                    None => Err("Expected route description but got None".to_string()),
                }
            },
        ),
        check_def(
            "the loaded playbook route triggers are empty",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, _params| {
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                if machine.route.triggers.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected route triggers [] but got {:?}",
                        machine.route.triggers
                    ))
                }
            },
        ),
        check_def(
            "the loaded playbook has {int} required fields",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let expected_count: usize = params
                    .get_int(0)
                    .ok_or("Expected count")?
                    .try_into()
                    .map_err(|_| "Negative count".to_string())?;
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                if machine.required_fields.len() != expected_count {
                    Err(format!(
                        "Expected {} required fields but got {}",
                        expected_count,
                        machine.required_fields.len()
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "required field {int} has name {string} type {string} description {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let idx: usize = params
                    .get_int(0)
                    .ok_or("Expected index")?
                    .try_into()
                    .map_err(|_| "Negative index".to_string())?;
                let name = params.get_string(1).ok_or("Expected name")?;
                let field_type = params.get_string(2).ok_or("Expected type")?;
                let description = params.get_string(3).ok_or("Expected description")?;
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                let field = machine
                    .required_fields
                    .get(idx)
                    .ok_or_else(|| format!("No required field at index {}", idx))?;
                let mut errs = Vec::new();
                if field.name != name {
                    errs.push(format!("name: expected '{}' got '{}'", name, field.name));
                }
                if field.field_type != field_type {
                    errs.push(format!(
                        "field_type: expected '{}' got '{}'",
                        field_type, field.field_type
                    ));
                }
                if field.description != description {
                    errs.push(format!(
                        "description: expected '{}' got '{}'",
                        description, field.description
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
            "the loaded playbook roles are {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let list_str = params.get_string(0).ok_or("Expected roles list")?;
                let expected = parse_string_list(&list_str)?;
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                if machine.roles != expected {
                    Err(format!(
                        "Expected roles {:?} but got {:?}",
                        expected, machine.roles
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the loaded playbook has {int} states",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let expected_count: usize = params
                    .get_int(0)
                    .ok_or("Expected count")?
                    .try_into()
                    .map_err(|_| "Negative count".to_string())?;
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                if machine.states.len() != expected_count {
                    Err(format!(
                        "Expected {} states but got {}",
                        expected_count,
                        machine.states.len()
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "state {int} has name {string} registry_section {string} is_review_gate {string} is_terminal {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let idx: usize = params
                    .get_int(0)
                    .ok_or("Expected index")?
                    .try_into()
                    .map_err(|_| "Negative index".to_string())?;
                let name = params.get_string(1).ok_or("Expected name")?;
                let registry_section = params.get_string(2).ok_or("Expected registry_section")?;
                let is_review_gate_str = params.get_string(3).ok_or("Expected is_review_gate bool")?;
                let is_terminal_str = params.get_string(4).ok_or("Expected is_terminal bool")?;
                let is_review_gate = parse_bool(&is_review_gate_str)?;
                let is_terminal = parse_bool(&is_terminal_str)?;

                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                let state = machine
                    .states
                    .get(idx)
                    .ok_or_else(|| format!("No state at index {}", idx))?;
                let mut errs = Vec::new();
                if state.name != name {
                    errs.push(format!("name: expected '{}' got '{}'", name, state.name));
                }
                if state.registry_section != registry_section {
                    errs.push(format!(
                        "registry_section: expected '{}' got '{}'",
                        registry_section, state.registry_section
                    ));
                }
                if state.is_review_gate != is_review_gate {
                    errs.push(format!(
                        "is_review_gate: expected {} got {}",
                        is_review_gate, state.is_review_gate
                    ));
                }
                if state.is_terminal != is_terminal {
                    errs.push(format!(
                        "is_terminal: expected {} got {}",
                        is_terminal, state.is_terminal
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
            "state {int} has role_filters {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let idx: usize = params
                    .get_int(0)
                    .ok_or("Expected index")?
                    .try_into()
                    .map_err(|_| "Negative index".to_string())?;
                let list_str = params.get_string(1).ok_or("Expected role_filters list")?;
                let expected_strs = parse_string_list(&list_str)?;

                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                let state = machine
                    .states
                    .get(idx)
                    .ok_or_else(|| format!("No state at index {}", idx))?;

                // Convert role_filters to snake_case strings for comparison.
                let actual_strs: Vec<String> = state
                    .role_filters
                    .iter()
                    .map(|rf| role_filter_to_str(rf).to_string())
                    .collect();

                if actual_strs != expected_strs {
                    Err(format!(
                        "Expected role_filters {:?} but got {:?}",
                        expected_strs, actual_strs
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "state {int} has projection_targets {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let idx: usize = params
                    .get_int(0)
                    .ok_or("Expected index")?
                    .try_into()
                    .map_err(|_| "Negative index".to_string())?;
                let list_str = params.get_string(1).ok_or("Expected projection_targets list")?;
                let expected = parse_string_list(&list_str)?;
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                let state = machine
                    .states
                    .get(idx)
                    .ok_or_else(|| format!("No state at index {}", idx))?;
                if state.projection_targets != expected {
                    Err(format!(
                        "Expected projection_targets {:?} but got {:?}",
                        expected, state.projection_targets
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "state {int} has hook {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let idx: usize = params
                    .get_int(0)
                    .ok_or("Expected index")?
                    .try_into()
                    .map_err(|_| "Negative index".to_string())?;
                let expected = params.get_string(1).ok_or("Expected hook value")?;
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                let state = machine
                    .states
                    .get(idx)
                    .ok_or_else(|| format!("No state at index {}", idx))?;
                match &state.hook {
                    Some(h) if h == &expected => Ok(()),
                    Some(h) => Err(format!("Expected hook '{}' but got '{}'", expected, h)),
                    None => Err(format!("Expected hook '{}' but got None", expected)),
                }
            },
        ),
        check_def(
            "state {int} has no hook",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let idx: usize = params
                    .get_int(0)
                    .ok_or("Expected index")?
                    .try_into()
                    .map_err(|_| "Negative index".to_string())?;
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                let state = machine
                    .states
                    .get(idx)
                    .ok_or_else(|| format!("No state at index {}", idx))?;
                if state.hook.is_some() {
                    Err(format!("Expected no hook but got '{}'", state.hook.as_deref().unwrap()))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the loaded playbook has {int} transitions",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let expected_count: usize = params
                    .get_int(0)
                    .ok_or("Expected count")?
                    .try_into()
                    .map_err(|_| "Negative count".to_string())?;
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                if machine.transitions.len() != expected_count {
                    Err(format!(
                        "Expected {} transitions but got {}",
                        expected_count,
                        machine.transitions.len()
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "transition {int} has from_state {string} to_state {string} required_role {string} requires_approver {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let idx: usize = params
                    .get_int(0)
                    .ok_or("Expected index")?
                    .try_into()
                    .map_err(|_| "Negative index".to_string())?;
                let from_state = params.get_string(1).ok_or("Expected from_state")?;
                let to_state = params.get_string(2).ok_or("Expected to_state")?;
                let required_role = params.get_string(3).ok_or("Expected required_role")?;
                let requires_approver_str = params.get_string(4).ok_or("Expected requires_approver bool")?;
                let requires_approver = parse_bool(&requires_approver_str)?;

                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                let transition = machine
                    .transitions
                    .get(idx)
                    .ok_or_else(|| format!("No transition at index {}", idx))?;
                let mut errs = Vec::new();
                if transition.from_state != from_state {
                    errs.push(format!(
                        "from_state: expected '{}' got '{}'",
                        from_state, transition.from_state
                    ));
                }
                if transition.to_state != to_state {
                    errs.push(format!(
                        "to_state: expected '{}' got '{}'",
                        to_state, transition.to_state
                    ));
                }
                if transition.required_role != required_role {
                    errs.push(format!(
                        "required_role: expected '{}' got '{}'",
                        required_role, transition.required_role
                    ));
                }
                if transition.requires_approver != requires_approver {
                    errs.push(format!(
                        "requires_approver: expected {} got {}",
                        requires_approver, transition.requires_approver
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
            "transition {int} has no required_satisfaction",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let idx: usize = params
                    .get_int(0)
                    .ok_or("Expected index")?
                    .try_into()
                    .map_err(|_| "Negative index".to_string())?;
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                let transition = machine
                    .transitions
                    .get(idx)
                    .ok_or_else(|| format!("No transition at index {}", idx))?;
                if transition.required_satisfaction.is_some() {
                    Err(format!(
                        "Expected no required_satisfaction but got {:?}",
                        transition.required_satisfaction
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "transition {int} has required_satisfaction {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let idx: usize = params
                    .get_int(0)
                    .ok_or("Expected index")?
                    .try_into()
                    .map_err(|_| "Negative index".to_string())?;
                let list_str = params.get_string(1).ok_or("Expected required_satisfaction list")?;
                let expected = parse_string_list(&list_str)?;
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                let transition = machine
                    .transitions
                    .get(idx)
                    .ok_or_else(|| format!("No transition at index {}", idx))?;
                match &transition.required_satisfaction {
                    Some(actual) if *actual == expected => Ok(()),
                    Some(actual) => Err(format!(
                        "Expected required_satisfaction {:?} but got {:?}",
                        expected, actual
                    )),
                    None => Err(format!(
                        "Expected required_satisfaction {:?} but got None",
                        expected
                    )),
                }
            },
        ),
        check_def(
            "transition {int} has hook {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let idx: usize = params
                    .get_int(0)
                    .ok_or("Expected index")?
                    .try_into()
                    .map_err(|_| "Negative index".to_string())?;
                let expected = params.get_string(1).ok_or("Expected hook value")?;
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                let transition = machine
                    .transitions
                    .get(idx)
                    .ok_or_else(|| format!("No transition at index {}", idx))?;
                match &transition.hook {
                    Some(h) if h == &expected => Ok(()),
                    Some(h) => Err(format!("Expected hook '{}' but got '{}'", expected, h)),
                    None => Err(format!("Expected hook '{}' but got None", expected)),
                }
            },
        ),
        // ===== Then: parse fails with error code =====
        check_def(
            "the parse fails with error code {string}",
            &[(ERROR_KEY, "Option<PlaybookLoadError>")],
            |ctx, params| {
                let expected_code = params.get_string(0).ok_or("Expected error code")?;
                let err = ctx
                    .get::<Option<PlaybookLoadError>>(ERROR_KEY)
                    .ok_or("No error key")?;
                match err {
                    Some(e) => {
                        if e.code() != expected_code {
                            Err(format!(
                                "Expected error code '{}' but got '{}' ({})",
                                expected_code,
                                e.code(),
                                e
                            ))
                        } else {
                            Ok(())
                        }
                    }
                    None => Err(format!(
                        "Expected parse failure with code '{}' but parse succeeded",
                        expected_code
                    )),
                }
            },
        ),
        check_def(
            "the error carries artifact_id {string}",
            &[(ERROR_KEY, "Option<PlaybookLoadError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected artifact_id")?;
                let err = ctx
                    .get::<Option<PlaybookLoadError>>(ERROR_KEY)
                    .ok_or("No error key")?
                    .as_ref()
                    .ok_or("No error to check")?;
                let actual = error_artifact_id(err).ok_or("Error has no artifact_id field")?;
                if actual != expected {
                    Err(format!("Expected artifact_id '{}' but got '{}'", expected, actual))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the error carries a line number",
            &[(ERROR_KEY, "Option<PlaybookLoadError>")],
            |ctx, _params| {
                let err = ctx
                    .get::<Option<PlaybookLoadError>>(ERROR_KEY)
                    .ok_or("No error key")?
                    .as_ref()
                    .ok_or("No error to check")?;
                match err {
                    PlaybookLoadError::YamlParseError { .. } => Ok(()),
                    _ => Err("Error is not a YamlParseError; no line number".to_string()),
                }
            },
        ),
        check_def(
            "the error carries a column number",
            &[(ERROR_KEY, "Option<PlaybookLoadError>")],
            |ctx, _params| {
                let err = ctx
                    .get::<Option<PlaybookLoadError>>(ERROR_KEY)
                    .ok_or("No error key")?
                    .as_ref()
                    .ok_or("No error to check")?;
                match err {
                    PlaybookLoadError::YamlParseError { .. } => Ok(()),
                    _ => Err("Error is not a YamlParseError; no column number".to_string()),
                }
            },
        ),
        check_def(
            "the error carries key_path {string}",
            &[(ERROR_KEY, "Option<PlaybookLoadError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected key_path")?;
                let err = ctx
                    .get::<Option<PlaybookLoadError>>(ERROR_KEY)
                    .ok_or("No error key")?
                    .as_ref()
                    .ok_or("No error to check")?;
                match err {
                    PlaybookLoadError::MissingRequiredKey { key_path, .. } => {
                        if key_path == &expected {
                            Ok(())
                        } else {
                            Err(format!("Expected key_path '{}' but got '{}'", expected, key_path))
                        }
                    }
                    _ => Err(format!("Error is not MissingRequiredKey: {}", err)),
                }
            },
        ),
        check_def(
            "the error carries from_state {string}",
            &[(ERROR_KEY, "Option<PlaybookLoadError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected from_state")?;
                let err = ctx
                    .get::<Option<PlaybookLoadError>>(ERROR_KEY)
                    .ok_or("No error key")?
                    .as_ref()
                    .ok_or("No error to check")?;
                let actual = error_from_state(err).ok_or("Error has no from_state field")?;
                if actual != expected {
                    Err(format!("Expected from_state '{}' but got '{}'", expected, actual))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the error carries to_state {string}",
            &[(ERROR_KEY, "Option<PlaybookLoadError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected to_state")?;
                let err = ctx
                    .get::<Option<PlaybookLoadError>>(ERROR_KEY)
                    .ok_or("No error key")?
                    .as_ref()
                    .ok_or("No error to check")?;
                let actual = error_to_state(err).ok_or("Error has no to_state field")?;
                if actual != expected {
                    Err(format!("Expected to_state '{}' but got '{}'", expected, actual))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the error carries role {string}",
            &[(ERROR_KEY, "Option<PlaybookLoadError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected role")?;
                let err = ctx
                    .get::<Option<PlaybookLoadError>>(ERROR_KEY)
                    .ok_or("No error key")?
                    .as_ref()
                    .ok_or("No error to check")?;
                match err {
                    PlaybookLoadError::UnknownRoleReference { role, .. } => {
                        if role == &expected {
                            Ok(())
                        } else {
                            Err(format!("Expected role '{}' but got '{}'", expected, role))
                        }
                    }
                    _ => Err(format!("Error is not UnknownRoleReference: {}", err)),
                }
            },
        ),
        check_def(
            "the error carries unknown_state {string}",
            &[(ERROR_KEY, "Option<PlaybookLoadError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected unknown_state")?;
                let err = ctx
                    .get::<Option<PlaybookLoadError>>(ERROR_KEY)
                    .ok_or("No error key")?
                    .as_ref()
                    .ok_or("No error to check")?;
                match err {
                    PlaybookLoadError::UnknownStateReference { unknown_state, .. } => {
                        if unknown_state == &expected {
                            Ok(())
                        } else {
                            Err(format!(
                                "Expected unknown_state '{}' but got '{}'",
                                expected, unknown_state
                            ))
                        }
                    }
                    _ => Err(format!("Error is not UnknownStateReference: {}", err)),
                }
            },
        ),
        check_def(
            "the error carries state {string}",
            &[(ERROR_KEY, "Option<PlaybookLoadError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?;
                let err = ctx
                    .get::<Option<PlaybookLoadError>>(ERROR_KEY)
                    .ok_or("No error key")?
                    .as_ref()
                    .ok_or("No error to check")?;
                match err {
                    PlaybookLoadError::ReviewGateMissingSatisfaction { state, .. } => {
                        if state == &expected {
                            Ok(())
                        } else {
                            Err(format!("Expected state '{}' but got '{}'", expected, state))
                        }
                    }
                    _ => Err(format!("Error is not ReviewGateMissingSatisfaction: {}", err)),
                }
            },
        ),
        check_def(
            "the error carries context {string}",
            &[(ERROR_KEY, "Option<PlaybookLoadError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected context")?;
                let err = ctx
                    .get::<Option<PlaybookLoadError>>(ERROR_KEY)
                    .ok_or("No error key")?
                    .as_ref()
                    .ok_or("No error to check")?;
                match err {
                    PlaybookLoadError::UnknownHookReference { context, .. }
                    | PlaybookLoadError::HookPathInvalid { context, .. }
                    | PlaybookLoadError::UnknownRoleKeyReference { context, .. } => {
                        if context == &expected {
                            Ok(())
                        } else {
                            Err(format!("Expected context '{}' but got '{}'", expected, context))
                        }
                    }
                    _ => Err(format!("Error does not carry a 'context' field: {}", err)),
                }
            },
        ),
        check_def(
            "the error carries filename {string}",
            &[(ERROR_KEY, "Option<PlaybookLoadError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected filename")?;
                let err = ctx
                    .get::<Option<PlaybookLoadError>>(ERROR_KEY)
                    .ok_or("No error key")?
                    .as_ref()
                    .ok_or("No error to check")?;
                match err {
                    PlaybookLoadError::UnknownHookReference { filename, .. }
                    | PlaybookLoadError::HookPathInvalid { filename, .. } => {
                        if filename == &expected {
                            Ok(())
                        } else {
                            Err(format!("Expected filename '{}' but got '{}'", expected, filename))
                        }
                    }
                    _ => Err(format!("Error is not UnknownHookReference: {}", err)),
                }
            },
        ),
    ]
}

// ===== Helpers =====

fn parse_bool(s: &str) -> Result<bool, String> {
    match s.trim() {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(format!("Expected 'true' or 'false', got '{}'", other)),
    }
}

fn role_filter_to_str(rf: &anvil_core::domain::playbook::types::RoleFilter) -> &'static str {
    use anvil_core::domain::playbook::types::RoleFilter;
    match rf {
        RoleFilter::DoerActionable => "doer_actionable",
        RoleFilter::ReviewPending => "review_pending",
        RoleFilter::ReviewAwaiting => "review_awaiting",
        RoleFilter::CreatorParent => "creator_parent",
        RoleFilter::Terminal => "terminal",
    }
}

fn error_artifact_id(err: &PlaybookLoadError) -> Option<String> {
    match err {
        // C9: a hearth-directory failure has no owning artifact.
        PlaybookLoadError::HearthDirectoryMoveFailed { .. } => None,
        PlaybookLoadError::HearthDirectoryCollision { .. } => None,
        PlaybookLoadError::HearthRootUnreadable { .. } => None,
        PlaybookLoadError::YamlParseError { artifact_id, .. } => Some(artifact_id.clone()),
        PlaybookLoadError::MissingRequiredKey { artifact_id, .. } => Some(artifact_id.clone()),
        PlaybookLoadError::UnknownRoleReference { artifact_id, .. } => Some(artifact_id.clone()),
        PlaybookLoadError::UnknownStateReference { artifact_id, .. } => Some(artifact_id.clone()),
        PlaybookLoadError::ReviewGateMissingSatisfaction { artifact_id, .. } => {
            Some(artifact_id.clone())
        }
        PlaybookLoadError::HookPathInvalid { artifact_id, .. } => Some(artifact_id.clone()),
        PlaybookLoadError::UnknownHookReference { artifact_id, .. } => Some(artifact_id.clone()),
        PlaybookLoadError::UnknownRoleKeyReference { artifact_id, .. } => Some(artifact_id.clone()),
        PlaybookLoadError::UnreachableState { artifact_id, .. } => Some(artifact_id.clone()),
        PlaybookLoadError::DeadEndState { artifact_id, .. } => Some(artifact_id.clone()),
        PlaybookLoadError::NoTerminalReachable { artifact_id, .. } => Some(artifact_id.clone()),
        PlaybookLoadError::UnknownQualityDimension { artifact_id, .. } => Some(artifact_id.clone()),
        PlaybookLoadError::InvalidRubricWeight { artifact_id, .. } => Some(artifact_id.clone()),
        PlaybookLoadError::OutcomePredicateUnknownState { artifact_id, .. } => {
            Some(artifact_id.clone())
        }
        PlaybookLoadError::MeasurementDefinitionMissing { artifact_id, .. } => {
            Some(artifact_id.clone())
        }
        PlaybookLoadError::EvidenceObligationMissing { artifact_id, .. }
        | PlaybookLoadError::EvidenceObligationOnFreeRegister { artifact_id, .. } => {
            Some(artifact_id.clone())
        }
        PlaybookLoadError::DuplicateKindRegistration { .. } => None,
    }
}

fn error_from_state(err: &PlaybookLoadError) -> Option<String> {
    match err {
        PlaybookLoadError::UnknownRoleReference { from_state, .. } => Some(from_state.clone()),
        PlaybookLoadError::UnknownStateReference { from_state, .. } => Some(from_state.clone()),
        _ => None,
    }
}

fn error_to_state(err: &PlaybookLoadError) -> Option<String> {
    match err {
        PlaybookLoadError::UnknownRoleReference { to_state, .. } => Some(to_state.clone()),
        PlaybookLoadError::UnknownStateReference { to_state, .. } => Some(to_state.clone()),
        _ => None,
    }
}
