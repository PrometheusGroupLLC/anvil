//! Step module for `actor_activity_fold.feature` (core domain seam).
//!
//! Exercises the pure `fold_actor_activity` + `fold_actor_activity_across_hearths`
//! folds over in-memory `(artifact_kind, ActivityEntry)` streams. No filesystem:
//! these are pure domain folds, so the steps seed the streams directly and
//! assert against the folded result.

use anvil_core::domain::actor_activity::{
    fold_actor_activity, fold_actor_activity_across_hearths, ActorActivityResult,
};
use anvil_core::domain::shared_types::ActivityEntry;
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

// One folded stream is a Vec<(artifact_kind, Vec<ActivityEntry>)>.
type Stream = Vec<(String, Vec<ActivityEntry>)>;

const STREAM_A_KEY: &str = "aa_stream_a";
const STREAM_B_KEY: &str = "aa_stream_b";
const RESULT_KEY: &str = "aa_result";

fn column_index(table: &DataTable, name: &str) -> Result<usize, String> {
    table
        .headers
        .iter()
        .position(|h| h == name)
        .ok_or_else(|| format!("Missing '{}' column in data table", name))
}

/// Parse a `artifact_kind | actor | state | at` table into a per-artifact
/// stream. Each row is one begin-marker; rows with the same artifact_kind are
/// grouped into a single artifact entry (the fold is order-independent across
/// artifacts, so one entry per kind suffices).
fn parse_stream(table: &DataTable) -> Result<Stream, String> {
    let kind_col = column_index(table, "artifact_kind")?;
    let actor_col = column_index(table, "actor")?;
    let state_col = column_index(table, "state")?;
    let at_col = column_index(table, "at")?;
    let mut stream: Stream = Vec::new();
    for row in &table.rows {
        let artifact_kind = row[kind_col].trim().to_string();
        let entry = ActivityEntry {
            kind: "begin".to_string(),
            actor: row[actor_col].trim().to_string(),
            state: row[state_col].trim().to_string(),
            at: row[at_col].trim().to_string(),
            conversation_id: String::new(),
        };
        // Each row becomes its own artifact tuple — simplest faithful modeling.
        stream.push((artifact_kind, vec![entry]));
    }
    Ok(stream)
}

fn result_entries(ctx: &Context) -> Result<ActorActivityResult, String> {
    ctx.get::<ActorActivityResult>(RESULT_KEY)
        .cloned()
        .ok_or_else(|| "No actor activity result in context".to_string())
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "an actor activity hearth with artifact begin-markers:",
            &[],
            &[(STREAM_A_KEY, "Stream")],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let stream = parse_stream(table)?;
                let mut out = Context::new();
                out.set::<Stream>(STREAM_A_KEY, stream);
                Ok(out)
            },
        ),
        step_def(
            "an actor activity hearth with no begin-markers",
            &[],
            &[(STREAM_A_KEY, "Stream")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set::<Stream>(STREAM_A_KEY, Vec::new());
                Ok(out)
            },
        ),
        step_def(
            "an actor activity primary hearth with artifact begin-markers:",
            &[],
            &[(STREAM_A_KEY, "Stream")],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let stream = parse_stream(table)?;
                let mut out = Context::new();
                out.set::<Stream>(STREAM_A_KEY, stream);
                Ok(out)
            },
        ),
        step_def(
            "an actor activity secondary hearth with artifact begin-markers:",
            &[(STREAM_A_KEY, "Stream")],
            &[(STREAM_A_KEY, "Stream"), (STREAM_B_KEY, "Stream")],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let stream_b = parse_stream(table)?;
                let mut out = Context::new();
                if let Some(a) = ctx.get::<Stream>(STREAM_A_KEY) {
                    out.set::<Stream>(STREAM_A_KEY, a.clone());
                }
                out.set::<Stream>(STREAM_B_KEY, stream_b);
                Ok(out)
            },
        ),
        step_def(
            "the actor activity fold is computed",
            &[(STREAM_A_KEY, "Stream")],
            &[(RESULT_KEY, "ActorActivityResult")],
            |ctx, _params| {
                let stream = ctx
                    .get::<Stream>(STREAM_A_KEY)
                    .cloned()
                    .ok_or("No actor activity stream A")?;
                let result = fold_actor_activity(&stream);
                let mut out = Context::new();
                out.set::<ActorActivityResult>(RESULT_KEY, result);
                Ok(out)
            },
        ),
        step_def(
            "the actor activity fold is computed across all hearths",
            &[(STREAM_A_KEY, "Stream"), (STREAM_B_KEY, "Stream")],
            &[(RESULT_KEY, "ActorActivityResult")],
            |ctx, _params| {
                let a = ctx
                    .get::<Stream>(STREAM_A_KEY)
                    .cloned()
                    .ok_or("No actor activity stream A")?;
                let b = ctx
                    .get::<Stream>(STREAM_B_KEY)
                    .cloned()
                    .ok_or("No actor activity stream B")?;
                let result = fold_actor_activity_across_hearths(&[a, b]);
                let mut out = Context::new();
                out.set::<ActorActivityResult>(RESULT_KEY, result);
                Ok(out)
            },
        ),
        check_def(
            "the actor activity entry for actor {string} has begin count {int}",
            &[(RESULT_KEY, "ActorActivityResult")],
            |ctx, params| {
                let actor = params.get_string(0).ok_or("Expected actor")?;
                let expected = params.get_int(1).ok_or("Expected count")? as u64;
                let result = result_entries(&ctx)?;
                let entry = result
                    .entries
                    .iter()
                    .find(|e| e.actor == actor)
                    .ok_or_else(|| format!("No entry for actor '{}'", actor))?;
                if entry.begin_count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "actor '{}': expected begin count {}, got {}",
                        actor, expected, entry.begin_count
                    ))
                }
            },
        ),
        check_def(
            "the actor activity entry for actor {string} has last active {string}",
            &[(RESULT_KEY, "ActorActivityResult")],
            |ctx, params| {
                let actor = params.get_string(0).ok_or("Expected actor")?;
                let expected = params.get_string(1).ok_or("Expected last active")?;
                let result = result_entries(&ctx)?;
                let entry = result
                    .entries
                    .iter()
                    .find(|e| e.actor == actor)
                    .ok_or_else(|| format!("No entry for actor '{}'", actor))?;
                if entry.last_active == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "actor '{}': expected last active '{}', got '{}'",
                        actor, expected, entry.last_active
                    ))
                }
            },
        ),
        check_def(
            "the actor activity entry for actor {string} has playbook kinds {string}",
            &[(RESULT_KEY, "ActorActivityResult")],
            |ctx, params| {
                let actor = params.get_string(0).ok_or("Expected actor")?;
                let expected: Vec<String> = params
                    .get_string(1)
                    .ok_or("Expected kinds")?
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                let result = result_entries(&ctx)?;
                let entry = result
                    .entries
                    .iter()
                    .find(|e| e.actor == actor)
                    .ok_or_else(|| format!("No entry for actor '{}'", actor))?;
                if entry.artifact_kinds == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "actor '{}': expected playbook kinds {:?}, got {:?}",
                        actor, expected, entry.artifact_kinds
                    ))
                }
            },
        ),
        check_def(
            "the actor activity result order is {string}",
            &[(RESULT_KEY, "ActorActivityResult")],
            |ctx, params| {
                let expected: Vec<String> = params
                    .get_string(0)
                    .ok_or("Expected order")?
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect();
                let result = result_entries(&ctx)?;
                let actual: Vec<String> = result.entries.iter().map(|e| e.actor.clone()).collect();
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("expected order {:?}, got {:?}", expected, actual))
                }
            },
        ),
        check_def(
            "the actor activity result has no entries",
            &[(RESULT_KEY, "ActorActivityResult")],
            |ctx, _params| {
                let result = result_entries(&ctx)?;
                if result.entries.is_empty() {
                    Ok(())
                } else {
                    Err(format!("expected no entries, got {}", result.entries.len()))
                }
            },
        ),
    ]
}
