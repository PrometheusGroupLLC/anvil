//! Step module for `hearth_local_engine_flags.feature`.
//!
//! Exercises the engine's HEARTH-LOCAL ENGINE FLAGS merge HERMETICALLY — no live
//! engine, no socket, no global-env mutation. The pure resolution
//! (`resolve_hearth_local_flags`) is driven against a real throwaway hearth dir
//! holding an `engine-flags.env`, with the process env supplied as an IN-MEMORY map
//! so the precedence logic (real env > file > default) is deterministic and
//! parallel-safe. The applied-override set is then bound to REAL behavior by feeding
//! the merged `ANVIL_ABSTENTION_LEDGER` value through `ledger_flag_on`, proving the
//! merge actually flips the toggle the engine reads.

use anvil_test_support::retained_temp_dir;
use anvil_core::domain::hooks::route_turn::RouteTurnOutcome;
use anvil_engine::abstention_ledger::{
    abstention_ledger_enabled, is_harness_generated, ledger_flag_on, now_rfc3339,
    record_abstention, AbstentionRecord,
};
use anvil_engine::engine_flags::{
    claimed_evidence_gate_enabled_for_hearth, claimed_evidence_gate_value_is_on,
    install_hearth_local_flags, resolve_hearth_local_flags, ResolvedFlags,
    ENFORCE_CLAIMED_EVIDENCE_FLAG, FLAGS_FILE_NAME,
};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// Serializes the ONE scenario that mutates the REAL process env (`set_var`) so it
/// never races another env-touching step. The window is tiny: merge → read → write →
/// restore, all inside a single locked step.
static REAL_ENV_MERGE_LOCK: Mutex<()> = Mutex::new(());

const HEARTH_KEY: &str = "ef_hearth";
const HANDLE_KEY: &str = "ef_hearth_handle";
const ENV_KEY: &str = "ef_env";
const RESOLVED_KEY: &str = "ef_resolved";
const LANE_A_KEY: &str = "ef_claimed_evidence_lane_a";
const LANE_A_HANDLE_KEY: &str = "ef_claimed_evidence_lane_a_handle";
const LANE_B_KEY: &str = "ef_claimed_evidence_lane_b";
const LANE_B_HANDLE_KEY: &str = "ef_claimed_evidence_lane_b_handle";

const LEDGER_KEY: &str = "ANVIL_ABSTENTION_LEDGER";
const AUTHORING_OBLIGATION_KEY: &str = "ANVIL_ENFORCE_EVIDENCE_OBLIGATION";

fn hearth_of(ctx: &Context) -> Result<PathBuf, String> {
    ctx.get::<PathBuf>(HEARTH_KEY)
        .cloned()
        .ok_or_else(|| "No engine-flags hearth in context".to_string())
}

fn env_of(ctx: &Context) -> HashMap<String, String> {
    ctx.get::<HashMap<String, String>>(ENV_KEY)
        .cloned()
        .unwrap_or_default()
}

fn resolved_of(ctx: &Context) -> Result<ResolvedFlags, String> {
    ctx.get::<ResolvedFlags>(RESOLVED_KEY)
        .cloned()
        .ok_or_else(|| "No resolved flags in context (resolve step not run)".to_string())
}

/// The effective value the engine would read for `key`: the applied override when
/// the file supplied it, else the process-env value (env wins), else `None`.
fn effective_value(
    resolved: &ResolvedFlags,
    env: &HashMap<String, String>,
    key: &str,
) -> Option<String> {
    resolved
        .to_apply
        .iter()
        .find(|e| e.key == key)
        .map(|e| e.value.clone())
        .or_else(|| env.get(key).cloned())
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ── seed a throwaway hearth carrying an engine-flags.env ──
        step_def(
            "a hearth-local engine-flags file with:",
            &[],
            &[(HEARTH_KEY, "PathBuf"), (HANDLE_KEY, "RetainedTempDir")],
            |ctx, params| {
                let content = params
                    .doc_string()
                    .ok_or("Expected engine-flags file content as a doc string")?
                    .to_string();
                let (handle, tmp) = retained_temp_dir("anvil-engine-flags-")?;
                std::fs::write(tmp.join(FLAGS_FILE_NAME), content)
                    .map_err(|e| format!("write engine-flags.env: {e}"))?;
                let mut out = ctx;
                out.set(HEARTH_KEY, tmp);
                out.set(HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        // ── seed a throwaway hearth with NO engine-flags.env ──
        step_def(
            "a hearth with no engine-flags file",
            &[],
            &[(HEARTH_KEY, "PathBuf"), (HANDLE_KEY, "RetainedTempDir")],
            |ctx, _params| {
                let (handle, tmp) = retained_temp_dir("anvil-engine-flags-none-")?;
                let mut out = ctx;
                out.set(HEARTH_KEY, tmp);
                out.set(HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        // ── the in-memory process env: empty ──
        step_def(
            "the process environment sets nothing",
            &[(HEARTH_KEY, "PathBuf")],
            &[
                (HEARTH_KEY, "PathBuf"),
                (HANDLE_KEY, "RetainedTempDir"),
                (ENV_KEY, "EnvMap"),
            ],
            |ctx, _params| {
                let mut out = ctx;
                out.set(ENV_KEY, HashMap::<String, String>::new());
                Ok(out)
            },
        ),
        // ── the in-memory process env: an explicit ledger flag ──
        step_def(
            "the process environment sets ANVIL_ABSTENTION_LEDGER to {string}",
            &[(HEARTH_KEY, "PathBuf")],
            &[
                (HEARTH_KEY, "PathBuf"),
                (HANDLE_KEY, "RetainedTempDir"),
                (ENV_KEY, "EnvMap"),
            ],
            |ctx, params| {
                let value = params
                    .get_string(0)
                    .ok_or("Expected env value")?
                    .to_string();
                let mut env = HashMap::<String, String>::new();
                env.insert(LEDGER_KEY.to_string(), value);
                let mut out = ctx;
                out.set(ENV_KEY, env);
                Ok(out)
            },
        ),
        // ── run the pure resolution against the hearth + in-memory env ──
        step_def(
            "the engine resolves the hearth-local flags",
            &[(HEARTH_KEY, "PathBuf"), (ENV_KEY, "EnvMap")],
            &[
                (HEARTH_KEY, "PathBuf"),
                (HANDLE_KEY, "RetainedTempDir"),
                (ENV_KEY, "EnvMap"),
                (RESOLVED_KEY, "ResolvedFlags"),
            ],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let env = env_of(&ctx);
                let resolved =
                    resolve_hearth_local_flags(Some(hearth.as_path()), |k| env.get(k).cloned());
                let mut out = ctx;
                out.set(RESOLVED_KEY, resolved);
                Ok(out)
            },
        ),
        // ── checks: applied overrides ──
        check_def(
            "the merge applies {string} with value {string}",
            &[(RESOLVED_KEY, "ResolvedFlags")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected key")?.to_string();
                let value = params.get_string(1).ok_or("Expected value")?.to_string();
                let resolved = resolved_of(&ctx)?;
                match resolved.to_apply.iter().find(|e| e.key == key) {
                    Some(entry) if entry.value == value => Ok(()),
                    Some(entry) => Err(format!(
                        "{key}: expected applied value {value:?}, got {:?}",
                        entry.value
                    )),
                    None => Err(format!(
                        "expected {key} to be applied, but it was not (applied: {:?})",
                        resolved.to_apply.iter().map(|e| &e.key).collect::<Vec<_>>()
                    )),
                }
            },
        ),
        check_def(
            "the merge does not apply ANVIL_ABSTENTION_LEDGER",
            &[(RESOLVED_KEY, "ResolvedFlags")],
            |ctx, _params| {
                let resolved = resolved_of(&ctx)?;
                if resolved.to_apply.iter().any(|e| e.key == LEDGER_KEY) {
                    Err("expected ANVIL_ABSTENTION_LEDGER NOT to be applied (process env should win), but it was".to_string())
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the merge applies no keys",
            &[(RESOLVED_KEY, "ResolvedFlags")],
            |ctx, _params| {
                let resolved = resolved_of(&ctx)?;
                if resolved.to_apply.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "expected no keys applied, got {:?}",
                        resolved.to_apply.iter().map(|e| &e.key).collect::<Vec<_>>()
                    ))
                }
            },
        ),
        check_def(
            "the merge applies exactly {int} key",
            &[(RESOLVED_KEY, "ResolvedFlags")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let resolved = resolved_of(&ctx)?;
                let actual = resolved.to_apply.len();
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "expected exactly {expected} key(s) applied, got {actual}: {:?}",
                        resolved.to_apply.iter().map(|e| &e.key).collect::<Vec<_>>()
                    ))
                }
            },
        ),
        check_def(
            "the merge ignores the non-ANVIL_ key {string}",
            &[(RESOLVED_KEY, "ResolvedFlags")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected key")?.to_string();
                let resolved = resolved_of(&ctx)?;
                if resolved.to_apply.iter().any(|e| e.key == key) {
                    return Err(format!("expected {key} to be ignored, but it was applied"));
                }
                if resolved.ignored.iter().any(|k| k == &key) {
                    Ok(())
                } else {
                    Err(format!(
                        "expected {key} in the ignored set, got ignored={:?}",
                        resolved.ignored
                    ))
                }
            },
        ),
        // ── checks: the merged value actually drives the real toggle ──
        check_def(
            "the merged ANVIL_ABSTENTION_LEDGER value resolves the abstention ledger enabled",
            &[(RESOLVED_KEY, "ResolvedFlags"), (ENV_KEY, "EnvMap")],
            |ctx, _params| {
                let resolved = resolved_of(&ctx)?;
                let env = env_of(&ctx);
                let value = effective_value(&resolved, &env, LEDGER_KEY);
                if ledger_flag_on(value.as_deref()) {
                    Ok(())
                } else {
                    Err(format!(
                        "expected the merged ANVIL_ABSTENTION_LEDGER value to enable the ledger, got {value:?}"
                    ))
                }
            },
        ),
        // ── REAL install I/O wrapper → REAL consumer read → REAL ledger append ──
        // Blocker-2 end-to-end proof: exercises the actual `install_hearth_local_flags`
        // (real fs read + real `set_var`), then the actual `abstention_ledger_enabled`
        // consumer (which reads the merged process env exactly as the anvil-hooks
        // route-turn process does), then a real `record_abstention` append. The real
        // env mutation is serialized + save/restored so the rest of the suite stays
        // hermetic. Env WINS in `router_config`, so once the file sets the flag the
        // consumer resolves it without touching any router.json.
        check_def(
            "installing the hearth-local flags enables the real abstention ledger and appends one record",
            &[(HEARTH_KEY, "PathBuf")],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let _guard = REAL_ENV_MERGE_LOCK.lock().unwrap();

                // Save + clear the real env key so the FILE is what flips it on.
                let prior = std::env::var(LEDGER_KEY).ok();
                std::env::remove_var(LEDGER_KEY);
                // Isolate the router-config FILE fallback: point it at a path that
                // does not exist so `router_config`'s file branch resolves to None.
                // Now the ONLY way `abstention_ledger_enabled()` can be true is the
                // env the merge `set_var`s — which is exactly what we are proving.
                let prior_cfg = std::env::var("ANVIL_ROUTER_CONFIG_FILE").ok();
                std::env::set_var(
                    "ANVIL_ROUTER_CONFIG_FILE",
                    hearth.join("no-such-router-config.json"),
                );

                // REAL install: reads the real (now-unset) env, reads the real file,
                // `set_var`s the override into the real process env.
                let resolved = install_hearth_local_flags(Some(hearth.as_path()));
                let applied_from_file = resolved.to_apply.iter().any(|e| e.key == LEDGER_KEY);

                // REAL consumer read — the same call the anvil-hooks route-turn
                // process makes; it must now see the file's opt-in via the merged env.
                let enabled = abstention_ledger_enabled();

                // REAL ledger append against the same hearth, on a genuine abstention.
                let record = AbstentionRecord {
                    message: "how do I do a thing with no playbook?".to_string(),
                    recent_context: String::new(),
                    conversation_id: "conv-engine-flags-e2e".to_string(),
                    candidate_set: Vec::new(),
                    at: now_rfc3339(),
                    source: "engine-flags-test".to_string(),
                };
                let wrote = record_abstention(
                    enabled,
                    &RouteTurnOutcome::NoMatch,
                    Some(hearth.as_path()),
                    &record,
                );
                let ledger_line_count = std::fs::read_to_string(
                    hearth.join("abstentions").join("ledger.jsonl"),
                )
                .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count())
                .unwrap_or(0);

                // Restore the real env before releasing the lock.
                match prior {
                    Some(v) => std::env::set_var(LEDGER_KEY, v),
                    None => std::env::remove_var(LEDGER_KEY),
                }
                match prior_cfg {
                    Some(v) => std::env::set_var("ANVIL_ROUTER_CONFIG_FILE", v),
                    None => std::env::remove_var("ANVIL_ROUTER_CONFIG_FILE"),
                }

                if !applied_from_file {
                    return Err("install did not apply ANVIL_ABSTENTION_LEDGER from the file".to_string());
                }
                if !enabled {
                    return Err("abstention_ledger_enabled() was false after the file merge — the flag never reached the real consumer".to_string());
                }
                if !wrote || ledger_line_count != 1 {
                    return Err(format!(
                        "expected exactly one appended ledger line (wrote={wrote}, lines={ledger_line_count})"
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the effective ANVIL_ABSTENTION_LEDGER value resolves the abstention ledger disabled",
            &[(RESOLVED_KEY, "ResolvedFlags"), (ENV_KEY, "EnvMap")],
            |ctx, _params| {
                let resolved = resolved_of(&ctx)?;
                let env = env_of(&ctx);
                let value = effective_value(&resolved, &env, LEDGER_KEY);
                if ledger_flag_on(value.as_deref()) {
                    Err(format!(
                        "expected the effective ANVIL_ABSTENTION_LEDGER value to DISABLE the ledger (process env off wins), got {value:?}"
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "claimed-evidence gate value {string} resolves {string}",
            &[],
            |_, params| {
                let raw = params.get_string(0).ok_or("Expected claimed-evidence value")?;
                let expected = params.get_string(1).ok_or("Expected enabled or disabled")?;
                let value = match raw {
                    "unset" => None,
                    "empty" => Some(""),
                    other => Some(other),
                };
                let actual = claimed_evidence_gate_value_is_on(value);
                let expected = match expected {
                    "enabled" => true,
                    "disabled" => false,
                    other => return Err(format!("Unknown gate outcome '{}'", other)),
                };
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "claimed-evidence value {:?}: expected enabled={}, got {}",
                        value, expected, actual
                    ))
                }
            },
        ),
        step_def(
            "two request lanes where only the first opts into claimed-evidence enforcement",
            &[],
            &[
                (LANE_A_KEY, "PathBuf"),
                (LANE_A_HANDLE_KEY, "RetainedTempDir"),
                (LANE_B_KEY, "PathBuf"),
                (LANE_B_HANDLE_KEY, "RetainedTempDir"),
            ],
            |ctx, _| {
                let (lane_a_handle, lane_a) = retained_temp_dir("anvil-evidence-lane-a-")?;
                let (lane_b_handle, lane_b) = retained_temp_dir("anvil-evidence-lane-b-")?;
                std::fs::write(
                    lane_a.join(FLAGS_FILE_NAME),
                    format!("{}=on\n", ENFORCE_CLAIMED_EVIDENCE_FLAG),
                )
                .map_err(|error| format!("write opted-in lane flags: {}", error))?;
                let mut out = ctx;
                out.set(LANE_A_KEY, lane_a);
                out.set(LANE_A_HANDLE_KEY, lane_a_handle);
                out.set(LANE_B_KEY, lane_b);
                out.set(LANE_B_HANDLE_KEY, lane_b_handle);
                Ok(out)
            },
        ),
        step_def(
            "the process environment enables only the authoring evidence-obligation flag",
            &[(LANE_A_KEY, "PathBuf"), (LANE_B_KEY, "PathBuf")],
            &[
                (LANE_A_KEY, "PathBuf"),
                (LANE_A_HANDLE_KEY, "RetainedTempDir"),
                (LANE_B_KEY, "PathBuf"),
                (LANE_B_HANDLE_KEY, "RetainedTempDir"),
                (ENV_KEY, "EnvMap"),
            ],
            |ctx, _| {
                let mut env = HashMap::new();
                env.insert(AUTHORING_OBLIGATION_KEY.to_string(), "1".to_string());
                let mut out = ctx;
                out.set(ENV_KEY, env);
                Ok(out)
            },
        ),
        check_def(
            "claimed-evidence enforcement is enabled only for the opted-in request lane",
            &[(LANE_A_KEY, "PathBuf"), (LANE_B_KEY, "PathBuf"), (ENV_KEY, "EnvMap")],
            |ctx, _| {
                let lane_a = ctx
                    .get::<PathBuf>(LANE_A_KEY)
                    .ok_or("Missing opted-in request lane")?;
                let lane_b = ctx
                    .get::<PathBuf>(LANE_B_KEY)
                    .ok_or("Missing default-off request lane")?;
                let env = env_of(&ctx);
                let lane_a_enabled = claimed_evidence_gate_enabled_for_hearth(lane_a, |key| {
                    env.get(key).cloned()
                });
                let lane_b_enabled = claimed_evidence_gate_enabled_for_hearth(lane_b, |key| {
                    env.get(key).cloned()
                });
                if lane_a_enabled && !lane_b_enabled {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected only lane A enabled; lane_a={}, lane_b={}",
                        lane_a_enabled, lane_b_enabled
                    ))
                }
            },
        ),
        check_def(
            "startup installation does not globalize claimed-evidence enforcement",
            &[(LANE_A_KEY, "PathBuf")],
            |ctx, _| {
                let lane_a = ctx
                    .get::<PathBuf>(LANE_A_KEY)
                    .ok_or("Missing opted-in request lane")?;
                let _guard = REAL_ENV_MERGE_LOCK.lock().unwrap();
                let prior = std::env::var(ENFORCE_CLAIMED_EVIDENCE_FLAG).ok();
                std::env::remove_var(ENFORCE_CLAIMED_EVIDENCE_FLAG);
                let resolved = install_hearth_local_flags(Some(lane_a));
                let installed = resolved
                    .to_apply
                    .iter()
                    .any(|entry| entry.key == ENFORCE_CLAIMED_EVIDENCE_FLAG);
                let process_value = std::env::var(ENFORCE_CLAIMED_EVIDENCE_FLAG).ok();
                match prior {
                    Some(value) => std::env::set_var(ENFORCE_CLAIMED_EVIDENCE_FLAG, value),
                    None => std::env::remove_var(ENFORCE_CLAIMED_EVIDENCE_FLAG),
                }
                if !installed && process_value.is_none() {
                    Ok(())
                } else {
                    Err(format!(
                        "Lane-local gate escaped startup filtering: installed={}, process={:?}",
                        installed, process_value
                    ))
                }
            },
        ),
        step_def(
            "the abstention ledger considers the message {string}",
            &[],
            &[("abst_recorded", "String")],
            |_ctx, params| {
                let m = params.get_string(0).ok_or("Expected message")?.to_string();
                // The pure decision only. Whether a harness block is DEMAND is
                // decidable from the text; the surrounding append path (flag,
                // outcome, hearth) is already covered by the scenarios above.
                let mut out = Context::new();
                out.set(
                    "abst_recorded",
                    if is_harness_generated(&m) { "no" } else { "yes" }.to_string(),
                );
                Ok(out)
            },
        ),
        check_def(
            "the abstention is recorded is {string}",
            &[("abst_recorded", "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected yes/no")?.to_string();
                let got = ctx.get::<String>("abst_recorded").ok_or("No verdict")?;
                if *got == want {
                    Ok(())
                } else {
                    Err(format!("recorded was {got:?}, expected {want:?}"))
                }
            },
        ),
    ]
}
