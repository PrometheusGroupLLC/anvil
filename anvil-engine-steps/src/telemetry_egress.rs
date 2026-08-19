//! Step module for `telemetry_egress.feature`.
//!
//! Exercises anvil's unified content-free telemetry HERMETICALLY — no live
//! engine, no socket. It drives the recorder's testable emit seams
//! (`anvil_engine::telemetry::emit_*`) against a throwaway `~/.anvil` dir, then
//! asserts against the raw `anvil-telemetry.jsonl` on disk:
//!
//! 1. a route decision writes a `route_decision` row;
//! 2. the row is CONTENT-FREE — the raw actor id never appears, only its salted
//!    hash (proving `Telemetry::hash_id` is on the egress path);
//! 3. the recorded rows roll up into a validating [`Envelope`].
//!
//! The recorder is rebuilt from the temp dir on demand; the per-install salt is
//! persisted under the same dir, so hashes are stable across steps.

use anvil_engine::telemetry::{
    build_recorder_at, emit_abstention, emit_route_decision, emit_ws_session, validate_envelope,
    Telemetry, Window,
};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;

const DIR_KEY: &str = "tel_dir";
const HANDLE_KEY: &str = "tel_handle";
const ACTOR_KEY: &str = "tel_actor";
const HASH_KEY: &str = "tel_hash";

fn dir_of(ctx: &Context) -> Result<PathBuf, String> {
    ctx.get::<PathBuf>(DIR_KEY)
        .cloned()
        .ok_or_else(|| "No telemetry dir in context".to_string())
}

fn recorder_of(ctx: &Context) -> Result<Telemetry, String> {
    let dir = dir_of(ctx)?;
    build_recorder_at(&dir).ok_or_else(|| "could not build telemetry recorder".to_string())
}

fn telemetry_file(dir: &PathBuf) -> PathBuf {
    dir.join("anvil-telemetry.jsonl")
}

fn read_telemetry_text(dir: &PathBuf) -> String {
    std::fs::read_to_string(telemetry_file(dir)).unwrap_or_default()
}

fn parse_num(raw: &str, what: &str) -> Result<f64, String> {
    raw.trim()
        .parse::<f64>()
        .map_err(|e| format!("bad {what} {raw:?}: {e}"))
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a throwaway anvil home for telemetry",
            &[],
            &[(DIR_KEY, "PathBuf"), (HANDLE_KEY, "RetainedTempDir")],
            |ctx, _params| {
                let (handle, tmp) = anvil_test_support::retained_temp_dir("anvil-telemetry-")?;
                let mut out = ctx;
                out.set(DIR_KEY, tmp);
                out.set(HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "a route decision is recorded for actor {string} with outcome {string} target tier {string} confidence {string} and latency {string}",
            &[(DIR_KEY, "PathBuf")],
            &[
                (DIR_KEY, "PathBuf"),
                (HANDLE_KEY, "RetainedTempDir"),
                (ACTOR_KEY, "String"),
                (HASH_KEY, "String"),
            ],
            |ctx, params| {
                let actor = params.get_string(0).ok_or("Expected actor")?.to_string();
                let outcome = params.get_string(1).ok_or("Expected outcome")?.to_string();
                let tier = params.get_string(2).ok_or("Expected tier")?.to_string();
                let confidence = parse_num(params.get_string(3).ok_or("Expected confidence")?, "confidence")?;
                let latency = parse_num(params.get_string(4).ok_or("Expected latency")?, "latency")?;
                let tel = recorder_of(&ctx)?;
                let hash = tel.hash_id(&actor);
                emit_route_decision(
                    &tel,
                    Some(&actor),
                    &outcome,
                    &tier,
                    Some(confidence),
                    Some(latency),
                )
                .map_err(|e| format!("emit_route_decision failed: {e}"))?;
                let mut out = ctx;
                out.set(ACTOR_KEY, actor);
                out.set(HASH_KEY, hash);
                Ok(out)
            },
        ),
        step_def(
            "an abstention is recorded with reason {string}",
            &[(DIR_KEY, "PathBuf")],
            &[(DIR_KEY, "PathBuf"), (HANDLE_KEY, "RetainedTempDir")],
            |ctx, params| {
                let reason = params.get_string(0).ok_or("Expected reason")?.to_string();
                let tel = recorder_of(&ctx)?;
                emit_abstention(&tel, &reason).map_err(|e| format!("emit_abstention failed: {e}"))?;
                Ok(ctx)
            },
        ),
        step_def(
            "a ws session is recorded with duration {string} and close reason {string}",
            &[(DIR_KEY, "PathBuf")],
            &[(DIR_KEY, "PathBuf"), (HANDLE_KEY, "RetainedTempDir")],
            |ctx, params| {
                let duration = parse_num(params.get_string(0).ok_or("Expected duration")?, "duration")?;
                let reason = params.get_string(1).ok_or("Expected close reason")?.to_string();
                let tel = recorder_of(&ctx)?;
                emit_ws_session(&tel, duration, &reason)
                    .map_err(|e| format!("emit_ws_session failed: {e}"))?;
                Ok(ctx)
            },
        ),
        check_def(
            "the anvil telemetry file contains a {string} row",
            &[(DIR_KEY, "PathBuf")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected event kind")?;
                let dir = dir_of(&ctx)?;
                let text = read_telemetry_text(&dir);
                let needle = format!("\"event_kind\":\"{kind}\"");
                if text.contains(&needle) {
                    Ok(())
                } else {
                    Err(format!(
                        "expected a {kind:?} row in the telemetry file, got:\n{text}"
                    ))
                }
            },
        ),
        check_def(
            "the raw actor id {string} does not appear in the anvil telemetry file",
            &[(DIR_KEY, "PathBuf")],
            |ctx, params| {
                let raw = params.get_string(0).ok_or("Expected raw id")?;
                let dir = dir_of(&ctx)?;
                let text = read_telemetry_text(&dir);
                if text.contains(raw) {
                    Err(format!(
                        "raw actor id {raw:?} LEAKED into the telemetry file:\n{text}"
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the salted actor hash appears in the anvil telemetry file",
            &[(DIR_KEY, "PathBuf"), (HASH_KEY, "String")],
            |ctx, _params| {
                let hash = ctx
                    .get::<String>(HASH_KEY)
                    .cloned()
                    .ok_or("No actor hash in context")?;
                let dir = dir_of(&ctx)?;
                let text = read_telemetry_text(&dir);
                if text.contains(&hash) {
                    Ok(())
                } else {
                    Err(format!(
                        "expected salted actor hash {hash:?} in the telemetry file, got:\n{text}"
                    ))
                }
            },
        ),
        check_def(
            "a telemetry rollup over the full window yields a validating envelope",
            &[(DIR_KEY, "PathBuf")],
            |ctx, _params| {
                let tel = recorder_of(&ctx)?;
                let window = Window {
                    start: 0,
                    end: u64::MAX,
                };
                // `rollup` already validates internally; re-validate explicitly to
                // prove the emitted Envelope honors the privacy invariants.
                let env = tel
                    .rollup(window, true)
                    .map_err(|e| format!("rollup failed: {e}"))?;
                validate_envelope(&env).map_err(|e| format!("envelope did not validate: {e}"))?;
                Ok(())
            },
        ),
        check_def(
            "the rollup envelope kit id is {string}",
            &[(DIR_KEY, "PathBuf")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected kit id")?;
                let tel = recorder_of(&ctx)?;
                let window = Window {
                    start: 0,
                    end: u64::MAX,
                };
                let env = tel
                    .rollup(window, true)
                    .map_err(|e| format!("rollup failed: {e}"))?;
                if env.kit_id == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "expected kit_id {expected:?}, got {:?}",
                        env.kit_id
                    ))
                }
            },
        ),
    ]
}
