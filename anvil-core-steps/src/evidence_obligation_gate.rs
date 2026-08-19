//! Step module for evidence_obligation_gate.feature (T-EEC-1 Phase P2).
//!
//! The machine-YAML setup and common parse assertions are reused from
//! `playbook_loader`; the obligation round-trip assertions are reused from
//! `evidence_obligation_schema`. This module owns only the independently
//! configurable gate invocation and obligation-error detail checks.

use anvil_core::domain::playbook::load_error::PlaybookLoadError;
use anvil_core::domain::playbook::loader::{load_from_yaml_with, LoaderEnforcement};
use anvil_core::domain::playbook::types::PlaybookMachine;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

/// Same keys the shared `playbook_loader` steps use.
const YAML_KEY: &str = "wl_yaml_text";
const ARTIFACT_ID_KEY: &str = "wl_artifact_id";
const MACHINE_KEY: &str = "wl_machine";
const ERROR_KEY: &str = "wl_error";

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "the playbook loader parses the file with artifact id {string} and no hook files under evidence obligation enforcement {string}",
            &[(YAML_KEY, "String")],
            &[
                (ARTIFACT_ID_KEY, "String"),
                (MACHINE_KEY, "Option<PlaybookMachine>"),
                (ERROR_KEY, "Option<PlaybookLoadError>"),
            ],
            |ctx, params| {
                let artifact_id = params
                    .get_string(0)
                    .ok_or("Expected artifact id")?
                    .to_string();
                let enforcement = params
                    .get_string(1)
                    .ok_or("Expected evidence obligation enforcement state")?;
                let evidence_obligation = match enforcement.as_ref() as &str {
                    "on" => true,
                    "off" => false,
                    other => {
                        return Err(format!(
                            "Expected evidence obligation enforcement 'on' or 'off', got '{}'",
                            other
                        ))
                    }
                };

                let yaml = ctx.get::<String>(YAML_KEY).ok_or("No yaml text")?;
                let result = load_from_yaml_with(
                    &artifact_id,
                    yaml,
                    &[],
                    LoaderEnforcement {
                        measurement_definition: false,
                        evidence_obligation,
                    },
                );

                let mut out = Context::new();
                out.set(ARTIFACT_ID_KEY, artifact_id);
                match result {
                    Ok(machine) => {
                        out.set(MACHINE_KEY, Some(machine));
                        out.set(ERROR_KEY, None::<PlaybookLoadError>);
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
            "the evidence obligation error carries detail containing {string}",
            &[(ERROR_KEY, "Option<PlaybookLoadError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected detail substring")?;
                let error = ctx
                    .get::<Option<PlaybookLoadError>>(ERROR_KEY)
                    .ok_or("No error key")?
                    .as_ref()
                    .ok_or("No error to check")?;
                let detail = match error {
                    PlaybookLoadError::EvidenceObligationMissing { detail, .. }
                    | PlaybookLoadError::EvidenceObligationOnFreeRegister { detail, .. } => detail,
                    _ => return Err(format!("Error is not an evidence obligation error: {}", error)),
                };
                if detail.contains(expected.as_ref() as &str) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected detail to contain '{}', got '{}'",
                        expected, detail
                    ))
                }
            },
        ),
    ]
}
