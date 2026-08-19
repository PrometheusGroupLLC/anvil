//! Step module for playbook seed feature files.
//!
//! Provides steps for:
//! - Accessing the `track_seed()` and `playbook_seed()` compiled-in seeds
//! - Calling `validate()` on a seed with empty hook files
//! - Asserting transition presence/absence on the interpreter-visible axis
//!   (from_state, to_state, required_role)

use anvil_core::domain::playbook::loader::validate;
use anvil_core::domain::playbook::seeds::{playbook_seed, track_seed};
use anvil_core::domain::playbook::types::PlaybookMachine;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

/// Context key for the current seed under test.
const SEED_KEY: &str = "ws_seed";
/// Context key for the validate() result.
const VALIDATE_RESULT_KEY: &str = "ws_validate_result";

pub fn steps() -> Vec<StepDef> {
    vec![
        // ===== Given: load the track seed =====
        step_def(
            "the track seed",
            &[],
            &[(SEED_KEY, "PlaybookMachine")],
            |_ctx, _params| {
                let seed = track_seed().clone();
                let mut out = Context::new();
                out.set(SEED_KEY, seed);
                Ok(out)
            },
        ),
        // ===== Given: load the playbook seed =====
        step_def(
            "the playbook seed",
            &[],
            &[(SEED_KEY, "PlaybookMachine")],
            |_ctx, _params| {
                let seed = playbook_seed().clone();
                let mut out = Context::new();
                out.set(SEED_KEY, seed);
                Ok(out)
            },
        ),
        // ===== When: validate track seed with its migrated hook files =====
        // The track seed declares lifecycle hooks, so validation is passed the
        // same hook filenames present in the on-disk track_lifecycle hooks/ directory.
        step_def(
            "validate is called on the track seed with its hook files",
            &[],
            &[(VALIDATE_RESULT_KEY, "String")],
            |_ctx, _params| {
                let hook_files = vec![
                    "spec-writing.md".to_string(),
                    "spec-review.md".to_string(),
                    "spec-revision.md".to_string(),
                    "plan-writing.md".to_string(),
                    "plan-review.md".to_string(),
                    "implementing.md".to_string(),
                    "impl-phase-review.md".to_string(),
                    "impl-review.md".to_string(),
                    "reflecting.md".to_string(),
                    "reflection-review.md".to_string(),
                    "amend-writing.md".to_string(),
                    "amend-review.md".to_string(),
                    "complete.md".to_string(),
                ];
                let result = validate(track_seed(), &hook_files);
                let mut out = Context::new();
                match result {
                    Ok(()) => out.set(VALIDATE_RESULT_KEY, "Ok".to_string()),
                    Err(e) => out.set(VALIDATE_RESULT_KEY, format!("Err: {}", e)),
                }
                Ok(out)
            },
        ),
        // ===== When: validate playbook seed with empty hook files =====
        step_def(
            "validate is called on the playbook seed with empty hook files",
            &[],
            &[(VALIDATE_RESULT_KEY, "String")],
            |_ctx, _params| {
                let result = validate(playbook_seed(), &[]);
                let mut out = Context::new();
                match result {
                    Ok(()) => out.set(VALIDATE_RESULT_KEY, "Ok".to_string()),
                    Err(e) => out.set(VALIDATE_RESULT_KEY, format!("Err: {}", e)),
                }
                Ok(out)
            },
        ),
        // ===== Then: validate result is Ok =====
        check_def(
            "the validation result is Ok",
            &[(VALIDATE_RESULT_KEY, "String")],
            |ctx, _params| {
                let result = ctx
                    .get::<String>(VALIDATE_RESULT_KEY)
                    .ok_or("No validate result")?;
                if result == "Ok" {
                    Ok(())
                } else {
                    Err(format!("Expected Ok but got: {}", result))
                }
            },
        ),
        // ===== Then: seed has transition (from, to, role) =====
        check_def(
            "the track seed has transition from {string} to {string} with role {string}",
            &[(SEED_KEY, "PlaybookMachine")],
            |ctx, params| {
                let from = params
                    .get_string(0)
                    .ok_or("Expected from_state")?
                    .to_string();
                let to = params.get_string(1).ok_or("Expected to_state")?.to_string();
                let role = params
                    .get_string(2)
                    .ok_or("Expected required_role")?
                    .to_string();
                let seed = ctx.get::<PlaybookMachine>(SEED_KEY).ok_or("No seed")?;
                let found = seed
                    .transitions
                    .iter()
                    .any(|t| t.from_state == from && t.to_state == to && t.required_role == role);
                if found {
                    Ok(())
                } else {
                    Err(format!(
                        "No transition from '{}' to '{}' with role '{}' in track seed.\nTransitions: {:?}",
                        from, to, role,
                        seed.transitions.iter().map(|t| format!("{} → {} ({})", t.from_state, t.to_state, t.required_role)).collect::<Vec<_>>()
                    ))
                }
            },
        ),
        // ===== Then: seed has no transitions from state =====
        check_def(
            "the track seed has no transitions from {string}",
            &[(SEED_KEY, "PlaybookMachine")],
            |ctx, params| {
                let from = params
                    .get_string(0)
                    .ok_or("Expected from_state")?
                    .to_string();
                let seed = ctx.get::<PlaybookMachine>(SEED_KEY).ok_or("No seed")?;
                let outgoing: Vec<_> = seed
                    .transitions
                    .iter()
                    .filter(|t| t.from_state == from)
                    .collect();
                if outgoing.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected no transitions from '{}' but found {} transition(s): {:?}",
                        from,
                        outgoing.len(),
                        outgoing
                            .iter()
                            .map(|t| format!(
                                "{} → {} ({})",
                                t.from_state, t.to_state, t.required_role
                            ))
                            .collect::<Vec<_>>()
                    ))
                }
            },
        ),
    ]
}
