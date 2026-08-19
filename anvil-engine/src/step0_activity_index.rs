//! Reads the temper-consumed §0 stream to answer, per instance, "how many
//! events, and when was the most recent one" — the signal the honest Live
//! panel (Fix B) needs to tell genuinely-in-progress work from a merely-open
//! (dormant) artifact.
//!
//! Reads `<temper_home>/.temper/step-measurements/<kind>/events.jsonl` — the
//! SAME per-kind partition [`FileSystemStep0StreamAdapter`] writes (see
//! `main.rs::emit_step0_stream`). One JSON object per line; each line's
//! `workflow_id` is the per-instance id. This module only COUNTS lines and
//! tracks the max `at` per `workflow_id` — it never reads `intent`/
//! `expected_output` prose.
//!
//! Fail-open throughout: no temper home, no events file for the kind, or a
//! malformed line yields an empty/partial index, never an error — this is a
//! best-effort read-side enrichment, not a load-bearing query.

use std::collections::HashMap;
use std::path::Path;

use anvil_core_hearth::fs_step_measurement_stream_adapter::FileSystemStep0StreamAdapter;

/// Per-`workflow_id` (instance id) §0 activity: event count + the most-recent
/// event's `at` (RFC3339). Absent from the map ⇒ zero events recorded.
pub type Step0ActivityIndex = HashMap<String, (u32, String)>;

/// Build the §0 activity index for one playbook `kind`. `temper_home: None`
/// (unresolvable) yields an empty index. A missing/unreadable events file, or
/// one with malformed lines, yields an empty or partial index — never an
/// error.
pub fn read_step0_activity_index(temper_home: Option<&Path>, kind: &str) -> Step0ActivityIndex {
    let mut index: Step0ActivityIndex = HashMap::new();
    let Some(home) = temper_home else {
        return index;
    };
    let adapter = FileSystemStep0StreamAdapter::new(home.to_path_buf());
    let path = adapter.events_path(kind);
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return index, // no events file for this kind — zero activity.
    };

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parsed: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    outcome = "step0_activity_index_parse_failed",
                    path = %path.display(),
                    error = %e,
                    "§0 activity-index line parse failed (non-fatal, skipped)"
                );
                continue;
            }
        };
        let workflow_id = parsed
            .get("workflow_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if workflow_id.is_empty() {
            continue;
        }
        let at = parsed
            .get("at")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        let entry = index.entry(workflow_id).or_insert((0, String::new()));
        entry.0 += 1;
        // RFC3339 timestamps sort lexically the same as chronologically.
        if at > entry.1 {
            entry.1 = at;
        }
    }

    index
}
