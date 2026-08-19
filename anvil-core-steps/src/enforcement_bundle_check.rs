//! Step module for `enforcement_bundle_check.feature`.
//!
//! Exercises the bundle-wide enforcement conformance path end to end:
//! - stages synthetic `playbooks/<name>/machine.yaml` files in a temp bundle,
//! - evaluates them through `evaluate_enforcement_bundle` (the checker's
//!   library core, which the `enforcement_bundle_check` example wraps), and
//! - asserts the three-way outcome + exit-code contract the `build-kit.sh`
//!   gate keys off (a genuine loader-drop = exit 1, which the gate maps to its
//!   publish-blocking exit 3).
//!
//! It also drives `HearthPlaybookRegistry::new_enforcing` directly to prove the
//! dropped-artifact diagnostics: `measurement_dropped_artifacts()` names exactly
//! the machine the measurement-definition gate rejected.

use anvil_core::domain::enforcement_bundle_check::{
    evaluate_enforcement_bundle, BundleCheckOutcome,
};
use anvil_core::domain::playbook::hearth_registry::HearthPlaybookRegistry;
use anvil_core::domain::playbook::registry::PlaybookRegistry;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const ROOT_KEY: &str = "ebc_root";
const HANDLE_KEY: &str = "ebc_handle";
const OUTCOME_KEY: &str = "ebc_outcome";

/// A machine that PASSES measurement enforcement: its one measured state carries
/// `success_criteria` and the machine declares an `outcome_predicate`.
fn backfilled_machine_yaml(kind: &str) -> String {
    format!(
        r#"kind: {kind}
directory: {kind}s
registry: {kind}s.md
description: A backfilled {kind} lifecycle.
required_fields: []
roles: [doer]
states:
  - name: active
    role_filters: []
    registry_section: active
    projection_targets: []
    is_review_gate: false
    is_terminal: false
    measurement_by_role:
      doer:
        intent: Do the work.
        expected_output: An artifact.
        success_criteria: The artifact names at least one falsifiable acceptance criterion.
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
outcome_predicate:
  terminal_state: completed
"#,
        kind = kind
    )
}

/// A machine that FAILS measurement enforcement: its measured state has NO
/// `success_criteria` and it declares NO `outcome_predicate`. Under the
/// enforcing loader this machine DROPS out of the registry.
fn unbackfilled_machine_yaml(kind: &str) -> String {
    format!(
        r#"kind: {kind}
directory: {kind}s
registry: {kind}s.md
description: An unbackfilled {kind} lifecycle.
required_fields: []
roles: [doer]
states:
  - name: active
    role_filters: []
    registry_section: active
    projection_targets: []
    is_review_gate: false
    is_terminal: false
    measurement_by_role:
      doer:
        intent: Do the work.
        expected_output: An artifact.
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

fn stage(root: &std::path::Path, dir: &str, contents: &str) -> Result<(), String> {
    let machine_dir = root.join("playbooks").join(dir);
    std::fs::create_dir_all(&machine_dir).map_err(|e| format!("mkdir {}: {}", dir, e))?;
    std::fs::write(machine_dir.join("machine.yaml"), contents)
        .map_err(|e| format!("write {}: {}", dir, e))
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a temp playbook bundle",
            &[],
            &[
                (ROOT_KEY, "PathBuf"),
                (HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, _params| {
                let temp_dir = tempfile::TempDir::new()
                    .map_err(|e| format!("Failed to create temp dir: {}", e))?;
                let root = temp_dir.path().to_path_buf();
                let handle: Arc<Mutex<Option<tempfile::TempDir>>> =
                    Arc::new(Mutex::new(Some(temp_dir)));
                let mut out = Context::new();
                out.set(ROOT_KEY, root);
                out.set(HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        stage_step(
            "the bundle stages a backfilled {string} machine",
            backfilled_machine_yaml,
        ),
        stage_step(
            "the bundle stages an unbackfilled {string} machine",
            unbackfilled_machine_yaml,
        ),
        step_def(
            "the enforcement bundle check is evaluated",
            &[
                (ROOT_KEY, "PathBuf"),
                (HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (OUTCOME_KEY, "BundleCheckOutcome"),
                (ROOT_KEY, "PathBuf"),
                // Carry the TempDir handle forward so the scratch bundle is not
                // deleted before the registry-diagnostic check steps run.
                (HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, _params| {
                let root = ctx.get::<PathBuf>(ROOT_KEY).ok_or("No bundle root")?.clone();
                let handle = ctx
                    .get::<Arc<Mutex<Option<tempfile::TempDir>>>>(HANDLE_KEY)
                    .ok_or("No bundle handle")?
                    .clone();
                let outcome = evaluate_enforcement_bundle(&root);
                let mut out = Context::new();
                out.set(OUTCOME_KEY, outcome);
                out.set(ROOT_KEY, root);
                out.set(HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        check_def(
            "the bundle check outcome is {string}",
            &[(OUTCOME_KEY, "BundleCheckOutcome")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected outcome name")?;
                let outcome = ctx
                    .get::<BundleCheckOutcome>(OUTCOME_KEY)
                    .ok_or("No bundle outcome")?;
                let actual = match outcome {
                    BundleCheckOutcome::Pass { .. } => "pass",
                    BundleCheckOutcome::Drop { .. } => "drop",
                    BundleCheckOutcome::Setup { .. } => "setup",
                };
                if actual == expected.as_ref() as &str {
                    Ok(())
                } else {
                    Err(format!("Expected outcome '{}' but got '{}'", expected, actual))
                }
            },
        ),
        check_def(
            "the bundle check exit code is {string}",
            &[(OUTCOME_KEY, "BundleCheckOutcome")],
            |ctx, params| {
                let expected: i32 = params
                    .get_string(0)
                    .ok_or("Expected exit code")?
                    .parse()
                    .map_err(|_| "exit code not an integer".to_string())?;
                let outcome = ctx
                    .get::<BundleCheckOutcome>(OUTCOME_KEY)
                    .ok_or("No bundle outcome")?;
                if outcome.exit_code() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected exit code {} but got {}",
                        expected,
                        outcome.exit_code()
                    ))
                }
            },
        ),
        check_def(
            "the bundle check drops artifact {string}",
            &[(OUTCOME_KEY, "BundleCheckOutcome")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected artifact id")?;
                let outcome = ctx
                    .get::<BundleCheckOutcome>(OUTCOME_KEY)
                    .ok_or("No bundle outcome")?;
                match outcome {
                    BundleCheckOutcome::Drop { dropped, .. } => {
                        if dropped.iter().any(|d| d == expected.as_ref() as &str) {
                            Ok(())
                        } else {
                            Err(format!(
                                "Expected dropped artifact '{}' but dropped set was {:?}",
                                expected, dropped
                            ))
                        }
                    }
                    other => Err(format!("Outcome is not a drop: {:?}", other)),
                }
            },
        ),
        check_def(
            "the enforcing registry over the bundle drops no artifacts",
            &[(ROOT_KEY, "PathBuf")],
            |ctx, _params| {
                let root = ctx.get::<PathBuf>(ROOT_KEY).ok_or("No bundle root")?.clone();
                let reg = HearthPlaybookRegistry::new_enforcing(root);
                let dropped = reg.measurement_dropped_artifacts();
                if dropped.is_empty() {
                    Ok(())
                } else {
                    Err(format!("Expected no drops but got {:?}", dropped))
                }
            },
        ),
        check_def(
            "the enforcing registry over the bundle registers kind {string}",
            &[(ROOT_KEY, "PathBuf")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let root = ctx.get::<PathBuf>(ROOT_KEY).ok_or("No bundle root")?.clone();
                let reg = HearthPlaybookRegistry::new_enforcing(root);
                if reg.kinds().iter().any(|k| k == &kind) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected kind '{}' registered but kinds were {:?}",
                        kind,
                        reg.kinds()
                    ))
                }
            },
        ),
        check_def(
            "the enforcing registry over the bundle reports measurement-dropped artifact {string}",
            &[(ROOT_KEY, "PathBuf")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected artifact id")?.to_string();
                let root = ctx.get::<PathBuf>(ROOT_KEY).ok_or("No bundle root")?.clone();
                let reg = HearthPlaybookRegistry::new_enforcing(root);
                let dropped = reg.measurement_dropped_artifacts();
                if dropped.iter().any(|d| *d == expected) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected measurement-dropped '{}' but drop set was {:?}",
                        expected, dropped
                    ))
                }
            },
        ),
    ]
}

/// A staging `Given` step whose machine.yaml body is produced by `gen(kind)`
/// and written to `playbooks/<kind>_lifecycle/machine.yaml`.
fn stage_step(pattern: &'static str, gen: fn(&str) -> String) -> StepDef {
    step_def(
        pattern,
        &[
            (ROOT_KEY, "PathBuf"),
            (HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
        ],
        &[
            (ROOT_KEY, "PathBuf"),
            (HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
        ],
        move |ctx, params| {
            let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
            let root = ctx.get::<PathBuf>(ROOT_KEY).ok_or("No bundle root")?.clone();
            let handle = ctx
                .get::<Arc<Mutex<Option<tempfile::TempDir>>>>(HANDLE_KEY)
                .ok_or("No bundle handle")?
                .clone();
            stage(&root, &format!("{}_lifecycle", kind), &gen(&kind))?;
            let mut out = Context::new();
            out.set(ROOT_KEY, root);
            out.set(HANDLE_KEY, handle);
            Ok(out)
        },
    )
}
