//! Step module for `step0_stream_adapter.feature` (anvil-core).
//!
//! Exercises the real `FileSystemStep0StreamAdapter` against a temp temper-home,
//! proving the append-only + per-kind-partitioned + idempotent-dedupe contract of
//! the temper-consumed full §0 step-measurement stream — the engine's seam.

use anvil_core_hearth::fs_step_measurement_stream_adapter::FileSystemStep0StreamAdapter;
use anvil_core::ports::step_measurement_stream_port::{Step0Event, Step0StreamWritePort};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;

use anvil_test_support::{retained_temp_dir, RetainedTempDir};

const HOME_KEY: &str = "s0a_home";
const HANDLE_KEY: &str = "s0a_handle";

fn make_event(kind: &str, from: &str, to: &str, seq: &str, tokens: Option<u64>) -> Step0Event {
    Step0Event {
        playbook_id: "wf-1".to_string(),
        track_id: kind.to_string(),
        from_state: from.to_string(),
        to_state: to.to_string(),
        role: "doer".to_string(),
        actor: "Tester-000000".to_string(),
        intent: "do the thing".to_string(),
        expected_output: "the thing done".to_string(),
        at: "2026-06-22T00:00:00Z".to_string(),
        model: "claude-opus-4-8".to_string(),
        tokens,
        duration_ms: None,
        event_seq: seq.to_string(),
    }
}

fn events_lines(home: &std::path::Path, kind: &str) -> Result<Vec<serde_json::Value>, String> {
    let path = FileSystemStep0StreamAdapter::new(home.to_path_buf()).events_path(kind);
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("read {}: {}", path.display(), e)),
    };
    contents
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).map_err(|e| format!("bad line: {}", e)))
        .collect()
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a temper home for the §0 stream",
            &[],
            &[(HOME_KEY, "PathBuf"), (HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, _params| {
                let (handle, tmp) = retained_temp_dir("anvil-step0-stream-")?;
                let mut out = Context::new();
                out.set(HOME_KEY, tmp);
                out.set::<RetainedTempDir>(HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        check_def(
            "appending the same §0 event twice for kind {string} writes one line",
            &[(HOME_KEY, "PathBuf")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let home = ctx.get::<PathBuf>(HOME_KEY).ok_or("No home")?;
                let adapter = FileSystemStep0StreamAdapter::new(home.clone());
                let ev = make_event(&kind, "spec", "spec_review", "doer", None);
                adapter.append_step0_event(&ev).map_err(|e| e.to_string())?;
                adapter.append_step0_event(&ev).map_err(|e| e.to_string())?;
                let lines = events_lines(home, &kind)?;
                if lines.len() == 1 {
                    Ok(())
                } else {
                    Err(format!("Expected 1 line after duplicate append, got {}", lines.len()))
                }
            },
        ),
        check_def(
            "appending two distinct §0 events in the same second for kind {string} writes two lines",
            &[(HOME_KEY, "PathBuf")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let home = ctx.get::<PathBuf>(HOME_KEY).ok_or("No home")?;
                let adapter = FileSystemStep0StreamAdapter::new(home.clone());
                // Same `at`, different logical step (distinct from/to) ⇒ distinct
                // event_id ⇒ two lines (no false-dedupe).
                let a = make_event(&kind, "spec", "spec_review", "doer", None);
                let b = make_event(&kind, "spec_review", "plan", "reviewer", None);
                adapter.append_step0_event(&a).map_err(|e| e.to_string())?;
                adapter.append_step0_event(&b).map_err(|e| e.to_string())?;
                let lines = events_lines(home, &kind)?;
                if lines.len() == 2 {
                    Ok(())
                } else {
                    Err(format!("Expected 2 lines for distinct events, got {}", lines.len()))
                }
            },
        ),
        // H3: two genuinely-distinct transitions identical in every field EXCEPT
        // the uniqueness component (event_seq) — same playbook_id/from/to/role/at
        // — must both be kept (a loop re-entering the same transition same-second).
        // A role-only seq would collapse them to ONE line (the bug this proves).
        check_def(
            "appending two §0 events identical except seq for kind {string} writes two lines",
            &[(HOME_KEY, "PathBuf")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let home = ctx.get::<PathBuf>(HOME_KEY).ok_or("No home")?;
                let adapter = FileSystemStep0StreamAdapter::new(home.clone());
                // Identical (playbook_id, from, to, role, at) — only event_seq
                // differs. With a real uniqueness component these are distinct
                // event_ids ⇒ two lines. With seq = role they'd collapse to one.
                let a = make_event(&kind, "answering", "answering", "seq-1", None);
                let b = make_event(&kind, "answering", "answering", "seq-2", None);
                adapter.append_step0_event(&a).map_err(|e| e.to_string())?;
                adapter.append_step0_event(&b).map_err(|e| e.to_string())?;
                let lines = events_lines(home, &kind)?;
                if lines.len() == 2 {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected 2 lines for distinct-seq events, got {}",
                        lines.len()
                    ))
                }
            },
        ),
        check_def(
            "appending a §0 event with no tokens for kind {string} omits the tokens field",
            &[(HOME_KEY, "PathBuf")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let home = ctx.get::<PathBuf>(HOME_KEY).ok_or("No home")?;
                let adapter = FileSystemStep0StreamAdapter::new(home.clone());
                let ev = make_event(&kind, "spec", "spec_review", "doer", None);
                adapter.append_step0_event(&ev).map_err(|e| e.to_string())?;
                let lines = events_lines(home, &kind)?;
                let line = lines.first().ok_or("No line written")?;
                if line.get("tokens").is_some() {
                    Err(format!("tokens key present despite None: {:?}", line))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the §0 stream partitions kind {string} and kind {string} into separate files",
            &[(HOME_KEY, "PathBuf")],
            |ctx, params| {
                let k1 = params.get_string(0).ok_or("Expected kind1")?.to_string();
                let k2 = params.get_string(1).ok_or("Expected kind2")?.to_string();
                let home = ctx.get::<PathBuf>(HOME_KEY).ok_or("No home")?;
                let adapter = FileSystemStep0StreamAdapter::new(home.clone());
                adapter
                    .append_step0_event(&make_event(&k1, "", "answering", "doer", None))
                    .map_err(|e| e.to_string())?;
                adapter
                    .append_step0_event(&make_event(&k2, "", "spec", "doer", None))
                    .map_err(|e| e.to_string())?;
                let l1 = events_lines(home, &k1)?;
                let l2 = events_lines(home, &k2)?;
                if l1.len() == 1 && l2.len() == 1 {
                    Ok(())
                } else {
                    Err(format!("Expected 1 line per partition, got {} and {}", l1.len(), l2.len()))
                }
            },
        ),
    ]
}
