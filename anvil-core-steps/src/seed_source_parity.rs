//! Step module for `seed_outcome_predicate_parity.feature`.
//!
//! The free lifecycle kinds exist in TWO source tiers: the hearth
//! `playbooks/<kind>_lifecycle/machine.yaml` (loaded at runtime) and the
//! compiled-in `seeds::<kind>_seed()` fallback (used when no yaml is present).
//! The outcome-predicate fold resolves a kind's predicate from whichever tier
//! wins, so if the two tiers disagree, enforcement is NOT universal — a
//! seed-resolved instance could be ungradeable while the yaml one is graded, or
//! vice versa.
//!
//! This module guards that the backfilled `outcome_predicate` is IDENTICAL
//! across both tiers (generate-don't-duplicate parity), and that the seeds
//! carrying measured states pass the same measurement-enforcing validation the
//! yaml loader applies.

use anvil_core::domain::playbook::loader::validate_with_id_enforcing;
use anvil_core::domain::playbook::seeds::{
    decision_seed, initiative_seed, learning_seed, milestone_seed, proposal_seed, spark_seed,
};
use anvil_core::domain::playbook::types::PlaybookMachine;
use brine_runner_rust::registry::{check_def, StepDef};

fn seed_for(kind: &str) -> Result<&'static PlaybookMachine, String> {
    Ok(match kind {
        "decision" => decision_seed(),
        "initiative" => initiative_seed(),
        "learning" => learning_seed(),
        "milestone" => milestone_seed(),
        "proposal" => proposal_seed(),
        "spark" => spark_seed(),
        other => return Err(format!("unknown seed kind '{}'", other)),
    })
}

/// Raw-parse `playbooks/<kind>_lifecycle/machine.yaml` into a `PlaybookMachine`
/// WITHOUT hook/cross-reference validation, so the parity check reads the
/// declared `outcome_predicate` without needing the on-disk hooks/ listing.
fn yaml_machine_for(kind: &str) -> Result<PlaybookMachine, String> {
    // TEST_SUPPORT_DIR is <workspace>/anvil-test-support (this module lives in a sibling steps crate).
    let manifest_dir = std::path::Path::new(anvil_test_support::TEST_SUPPORT_DIR);
    let workspace_root = manifest_dir
        .parent()
        .ok_or("expected workspace root as parent of anvil-test-support")?;
    let path = workspace_root
        .join("playbooks")
        .join(format!("{}_lifecycle", kind))
        .join("machine.yaml");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("read {}: {}", path.display(), e))?;
    serde_yaml::from_str::<PlaybookMachine>(&text)
        .map_err(|e| format!("parse {}: {}", path.display(), e))
}

pub fn steps() -> Vec<StepDef> {
    vec![
        check_def(
            "the {string} seed and its lifecycle machine.yaml declare the same outcome_predicate",
            &[],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let seed = seed_for(&kind)?;
                let yaml = yaml_machine_for(&kind)?;
                let seed_pred = seed.outcome_predicate.as_ref();
                let yaml_pred = yaml.outcome_predicate.as_ref();
                if seed_pred.is_none() {
                    return Err(format!("the {} seed declares no outcome_predicate", kind));
                }
                if yaml_pred.is_none() {
                    return Err(format!(
                        "the {} machine.yaml declares no outcome_predicate",
                        kind
                    ));
                }
                if seed_pred == yaml_pred {
                    Ok(())
                } else {
                    Err(format!(
                        "outcome_predicate drift for {}:\n  seed: {:?}\n  yaml: {:?}",
                        kind, seed_pred, yaml_pred
                    ))
                }
            },
        ),
        check_def(
            "the {string} seed passes measurement enforcement",
            &[],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let seed = seed_for(&kind)?;
                // Feed the seed's OWN hook filenames so the enforcing validation
                // exercises the measurement-definition gate, not a hook-file miss.
                let hook_files: Vec<String> = seed
                    .states
                    .iter()
                    .flat_map(|s| {
                        s.hook
                            .iter()
                            .cloned()
                            .chain(s.hooks_by_role.values().cloned())
                    })
                    .collect();
                validate_with_id_enforcing(seed, &seed.kind, &hook_files)
                    .map_err(|e| format!("the {} seed fails measurement enforcement: {}", kind, e))
            },
        ),
    ]
}
