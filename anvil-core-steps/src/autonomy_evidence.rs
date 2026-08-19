//! Steps for `autonomy_evidence.feature` — the case a playbook makes for a
//! rung, folded from what was recorded.
//!
//! Exercises the pure `fold_autonomy_evidence` fold directly over literal
//! `EvidenceRunInput`s, so each definition can be pinned on its own. The trip
//! from the event store to the folded step lives at its own seam in
//! `transition_verdict_fold.feature`, and seeding event files here would test
//! this feature against a second copy of that fold rather than against the one
//! the engine runs.
//!
//! WHETHER A RUN REACHED THE END, AND HOW MANY TIMES IT WAS SENT BACK, ARE
//! STATED BY THE FEATURE. They are `playbook_run_fidelity`'s answers, and this
//! fold takes them rather than forming a second opinion; the feature therefore
//! supplies them the way the caller does (`sent back N times`) instead of
//! deriving them from the seeded state names. That separation is what makes the
//! disagreement scenario — a count that knows about more corrections than the
//! history can name — expressible at all.

use anvil_core::domain::autonomy_evidence::{
    fold_autonomy_evidence,
    grade_cleanliness_score,
    AutonomyEvidence,
    Correction,
    EvidenceRunInput,
    RunVerdict,
};
use anvil_core::domain::status::StatusTransition;
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, Params, StepDef};
use std::collections::HashMap;

const RUNS_KEY: &str = "ae_runs";
const CASE_KEY: &str = "ae_case";
const SCORE_KEY: &str = "ae_score";

const SEED_KEYS: &[(&str, &str)] = &[(RUNS_KEY, "Vec<EvidenceRunInput>")];
const CASE_KEYS: &[(&str, &str)] = &[(CASE_KEY, "Option<AutonomyEvidence>")];

fn column_index(table: &DataTable, name: &str) -> Result<usize, String> {
    table
        .headers
        .iter()
        .position(|h| h == name)
        .ok_or_else(|| format!("Missing '{}' column in data table", name))
}

/// An empty cell means the record does not carry the field — `None`, never
/// `Some("")`. Reading it as `Some("")` would make the no-verdict assertions
/// pass against a fold that fabricates placeholders.
fn optional(cell: &str) -> Option<String> {
    let trimmed = cell.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn parse_steps(table: &DataTable) -> Result<Vec<StatusTransition>, String> {
    let to_col = column_index(table, "to_state")?;
    let at_col = column_index(table, "at")?;
    let actor_col = column_index(table, "actor")?;
    let role_col = column_index(table, "role")?;
    let sat_col = column_index(table, "satisfaction")?;
    // `approver` is optional: most scenarios have no approvals, and requiring
    // the column would make every table wider than the fact it pins.
    let approver_col = table.headers.iter().position(|h| h == "approver");
    Ok(table
        .rows
        .iter()
        .map(|row| StatusTransition {
            to: row[to_col].trim().to_string(),
            at: optional(&row[at_col]),
            actor: optional(&row[actor_col]),
            role: optional(&row[role_col]),
            approver: approver_col.and_then(|c| optional(&row[c])),
            note: None,
            event_type: None,
            satisfaction: optional(&row[sat_col]),
        })
        .collect())
}

fn seed_run(
    ctx: &Context,
    params: &Params,
    reached_terminal: bool,
    transitions: Vec<StatusTransition>,
) -> Result<Context, String> {
    let id = params.get_string(0).ok_or("Expected a run id")?.to_string();
    let revision_cycles = params.get_int(1).ok_or("Expected a sent-back count")? as u64;
    let mut runs = ctx
        .get::<Vec<EvidenceRunInput>>(RUNS_KEY)
        .cloned()
        .unwrap_or_default();
    runs.push(EvidenceRunInput {
        instance_id: id,
        reached_terminal,
        revision_cycles,
        transitions,
        actors: None,
    });
    let mut out = Context::new();
    out.set(RUNS_KEY, runs);
    Ok(out)
}

fn seed_with_table(ctx: &Context, params: &Params, reached_terminal: bool) -> Result<Context, String> {
    let table = params.data_table().ok_or("Expected a steps table")?;
    let steps = parse_steps(table)?;
    seed_run(ctx, params, reached_terminal, steps)
}

fn case(ctx: &Context) -> Result<&AutonomyEvidence, String> {
    ctx.get::<Option<AutonomyEvidence>>(CASE_KEY)
        .ok_or_else(|| "The case was never folded".to_string())?
        .as_ref()
        .ok_or_else(|| "There is no case at all — the fold answered absent".to_string())
}

fn verdict<'a>(ctx: &'a Context, run_id: &str) -> Result<&'a RunVerdict, String> {
    case(ctx)?
        .run_verdicts
        .iter()
        .find(|v| v.run_id == run_id)
        .ok_or_else(|| {
            format!(
                "no run '{}' in the case (case holds: {})",
                run_id,
                case(ctx)
                    .map(|c| c
                        .run_verdicts
                        .iter()
                        .map(|v| v.run_id.as_str())
                        .collect::<Vec<_>>()
                        .join(","))
                    .unwrap_or_default()
            )
        })
}

/// The case's single correction, or a failure naming how many there really are.
fn only_correction(ctx: &Context) -> Result<&Correction, String> {
    let all = &case(ctx)?.corrections;
    match all.len() {
        1 => Ok(&all[0]),
        n => Err(format!(
            "the case names {} wrong action(s), expected exactly 1: {:?}",
            n, all
        )),
    }
}

/// Every token a spend figure could reasonably be named. The no-cost assertion
/// walks the serialized record for any KEY containing one of these, so it
/// catches a field added under any of the names a future author might pick, not
/// just the literal `cost`. The same list `run_detail`'s structural check uses.
const COST_TOKENS: &[&str] = &[
    "cost", "spend", "price", "usd", "dollar", "token", "budget", "charge", "bill",
];

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
            "a completed run {string} sent back {int} time, with the steps:",
            &[],
            SEED_KEYS,
            |ctx, params| seed_with_table(&ctx, &params, true),
        ),
        step_def(
            "a completed run {string} sent back {int} times, with the steps:",
            &[],
            SEED_KEYS,
            |ctx, params| seed_with_table(&ctx, &params, true),
        ),
        step_def(
            "an unfinished run {string} sent back {int} times, with the steps:",
            &[],
            SEED_KEYS,
            |ctx, params| seed_with_table(&ctx, &params, false),
        ),
        step_def(
            "a completed run {string} sent back {int} times, with no steps",
            &[],
            SEED_KEYS,
            |ctx, params| seed_run(&ctx, &params, true, Vec::new()),
        ),
        step_def(
            "no runs at all",
            &[],
            SEED_KEYS,
            |_ctx, _params| {
                let mut out = Context::new();
                out.set(RUNS_KEY, Vec::<EvidenceRunInput>::new());
                Ok(out)
            },
        ),
        step_def(
            "the run {string} lists the actors:",
            SEED_KEYS,
            SEED_KEYS,
            |ctx, params| {
                let run_id = params.get_string(0).ok_or("Expected a run id")?.to_string();
                let table = params.data_table().ok_or("Expected an actors table")?;
                let name_col = column_index(table, "name")?;
                let type_col = column_index(table, "type")?;
                let mut actors: HashMap<String, serde_yaml::Value> = HashMap::new();
                for row in &table.rows {
                    let mut entry = serde_yaml::Mapping::new();
                    entry.insert(
                        serde_yaml::Value::String("type".to_string()),
                        serde_yaml::Value::String(row[type_col].trim().to_string()),
                    );
                    actors.insert(
                        row[name_col].trim().to_string(),
                        serde_yaml::Value::Mapping(entry),
                    );
                }
                let mut runs = ctx
                    .get::<Vec<EvidenceRunInput>>(RUNS_KEY)
                    .cloned()
                    .unwrap_or_default();
                let run = runs
                    .iter_mut()
                    .find(|r| r.instance_id == run_id)
                    .ok_or_else(|| format!("no seeded run '{}'", run_id))?;
                run.actors = Some(actors);
                let mut out = Context::new();
                out.set(RUNS_KEY, runs);
                Ok(out)
            },
        ),
        step_def(
            "the case is folded",
            SEED_KEYS,
            CASE_KEYS,
            |ctx, _params| {
                let runs = ctx
                    .get::<Vec<EvidenceRunInput>>(RUNS_KEY)
                    .ok_or("No seeded runs")?
                    .clone();
                let mut out = Context::new();
                out.set(CASE_KEY, fold_autonomy_evidence(&runs));
                Ok(out)
            },
        ),
        // ── the cleanliness score, graded on its own ──────────────────────
        step_def(
            "a run that reached the end and was sent back {int} time",
            &[],
            &[(SCORE_KEY, "f64")],
            |_ctx, params| score_step(&params, true),
        ),
        step_def(
            "a run that reached the end and was sent back {int} times",
            &[],
            &[(SCORE_KEY, "f64")],
            |_ctx, params| score_step(&params, true),
        ),
        step_def(
            "a run that did not reach the end and was sent back {int} times",
            &[],
            &[(SCORE_KEY, "f64")],
            |_ctx, params| score_step(&params, false),
        ),
        check_def(
            "its cleanliness score is {int}",
            &[(SCORE_KEY, "f64")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected a score")? as f64;
                let actual = *ctx.get::<f64>(SCORE_KEY).ok_or("No score was graded")?;
                if (actual - expected).abs() < f64::EPSILON {
                    Ok(())
                } else {
                    Err(format!(
                        "the cleanliness score is {}, expected {}",
                        actual, expected
                    ))
                }
            },
        ),
        // ── clean ─────────────────────────────────────────────────────────
        check_def("the run {string} is clean", CASE_KEYS, |ctx, params| {
            let id = params.get_string(0).ok_or("Expected a run id")?;
            let v = verdict(&ctx, id)?;
            if v.clean {
                Ok(())
            } else {
                Err(format!(
                    "run '{}' is not clean: reached_terminal={}, revision_cycles={}",
                    id, v.reached_terminal, v.revision_cycles
                ))
            }
        }),
        check_def("the run {string} is not clean", CASE_KEYS, |ctx, params| {
            let id = params.get_string(0).ok_or("Expected a run id")?;
            let v = verdict(&ctx, id)?;
            if v.clean {
                Err(format!(
                    "run '{}' is clean: reached_terminal={}, revision_cycles={}",
                    id, v.reached_terminal, v.revision_cycles
                ))
            } else {
                Ok(())
            }
        }),
        // ── the attribution ───────────────────────────────────────────────
        check_def(
            "the case names {int} wrong actions",
            CASE_KEYS,
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected a count")? as usize;
                let c = case(&ctx)?;
                if c.corrections.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "the case names {} wrong action(s), expected {}: {:?}",
                        c.corrections.len(),
                        expected,
                        c.corrections
                    ))
                }
            },
        ),
        check_def(
            "the case names {int} wrong action, and it happened in run {string}",
            CASE_KEYS,
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected a count")? as usize;
                let run_id = params.get_string(1).ok_or("Expected a run id")?;
                let c = case(&ctx)?;
                if c.corrections.len() != expected {
                    return Err(format!(
                        "the case names {} wrong action(s), expected {}",
                        c.corrections.len(),
                        expected
                    ));
                }
                let found = only_correction(&ctx)?;
                if found.run_id == run_id {
                    Ok(())
                } else {
                    Err(format!(
                        "the wrong action names run '{}', expected '{}'",
                        found.run_id, run_id
                    ))
                }
            },
        ),
        check_def(
            "the wrong action was caught at {string}",
            CASE_KEYS,
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected a moment")?;
                let found = only_correction(&ctx)?;
                if found.at == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "the wrong action was caught at '{}', expected '{}'",
                        found.at, expected
                    ))
                }
            },
        ),
        check_def(
            "the wrong action was caught by {string} at the step {string}",
            CASE_KEYS,
            |ctx, params| {
                let catcher = params.get_string(0).ok_or("Expected a catcher")?;
                let wrong_state = params.get_string(1).ok_or("Expected a step state")?;
                let found = only_correction(&ctx)?;
                if found.caught_by != catcher {
                    return Err(format!(
                        "the wrong action was caught by '{}', expected '{}'",
                        found.caught_by, catcher
                    ));
                }
                if found.wrong_step_state != wrong_state {
                    return Err(format!(
                        "the wrong action overturned the step '{}' (taken by '{}'), expected '{}'",
                        found.wrong_step_state, found.wrong_step_actor, wrong_state
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the wrong action landed in {string} with the verdict {string}, overturning a step by {string}",
            CASE_KEYS,
            |ctx, params| {
                let state = params.get_string(0).ok_or("Expected a revision state")?;
                let verdict = params.get_string(1).ok_or("Expected a verdict")?;
                let by = params.get_string(2).ok_or("Expected an actor")?;
                let found = only_correction(&ctx)?;
                if found.revision_state != state {
                    return Err(format!(
                        "the wrong action landed in '{}', expected '{}'",
                        found.revision_state, state
                    ));
                }
                // An empty expectation is an assertion of ABSENCE: a machine can
                // route work back with nobody recording a verdict on the step,
                // and inventing one there would be the placeholder this whole
                // change exists to refuse.
                if found.verdict != verdict {
                    return Err(format!(
                        "the wrong action carries the verdict '{}', expected '{}'",
                        found.verdict, verdict
                    ));
                }
                if found.wrong_step_actor != by {
                    return Err(format!(
                        "the wrong action overturned a step by '{}', expected '{}'",
                        found.wrong_step_actor, by
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the wrong action caught by {string} happened in run {string}",
            CASE_KEYS,
            |ctx, params| {
                let catcher = params.get_string(0).ok_or("Expected a catcher")?;
                let run_id = params.get_string(1).ok_or("Expected a run id")?;
                let c = case(&ctx)?;
                let found = c
                    .corrections
                    .iter()
                    .find(|x| x.caught_by == catcher)
                    .ok_or_else(|| {
                        format!(
                            "no wrong action caught by '{}'; the case names: {:?}",
                            catcher, c.corrections
                        )
                    })?;
                if found.run_id == run_id {
                    Ok(())
                } else {
                    Err(format!(
                        "the wrong action caught by '{}' names run '{}', expected '{}'",
                        catcher, found.run_id, run_id
                    ))
                }
            },
        ),
        check_def(
            "the run {string} names {int} wrong action the count does not know about, and the two counts disagree",
            CASE_KEYS,
            |ctx, params| {
                let id = params.get_string(0).ok_or("Expected a run id")?;
                let beyond = params.get_int(1).ok_or("Expected a count")? as u64;
                let v = verdict(&ctx, id)?;
                if v.corrections_beyond_count != beyond {
                    return Err(format!(
                        "run '{}' names {} wrong action(s) beyond the declared count, expected {} (declared={}, attributed={})",
                        id,
                        v.corrections_beyond_count,
                        beyond,
                        v.revision_cycles,
                        v.corrections.len()
                    ));
                }
                if v.counts_agree {
                    return Err(format!(
                        "run '{}' reports the two counts AGREE (declared={}, attributed={}); \
                         a disagreement that reads as agreement is the whole defect",
                        id,
                        v.revision_cycles,
                        v.corrections.len()
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the run {string} names {int} wrong action and {int} it could not name",
            CASE_KEYS,
            |ctx, params| {
                let id = params.get_string(0).ok_or("Expected a run id")?;
                let named = params.get_int(1).ok_or("Expected a named count")? as usize;
                let unnamed = params.get_int(2).ok_or("Expected an unnamed count")? as u64;
                let v = verdict(&ctx, id)?;
                if v.corrections.len() != named {
                    return Err(format!(
                        "run '{}' names {} wrong action(s), expected {}",
                        id,
                        v.corrections.len(),
                        named
                    ));
                }
                if v.corrections_unattributed != unnamed {
                    return Err(format!(
                        "run '{}' reports {} correction(s) it could not name, expected {} (revision_cycles={})",
                        id, v.corrections_unattributed, unnamed, v.revision_cycles
                    ));
                }
                Ok(())
            },
        ),
        // ── the hands ─────────────────────────────────────────────────────
        check_def(
            "the run {string} records {int} human touch",
            CASE_KEYS,
            |ctx, params| check_human_touches(&ctx, &params),
        ),
        check_def(
            "the run {string} records {int} human touches",
            CASE_KEYS,
            |ctx, params| check_human_touches(&ctx, &params),
        ),
        check_def(
            "the run {string} records no human touch count",
            CASE_KEYS,
            |ctx, params| {
                let id = params.get_string(0).ok_or("Expected a run id")?;
                let v = verdict(&ctx, id)?;
                // ABSENT, not zero. The discriminator is `is_some()`: a check
                // written as `touches == 0` would pass against an
                // implementation that answers a fabricated zero for a run with
                // nothing to count over, which is what this forbids.
                match v.human_touches {
                    None => Ok(()),
                    Some(count) => Err(format!(
                        "run '{}' records {} human touch(es); expected no count at all",
                        id, count
                    )),
                }
            },
        ),
        check_def(
            "the case reports a human-touch delta of {int}",
            CASE_KEYS,
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected a delta")? as i64;
                let c = case(&ctx)?;
                match c.human_touch_delta {
                    Some(actual) if actual == expected => Ok(()),
                    Some(actual) => Err(format!(
                        "the case reports a human-touch delta of {}, expected {} (earliest={:?}, latest={:?})",
                        actual, expected, c.human_touches_earliest, c.human_touches_latest
                    )),
                    None => Err(format!(
                        "the case reports NO human-touch delta, expected {} (earliest={:?}, latest={:?})",
                        expected, c.human_touches_earliest, c.human_touches_latest
                    )),
                }
            },
        ),
        check_def(
            "the case reports no human-touch delta",
            CASE_KEYS,
            |ctx, _params| {
                let c = case(&ctx)?;
                match c.human_touch_delta {
                    None => Ok(()),
                    Some(actual) => Err(format!(
                        "the case reports a human-touch delta of {}; expected none at all (earliest={:?}, latest={:?})",
                        actual, c.human_touches_earliest, c.human_touches_latest
                    )),
                }
            },
        ),
        // ── the two populations ───────────────────────────────────────────
        check_def(
            "the case reports {int} clean of {int} attempted",
            CASE_KEYS,
            |ctx, params| {
                let clean = params.get_int(0).ok_or("Expected a clean count")? as u32;
                let attempted = params.get_int(1).ok_or("Expected an attempted count")? as u32;
                let c = case(&ctx)?;
                // BOTH POPULATIONS, BOTH PRINTED, BOTH REQUIRED NON-ZERO. The
                // clean count alone passes on a playbook with one run, and a
                // zero on either side is the shape a vacuous pass takes.
                if c.runs_clean == 0 || c.runs_attempted == 0 {
                    return Err(format!(
                        "the case reports {} clean of {} attempted — a zero population proves nothing",
                        c.runs_clean, c.runs_attempted
                    ));
                }
                if c.runs_clean == clean && c.runs_attempted == attempted {
                    Ok(())
                } else {
                    Err(format!(
                        "the case reports {} clean of {} attempted, expected {} of {}",
                        c.runs_clean, c.runs_attempted, clean, attempted
                    ))
                }
            },
        ),
        check_def("there is no case to make", CASE_KEYS, |ctx, _params| {
            let held = ctx
                .get::<Option<AutonomyEvidence>>(CASE_KEY)
                .ok_or("The case was never folded")?;
            match held {
                None => Ok(()),
                Some(c) => Err(format!(
                    "a case was folded reporting {} clean of {} attempted; expected no case at all",
                    c.runs_clean, c.runs_attempted
                )),
            }
        }),
        check_def(
            "the case reports {int} bad runs in a row at the newest end",
            CASE_KEYS,
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected a count")? as u32;
                let c = case(&ctx)?;
                if c.consecutive_unclean_from_newest == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "the case reports {} bad runs in a row at the newest end, expected {} (runs oldest-first: {})",
                        c.consecutive_unclean_from_newest,
                        expected,
                        c.run_verdicts
                            .iter()
                            .map(|v| format!("{}={}", v.run_id, if v.clean { "clean" } else { "bad" }))
                            .collect::<Vec<_>>()
                            .join(",")
                    ))
                }
            },
        ),
        // ── the refusal ───────────────────────────────────────────────────
        check_def(
            "no part of the case carries a cost figure, and the case is not empty",
            CASE_KEYS,
            |ctx, _params| {
                let c = case(&ctx)?;
                let serialized = serde_json::to_value(c)
                    .map_err(|e| format!("the case could not be serialized: {}", e))?;
                let mut keys = Vec::new();
                all_keys(&serialized, &mut keys);
                // THE NON-EMPTY ARM. A record that serialized nothing carries no
                // cost-shaped key either, so the forbidden-key half alone is
                // green on an empty record.
                if c.corrections.is_empty() || c.run_verdicts.is_empty() {
                    return Err(format!(
                        "the case is empty ({} run verdict(s), {} correction(s)) — nothing was examined",
                        c.run_verdicts.len(),
                        c.corrections.len()
                    ));
                }
                let offenders: Vec<&String> = keys
                    .iter()
                    .filter(|k| {
                        let lower = k.to_lowercase();
                        COST_TOKENS.iter().any(|t| lower.contains(t))
                    })
                    .collect();
                if offenders.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "the case carries cost-shaped key(s) {:?} across {} keys examined",
                        offenders,
                        keys.len()
                    ))
                }
            },
        ),
    ]
}

fn score_step(params: &Params, reached_terminal: bool) -> Result<Context, String> {
    let revision_cycles = params.get_int(0).ok_or("Expected a sent-back count")? as u64;
    let mut out = Context::new();
    out.set(
        SCORE_KEY,
        grade_cleanliness_score(reached_terminal, revision_cycles),
    );
    Ok(out)
}

fn check_human_touches(ctx: &Context, params: &Params) -> Result<(), String> {
    let id = params.get_string(0).ok_or("Expected a run id")?.to_string();
    let expected = params.get_int(1).ok_or("Expected a touch count")? as u32;
    let v = verdict(ctx, &id)?;
    match v.human_touches {
        Some(actual) if actual == expected => Ok(()),
        Some(actual) => Err(format!(
            "run '{}' records {} human touch(es), expected {}",
            id, actual, expected
        )),
        None => Err(format!(
            "run '{}' records NO human touch count at all, expected {}",
            id, expected
        )),
    }
}
