//! Step module for `activity_summary.feature` (core domain seam).
//!
//! Exercises the pure `fold_activity_summary` + `fold_activity_summary_across_hearths`
//! folds over in-memory `ActivityLogRecord` streams. No filesystem: these are
//! pure domain folds, so the steps seed record vectors directly and assert
//! against the folded result.

use anvil_core::domain::activity_summary::{
    fold_activity_summary, fold_activity_summary_across_hearths, ActivitySummaryResult, LabelCount,
};
use anvil_core::domain::usage_timeseries::Granularity;
use anvil_core::ports::activity_log_port::ActivityLogRecord;
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

const RECORDS_KEY: &str = "as_records";
const RECORDS_A_KEY: &str = "as_records_a";
const RECORDS_B_KEY: &str = "as_records_b";
const RESULT_KEY: &str = "as_result";

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

fn actor_hash_opt(row: &[String], idx: Option<usize>) -> Option<String> {
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
    let outcome_col = column_index(table, "outcome")?;
    let wfk_col = opt_column_index(table, "artifact_kind");
    let from_col = opt_column_index(table, "from_state");
    let to_col = opt_column_index(table, "to_state");
    let hash_col = opt_column_index(table, "actor_hash");
    let source_col = opt_column_index(table, "source");
    let conv_col = opt_column_index(table, "conversation_hash");
    let call_state_col = opt_column_index(table, "call_state");
    let at_col = column_index(table, "at")?;
    let mut records = Vec::new();
    for row in &table.rows {
        records.push(ActivityLogRecord {
            command: row[command_col].trim().to_string(),
            outcome: row[outcome_col].trim().to_string(),
            artifact_kind: wfk_col
                .map(|i| row[i].trim().to_string())
                .unwrap_or_default(),
            from_state: from_col
                .map(|i| row[i].trim().to_string())
                .unwrap_or_default(),
            to_state: to_col
                .map(|i| row[i].trim().to_string())
                .unwrap_or_default(),
            actor_hash: actor_hash_opt(row, hash_col),
            at: row[at_col].trim().to_string(),
            source: source_col
                .map(|i| row[i].trim().to_string())
                .unwrap_or_default(),
            conversation_hash: actor_hash_opt(row, conv_col),
            project_label: None,
            playbook_run_id: None,
            call_state: call_state_col.and_then(|i| {
                let v = row[i].trim();
                if v.is_empty() || v == "-" {
                    None
                } else {
                    Some(v.to_string())
                }
            }),
        });
    }
    Ok(records)
}

fn find_count<'a>(list: &'a [LabelCount], label: &str) -> Option<&'a LabelCount> {
    list.iter().find(|c| c.label == label)
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "an activity summary record stream:",
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
            "an empty activity summary record stream",
            &[],
            &[(RECORDS_KEY, "Vec<ActivityLogRecord>")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set(RECORDS_KEY, Vec::<ActivityLogRecord>::new());
                Ok(out)
            },
        ),
        step_def(
            "activity summary hearth A stream:",
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
            "activity summary hearth B stream:",
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
            "the activity summary is folded by day",
            &[(RECORDS_KEY, "Vec<ActivityLogRecord>")],
            &[(RESULT_KEY, "ActivitySummaryResult")],
            |ctx, _params| {
                let records = ctx
                    .get::<Vec<ActivityLogRecord>>(RECORDS_KEY)
                    .cloned()
                    .unwrap_or_default();
                let result = fold_activity_summary(&records, Granularity::Day);
                let mut out = Context::new();
                out.set(RESULT_KEY, result);
                Ok(out)
            },
        ),
        step_def(
            "the activity summary is folded by week",
            &[(RECORDS_KEY, "Vec<ActivityLogRecord>")],
            &[(RESULT_KEY, "ActivitySummaryResult")],
            |ctx, _params| {
                let records = ctx
                    .get::<Vec<ActivityLogRecord>>(RECORDS_KEY)
                    .cloned()
                    .unwrap_or_default();
                let result = fold_activity_summary(&records, Granularity::Week);
                let mut out = Context::new();
                out.set(RESULT_KEY, result);
                Ok(out)
            },
        ),
        step_def(
            "the activity summary is folded across both hearths by day",
            &[
                (RECORDS_A_KEY, "Vec<ActivityLogRecord>"),
                (RECORDS_B_KEY, "Vec<ActivityLogRecord>"),
            ],
            &[(RESULT_KEY, "ActivitySummaryResult")],
            |ctx, _params| {
                let a = ctx
                    .get::<Vec<ActivityLogRecord>>(RECORDS_A_KEY)
                    .cloned()
                    .unwrap_or_default();
                let b = ctx
                    .get::<Vec<ActivityLogRecord>>(RECORDS_B_KEY)
                    .cloned()
                    .unwrap_or_default();
                let result = fold_activity_summary_across_hearths(&[a, b], Granularity::Day);
                let mut out = Context::new();
                out.set(RESULT_KEY, result);
                Ok(out)
            },
        ),
        check_def(
            "the activity summary has total turns {int}",
            &[(RESULT_KEY, "ActivitySummaryResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<ActivitySummaryResult>(RESULT_KEY)
                    .ok_or("No activity summary result")?;
                if result.total_turns == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected total turns {}, got {}",
                        expected, result.total_turns
                    ))
                }
            },
        ),
        check_def(
            "the activity summary has {int} routed conversations",
            &[(RESULT_KEY, "ActivitySummaryResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<ActivitySummaryResult>(RESULT_KEY)
                    .ok_or("No activity summary result")?;
                if result.routed_conversations == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected routed conversations {}, got {}",
                        expected, result.routed_conversations
                    ))
                }
            },
        ),
        check_def(
            "the activity summary has {int} converted conversations",
            &[(RESULT_KEY, "ActivitySummaryResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<ActivitySummaryResult>(RESULT_KEY)
                    .ok_or("No activity summary result")?;
                if result.converted_conversations == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected converted conversations {}, got {}",
                        expected, result.converted_conversations
                    ))
                }
            },
        ),
        check_def(
            "the activity summary has {int} buckets",
            &[(RESULT_KEY, "ActivitySummaryResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let result = ctx
                    .get::<ActivitySummaryResult>(RESULT_KEY)
                    .ok_or("No activity summary result")?;
                if result.buckets.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} buckets, got {}",
                        expected,
                        result.buckets.len()
                    ))
                }
            },
        ),
        check_def(
            "the activity summary by_command {string} has count {int}",
            &[(RESULT_KEY, "ActivitySummaryResult")],
            |ctx, params| {
                let label = params.get_string(0).ok_or("Expected label")?.to_string();
                let expected = params.get_int(1).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<ActivitySummaryResult>(RESULT_KEY)
                    .ok_or("No activity summary result")?;
                let entry = find_count(&result.by_command, &label)
                    .ok_or_else(|| format!("No by_command entry for '{}'", label))?;
                if entry.count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "by_command '{}': expected {}, got {}",
                        label, expected, entry.count
                    ))
                }
            },
        ),
        check_def(
            "the activity summary by_command has {int} entries",
            &[(RESULT_KEY, "ActivitySummaryResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let result = ctx
                    .get::<ActivitySummaryResult>(RESULT_KEY)
                    .ok_or("No activity summary result")?;
                if result.by_command.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} by_command entries, got {}",
                        expected,
                        result.by_command.len()
                    ))
                }
            },
        ),
        check_def(
            "the activity summary by_command is ordered descending by count",
            &[(RESULT_KEY, "ActivitySummaryResult")],
            |ctx, _params| {
                let result = ctx
                    .get::<ActivitySummaryResult>(RESULT_KEY)
                    .ok_or("No activity summary result")?;
                let counts: Vec<u64> = result.by_command.iter().map(|c| c.count).collect();
                if counts.windows(2).all(|w| w[0] >= w[1]) {
                    Ok(())
                } else {
                    Err(format!("by_command not descending by count: {:?}", counts))
                }
            },
        ),
        check_def(
            "the activity summary by_route_outcome {string} has count {int}",
            &[(RESULT_KEY, "ActivitySummaryResult")],
            |ctx, params| {
                let label = params.get_string(0).ok_or("Expected label")?.to_string();
                let expected = params.get_int(1).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<ActivitySummaryResult>(RESULT_KEY)
                    .ok_or("No activity summary result")?;
                let entry = find_count(&result.by_route_outcome, &label)
                    .ok_or_else(|| format!("No by_route_outcome entry for '{}'", label))?;
                if entry.count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "by_route_outcome '{}': expected {}, got {}",
                        label, expected, entry.count
                    ))
                }
            },
        ),
        check_def(
            "the activity summary by_route_outcome has {int} entries",
            &[(RESULT_KEY, "ActivitySummaryResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let result = ctx
                    .get::<ActivitySummaryResult>(RESULT_KEY)
                    .ok_or("No activity summary result")?;
                if result.by_route_outcome.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} by_route_outcome entries, got {}",
                        expected,
                        result.by_route_outcome.len()
                    ))
                }
            },
        ),
        check_def(
            "the activity summary by_source {string} has count {int}",
            &[(RESULT_KEY, "ActivitySummaryResult")],
            |ctx, params| {
                let label = params.get_string(0).ok_or("Expected label")?.to_string();
                let expected = params.get_int(1).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<ActivitySummaryResult>(RESULT_KEY)
                    .ok_or("No activity summary result")?;
                let entry = find_count(&result.by_source, &label)
                    .ok_or_else(|| format!("No by_source entry for '{}'", label))?;
                if entry.count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "by_source '{}': expected {}, got {}",
                        label, expected, entry.count
                    ))
                }
            },
        ),
        check_def(
            "the activity summary by_source has {int} entries",
            &[(RESULT_KEY, "ActivitySummaryResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let result = ctx
                    .get::<ActivitySummaryResult>(RESULT_KEY)
                    .ok_or("No activity summary result")?;
                if result.by_source.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} by_source entries, got {}",
                        expected,
                        result.by_source.len()
                    ))
                }
            },
        ),
        check_def(
            "the activity summary by_artifact_kind {string} has count {int}",
            &[(RESULT_KEY, "ActivitySummaryResult")],
            |ctx, params| {
                let label = params.get_string(0).ok_or("Expected label")?.to_string();
                let expected = params.get_int(1).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<ActivitySummaryResult>(RESULT_KEY)
                    .ok_or("No activity summary result")?;
                let entry = find_count(&result.by_artifact_kind, &label)
                    .ok_or_else(|| format!("No by_artifact_kind entry for '{}'", label))?;
                if entry.count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "by_artifact_kind '{}': expected {}, got {}",
                        label, expected, entry.count
                    ))
                }
            },
        ),
        check_def(
            "the activity summary by_artifact_kind has {int} entries",
            &[(RESULT_KEY, "ActivitySummaryResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let result = ctx
                    .get::<ActivitySummaryResult>(RESULT_KEY)
                    .ok_or("No activity summary result")?;
                if result.by_artifact_kind.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} by_artifact_kind entries, got {}",
                        expected,
                        result.by_artifact_kind.len()
                    ))
                }
            },
        ),
        check_def(
            "the activity summary bucket {string} has total turns {int}",
            &[(RESULT_KEY, "ActivitySummaryResult")],
            |ctx, params| {
                let period = params.get_string(0).ok_or("Expected period")?.to_string();
                let expected = params.get_int(1).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<ActivitySummaryResult>(RESULT_KEY)
                    .ok_or("No activity summary result")?;
                let bucket = result
                    .buckets
                    .iter()
                    .find(|b| b.period_start == period)
                    .ok_or_else(|| format!("No bucket for period '{}'", period))?;
                if bucket.total_turns == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "bucket '{}': expected total turns {}, got {}",
                        period, expected, bucket.total_turns
                    ))
                }
            },
        ),
        check_def(
            "the activity summary bucket {string} has distinct actors {int}",
            &[(RESULT_KEY, "ActivitySummaryResult")],
            |ctx, params| {
                let period = params.get_string(0).ok_or("Expected period")?.to_string();
                let expected = params.get_int(1).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<ActivitySummaryResult>(RESULT_KEY)
                    .ok_or("No activity summary result")?;
                let bucket = result
                    .buckets
                    .iter()
                    .find(|b| b.period_start == period)
                    .ok_or_else(|| format!("No bucket for period '{}'", period))?;
                if bucket.distinct_actors == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "bucket '{}': expected distinct actors {}, got {}",
                        period, expected, bucket.distinct_actors
                    ))
                }
            },
        ),
        check_def(
            "the activity summary by_call_state {string} has count {int}",
            &[(RESULT_KEY, "ActivitySummaryResult")],
            |ctx, params| {
                let label = params.get_string(0).ok_or("Expected label")?.to_string();
                let expected = params.get_int(1).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<ActivitySummaryResult>(RESULT_KEY)
                    .ok_or("No activity summary result")?;
                let entry = find_count(&result.by_call_state, &label)
                    .ok_or_else(|| format!("No by_call_state entry for '{}'", label))?;
                if entry.count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "by_call_state '{}': expected {}, got {}",
                        label, expected, entry.count
                    ))
                }
            },
        ),
        check_def(
            "the activity summary by_call_state has {int} entries",
            &[(RESULT_KEY, "ActivitySummaryResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let result = ctx
                    .get::<ActivitySummaryResult>(RESULT_KEY)
                    .ok_or("No activity summary result")?;
                if result.by_call_state.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} by_call_state entries, got {}",
                        expected,
                        result.by_call_state.len()
                    ))
                }
            },
        ),
        check_def(
            "the activity summary has {int} playbook step turns",
            &[(RESULT_KEY, "ActivitySummaryResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<ActivitySummaryResult>(RESULT_KEY)
                    .ok_or("No activity summary result")?;
                if result.playbook_step_turns == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected playbook step turns {}, got {}",
                        expected, result.playbook_step_turns
                    ))
                }
            },
        ),
        check_def(
            "the activity summary buckets are ordered ascending by period start",
            &[(RESULT_KEY, "ActivitySummaryResult")],
            |ctx, _params| {
                let result = ctx
                    .get::<ActivitySummaryResult>(RESULT_KEY)
                    .ok_or("No activity summary result")?;
                let starts: Vec<String> = result
                    .buckets
                    .iter()
                    .map(|b| b.period_start.clone())
                    .collect();
                let mut sorted = starts.clone();
                sorted.sort();
                if starts == sorted {
                    Ok(())
                } else {
                    Err(format!(
                        "Buckets not ascending by period start: {:?}",
                        starts
                    ))
                }
            },
        ),
    ]
}
