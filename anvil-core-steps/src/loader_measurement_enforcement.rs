//! Step module for loader_measurement_enforcement.feature.
//!
//! The Given ("a playbook machine.yaml with content:") and the DEFAULT
//! (non-enforcing) When step ("the playbook loader parses the file with
//! artifact id {string} and no hook files") are reused from `playbook_loader`
//! — this module only adds the ENFORCING When step
//! (`load_from_yaml_enforcing`) and the new-error `Then` check, reading the
//! same `wl_yaml_text`/`wl_machine`/`wl_error` context keys the loader steps
//! populate (same reuse pattern `quality_rubric` uses).

use anvil_core::domain::playbook::load_error::PlaybookLoadError;
use anvil_core::domain::playbook::loader::load_from_yaml_enforcing;
use anvil_core::domain::playbook::types::PlaybookMachine;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

/// Same keys the `playbook_loader` steps store the parsed YAML/machine/error
/// under.
const YAML_KEY: &str = "wl_yaml_text";
const ARTIFACT_ID_KEY: &str = "wl_artifact_id";
const MACHINE_KEY: &str = "wl_machine";
const ERROR_KEY: &str = "wl_error";

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "the playbook loader parses the file with artifact id {string} and no hook files under measurement enforcement",
            &[(YAML_KEY, "String")],
            &[
                (ARTIFACT_ID_KEY, "String"),
                (MACHINE_KEY, "Option<PlaybookMachine>"),
                (ERROR_KEY, "Option<PlaybookLoadError>"),
            ],
            |ctx, params| {
                let artifact_id = params.get_string(0).ok_or("Expected artifact id")?.to_string();
                let yaml = ctx.get::<String>(YAML_KEY).ok_or("No yaml text")?;
                let result = load_from_yaml_enforcing(&artifact_id, yaml, &[]);
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
        check_def(
            "the error carries detail containing {string}",
            &[(ERROR_KEY, "Option<PlaybookLoadError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected detail substring")?;
                let err = ctx
                    .get::<Option<PlaybookLoadError>>(ERROR_KEY)
                    .ok_or("No error key")?
                    .as_ref()
                    .ok_or("No error to check")?;
                match err {
                    PlaybookLoadError::MeasurementDefinitionMissing { detail, .. } => {
                        if detail.contains(expected) {
                            Ok(())
                        } else {
                            Err(format!(
                                "Expected detail to contain '{}', got '{}'",
                                expected, detail
                            ))
                        }
                    }
                    _ => Err(format!("Error is not MeasurementDefinitionMissing: {}", err)),
                }
            },
        ),
    ]
}
