//! Step module for `outcome_predicate_fold.feature` (core domain seam).
//!
//! Exercises the pure `fold_outcome_predicate` fold over an in-memory
//! `ActivityLogRecord` stream, using either `SeedPlaybookRegistry` (whose
//! compiled-in seeds all carry `outcome_predicate: None` — the "ungradeable,
//! not a failure" path) or a scenario-declared in-memory registry double
//! (kind -> `PlaybookMachine` carrying a declared `outcome_predicate`, for the
//! "declared and evaluated" path).
//!
//! Because a brine map step retains only its declared `provides` keys, the
//! registry-seeding step re-emits the activity key so both reach the fold
//! step (same pattern as `survivor_outcome.rs`).

use anvil_core::domain::outcome_predicate_fold::{fold_outcome_predicate, OutcomePredicateResult};
use anvil_core::domain::playbook::registry::{PlaybookRegistry, SeedPlaybookRegistry};
use anvil_core::domain::playbook::types::{OutcomePredicate, PlaybookMachine};
use anvil_core::ports::activity_log_port::ActivityLogRecord;
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, Params, StepDef};
use std::collections::HashMap;

const ACTIVITY_KEY: &str = "opf_activity";
const MACHINES_KEY: &str = "opf_machines";
const RESULT_KEY: &str = "opf_result";

/// In-memory registry double built from a scenario's `outcome-predicate
/// registry:` table.
struct VecRegistry {
    map: HashMap<String, PlaybookMachine>,
}

impl PlaybookRegistry for VecRegistry {
    fn machine_for<'a>(&'a self, kind: &str) -> Option<&'a PlaybookMachine>
    where
        Self: 'a,
    {
        self.map.get(kind)
    }

    fn playbook_id_for(&self, kind: &str) -> Option<String> {
        self.map.get(kind).map(|_| format!("{}_dir", kind))
    }
}

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

fn parse_registry_machines(table: &DataTable) -> Result<Vec<PlaybookMachine>, String> {
    let kind_col = column_index(table, "kind")?;
    let predicate_col = opt_column_index(table, "predicate_terminal_state");
    let mut machines = Vec::new();
    for row in &table.rows {
        let kind = row[kind_col].trim().to_string();
        let outcome_predicate = opt_string(row, predicate_col).map(|terminal_state| {
            OutcomePredicate {
                terminal_state,
                check: None,
            }
        });
        machines.push(PlaybookMachine {
            kind,
            outcome_predicate,
            ..Default::default()
        });
    }
    Ok(machines)
}

fn assert_outcome_field(ctx: &Context, params: &Params, field: &str) -> Result<(), String> {
    let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
    let expected = params.get_int(1).ok_or("Expected count")? as u64;
    let result = ctx
        .get::<OutcomePredicateResult>(RESULT_KEY)
        .ok_or("No outcome predicate result")?;
    let row = result
        .outcome(&kind)
        .ok_or_else(|| format!("No outcome predicate row for '{}'", kind))?;
    let actual = match field {
        "begun" => row.begun,
        "predicate_satisfied" => row.predicate_satisfied,
        other => return Err(format!("Unknown field '{}'", other)),
    };
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "outcome predicate outcome '{}' {}: expected {}, got {}",
            kind, field, expected, actual
        ))
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "an outcome-predicate activity stream:",
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
            "an empty outcome-predicate activity stream",
            &[],
            &[(ACTIVITY_KEY, "Vec<ActivityLogRecord>")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set(ACTIVITY_KEY, Vec::<ActivityLogRecord>::new());
                Ok(out)
            },
        ),
        step_def(
            "an outcome-predicate registry:",
            &[(ACTIVITY_KEY, "Vec<ActivityLogRecord>")],
            &[
                (ACTIVITY_KEY, "Vec<ActivityLogRecord>"),
                (MACHINES_KEY, "Vec<PlaybookMachine>"),
            ],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let activity = ctx
                    .get::<Vec<ActivityLogRecord>>(ACTIVITY_KEY)
                    .cloned()
                    .unwrap_or_default();
                let mut out = Context::new();
                out.set(ACTIVITY_KEY, activity);
                out.set(MACHINES_KEY, parse_registry_machines(table)?);
                Ok(out)
            },
        ),
        step_def(
            "the outcome predicate is folded against the seed registry",
            &[(ACTIVITY_KEY, "Vec<ActivityLogRecord>")],
            &[(RESULT_KEY, "OutcomePredicateResult")],
            |ctx, _params| {
                let activity = ctx
                    .get::<Vec<ActivityLogRecord>>(ACTIVITY_KEY)
                    .cloned()
                    .unwrap_or_default();
                let result = fold_outcome_predicate(&activity, &SeedPlaybookRegistry);
                let mut out = Context::new();
                out.set(RESULT_KEY, result);
                Ok(out)
            },
        ),
        step_def(
            "the outcome predicate is folded against the outcome-predicate registry",
            &[
                (ACTIVITY_KEY, "Vec<ActivityLogRecord>"),
                (MACHINES_KEY, "Vec<PlaybookMachine>"),
            ],
            &[(RESULT_KEY, "OutcomePredicateResult")],
            |ctx, _params| {
                let activity = ctx
                    .get::<Vec<ActivityLogRecord>>(ACTIVITY_KEY)
                    .cloned()
                    .unwrap_or_default();
                let machines = ctx
                    .get::<Vec<PlaybookMachine>>(MACHINES_KEY)
                    .cloned()
                    .unwrap_or_default();
                let map: HashMap<String, PlaybookMachine> = machines
                    .into_iter()
                    .map(|machine| (machine.kind.clone(), machine))
                    .collect();
                let registry = VecRegistry { map };
                let result = fold_outcome_predicate(&activity, &registry);
                let mut out = Context::new();
                out.set(RESULT_KEY, result);
                Ok(out)
            },
        ),
        check_def(
            "the outcome predicate outcome for {string} has begun {int}",
            &[(RESULT_KEY, "OutcomePredicateResult")],
            |ctx, params| assert_outcome_field(&ctx, params, "begun"),
        ),
        check_def(
            "the outcome predicate outcome for {string} has predicate_satisfied {int}",
            &[(RESULT_KEY, "OutcomePredicateResult")],
            |ctx, params| assert_outcome_field(&ctx, params, "predicate_satisfied"),
        ),
        check_def(
            "the outcome predicate outcome for {string} has a declared predicate",
            &[(RESULT_KEY, "OutcomePredicateResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let result = ctx
                    .get::<OutcomePredicateResult>(RESULT_KEY)
                    .ok_or("No outcome predicate result")?;
                let row = result
                    .outcome(&kind)
                    .ok_or_else(|| format!("No outcome predicate row for '{}'", kind))?;
                if row.predicate_declared {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected '{}' to have a declared predicate, but it did not",
                        kind
                    ))
                }
            },
        ),
        check_def(
            "the outcome predicate outcome for {string} has no declared predicate",
            &[(RESULT_KEY, "OutcomePredicateResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let result = ctx
                    .get::<OutcomePredicateResult>(RESULT_KEY)
                    .ok_or("No outcome predicate result")?;
                let row = result
                    .outcome(&kind)
                    .ok_or_else(|| format!("No outcome predicate row for '{}'", kind))?;
                if !row.predicate_declared {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected '{}' to have no declared predicate, but it did",
                        kind
                    ))
                }
            },
        ),
        check_def(
            "the outcome predicate outcome for {string} has predicate rate permille {int}",
            &[(RESULT_KEY, "OutcomePredicateResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let expected = params.get_int(1).ok_or("Expected permille")? as u64;
                let result = ctx
                    .get::<OutcomePredicateResult>(RESULT_KEY)
                    .ok_or("No outcome predicate result")?;
                let row = result
                    .outcome(&kind)
                    .ok_or_else(|| format!("No outcome predicate row for '{}'", kind))?;
                let actual = (row.predicate_rate * 1000.0).round() as u64;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "outcome predicate outcome '{}' rate permille: expected {}, got {} (rate {})",
                        kind, expected, actual, row.predicate_rate
                    ))
                }
            },
        ),
        check_def(
            "the outcome predicate result has {int} rows",
            &[(RESULT_KEY, "OutcomePredicateResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let result = ctx
                    .get::<OutcomePredicateResult>(RESULT_KEY)
                    .ok_or("No outcome predicate result")?;
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
