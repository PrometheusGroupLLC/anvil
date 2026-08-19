//! Step module for `anvil-core/features/join_episode_fold.feature`.
//!
//! The whole scenario lives under ONE context key. Brine replaces the running
//! context with whatever a mapping step returns, retained to that step's
//! declared `provides` — so a module with one key per fact has to re-carry every
//! key on every step, and one that forgets silently drops half its setup.
//!
//! Delivery records are built through `project_delivery_record` because
//! `DeliveryLogRecord` is `#[non_exhaustive]` and this is a different crate: a
//! struct literal does not compile. `pre_migration` is then set on the returned
//! value, which is exactly what the READER does — it is the one field the writer
//! can never produce.

use anvil_core::domain::join_episode::{
    fold_join_coverage, fold_join_episodes, group_episodes, is_join_relevant, DeliveryEpisode,
    EpisodeGroup, EpisodeGroupKey, EpisodeGrouping, HearthJoinCoverage, HearthJoinInput,
    JoinCoverageReport, JoinEpisodeSet, JoinOptions, TerminalStatus, JOIN_FILTER_VERSION,
};
use anvil_core::domain::playbook::registry::{PlaybookRegistry, PlaybookSource};
use anvil_core::domain::playbook::types::{PlaybookMachine, StateDefinition};
use anvil_core::domain::telemetry_salt::UNKNOWN_KEY_EPOCH;
use anvil_core::domain::usage_timeseries::Granularity;
use anvil_core::ports::activity_log_port::ActivityLogRecord;
use anvil_core::ports::delivery_log_port::{
    project_delivery_record, DeliveryLogRecord, DeliveryObservation,
};
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, Params, StepDef};
use std::collections::BTreeMap;

const S: &str = "je_state";

/// Names a report field that would pool two hearths into one answer. The two
/// hearths measured during planning sit at 32.9% and 64.4% begin-side coverage
/// under different salts; a pooled number would be dominated by whichever is
/// better instrumented and would still be printed to two decimals.
const POOLED_KEY_MARKERS: &[&str] = &["total", "pooled", "overall", "fleet", "across_hearths"];

#[derive(Clone, Default)]
struct Seed {
    label: String,
    salt_epoch: Option<String>,
    delivery: Vec<DeliveryLogRecord>,
    ids: Vec<String>,
    activity: Vec<ActivityLogRecord>,
    read_defects: u64,
    scanned: u64,
}

#[derive(Clone, Default)]
struct Scenario {
    machines: BTreeMap<String, PlaybookMachine>,
    options: JoinOptions,
    hearths: Vec<Seed>,
    set: Option<JoinEpisodeSet>,
    report: Option<JoinCoverageReport>,
    second_set: Option<JoinEpisodeSet>,
    second_report: Option<JoinCoverageReport>,
    groups: Option<Vec<EpisodeGroup>>,
    retained: Option<Vec<ActivityLogRecord>>,
}

struct FixtureRegistry {
    machines: BTreeMap<String, PlaybookMachine>,
}

impl PlaybookRegistry for FixtureRegistry {
    fn machine_for<'a>(&'a self, kind: &str) -> Option<&'a PlaybookMachine>
    where
        Self: 'a,
    {
        self.machines.get(kind)
    }

    fn playbook_id_for(&self, kind: &str) -> Option<String> {
        self.machines.get(kind).map(|_| format!("{}_dir", kind))
    }

    fn source_for(&self, kind: &str) -> Option<PlaybookSource> {
        self.playbook_id_for(kind).map(|playbook_id| PlaybookSource {
            hearth: None,
            playbook_id,
        })
    }
}

fn state(name: &str, is_terminal: bool) -> StateDefinition {
    StateDefinition {
        name: name.to_string(),
        role_filters: Vec::new(),
        registry_section: String::new(),
        projection_targets: Vec::new(),
        is_review_gate: false,
        is_terminal,
        hook: None,
        hooks_by_role: BTreeMap::new(),
        measurement_by_role: BTreeMap::new(),
    }
}

fn sp(params: &Params, index: usize) -> Result<String, String> {
    params
        .get_string(index)
        .map(|s| s.to_string())
        .ok_or_else(|| format!("Expected string parameter {}", index))
}

fn ip(params: &Params, index: usize) -> Result<u64, String> {
    params
        .get_int(index)
        .map(|v| v as u64)
        .ok_or_else(|| format!("Expected integer parameter {}", index))
}

fn eq<T: PartialEq + std::fmt::Debug>(what: String, actual: T, expected: T) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        Err(format!("{}: got {:?}, expected {:?}", what, actual, expected))
    }
}

fn column(table: &DataTable, name: &str) -> Result<usize, String> {
    opt_column(table, name).ok_or_else(|| format!("Missing '{}' column in data table", name))
}

fn opt_column(table: &DataTable, name: &str) -> Option<usize> {
    table.headers.iter().position(|h| h.trim() == name)
}

/// A `-` cell means the empty string. An empty cell reads as an alignment
/// artifact in a rendered table, and the empty string is exactly what several of
/// these scenarios are about.
fn cell(row: &[String], index: Option<usize>, default: &str) -> String {
    match index {
        None => default.to_string(),
        Some(i) => match row[i].trim() {
            "-" => String::new(),
            "" => default.to_string(),
            v => v.to_string(),
        },
    }
}

fn opt_cell(row: &[String], index: Option<usize>) -> Option<String> {
    Some(cell(row, index, "")).filter(|v| !v.is_empty())
}

fn scenario(ctx: &Context) -> Result<Scenario, String> {
    ctx.get::<Scenario>(S)
        .cloned()
        .ok_or_else(|| "No join scenario — the Background never ran".to_string())
}

fn out(sc: Scenario) -> Result<Context, String> {
    Ok(Context::new().with(S, sc))
}

fn last_hearth(sc: &mut Scenario) -> Result<&mut Seed, String> {
    sc.hearths
        .last_mut()
        .ok_or_else(|| "No join hearth has been declared yet".to_string())
}

fn run_fold(sc: &Scenario, options: &JoinOptions) -> (JoinEpisodeSet, JoinCoverageReport) {
    let registry = FixtureRegistry {
        machines: sc.machines.clone(),
    };
    let inputs: Vec<HearthJoinInput<'_>> = sc
        .hearths
        .iter()
        .map(|h| HearthJoinInput {
            hearth_label: h.label.clone(),
            delivery: &h.delivery,
            activity: &h.activity,
            hearth_salt_file_epoch: h.salt_epoch.clone(),
            read_defects: h.read_defects,
            activity_rows_scanned: h.scanned,
        })
        .collect();
    let set = fold_join_episodes(&inputs, &registry, options);
    let report = fold_join_coverage(&set);
    (set, report)
}

fn set_of(sc: &Scenario) -> Result<&JoinEpisodeSet, String> {
    sc.set
        .as_ref()
        .ok_or_else(|| "The join episodes were never folded".to_string())
}

fn report_of(sc: &Scenario) -> Result<&JoinCoverageReport, String> {
    sc.report
        .as_ref()
        .ok_or_else(|| "The coverage report was never folded".to_string())
}

/// Resolve a feature-file delivery-row id to its episode. An id that produced no
/// episode is an error naming that fact, because "the fold dropped the row" is
/// precisely the failure the unjoin buckets exist to prevent.
fn episode(sc: &Scenario, id: &str) -> Result<DeliveryEpisode, String> {
    let set = set_of(sc)?;
    for hearth in &sc.hearths {
        let Some(i) = hearth.ids.iter().position(|rid| rid == id) else {
            continue;
        };
        let record = &hearth.delivery[i];
        let hits: Vec<&DeliveryEpisode> = set
            .episodes
            .iter()
            .filter(|e| {
                e.hearth_label == hearth.label
                    && e.delivery_at == record.at
                    && e.guidance_kind == record.guidance_kind
            })
            .collect();
        return match hits.len() {
            1 => Ok(hits[0].clone()),
            0 => Err(format!(
                "delivery row '{}' produced NO episode — the fold dropped a row instead of \
                 bucketing it",
                id
            )),
            n => Err(format!(
                "delivery row '{}' matches {} episodes; give the seeded rows distinct timestamps",
                id, n
            )),
        };
    }
    Err(format!("no seeded delivery row is named '{}'", id))
}

fn last_transition(sc: &Scenario, id: &str) -> Result<Option<String>, String> {
    Ok(episode(sc, id)?
        .matched_begin
        .ok_or_else(|| format!("episode '{}' has no matched begin", id))?
        .last_transition_at)
}

fn coverage<'a>(sc: &'a Scenario, label: &str) -> Result<&'a HearthJoinCoverage, String> {
    set_of(sc)?
        .per_hearth
        .iter()
        .find(|c| c.hearth_label == label)
        .ok_or_else(|| format!("no per-hearth coverage entry labelled '{}'", label))
}

/// Read a coverage field by its SERIALIZED name, so the assertion also proves
/// the field reaches the wire under the name the feature file uses.
fn coverage_field(entry: &HearthJoinCoverage, path: &str) -> Result<u64, String> {
    let mut value = serde_json::to_value(entry).map_err(|e| e.to_string())?;
    for segment in path.split('.') {
        value = value
            .get(segment)
            .cloned()
            .ok_or_else(|| format!("the coverage entry has no field '{}'", path))?;
    }
    value
        .as_u64()
        .ok_or_else(|| format!("coverage field '{}' is not a count: {}", path, value))
}

fn render_key(key: &EpisodeGroupKey) -> String {
    let mut parts = vec![key.hearth_label.clone()];
    for part in [&key.guidance_kind, &key.period_start, &key.playbook_run_id] {
        if let Some(value) = part {
            parts.push(value.clone());
        }
    }
    parts.join("/")
}

fn groups_of(sc: &Scenario) -> Result<&Vec<EpisodeGroup>, String> {
    sc.groups
        .as_ref()
        .ok_or_else(|| "The episodes were never grouped".to_string())
}

fn rendered_keys(groups: &[EpisodeGroup]) -> Vec<String> {
    groups.iter().map(|g| render_key(&g.key)).collect()
}

fn grouping_of(name: &str) -> Result<EpisodeGrouping, String> {
    Ok(match name {
        "hearth" => EpisodeGrouping::Hearth,
        "hearth_kind" => EpisodeGrouping::HearthKind,
        "hearth_period_day" => EpisodeGrouping::HearthPeriod(Granularity::Day),
        "hearth_period_week" => EpisodeGrouping::HearthPeriod(Granularity::Week),
        "hearth_kind_period_week" => EpisodeGrouping::HearthKindPeriod(Granularity::Week),
        "hearth_run" => EpisodeGrouping::HearthRun,
        other => return Err(format!("unknown grouping '{}'", other)),
    })
}

fn collect_keys(value: &serde_json::Value, into: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, nested) in map {
                into.push(key.clone());
                collect_keys(nested, into);
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|v| collect_keys(v, into)),
        _ => {}
    }
}

fn parse_delivery(table: &DataTable) -> Result<(Vec<DeliveryLogRecord>, Vec<String>), String> {
    let id_col = column(table, "id")?;
    let at_col = column(table, "at")?;
    let hash_col = opt_column(table, "conversation_hash");
    let kind_col = opt_column(table, "guidance_kind");
    let outcome_col = opt_column(table, "outcome");
    let produced_col = opt_column(table, "guidance_produced");
    let root_col = opt_column(table, "project_root");
    let source_col = opt_column(table, "source");
    let resume_col = opt_column(table, "resume_source");
    let candidates_col = opt_column(table, "engine_candidates");
    let pre_col = opt_column(table, "pre_migration");
    let mut records = Vec::new();
    let mut ids = Vec::new();
    for row in &table.rows {
        let (at, hash) = (cell(row, Some(at_col), ""), cell(row, hash_col, ""));
        let (kind, outcome) = (
            cell(row, kind_col, ""),
            cell(row, outcome_col, "guidance_produced"),
        );
        let (root, source) = (
            cell(row, root_col, "/x/sample-project"),
            cell(row, source_col, "claude-code"),
        );
        let resume = cell(row, resume_col, "");
        let mut record = project_delivery_record(&DeliveryObservation {
            at: &at,
            source: &source,
            project_root: &root,
            engine_conversation_hash: &hash,
            guidance_kind: &kind,
            engine_candidates: cell(row, candidates_col, "0")
                .parse()
                .map_err(|e| format!("engine_candidates: {e}"))?,
            guidance_produced: cell(row, produced_col, "true") == "true",
            guidance_bytes: 0,
            outcome: &outcome,
            resume_source: &resume,
            router_cause: "",
        });
        // The one field the writer can never set. Only a row read back from a
        // pre-migration line carries it, so the step does what the reader does.
        record.pre_migration = cell(row, pre_col, "false") == "true";
        records.push(record);
        ids.push(cell(row, Some(id_col), ""));
    }
    Ok((records, ids))
}

fn parse_activity(table: &DataTable) -> Result<Vec<ActivityLogRecord>, String> {
    let command_col = column(table, "command")?;
    let at_col = column(table, "at")?;
    let kind_col = opt_column(table, "artifact_kind");
    let from_col = opt_column(table, "from_state");
    let to_col = opt_column(table, "to_state");
    let hash_col = opt_column(table, "conversation_hash");
    let run_col = opt_column(table, "playbook_run_id");
    let label_col = opt_column(table, "project_label");
    Ok(table
        .rows
        .iter()
        .map(|row| ActivityLogRecord {
            command: cell(row, Some(command_col), ""),
            outcome: "ok".to_string(),
            artifact_kind: cell(row, kind_col, ""),
            from_state: cell(row, from_col, ""),
            to_state: cell(row, to_col, ""),
            actor_hash: None,
            at: cell(row, Some(at_col), ""),
            source: "claude-code".to_string(),
            conversation_hash: opt_cell(row, hash_col),
            project_label: opt_cell(row, label_col),
            playbook_run_id: opt_cell(row, run_col),
            call_state: None,
        })
        .collect())
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "join fixture playbooks:",
            &[],
            &[(S, "Scenario")],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected a data table")?;
                let (kind_col, state_col) = (column(table, "kind")?, column(table, "state")?);
                let terminal_col = column(table, "is_terminal")?;
                let mut machines: BTreeMap<String, PlaybookMachine> = BTreeMap::new();
                for row in &table.rows {
                    let kind = cell(row, Some(kind_col), "");
                    machines
                        .entry(kind.clone())
                        .or_insert_with(|| PlaybookMachine {
                            kind,
                            ..Default::default()
                        })
                        .states
                        .push(state(
                            &cell(row, Some(state_col), ""),
                            cell(row, Some(terminal_col), "false") == "true",
                        ));
                }
                out(Scenario {
                    machines,
                    ..Default::default()
                })
            },
        ),
        step_def(
            "join options window {string} to {string} key epoch {string}",
            &[(S, "Scenario")],
            &[(S, "Scenario")],
            |ctx, params| {
                let mut sc = scenario(&ctx)?;
                sc.options = JoinOptions {
                    window_start: sp(params, 0)?,
                    window_end: sp(params, 1)?,
                    project_label: None,
                    key_epoch: sp(params, 2)?,
                };
                out(sc)
            },
        ),
        step_def(
            "a join hearth {string}",
            &[(S, "Scenario")],
            &[(S, "Scenario")],
            |ctx, params| {
                let mut sc = scenario(&ctx)?;
                sc.hearths.push(Seed {
                    label: sp(params, 0)?,
                    ..Default::default()
                });
                out(sc)
            },
        ),
        step_def(
            "a join hearth {string} with salt file epoch {string}",
            &[(S, "Scenario")],
            &[(S, "Scenario")],
            |ctx, params| {
                let mut sc = scenario(&ctx)?;
                sc.hearths.push(Seed {
                    label: sp(params, 0)?,
                    salt_epoch: Some(sp(params, 1)?),
                    ..Default::default()
                });
                out(sc)
            },
        ),
        step_def(
            "the hearth read metadata is {int} read defects and {int} activity rows scanned",
            &[(S, "Scenario")],
            &[(S, "Scenario")],
            |ctx, params| {
                let mut sc = scenario(&ctx)?;
                let (defects, scanned) = (ip(params, 0)?, ip(params, 1)?);
                let hearth = last_hearth(&mut sc)?;
                hearth.read_defects = defects;
                hearth.scanned = scanned;
                out(sc)
            },
        ),
        step_def(
            "delivery rows:",
            &[(S, "Scenario")],
            &[(S, "Scenario")],
            |ctx, params| {
                let mut sc = scenario(&ctx)?;
                let (records, ids) =
                    parse_delivery(params.data_table().ok_or("Expected a table")?)?;
                let hearth = last_hearth(&mut sc)?;
                hearth.delivery = records;
                hearth.ids = ids;
                out(sc)
            },
        ),
        step_def(
            "activity rows:",
            &[(S, "Scenario")],
            &[(S, "Scenario")],
            |ctx, params| {
                let mut sc = scenario(&ctx)?;
                let records = parse_activity(params.data_table().ok_or("Expected a table")?)?;
                last_hearth(&mut sc)?.activity = records;
                out(sc)
            },
        ),
        step_def(
            "the join episodes are folded",
            &[(S, "Scenario")],
            &[(S, "Scenario")],
            |ctx, _params| {
                let mut sc = scenario(&ctx)?;
                let (set, report) = run_fold(&sc, &sc.options.clone());
                sc.set = Some(set);
                sc.report = Some(report);
                out(sc)
            },
        ),
        step_def(
            "the join episodes are folded again",
            &[(S, "Scenario")],
            &[(S, "Scenario")],
            |ctx, _params| {
                let mut sc = scenario(&ctx)?;
                let (set, report) = run_fold(&sc, &sc.options.clone());
                sc.second_set = Some(set);
                sc.second_report = Some(report);
                out(sc)
            },
        ),
        step_def(
            "the join episodes are folded with an unknown key epoch",
            &[(S, "Scenario")],
            &[(S, "Scenario")],
            |ctx, _params| {
                let mut sc = scenario(&ctx)?;
                let mut options = sc.options.clone();
                options.key_epoch = UNKNOWN_KEY_EPOCH.to_string();
                let (set, report) = run_fold(&sc, &options);
                sc.set = Some(set);
                sc.report = Some(report);
                out(sc)
            },
        ),
        step_def(
            "the episodes are grouped by {string}",
            &[(S, "Scenario")],
            &[(S, "Scenario")],
            |ctx, params| {
                let mut sc = scenario(&ctx)?;
                let grouping = grouping_of(&sp(params, 0)?)?;
                sc.groups = Some(group_episodes(set_of(&sc)?, grouping));
                out(sc)
            },
        ),
        step_def(
            "the activity rows are filtered by is_join_relevant",
            &[(S, "Scenario")],
            &[(S, "Scenario")],
            |ctx, _params| {
                let mut sc = scenario(&ctx)?;
                sc.retained = Some(
                    sc.hearths
                        .iter()
                        .flat_map(|h| h.activity.iter())
                        .filter(|r| is_join_relevant(r))
                        .cloned()
                        .collect(),
                );
                out(sc)
            },
        ),
        check_def(
            "the join set holds {int} episodes and {int} unmatched begins",
            &[(S, "Scenario")],
            |ctx, params| {
                let sc = scenario(&ctx)?;
                let set = set_of(&sc)?;
                eq(
                    "the folded set (episodes, unmatched begins)".into(),
                    (set.episodes.len() as u64, set.unmatched_begins.len() as u64),
                    (ip(params, 0)?, ip(params, 1)?),
                )
            },
        ),
        check_def(
            "the episode {string} joins to begin {string}",
            &[(S, "Scenario")],
            |ctx, params| {
                let sc = scenario(&ctx)?;
                let (id, run) = (sp(params, 0)?, sp(params, 1)?);
                let episode = episode(&sc, &id)?;
                match &episode.matched_begin {
                    Some(m) if m.playbook_run_id == run => Ok(()),
                    Some(m) => Err(format!(
                        "episode '{}' joined to begin '{}', not '{}'",
                        id, m.playbook_run_id, run
                    )),
                    None => Err(format!(
                        "episode '{}' did not join at all — it is unjoined with reason {:?}",
                        id, episode.unjoin_reason
                    )),
                }
            },
        ),
        check_def(
            "the episode {string} is unjoined with reason {string}",
            &[(S, "Scenario")],
            |ctx, params| {
                let sc = scenario(&ctx)?;
                let (id, expected) = (sp(params, 0)?, sp(params, 1)?);
                let episode = episode(&sc, &id)?;
                match &episode.unjoin_reason {
                    Some(reason) => eq(
                        format!("episode '{}' unjoin reason", id),
                        format!("{:?}", reason),
                        expected,
                    ),
                    None => Err(format!(
                        "episode '{}' JOINED (to begin {:?}) — expected it unjoined with {}",
                        id, episode.matched_begin, expected
                    )),
                }
            },
        ),
        check_def(
            "the begin {string} is unjoined with reason {string}",
            &[(S, "Scenario")],
            |ctx, params| {
                let sc = scenario(&ctx)?;
                let (run, expected) = (sp(params, 0)?, sp(params, 1)?);
                let set = set_of(&sc)?;
                let found = set
                    .unmatched_begins
                    .iter()
                    .find(|b| b.playbook_run_id == run)
                    .ok_or_else(|| {
                        format!(
                            "begin '{}' is not among the {} unmatched begin(s) — it either joined \
                             or was never counted",
                            run,
                            set.unmatched_begins.len()
                        )
                    })?;
                eq(
                    format!("begin '{}' unjoin reason", run),
                    format!("{:?}", found.reason),
                    expected,
                )
            },
        ),
        check_def(
            "the episode {string} terminal status is {string}",
            &[(S, "Scenario")],
            |ctx, params| {
                let sc = scenario(&ctx)?;
                let (id, expected) = (sp(params, 0)?, sp(params, 1)?);
                eq(
                    format!("episode '{}' terminal status", id),
                    format!("{:?}", episode(&sc, &id)?.terminal),
                    expected,
                )
            },
        ),
        check_def(
            "the episode {string} last transition at is {string}",
            &[(S, "Scenario")],
            |ctx, params| {
                let sc = scenario(&ctx)?;
                let (id, expected) = (sp(params, 0)?, sp(params, 1)?);
                eq(
                    format!("episode '{}' last transition at", id),
                    last_transition(&sc, &id)?,
                    Some(expected),
                )
            },
        ),
        check_def(
            "the episode {string} has no last transition at",
            &[(S, "Scenario")],
            |ctx, params| {
                let sc = scenario(&ctx)?;
                let id = sp(params, 0)?;
                eq(
                    format!("episode '{}' last transition at", id),
                    last_transition(&sc, &id)?,
                    None,
                )
            },
        ),
        check_def(
            "the last transition is present exactly when the run state is known",
            &[(S, "Scenario")],
            |ctx, _params| {
                let sc = scenario(&ctx)?;
                let mut joined = 0usize;
                for episode in &set_of(&sc)?.episodes {
                    let Some(matched) = &episode.matched_begin else {
                        continue;
                    };
                    joined += 1;
                    let unknown = episode.terminal == TerminalStatus::UnknownRunState;
                    if matched.last_transition_at.is_none() != unknown {
                        return Err(format!(
                            "the iff is broken for run '{}': last_transition_at is {:?} while \
                             terminal is {:?}",
                            matched.playbook_run_id, matched.last_transition_at, episode.terminal
                        ));
                    }
                }
                // Both sides, or it is half a guard.
                if joined < 2 {
                    return Err(format!(
                        "only {} joined episode(s) — an iff asserted over one side is half a guard",
                        joined
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "every episode carries exactly one of matched begin or unjoin reason",
            &[(S, "Scenario")],
            |ctx, _params| {
                let sc = scenario(&ctx)?;
                let set = set_of(&sc)?;
                if set.episodes.is_empty() {
                    return Err("no episodes — the partition assertion is vacuous".into());
                }
                for episode in &set.episodes {
                    if episode.matched_begin.is_some() == episode.unjoin_reason.is_some() {
                        return Err(format!(
                            "episode at {} carries matched_begin={:?} and unjoin_reason={:?} — \
                             exactly one is required",
                            episode.delivery_at, episode.matched_begin, episode.unjoin_reason
                        ));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "the episodes are ordered {string}",
            &[(S, "Scenario")],
            |ctx, params| {
                let sc = scenario(&ctx)?;
                let ids = sp(params, 0)?;
                let expected: Vec<&str> = ids.split(',').map(|s| s.trim()).collect();
                eq(
                    "episode count".into(),
                    set_of(&sc)?.episodes.len(),
                    expected.len(),
                )?;
                for (position, id) in expected.iter().enumerate() {
                    if set_of(&sc)?.episodes[position] != episode(&sc, id)? {
                        return Err(format!(
                            "episode '{}' is not at position {} — the fold re-ordered its input",
                            id,
                            position + 1
                        ));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "the coverage for hearth {string} reports {string} as {int}",
            &[(S, "Scenario")],
            |ctx, params| {
                let sc = scenario(&ctx)?;
                let (label, field) = (sp(params, 0)?, sp(params, 1)?);
                eq(
                    format!("hearth '{}' field '{}'", label, field),
                    coverage_field(coverage(&sc, &label)?, &field)?,
                    ip(params, 2)?,
                )
            },
        ),
        check_def(
            "the coverage for hearth {string} reports window {string} to {string}",
            &[(S, "Scenario")],
            |ctx, params| {
                let sc = scenario(&ctx)?;
                let label = sp(params, 0)?;
                let entry = coverage(&sc, &label)?;
                eq(
                    format!("hearth '{}' window", label),
                    (entry.window_start.clone(), entry.window_end.clone()),
                    (sp(params, 1)?, sp(params, 2)?),
                )
            },
        ),
        check_def(
            "the coverage for hearth {string} reports {string} as text {string}",
            &[(S, "Scenario")],
            |ctx, params| {
                let (sc, label, field) = (scenario(&ctx)?, sp(params, 0)?, sp(params, 1)?);
                let entry = coverage(&sc, &label)?;
                let actual = match field.as_str() {
                    "key_epoch" => entry.key_epoch.clone(),
                    "key_epoch_reconciliation" => {
                        format!("{:?}", entry.key_epoch_reconciliation)
                    }
                    other => return Err(format!("unknown text field '{}'", other)),
                };
                eq(format!("hearth '{}' {}", label, field), actual, sp(params, 2)?)
            },
        ),
        check_def(
            "the report exposes {int} per hearth entries",
            &[(S, "Scenario")],
            |ctx, params| {
                let sc = scenario(&ctx)?;
                let expected = ip(params, 0)?;
                // One entry per INPUT hearth: asserting the count alone would
                // pass a report that invented or merged an entry.
                eq(
                    "(report entries, input hearths)".into(),
                    (
                        report_of(&sc)?.per_hearth.len() as u64,
                        sc.hearths.len() as u64,
                    ),
                    (expected, expected),
                )
            },
        ),
        check_def(
            "the per hearth entries are labelled {string}",
            &[(S, "Scenario")],
            |ctx, params| {
                let sc = scenario(&ctx)?;
                let labels = sp(params, 0)?;
                let actual: Vec<String> = set_of(&sc)?
                    .per_hearth
                    .iter()
                    .map(|c| c.hearth_label.clone())
                    .collect();
                eq(
                    "per_hearth order (the fold must re-sort nothing)".into(),
                    actual,
                    labels.split(',').map(|s| s.trim().to_string()).collect(),
                )
            },
        ),
        check_def(
            "the report filter version is the {string} constant",
            &[(S, "Scenario")],
            |ctx, params| {
                let sc = scenario(&ctx)?;
                let name = sp(params, 0)?;
                let expected = match name.as_str() {
                    "JOIN_FILTER_VERSION" => JOIN_FILTER_VERSION,
                    other => return Err(format!("unknown version constant '{}'", other)),
                };
                eq(
                    format!("(report, set) filter version against {}", name),
                    (
                        report_of(&sc)?.filter_version.as_str(),
                        set_of(&sc)?.filter_version,
                    ),
                    (expected, expected),
                )
            },
        ),
        check_def(
            "the serialized report exposes no pooled coverage key",
            &[(S, "Scenario")],
            |ctx, _params| {
                let sc = scenario(&ctx)?;
                let value = serde_json::to_value(report_of(&sc)?).map_err(|e| e.to_string())?;
                let object = value.as_object().ok_or("the report is not a JSON object")?;
                let mut top: Vec<&String> = object.keys().collect();
                top.sort();
                if top != ["filter_version", "per_hearth"] {
                    return Err(format!(
                        "the report's top-level keys are {:?}; a fleet total may not be added",
                        top
                    ));
                }
                let mut keys = Vec::new();
                collect_keys(&value, &mut keys);
                match keys
                    .iter()
                    .find(|k| POOLED_KEY_MARKERS.iter().any(|m| k.contains(m)))
                {
                    None => Ok(()),
                    Some(key) => Err(format!(
                        "the serialized report exposes '{}', which pools two hearths into one \
                         answer",
                        key
                    )),
                }
            },
        ),
        check_def(
            "the grouping yields {int} groups",
            &[(S, "Scenario")],
            |ctx, params| {
                let sc = scenario(&ctx)?;
                let groups = groups_of(&sc)?;
                eq(
                    format!("group count over {:?}", rendered_keys(groups)),
                    groups.len() as u64,
                    ip(params, 0)?,
                )
            },
        ),
        check_def(
            "the group {string} carries n {int}",
            &[(S, "Scenario")],
            |ctx, params| {
                let sc = scenario(&ctx)?;
                let name = sp(params, 0)?;
                let groups = groups_of(&sc)?;
                let group = groups
                    .iter()
                    .find(|g| render_key(&g.key) == name)
                    .ok_or_else(|| {
                        format!(
                            "no group '{}' — a suppressed cell is indistinguishable from an absent \
                             one, which is why nothing is suppressed. Groups: {:?}",
                            name,
                            rendered_keys(groups)
                        )
                    })?;
                eq(
                    format!("group '{}' (n, indices)", name),
                    (group.n as u64, group.episode_indices.len() as u64),
                    (ip(params, 1)?, ip(params, 1)?),
                )
            },
        ),
        check_def(
            "every reported group carries its own n",
            &[(S, "Scenario")],
            |ctx, _params| {
                let sc = scenario(&ctx)?;
                let groups = groups_of(&sc)?;
                if groups.is_empty() {
                    return Err("no groups — the assertion is vacuous".into());
                }
                for group in groups {
                    if group.n == 0 || group.n != group.episode_indices.len() {
                        return Err(format!(
                            "group '{}' declares n={} over {} episode(s)",
                            render_key(&group.key),
                            group.n,
                            group.episode_indices.len()
                        ));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "the second fold produces an identical set and report",
            &[(S, "Scenario")],
            |ctx, _params| {
                let sc = scenario(&ctx)?;
                let first = set_of(&sc)?;
                let second = sc.second_set.as_ref().ok_or("The second fold never ran")?;
                if first != second {
                    return Err("the two folds produced different episode vectors".into());
                }
                if first.episodes.is_empty() {
                    return Err(
                        "both folds produced nothing — determinism over an empty vector is vacuous"
                            .into(),
                    );
                }
                if sc.report != sc.second_report {
                    return Err("the two folds produced different reports".into());
                }
                Ok(())
            },
        ),
        check_def(
            "the retained activity rows carry commands {string} in order",
            &[(S, "Scenario")],
            |ctx, params| {
                let sc = scenario(&ctx)?;
                let retained = sc.retained.as_ref().ok_or("The rows were never filtered")?;
                let expected = sp(params, 0)?;
                eq(
                    "commands retained by is_join_relevant".into(),
                    retained
                        .iter()
                        .map(|r| r.command.clone())
                        .collect::<Vec<_>>()
                        .join(","),
                    expected,
                )
            },
        ),
    ]
}
