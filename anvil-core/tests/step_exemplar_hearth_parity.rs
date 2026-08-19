//! Parity guard — every real `step-*.md` anchor under the track_lifecycle
//! hearth kind's `exemplars/` directory must parse via
//! `load_step_exemplar_from_markdown` without error.
//!
//! This is what proves the Rust `StepExemplar` schema admits exactly what
//! foundry-evaluation-hearth's `step_quality_grader.py` (`_load_exemplars` /
//! `parse_fm`) already consumes in production — not a redesign, a parity
//! check against real anchor files. Mirrors the skip-when-absent pattern in
//! `hearth_lint.rs` / `track_copies_parity.rs`: the live hearth is a sibling
//! checkout (`forge/` symlink -> ../anvil-hearth) that CI without the sibling
//! repo won't have.

use anvil_core::domain::playbook::exemplar_step::load_step_exemplar_from_markdown;
use std::path::PathBuf;

/// Resolve the live hearth's track_lifecycle exemplars directory via the
/// repo-standard `forge/` symlink, or `None` if the sibling checkout is
/// absent (mirrors `hearth_lint::live_hearth`).
fn live_step_exemplars_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../forge/playbooks/20260422T0000_track_lifecycle/exemplars");
    dir.is_dir().then(|| dir.canonicalize().unwrap_or(dir))
}

#[test]
fn every_real_step_exemplar_loads() {
    let Some(dir) = live_step_exemplars_dir() else {
        eprintln!(
            "step_exemplar_hearth_parity: no live hearth (forge/ absent, no sibling checkout) \
             — skipping"
        );
        return;
    };

    let mut loaded = 0usize;
    let mut failures = Vec::new();

    let entries = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read exemplars dir {}: {}", dir.display(), e));
    for entry in entries.flatten() {
        let path = entry.path();
        let is_step_file = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("step-") && n.ends_with(".md"));
        if !is_step_file {
            continue;
        }

        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
        match load_step_exemplar_from_markdown(&text) {
            Ok(_) => loaded += 1,
            Err(e) => failures.push(format!("{}: {}", path.display(), e)),
        }
    }

    assert!(
        failures.is_empty(),
        "{} real step-*.md anchor(s) failed to parse via load_step_exemplar_from_markdown \
         (the Rust StepExemplar schema has drifted from what step_quality_grader.py consumes):\n{:#?}",
        failures.len(),
        failures
    );
    assert!(
        loaded > 0,
        "expected at least one real step-*.md anchor under {} — found zero; the parity guard \
         would pass vacuously",
        dir.display()
    );
    eprintln!(
        "step_exemplar_hearth_parity: loaded {} real step exemplar(s) from {}",
        loaded,
        dir.display()
    );
}
