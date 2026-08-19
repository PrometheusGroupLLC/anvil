//! Step module for `survivor_outcome.feature` (core domain seam).
//!
//! Exercises the pure `fold_survivor_outcome` fold over in-memory
//! `ActivityLogRecord` + `PlaybookMeasurementRecord` streams, using
//! `SeedPlaybookRegistry` for the per-kind terminal predicate (the track seed
//! makes `superseded` terminal and `implementing`/`completed` non-terminal). Each
//! scenario seeds an activity stream and a measurement stream (either may be
//! empty), then folds and asserts on the per-kind survivor-corrected rows.
//!
//! Because a brine map step retains only its declared `provides` keys, the
//! measurement-seeding steps re-emit the activity key so both streams reach the
//! fold step.

use anvil_core::domain::playbook::registry::SeedPlaybookRegistry;
use anvil_core::domain::survivor_outcome::{fold_survivor_outcome, SurvivorOutcomeResult};
use anvil_core::ports::activity_log_port::ActivityLogRecord;
use anvil_core::ports::playbook_measurement_port::{
    QualitySignal, PlaybookMeasurementRecord, PLAYBOOK_MEASUREMENT_KIND,
};
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, Params, StepDef};

const ACTIVITY_KEY: &str = "sv_activity";
const MEASUREMENTS_KEY: &str = "sv_measurements";
const RESULT_KEY: &str = "sv_result";

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

fn parse_activity(table: &DataTable) -> Result<Vec<ActivityLogRecord>, String> {
    let command_col = opt_column_index(table, "command");
    let from_col = opt_column_index(table, "from_state");
    let to_col = opt_column_index(table, "to_state");
    let at_col = opt_column_index(table, "at");
    let wfk_col = opt_column_index(table, "artifact_kind");
    let instance_col = opt_column_index(table, "playbook_run_id");
    let mut records = Vec::new();
    for row in &table.rows {
        records.push(ActivityLogRecord {
            command: command_col
                .map(|i| row[i].trim().to_string())
                .unwrap_or_default(),
            outcome: String::new(),
            artifact_kind: wfk_col
                .map(|i| row[i].trim().to_string())
                .unwrap_or_default(),
            from_state: from_col
                .map(|i| row[i].trim().to_string())
                .unwrap_or_default(),
            to_state: to_col
                .map(|i| row[i].trim().to_string())
                .unwrap_or_default(),
            actor_hash: None,
            at: at_col
                .map(|i| row[i].trim().to_string())
                .unwrap_or_default(),
            source: String::new(),
            conversation_hash: None,
            project_label: None,
            playbook_run_id: opt_string(row, instance_col),
            call_state: None,
        });
    }
    Ok(records)
}

fn parse_measurements(table: &DataTable) -> Result<Vec<PlaybookMeasurementRecord>, String> {
    let instance_col = column_index(table, "playbook_run_id")?;
    let success_col = column_index(table, "success")?;
    let wfk_col = opt_column_index(table, "artifact_kind");
    let mut records = Vec::new();
    for row in &table.rows {
        let success = match row[success_col].trim() {
            "true" => true,
            "false" => false,
            other => {
                return Err(format!(
                    "Expected 'true'/'false' for success, got '{}'",
                    other
                ))
            }
        };
        records.push(PlaybookMeasurementRecord {
            kind: PLAYBOOK_MEASUREMENT_KIND.to_string(),
            artifact_kind: wfk_col
                .map(|i| row[i].trim().to_string())
                .unwrap_or_default(),
            playbook_run_id: Some(row[instance_col].trim().to_string()),
            terminal_state: String::new(),
            terminal_reached: true,
            outcome: "terminal_reached".to_string(),
            success,
            quality_dimension_scores: Vec::new(),
            quality_overall: None,
            quality_grader: None,
            quality_signal: QualitySignal::Leading,
            at: String::new(),
            conversation_hash: None,
            project_label: None,
            playbook_version: None,
        });
    }
    Ok(records)
}

fn assert_outcome_field(ctx: &Context, params: &Params, field: &str) -> Result<(), String> {
    let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
    let expected = params.get_int(1).ok_or("Expected count")? as u64;
    let result = ctx
        .get::<SurvivorOutcomeResult>(RESULT_KEY)
        .ok_or("No survivor outcome result")?;
    let row = result
        .outcome(&kind)
        .ok_or_else(|| format!("No survivor outcome row for '{}'", kind))?;
    let actual = match field {
        "begun" => row.begun,
        "terminal" => row.terminal,
        "outcome_satisfied" => row.outcome_satisfied,
        "dangling" => row.dangling(),
        other => return Err(format!("Unknown field '{}'", other)),
    };
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "survivor outcome '{}' {}: expected {}, got {}",
            kind, field, expected, actual
        ))
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a survivor activity stream:",
            &[],
            &[(ACTIVITY_KEY, "Vec<ActivityLogRecord>")],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let mut out = Context::new();
                out.set(ACTIVITY_KEY, parse_activity(table)?);
                Ok(out)
            },
        ),
        step_def(
            "an empty survivor activity stream",
            &[],
            &[(ACTIVITY_KEY, "Vec<ActivityLogRecord>")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set(ACTIVITY_KEY, Vec::<ActivityLogRecord>::new());
                Ok(out)
            },
        ),
        step_def(
            "a survivor measurement stream:",
            &[(ACTIVITY_KEY, "Vec<ActivityLogRecord>")],
            &[
                (ACTIVITY_KEY, "Vec<ActivityLogRecord>"),
                (MEASUREMENTS_KEY, "Vec<PlaybookMeasurementRecord>"),
            ],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let activity = ctx
                    .get::<Vec<ActivityLogRecord>>(ACTIVITY_KEY)
                    .cloned()
                    .unwrap_or_default();
                let mut out = Context::new();
                out.set(ACTIVITY_KEY, activity);
                out.set(MEASUREMENTS_KEY, parse_measurements(table)?);
                Ok(out)
            },
        ),
        step_def(
            "an empty survivor measurement stream",
            &[(ACTIVITY_KEY, "Vec<ActivityLogRecord>")],
            &[
                (ACTIVITY_KEY, "Vec<ActivityLogRecord>"),
                (MEASUREMENTS_KEY, "Vec<PlaybookMeasurementRecord>"),
            ],
            |ctx, _params| {
                let activity = ctx
                    .get::<Vec<ActivityLogRecord>>(ACTIVITY_KEY)
                    .cloned()
                    .unwrap_or_default();
                let mut out = Context::new();
                out.set(ACTIVITY_KEY, activity);
                out.set(MEASUREMENTS_KEY, Vec::<PlaybookMeasurementRecord>::new());
                Ok(out)
            },
        ),
        step_def(
            "the survivor outcome is folded",
            &[
                (ACTIVITY_KEY, "Vec<ActivityLogRecord>"),
                (MEASUREMENTS_KEY, "Vec<PlaybookMeasurementRecord>"),
            ],
            &[(RESULT_KEY, "SurvivorOutcomeResult")],
            |ctx, _params| {
                let activity = ctx
                    .get::<Vec<ActivityLogRecord>>(ACTIVITY_KEY)
                    .cloned()
                    .unwrap_or_default();
                let measurements = ctx
                    .get::<Vec<PlaybookMeasurementRecord>>(MEASUREMENTS_KEY)
                    .cloned()
                    .unwrap_or_default();
                let result = fold_survivor_outcome(&activity, &measurements, &SeedPlaybookRegistry);
                let mut out = Context::new();
                out.set(RESULT_KEY, result);
                Ok(out)
            },
        ),
        check_def(
            "the survivor outcome for {string} has begun {int}",
            &[(RESULT_KEY, "SurvivorOutcomeResult")],
            |ctx, params| assert_outcome_field(&ctx, params, "begun"),
        ),
        check_def(
            "the survivor outcome for {string} has terminal {int}",
            &[(RESULT_KEY, "SurvivorOutcomeResult")],
            |ctx, params| assert_outcome_field(&ctx, params, "terminal"),
        ),
        check_def(
            "the survivor outcome for {string} has outcome_satisfied {int}",
            &[(RESULT_KEY, "SurvivorOutcomeResult")],
            |ctx, params| assert_outcome_field(&ctx, params, "outcome_satisfied"),
        ),
        check_def(
            "the survivor outcome for {string} has {int} dangling",
            &[(RESULT_KEY, "SurvivorOutcomeResult")],
            |ctx, params| assert_outcome_field(&ctx, params, "dangling"),
        ),
        check_def(
            "the survivor outcome for {string} has outcome rate permille {int}",
            &[(RESULT_KEY, "SurvivorOutcomeResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let expected = params.get_int(1).ok_or("Expected permille")? as u64;
                let result = ctx
                    .get::<SurvivorOutcomeResult>(RESULT_KEY)
                    .ok_or("No survivor outcome result")?;
                let row = result
                    .outcome(&kind)
                    .ok_or_else(|| format!("No survivor outcome row for '{}'", kind))?;
                let actual = (row.outcome_rate * 1000.0).round() as u64;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "survivor outcome '{}' rate permille: expected {}, got {} (rate {})",
                        kind, expected, actual, row.outcome_rate
                    ))
                }
            },
        ),
        check_def(
            "the survivor outcome has {int} rows",
            &[(RESULT_KEY, "SurvivorOutcomeResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let result = ctx
                    .get::<SurvivorOutcomeResult>(RESULT_KEY)
                    .ok_or("No survivor outcome result")?;
                if result.outcomes.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} rows, got {}",
                        expected,
                        result.outcomes.len()
                    ))
                }
            },
        ),
    ]
}
