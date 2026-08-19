//! Hearth lint — the "loads-gate" from the 2026-07-06 fleet measurement review.
//!
//! A playbook whose machine.yaml fails the loader never enters the registry
//! ("active candidacy = registry-resolvability"), so every gate, measurement,
//! and rubric on it is counterfactual. This gate makes that failure LOUD at
//! `cargo test` time instead of silent at route time.
//!
//! Two invariants over the LIVE workspace hearth (skips cleanly when absent —
//! CI without the sibling checkout):
//!   1. ZERO invalid artifacts — every machine.yaml the hearth ships must load.
//!   2. Review-gate ratchet — every `*_review` state must declare
//!      `is_review_gate: true` (which activates the loader's
//!      required-satisfaction-on-every-exit invariant), EXCEPT the documented
//!      allowlist of legacy-encoded gates pending their migration track.
//!
//! Allowlist policy: entries may only be REMOVED (ratchet). Each entry names
//! the migration that retires it.

use anvil_core::domain::playbook::hearth_registry::HearthPlaybookRegistry;
use anvil_core::domain::playbook::integrity::playbook_integrity;
use anvil_core::domain::playbook::registry::PlaybookRegistry;
use std::path::PathBuf;

/// Resolve the live hearth: `ANVIL_HEARTH_LINT_PATH` override, else the
/// repo-standard `forge/` symlink (-> ../anvil-hearth), else skip.
fn live_hearth() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ANVIL_HEARTH_LINT_PATH") {
        let p = PathBuf::from(p);
        return p.join("playbooks").is_dir().then_some(p);
    }
    let forge = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../forge");
    forge
        .join("playbooks")
        .is_dir()
        .then(|| forge.canonicalize().unwrap_or(forge))
}

/// Legacy `*_review` states still `is_review_gate: false`, pending their
/// satisfaction-encoding migration. EMPTY as of the track-lifecycle
/// satisfaction-encoding reconciliation (2026-07-07): track's 6 gates are now
/// `is_review_gate: true` with satisfaction tokens on every exit, so the ratchet
/// holds fleet-wide with ZERO exceptions. Entries may only be ADDED with a named
/// retirement migration, and removed when that migration lands.
const RATCHET_ALLOWLIST: &[(&str, &str)] = &[];

#[test]
fn hearth_zero_invalid_artifacts() {
    let Some(hearth) = live_hearth() else {
        eprintln!("hearth_lint: no live hearth (forge/ absent, no override) — skipping");
        return;
    };
    let registry = HearthPlaybookRegistry::new(hearth.clone());
    let errors = registry.invalid_artifacts();
    assert!(
        errors.is_empty(),
        "hearth {} ships {} loader-INVALID machine(s) — every gate/measurement/rubric on them is \
         counterfactual until they load:\n{:#?}",
        hearth.display(),
        errors.len(),
        errors
    );
}

#[test]
fn hearth_review_gate_ratchet() {
    let Some(hearth) = live_hearth() else {
        eprintln!("hearth_lint: no live hearth — skipping");
        return;
    };
    let registry = HearthPlaybookRegistry::new(hearth);
    let mut violations = Vec::new();
    // Rubber-stamp detection is the SHARED `playbook_integrity` fold (one
    // implementation, no drift with the atlas surface). On an all-valid live
    // hearth the fleet definition (`is_review_gate:false` OR a null-satisfaction
    // exit) reduces to the narrow `is_review_gate:false` check, because the
    // loader guarantees every exit of an `is_review_gate:true` state carries a
    // non-null satisfaction. The allowlist stays HERE, in the test assertion
    // only — the fold (and the atlas) surface every rubber stamp incl. `track`.
    for machine in registry.all_machines() {
        for gate in playbook_integrity(machine).rubber_stamp_gates {
            let allowlisted = RATCHET_ALLOWLIST
                .iter()
                .any(|(k, s)| *k == machine.kind && *s == gate);
            if !allowlisted {
                violations.push(format!("{}::{}", machine.kind, gate));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "{} review-looking state(s) declare is_review_gate:false (reviewer verdict cannot be \
         loader-enforced — rubber-stamp risk). Either flip is_review_gate:true (and give every \
         exit a required_satisfaction token) or add to the allowlist WITH a named migration:\n{:#?}",
        violations.len(),
        violations
    );
}
