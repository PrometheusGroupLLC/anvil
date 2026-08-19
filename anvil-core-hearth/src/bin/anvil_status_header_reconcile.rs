//! `anvil-status-header-reconcile` — audit and repair the `state:` header that
//! every `status.yaml` carries, against the artifact's own transition events.
//!
//! # Why a backfill exists
//!
//! Before the fix in `hearth::status_header`, a transition wrote an event file
//! and never touched the header, so the header froze at its creation-time
//! value. Fixing the write path repairs nothing already on disk: an artifact
//! that transitioned 14 times still declares its first state. This binary is
//! the repair, and it is deliberately a SEPARATE, EXPLICIT step — governance
//! files are not silently rewritten by a process that was only asked to look.
//!
//! # Contract
//!
//! * **Report by default.** With no `--apply`, not one byte is written. The
//!   report is identical either way, so the dry run is a true preview.
//! * **Never silent.** Every artifact inspected is printed with its before →
//!   after. `--drifted-only` narrows the PRINTING, never the inspection, and
//!   the totals always count everything looked at.
//! * **Never guesses.** An artifact whose status.yaml or transition evidence
//!   cannot be read is reported as an ERROR and left alone — folding unreadable
//!   evidence away would write a stale state into the header and call it a
//!   repair. Errors set a non-zero exit code.
//! * **Removes staleness; does not arbitrate.** A header is rewritten only when
//!   it is provably a stale snapshot of the artifact's own transition log —
//!   engine-written events exist AND the header's value appears in that
//!   history — or when there is no header at all. Anything else is reported
//!   UNVERIFIABLE and left untouched. Both halves earned their place against
//!   live data: three foundry-hearth tracks declare `state: complete` over a
//!   hand-authored array that stops at `build`, and one declares
//!   `state: abandoned` with a single event (the creation seed to `spec`). A
//!   fold-wins reconciler would have rewritten all four backwards.
//!
//! # Usage
//!
//! ```text
//! anvil-status-header-reconcile --hearth <path> [--apply] [--drifted-only] [--json]
//! ```
//!
//! Exit codes: `0` — every inspected artifact resolved (whether or not it
//! drifted); `1` — at least one artifact could not be inspected; `2` — usage
//! error; `3` — the hearth itself could not be scanned.

use anvil_core_hearth::status_header::{
    reconcile_status_header, HeaderPolicy, HeaderReconciliation, HeaderVerdict, ReconcileMode,
    StatusHeaderError, STATUS_FILE,
};
use std::path::{Path, PathBuf};

fn main() {
    let mut hearth: Option<PathBuf> = None;
    let mut mode = ReconcileMode::Report;
    let mut drifted_only = false;
    let mut json = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--hearth" => hearth = args.next().map(PathBuf::from),
            "--apply" => mode = ReconcileMode::Apply,
            "--drifted-only" => drifted_only = true,
            "--json" => json = true,
            other => {
                eprintln!("anvil-status-header-reconcile: unknown argument {other:?}");
                std::process::exit(2);
            }
        }
    }
    let Some(hearth) = hearth else {
        eprintln!("anvil-status-header-reconcile: --hearth <path> is required");
        std::process::exit(2);
    };
    if !hearth.is_dir() {
        eprintln!(
            "anvil-status-header-reconcile: {} is not a directory",
            hearth.display()
        );
        std::process::exit(3);
    }

    let artifacts = match find_artifact_dirs(&hearth) {
        Ok(dirs) => dirs,
        Err(e) => {
            eprintln!("anvil-status-header-reconcile: cannot scan {}: {e}", hearth.display());
            std::process::exit(3);
        }
    };

    let mut clean = 0usize;
    let mut drifted: Vec<HeaderReconciliation> = Vec::new();
    let mut unverifiable: Vec<HeaderReconciliation> = Vec::new();
    let mut errors: Vec<(PathBuf, StatusHeaderError)> = Vec::new();
    let mut rows: Vec<serde_json::Value> = Vec::new();

    for dir in &artifacts {
        let rel = dir.strip_prefix(&hearth).unwrap_or(dir).display().to_string();
        match reconcile_status_header(dir, mode, HeaderPolicy::Conservative) {
            Ok(r) => {
                rows.push(serde_json::json!({
                    "artifact": rel,
                    "before": r.declared,
                    "after": r.resolved,
                    "verdict": match r.verdict {
                        HeaderVerdict::Agrees => "agrees",
                        HeaderVerdict::Drifted => "drifted",
                        HeaderVerdict::Unverifiable => "unverifiable",
                    },
                    "transition_events": r.event_count,
                    "written": r.written,
                }));
                match r.verdict {
                    HeaderVerdict::Drifted => drifted.push(r),
                    HeaderVerdict::Unverifiable => unverifiable.push(r),
                    HeaderVerdict::Agrees => {
                        clean += 1;
                        if !drifted_only && !json {
                            println!("  ok       {rel}  state: {}", r.resolved);
                        }
                    }
                }
            }
            Err(e) => {
                rows.push(serde_json::json!({
                    "artifact": rel,
                    "error": e.to_string(),
                }));
                errors.push((dir.clone(), e));
            }
        }
    }

    if json {
        let out = serde_json::json!({
            "hearth": hearth.display().to_string(),
            "mode": if mode == ReconcileMode::Apply { "apply" } else { "report" },
            "inspected": artifacts.len(),
            "clean": clean,
            "drifted": drifted.len(),
            "unverifiable": unverifiable.len(),
            "errors": errors.len(),
            "artifacts": rows,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else {
        let rel_of = |r: &HeaderReconciliation| {
            r.path
                .parent()
                .and_then(|p| p.strip_prefix(&hearth).ok())
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| r.path.display().to_string())
        };
        for r in &drifted {
            let verb = if r.written { "REPAIRED" } else { "DRIFT   " };
            println!(
                "  {verb} {}  {} -> {}  ({} events)",
                rel_of(r),
                r.declared.as_deref().unwrap_or("(no header)"),
                r.resolved,
                r.event_count
            );
        }
        // Never suppressed by --drifted-only: an unverifiable artifact is the
        // finding a human has to settle, and hiding it is how it stays wrong.
        for r in &unverifiable {
            println!(
                "  UNVERIFIABLE {}  header {} vs transition tail {} ({} events) — the header is \
                 not a stale value of this artifact's own log; left untouched",
                rel_of(r),
                r.declared.as_deref().unwrap_or("(no header)"),
                r.resolved,
                r.event_count
            );
        }
        for (dir, e) in &errors {
            println!("  ERROR    {}: {e}", dir.display());
        }
        println!(
            "\n{} hearth={}\ninspected={} clean={} drifted={} unverifiable={} errors={}",
            if mode == ReconcileMode::Apply {
                "APPLIED (headers rewritten)"
            } else {
                "REPORT ONLY (no bytes written; re-run with --apply to repair)"
            },
            hearth.display(),
            artifacts.len(),
            clean,
            drifted.len(),
            unverifiable.len(),
            errors.len(),
        );
    }

    if !errors.is_empty() {
        std::process::exit(1);
    }
}

/// Every directory under the hearth that holds a `status.yaml`.
///
/// Deliberately a full recursive walk rather than an allow-list of the six
/// legacy artifact directories: domain-machine kinds live in their own
/// directories (`playbooks/`, and whatever a kit contributes), and an
/// allow-list would report "0 drifted" for a whole class it never looked at.
/// What the walk DOES exclude, and why:
///
/// * `transitions/`, `.git/`, `target/`, and any dotted directory — the event
///   store and VCS/build noise, none of which is an artifact.
/// * `journal/` — the K8 in-flight commit staging area; a journaled copy of a
///   status.yaml is not the live one.
///
/// Anything else with a `status.yaml` is inspected, including nested artifacts.
fn find_artifact_dirs(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if dir.join(STATUS_FILE).is_file() {
            out.push(dir.clone());
        }
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "transitions" || name == "journal" || name == "target"
            {
                continue;
            }
            stack.push(entry.path());
        }
    }
    out.sort();
    Ok(out)
}
