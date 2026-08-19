//! Regenerate the track_lifecycle fixture from the canonical seed, so the
//! parity guard (seed ↔ fixture structural equality) holds by construction after
//! the satisfaction-encoding reconciliation. Serializes `track_seed()` to YAML,
//! re-parses via the real loader, asserts struct equality, then writes the file.
//!
//! Run: `cargo run --example regen_track_fixture -p anvil-core`

use anvil_core::domain::playbook::loader::load_from_yaml;
use anvil_core::domain::playbook::seeds::track_seed;

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/track_lifecycle_machine.yaml"
);

fn main() {
    let seed = track_seed().clone();
    let yaml = serde_yaml::to_string(&seed).expect("serialize seed to yaml");

    // The loader validates hook references against the on-disk hooks/ listing —
    // mirror the equivalence test's hook_files set.
    let hook_files: Vec<String> = [
        "spec-writing.md", "spec-review.md", "spec-revision.md", "plan-writing.md",
        "plan-review.md", "implementing.md", "impl-phase-review.md", "impl-review.md",
        "reflecting.md", "reflection-review.md", "amend-writing.md", "amend-review.md",
        "complete.md",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    let reparsed = load_from_yaml("track_lifecycle_machine.yaml", &yaml, &hook_files)
        .expect("serialized seed must re-parse via the real loader");

    assert!(
        reparsed == seed,
        "round-trip mismatch: serialized seed did not re-parse to an equal struct"
    );

    std::fs::write(FIXTURE, &yaml).expect("write fixture");
    println!(
        "regenerated {} ({} bytes); round-trip struct-equal to track_seed()",
        FIXTURE,
        yaml.len()
    );
}
