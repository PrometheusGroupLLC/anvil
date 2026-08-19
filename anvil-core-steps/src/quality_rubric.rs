//! Step module for quality_success_rubric.feature.
//!
//! Covers Phase A (shared quality-dimension vocabulary validation) and the
//! Phase B assertions on a parsed `PlaybookMachine`'s `success_rubric` and a
//! `MeasurementSpec`'s `success_criteria`. The machine-parse steps themselves
//! (Given/When/"the parse succeeds"/"the parse fails with error code") are
//! reused from `playbook_loader`; this module only adds the vocabulary checks
//! and the new-field assertions, reading the same `wl_machine` context key the
//! loader steps populate.

use anvil_core::domain::playbook::loader::load_from_yaml;
use anvil_core::domain::playbook::types::{
    is_valid_quality_dimension, EvidenceClass, PlaybookMachine,
};
use brine_runner_rust::registry::{check_def, StepDef};
use std::path::PathBuf;

/// Snake_case label for an [`EvidenceClass`], matching the serde rename.
fn evidence_class_to_str(ec: &EvidenceClass) -> &'static str {
    match ec {
        EvidenceClass::ArtifactOfConsequence => "artifact_of_consequence",
        EvidenceClass::VerifiableCitation => "verifiable_citation",
        EvidenceClass::SelfDescription => "self_description",
    }
}

/// Same key the `playbook_loader` steps store the parsed machine under.
const MACHINE_KEY: &str = "wl_machine";

/// Parse a Gherkin list literal like `["a", "b"]` into a Vec<String>.
fn parse_string_list(s: &str) -> Result<Vec<String>, String> {
    let s = s.trim();
    if s == "[]" {
        return Ok(vec![]);
    }
    if !s.starts_with('[') || !s.ends_with(']') {
        return Err(format!("Expected a list literal like [\"a\"], got: {}", s));
    }
    let inner = &s[1..s.len() - 1];
    Ok(inner
        .split(',')
        .map(|item| {
            let t = item.trim();
            if (t.starts_with('"') && t.ends_with('"'))
                || (t.starts_with('\'') && t.ends_with('\''))
            {
                t[1..t.len() - 1].to_string()
            } else {
                t.to_string()
            }
        })
        .filter(|s| !s.is_empty())
        .collect())
}

fn machine<'a>(
    ctx: &'a brine_runner_rust::context::Context,
) -> Result<&'a PlaybookMachine, String> {
    ctx.get::<Option<PlaybookMachine>>(MACHINE_KEY)
        .ok_or("No machine key")?
        .as_ref()
        .ok_or("Parse did not succeed".to_string())
}

/// Every directory under the repo that physically holds a `machine.yaml`
/// (real playbooks + test fixtures). Resolved relative to this crate's
/// `CARGO_MANIFEST_DIR` (the `anvil-test-support/` dir): playbooks live at the
/// workspace root, fixtures under this crate.
fn on_disk_machine_dirs() -> Vec<PathBuf> {
    let manifest = PathBuf::from(anvil_test_support::TEST_SUPPORT_DIR);
    let workspace_root = manifest
        .parent()
        .map(PathBuf::from)
        .unwrap_or(manifest.clone());
    let mut dirs = Vec::new();
    for base in [workspace_root.join("playbooks"), manifest.join("fixtures")] {
        if let Ok(entries) = std::fs::read_dir(&base) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.join("machine.yaml").is_file() {
                    dirs.push(path);
                }
            }
        }
    }
    dirs.sort();
    dirs
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ===== Backward-compat guard: every on-disk machine still parses =====
        // Guards the deny_unknown_fields trap — after adding the additive
        // success_rubric / success_criteria fields, every real machine.yaml on
        // disk must still load cleanly through the loader.
        check_def(
            "every on-disk machine.yaml still parses through the loader",
            &[],
            |_ctx, _params| {
                let dirs = on_disk_machine_dirs();
                if dirs.is_empty() {
                    return Err("Found no on-disk machine.yaml files to check".to_string());
                }
                let mut failures = Vec::new();
                for dir in &dirs {
                    let yaml = match std::fs::read_to_string(dir.join("machine.yaml")) {
                        Ok(y) => y,
                        Err(e) => {
                            failures.push(format!("{}: read failed: {}", dir.display(), e));
                            continue;
                        }
                    };
                    // Collect hooks/*.md filenames so hook references validate.
                    let mut hook_files = Vec::new();
                    if let Ok(entries) = std::fs::read_dir(dir.join("hooks")) {
                        for entry in entries.flatten() {
                            if let Some(name) = entry.file_name().to_str() {
                                hook_files.push(name.to_string());
                            }
                        }
                    }
                    let artifact_id = dir
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown")
                        .to_string();
                    if let Err(e) = load_from_yaml(&artifact_id, &yaml, &hook_files) {
                        failures.push(format!("{}: {}", dir.display(), e));
                    }
                }
                if failures.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "{} on-disk machine.yaml file(s) failed to parse:\n{}",
                        failures.len(),
                        failures.join("\n")
                    ))
                }
            },
        ),
        // ===== Phase A: vocabulary validation =====
        check_def(
            "the quality dimension {string} is in the shared vocabulary",
            &[],
            |_ctx, params| {
                let dim = params.get_string(0).ok_or("Expected dimension")?;
                if is_valid_quality_dimension(&dim) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected '{}' to be a canonical quality dimension but it was rejected",
                        dim
                    ))
                }
            },
        ),
        check_def(
            "the quality dimension {string} is not in the shared vocabulary",
            &[],
            |_ctx, params| {
                let dim = params.get_string(0).ok_or("Expected dimension")?;
                if is_valid_quality_dimension(&dim) {
                    Err(format!(
                        "Expected '{}' to be rejected but it was accepted as canonical",
                        dim
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        // ===== Phase B: success_rubric assertions =====
        check_def(
            "the loaded playbook has no success_rubric",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, _params| {
                let m = machine(&ctx)?;
                if m.success_rubric.is_some() {
                    Err(format!(
                        "Expected no success_rubric but got {:?}",
                        m.success_rubric
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the loaded playbook success_rubric has {int} dimensions",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let expected: usize = params
                    .get_int(0)
                    .ok_or("Expected count")?
                    .try_into()
                    .map_err(|_| "Negative count".to_string())?;
                let rubric = machine(&ctx)?
                    .success_rubric
                    .as_ref()
                    .ok_or("No success_rubric on the loaded machine")?;
                if rubric.dimensions.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} rubric dimensions but got {}",
                        expected,
                        rubric.dimensions.len()
                    ))
                }
            },
        ),
        check_def(
            "the loaded playbook success_rubric dimension {int} is {string} weight {int}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let idx: usize = params
                    .get_int(0)
                    .ok_or("Expected index")?
                    .try_into()
                    .map_err(|_| "Negative index".to_string())?;
                let dimension = params.get_string(1).ok_or("Expected dimension")?;
                let weight: u32 = params
                    .get_int(2)
                    .ok_or("Expected weight")?
                    .try_into()
                    .map_err(|_| "Negative weight".to_string())?;
                let rubric = machine(&ctx)?
                    .success_rubric
                    .as_ref()
                    .ok_or("No success_rubric on the loaded machine")?;
                let entry = rubric
                    .dimensions
                    .get(idx)
                    .ok_or_else(|| format!("No rubric dimension at index {}", idx))?;
                let mut errs = Vec::new();
                if entry.dimension != dimension {
                    errs.push(format!(
                        "dimension: expected '{}' got '{}'",
                        dimension, entry.dimension
                    ));
                }
                if entry.weight != weight {
                    errs.push(format!("weight: expected {} got {}", weight, entry.weight));
                }
                if errs.is_empty() {
                    Ok(())
                } else {
                    Err(errs.join("; "))
                }
            },
        ),
        check_def(
            "the loaded playbook success_rubric grader is {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected grader")?;
                let rubric = machine(&ctx)?
                    .success_rubric
                    .as_ref()
                    .ok_or("No success_rubric on the loaded machine")?;
                match rubric.grader.as_deref() {
                    Some(actual) if actual == expected.as_ref() as &str => Ok(()),
                    Some(actual) => Err(format!(
                        "Expected grader '{}' but got '{}'",
                        expected, actual
                    )),
                    None => Err("Expected a grader but got None".to_string()),
                }
            },
        ),
        check_def(
            "the loaded playbook success_rubric lagging_signals are {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let expected = parse_string_list(&params.get_string(0).ok_or("Expected list")?)?;
                let rubric = machine(&ctx)?
                    .success_rubric
                    .as_ref()
                    .ok_or("No success_rubric on the loaded machine")?;
                if rubric.lagging_signals == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected lagging_signals {:?} but got {:?}",
                        expected, rubric.lagging_signals
                    ))
                }
            },
        ),
        // ===== Phase B: success_criteria on MeasurementSpec =====
        check_def(
            "the loaded measurement for state {string} role {string} has success_criteria {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let state = params.get_string(0).ok_or("Expected state")?;
                let role = params.get_string(1).ok_or("Expected role")?;
                let expected = params.get_string(2).ok_or("Expected success_criteria")?;
                let m = machine(&ctx)?;
                let spec = m
                    .states
                    .iter()
                    .find(|s| s.name == state)
                    .and_then(|s| s.measurement_by_role.get(role.as_ref() as &str))
                    .ok_or_else(|| {
                        format!("No measurement for state '{}' role '{}'", state, role)
                    })?;
                match spec.success_criteria.as_deref() {
                    Some(actual) if actual == expected.as_ref() as &str => Ok(()),
                    Some(actual) => Err(format!(
                        "Expected success_criteria '{}' but got '{}'",
                        expected, actual
                    )),
                    None => Err("Expected success_criteria but got None".to_string()),
                }
            },
        ),
        check_def(
            "the loaded measurement for state {string} role {string} has no success_criteria",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let state = params.get_string(0).ok_or("Expected state")?;
                let role = params.get_string(1).ok_or("Expected role")?;
                let m = machine(&ctx)?;
                let spec = m
                    .states
                    .iter()
                    .find(|s| s.name == state)
                    .and_then(|s| s.measurement_by_role.get(role.as_ref() as &str))
                    .ok_or_else(|| {
                        format!("No measurement for state '{}' role '{}'", state, role)
                    })?;
                if spec.success_criteria.is_some() {
                    Err(format!(
                        "Expected no success_criteria but got {:?}",
                        spec.success_criteria
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        // ===== Phase 3a: evidence_class on a rubric dimension =====
        check_def(
            "the loaded playbook success_rubric dimension {int} evidence_class is {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let idx: usize = params
                    .get_int(0)
                    .ok_or("Expected index")?
                    .try_into()
                    .map_err(|_| "Negative index".to_string())?;
                let expected = params.get_string(1).ok_or("Expected evidence_class")?;
                let rubric = machine(&ctx)?
                    .success_rubric
                    .as_ref()
                    .ok_or("No success_rubric on the loaded machine")?;
                let entry = rubric
                    .dimensions
                    .get(idx)
                    .ok_or_else(|| format!("No rubric dimension at index {}", idx))?;
                let actual = evidence_class_to_str(&entry.evidence_class);
                if actual == expected.as_ref() as &str {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected evidence_class '{}' but got '{}'",
                        expected, actual
                    ))
                }
            },
        ),
        // ===== Phase 3a: anchors on a success_rubric =====
        check_def(
            "the loaded playbook success_rubric has {int} anchors",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let expected: usize = params
                    .get_int(0)
                    .ok_or("Expected count")?
                    .try_into()
                    .map_err(|_| "Negative count".to_string())?;
                let rubric = machine(&ctx)?
                    .success_rubric
                    .as_ref()
                    .ok_or("No success_rubric on the loaded machine")?;
                if rubric.anchors.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} anchors but got {}",
                        expected,
                        rubric.anchors.len()
                    ))
                }
            },
        ),
        check_def(
            "the loaded playbook success_rubric anchor {int} is instance {string} band {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let idx: usize = params
                    .get_int(0)
                    .ok_or("Expected index")?
                    .try_into()
                    .map_err(|_| "Negative index".to_string())?;
                let instance = params.get_string(1).ok_or("Expected instance")?;
                let band = params.get_string(2).ok_or("Expected band")?;
                let rubric = machine(&ctx)?
                    .success_rubric
                    .as_ref()
                    .ok_or("No success_rubric on the loaded machine")?;
                let anchor = rubric
                    .anchors
                    .get(idx)
                    .ok_or_else(|| format!("No anchor at index {}", idx))?;
                let mut errs = Vec::new();
                if anchor.instance != instance {
                    errs.push(format!(
                        "instance: expected '{}' got '{}'",
                        instance, anchor.instance
                    ));
                }
                if anchor.band != band {
                    errs.push(format!("band: expected '{}' got '{}'", band, anchor.band));
                }
                if errs.is_empty() {
                    Ok(())
                } else {
                    Err(errs.join("; "))
                }
            },
        ),
        // ===== Phase 3a: outcome_predicate on a PlaybookMachine =====
        check_def(
            "the loaded playbook has no outcome_predicate",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, _params| {
                let m = machine(&ctx)?;
                if m.outcome_predicate.is_some() {
                    Err(format!(
                        "Expected no outcome_predicate but got {:?}",
                        m.outcome_predicate
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the loaded playbook outcome_predicate terminal_state is {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected terminal_state")?;
                let predicate = machine(&ctx)?
                    .outcome_predicate
                    .as_ref()
                    .ok_or("No outcome_predicate on the loaded machine")?;
                if predicate.terminal_state == expected.as_ref() as &str {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected terminal_state '{}' but got '{}'",
                        expected, predicate.terminal_state
                    ))
                }
            },
        ),
        check_def(
            "the loaded playbook outcome_predicate check is {string}",
            &[(MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected check")?;
                let predicate = machine(&ctx)?
                    .outcome_predicate
                    .as_ref()
                    .ok_or("No outcome_predicate on the loaded machine")?;
                match predicate.check.as_deref() {
                    Some(actual) if actual == expected.as_ref() as &str => Ok(()),
                    Some(actual) => Err(format!(
                        "Expected check '{}' but got '{}'",
                        expected, actual
                    )),
                    None => Err("Expected a check but got None".to_string()),
                }
            },
        ),
    ]
}
