//! Step definitions for the per-file transition event store
//! (`anvil_core::ports::transition_event_write_port` +
//! `anvil_core_hearth::fs_transition_event_adapter`, folded through the
//! `transition_log` seam).
//!
//! These steps drive the REAL filesystem adapters against a temp hearth so the
//! conflict-free one-file-per-event invariant and the dual-read fold are
//! exercised end-to-end. The seeded track is always
//! `tracks/20260417T0100_event_track`.

use anvil_test_support::{carry_retained_temp_dir, retained_temp_dir};
use anvil_core::domain::shared_types::TransitionContent;
use anvil_core_hearth::fs_snapshot_adapter::FileSystemSnapshotAdapter;
use anvil_core_hearth::fs_transition_event_adapter::FileSystemTransitionEventAdapter;
use anvil_core::ports::snapshot_port::SnapshotPort;
use anvil_core::ports::transition_event_write_port::{
    TransitionEventWritePort, TransitionRecord, TRANSITIONS_DIR,
};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;

const TRACK_REL: &str = "tracks/20260417T0100_event_track";

fn track_dir(hearth: &PathBuf) -> PathBuf {
    hearth.join(TRACK_REL)
}

fn events_dir(hearth: &PathBuf) -> PathBuf {
    track_dir(hearth).join(TRANSITIONS_DIR)
}

fn list_event_files(hearth: &PathBuf) -> Vec<(String, TransitionRecord)> {
    let dir = events_dir(hearth);
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".yaml") {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if let Ok(rec) = serde_yaml::from_str::<TransitionRecord>(&content) {
                    out.push((name, rec));
                }
            }
        }
    }
    // Stable ordering by the embedded `at` then file name so "latest" is
    // deterministic across filesystem read order.
    out.sort_by(|a, b| a.1.at.cmp(&b.1.at).then(a.0.cmp(&b.0)));
    out
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // Seed a temp hearth with a single track at a given state (top-level
        // `state:` + one legacy transition matching it).
        step_def(
            "a transition-event hearth with a track at {string}",
            &[],
            &[
                ("te_hearth", "PathBuf"),
                ("te_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let state = params.get_string(0).ok_or("Expected state")?.to_string();
                let (handle, hearth) = retained_temp_dir("anvil-transition-event-")?;
                let dir = hearth.join(TRACK_REL);
                std::fs::create_dir_all(&dir).map_err(|e| format!("create track dir: {}", e))?;
                let status = format!(
                    "version: 1\nkind: track\nstate: {state}\nactors: {{}}\ntransitions:\n  - to: {state}\n    at: 2026-04-17T00:00:00Z\n    actor: Author-000001\n    role: spec\n",
                    state = state
                );
                std::fs::write(dir.join("status.yaml"), status)
                    .map_err(|e| format!("write status.yaml: {}", e))?;
                std::fs::write(dir.join("spec.md"), "# Event Track\n\nBody.\n")
                    .map_err(|e| format!("write spec.md: {}", e))?;
                let mut out = Context::new();
                out.set("te_hearth", hearth);
                out.set("te_hearth_handle", handle);
                Ok(out)
            },
        ),
        // Seed a temp hearth with a freshly-scaffolded track: a status.yaml with
        // a top-level `state:` but NO transitions array and NO event dir — the
        // shape a brand-new artifact has the instant before its creation-seed
        // transition is recorded (mirrors build_initial_artifact_status_yaml).
        step_def(
            "a transition-event hearth with a fresh track at {string} and no recorded transitions",
            &[],
            &[
                ("te_hearth", "PathBuf"),
                ("te_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let state = params.get_string(0).ok_or("Expected state")?.to_string();
                let (handle, hearth) = retained_temp_dir("anvil-transition-event-")?;
                let dir = hearth.join(TRACK_REL);
                std::fs::create_dir_all(&dir).map_err(|e| format!("create track dir: {}", e))?;
                let status = format!(
                    "version: 1\nkind: track\nstate: {state}\nactors: {{}}\n",
                    state = state
                );
                std::fs::write(dir.join("status.yaml"), status)
                    .map_err(|e| format!("write status.yaml: {}", e))?;
                std::fs::write(dir.join("spec.md"), "# Event Track\n\nBody.\n")
                    .map_err(|e| format!("write spec.md: {}", e))?;
                let mut out = Context::new();
                out.set("te_hearth", hearth);
                out.set("te_hearth_handle", handle);
                Ok(out)
            },
        ),
        // Record a transition through the REAL FileSystemSnapshotAdapter
        // (which now writes a per-file event, not the array).
        step_def(
            "a transition to {string} by {string} role {string} at {string} is recorded",
            &[("te_hearth", "PathBuf")],
            &[
                ("te_hearth", "PathBuf"),
                ("te_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let to = params.get_string(0).ok_or("Expected to")?.to_string();
                let actor = params.get_string(1).ok_or("Expected actor")?.to_string();
                let role = params.get_string(2).ok_or("Expected role")?.to_string();
                let at = params.get_string(3).ok_or("Expected at")?.to_string();
                let hearth = ctx.get::<PathBuf>("te_hearth").ok_or("No te_hearth")?.clone();
                let adapter = FileSystemSnapshotAdapter::new(hearth.clone());
                let transition = TransitionContent {
                    to,
                    at,
                    actor,
                    role,
                    approver: None,
                    note: None,
                    satisfaction: None,
                    event_type: None,
                };
                adapter
                    .append_transition(TRACK_REL, &transition)
                    .map_err(|e| format!("append_transition: {}", e))?;
                let mut out = Context::new();
                out.set("te_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "te_hearth_handle");
                Ok(out)
            },
        ),
        // Seed an event file DIRECTLY via the narrow port (used to present
        // events in arbitrary on-disk order for the fold test).
        step_def(
            "a transition event file for {string} at {string} by {string} role {string}",
            &[("te_hearth", "PathBuf")],
            &[
                ("te_hearth", "PathBuf"),
                ("te_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let to = params.get_string(0).ok_or("Expected to")?.to_string();
                let at = params.get_string(1).ok_or("Expected at")?.to_string();
                let actor = params.get_string(2).ok_or("Expected actor")?.to_string();
                let role = params.get_string(3).ok_or("Expected role")?.to_string();
                let hearth = ctx.get::<PathBuf>("te_hearth").ok_or("No te_hearth")?.clone();
                let adapter = FileSystemTransitionEventAdapter::new(hearth.clone());
                let record = TransitionRecord {
                    to,
                    at,
                    actor,
                    role,
                    approver: None,
                    note: None,
                    satisfaction: None,
                    event_type: None,
                };
                adapter
                    .append_transition_event(TRACK_REL, &record)
                    .map_err(|e| format!("append_transition_event: {}", e))?;
                let mut out = Context::new();
                out.set("te_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "te_hearth_handle");
                Ok(out)
            },
        ),
        // Resolve current state through the REAL fs read seam (folds events).
        step_def(
            "the track current state is read through the seam",
            &[("te_hearth", "PathBuf")],
            &[
                ("te_hearth", "PathBuf"),
                ("te_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("te_read_state", "String"),
            ],
            |ctx, _params| {
                let hearth = ctx.get::<PathBuf>("te_hearth").ok_or("No te_hearth")?.clone();
                let adapter = FileSystemSnapshotAdapter::new(hearth.clone());
                let state = adapter
                    .read_artifact_state(TRACK_REL)
                    .map_err(|e| format!("read_artifact_state: {}", e))?;
                let mut out = Context::new();
                out.set("te_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "te_hearth_handle");
                out.set("te_read_state", state);
                Ok(out)
            },
        ),
        check_def(
            "the transitions event directory for the track contains exactly {int} file",
            &[("te_hearth", "PathBuf")],
            |ctx, params| {
                let expected: i64 = params.get_int(0).ok_or("Expected count")?;
                let hearth = ctx.get::<PathBuf>("te_hearth").ok_or("No te_hearth")?;
                let files = list_event_files(hearth);
                if files.len() as i64 == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} event file(s), found {}: {:?}",
                        expected,
                        files.len(),
                        files.iter().map(|(n, _)| n).collect::<Vec<_>>()
                    ))
                }
            },
        ),
        check_def(
            "the latest transition event file carries to {string} actor {string} role {string} at {string}",
            &[("te_hearth", "PathBuf")],
            |ctx, params| {
                let to = params.get_string(0).ok_or("Expected to")?;
                let actor = params.get_string(1).ok_or("Expected actor")?;
                let role = params.get_string(2).ok_or("Expected role")?;
                let at = params.get_string(3).ok_or("Expected at")?;
                let hearth = ctx.get::<PathBuf>("te_hearth").ok_or("No te_hearth")?;
                let files = list_event_files(hearth);
                let (_, rec) = files.last().ok_or("No event files present")?;
                if rec.to == to.as_ref() as &str
                    && rec.actor == actor.as_ref() as &str
                    && rec.role == role.as_ref() as &str
                    && rec.at == at.as_ref() as &str
                {
                    Ok(())
                } else {
                    Err(format!("Latest event record mismatch: {:?}", rec))
                }
            },
        ),
        check_def(
            "the latest transition event file name is not derived from a sibling count",
            &[("te_hearth", "PathBuf")],
            |ctx, _params| {
                let hearth = ctx.get::<PathBuf>("te_hearth").ok_or("No te_hearth")?;
                let files = list_event_files(hearth);
                let (name, rec) = files.last().ok_or("No event files present")?;
                // A count-derived name would be a small ordinal (e.g. "0",
                // "1", "001"). Assert instead the name leads with a
                // high-resolution timestamp prefix (the event year), embeds the
                // actor, and ends with a non-trivial random suffix — all
                // content/clock-derived, never a sibling count.
                let stem = name.strip_suffix(".yaml").unwrap_or(name);
                let year = rec.at.get(0..4).unwrap_or("");
                if !year.is_empty() && !stem.starts_with(year) {
                    return Err(format!(
                        "Event file name '{}' does not lead with a timestamp prefix",
                        name
                    ));
                }
                let safe_actor = rec.actor.replace(':', "-");
                if !stem.contains(&safe_actor) {
                    return Err(format!(
                        "Event file name '{}' does not embed the actor '{}'",
                        name, safe_actor
                    ));
                }
                let parts: Vec<&str> = stem.rsplitn(2, '_').collect();
                let suffix = parts.first().copied().unwrap_or("");
                if suffix.len() < 8 {
                    return Err(format!(
                        "Event file name '{}' has no content-independent random suffix (got '{}')",
                        name, suffix
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the two transition event files have distinct names",
            &[("te_hearth", "PathBuf")],
            |ctx, _params| {
                let hearth = ctx.get::<PathBuf>("te_hearth").ok_or("No te_hearth")?;
                let files = list_event_files(hearth);
                if files.len() != 2 {
                    return Err(format!("Expected exactly 2 event files, found {}", files.len()));
                }
                if files[0].0 == files[1].0 {
                    Err(format!("Event files share a name: '{}'", files[0].0))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the folded current state is {string}",
            &[("te_read_state", "String")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?;
                let state = ctx.get::<String>("te_read_state").ok_or("No te_read_state")?;
                if state.as_ref() as &str == expected.as_ref() as &str {
                    Ok(())
                } else {
                    Err(format!("Expected folded state '{}', got '{}'", expected, state))
                }
            },
        ),
        check_def(
            "the track status.yaml does not contain a transition to {string}",
            &[("te_hearth", "PathBuf")],
            |ctx, params| {
                let to = params.get_string(0).ok_or("Expected to")?;
                let hearth = ctx.get::<PathBuf>("te_hearth").ok_or("No te_hearth")?;
                let content = std::fs::read_to_string(track_dir(hearth).join("status.yaml"))
                    .map_err(|e| format!("read status.yaml: {}", e))?;
                let needle = format!("to: {}", to);
                if content.contains(&needle) {
                    Err(format!(
                        "status.yaml unexpectedly contains a transition '{}':\n{}",
                        needle, content
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the track status.yaml top-level state is {string}",
            &[("te_hearth", "PathBuf")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?;
                let hearth = ctx.get::<PathBuf>("te_hearth").ok_or("No te_hearth")?;
                let content = std::fs::read_to_string(track_dir(hearth).join("status.yaml"))
                    .map_err(|e| format!("read status.yaml: {}", e))?;
                let want = format!("state: {}", expected);
                let found = content
                    .lines()
                    .any(|l| !l.starts_with(char::is_whitespace) && l == want.as_str());
                if found {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected top-level '{}' in status.yaml, content:\n{}",
                        want, content
                    ))
                }
            },
        ),
        check_def(
            "the track current state through the seam is {string}",
            &[("te_hearth", "PathBuf")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?;
                let hearth = ctx.get::<PathBuf>("te_hearth").ok_or("No te_hearth")?;
                let adapter = FileSystemSnapshotAdapter::new(hearth.clone());
                let state = adapter
                    .read_artifact_state(TRACK_REL)
                    .map_err(|e| format!("read_artifact_state: {}", e))?;
                if state == expected.as_ref() as &str {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected seam current state '{}', got '{}'",
                        expected, state
                    ))
                }
            },
        ),
    ]
}
