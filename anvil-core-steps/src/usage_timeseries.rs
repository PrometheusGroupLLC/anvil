//! Step module for `usage_timeseries.feature` and `playbook_step_volume.feature`.
//!
//! Exercises the pure `fold_usage_timeseries` + `fold_playbook_step_volume`
//! folds over in-memory record streams (the durable sinks' record types). No
//! filesystem: these are pure domain folds, so the steps seed record vectors
//! directly and assert against the folded result.

use anvil_core::domain::usage_timeseries::{
    fold_usage_timeseries, fold_usage_timeseries_across_hearths, fold_playbook_step_volume,
    fold_playbook_step_volume_across_hearths, Granularity, UsageTimeSeriesResult,
    PlaybookStepVolumeResult,
};
use anvil_core::ports::routing_activity_port::RoutingActivityRecord;
use anvil_core::ports::step_measurement_port::StepMeasurementRecord;
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

const ROUTING_KEY: &str = "uts_routing_records";
const STEP_KEY: &str = "uts_step_records";
const ACTOR_STEP_KEY: &str = "uts_actor_step_records";
const TS_RESULT_KEY: &str = "uts_ts_result";
const SV_RESULT_KEY: &str = "uts_sv_result";
// Cross-hearth: two independent per-hearth streams keyed A and B.
const ROUTING_A_KEY: &str = "uts_routing_a";
const ROUTING_B_KEY: &str = "uts_routing_b";
const STEP_A_KEY: &str = "uts_step_a";
const STEP_B_KEY: &str = "uts_step_b";

/// Optional column lookup — returns None when the column is absent so a table
/// can omit the new `actor_hash` / `artifact_kind` columns.
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

/// Parse a step-measurement data table. Columns: kind, from_state, to_state,
/// role, at (required); artifact_kind, actor_hash (optional). When the
/// `artifact_kind` column is ABSENT, the `kind` value is placed in
/// `artifact_kind` (the real, post-fix path the engine writes). When the
/// `artifact_kind` column is PRESENT, its value is used verbatim — including
/// empty, to exercise the legacy `kind`-fallback path.
fn parse_step_records(table: &DataTable) -> Result<Vec<StepMeasurementRecord>, String> {
    let kind_col = column_index(table, "kind")?;
    let from_col = column_index(table, "from_state")?;
    let to_col = column_index(table, "to_state")?;
    let role_col = column_index(table, "role")?;
    let at_col = column_index(table, "at")?;
    let wfk_col = opt_column_index(table, "artifact_kind");
    let hash_col = opt_column_index(table, "actor_hash");
    let mut records = Vec::new();
    for row in &table.rows {
        let kind = row[kind_col].trim().to_string();
        let artifact_kind = match wfk_col {
            Some(i) => row[i].trim().to_string(),
            None => kind.clone(),
        };
        records.push(StepMeasurementRecord {
            kind,
            from_state: row[from_col].trim().to_string(),
            to_state: row[to_col].trim().to_string(),
            role: row[role_col].trim().to_string(),
            intent_present: false,
            expected_output_present: false,
            at: row[at_col].trim().to_string(),
            artifact_kind,
            actor_hash: actor_hash_opt(row, hash_col),
            conversation_hash: None,
            project_label: None,
            playbook_run_id: None,
            evidence: None,
        });
    }
    Ok(records)
}

/// Seed one hearth's combined stream from a table with `kind | outcome | at |
/// actor_hash`. Each row is BOTH a routing-activity record (kind/outcome/at) and
/// a step-measurement record carrying the actor_hash at the same `at` — modeling
/// one actor's call. Empty/"-" actor_hash → None.
fn seed_hearth_stream(
    params: &brine_runner_rust::registry::Params,
    routing_key: &str,
    step_key: &str,
) -> Result<Context, String> {
    let table = params.data_table().ok_or("Expected data table")?;
    let kind_col = column_index(table, "kind")?;
    let outcome_col = column_index(table, "outcome")?;
    let at_col = column_index(table, "at")?;
    let hash_col = opt_column_index(table, "actor_hash");
    let mut routing = Vec::new();
    let mut steps = Vec::new();
    for row in &table.rows {
        let kind = row[kind_col].trim().to_string();
        let at = row[at_col].trim().to_string();
        routing.push(RoutingActivityRecord {
            kind: kind.clone(),
            outcome: row[outcome_col].trim().to_string(),
            at: at.clone(),
            conversation_hash: None,
            project_label: None,
        });
        steps.push(StepMeasurementRecord {
            kind: "step_measurement".to_string(),
            from_state: String::new(),
            to_state: "in_progress".to_string(),
            role: "doer".to_string(),
            intent_present: false,
            expected_output_present: false,
            at,
            artifact_kind: kind,
            actor_hash: actor_hash_opt(row, hash_col),
            conversation_hash: None,
            project_label: None,
            playbook_run_id: None,
            evidence: None,
        });
    }
    let mut out = Context::new();
    out.set(routing_key, routing);
    out.set(step_key, steps);
    Ok(out)
}

fn column_index(table: &DataTable, name: &str) -> Result<usize, String> {
    table
        .headers
        .iter()
        .position(|h| h == name)
        .ok_or_else(|| format!("Missing '{}' column in data table", name))
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ===== UsageTimeSeries =====
        step_def(
            "a usage timeseries record stream:",
            &[],
            &[(ROUTING_KEY, "Vec<RoutingActivityRecord>")],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let kind_col = column_index(table, "kind")?;
                let outcome_col = column_index(table, "outcome")?;
                let at_col = column_index(table, "at")?;
                let mut records = Vec::new();
                for row in &table.rows {
                    records.push(RoutingActivityRecord {
                        kind: row[kind_col].trim().to_string(),
                        outcome: row[outcome_col].trim().to_string(),
                        at: row[at_col].trim().to_string(),
                        conversation_hash: None,
                        project_label: None,
                    });
                }
                let mut out = Context::new();
                out.set(ROUTING_KEY, records);
                Ok(out)
            },
        ),
        step_def(
            "an empty usage timeseries record stream",
            &[],
            &[(ROUTING_KEY, "Vec<RoutingActivityRecord>")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set(ROUTING_KEY, Vec::<RoutingActivityRecord>::new());
                Ok(out)
            },
        ),
        // A distinct-actor step-measurement stream feeding the bucketed
        // distinct-actor count. `actor_hash` empty / "-" → None (excluded).
        // Consumes + re-provides ROUTING_KEY so the routing stream the prior
        // Given set survives into the fold step (brine retains only `provides`).
        step_def(
            "a usage timeseries actor step stream:",
            &[(ROUTING_KEY, "Vec<RoutingActivityRecord>")],
            &[
                (ACTOR_STEP_KEY, "Vec<StepMeasurementRecord>"),
                (ROUTING_KEY, "Vec<RoutingActivityRecord>"),
            ],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let hash_col = opt_column_index(table, "actor_hash");
                let at_col = column_index(table, "at")?;
                let mut records = Vec::new();
                for row in &table.rows {
                    records.push(StepMeasurementRecord {
                        kind: "step_measurement".to_string(),
                        from_state: String::new(),
                        to_state: String::new(),
                        role: "doer".to_string(),
                        intent_present: false,
                        expected_output_present: false,
                        at: row[at_col].trim().to_string(),
                        artifact_kind: String::new(),
                        actor_hash: actor_hash_opt(row, hash_col),
                        conversation_hash: None,
                        project_label: None,
                        playbook_run_id: None,
                        evidence: None,
                    });
                }
                let routing = ctx
                    .get::<Vec<RoutingActivityRecord>>(ROUTING_KEY)
                    .cloned()
                    .unwrap_or_default();
                let mut out = Context::new();
                out.set(ACTOR_STEP_KEY, records);
                out.set(ROUTING_KEY, routing);
                Ok(out)
            },
        ),
        step_def(
            "the usage timeseries is folded by day",
            &[(ROUTING_KEY, "Vec<RoutingActivityRecord>")],
            &[(TS_RESULT_KEY, "UsageTimeSeriesResult")],
            |ctx, _params| {
                let records = ctx
                    .get::<Vec<RoutingActivityRecord>>(ROUTING_KEY)
                    .cloned()
                    .unwrap_or_default();
                let steps = ctx
                    .get::<Vec<StepMeasurementRecord>>(ACTOR_STEP_KEY)
                    .cloned()
                    .unwrap_or_default();
                let result = fold_usage_timeseries(&records, &steps, Granularity::Day);
                let mut out = Context::new();
                out.set(TS_RESULT_KEY, result);
                Ok(out)
            },
        ),
        step_def(
            "the usage timeseries is folded by week",
            &[(ROUTING_KEY, "Vec<RoutingActivityRecord>")],
            &[(TS_RESULT_KEY, "UsageTimeSeriesResult")],
            |ctx, _params| {
                let records = ctx
                    .get::<Vec<RoutingActivityRecord>>(ROUTING_KEY)
                    .cloned()
                    .unwrap_or_default();
                let steps = ctx
                    .get::<Vec<StepMeasurementRecord>>(ACTOR_STEP_KEY)
                    .cloned()
                    .unwrap_or_default();
                let result = fold_usage_timeseries(&records, &steps, Granularity::Week);
                let mut out = Context::new();
                out.set(TS_RESULT_KEY, result);
                Ok(out)
            },
        ),
        check_def(
            "the usage timeseries has {int} buckets",
            &[(TS_RESULT_KEY, "UsageTimeSeriesResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let result = ctx
                    .get::<UsageTimeSeriesResult>(TS_RESULT_KEY)
                    .ok_or("No usage timeseries result")?;
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
            "the usage timeseries bucket {string} has total calls {int}",
            &[(TS_RESULT_KEY, "UsageTimeSeriesResult")],
            |ctx, params| {
                let period = params.get_string(0).ok_or("Expected period")?.to_string();
                let expected = params.get_int(1).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<UsageTimeSeriesResult>(TS_RESULT_KEY)
                    .ok_or("No usage timeseries result")?;
                let bucket = result
                    .buckets
                    .iter()
                    .find(|b| b.period_start == period)
                    .ok_or_else(|| format!("No bucket for period '{}'", period))?;
                if bucket.total_calls == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Bucket '{}': expected total {}, got {}",
                        period, expected, bucket.total_calls
                    ))
                }
            },
        ),
        check_def(
            "the usage timeseries bucket {string} per-playbook kind {string} has call count {int}",
            &[(TS_RESULT_KEY, "UsageTimeSeriesResult")],
            |ctx, params| {
                let period = params.get_string(0).ok_or("Expected period")?.to_string();
                let kind = params.get_string(1).ok_or("Expected kind")?.to_string();
                let expected = params.get_int(2).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<UsageTimeSeriesResult>(TS_RESULT_KEY)
                    .ok_or("No usage timeseries result")?;
                let bucket = result
                    .buckets
                    .iter()
                    .find(|b| b.period_start == period)
                    .ok_or_else(|| format!("No bucket for period '{}'", period))?;
                let entry = bucket
                    .per_artifact_kind
                    .iter()
                    .find(|w| w.kind == kind)
                    .ok_or_else(|| format!("No per-playbook entry for kind '{}'", kind))?;
                if entry.call_count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Bucket '{}' kind '{}': expected {}, got {}",
                        period, kind, expected, entry.call_count
                    ))
                }
            },
        ),
        check_def(
            "the usage timeseries buckets are ordered ascending by period start",
            &[(TS_RESULT_KEY, "UsageTimeSeriesResult")],
            |ctx, _params| {
                let result = ctx
                    .get::<UsageTimeSeriesResult>(TS_RESULT_KEY)
                    .ok_or("No usage timeseries result")?;
                let starts: Vec<String> =
                    result.buckets.iter().map(|b| b.period_start.clone()).collect();
                let mut sorted = starts.clone();
                sorted.sort();
                if starts == sorted {
                    Ok(())
                } else {
                    Err(format!("Buckets not ascending by period start: {:?}", starts))
                }
            },
        ),
        check_def(
            "the usage timeseries bucket {string} per-playbook is ordered ascending by kind",
            &[(TS_RESULT_KEY, "UsageTimeSeriesResult")],
            |ctx, params| {
                let period = params.get_string(0).ok_or("Expected period")?.to_string();
                let result = ctx
                    .get::<UsageTimeSeriesResult>(TS_RESULT_KEY)
                    .ok_or("No usage timeseries result")?;
                let bucket = result
                    .buckets
                    .iter()
                    .find(|b| b.period_start == period)
                    .ok_or_else(|| format!("No bucket for period '{}'", period))?;
                let kinds: Vec<String> =
                    bucket.per_artifact_kind.iter().map(|w| w.kind.clone()).collect();
                let mut sorted = kinds.clone();
                sorted.sort();
                if kinds == sorted {
                    Ok(())
                } else {
                    Err(format!("per_artifact_kind not ascending by kind: {:?}", kinds))
                }
            },
        ),
        check_def(
            "the usage timeseries bucket {string} has distinct actors {int}",
            &[(TS_RESULT_KEY, "UsageTimeSeriesResult")],
            |ctx, params| {
                let period = params.get_string(0).ok_or("Expected period")?.to_string();
                let expected = params.get_int(1).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<UsageTimeSeriesResult>(TS_RESULT_KEY)
                    .ok_or("No usage timeseries result")?;
                let bucket = result
                    .buckets
                    .iter()
                    .find(|b| b.period_start == period)
                    .ok_or_else(|| format!("No bucket for period '{}'", period))?;
                if bucket.distinct_actors == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Bucket '{}': expected distinct actors {}, got {}",
                        period, expected, bucket.distinct_actors
                    ))
                }
            },
        ),
        // ===== Cross-hearth merge (usage timeseries) =====
        step_def(
            "hearth A routing+actor stream:",
            &[],
            &[
                (ROUTING_A_KEY, "Vec<RoutingActivityRecord>"),
                (STEP_A_KEY, "Vec<StepMeasurementRecord>"),
            ],
            |_ctx, params| seed_hearth_stream(params, ROUTING_A_KEY, STEP_A_KEY),
        ),
        step_def(
            "hearth B routing+actor stream:",
            &[
                (ROUTING_A_KEY, "Vec<RoutingActivityRecord>"),
                (STEP_A_KEY, "Vec<StepMeasurementRecord>"),
            ],
            &[
                (ROUTING_B_KEY, "Vec<RoutingActivityRecord>"),
                (STEP_B_KEY, "Vec<StepMeasurementRecord>"),
                (ROUTING_A_KEY, "Vec<RoutingActivityRecord>"),
                (STEP_A_KEY, "Vec<StepMeasurementRecord>"),
            ],
            |ctx, params| {
                let mut out = seed_hearth_stream(params, ROUTING_B_KEY, STEP_B_KEY)?;
                // Carry hearth A forward (brine retains only `provides`).
                let ra = ctx
                    .get::<Vec<RoutingActivityRecord>>(ROUTING_A_KEY)
                    .cloned()
                    .unwrap_or_default();
                let sa = ctx
                    .get::<Vec<StepMeasurementRecord>>(STEP_A_KEY)
                    .cloned()
                    .unwrap_or_default();
                out.set(ROUTING_A_KEY, ra);
                out.set(STEP_A_KEY, sa);
                Ok(out)
            },
        ),
        step_def(
            "the usage timeseries is folded across both hearths by day",
            &[
                (ROUTING_A_KEY, "Vec<RoutingActivityRecord>"),
                (ROUTING_B_KEY, "Vec<RoutingActivityRecord>"),
            ],
            &[(TS_RESULT_KEY, "UsageTimeSeriesResult")],
            |ctx, _params| {
                let ra = ctx
                    .get::<Vec<RoutingActivityRecord>>(ROUTING_A_KEY)
                    .cloned()
                    .unwrap_or_default();
                let rb = ctx
                    .get::<Vec<RoutingActivityRecord>>(ROUTING_B_KEY)
                    .cloned()
                    .unwrap_or_default();
                let sa = ctx
                    .get::<Vec<StepMeasurementRecord>>(STEP_A_KEY)
                    .cloned()
                    .unwrap_or_default();
                let sb = ctx
                    .get::<Vec<StepMeasurementRecord>>(STEP_B_KEY)
                    .cloned()
                    .unwrap_or_default();
                let result = fold_usage_timeseries_across_hearths(
                    &[(ra, sa), (rb, sb)],
                    Granularity::Day,
                );
                let mut out = Context::new();
                out.set(TS_RESULT_KEY, result);
                Ok(out)
            },
        ),
        step_def(
            "the playbook step volume is folded across both hearths for kind {string}",
            &[
                (STEP_A_KEY, "Vec<StepMeasurementRecord>"),
                (STEP_B_KEY, "Vec<StepMeasurementRecord>"),
            ],
            &[(SV_RESULT_KEY, "PlaybookStepVolumeResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let sa = ctx
                    .get::<Vec<StepMeasurementRecord>>(STEP_A_KEY)
                    .cloned()
                    .unwrap_or_default();
                let sb = ctx
                    .get::<Vec<StepMeasurementRecord>>(STEP_B_KEY)
                    .cloned()
                    .unwrap_or_default();
                let result = fold_playbook_step_volume_across_hearths(&[sa, sb], &kind);
                let mut out = Context::new();
                out.set(SV_RESULT_KEY, result);
                Ok(out)
            },
        ),
        // ===== PlaybookStepVolume =====
        step_def(
            "a playbook step volume record stream:",
            &[],
            &[(STEP_KEY, "Vec<StepMeasurementRecord>")],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let records = parse_step_records(table)?;
                let mut out = Context::new();
                out.set(STEP_KEY, records);
                Ok(out)
            },
        ),
        step_def(
            "an empty playbook step volume record stream",
            &[],
            &[(STEP_KEY, "Vec<StepMeasurementRecord>")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set(STEP_KEY, Vec::<StepMeasurementRecord>::new());
                Ok(out)
            },
        ),
        step_def(
            "the playbook step volume is folded for kind {string}",
            &[(STEP_KEY, "Vec<StepMeasurementRecord>")],
            &[(SV_RESULT_KEY, "PlaybookStepVolumeResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let records = ctx
                    .get::<Vec<StepMeasurementRecord>>(STEP_KEY)
                    .cloned()
                    .unwrap_or_default();
                let result = fold_playbook_step_volume(&records, &kind);
                let mut out = Context::new();
                out.set(SV_RESULT_KEY, result);
                Ok(out)
            },
        ),
        check_def(
            "the playbook step volume has {int} steps",
            &[(SV_RESULT_KEY, "PlaybookStepVolumeResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let result = ctx
                    .get::<PlaybookStepVolumeResult>(SV_RESULT_KEY)
                    .ok_or("No playbook step volume result")?;
                if result.steps.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} steps, got {}",
                        expected,
                        result.steps.len()
                    ))
                }
            },
        ),
        check_def(
            "the playbook step volume step from {string} to {string} role {string} has call count {int}",
            &[(SV_RESULT_KEY, "PlaybookStepVolumeResult")],
            |ctx, params| {
                let from = params.get_string(0).ok_or("Expected from")?.to_string();
                let to = params.get_string(1).ok_or("Expected to")?.to_string();
                let role = params.get_string(2).ok_or("Expected role")?.to_string();
                let expected = params.get_int(3).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<PlaybookStepVolumeResult>(SV_RESULT_KEY)
                    .ok_or("No playbook step volume result")?;
                let step = result
                    .steps
                    .iter()
                    .find(|s| s.from_state == from && s.to_state == to && s.role == role)
                    .ok_or_else(|| {
                        format!("No step ({} -> {}, {})", from, to, role)
                    })?;
                if step.call_count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Step ({} -> {}, {}): expected {}, got {}",
                        from, to, role, expected, step.call_count
                    ))
                }
            },
        ),
        check_def(
            "the playbook step volume steps are ordered by descending call count",
            &[(SV_RESULT_KEY, "PlaybookStepVolumeResult")],
            |ctx, _params| {
                let result = ctx
                    .get::<PlaybookStepVolumeResult>(SV_RESULT_KEY)
                    .ok_or("No playbook step volume result")?;
                let counts: Vec<u64> = result.steps.iter().map(|s| s.call_count).collect();
                let descending = counts.windows(2).all(|w| w[0] >= w[1]);
                if descending {
                    Ok(())
                } else {
                    Err(format!("Steps not ordered by descending count: {:?}", counts))
                }
            },
        ),
    ]
}
