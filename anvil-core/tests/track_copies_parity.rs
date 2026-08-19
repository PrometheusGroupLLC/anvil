//! Parity guard — every ON-DISK copy of the track_lifecycle machine.yaml must
//! parse struct-equal to the canonical `track_seed()`.
//!
//! The fleet review found the four copies had DIVERGED (the production hearth
//! copy carried the rubric but no tokens; the seed/fixture/kit carried tokens
//! but no rubric). The satisfaction-encoding reconciliation made the seed the
//! single source of truth and regenerated the on-disk copies from it. This
//! guard is what keeps them from drifting again:
//!   - the KIT source copy (`playbooks/track_lifecycle/machine.yaml`) is in-repo
//!     and always checked → a hard assertion.
//!   - the PRODUCTION hearth copy (via the `forge/` symlink → ../anvil-hearth)
//!     is a sibling checkout → asserted when present, skipped when absent (CI
//!     without the sibling repo), mirroring `hearth_lint`.
//!
//! The existing `playbook_seed_yaml_equivalence` brine feature covers
//! seed ↔ fixture; this adds seed ↔ kit and seed ↔ hearth.

use anvil_core::domain::playbook::loader::load_from_yaml;
use anvil_core::domain::playbook::seeds::track_seed;
use std::path::PathBuf;

fn hook_files() -> Vec<String> {
    [
        "spec-writing.md", "spec-review.md", "spec-revision.md", "plan-writing.md",
        "plan-review.md", "implementing.md", "impl-phase-review.md", "impl-review.md",
        "reflecting.md", "reflection-review.md", "amend-writing.md", "amend-review.md",
        "complete.md",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Assert the machine.yaml at `path` parses struct-equal to `track_seed()`.
fn assert_copy_equals_seed(path: &PathBuf, label: &str) {
    let yaml = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read {} copy at {}: {}", label, path.display(), e));
    let machine = load_from_yaml("track_lifecycle_machine.yaml", &yaml, &hook_files())
        .unwrap_or_else(|e| panic!("parse {} copy at {}: {:?}", label, path.display(), e));
    assert!(
        machine == *track_seed(),
        "{} copy at {} has DRIFTED from the canonical track_seed() — regenerate it from the seed \
         (cargo run --example regen_track_copies / regen_track_fixture)",
        label,
        path.display()
    );
}

#[test]
fn kit_source_copy_equals_seed() {
    // CARGO_MANIFEST_DIR = <workspace>/anvil-core → kit source is ../playbooks/…
    let kit = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../playbooks/track_lifecycle/machine.yaml");
    assert!(
        kit.is_file(),
        "kit source copy missing at {} — it is in-repo and must exist",
        kit.display()
    );
    assert_copy_equals_seed(&kit, "kit-source");
}

#[test]
fn production_hearth_copy_equals_seed() {
    // The live hearth via the repo-standard forge/ symlink (→ ../anvil-hearth).
    let hearth = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../forge/playbooks/20260422T0000_track_lifecycle/machine.yaml");
    if !hearth.is_file() {
        eprintln!(
            "track_copies_parity: hearth copy absent at {} (no sibling checkout) — skipping",
            hearth.display()
        );
        return;
    }
    assert_copy_equals_seed(&hearth, "production-hearth");
}
