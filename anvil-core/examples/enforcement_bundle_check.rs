//! Enforcement-load conformance check for a bundle of playbook machines.
//!
//! Thin CLI wrapper around
//! [`anvil_core::domain::enforcement_bundle_check::evaluate_enforcement_bundle`]:
//! it prints a human-readable report and exits with the outcome's stable
//! [`BundleCheckOutcome::exit_code`]. The decision logic (and its three-way
//! exit contract) lives in the library so it is exercised by a wired
//! `.feature` (`enforcement_bundle_check.feature`), not only by this binary.
//!
//! This is the diagnostic behind two guarantees:
//!   * the backfill verify step (run against the repo's `playbooks/` source), and
//!   * the build-kit conformance gate (run against the staged `dist/anvil-kit/`),
//!     which makes it structurally impossible to ship a half-migrated kit where
//!     an unbackfilled kind silently drops out of the registry.
//!
//! Exit statuses (STABLE — build-kit.sh maps ONLY `1` to its publish-blocking
//! exit `3`):
//!   0 = every staged machine registers under enforcement
//!   1 = a genuine loader-drop (a machine failed measurement enforcement)
//!   2 = setup failure (no playbooks/ dir, or it could not be read)
//!
//! Usage:
//!   cargo run --example enforcement_bundle_check -p anvil-core -- <root>
//!
//! `<root>` is the directory that CONTAINS a `playbooks/` subdir (the repo root,
//! or the assembled kit root). Defaults to the anvil repo root inferred from
//! this crate's manifest dir when no argument is given.

use std::path::PathBuf;
use std::process::exit;

use anvil_core::domain::enforcement_bundle_check::{evaluate_enforcement_bundle, BundleCheckOutcome};

fn main() {
    let root: PathBuf = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            // anvil-core/ -> repo root
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."))
        });

    println!("== enforcement bundle check ==");
    println!("root:      {}", root.display());
    println!("playbooks: {}", root.join("playbooks").display());

    let outcome = evaluate_enforcement_bundle(&root);
    match &outcome {
        BundleCheckOutcome::Pass { staged, loaded } => {
            println!("staged machines ({}): {:?}", staged.len(), staged);
            println!("\nloaded under enforcement ({} kind(s)):", loaded.len());
            for k in loaded {
                println!("    OK  {}", k);
            }
            println!(
                "\n\u{2713} PASS — all {} staged machine(s) load under ANVIL_ENFORCE_MEASUREMENT_DEFINITION=1",
                staged.len()
            );
        }
        BundleCheckOutcome::Drop {
            staged,
            loaded,
            dropped,
            invalid,
        } => {
            println!("staged machines ({}): {:?}", staged.len(), staged);
            println!("\nloaded under enforcement ({} kind(s)):", loaded.len());
            for k in loaded {
                println!("    OK  {}", k);
            }
            if !dropped.is_empty() {
                println!("\nmeasurement-dropped artifacts ({}):", dropped.len());
                for d in dropped {
                    println!("    DROP  {}", d);
                }
            }
            if !invalid.is_empty() {
                println!("\ninvalid artifacts ({}):", invalid.len());
                for err in invalid {
                    println!("    DROP  {}", err);
                }
            }
            eprintln!(
                "\n\u{2717} FAIL — {} staged machine(s) but only {} load under enforcement; {} load error(s).",
                staged.len(),
                loaded.len(),
                invalid.len()
            );
            eprintln!("   A half-migrated kit would ship broken lifecycles. Backfill success_criteria + outcome_predicate.");
        }
        BundleCheckOutcome::Setup { reason } => {
            eprintln!("FAIL (setup): {}", reason);
        }
    }

    exit(outcome.exit_code());
}
