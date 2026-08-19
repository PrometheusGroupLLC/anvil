//! Step module for `run_detail.feature` (core domain seam).
//!
//! Exercises the pure `fold_run_detail` fold directly over literal
//! `RunDetailInput`s, so the ordering rule, the nesting rule, the
//! empty-means-absent rule and the no-cost shape can each be pinned precisely
//! without a fixture hearth or an engine process. The full-stack wiring — the
//! two-level directory walk, the `parent_id` selection and both wire surfaces
//! — is covered at its own seam by
//! `anvil-engine/features/run_detail_rpc.feature`.
//!
//! The steps seed ALREADY-FOLDED transitions on purpose. The per-file event
//! store is folded once, by `anvil_core::domain::transition_log`; seeding event
//! files here and folding them again would test this feature against a second
//! copy of that fold rather than against the one the engine runs.

use anvil_core::domain::run_detail::{fold_run_detail, RunDetailInput, RunNode};
use anvil_core::domain::status::StatusTransition;
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::collections::HashMap;

const ROOT_KEY: &str = "rd_root";
const CHILDREN_KEY: &str = "rd_children";
const ACTORS_KEY: &str = "rd_actor_tables";
const NODES_KEY: &str = "rd_nodes";

/// Every key the seeding steps thread forward. A brine Map step's output
/// context is retained to its declared `provides` ONLY, so each accumulating
/// Given has to re-declare and re-emit the whole set or the next one reports
/// unsatisfied keys.
const SEED_KEYS: &[(&str, &str)] = &[
    (ROOT_KEY, "RunDetailInput"),
    (CHILDREN_KEY, "Vec<RunDetailInput>"),
    (ACTORS_KEY, "HashMap<String, HashMap<String, Value>>"),
];

type ActorTables = HashMap<String, HashMap<String, serde_yaml::Value>>;

fn column_index(table: &DataTable, name: &str) -> Result<usize, String> {
    table
        .headers
        .iter()
        .position(|h| h == name)
        .ok_or_else(|| format!("Missing '{}' column in data table", name))
}

/// An empty cell means the record does not carry the field — `None`, never
/// `Some("")`. Reading it as `Some("")` would make the "absent is empty"
/// scenarios pass against a fold that fabricates placeholders.
fn optional(cell: &str) -> Option<String> {
    let trimmed = cell.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Parse a `steps:` table into the ALREADY-FOLDED history the engine hands the
/// fold.
fn parse_steps(table: &DataTable) -> Result<Vec<StatusTransition>, String> {
    let to_col = column_index(table, "to_state")?;
    let at_col = column_index(table, "at")?;
    let actor_col = column_index(table, "actor")?;
    let role_col = column_index(table, "role")?;
    let approver_col = column_index(table, "approver")?;
    let note_col = column_index(table, "note")?;
    Ok(table
        .rows
        .iter()
        .map(|row| StatusTransition {
            to: row[to_col].trim().to_string(),
            at: optional(&row[at_col]),
            actor: optional(&row[actor_col]),
            role: optional(&row[role_col]),
            approver: optional(&row[approver_col]),
            note: optional(&row[note_col]),
            event_type: None,
            satisfaction: None,
        })
        .collect())
}

fn input(instance_id: &str, transitions: Vec<StatusTransition>) -> RunDetailInput {
    RunDetailInput {
        instance_id: instance_id.to_string(),
        kind: "track".to_string(),
        state: transitions
            .last()
            .map(|t| t.to.clone())
            .unwrap_or_default(),
        artifact_dir: format!("/hearth/tracks/{}", instance_id),
        parent_id: String::new(),
        transitions,
        actors: None,
    }
}

/// Clone the whole seeded set forward. Every accumulating Given calls this,
/// mutates its one part, and emits the result.
fn carry(ctx: &Context) -> Result<(RunDetailInput, Vec<RunDetailInput>, ActorTables), String> {
    let root = ctx
        .get::<RunDetailInput>(ROOT_KEY)
        .ok_or("No seeded root run")?
        .clone();
    let children = ctx
        .get::<Vec<RunDetailInput>>(CHILDREN_KEY)
        .cloned()
        .unwrap_or_default();
    let actors = ctx
        .get::<ActorTables>(ACTORS_KEY)
        .cloned()
        .unwrap_or_default();
    Ok((root, children, actors))
}

fn emit(root: RunDetailInput, children: Vec<RunDetailInput>, actors: ActorTables) -> Context {
    let mut out = Context::new();
    out.set(ROOT_KEY, root);
    out.set(CHILDREN_KEY, children);
    out.set(ACTORS_KEY, actors);
    out
}

/// Attach the seeded actor tables to the runs they belong to, immediately
/// before folding.
fn with_actors(mut run: RunDetailInput, tables: &ActorTables) -> RunDetailInput {
    run.actors = tables.get(&run.instance_id).cloned();
    run
}

fn nodes(ctx: &Context) -> Result<&Vec<RunNode>, String> {
    ctx.get::<Vec<RunNode>>(NODES_KEY)
        .ok_or_else(|| "No folded record — the fold step did not run".to_string())
}

fn node<'a>(ctx: &'a Context, instance_id: &str) -> Result<&'a RunNode, String> {
    nodes(ctx)?
        .iter()
        .find(|n| n.instance_id == instance_id)
        .ok_or_else(|| {
            format!(
                "no run '{}' in the record (record holds: {})",
                instance_id,
                nodes(ctx)
                    .map(|n| n
                        .iter()
                        .map(|x| x.instance_id.as_str())
                        .collect::<Vec<_>>()
                        .join(","))
                    .unwrap_or_default()
            )
        })
}

/// Every token a spend figure could reasonably be named. The no-cost assertion
/// walks the serialized record for any KEY containing one of these, so it
/// catches a field added under any of the names a future author might pick,
/// not just the literal `cost`.
const COST_TOKENS: &[&str] = &[
    "cost", "spend", "price", "usd", "dollar", "token", "budget", "charge", "bill",
];

/// Recursively collect every object key in a serialized record.
fn all_keys(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                out.push(key.clone());
                all_keys(child, out);
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                all_keys(child, out);
            }
        }
        _ => {}
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a run {string} that has taken steps:",
            &[],
            SEED_KEYS,
            |_ctx, params| {
                let id = params.get_string(0).ok_or("Expected a run id")?.to_string();
                let table = params.data_table().ok_or("Expected a steps table")?;
                Ok(emit(
                    input(&id, parse_steps(table)?),
                    Vec::new(),
                    ActorTables::new(),
                ))
            },
        ),
        // The same seeding, with the reviewer's verdict column. A separate step
        // rather than a wider table on every existing scenario: most of this
        // feature's scenarios are about ordering and nesting and have no verdict
        // to state, and widening their tables would put a column in front of a
        // reader that the scenario says nothing about.
        step_def(
            "a run {string} that has taken steps with verdicts:",
            &[],
            SEED_KEYS,
            |_ctx, params| {
                let id = params.get_string(0).ok_or("Expected a run id")?.to_string();
                let table = params.data_table().ok_or("Expected a steps table")?;
                let sat_col = table
                    .headers
                    .iter()
                    .position(|h| h == "satisfaction")
                    .ok_or("Missing 'satisfaction' column in data table")?;
                let mut transitions = parse_steps(table)?;
                for (row, transition) in table.rows.iter().zip(transitions.iter_mut()) {
                    transition.satisfaction = optional(&row[sat_col]);
                }
                Ok(emit(input(&id, transitions), Vec::new(), ActorTables::new()))
            },
        ),
        step_def(
            "a run {string} was started from inside it with steps:",
            SEED_KEYS,
            SEED_KEYS,
            |ctx, params| {
                let id = params.get_string(0).ok_or("Expected a run id")?.to_string();
                let table = params.data_table().ok_or("Expected a steps table")?;
                let (root, mut children, actors) = carry(&ctx)?;
                let mut child = input(&id, parse_steps(table)?);
                child.parent_id = root.instance_id.clone();
                children.push(child);
                Ok(emit(root, children, actors))
            },
        ),
        step_def(
            "the run {string} carries an agent actor {string} configured:",
            SEED_KEYS,
            SEED_KEYS,
            |ctx, params| {
                let run_id = params.get_string(0).ok_or("Expected a run id")?.to_string();
                let name = params.get_string(1).ok_or("Expected an actor name")?.to_string();
                let table = params
                    .data_table()
                    .ok_or("Expected a configurations table")?;
                let at_col = column_index(table, "at")?;
                let model_col = column_index(table, "model")?;
                let provider_col = column_index(table, "provider")?;
                // Built as real YAML so the fold reads the SAME shape
                // `fs_actor_write_adapter` writes into a live status.yaml.
                let configurations: Vec<String> = table
                    .rows
                    .iter()
                    .map(|row| {
                        format!(
                            "  - at: \"{}\"\n    model: {}\n    provider: {}\n",
                            row[at_col].trim(),
                            row[model_col].trim(),
                            row[provider_col].trim()
                        )
                    })
                    .collect();
                let yaml = format!("type: agent\nconfigurations:\n{}", configurations.concat());
                let value: serde_yaml::Value = serde_yaml::from_str(&yaml)
                    .map_err(|e| format!("bad actor yaml: {}\n{}", e, yaml))?;
                let (root, children, mut actors) = carry(&ctx)?;
                actors.entry(run_id).or_default().insert(name, value);
                Ok(emit(root, children, actors))
            },
        ),
        step_def(
            "the run {string} carries a human actor {string}",
            SEED_KEYS,
            SEED_KEYS,
            |ctx, params| {
                let run_id = params.get_string(0).ok_or("Expected a run id")?.to_string();
                let name = params.get_string(1).ok_or("Expected an actor name")?.to_string();
                // A human carries NO configurations — the shape that must yield
                // an empty model rather than a guessed one.
                let value: serde_yaml::Value = serde_yaml::from_str("type: human\n")
                    .map_err(|e| format!("bad actor yaml: {}", e))?;
                let (root, children, mut actors) = carry(&ctx)?;
                actors.entry(run_id).or_default().insert(name, value);
                Ok(emit(root, children, actors))
            },
        ),
        step_def(
            "the run's record is folded",
            SEED_KEYS,
            &[(NODES_KEY, "Vec<RunNode>")],
            |ctx, _params| {
                let (root, children, actors) = carry(&ctx)?;
                let root = with_actors(root, &actors);
                let children: Vec<RunDetailInput> = children
                    .into_iter()
                    .map(|child| with_actors(child, &actors))
                    .collect();
                let mut out = Context::new();
                out.set(NODES_KEY, fold_run_detail(&root, &children));
                Ok(out)
            },
        ),
        check_def(
            "the record lists the runs in the order {string}",
            &[(NODES_KEY, "Vec<RunNode>")],
            |ctx, params| {
                let expected: Vec<&str> = params
                    .get_string(0)
                    .ok_or("Expected a comma-separated run order")?
                    .split(',')
                    .map(str::trim)
                    .collect();
                let actual: Vec<&str> = nodes(&ctx)?
                    .iter()
                    .map(|n| n.instance_id.as_str())
                    .collect();
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("record order {:?}, expected {:?}", actual, expected))
                }
            },
        ),
        check_def(
            "the run {string} is at depth {int} and is nested under nothing",
            &[(NODES_KEY, "Vec<RunNode>")],
            |ctx, params| {
                let id = params.get_string(0).ok_or("Expected a run id")?;
                let depth = params.get_int(1).ok_or("Expected a depth")? as u32;
                let node = node(&ctx, id)?;
                if node.depth != depth {
                    return Err(format!("run '{}' is at depth {}, expected {}", id, node.depth, depth));
                }
                if !node.parent_instance_id.is_empty() {
                    return Err(format!(
                        "run '{}' is nested under '{}', expected nothing",
                        id, node.parent_instance_id
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the run {string} is at depth {int} nested under {string}",
            &[(NODES_KEY, "Vec<RunNode>")],
            |ctx, params| {
                let id = params.get_string(0).ok_or("Expected a run id")?;
                let depth = params.get_int(1).ok_or("Expected a depth")? as u32;
                let parent = params.get_string(2).ok_or("Expected a parent run id")?;
                let node = node(&ctx, id)?;
                if node.depth != depth {
                    return Err(format!("run '{}' is at depth {}, expected {}", id, node.depth, depth));
                }
                if node.parent_instance_id != parent {
                    return Err(format!(
                        "run '{}' is nested under '{}', expected '{}'",
                        id, node.parent_instance_id, parent
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the run {string} has steps to states {string}",
            &[(NODES_KEY, "Vec<RunNode>")],
            |ctx, params| {
                let id = params.get_string(0).ok_or("Expected a run id")?;
                let expected: Vec<&str> = params
                    .get_string(1)
                    .ok_or("Expected comma-separated states")?
                    .split(',')
                    .map(str::trim)
                    .collect();
                let actual: Vec<&str> = node(&ctx, id)?
                    .steps
                    .iter()
                    .map(|s| s.to_state.as_str())
                    .collect();
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("run '{}' steps {:?}, expected {:?}", id, actual, expected))
                }
            },
        ),
        check_def(
            "the step to {string} in run {string} carries the verdict {string}",
            &[(NODES_KEY, "Vec<RunNode>")],
            |ctx, params| {
                let to_state = params.get_string(0).ok_or("Expected a state")?;
                let id = params.get_string(1).ok_or("Expected a run id")?;
                let expected = params.get_string(2).ok_or("Expected a verdict")?;
                let step = find_step(&ctx, id, to_state)?;
                // An EMPTY expectation asserts absence. This record's convention
                // is that empty means the record does not carry the field — a
                // surface's cue to render nothing, never "none" or an invented
                // approval.
                if step.verdict == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "the step to '{}' in run '{}' carries the verdict '{}', expected '{}'",
                        to_state, id, step.verdict, expected
                    ))
                }
            },
        ),
        check_def(
            "the run {string} lists actors {string}",
            &[(NODES_KEY, "Vec<RunNode>")],
            |ctx, params| {
                let id = params.get_string(0).ok_or("Expected a run id")?;
                let expected: Vec<&str> = params
                    .get_string(1)
                    .ok_or("Expected comma-separated actor names")?
                    .split(',')
                    .map(str::trim)
                    .collect();
                let actual: Vec<&str> = node(&ctx, id)?
                    .actors
                    .iter()
                    .map(|a| a.name.as_str())
                    .collect();
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("run '{}' actors {:?}, expected {:?}", id, actual, expected))
                }
            },
        ),
        check_def(
            "the step to {string} in run {string} was taken by {string} playing {string} at {string}",
            &[(NODES_KEY, "Vec<RunNode>")],
            |ctx, params| {
                let to_state = params.get_string(0).ok_or("Expected a to_state")?;
                let id = params.get_string(1).ok_or("Expected a run id")?;
                let actor = params.get_string(2).ok_or("Expected an actor")?;
                let role = params.get_string(3).ok_or("Expected a role")?;
                let at = params.get_string(4).ok_or("Expected a time")?;
                let step = find_step(&ctx, id, to_state)?;
                if step.actor != actor || step.role != role || step.at != at {
                    return Err(format!(
                        "step to '{}' in run '{}' is actor='{}' role='{}' at='{}', expected actor='{}' role='{}' at='{}'",
                        to_state, id, step.actor, step.role, step.at, actor, role, at
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the step to {string} in run {string} names approver {string}",
            &[(NODES_KEY, "Vec<RunNode>")],
            |ctx, params| {
                let to_state = params.get_string(0).ok_or("Expected a to_state")?;
                let id = params.get_string(1).ok_or("Expected a run id")?;
                let approver = params.get_string(2).ok_or("Expected an approver")?;
                let step = find_step(&ctx, id, to_state)?;
                if step.approver == approver {
                    Ok(())
                } else {
                    Err(format!(
                        "step to '{}' in run '{}' names approver '{}', expected '{}'",
                        to_state, id, step.approver, approver
                    ))
                }
            },
        ),
        check_def(
            "the step to {string} in run {string} carries no approver",
            &[(NODES_KEY, "Vec<RunNode>")],
            |ctx, params| {
                let to_state = params.get_string(0).ok_or("Expected a to_state")?;
                let id = params.get_string(1).ok_or("Expected a run id")?;
                let step = find_step(&ctx, id, to_state)?;
                if step.approver.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "step to '{}' in run '{}' names approver '{}' — a step nobody approved must name nobody",
                        to_state, id, step.approver
                    ))
                }
            },
        ),
        check_def(
            "the step to {string} in run {string} carries no note",
            &[(NODES_KEY, "Vec<RunNode>")],
            |ctx, params| {
                let to_state = params.get_string(0).ok_or("Expected a to_state")?;
                let id = params.get_string(1).ok_or("Expected a run id")?;
                let step = find_step(&ctx, id, to_state)?;
                if step.note.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "step to '{}' in run '{}' carries note '{}', expected none",
                        to_state, id, step.note
                    ))
                }
            },
        ),
        check_def(
            "the actor {string} in run {string} is an {string} on model {string} from {string}",
            &[(NODES_KEY, "Vec<RunNode>")],
            |ctx, params| check_actor(&ctx, &params),
        ),
        check_def(
            "the actor {string} in run {string} is a {string} on model {string} from {string}",
            &[(NODES_KEY, "Vec<RunNode>")],
            |ctx, params| check_actor(&ctx, &params),
        ),
        check_def(
            "no part of the record carries a cost figure",
            &[(NODES_KEY, "Vec<RunNode>")],
            |ctx, _params| {
                let serialized = serde_json::to_value(nodes(&ctx)?)
                    .map_err(|e| format!("could not serialize the record: {}", e))?;
                let mut keys = Vec::new();
                all_keys(&serialized, &mut keys);
                let offenders: Vec<String> = keys
                    .into_iter()
                    .filter(|key| {
                        let lower = key.to_ascii_lowercase();
                        COST_TOKENS.iter().any(|token| lower.contains(token))
                    })
                    .collect();
                if offenders.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "the record carries cost-shaped field(s) {:?} — anvil records no cost, so any \
                         value here is fabricated",
                        offenders
                    ))
                }
            },
        ),
        check_def(
            "the record was not empty",
            &[(NODES_KEY, "Vec<RunNode>")],
            |ctx, _params| {
                // The control for the assertion above: an empty record carries no
                // cost key either, so without this the no-cost scenario would pass
                // against a fold that returns nothing at all.
                let record = nodes(&ctx)?;
                if record.iter().any(|n| !n.steps.is_empty()) && record.len() > 1 {
                    Ok(())
                } else {
                    Err(format!(
                        "the record holds {} run(s) and no steps to speak of — the no-cost \
                         assertion would have been vacuous",
                        record.len()
                    ))
                }
            },
        ),
    ]
}

fn find_step(
    ctx: &Context,
    instance_id: &str,
    to_state: &str,
) -> Result<anvil_core::domain::run_detail::RunStep, String> {
    node(ctx, instance_id)?
        .steps
        .iter()
        .find(|s| s.to_state == to_state)
        .cloned()
        .ok_or_else(|| format!("run '{}' has no step to '{}'", instance_id, to_state))
}

/// Shared body for the two actor assertions — the feature reads "is an agent"
/// and "is a human", which are two patterns for one check.
fn check_actor(
    ctx: &Context,
    params: &brine_runner_rust::registry::Params,
) -> Result<(), String> {
    let name = params.get_string(0).ok_or("Expected an actor name")?;
    let id = params.get_string(1).ok_or("Expected a run id")?;
    let actor_type = params.get_string(2).ok_or("Expected an actor type")?;
    let model = params.get_string(3).ok_or("Expected a model")?;
    let provider = params.get_string(4).ok_or("Expected a provider")?;
    let actor = node(ctx, id)?
        .actors
        .iter()
        .find(|a| a.name == name)
        .ok_or_else(|| format!("run '{}' lists no actor '{}'", id, name))?;
    if actor.actor_type != actor_type || actor.model != model || actor.provider != provider {
        return Err(format!(
            "actor '{}' in run '{}' is type='{}' model='{}' provider='{}', expected type='{}' model='{}' provider='{}'",
            name, id, actor.actor_type, actor.model, actor.provider, actor_type, model, provider
        ));
    }
    Ok(())
}
