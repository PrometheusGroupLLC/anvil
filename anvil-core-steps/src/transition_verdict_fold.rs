//! Steps for `transition_verdict_fold.feature` — the reviewer verdict's trip
//! from the event file, through the one transition-log fold, onto the folded
//! step every reader sees.
//!
//! Exercises the REAL filesystem: the fixture writes event files through the
//! shipped `FileSystemTransitionEventAdapter` and the assertions read what
//! `resolve_transitions_with_events` folds back. The property under test is that
//! the verdict survives serialisation and the fold, and neither is observable
//! against an in-memory stand-in.
//!
//! THE ORACLE DOES NOT READ THE THING IT CHECKS. The checks never touch the
//! `TransitionRecord`s the fixture wrote; they read the `StatusTransition`s the
//! fold produced. A check that re-read the seeded records would prove only that
//! the fixture can remember its own input.

use anvil_core::domain::status::{FullStatusYaml, StatusTransition};
use anvil_core::domain::transition_log::resolve_transitions_with_events;
use anvil_core_hearth::fs_transition_event_adapter::FileSystemTransitionEventAdapter;
use anvil_core::ports::transition_event_write_port::{
    TransitionEventWritePort,
    TransitionRecord,
};
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;

const DIR_KEY: &str = "tvf_dir";
const STATUS_KEY: &str = "tvf_status_yaml";
const FOLDED_KEY: &str = "tvf_folded";

const SEED_KEYS: &[(&str, &str)] = &[(DIR_KEY, "PathBuf"), (STATUS_KEY, "String")];

fn column_index(table: &DataTable, name: &str) -> Result<usize, String> {
    table
        .headers
        .iter()
        .position(|h| h == name)
        .ok_or_else(|| format!("Missing '{}' column in data table", name))
}

/// An empty cell means the record does not carry the field.
fn optional(cell: &str) -> Option<String> {
    let trimmed = cell.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// A fresh scratch hearth per scenario, under the process's own temp dir and
/// named with the process id so two concurrent runs cannot collide.
fn scratch_hearth() -> Result<PathBuf, String> {
    let root = std::env::temp_dir().join(format!(
        "anvil-tvf-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join("tracks").join("run")).map_err(|e| e.to_string())?;
    Ok(root)
}

fn folded(ctx: &Context) -> Result<&Vec<StatusTransition>, String> {
    ctx.get::<Vec<StatusTransition>>(FOLDED_KEY)
        .ok_or_else(|| "No folded history — the fold step did not run".to_string())
}

fn step_to<'a>(ctx: &'a Context, to_state: &str) -> Result<&'a StatusTransition, String> {
    folded(ctx)?
        .iter()
        .find(|t| t.to == to_state)
        .ok_or_else(|| {
            format!(
                "no folded step to '{}' (history holds: {})",
                to_state,
                folded(ctx)
                    .map(|h| h
                        .iter()
                        .map(|t| t.to.as_str())
                        .collect::<Vec<_>>()
                        .join(","))
                    .unwrap_or_default()
            )
        })
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "an artifact directory with the transition events:",
            &[],
            SEED_KEYS,
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected an events table")?;
                let to_col = column_index(table, "to")?;
                let at_col = column_index(table, "at")?;
                let actor_col = column_index(table, "actor")?;
                let role_col = column_index(table, "role")?;
                let sat_col = column_index(table, "satisfaction")?;

                let hearth = scratch_hearth()?;
                let adapter = FileSystemTransitionEventAdapter::new(hearth.clone());
                for (index, row) in table.rows.iter().enumerate() {
                    let record = TransitionRecord {
                        to: row[to_col].trim().to_string(),
                        at: row[at_col].trim().to_string(),
                        actor: row[actor_col].trim().to_string(),
                        role: row[role_col].trim().to_string(),
                        approver: None,
                        note: None,
                        satisfaction: optional(&row[sat_col]),
                        event_type: None,
                    };
                    adapter
                        .append_transition_event("tracks/run", &record)
                        .map_err(|e| format!("seeding event {}: {}", index, e))?;
                }

                let mut out = Context::new();
                out.set(DIR_KEY, hearth.join("tracks").join("run"));
                out.set(STATUS_KEY, "version: 1\nkind: track\n".to_string());
                Ok(out)
            },
        ),
        step_def(
            "an artifact whose legacy status.yaml lists the transitions:",
            &[],
            SEED_KEYS,
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected a transitions table")?;
                let to_col = column_index(table, "to")?;
                let at_col = column_index(table, "at")?;
                let actor_col = column_index(table, "actor")?;
                let role_col = column_index(table, "role")?;

                // Written as YAML TEXT and parsed by the shipped deserializer,
                // not built as a literal struct: the claim is that a legacy row
                // — which has no satisfaction column and never will — folds to
                // no verdict, and constructing the struct in Rust would let the
                // fixture choose the value the fold is supposed to derive.
                let mut yaml = String::from("version: 1\nkind: track\ntransitions:\n");
                for row in &table.rows {
                    yaml.push_str(&format!(
                        "  - to: {}\n    at: \"{}\"\n    actor: {}\n    role: {}\n",
                        row[to_col].trim(),
                        row[at_col].trim(),
                        row[actor_col].trim(),
                        row[role_col].trim(),
                    ));
                }

                let hearth = scratch_hearth()?;
                let mut out = Context::new();
                out.set(DIR_KEY, hearth.join("tracks").join("run"));
                out.set(STATUS_KEY, yaml);
                Ok(out)
            },
        ),
        step_def(
            "an artifact whose legacy status.yaml lists the transitions with verdicts:",
            &[],
            SEED_KEYS,
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected a transitions table")?;
                let to_col = column_index(table, "to")?;
                let at_col = column_index(table, "at")?;
                let actor_col = column_index(table, "actor")?;
                let role_col = column_index(table, "role")?;
                let sat_col = column_index(table, "satisfaction")?;

                let mut yaml = String::from("version: 1\nkind: track\ntransitions:\n");
                for row in &table.rows {
                    yaml.push_str(&format!(
                        "  - to: {}\n    at: \"{}\"\n    actor: {}\n    role: {}\n    satisfaction: {}\n",
                        row[to_col].trim(),
                        row[at_col].trim(),
                        row[actor_col].trim(),
                        row[role_col].trim(),
                        row[sat_col].trim(),
                    ));
                }

                let hearth = scratch_hearth()?;
                let mut out = Context::new();
                out.set(DIR_KEY, hearth.join("tracks").join("run"));
                out.set(STATUS_KEY, yaml);
                Ok(out)
            },
        ),
        step_def(
            "the artifact's history is folded from the event store",
            SEED_KEYS,
            &[(FOLDED_KEY, "Vec<StatusTransition>")],
            |ctx, _params| {
                let dir = ctx
                    .get::<PathBuf>(DIR_KEY)
                    .ok_or("No seeded artifact directory")?
                    .clone();
                let yaml = ctx
                    .get::<String>(STATUS_KEY)
                    .ok_or("No seeded status.yaml")?
                    .clone();
                let status: FullStatusYaml =
                    serde_yaml::from_str(&yaml).map_err(|e| format!("status.yaml: {}", e))?;
                let history = resolve_transitions_with_events(&status, &dir)
                    .map_err(|e| format!("fold refused: {}", e))?;
                let mut out = Context::new();
                out.set(FOLDED_KEY, history);
                Ok(out)
            },
        ),
        check_def(
            "the folded step to {string} carries the verdict {string}",
            &[(FOLDED_KEY, "Vec<StatusTransition>")],
            |ctx, params| {
                let to_state = params.get_string(0).ok_or("Expected a state")?;
                let expected = params.get_string(1).ok_or("Expected a verdict")?;
                let step = step_to(&ctx, to_state)?;
                match step.satisfaction.as_deref() {
                    Some(actual) if actual == expected => Ok(()),
                    other => Err(format!(
                        "folded step to '{}' carries verdict {:?}, expected '{}'",
                        to_state, other, expected
                    )),
                }
            },
        ),
        check_def(
            "the folded step to {string} was recorded by {string}",
            &[(FOLDED_KEY, "Vec<StatusTransition>")],
            |ctx, params| {
                let to_state = params.get_string(0).ok_or("Expected a state")?;
                let expected = params.get_string(1).ok_or("Expected an actor")?;
                let step = step_to(&ctx, to_state)?;
                match step.actor.as_deref() {
                    Some(actual) if actual == expected => Ok(()),
                    other => Err(format!(
                        "folded step to '{}' was recorded by {:?}, expected '{}'",
                        to_state, other, expected
                    )),
                }
            },
        ),
        check_def(
            "the folded step to {string} carries no verdict",
            &[(FOLDED_KEY, "Vec<StatusTransition>")],
            |ctx, params| {
                let to_state = params.get_string(0).ok_or("Expected a state")?;
                let step = step_to(&ctx, to_state)?;
                // `None`, not `Some("")`. An empty string is a value, and a
                // reader asking only whether the field is present would take it
                // for a verdict nobody rendered.
                match step.satisfaction.as_deref() {
                    None => Ok(()),
                    Some(actual) => Err(format!(
                        "folded step to '{}' carries verdict '{}', expected none at all",
                        to_state, actual
                    )),
                }
            },
        ),
    ]
}
