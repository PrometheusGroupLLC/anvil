//! Step module for `live_instances_fold.feature` (core domain seam).
//!
//! Exercises the pure `fold_live_instances` fold directly over an in-memory
//! `Vec<LiveInstanceInput>` + an injected `now`, so the LIVE vs DORMANT
//! classification (recency window OR raw-actor-present) and the new
//! per-instance detail fields (current_step, action_count, artifact_dir) can
//! be tested precisely — including the exact 24h boundary — without spinning
//! up a fixture hearth + engine process (that full-stack wiring is covered by
//! `anvil-engine/features/live_instances_rpc.feature`).

use anvil_core::domain::live_instances::{fold_live_instances, LiveInstanceInput, LiveInstances};
use anvil_core::domain::shared_types::ActivityEntry;
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use chrono::{DateTime, Utc};

const INPUTS_KEY: &str = "lif_inputs";
const RESULT_KEY: &str = "lif_result";

fn column_index(table: &DataTable, name: &str) -> Result<usize, String> {
    table
        .headers
        .iter()
        .position(|h| h == name)
        .ok_or_else(|| format!("Missing '{}' column in data table", name))
}

/// Parse the `live-instance inputs:` table into `Vec<LiveInstanceInput>`. One
/// row per instance. `actor` empty ⇒ no begin-marker at all (an instance with
/// neither a begin-marker nor a recent §0 event is DORMANT).
fn parse_inputs(table: &DataTable) -> Result<Vec<LiveInstanceInput>, String> {
    let instance_col = column_index(table, "instance")?;
    let kind_col = column_index(table, "kind")?;
    let state_col = column_index(table, "state")?;
    let actor_col = column_index(table, "actor")?;
    let begin_at_col = column_index(table, "begin_at")?;
    let action_count_col = column_index(table, "action_count")?;
    let last_step0_at_col = column_index(table, "last_step0_at")?;
    let artifact_dir_col = column_index(table, "artifact_dir")?;

    let mut inputs = Vec::new();
    for row in &table.rows {
        let actor = row[actor_col].trim().to_string();
        let activity = if actor.is_empty() {
            Vec::new()
        } else {
            vec![ActivityEntry {
                kind: "begin".to_string(),
                actor: actor.clone(),
                state: row[state_col].trim().to_string(),
                at: row[begin_at_col].trim().to_string(),
                conversation_id: String::new(),
            }]
        };
        let action_count: u32 = row[action_count_col]
            .trim()
            .parse()
            .map_err(|e| format!("bad action_count: {}", e))?;
        inputs.push(LiveInstanceInput {
            instance_id: row[instance_col].trim().to_string(),
            kind: row[kind_col].trim().to_string(),
            state: row[state_col].trim().to_string(),
            activity,
            action_count,
            last_step0_at: row[last_step0_at_col].trim().to_string(),
            artifact_dir: row[artifact_dir_col].trim().to_string(),
        });
    }
    Ok(inputs)
}

fn find<'a>(result: &'a LiveInstances, instance_id: &str) -> Option<&'a anvil_core::domain::live_instances::LiveInstance> {
    result.instances.iter().find(|i| i.instance_id == instance_id)
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "live-instance inputs:",
            &[],
            &[(INPUTS_KEY, "Vec<LiveInstanceInput>")],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let mut out = Context::new();
                out.set(INPUTS_KEY, parse_inputs(table)?);
                Ok(out)
            },
        ),
        step_def(
            "the live instances are folded with now {string}",
            &[(INPUTS_KEY, "Vec<LiveInstanceInput>")],
            &[(RESULT_KEY, "LiveInstances")],
            |ctx, params| {
                let now_str = params.get_string(0).ok_or("Expected now")?;
                let now: DateTime<Utc> = DateTime::parse_from_rfc3339(now_str)
                    .map_err(|e| format!("bad now '{}': {}", now_str, e))?
                    .with_timezone(&Utc);
                let inputs = ctx
                    .get::<Vec<LiveInstanceInput>>(INPUTS_KEY)
                    .cloned()
                    .unwrap_or_default();
                let result = fold_live_instances(&inputs, now);
                let mut out = Context::new();
                out.set(RESULT_KEY, result);
                Ok(out)
            },
        ),
        check_def(
            "the live view has an instance {string}",
            &[(RESULT_KEY, "LiveInstances")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected instance id")?;
                let result = ctx.get::<LiveInstances>(RESULT_KEY).ok_or("No live view result")?;
                find(result, key)
                    .map(|_| ())
                    .ok_or_else(|| format!("No live instance for '{}' (dormant or absent)", key))
            },
        ),
        check_def(
            "the live view has no instance {string}",
            &[(RESULT_KEY, "LiveInstances")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected instance id")?;
                let result = ctx.get::<LiveInstances>(RESULT_KEY).ok_or("No live view result")?;
                if find(result, key).is_some() {
                    Err(format!("Live instance '{}' present, expected dormant/absent", key))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the live view has idle_count {int}",
            &[(RESULT_KEY, "LiveInstances")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected idle_count")? as u32;
                let result = ctx.get::<LiveInstances>(RESULT_KEY).ok_or("No live view result")?;
                if result.idle_count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "expected idle_count {}, got {}",
                        expected, result.idle_count
                    ))
                }
            },
        ),
        check_def(
            "the live instance {string} has current_step {string}",
            &[(RESULT_KEY, "LiveInstances")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected instance id")?;
                let expected = params.get_string(1).ok_or("Expected current_step")?;
                let result = ctx.get::<LiveInstances>(RESULT_KEY).ok_or("No live view result")?;
                let inst = find(result, key).ok_or_else(|| format!("No live instance for '{}'", key))?;
                if inst.current_step == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "instance '{}' current_step '{}' != expected '{}'",
                        key, inst.current_step, expected
                    ))
                }
            },
        ),
        check_def(
            "the live instance {string} has action_count {int}",
            &[(RESULT_KEY, "LiveInstances")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected instance id")?;
                let expected = params.get_int(1).ok_or("Expected action_count")? as u32;
                let result = ctx.get::<LiveInstances>(RESULT_KEY).ok_or("No live view result")?;
                let inst = find(result, key).ok_or_else(|| format!("No live instance for '{}'", key))?;
                if inst.action_count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "instance '{}' action_count {} != expected {}",
                        key, inst.action_count, expected
                    ))
                }
            },
        ),
        check_def(
            "the live instance {string} has artifact_dir {string}",
            &[(RESULT_KEY, "LiveInstances")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected instance id")?;
                let expected = params.get_string(1).ok_or("Expected artifact_dir")?;
                let result = ctx.get::<LiveInstances>(RESULT_KEY).ok_or("No live view result")?;
                let inst = find(result, key).ok_or_else(|| format!("No live instance for '{}'", key))?;
                if inst.artifact_dir == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "instance '{}' artifact_dir '{}' != expected '{}'",
                        key, inst.artifact_dir, expected
                    ))
                }
            },
        ),
        check_def(
            "the live instance {string} has actor {string}",
            &[(RESULT_KEY, "LiveInstances")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected instance id")?;
                let expected = params.get_string(1).ok_or("Expected actor")?;
                let result = ctx.get::<LiveInstances>(RESULT_KEY).ok_or("No live view result")?;
                let inst = find(result, key).ok_or_else(|| format!("No live instance for '{}'", key))?;
                if inst.actor == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "instance '{}' actor '{}' != expected '{}'",
                        key, inst.actor, expected
                    ))
                }
            },
        ),
    ]
}
