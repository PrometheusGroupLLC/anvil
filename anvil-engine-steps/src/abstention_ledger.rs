//! Step module for `abstention_ledger.feature`.
//!
//! Exercises the engine's opt-in abstention ledger HERMETICALLY — no live engine,
//! no Kiln, no socket. Two seams:
//!
//! 1. `record_abstention` — the enabled + outcome fold that appends one
//!    conversation-aware JSON line to `<hearth>/abstentions/ledger.jsonl`. Driven
//!    directly against a throwaway temp hearth: flag off → nothing; flag on +
//!    abstention → one record carrying message / conversation_id / recent_context /
//!    candidate_set; flag on + a routed kind → nothing; two abstentions in one
//!    conversation → two records sharing the id.
//! 2. `ledger_flag_on` — the pure on/off flag resolution, exercised over string
//!    values with no environment/config-file dependency.

use anvil_test_support::retained_temp_dir;
use anvil_core::domain::hooks::route_turn::RouteTurnOutcome;
use anvil_engine::abstention_ledger::{
    ledger_flag_on, now_rfc3339, record_abstention, AbstentionRecord,
};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;

const HEARTH_KEY: &str = "al_hearth";
const HEARTH_HANDLE_KEY: &str = "al_hearth_handle";
const ENABLED_KEY: &str = "al_enabled";
const FLAG_RESULT_KEY: &str = "al_flag_result";

fn parse_csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn ledger_path(hearth: &PathBuf) -> PathBuf {
    hearth.join("abstentions").join("ledger.jsonl")
}

/// Read every record currently in the ledger (empty when the file is absent).
fn read_records(hearth: &PathBuf) -> Result<Vec<AbstentionRecord>, String> {
    let path = ledger_path(hearth);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return Ok(Vec::new()),
    };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            serde_json::from_str::<AbstentionRecord>(l)
                .map_err(|e| format!("bad ledger line {:?}: {}", l, e))
        })
        .collect()
}

fn hearth_of(ctx: &Context) -> Result<PathBuf, String> {
    ctx.get::<PathBuf>(HEARTH_KEY)
        .or_else(|| ctx.get::<PathBuf>("hearth_path"))
        .cloned()
        .ok_or_else(|| "No abstention-ledger hearth in context".to_string())
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ── hermetic ledger-append seam ──
        step_def(
            "a throwaway hearth for the abstention ledger",
            &[],
            &[(HEARTH_KEY, "PathBuf"), (HEARTH_HANDLE_KEY, "RetainedTempDir")],
            |ctx, _params| {
                let (handle, tmp) = retained_temp_dir("anvil-abstention-ledger-")?;
                // Thread the incoming context forward (steps replace, not merge).
                let mut out = ctx;
                out.set(HEARTH_KEY, tmp);
                out.set(HEARTH_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "the abstention ledger flag is on",
            &[],
            &[
                (HEARTH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "RetainedTempDir"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                (ENABLED_KEY, "bool"),
            ],
            |ctx, _params| {
                let mut out = ctx;
                let hearth = hearth_of(&out)?;
                out.set(HEARTH_KEY, hearth.clone());
                out.set("hearth_path", hearth);
                out.set(ENABLED_KEY, true);
                Ok(out)
            },
        ),
        step_def(
            "the abstention ledger flag is off",
            &[(HEARTH_KEY, "PathBuf")],
            &[
                (HEARTH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "RetainedTempDir"),
                (ENABLED_KEY, "bool"),
            ],
            |ctx, _params| {
                let mut out = ctx;
                out.set(ENABLED_KEY, false);
                Ok(out)
            },
        ),
        step_def(
            "the route-turn final outcome is an abstention on message {string} in conversation {string} with context {string} and candidates {string}",
            &[(HEARTH_KEY, "PathBuf"), (ENABLED_KEY, "bool")],
            &[
                (HEARTH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "RetainedTempDir"),
                (ENABLED_KEY, "bool"),
            ],
            |ctx, params| {
                let message = params.get_string(0).ok_or("Expected message")?.to_string();
                let conversation_id =
                    params.get_string(1).ok_or("Expected conversation")?.to_string();
                let recent_context = params.get_string(2).ok_or("Expected context")?.to_string();
                let candidate_set = parse_csv(params.get_string(3).ok_or("Expected candidates")?);
                let enabled = *ctx.get::<bool>(ENABLED_KEY).ok_or("No enabled flag")?;
                let hearth = hearth_of(&ctx)?;
                let record = AbstentionRecord {
                    message,
                    recent_context,
                    conversation_id,
                    candidate_set,
                    at: now_rfc3339(),
                    source: "test-harness".to_string(),
                };
                // The router's FINAL outcome for an abstaining turn is NoMatch.
                record_abstention(enabled, &RouteTurnOutcome::NoMatch, Some(hearth.as_path()), &record);
                // Thread context forward so a second abstention (same conversation)
                // and the downstream checks still see the hearth + flag.
                Ok(ctx)
            },
        ),
        step_def(
            "the route-turn final outcome routes to kind {string} on message {string} in conversation {string}",
            &[(HEARTH_KEY, "PathBuf"), (ENABLED_KEY, "bool")],
            &[
                (HEARTH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "RetainedTempDir"),
                (ENABLED_KEY, "bool"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let message = params.get_string(1).ok_or("Expected message")?.to_string();
                let conversation_id =
                    params.get_string(2).ok_or("Expected conversation")?.to_string();
                let enabled = *ctx.get::<bool>(ENABLED_KEY).ok_or("No enabled flag")?;
                let hearth = hearth_of(&ctx)?;
                let record = AbstentionRecord {
                    message,
                    recent_context: String::new(),
                    conversation_id,
                    candidate_set: vec![kind.clone()],
                    at: now_rfc3339(),
                    source: "test-harness".to_string(),
                };
                // A routed turn resolves to a single kind — NOT an abstention.
                let routed = RouteTurnOutcome::Single {
                    kind,
                    description: String::new(),
                    why: String::new(),
                    required_fields: Vec::new(),
                    guidance: String::new(),
                };
                record_abstention(enabled, &routed, Some(hearth.as_path()), &record);
                Ok(ctx)
            },
        ),
        check_def(
            "the abstention ledger file does not exist",
            &[],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let path = ledger_path(&hearth);
                if path.exists() {
                    Err(format!("Expected no ledger file, but {:?} exists", path))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the abstention ledger record count is {int}",
            &[(HEARTH_KEY, "PathBuf")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let hearth = hearth_of(&ctx)?;
                let actual = read_records(&hearth)?.len();
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("Expected {} records, got {}", expected, actual))
                }
            },
        ),
        check_def(
            "the abstention ledger record {int} has message {string}",
            &[(HEARTH_KEY, "PathBuf")],
            |ctx, params| {
                let idx = params.get_int(0).ok_or("Expected record index")? as usize;
                let expected = params.get_string(1).ok_or("Expected message")?.to_string();
                let hearth = hearth_of(&ctx)?;
                let records = read_records(&hearth)?;
                let record = records
                    .get(idx - 1)
                    .ok_or_else(|| format!("No record at index {}", idx))?;
                if record.message == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "record {}: expected message {:?}, got {:?}",
                        idx, expected, record.message
                    ))
                }
            },
        ),
        check_def(
            "the abstention ledger record {int} has conversation_id {string}",
            &[(HEARTH_KEY, "PathBuf")],
            |ctx, params| {
                let idx = params.get_int(0).ok_or("Expected record index")? as usize;
                let expected = params.get_string(1).ok_or("Expected id")?.to_string();
                let hearth = hearth_of(&ctx)?;
                let records = read_records(&hearth)?;
                let record = records
                    .get(idx - 1)
                    .ok_or_else(|| format!("No record at index {}", idx))?;
                if record.conversation_id == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "record {}: expected conversation_id {:?}, got {:?}",
                        idx, expected, record.conversation_id
                    ))
                }
            },
        ),
        check_def(
            "the abstention ledger record {int} has recent_context {string}",
            &[(HEARTH_KEY, "PathBuf")],
            |ctx, params| {
                let idx = params.get_int(0).ok_or("Expected record index")? as usize;
                let expected = params.get_string(1).ok_or("Expected context")?.to_string();
                let hearth = hearth_of(&ctx)?;
                let records = read_records(&hearth)?;
                let record = records
                    .get(idx - 1)
                    .ok_or_else(|| format!("No record at index {}", idx))?;
                if record.recent_context == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "record {}: expected recent_context {:?}, got {:?}",
                        idx, expected, record.recent_context
                    ))
                }
            },
        ),
        check_def(
            "the abstention ledger record {int} has candidate_set {string}",
            &[(HEARTH_KEY, "PathBuf")],
            |ctx, params| {
                let idx = params.get_int(0).ok_or("Expected record index")? as usize;
                let expected = parse_csv(params.get_string(1).ok_or("Expected candidate_set")?);
                let hearth = hearth_of(&ctx)?;
                let records = read_records(&hearth)?;
                let record = records
                    .get(idx - 1)
                    .ok_or_else(|| format!("No record at index {}", idx))?;
                if record.candidate_set == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "record {}: expected candidate_set {:?}, got {:?}",
                        idx, expected, record.candidate_set
                    ))
                }
            },
        ),
        check_def(
            "every abstention ledger record has conversation_id {string}",
            &[(HEARTH_KEY, "PathBuf")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected id")?.to_string();
                let hearth = hearth_of(&ctx)?;
                let records = read_records(&hearth)?;
                if records.is_empty() {
                    return Err("No records to check".to_string());
                }
                for (i, record) in records.iter().enumerate() {
                    if record.conversation_id != expected {
                        return Err(format!(
                            "record {}: expected conversation_id {:?}, got {:?}",
                            i + 1,
                            expected,
                            record.conversation_id
                        ));
                    }
                }
                Ok(())
            },
        ),
        // ── line-atomicity seam ──
        step_def(
            "{int} concurrent abstaining turns each append {int} records to the ledger",
            &[(HEARTH_KEY, "PathBuf")],
            &[(HEARTH_KEY, "PathBuf"), (HEARTH_HANDLE_KEY, "RetainedTempDir")],
            |ctx, params| {
                let threads = params.get_int(0).ok_or("Expected thread count")? as usize;
                let per_thread = params.get_int(1).ok_or("Expected per-thread count")? as usize;
                let hearth = hearth_of(&ctx)?;
                // Each thread stands in for an independent `anvil-hooks route-turn`
                // process appending to the SAME ledger concurrently.
                let handles: Vec<_> = (0..threads)
                    .map(|t| {
                        let hearth = hearth.clone();
                        std::thread::spawn(move || {
                            for i in 0..per_thread {
                                let record = AbstentionRecord {
                                    message: format!(
                                        "thread {t} turn {i} — a reasonably sized abstention message so the append is a non-trivial write"
                                    ),
                                    recent_context: format!("prior context for t{t} i{i}"),
                                    conversation_id: format!("conv-{t}-{i}"),
                                    candidate_set: vec![
                                        "daily_recap".to_string(),
                                        "weekly_recap".to_string(),
                                    ],
                                    at: now_rfc3339(),
                                    source: "test-harness".to_string(),
                                };
                                record_abstention(
                                    true,
                                    &RouteTurnOutcome::NoMatch,
                                    Some(hearth.as_path()),
                                    &record,
                                );
                            }
                        })
                    })
                    .collect();
                for h in handles {
                    h.join().map_err(|_| "an appender thread panicked".to_string())?;
                }
                Ok(ctx)
            },
        ),
        check_def(
            "the abstention ledger physical line count is {int}",
            &[(HEARTH_KEY, "PathBuf")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let hearth = hearth_of(&ctx)?;
                let text = std::fs::read_to_string(ledger_path(&hearth))
                    .map_err(|e| format!("read ledger: {e}"))?;
                let actual = text.lines().filter(|l| !l.trim().is_empty()).count();
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "expected {} physical (non-empty) lines, got {} — glued/torn appends collapse or split lines",
                        expected, actual
                    ))
                }
            },
        ),
        check_def(
            "every abstention ledger line parses as exactly one record",
            &[(HEARTH_KEY, "PathBuf")],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let text = std::fs::read_to_string(ledger_path(&hearth))
                    .map_err(|e| format!("read ledger: {e}"))?;
                let mut total = 0usize;
                let mut bad: Vec<&str> = Vec::new();
                for line in text.lines() {
                    if line.trim().is_empty() {
                        continue;
                    }
                    total += 1;
                    // serde_json::from_str rejects trailing content, so a glued
                    // `{..}{..}` line fails here.
                    if serde_json::from_str::<AbstentionRecord>(line).is_err() {
                        bad.push(line);
                    }
                }
                if bad.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "{} of {} ledger lines are not a single record (glued/torn appends); first bad line: {:?}",
                        bad.len(),
                        total,
                        bad.first()
                    ))
                }
            },
        ),
        // ── pure flag-resolution seam ──
        step_def(
            "the abstention ledger flag value is {string}",
            &[],
            &[(FLAG_RESULT_KEY, "bool")],
            |ctx, params| {
                let value = params.get_string(0).ok_or("Expected flag value")?.to_string();
                let mut out = ctx;
                out.set(FLAG_RESULT_KEY, ledger_flag_on(Some(&value)));
                Ok(out)
            },
        ),
        step_def(
            "the abstention ledger flag is unset",
            &[],
            &[(FLAG_RESULT_KEY, "bool")],
            |ctx, _params| {
                let mut out = ctx;
                out.set(FLAG_RESULT_KEY, ledger_flag_on(None));
                Ok(out)
            },
        ),
        check_def(
            "the abstention ledger flag resolves enabled",
            &[(FLAG_RESULT_KEY, "bool")],
            |ctx, _params| match ctx.get::<bool>(FLAG_RESULT_KEY) {
                Some(true) => Ok(()),
                other => Err(format!("Expected enabled, got {:?}", other)),
            },
        ),
        check_def(
            "the abstention ledger flag resolves disabled",
            &[(FLAG_RESULT_KEY, "bool")],
            |ctx, _params| match ctx.get::<bool>(FLAG_RESULT_KEY) {
                Some(false) => Ok(()),
                other => Err(format!("Expected disabled, got {:?}", other)),
            },
        ),
    ]
}
