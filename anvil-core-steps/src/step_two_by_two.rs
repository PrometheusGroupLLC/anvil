//! Step module for `step_two_by_two.feature` (core domain seam).
//!
//! Exercises the pure `fold_step_two_by_two` fold over in-memory
//! `ActivityLogRecord` + `ReviewVerdictRecord` streams. No filesystem. Each
//! scenario seeds an activity stream and a verdict stream (either may be empty),
//! then folds and asserts on the resulting per-(kind, step_kind) cells.
//!
//! Because a brine map step retains only its declared `provides` keys, the
//! verdict-seeding steps re-emit the activity key (the hearth-A/B pattern the
//! fidelity module uses) so both streams reach the fold step.

use anvil_core::domain::step_two_by_two::{fold_step_two_by_two, StepTwoByTwoResult};
use anvil_core::ports::activity_log_port::ActivityLogRecord;
use anvil_core::ports::review_verdict_port::{
    ReviewVerdictRecord, VerdictFinding, REVIEW_VERDICT_KIND,
};
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

const ACTIVITY_KEY: &str = "s2_activity";
const VERDICTS_KEY: &str = "s2_verdicts";
const RESULT_KEY: &str = "s2_result";

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

/// One data-table row → one review-verdict record carrying a single finding.
fn parse_verdicts(table: &DataTable) -> Result<Vec<ReviewVerdictRecord>, String> {
    let wfk_col = column_index(table, "artifact_kind")?;
    let gate_col = column_index(table, "gate_state")?;
    let dim_col = opt_column_index(table, "dimension");
    let sev_col = opt_column_index(table, "severity");
    let origin_col = opt_column_index(table, "origin_phase");
    let mut records = Vec::new();
    for row in &table.rows {
        let finding = VerdictFinding {
            dimension: dim_col
                .map(|i| row[i].trim().to_string())
                .unwrap_or_default(),
            severity: sev_col
                .map(|i| row[i].trim().to_string())
                .unwrap_or_default(),
            origin_phase: opt_string(row, origin_col),
        };
        records.push(ReviewVerdictRecord {
            kind: REVIEW_VERDICT_KIND.to_string(),
            artifact_kind: row[wfk_col].trim().to_string(),
            playbook_run_id: None,
            gate_state: row[gate_col].trim().to_string(),
            satisfaction: String::new(),
            is_final_gate: false,
            findings: vec![finding],
            intent_confidence: None,
            outcome: "ok".to_string(),
            at: String::new(),
            conversation_hash: None,
            project_label: None,
            playbook_version: None,
        });
    }
    Ok(records)
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a step 2x2 activity stream:",
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
            "an empty step 2x2 activity stream",
            &[],
            &[(ACTIVITY_KEY, "Vec<ActivityLogRecord>")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set(ACTIVITY_KEY, Vec::<ActivityLogRecord>::new());
                Ok(out)
            },
        ),
        step_def(
            "a step 2x2 verdict stream:",
            &[(ACTIVITY_KEY, "Vec<ActivityLogRecord>")],
            &[
                (ACTIVITY_KEY, "Vec<ActivityLogRecord>"),
                (VERDICTS_KEY, "Vec<ReviewVerdictRecord>"),
            ],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let activity = ctx
                    .get::<Vec<ActivityLogRecord>>(ACTIVITY_KEY)
                    .cloned()
                    .unwrap_or_default();
                let mut out = Context::new();
                out.set(ACTIVITY_KEY, activity);
                out.set(VERDICTS_KEY, parse_verdicts(table)?);
                Ok(out)
            },
        ),
        step_def(
            "an empty step 2x2 verdict stream",
            &[(ACTIVITY_KEY, "Vec<ActivityLogRecord>")],
            &[
                (ACTIVITY_KEY, "Vec<ActivityLogRecord>"),
                (VERDICTS_KEY, "Vec<ReviewVerdictRecord>"),
            ],
            |ctx, _params| {
                let activity = ctx
                    .get::<Vec<ActivityLogRecord>>(ACTIVITY_KEY)
                    .cloned()
                    .unwrap_or_default();
                let mut out = Context::new();
                out.set(ACTIVITY_KEY, activity);
                out.set(VERDICTS_KEY, Vec::<ReviewVerdictRecord>::new());
                Ok(out)
            },
        ),
        step_def(
            "the step 2x2 is folded",
            &[
                (ACTIVITY_KEY, "Vec<ActivityLogRecord>"),
                (VERDICTS_KEY, "Vec<ReviewVerdictRecord>"),
            ],
            &[(RESULT_KEY, "StepTwoByTwoResult")],
            |ctx, _params| {
                let activity = ctx
                    .get::<Vec<ActivityLogRecord>>(ACTIVITY_KEY)
                    .cloned()
                    .unwrap_or_default();
                let verdicts = ctx
                    .get::<Vec<ReviewVerdictRecord>>(VERDICTS_KEY)
                    .cloned()
                    .unwrap_or_default();
                let result = fold_step_two_by_two(&activity, &verdicts);
                let mut out = Context::new();
                out.set(RESULT_KEY, result);
                Ok(out)
            },
        ),
        check_def(
            "the step 2x2 cell for {string} step {string} has gate observations {int}",
            &[(RESULT_KEY, "StepTwoByTwoResult")],
            |ctx, params| assert_cell_field(&ctx, params, "gate_observations"),
        ),
        check_def(
            "the step 2x2 cell for {string} step {string} has one-shot passes {int}",
            &[(RESULT_KEY, "StepTwoByTwoResult")],
            |ctx, params| assert_cell_field(&ctx, params, "one_shot_passes"),
        ),
        check_def(
            "the step 2x2 cell for {string} step {string} has escape observations {int}",
            &[(RESULT_KEY, "StepTwoByTwoResult")],
            |ctx, params| assert_cell_field(&ctx, params, "escape_observations"),
        ),
        check_def(
            "the step 2x2 cell for {string} step {string} has defect escapes {int}",
            &[(RESULT_KEY, "StepTwoByTwoResult")],
            |ctx, params| assert_cell_field(&ctx, params, "defect_escapes"),
        ),
        check_def(
            "the step 2x2 has no cell for {string} step {string}",
            &[(RESULT_KEY, "StepTwoByTwoResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let step = params.get_string(1).ok_or("Expected step")?.to_string();
                let result = ctx
                    .get::<StepTwoByTwoResult>(RESULT_KEY)
                    .ok_or("No step 2x2 result")?;
                if result.cell(&kind, &step).is_some() {
                    Err(format!(
                        "Expected no cell for ({}, {}) but found one",
                        kind, step
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the step 2x2 has {int} cells",
            &[(RESULT_KEY, "StepTwoByTwoResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let result = ctx
                    .get::<StepTwoByTwoResult>(RESULT_KEY)
                    .ok_or("No step 2x2 result")?;
                if result.cells.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} cells, got {}",
                        expected,
                        result.cells.len()
                    ))
                }
            },
        ),
    ]
}

fn assert_cell_field(
    ctx: &brine_runner_rust::context::Context,
    params: &brine_runner_rust::registry::Params,
    field: &str,
) -> Result<(), String> {
    let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
    let step = params.get_string(1).ok_or("Expected step")?.to_string();
    let expected = params.get_int(2).ok_or("Expected count")? as u64;
    let result = ctx
        .get::<StepTwoByTwoResult>(RESULT_KEY)
        .ok_or("No step 2x2 result")?;
    let cell = result
        .cell(&kind, &step)
        .ok_or_else(|| format!("No cell for ({}, {})", kind, step))?;
    let actual = match field {
        "gate_observations" => cell.gate_observations,
        "one_shot_passes" => cell.one_shot_passes,
        "escape_observations" => cell.escape_observations,
        "defect_escapes" => cell.defect_escapes,
        other => return Err(format!("Unknown field '{}'", other)),
    };
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "cell ({}, {}) {}: expected {}, got {}",
            kind, step, field, expected, actual
        ))
    }
}
