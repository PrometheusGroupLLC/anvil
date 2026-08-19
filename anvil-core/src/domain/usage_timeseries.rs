//! UsageTimeSeries + PlaybookStepVolume read-side folds.
//!
//! Two pure aggregations over the durable, redacted append-only sinks that back
//! the dashboard's call-volume views:
//!
//!   1. [`fold_usage_timeseries`] folds the routing-activity record stream into
//!      time buckets (per calendar day or ISO week of each record's `at`),
//!      carrying the total call count and a per-playbook breakdown within each
//!      bucket. This is the SAME source the cumulative `PlaybookActivity` query
//!      folds (`routing-activity.jsonl`), bucketed by time instead of summed.
//!
//!   2. [`fold_playbook_step_volume`] folds the step-measurement record stream,
//!      filtered to a single playbook `kind`, into per-step call counts grouped
//!      by (from_state, to_state, role).
//!
//! Both are pure functions over already-read record vectors — no filesystem, no
//! ports. The engine reads the sinks via the existing read ports and hands the
//! vectors here. An empty input stream yields an empty result (never an error):
//! a fresh hearth has no routing/step records yet, and the dashboard renders
//! "no activity" from empty buckets.

use crate::ports::activity_log_port::ActivityLogRecord;
use crate::ports::routing_activity_port::RoutingActivityRecord;
use crate::ports::step_measurement_port::StepMeasurementRecord;
use chrono::{Datelike, NaiveDate};
use std::collections::{BTreeMap, BTreeSet};

/// Bucketing granularity for the usage time series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Granularity {
    /// One bucket per calendar day (UTC), keyed by the `YYYY-MM-DD` date.
    Day,
    /// One bucket per ISO-8601 week, keyed by the `YYYY-MM-DD` date of that
    /// week's Monday.
    Week,
}

impl Granularity {
    /// Parse the wire string. Empty or unrecognized → [`Granularity::Day`]
    /// (the documented default), so the caller never has to special-case absence.
    pub fn parse(s: &str) -> Granularity {
        match s {
            "week" => Granularity::Week,
            _ => Granularity::Day,
        }
    }
}

/// Per-playbook call count within a single time bucket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactKindPeriodCount {
    pub kind: String,
    pub call_count: u64,
}

/// One time bucket: a period start (ISO date), the total calls in the period,
/// and the per-playbook breakdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeBucket {
    /// ISO date (`YYYY-MM-DD`) of the bucket's start — the calendar day for
    /// day granularity, the week's Monday for week granularity.
    pub period_start: String,
    pub total_calls: u64,
    /// Count of `begin` command turns whose `at` falls in this bucket. Zero for
    /// folds that do not include the universal activity log.
    pub begin_count: u64,
    /// Count of `complete` command turns whose `at` falls in this bucket. Zero
    /// for folds that do not include the universal activity log.
    pub complete_count: u64,
    /// Count of DISTINCT `actor_hash` among step-measurement records whose `at`
    /// falls in this bucket. Records with `actor_hash = None` are excluded from
    /// the distinct set.
    pub distinct_actors: u64,
    /// Per-artifact-kind breakdown, ordered ascending by kind.
    pub per_artifact_kind: Vec<ArtifactKindPeriodCount>,
}

/// The full UsageTimeSeries result: buckets ordered ascending by period_start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageTimeSeriesResult {
    pub buckets: Vec<TimeBucket>,
}

/// Per-step call count within a single playbook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepCount {
    pub from_state: String,
    pub to_state: String,
    pub role: String,
    pub call_count: u64,
}

/// The full PlaybookStepVolume result: steps for one kind, ordered by descending
/// call_count then ascending from_state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybookStepVolumeResult {
    pub kind: String,
    pub steps: Vec<StepCount>,
}

/// Derive the bucket key (an ISO date string) for a record's `at` timestamp.
///
/// Parses the leading `YYYY-MM-DD` of an ISO-8601 timestamp. Day granularity
/// returns that date verbatim; week granularity returns the Monday of that
/// date's ISO week. A record whose `at` does not begin with a parseable date is
/// skipped (returns `None`) — a malformed timestamp must not crash the fold.
pub fn bucket_key(at: &str, granularity: Granularity) -> Option<String> {
    // ISO-8601 timestamps begin with `YYYY-MM-DD`; take the first 10 chars.
    let date_part = at.get(0..10)?;
    let date = NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()?;
    match granularity {
        Granularity::Day => Some(date.format("%Y-%m-%d").to_string()),
        Granularity::Week => {
            // Monday of the ISO week containing `date`.
            let weekday_from_monday = date.weekday().num_days_from_monday();
            let monday = date - chrono::Duration::days(weekday_from_monday as i64);
            Some(monday.format("%Y-%m-%d").to_string())
        }
    }
}

/// Fold a routing-activity record stream into time buckets.
///
/// Each record with a non-empty `kind` and a parseable `at` contributes one call
/// to its period bucket and to that period's per-kind breakdown. Records with an
/// empty `kind` (e.g. a no-match outcome) are excluded — they did not call a
/// playbook. Buckets are ordered ascending by period_start; within a bucket the
/// per-playbook breakdown is ordered ascending by kind. An empty input → empty
/// buckets.
pub fn fold_usage_timeseries(
    records: &[RoutingActivityRecord],
    step_records: &[StepMeasurementRecord],
    granularity: Granularity,
) -> UsageTimeSeriesResult {
    // period_start -> (kind -> count)
    let mut by_period: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    for record in records {
        if record.kind.is_empty() {
            continue;
        }
        let Some(key) = bucket_key(&record.at, granularity) else {
            continue;
        };
        *by_period
            .entry(key)
            .or_default()
            .entry(record.kind.clone())
            .or_insert(0) += 1;
    }

    // period_start -> set of distinct actor_hash. A step-measurement record with
    // `actor_hash = None` (no salt, fail-safe) is excluded from the set. The set
    // is keyed independently of the call-volume map so an actor active in a
    // period with no routing record still yields its own bucket (with zero
    // calls). Cross-hearth correctness relies on a per-deployment salt: the same
    // actor hashes identically everywhere, so unioning sets dedups correctly.
    let mut actors_by_period: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for record in step_records {
        let Some(hash) = &record.actor_hash else {
            continue;
        };
        let Some(key) = bucket_key(&record.at, granularity) else {
            continue;
        };
        actors_by_period
            .entry(key)
            .or_default()
            .insert(hash.clone());
    }

    // Union the period keys from both maps so a bucket appears if it has EITHER
    // calls or distinct actors.
    let mut period_keys: BTreeSet<String> = BTreeSet::new();
    period_keys.extend(by_period.keys().cloned());
    period_keys.extend(actors_by_period.keys().cloned());

    let buckets = period_keys
        .into_iter()
        .map(|period_start| {
            let kind_counts = by_period.remove(&period_start).unwrap_or_default();
            let total_calls = kind_counts.values().sum();
            let per_artifact_kind = kind_counts
                .into_iter()
                .map(|(kind, call_count)| ArtifactKindPeriodCount { kind, call_count })
                .collect();
            let distinct_actors = actors_by_period
                .get(&period_start)
                .map(|set| set.len() as u64)
                .unwrap_or(0);
            TimeBucket {
                period_start,
                total_calls,
                begin_count: 0,
                complete_count: 0,
                distinct_actors,
                per_artifact_kind,
            }
        })
        .collect();

    UsageTimeSeriesResult { buckets }
}

/// Cross-hearth merge of multiple per-hearth `(routing, step)` record streams
/// into a single folded time series.
///
/// Concatenates every hearth's routing-activity and step-measurement records,
/// then folds the union with [`fold_usage_timeseries`]. Call counts SUM across
/// hearths (a `lore_query` call in hearth A and another in hearth B is two
/// calls in the merged period). Distinct actors UNION across hearths before
/// counting — correct ONLY because the salt is per-deployment, so the same
/// actor hashes to the same `actor_hash` in every hearth.
pub fn fold_usage_timeseries_across_hearths(
    streams: &[(Vec<RoutingActivityRecord>, Vec<StepMeasurementRecord>)],
    granularity: Granularity,
) -> UsageTimeSeriesResult {
    let mut routing: Vec<RoutingActivityRecord> = Vec::new();
    let mut steps: Vec<StepMeasurementRecord> = Vec::new();
    for (r, s) in streams {
        routing.extend(r.iter().cloned());
        steps.extend(s.iter().cloned());
    }
    fold_usage_timeseries(&routing, &steps, granularity)
}

/// Fold the UNIVERSAL ACTIVITY LOG into time buckets — the ALL-TURNS denominator.
///
/// Unlike [`fold_usage_timeseries`] (which folds the routing-activity sink and
/// counts only playbook-attributed `route` calls), this counts EVERY command
/// turn so the over-time / per-hearth views reconcile with `activity_summary`'s
/// total (begin/checkin/snapshot/complete/route/catalog/describe). Per bucket:
///   - `total_calls` = every record in the period (the headline denominator);
///   - `begin_count` = `begin` records in the period;
///   - `complete_count` = `complete` records in the period;
///   - `per_artifact_kind` = records with a non-empty `workflow_kind` (the subset);
///   - `distinct_actors` = distinct `actor_hash` (None excluded).
/// Buckets ascending by period_start; per_artifact_kind ascending by kind. Empty in →
/// empty buckets.
pub fn fold_usage_timeseries_from_activity_log(
    records: &[ActivityLogRecord],
    granularity: Granularity,
) -> UsageTimeSeriesResult {
    // period -> total turns
    let mut totals: BTreeMap<String, u64> = BTreeMap::new();
    // period -> begin turns
    let mut begin_counts: BTreeMap<String, u64> = BTreeMap::new();
    // period -> complete turns
    let mut complete_counts: BTreeMap<String, u64> = BTreeMap::new();
    // period -> (kind -> count)
    let mut by_period_kind: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    // period -> distinct actor_hash
    let mut actors_by_period: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for record in records {
        let Some(key) = bucket_key(&record.at, granularity) else {
            continue;
        };
        *totals.entry(key.clone()).or_insert(0) += 1;
        match record.command.as_str() {
            "begin" => *begin_counts.entry(key.clone()).or_insert(0) += 1,
            "complete" => *complete_counts.entry(key.clone()).or_insert(0) += 1,
            _ => {}
        }
        if !record.artifact_kind.is_empty() {
            *by_period_kind
                .entry(key.clone())
                .or_default()
                .entry(record.artifact_kind.clone())
                .or_insert(0) += 1;
        }
        if let Some(hash) = &record.actor_hash {
            actors_by_period
                .entry(key)
                .or_default()
                .insert(hash.clone());
        }
    }

    // A bucket appears if it has either turns or distinct actors.
    let mut period_keys: BTreeSet<String> = BTreeSet::new();
    period_keys.extend(totals.keys().cloned());
    period_keys.extend(actors_by_period.keys().cloned());

    let buckets = period_keys
        .into_iter()
        .map(|period_start| {
            let total_calls = totals.get(&period_start).copied().unwrap_or(0);
            let begin_count = begin_counts.get(&period_start).copied().unwrap_or(0);
            let complete_count = complete_counts.get(&period_start).copied().unwrap_or(0);
            let per_artifact_kind = by_period_kind
                .remove(&period_start)
                .unwrap_or_default()
                .into_iter()
                .map(|(kind, call_count)| ArtifactKindPeriodCount { kind, call_count })
                .collect();
            let distinct_actors = actors_by_period
                .get(&period_start)
                .map(|set| set.len() as u64)
                .unwrap_or(0);
            TimeBucket {
                period_start,
                total_calls,
                begin_count,
                complete_count,
                distinct_actors,
                per_artifact_kind,
            }
        })
        .collect();

    UsageTimeSeriesResult { buckets }
}

/// Cross-hearth merge of multiple per-hearth activity-log streams into one folded
/// time series (concatenate, then [`fold_usage_timeseries_from_activity_log`]).
/// total_calls SUM across hearths; distinct actors UNION (per-deployment salt).
pub fn fold_usage_timeseries_from_activity_log_across_hearths(
    streams: &[Vec<ActivityLogRecord>],
    granularity: Granularity,
) -> UsageTimeSeriesResult {
    let mut all: Vec<ActivityLogRecord> = Vec::new();
    for s in streams {
        all.extend(s.iter().cloned());
    }
    fold_usage_timeseries_from_activity_log(&all, granularity)
}

/// Fold a step-measurement record stream, filtered to `kind`, into per-step
/// counts grouped by (from_state, to_state, role).
///
/// Records whose `kind` does not match are excluded. The result's `steps` are
/// ordered by descending call_count, then ascending from_state (then to_state,
/// then role) for a stable tie-break. An unknown/zero-record kind → empty steps.
pub fn fold_playbook_step_volume(
    records: &[StepMeasurementRecord],
    kind: &str,
) -> PlaybookStepVolumeResult {
    // (from_state, to_state, role) -> count
    let mut counts: BTreeMap<(String, String, String), u64> = BTreeMap::new();
    for record in records {
        // Filter on the REAL playbook kind. Legacy records lack `workflow_kind`
        // (empty) — fall back to the `kind` field so historical step
        // measurements (which stored the playbook kind in `kind`) still match.
        let record_kind = if record.artifact_kind.is_empty() {
            &record.kind
        } else {
            &record.artifact_kind
        };
        if record_kind != kind {
            continue;
        }
        *counts
            .entry((
                record.from_state.clone(),
                record.to_state.clone(),
                record.role.clone(),
            ))
            .or_insert(0) += 1;
    }

    let mut steps: Vec<StepCount> = counts
        .into_iter()
        .map(|((from_state, to_state, role), call_count)| StepCount {
            from_state,
            to_state,
            role,
            call_count,
        })
        .collect();

    // Descending call_count, then ascending (from_state, to_state, role).
    steps.sort_by(|a, b| {
        b.call_count
            .cmp(&a.call_count)
            .then_with(|| a.from_state.cmp(&b.from_state))
            .then_with(|| a.to_state.cmp(&b.to_state))
            .then_with(|| a.role.cmp(&b.role))
    });

    PlaybookStepVolumeResult {
        kind: kind.to_string(),
        steps,
    }
}

/// Cross-hearth merge of multiple per-hearth step-measurement streams into a
/// single per-step volume for one `kind`. Concatenates every hearth's records,
/// then folds with [`fold_playbook_step_volume`] so per-step counts SUM across
/// hearths.
pub fn fold_playbook_step_volume_across_hearths(
    streams: &[Vec<StepMeasurementRecord>],
    kind: &str,
) -> PlaybookStepVolumeResult {
    let mut all: Vec<StepMeasurementRecord> = Vec::new();
    for s in streams {
        all.extend(s.iter().cloned());
    }
    fold_playbook_step_volume(&all, kind)
}
