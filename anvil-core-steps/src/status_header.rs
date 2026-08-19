//! Step definitions for `status_header_projection.feature` — the `state:`
//! header in `status.yaml` as a derived projection of `<artifact>/transitions/`.
//!
//! These steps drive the REAL adapters against a REAL temp hearth: transitions
//! go through `FileSystemSnapshotAdapter::append_transition` (the production
//! write path), state is read back through
//! `SnapshotPort::read_artifact_state` (the production read seam), and the
//! header is asserted by reading the bytes on disk. Nothing here is a stand-in.
//!
//! The seeded artifact is always `tracks/20260806T0100_header_track`.

use anvil_core::domain::shared_types::TransitionContent;
use anvil_core_hearth::fs_snapshot_adapter::FileSystemSnapshotAdapter;
use anvil_core::domain::status_header::declared_state_header;
use anvil_core_hearth::status_header::{
    reconcile_status_header, HeaderPolicy, HeaderVerdict, ReconcileMode,
};
use anvil_core::ports::snapshot_port::SnapshotPort;
use anvil_core::ports::transition_event_write_port::TRANSITIONS_DIR;
use anvil_test_support::{carry_retained_temp_dir, retained_temp_dir};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;

const TRACK_REL: &str = "tracks/20260806T0100_header_track";

fn track_dir(hearth: &PathBuf) -> PathBuf {
    hearth.join(TRACK_REL)
}

fn status_text(hearth: &PathBuf) -> Result<String, String> {
    std::fs::read_to_string(track_dir(hearth).join("status.yaml"))
        .map_err(|e| format!("read status.yaml: {e}"))
}

/// Seed a hearth whose track carries `status.yaml` (with or without a header)
/// plus, unless `seed_event` is false, the creation-seed transition event that
/// a real artifact gets at genesis.
fn seed(state: &str, with_header: bool, seed_event: bool) -> Result<(Context, PathBuf), String> {
    let (handle, hearth) = retained_temp_dir("anvil-status-header-")?;
    let dir = hearth.join(TRACK_REL);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create track dir: {e}"))?;
    let header = if with_header {
        format!("state: {state}\n")
    } else {
        String::new()
    };
    // The shape `build_initial_artifact_status_yaml` writes: version, kind,
    // header, an actors block, an EMPTY legacy transitions array.
    let status = format!(
        "version: 1\nkind: track\n{header}origin_turn: header-projection\nactors: {{}}\ntransitions:\n"
    );
    std::fs::write(dir.join("status.yaml"), status)
        .map_err(|e| format!("write status.yaml: {e}"))?;
    std::fs::write(dir.join("spec.md"), "# Header Track\n\nBody.\n")
        .map_err(|e| format!("write spec.md: {e}"))?;
    if seed_event {
        let adapter = FileSystemSnapshotAdapter::new(hearth.clone());
        adapter
            .append_transition(
                TRACK_REL,
                &TransitionContent {
                    to: state.to_string(),
                    at: "2026-08-06T00:00:00Z".to_string(),
                    actor: "Author-000001".to_string(),
                    role: "spec".to_string(),
                    approver: None,
                    note: None,
                    satisfaction: None,
                    event_type: None,
                },
            )
            .map_err(|e| format!("creation-seed transition: {e}"))?;
    }
    let mut out = Context::new();
    out.set("sh_hearth", hearth.clone());
    out.set("sh_hearth_handle", handle);
    Ok((out, hearth))
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a status-header hearth with a track created at {string}",
            &[],
            &[
                ("sh_hearth", "PathBuf"),
                ("sh_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let state = params.get_string(0).ok_or("Expected state")?.to_string();
                let (out, _) = seed(&state, true, true)?;
                Ok(out)
            },
        ),
        step_def(
            "a status-header hearth with a track created at {string} and no state header",
            &[],
            &[
                ("sh_hearth", "PathBuf"),
                ("sh_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let state = params.get_string(0).ok_or("Expected state")?.to_string();
                let (out, _) = seed(&state, false, true)?;
                Ok(out)
            },
        ),
        step_def(
            "a status-header hearth with a track that has no state header and no transitions",
            &[],
            &[
                ("sh_hearth", "PathBuf"),
                ("sh_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, _params| {
                let (out, _) = seed("spec", false, false)?;
                Ok(out)
            },
        ),
        // The shape that made `HeaderVerdict::Unverifiable` mandatory, taken
        // from `foundry-hearth/tracks/20260702T2010_socket_activation_for_kit_shims`:
        // a hand-authored status.yaml whose header is AHEAD of its legacy
        // `transitions:` array (the final state change was recorded only as an
        // `activity:` marker), and NO `transitions/` event store.
        step_def(
            "a status-header hearth with a legacy-only track whose header {string} is ahead of its array tail {string}",
            &[],
            &[
                ("sh_hearth", "PathBuf"),
                ("sh_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let header = params.get_string(0).ok_or("Expected header")?.to_string();
                let tail = params.get_string(1).ok_or("Expected tail")?.to_string();
                let (handle, hearth) = retained_temp_dir("anvil-status-header-")?;
                let dir = hearth.join(TRACK_REL);
                std::fs::create_dir_all(&dir).map_err(|e| format!("create track dir: {e}"))?;
                let status = format!(
                    "version: 1\nkind: track\nstate: {header}\ntransitions:\n  - to: spec\n    \
                     at: \"2026-08-06T00:00:00Z\"\n  - to: {tail}\n    at: \
                     \"2026-08-06T01:00:00Z\"\nactivity:\n  - kind: complete\n    actor: \
                     Doer-222222\n    state: {header}\n    at: \"2026-08-06T02:00:00Z\"\n"
                );
                std::fs::write(dir.join("status.yaml"), status)
                    .map_err(|e| format!("write status.yaml: {e}"))?;
                let mut out = Context::new();
                out.set("sh_hearth", hearth);
                out.set("sh_hearth_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a status-header hearth with an artifact directory that has no status.yaml",
            &[],
            &[
                ("sh_hearth", "PathBuf"),
                ("sh_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, _params| {
                let (out, hearth) = seed("spec", true, false)?;
                std::fs::remove_file(track_dir(&hearth).join("status.yaml"))
                    .map_err(|e| format!("remove status.yaml: {e}"))?;
                Ok(out)
            },
        ),
        // The production write path, exactly as the engine calls it.
        step_def(
            "the artifact transitions to {string} at {string}",
            &[("sh_hearth", "PathBuf")],
            &[
                ("sh_hearth", "PathBuf"),
                ("sh_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let to = params.get_string(0).ok_or("Expected to")?.to_string();
                let at = params.get_string(1).ok_or("Expected at")?.to_string();
                let hearth = ctx.get::<PathBuf>("sh_hearth").ok_or("No sh_hearth")?.clone();
                FileSystemSnapshotAdapter::new(hearth.clone())
                    .append_transition(
                        TRACK_REL,
                        &TransitionContent {
                            to,
                            at,
                            actor: "Doer-222222".to_string(),
                            role: "advance".to_string(),
                            approver: None,
                            note: None,
                            satisfaction: None,
                            event_type: None,
                        },
                    )
                    .map_err(|e| format!("append_transition: {e}"))?;
                let mut out = Context::new();
                out.set("sh_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "sh_hearth_handle");
                Ok(out)
            },
        ),
        // The adversarial edit: a human (or a bad merge) writes a state the
        // events do not support.
        step_def(
            "the status.yaml state header is hand-edited to {string}",
            &[("sh_hearth", "PathBuf")],
            &[
                ("sh_hearth", "PathBuf"),
                ("sh_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let forged = params.get_string(0).ok_or("Expected state")?.to_string();
                let hearth = ctx.get::<PathBuf>("sh_hearth").ok_or("No sh_hearth")?.clone();
                let content = status_text(&hearth)?;
                let edited: String = content
                    .lines()
                    .map(|l| {
                        if l.starts_with("state:") {
                            format!("state: {forged}")
                        } else {
                            l.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\n";
                if declared_state_header(&edited).as_deref() != Some(forged.as_str()) {
                    return Err(format!(
                        "hand-edit did not take: header is {:?}",
                        declared_state_header(&edited)
                    ));
                }
                std::fs::write(track_dir(&hearth).join("status.yaml"), edited)
                    .map_err(|e| format!("write hand-edited status.yaml: {e}"))?;
                let mut out = Context::new();
                out.set("sh_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "sh_hearth_handle");
                Ok(out)
            },
        ),
        step_def(
            "the header reconciler runs in report mode",
            &[("sh_hearth", "PathBuf")],
            &[
                ("sh_hearth", "PathBuf"),
                ("sh_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("sh_declared", "String"),
                ("sh_resolved", "String"),
                ("sh_verdict", "String"),
                ("sh_written", "String"),
            ],
            |ctx, _params| run_reconcile(ctx, ReconcileMode::Report),
        ),
        step_def(
            "the header reconciler runs in apply mode",
            &[("sh_hearth", "PathBuf")],
            &[
                ("sh_hearth", "PathBuf"),
                ("sh_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("sh_declared", "String"),
                ("sh_resolved", "String"),
                ("sh_verdict", "String"),
                ("sh_written", "String"),
            ],
            |ctx, _params| run_reconcile(ctx, ReconcileMode::Apply),
        ),
        // The refusal arm: the reconciler must ERROR, never quietly project a
        // state it could not derive.
        step_def(
            "the header reconciler runs in apply mode and is expected to refuse",
            &[("sh_hearth", "PathBuf")],
            &[
                ("sh_hearth", "PathBuf"),
                ("sh_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("sh_error", "String"),
            ],
            |ctx, _params| {
                let hearth = ctx.get::<PathBuf>("sh_hearth").ok_or("No sh_hearth")?.clone();
                let outcome = reconcile_status_header(
                    &track_dir(&hearth),
                    ReconcileMode::Apply,
                    HeaderPolicy::Conservative,
                );
                let mut out = Context::new();
                out.set("sh_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "sh_hearth_handle");
                match outcome {
                    Ok(r) => Err(format!(
                        "expected a refusal, got a reconciliation: {:?} -> {}",
                        r.declared, r.resolved
                    )),
                    Err(e) => {
                        out.set("sh_error", e.to_string());
                        Ok(out)
                    }
                }
            },
        ),
        check_def(
            "the status.yaml state header is {string}",
            &[("sh_hearth", "PathBuf")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?;
                let hearth = ctx.get::<PathBuf>("sh_hearth").ok_or("No sh_hearth")?;
                let content = status_text(hearth)?;
                match declared_state_header(&content) {
                    Some(actual) if actual == expected.as_ref() as &str => Ok(()),
                    other => Err(format!(
                        "Expected top-level 'state: {}', found {:?}. status.yaml:\n{}",
                        expected, other, content
                    )),
                }
            },
        ),
        check_def(
            "the state resolved through the engine seam is {string}",
            &[("sh_hearth", "PathBuf")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?;
                let hearth = ctx.get::<PathBuf>("sh_hearth").ok_or("No sh_hearth")?;
                let state = FileSystemSnapshotAdapter::new(hearth.clone())
                    .read_artifact_state(TRACK_REL)
                    .map_err(|e| format!("read_artifact_state: {e}"))?;
                if state == expected.as_ref() as &str {
                    Ok(())
                } else {
                    Err(format!("Expected seam state '{expected}', got '{state}'"))
                }
            },
        ),
        check_def(
            "the transitions event directory holds exactly {int} events",
            &[("sh_hearth", "PathBuf")],
            |ctx, params| {
                let expected: i64 = params.get_int(0).ok_or("Expected count")?;
                let hearth = ctx.get::<PathBuf>("sh_hearth").ok_or("No sh_hearth")?;
                let dir = track_dir(hearth).join(TRANSITIONS_DIR);
                let names: Vec<String> = std::fs::read_dir(&dir)
                    .map_err(|e| format!("read {}: {e}", dir.display()))?
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .filter(|n| n.ends_with(".yaml"))
                    .collect();
                if names.len() as i64 == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {expected} event file(s), found {}: {names:?}",
                        names.len()
                    ))
                }
            },
        ),
        check_def(
            "the status.yaml keeps its other keys unchanged",
            &[("sh_hearth", "PathBuf")],
            |ctx, _params| {
                let hearth = ctx.get::<PathBuf>("sh_hearth").ok_or("No sh_hearth")?;
                let content = status_text(hearth)?;
                // A header repair is a ONE-LINE edit. Re-serializing the file
                // would reorder and reformat these blocks across every artifact
                // in the hearth, so their survival is the contract.
                for key in ["version: 1", "kind: track", "origin_turn: header-projection", "actors: {}", "transitions:"] {
                    if !content.lines().any(|l| l == key) {
                        return Err(format!(
                            "status.yaml lost the untouched key '{key}':\n{content}"
                        ));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "the reconciler reports drift from {string} to {string}",
            &[
                ("sh_declared", "String"),
                ("sh_resolved", "String"),
                ("sh_verdict", "String"),
            ],
            |ctx, params| {
                let before = params.get_string(0).ok_or("Expected before")?;
                let after = params.get_string(1).ok_or("Expected after")?;
                let declared = ctx.get::<String>("sh_declared").ok_or("No sh_declared")?;
                let resolved = ctx.get::<String>("sh_resolved").ok_or("No sh_resolved")?;
                let verdict = ctx.get::<String>("sh_verdict").ok_or("No sh_verdict")?;
                if verdict.as_str() != "drifted" {
                    return Err(format!(
                        "Expected a repairable drift, got verdict '{verdict}' \
                         (declared={declared}, resolved={resolved})"
                    ));
                }
                if declared.as_str() != before.as_ref() as &str {
                    return Err(format!("Expected before '{before}', got '{declared}'"));
                }
                if resolved.as_str() != after.as_ref() as &str {
                    return Err(format!("Expected after '{after}', got '{resolved}'"));
                }
                Ok(())
            },
        ),
        check_def(
            "the reconciler reports no drift",
            &[("sh_verdict", "String")],
            |ctx, _params| {
                let verdict = ctx.get::<String>("sh_verdict").ok_or("No sh_verdict")?;
                if verdict.as_str() == "agrees" {
                    Ok(())
                } else {
                    Err(format!("Expected no drift, got verdict '{verdict}'"))
                }
            },
        ),
        check_def(
            "the reconciler reports the header unverifiable, showing {string} against {string}",
            &[
                ("sh_verdict", "String"),
                ("sh_declared", "String"),
                ("sh_resolved", "String"),
            ],
            |ctx, params| {
                let header = params.get_string(0).ok_or("Expected header")?;
                let tail = params.get_string(1).ok_or("Expected tail")?;
                let verdict = ctx.get::<String>("sh_verdict").ok_or("No sh_verdict")?;
                let declared = ctx.get::<String>("sh_declared").ok_or("No sh_declared")?;
                let resolved = ctx.get::<String>("sh_resolved").ok_or("No sh_resolved")?;
                if verdict.as_str() != "unverifiable" {
                    return Err(format!(
                        "Expected verdict 'unverifiable', got '{verdict}' — a legacy-array-only \
                         artifact must NOT be repaired from the array"
                    ));
                }
                if declared.as_str() != header.as_ref() as &str {
                    return Err(format!("Expected header '{header}', got '{declared}'"));
                }
                if resolved.as_str() != tail.as_ref() as &str {
                    return Err(format!("Expected array tail '{tail}', got '{resolved}'"));
                }
                Ok(())
            },
        ),
        check_def(
            "the reconciler wrote nothing",
            &[("sh_written", "String")],
            |ctx, _params| {
                let written = ctx.get::<String>("sh_written").ok_or("No sh_written")?;
                if written.as_str() == "false" {
                    Ok(())
                } else {
                    Err("Expected no write, the reconciler rewrote the file".to_string())
                }
            },
        ),
        check_def(
            "the reconciler wrote the repair",
            &[("sh_written", "String")],
            |ctx, _params| {
                let written = ctx.get::<String>("sh_written").ok_or("No sh_written")?;
                if written.as_str() == "true" {
                    Ok(())
                } else {
                    Err("Expected the repair to be written, nothing was".to_string())
                }
            },
        ),
        check_def(
            "the reconciler refuses with {string}",
            &[("sh_error", "String")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected error code")?;
                let error = ctx.get::<String>("sh_error").ok_or("No sh_error")?;
                if error.contains(needle.as_ref() as &str) {
                    Ok(())
                } else {
                    Err(format!("Expected refusal '{needle}', got '{error}'"))
                }
            },
        ),
    ]
}

fn run_reconcile(ctx: Context, mode: ReconcileMode) -> Result<Context, String> {
    let hearth = ctx.get::<PathBuf>("sh_hearth").ok_or("No sh_hearth")?.clone();
    let r = reconcile_status_header(&track_dir(&hearth), mode, HeaderPolicy::Conservative)
        .map_err(|e| format!("reconcile_status_header: {e}"))?;
    let mut out = Context::new();
    out.set("sh_hearth", hearth);
    carry_retained_temp_dir(&ctx, &mut out, "sh_hearth_handle");
    out.set("sh_declared", r.declared.unwrap_or_default());
    out.set("sh_resolved", r.resolved);
    out.set(
        "sh_verdict",
        match r.verdict {
            HeaderVerdict::Agrees => "agrees",
            HeaderVerdict::Drifted => "drifted",
            HeaderVerdict::Unverifiable => "unverifiable",
        }
        .to_string(),
    );
    out.set("sh_written", r.written.to_string());
    Ok(out)
}
