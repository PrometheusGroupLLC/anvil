//! Step module for `playbook_seed_yaml_equivalence.feature`.
//!
//! Provides steps for:
//! - Loading the track lifecycle machine.yaml from the checked-in fixture under
//!   `anvil-core/tests/fixtures/track_lifecycle_machine.yaml`
//! - Loading the compiled-in `seeds::track_seed()`
//! - Asserting structural equality between the two `PlaybookMachine` values
//!
//! The fixture is a copy of the Phase 1 hearth artifact. If the seed and the
//! YAML drift, this scenario fails — alerting the implementer to update the
//! fixture or the seed.

use anvil_core::domain::playbook::loader::load_from_yaml;
use anvil_core::domain::playbook::seeds::track_seed;
use anvil_core::domain::playbook::types::PlaybookMachine;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

/// Context key: the machine loaded from the fixture file.
const FIXTURE_MACHINE_KEY: &str = "seq_fixture_machine";
/// Context key: the machine loaded from the compiled-in seed.
const SEED_MACHINE_KEY: &str = "seq_seed_machine";

/// Absolute path to the fixture file, resolved at compile time via `CARGO_MANIFEST_DIR`.
///
/// `CARGO_MANIFEST_DIR` is set to the manifest directory of the *calling test
/// binary* by cargo — for `anvil-core/tests/brine_runner.rs`, that is
/// `<workspace>/anvil-core`. The fixture therefore lives at
/// `<workspace>/anvil-core/tests/fixtures/track_lifecycle_machine.yaml`.
fn fixture_path() -> std::path::PathBuf {
    // Locate the fixture relative to the workspace root.
    // `CARGO_MANIFEST_DIR` for `anvil-test-support` is `<workspace>/anvil-test-support`.
    // Walk up one directory to reach the workspace root, then descend into
    // `anvil-core/tests/fixtures/`.
    let manifest_dir = std::path::Path::new(anvil_test_support::TEST_SUPPORT_DIR);
    let workspace_root = manifest_dir
        .parent()
        .expect("expected workspace root to be parent of anvil-test-support");
    workspace_root
        .join("anvil-core")
        .join("tests")
        .join("fixtures")
        .join("track_lifecycle_machine.yaml")
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ===== Given: load track lifecycle machine.yaml from fixture =====
        step_def(
            "the track lifecycle machine.yaml is loaded from the fixture",
            &[],
            &[(FIXTURE_MACHINE_KEY, "PlaybookMachine")],
            |_ctx, _params| {
                let path = fixture_path();
                let yaml_text = std::fs::read_to_string(&path)
                    .map_err(|e| format!("Failed to read fixture at {}: {}", path.display(), e))?;
                // P5: the fixture now references the migrated (spec, doer) and
                // (spec_review, reviewer) hooks. Pass those filenames so the
                // loader's hook-reference validation passes — mirroring the
                // on-disk hooks/ listing for the track_lifecycle playbook.
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
                let machine =
                    load_from_yaml("track_lifecycle_machine.yaml", &yaml_text, &hook_files)
                        .map_err(|e| format!("Failed to parse fixture: {}", e))?;
                let mut out = Context::new();
                out.set(FIXTURE_MACHINE_KEY, machine);
                Ok(out)
            },
        ),
        // ===== And: load track seed from compiled-in seed =====
        // Requires FIXTURE_MACHINE_KEY to carry it forward into the assertion step.
        step_def(
            "the track seed is loaded from the compiled-in seed",
            &[(FIXTURE_MACHINE_KEY, "PlaybookMachine")],
            &[
                (FIXTURE_MACHINE_KEY, "PlaybookMachine"),
                (SEED_MACHINE_KEY, "PlaybookMachine"),
            ],
            |ctx, _params| {
                let fixture = ctx
                    .get::<PlaybookMachine>(FIXTURE_MACHINE_KEY)
                    .ok_or("No fixture machine in context")?
                    .clone();
                let seed: PlaybookMachine = track_seed().clone();
                let mut out = Context::new();
                out.set(FIXTURE_MACHINE_KEY, fixture);
                out.set(SEED_MACHINE_KEY, seed);
                Ok(out)
            },
        ),
        // ===== Then: the two PlaybookMachine structs are structurally equal =====
        check_def(
            "the two PlaybookMachine structs are structurally equal",
            &[
                (FIXTURE_MACHINE_KEY, "PlaybookMachine"),
                (SEED_MACHINE_KEY, "PlaybookMachine"),
            ],
            |ctx, _params| {
                let fixture = ctx
                    .get::<PlaybookMachine>(FIXTURE_MACHINE_KEY)
                    .ok_or("No fixture machine in context")?;
                let seed = ctx
                    .get::<PlaybookMachine>(SEED_MACHINE_KEY)
                    .ok_or("No seed machine in context")?;
                if fixture == seed {
                    Ok(())
                } else {
                    Err(format!(
                        "Fixture and seed PlaybookMachine are not equal.\n\
                         Fixture kind={} states={} transitions={}\n\
                         Seed    kind={} states={} transitions={}",
                        fixture.kind,
                        fixture.states.len(),
                        fixture.transitions.len(),
                        seed.kind,
                        seed.states.len(),
                        seed.transitions.len(),
                    ))
                }
            },
        ),
    ]
}
