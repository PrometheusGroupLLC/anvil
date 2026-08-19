//! Step module for `playbook_fidelity.feature` (core domain seam).
//!
//! Exercises the pure `fold_playbook_run_fidelity` +
//! `fold_playbook_run_fidelity_across_hearths` folds over in-memory
//! `ActivityLogRecord` streams. No filesystem: these are pure domain folds, so
//! the steps seed record vectors directly and assert against the folded result.
//! The terminal predicate is resolved via `SeedPlaybookRegistry` (the track seed
//! makes `completed` non-terminal and `abandoned`/`superseded` terminal).

use anvil_core::domain::playbook::registry::SeedPlaybookRegistry;
use anvil_core::domain::playbook_run_fidelity::{
    fold_playbook_run_fidelity, fold_playbook_run_fidelity_across_hearths, PlaybookRunFidelityResult,
};
use anvil_core::ports::activity_log_port::ActivityLogRecord;
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

const RECORDS_KEY: &str = "wf_records";
const RECORDS_A_KEY: &str = "wf_records_a";
const RECORDS_B_KEY: &str = "wf_records_b";
const RESULT_KEY: &str = "wf_result";

fn column_index(table: &DataTable, name: &str) -> Result<usize, String> {
    table
        .headers
        .iter()
        .position(|h| h == name)
        .ok_or_else(|| format!("Missing '{}' column in data table", name))
}

fn opt_column_index(table: &DataTable, name: &str) -> Option<usize> {
    table.headers.iter().position(|h| h == name)
}

/// Parse an optional string column: empty / "-" → `None`.
fn opt_string(row: &[String], idx: Option<usize>) -> Option<String> {
    idx.and_then(|i| {
        let v = row[i].trim();
        if v.is_empty() || v == "-" {
            None
        } else {
            Some(v.to_string())
        }
    })
}

fn parse_records(table: &DataTable) -> Result<Vec<ActivityLogRecord>, String> {
    let command_col = column_index(table, "command")?;
    let from_col = opt_column_index(table, "from_state");
    let to_col = opt_column_index(table, "to_state");
    let hash_col = opt_column_index(table, "actor_hash");
    let at_col = column_index(table, "at")?;
    let wfk_col = opt_column_index(table, "artifact_kind");
    let instance_col = opt_column_index(table, "playbook_run_id");
    let outcome_col = opt_column_index(table, "outcome");
    let mut records = Vec::new();
    for row in &table.rows {
        records.push(ActivityLogRecord {
            command: row[command_col].trim().to_string(),
            outcome: outcome_col
                .map(|i| row[i].trim().to_string())
                .unwrap_or_default(),
            artifact_kind: wfk_col
                .map(|i| row[i].trim().to_string())
                .unwrap_or_default(),
            from_state: from_col
                .map(|i| row[i].trim().to_string())
                .unwrap_or_default(),
            to_state: to_col
                .map(|i| row[i].trim().to_string())
                .unwrap_or_default(),
            actor_hash: opt_string(row, hash_col),
            at: row[at_col].trim().to_string(),
            source: String::new(),
            conversation_hash: None,
            project_label: None,
            playbook_run_id: opt_string(row, instance_col),
            call_state: None,
        });
    }
    Ok(records)
}

fn completion_for<'a>(
    result: &'a PlaybookRunFidelityResult,
    kind: &str,
) -> Option<&'a anvil_core::domain::playbook_run_fidelity::KindCompletion> {
    result.completion.iter().find(|c| c.kind == kind)
}

fn parse_bool(value: &str) -> Result<bool, String> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("expected true or false, got '{}'", value)),
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a playbook fidelity record stream:",
            &[],
            &[(RECORDS_KEY, "Vec<ActivityLogRecord>")],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let records = parse_records(table)?;
                let mut out = Context::new();
                out.set(RECORDS_KEY, records);
                Ok(out)
            },
        ),
        step_def(
            "an empty playbook fidelity record stream",
            &[],
            &[(RECORDS_KEY, "Vec<ActivityLogRecord>")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set(RECORDS_KEY, Vec::<ActivityLogRecord>::new());
                Ok(out)
            },
        ),
        step_def(
            "playbook fidelity hearth A stream:",
            &[],
            &[(RECORDS_A_KEY, "Vec<ActivityLogRecord>")],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let records = parse_records(table)?;
                let mut out = Context::new();
                out.set(RECORDS_A_KEY, records);
                Ok(out)
            },
        ),
        step_def(
            "playbook fidelity hearth B stream:",
            &[(RECORDS_A_KEY, "Vec<ActivityLogRecord>")],
            &[
                (RECORDS_B_KEY, "Vec<ActivityLogRecord>"),
                (RECORDS_A_KEY, "Vec<ActivityLogRecord>"),
            ],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let records = parse_records(table)?;
                let a = ctx
                    .get::<Vec<ActivityLogRecord>>(RECORDS_A_KEY)
                    .cloned()
                    .unwrap_or_default();
                let mut out = Context::new();
                out.set(RECORDS_B_KEY, records);
                out.set(RECORDS_A_KEY, a);
                Ok(out)
            },
        ),
        step_def(
            "the playbook fidelity is folded",
            &[(RECORDS_KEY, "Vec<ActivityLogRecord>")],
            &[(RESULT_KEY, "PlaybookRunFidelityResult")],
            |ctx, _params| {
                let records = ctx
                    .get::<Vec<ActivityLogRecord>>(RECORDS_KEY)
                    .cloned()
                    .unwrap_or_default();
                let result = fold_playbook_run_fidelity(&records, &SeedPlaybookRegistry);
                let mut out = Context::new();
                out.set(RESULT_KEY, result);
                Ok(out)
            },
        ),
        step_def(
            "the playbook fidelity is folded across both hearths",
            &[
                (RECORDS_A_KEY, "Vec<ActivityLogRecord>"),
                (RECORDS_B_KEY, "Vec<ActivityLogRecord>"),
            ],
            &[(RESULT_KEY, "PlaybookRunFidelityResult")],
            |ctx, _params| {
                let a = ctx
                    .get::<Vec<ActivityLogRecord>>(RECORDS_A_KEY)
                    .cloned()
                    .unwrap_or_default();
                let b = ctx
                    .get::<Vec<ActivityLogRecord>>(RECORDS_B_KEY)
                    .cloned()
                    .unwrap_or_default();
                let result = fold_playbook_run_fidelity_across_hearths(&[a, b], &SeedPlaybookRegistry);
                let mut out = Context::new();
                out.set(RESULT_KEY, result);
                Ok(out)
            },
        ),
        check_def(
            "the playbook fidelity completion for {string} has begun {int}",
            &[(RESULT_KEY, "PlaybookRunFidelityResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let expected = params.get_int(1).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<PlaybookRunFidelityResult>(RESULT_KEY)
                    .ok_or("No playbook fidelity result")?;
                let entry = completion_for(result, &kind)
                    .ok_or_else(|| format!("No completion entry for '{}'", kind))?;
                if entry.begun == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "completion '{}': expected begun {}, got {}",
                        kind, expected, entry.begun
                    ))
                }
            },
        ),
        check_def(
            "the playbook fidelity completion for {string} has terminal {int}",
            &[(RESULT_KEY, "PlaybookRunFidelityResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let expected = params.get_int(1).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<PlaybookRunFidelityResult>(RESULT_KEY)
                    .ok_or("No playbook fidelity result")?;
                let entry = completion_for(result, &kind)
                    .ok_or_else(|| format!("No completion entry for '{}'", kind))?;
                if entry.terminal == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "completion '{}': expected terminal {}, got {}",
                        kind, expected, entry.terminal
                    ))
                }
            },
        ),
        check_def(
            "the playbook fidelity completion has {int} entries",
            &[(RESULT_KEY, "PlaybookRunFidelityResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let result = ctx
                    .get::<PlaybookRunFidelityResult>(RESULT_KEY)
                    .ok_or("No playbook fidelity result")?;
                if result.completion.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} completion entries, got {}",
                        expected,
                        result.completion.len()
                    ))
                }
            },
        ),
        check_def(
            "the playbook fidelity has {int} dangling instances",
            &[(RESULT_KEY, "PlaybookRunFidelityResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<PlaybookRunFidelityResult>(RESULT_KEY)
                    .ok_or("No playbook fidelity result")?;
                if result.dangling_instances == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} dangling instances, got {}",
                        expected, result.dangling_instances
                    ))
                }
            },
        ),
        check_def(
            "the playbook fidelity dangling for {string} has count {int}",
            &[(RESULT_KEY, "PlaybookRunFidelityResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let expected = params.get_int(1).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<PlaybookRunFidelityResult>(RESULT_KEY)
                    .ok_or("No playbook fidelity result")?;
                let entry = result
                    .dangling_by_kind
                    .iter()
                    .find(|c| c.label == kind)
                    .ok_or_else(|| format!("No dangling entry for '{}'", kind))?;
                if entry.count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "dangling '{}': expected {}, got {}",
                        kind, expected, entry.count
                    ))
                }
            },
        ),
        check_def(
            "the playbook fidelity has {int} revision cycles total",
            &[(RESULT_KEY, "PlaybookRunFidelityResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<PlaybookRunFidelityResult>(RESULT_KEY)
                    .ok_or("No playbook fidelity result")?;
                if result.revision_cycles_total == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} revision cycles total, got {}",
                        expected, result.revision_cycles_total
                    ))
                }
            },
        ),
        check_def(
            "the playbook fidelity revision cycles for {string} has count {int}",
            &[(RESULT_KEY, "PlaybookRunFidelityResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let expected = params.get_int(1).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<PlaybookRunFidelityResult>(RESULT_KEY)
                    .ok_or("No playbook fidelity result")?;
                let entry = result
                    .revision_cycles
                    .iter()
                    .find(|c| c.label == kind)
                    .ok_or_else(|| format!("No revision cycles entry for '{}'", kind))?;
                if entry.count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "revision cycles '{}': expected {}, got {}",
                        kind, expected, entry.count
                    ))
                }
            },
        ),
        check_def(
            "the playbook fidelity has {int} review exits",
            &[(RESULT_KEY, "PlaybookRunFidelityResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<PlaybookRunFidelityResult>(RESULT_KEY)
                    .ok_or("No playbook fidelity result")?;
                if result.review.review_exits == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} review exits, got {}",
                        expected, result.review.review_exits
                    ))
                }
            },
        ),
        check_def(
            "the playbook fidelity has {int} delegated exits",
            &[(RESULT_KEY, "PlaybookRunFidelityResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<PlaybookRunFidelityResult>(RESULT_KEY)
                    .ok_or("No playbook fidelity result")?;
                if result.review.delegated_exits == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} delegated exits, got {}",
                        expected, result.review.delegated_exits
                    ))
                }
            },
        ),
        check_def(
            "the playbook fidelity has {int} self review exits",
            &[(RESULT_KEY, "PlaybookRunFidelityResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<PlaybookRunFidelityResult>(RESULT_KEY)
                    .ok_or("No playbook fidelity result")?;
                if result.review.self_review_exits == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} self review exits, got {}",
                        expected, result.review.self_review_exits
                    ))
                }
            },
        ),
        check_def(
            "the playbook fidelity has a review elapsed sample of {int} seconds",
            &[(RESULT_KEY, "PlaybookRunFidelityResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected seconds")? as u64;
                let result = ctx
                    .get::<PlaybookRunFidelityResult>(RESULT_KEY)
                    .ok_or("No playbook fidelity result")?;
                if result.review_elapsed_seconds.contains(&expected) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected a review elapsed sample of {} seconds, got {:?}",
                        expected, result.review_elapsed_seconds
                    ))
                }
            },
        ),
        check_def(
            "playbook fidelity instance {string} has kind {string} folded state {string} transition count {int} reached terminal {string} dangling {string} and revision cycles {int}",
            &[(RESULT_KEY, "PlaybookRunFidelityResult")],
            |ctx, params| {
                let instance_id = params.get_string(0).ok_or("Expected instance id")?;
                let expected_kind = params.get_string(1).ok_or("Expected kind")?;
                let expected_state = params.get_string(2).ok_or("Expected folded state")?;
                let expected_transition_count =
                    params.get_int(3).ok_or("Expected transition count")? as u64;
                let expected_terminal =
                    parse_bool(params.get_string(4).ok_or("Expected reached terminal")?)?;
                let expected_dangling =
                    parse_bool(params.get_string(5).ok_or("Expected dangling")?)?;
                let expected_revision_cycles =
                    params.get_int(6).ok_or("Expected revision cycles")? as u64;
                let result = ctx
                    .get::<PlaybookRunFidelityResult>(RESULT_KEY)
                    .ok_or("No playbook fidelity result")?;
                let row = result
                    .instances
                    .iter()
                    .find(|i| i.instance_id == instance_id)
                    .ok_or_else(|| format!("No playbook fidelity instance '{}'", instance_id))?;
                if row.kind != expected_kind {
                    return Err(format!(
                        "{} kind: expected '{}', got '{}'",
                        instance_id, expected_kind, row.kind
                    ));
                }
                if row.folded_state != expected_state {
                    return Err(format!(
                        "{} folded_state: expected '{}', got '{}'",
                        instance_id, expected_state, row.folded_state
                    ));
                }
                if row.transition_count != expected_transition_count {
                    return Err(format!(
                        "{} transition_count: expected {}, got {}",
                        instance_id, expected_transition_count, row.transition_count
                    ));
                }
                if row.reached_terminal != expected_terminal {
                    return Err(format!(
                        "{} reached_terminal: expected {}, got {}",
                        instance_id, expected_terminal, row.reached_terminal
                    ));
                }
                if row.dangling != expected_dangling {
                    return Err(format!(
                        "{} dangling: expected {}, got {}",
                        instance_id, expected_dangling, row.dangling
                    ));
                }
                if row.revision_cycles != expected_revision_cycles {
                    return Err(format!(
                        "{} revision_cycles: expected {}, got {}",
                        instance_id, expected_revision_cycles, row.revision_cycles
                    ));
                }
                if !row.begun {
                    return Err(format!("{} begun: expected true, got false", instance_id));
                }
                Ok(())
            },
        ),
        check_def(
            "playbook fidelity instance {string} has folded state {string}",
            &[(RESULT_KEY, "PlaybookRunFidelityResult")],
            |ctx, params| {
                let instance_id = params.get_string(0).ok_or("Expected instance id")?;
                let expected_state = params.get_string(1).ok_or("Expected folded state")?;
                let result = ctx
                    .get::<PlaybookRunFidelityResult>(RESULT_KEY)
                    .ok_or("No playbook fidelity result")?;
                let row = result
                    .instances
                    .iter()
                    .find(|i| i.instance_id == instance_id)
                    .ok_or_else(|| format!("No playbook fidelity instance '{}'", instance_id))?;
                if row.folded_state == expected_state {
                    Ok(())
                } else {
                    Err(format!(
                        "{} folded_state: expected '{}', got '{}'",
                        instance_id, expected_state, row.folded_state
                    ))
                }
            },
        ),
        check_def(
            "playbook fidelity instance rows are ordered by instance id",
            &[(RESULT_KEY, "PlaybookRunFidelityResult")],
            |ctx, _params| {
                let result = ctx
                    .get::<PlaybookRunFidelityResult>(RESULT_KEY)
                    .ok_or("No playbook fidelity result")?;
                let ids: Vec<&str> = result
                    .instances
                    .iter()
                    .map(|row| row.instance_id.as_str())
                    .collect();
                let mut sorted = ids.clone();
                sorted.sort_unstable();
                if ids == sorted {
                    Ok(())
                } else {
                    Err(format!("instance rows are not ordered: {:?}", ids))
                }
            },
        ),
    ]
}
