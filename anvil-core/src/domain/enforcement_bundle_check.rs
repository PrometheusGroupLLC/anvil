//! Enforcement-load conformance check for a bundle of playbook machines.
//!
//! Loads every `{root}/playbooks/*/machine.yaml` through the ENFORCING loader
//! (`HearthPlaybookRegistry::new_enforcing`, the same path the engine takes when
//! `ANVIL_ENFORCE_MEASUREMENT_DEFINITION=1`) and reports whether EVERY staged
//! machine registers. A machine that fails enforcement (a measured state
//! lacking `success_criteria`, or a missing/blank `outcome_predicate`) is
//! DROPPED from the registry — silently, without this gate.
//!
//! The decision logic lives here (in the library) rather than in the
//! `enforcement_bundle_check` example so it is exercised by a wired `.feature`
//! (`enforcement_bundle_check.feature`) instead of only by a shell invocation.
//! The example is a thin wrapper: it calls [`evaluate_enforcement_bundle`],
//! prints the human-readable report, and exits with [`BundleCheckOutcome::exit_code`].
//!
//! The three DISTINCT exit statuses are a contract the `build-kit.sh` gate
//! depends on: it maps a genuine loader-drop (`1`) — and ONLY that — to its own
//! publish-blocking exit `3`, while a setup failure (`2`) stays a setup failure.

use crate::domain::playbook::hearth_registry::HearthPlaybookRegistry;
use crate::domain::playbook::registry::PlaybookRegistry;
use std::collections::BTreeSet;
use std::path::Path;

/// The verdict of an enforcement-bundle check over a `{root}/playbooks/` tree.
///
/// Each variant maps to exactly one process exit status via [`Self::exit_code`]
/// — the contract the build gate keys off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleCheckOutcome {
    /// Every staged machine dir registered under enforcement.
    Pass {
        /// Machine directory names found under `playbooks/`.
        staged: Vec<String>,
        /// Registry kinds that loaded under enforcement.
        loaded: Vec<String>,
    },
    /// At least one staged machine failed to register under enforcement — a
    /// genuine loader-drop. `dropped` names the artifact ids the
    /// measurement-definition gate rejected; `invalid` is every load-error
    /// string (a superset of `dropped`).
    Drop {
        staged: Vec<String>,
        loaded: Vec<String>,
        dropped: Vec<String>,
        invalid: Vec<String>,
    },
    /// The check could not run: no `playbooks/` dir, it was unreadable, or it
    /// held no `machine.yaml` files. Distinct from a drop — nothing was judged.
    Setup {
        /// Human-readable reason the check could not run.
        reason: String,
    },
}

impl BundleCheckOutcome {
    /// The process exit status this outcome maps to. STABLE contract:
    /// `0` = pass, `1` = genuine loader-drop, `2` = setup failure.
    pub fn exit_code(&self) -> i32 {
        match self {
            BundleCheckOutcome::Pass { .. } => 0,
            BundleCheckOutcome::Drop { .. } => 1,
            BundleCheckOutcome::Setup { .. } => 2,
        }
    }

    /// True when every staged machine registered under enforcement.
    pub fn is_pass(&self) -> bool {
        matches!(self, BundleCheckOutcome::Pass { .. })
    }

    /// True when at least one staged machine dropped under enforcement.
    pub fn is_drop(&self) -> bool {
        matches!(self, BundleCheckOutcome::Drop { .. })
    }

    /// True when the check could not run (setup failure).
    pub fn is_setup(&self) -> bool {
        matches!(self, BundleCheckOutcome::Setup { .. })
    }
}

/// Evaluate the enforcement-bundle check over `root` (the dir CONTAINING a
/// `playbooks/` subdir — the repo root or an assembled kit root).
///
/// Reads the staged machine directories, then loads them all through the
/// ENFORCING loader and compares: every staged dir must yield a registered
/// kind AND `invalid_artifacts()` must be empty. Any staged dir that fails
/// enforcement leaves fewer kinds than staged dirs and an entry in
/// `invalid_artifacts()` — either signal produces [`BundleCheckOutcome::Drop`].
pub fn evaluate_enforcement_bundle(root: &Path) -> BundleCheckOutcome {
    let playbooks_dir = root.join("playbooks");

    let mut staged: BTreeSet<String> = BTreeSet::new();
    match std::fs::read_dir(&playbooks_dir) {
        Ok(iter) => {
            for entry in iter.flatten() {
                let p = entry.path();
                if p.is_dir() && p.join("machine.yaml").is_file() {
                    if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                        staged.insert(name.to_string());
                    }
                }
            }
        }
        Err(e) => {
            return BundleCheckOutcome::Setup {
                reason: format!("cannot read {}: {}", playbooks_dir.display(), e),
            };
        }
    }

    if staged.is_empty() {
        return BundleCheckOutcome::Setup {
            reason: format!(
                "no playbook machine.yaml files found under {}",
                playbooks_dir.display()
            ),
        };
    }

    // Enforcing load — the exact path the engine takes with enforcement on.
    let enforcing = HearthPlaybookRegistry::new_enforcing(root.to_path_buf());
    let loaded: Vec<String> = enforcing.kinds();
    let dropped: Vec<String> = enforcing
        .measurement_dropped_artifacts()
        .into_iter()
        .map(|s| s.to_string())
        .collect();
    let invalid: Vec<String> = enforcing
        .invalid_artifacts()
        .iter()
        .map(|e| e.to_string())
        .collect();

    let staged_vec: Vec<String> = staged.iter().cloned().collect();

    if invalid.is_empty() && loaded.len() == staged.len() {
        BundleCheckOutcome::Pass {
            staged: staged_vec,
            loaded,
        }
    } else {
        BundleCheckOutcome::Drop {
            staged: staged_vec,
            loaded,
            dropped,
            invalid,
        }
    }
}
