//! Step module for `step_measurement_stream.feature`.
//!
//! Asserts against the temper-consumed full §0 step-measurement stream the
//! engine writes at
//! `<ANVIL_TEMPER_HOME>/.temper/step-measurements/<kind>/events.jsonl`. The
//! spawn step (`the engine is started with that hearth`) sets ANVIL_TEMPER_HOME
//! to `<hearth>/__temper_home__`, so the events file lands inside the test temp
//! tree and is read back here.

use anvil_core_hearth::fs_step_measurement_adapter::FileSystemStepMeasurementAdapter;
use anvil_core::ports::step_measurement_port::{
    StepMeasurementReadPort, StepMeasurementRecord,
};
use brine_runner_rust::registry::{check_def, StepDef};
use std::path::{Path, PathBuf};

const LEAN_SINK_POLL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
const LEAN_SINK_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(20);

/// The per-kind events file under the test temper home rooted at the hearth.
fn events_path(hearth: &Path, kind: &str) -> PathBuf {
    hearth
        .join("__temper_home__")
        .join(".temper")
        .join("step-measurements")
        .join(kind)
        .join("events.jsonl")
}

/// Parse every line of the events file as a JSON object. Missing file ⇒ empty.
fn read_events(hearth: &Path, kind: &str) -> Result<Vec<serde_json::Value>, String> {
    let path = events_path(hearth, kind);
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("read {}: {}", path.display(), e)),
    };
    let mut out = Vec::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(line)
            .map_err(|e| format!("malformed §0 line '{}': {}", line, e))?;
        out.push(v);
    }
    Ok(out)
}

fn field_matches(field: Option<&serde_json::Value>, expected: &str) -> bool {
    if expected == "<non-empty>" {
        return match field {
            Some(serde_json::Value::String(s)) => !s.is_empty(),
            Some(serde_json::Value::Null) | None => false,
            Some(_) => true,
        };
    }
    match field {
        Some(serde_json::Value::String(s)) => s == expected,
        Some(serde_json::Value::Bool(b)) => b.to_string() == expected,
        Some(serde_json::Value::Number(n)) => n.to_string() == expected,
        // An empty-cell expectation must match a present-but-empty string.
        None => expected.is_empty(),
        Some(other) => other.to_string() == expected,
    }
}

fn wait_for_lean_step_measurements(
    hearth: &Path,
    expected: usize,
) -> Result<Vec<StepMeasurementRecord>, String> {
    let deadline = std::time::Instant::now() + LEAN_SINK_POLL_TIMEOUT;
    let mut last_observation: String;

    loop {
        match FileSystemStepMeasurementAdapter::new(hearth).read_step_measurements() {
            Ok(records) if records.len() >= expected => return Ok(records),
            Ok(records) => {
                last_observation = format!("{} record(s): {:?}", records.len(), records);
            }
            Err(error) => last_observation = error.to_string(),
        }

        let now = std::time::Instant::now();
        if now >= deadline {
            return Err(format!(
                "lean step-measurement sink did not reach at least {} record(s) within {:?}: {}",
                expected, LEAN_SINK_POLL_TIMEOUT, last_observation
            ));
        }
        std::thread::sleep(LEAN_SINK_POLL_INTERVAL.min(deadline - now));
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // Seed a DECIDED step-measurement-emit-privacy decision into the hearth
        // so the engine's emit gate opens. The decision id is date-prefixed to
        // mirror the real scaffold form; the engine matches it by suffix.
        // Seed a DECIDED decision into the hearth (read-only on context — a pure
        // file side effect — so every other context key, incl. the TempDir
        // retention handle, is preserved). The id is date-prefixed to mirror the
        // real scaffold; the engine matches it by suffix.
        check_def(
            "the hearth has a decided {string} decision",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let id = params.get_string(0).ok_or("Expected decision id")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let dir_name = format!("20260622T0000_{}", id.replace('-', "_"));
                let dir = hearth.join("decisions").join(&dir_name);
                std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir decision: {}", e))?;
                let status = "version: 1\nkind: decision\nstate: decided\ntransitions:\n  - to: tension\n    at: 2026-06-22T00:00:00Z\n    actor: seed\n    role: doer\n  - to: decided\n    at: 2026-06-22T00:01:00Z\n    actor: seed\n    role: decide\n";
                std::fs::write(dir.join("status.yaml"), status)
                    .map_err(|e| format!("write decision status: {}", e))?;
                std::fs::write(dir.join("definition.md"), "# decided\n")
                    .map_err(|e| format!("write decision def: {}", e))?;
                Ok(())
            },
        ),
        // Block events.jsonl by pre-creating a DIRECTORY where the file should
        // be — the append open fails, exercising the fail-open path. Read-only
        // on context (pure file side effect).
        check_def(
            "the temper step0 stream path for kind {string} is blocked by a file",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let path = events_path(hearth, &kind);
                std::fs::create_dir_all(&path)
                    .map_err(|e| format!("create blocking dir {}: {}", path.display(), e))?;
                Ok(())
            },
        ),
        // The MID-SCENARIO form. `is blocked by a file` blocks a path that does
        // not exist yet; by the time these two scenarios block it, the begin RPC
        // has already created `events.jsonl`, so the same implementation fails
        // with EEXIST. Blocking an EXISTING stream is the case the fail-open
        // path actually has to survive in production — the file is there and
        // then becomes unwritable — and it had no step definition at all, which
        // is why both scenarios were undefined rather than green.
        check_def(
            "the temper step0 stream path for kind {string} becomes blocked",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let path = events_path(hearth, &kind);
                if path.is_dir() {
                    return Err(format!(
                        "{} is ALREADY a directory — this step would then prove nothing",
                        path.display()
                    ));
                }
                if path.exists() {
                    std::fs::remove_file(&path)
                        .map_err(|e| format!("remove {}: {}", path.display(), e))?;
                }
                std::fs::create_dir_all(&path)
                    .map_err(|e| format!("create blocking dir {}: {}", path.display(), e))?;
                Ok(())
            },
        ),
        check_def(
            "the temper step0 stream for kind {string} has {int} events",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let expected = params.get_int(1).ok_or("Expected count")? as usize;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let events = read_events(hearth, &kind)?;
                if events.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} §0 events for kind '{}', got {}: {:?}",
                        expected,
                        kind,
                        events.len(),
                        events
                    ))
                }
            },
        ),
        check_def(
            "the temper step0 stream for kind {string} has exactly {int} events with to_state {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let expected = params.get_int(1).ok_or("Expected count")? as usize;
                let to_state = params.get_string(2).ok_or("Expected to_state")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let events = read_events(hearth, &kind)?;
                let count = events
                    .iter()
                    .filter(|e| {
                        e.get("to_state").and_then(|v| v.as_str()) == Some(to_state.as_str())
                    })
                    .count();
                if count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected exactly {} §0 events with to_state '{}' for kind '{}', got {}: {:?}",
                        expected, to_state, kind, count, events
                    ))
                }
            },
        ),
        check_def(
            "the temper step0 stream for kind {string} events are in lifecycle order by to_state {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let order_csv = params.get_string(1).ok_or("Expected order csv")?.to_string();
                let want: Vec<&str> = order_csv.split(',').map(|s| s.trim()).collect();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let events = read_events(hearth, &kind)?;
                let got: Vec<String> = events
                    .iter()
                    .map(|e| {
                        e.get("to_state")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string()
                    })
                    .collect();
                if got.iter().map(|s| s.as_str()).eq(want.iter().copied()) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected §0 to_state order {:?}, got {:?}",
                        want, got
                    ))
                }
            },
        ),
        check_def(
            "the temper step0 stream for kind {string} has an event with fields:",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let table = params.data_table().ok_or("Expected data table")?;
                let mut wanted: Vec<(String, String)> = Vec::new();
                if table.headers.len() >= 2 {
                    wanted.push((
                        table.headers[0].trim().to_string(),
                        table.headers[1].trim().to_string(),
                    ));
                }
                for row in &table.rows {
                    if row.len() >= 2 {
                        wanted.push((row[0].trim().to_string(), row[1].trim().to_string()));
                    }
                }
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let events = read_events(hearth, &kind)?;
                let found = events.iter().any(|e| {
                    wanted
                        .iter()
                        .all(|(k, expected)| field_matches(e.get(k), expected))
                });
                if found {
                    Ok(())
                } else {
                    Err(format!(
                        "No §0 event for kind '{}' matched all fields {:?}; events: {:?}",
                        kind, wanted, events
                    ))
                }
            },
        ),
        // H1: playbook_id (per-run instance id) and track_id (kind) are DIFFERENT
        // values. This asserts the bug fix — they must not collapse onto one id.
        check_def(
            "the temper step0 stream for kind {string} event has {string} not equal to {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let field_a = params.get_string(1).ok_or("Expected field a")?.to_string();
                let field_b = params.get_string(2).ok_or("Expected field b")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let events = read_events(hearth, &kind)?;
                if events.is_empty() {
                    return Err(format!("No §0 events for kind '{}' to check", kind));
                }
                for e in &events {
                    let a = e.get(&field_a).and_then(|v| v.as_str()).unwrap_or_default();
                    let b = e.get(&field_b).and_then(|v| v.as_str()).unwrap_or_default();
                    if a.is_empty() {
                        return Err(format!("Field '{}' is empty: {:?}", field_a, e));
                    }
                    if a == b {
                        return Err(format!(
                            "Expected '{}' != '{}' but both are '{}': {:?}",
                            field_a, field_b, a, e
                        ));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "the temper step0 stream for kind {string} event has no {string} field",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let field = params.get_string(1).ok_or("Expected field")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let events = read_events(hearth, &kind)?;
                if events.is_empty() {
                    return Err(format!("No §0 events for kind '{}' to check", kind));
                }
                for e in &events {
                    if e.get(&field).is_some() {
                        return Err(format!(
                            "§0 event unexpectedly carries field '{}': {:?}",
                            field, e
                        ));
                    }
                }
                Ok(())
            },
        ),
        // The lean booleans-only hearth sink — proves the lean fallback still
        // wrote while the rich §0 gate was closed.
        check_def(
            "the lean step-measurement sink has at least {int} records",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = wait_for_lean_step_measurements(hearth, expected)?;
                if records.len() >= expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected at least {} lean records, got {}: {:?}",
                        expected,
                        records.len(),
                        records
                    ))
                }
            },
        ),
    ]
}
