//! Step module for evidence_obligation_schema.feature (T-EEC-1 Phase 0).
//!
//! Adds only the schema-surface steps unique to the obligation field:
//! asserting a loaded `MeasurementSpec` carries the declared
//! `evidence_obligation`, and re-serializing a loaded machine through
//! `serde_yaml` to prove byte-identity-when-absent (the key is omitted) and
//! round-trip-when-present (the key survives). The Given ("a playbook
//! machine.yaml with content:"), the default When ("...and no hook files"),
//! and the parse-success / parse-error / version-slot steps are reused from
//! `playbook_loader` and `playbook_version` — this module reads the same
//! `wl_machine` context key the loader steps populate.

use anvil_core::domain::playbook::loader::load_from_yaml;
use anvil_core::domain::playbook::types::{EvidenceClass, PlaybookMachine};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

/// Key `playbook_loader` stores the parsed machine under.
const MACHINE_KEY: &str = "wl_machine";
/// The serde_yaml text produced by the re-serialize step.
const RESERIALIZED_KEY: &str = "eo_reserialized_yaml";
/// The machine parsed back from the re-serialized YAML (round-trip proof).
const RELOADED_KEY: &str = "eo_reloaded_machine";

/// Snake_case name of an `EvidenceClass`, matching the YAML/serde vocabulary.
fn class_name(class: EvidenceClass) -> &'static str {
    match class {
        EvidenceClass::ArtifactOfConsequence => "artifact_of_consequence",
        EvidenceClass::VerifiableCitation => "verifiable_citation",
        EvidenceClass::SelfDescription => "self_description",
    }
}

/// Parse a comma-separated class list like `artifact_of_consequence, verifiable_citation`.
fn parse_expected(list: &str) -> Vec<String> {
    list.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Assert the obligation on `machine.states[state].measurement_by_role[role]`
/// equals `expected` (order-preserving snake_case list).
fn assert_obligation(
    machine: &PlaybookMachine,
    state: &str,
    role: &str,
    expected: &[String],
) -> Result<(), String> {
    let state_def = machine
        .states
        .iter()
        .find(|s| s.name == state)
        .ok_or_else(|| format!("no state '{}' in loaded machine", state))?;
    let spec = state_def
        .measurement_by_role
        .get(role)
        .ok_or_else(|| format!("no measurement_by_role entry for state '{}' role '{}'", state, role))?;
    let actual: Vec<String> = spec
        .evidence_obligation
        .iter()
        .map(|c| class_name(*c).to_string())
        .collect();
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "state '{}' role '{}' obligation: expected {:?} but got {:?}",
            state, role, expected, actual
        ))
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        check_def(
            "the loaded spec for state {string} role {string} requires evidence classes {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let state = params.get_string(0).ok_or("Expected state name")?;
                let role = params.get_string(1).ok_or("Expected role name")?;
                let expected = parse_expected(&params.get_string(2).ok_or("Expected class list")?);
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                assert_obligation(machine, &state, &role, &expected)
            },
        ),
        step_def(
            "the loaded machine is re-serialized to YAML and reloaded with artifact id {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            &[
                (RESERIALIZED_KEY, "String"),
                (RELOADED_KEY, "Option<PlaybookMachine>"),
            ],
            |ctx, params| {
                let artifact_id = params.get_string(0).ok_or("Expected artifact id")?.to_string();
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                let yaml = serde_yaml::to_string(machine)
                    .map_err(|e| format!("re-serialize to YAML failed: {}", e))?;
                let reloaded = load_from_yaml(&artifact_id, &yaml, &[])
                    .map_err(|e| format!("reload of re-serialized YAML failed: {}", e))?;
                let mut out = Context::new();
                out.set(RESERIALIZED_KEY, yaml);
                out.set(RELOADED_KEY, Some(reloaded));
                Ok(out)
            },
        ),
        check_def(
            "the re-serialized YAML omits an evidence_obligation key",
            &[(RESERIALIZED_KEY, "String")],
            |ctx, _params| {
                let yaml = ctx.get::<String>(RESERIALIZED_KEY).ok_or("No re-serialized YAML")?;
                if yaml.contains("evidence_obligation") {
                    Err(format!(
                        "expected no evidence_obligation key but the YAML carried one:\n{}",
                        yaml
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the re-serialized YAML contains an evidence_obligation key",
            &[(RESERIALIZED_KEY, "String")],
            |ctx, _params| {
                let yaml = ctx.get::<String>(RESERIALIZED_KEY).ok_or("No re-serialized YAML")?;
                if yaml.contains("evidence_obligation") {
                    Ok(())
                } else {
                    Err(format!(
                        "expected an evidence_obligation key but the YAML omitted it:\n{}",
                        yaml
                    ))
                }
            },
        ),
        check_def(
            "the reloaded spec for state {string} role {string} requires evidence classes {string}",
            &[(RELOADED_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let state = params.get_string(0).ok_or("Expected state name")?;
                let role = params.get_string(1).ok_or("Expected role name")?;
                let expected = parse_expected(&params.get_string(2).ok_or("Expected class list")?);
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(RELOADED_KEY)
                    .ok_or("No reloaded machine key")?
                    .as_ref()
                    .ok_or("Reload did not succeed")?;
                assert_obligation(machine, &state, &role, &expected)
            },
        ),
    ]
}
