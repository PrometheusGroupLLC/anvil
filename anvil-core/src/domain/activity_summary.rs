//! ActivitySummary read-side fold over the universal activity-log sink.
//!
//! Folds the durable, redacted `activity-log.jsonl` record stream (one record
//! per command turn the engine serves) into the dashboard's usage summary:
//!
//!   - `total_turns`     — every recorded command turn.
//!   - `by_command`      — turn count per command verb.
//!   - `by_route_outcome`— for `route` turns only, the count per route resolution
//!                         outcome ("single" | "candidates" | "no_match"); this is
//!                         how routed-vs-abstained is measured.
//!   - `by_artifact_kind`     — turn count per resolved artifact_kind (records with an
//!                         empty artifact_kind are excluded — they routed/touched
//!                         no kind).
//!   - `buckets`         — per calendar-day (or ISO-week) totals, each carrying
//!                         the period's total turns and its distinct non-None
//!                         actor count.
//!
//! Pure function over an already-read record vector — no filesystem, no ports.
//! The engine reads the sink via the read port and hands the vector here. An
//! empty input yields a zeroed result (never an error): a fresh hearth has no
//! activity yet. Bucketing reuses the same `Granularity` + bucket-key derivation
//! as the usage time series so the two surfaces agree on period boundaries.

use crate::domain::usage_timeseries::Granularity;
use crate::ports::activity_log_port::ActivityLogRecord;
use chrono::{Datelike, NaiveDate};
use std::collections::{BTreeMap, BTreeSet};

/// A label/count pair (command, route-outcome, or artifact_kind). Lists of these
/// are ordered descending by count, then ascending by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelCount {
    pub label: String,
    pub count: u64,
}

/// One time bucket of the activity summary: a period start (ISO date), the total
/// turns in the period, and the distinct non-None actor count for the period.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityBucket {
    /// ISO date (`YYYY-MM-DD`) of the bucket's start — the calendar day for day
    /// granularity, the week's Monday for week granularity.
    pub period_start: String,
    pub total_turns: u64,
    /// Count of DISTINCT `actor_hash` among the records whose `at` falls in this
    /// bucket. Records with `actor_hash = None` are excluded from the set.
    pub distinct_actors: u64,
}

/// The full ActivitySummary fold result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivitySummaryResult {
    pub total_turns: u64,
    /// Turn count per command, descending by count then ascending by command.
    pub by_command: Vec<LabelCount>,
    /// Route-resolution outcome count over `route` turns, descending by count
    /// then ascending by outcome.
    pub by_route_outcome: Vec<LabelCount>,
    /// Turn count per resolved artifact_kind (empty kinds excluded), descending
    /// by count then ascending by kind.
    pub by_artifact_kind: Vec<LabelCount>,
    /// Routed-turn count per originating harness `source` over `route` turns
    /// only — the per-harness routing breakdown. An empty source (old hooks /
    /// old records) is bucketed under [`SOURCE_UNKNOWN`]. Descending by count
    /// then ascending by source.
    pub by_source: Vec<LabelCount>,
    /// Time buckets ordered ascending by period_start.
    pub buckets: Vec<ActivityBucket>,
    /// Distinct conversations (by `conversation_hash`) that fired at least one
    /// `route` turn — the honest adoption DENOMINATOR. Routes fire many times per
    /// session, so this dedups per conversation. Records with no
    /// `conversation_hash` (e.g. pre-fix history, direct calls without a session)
    /// are excluded.
    pub routed_conversations: u64,
    /// Distinct ROUTED conversations that also ENGAGED a playbook — a turn whose
    /// command is one of begin/snapshot/complete/amend (actual playbook use, not
    /// just routing/describe/catalog/checkin). The honest adoption NUMERATOR:
    /// `converted_conversations / routed_conversations` is the conversion rate.
    pub converted_conversations: u64,
    /// Per-call-state count over `route` turns only — the Layer-1 honest-adoption
    /// classification ("no_playbook_run" | "start_opportunity" | "mid_playbook_run"). A
    /// route record lacking the field folds into the [`CALL_STATE_UNKNOWN`]
    /// bucket. Coverage = (began-from-START + mid_playbook_run) ÷ total route turns;
    /// the start-conversion denominator is start_opportunity ONLY (excludes mid).
    /// Descending by count then ascending by call-state.
    pub by_call_state: Vec<LabelCount>,
    /// Total playbook-STEP turns: the count of begin/snapshot/complete/amend
    /// turns across the window — the VOLUME of actual playbook-driving, not a
    /// per-conversation flag. `begin` fires once; the phase steps (snapshot)
    /// fire many times per playbook (a review round-trips repeatedly), so this is
    /// the truest "is anvil being used" signal. `playbook_step_turns /
    /// converted_conversations` is the average DEPTH a routed-and-engaged
    /// conversation drives a playbook.
    pub playbook_step_turns: u64,
}

/// The command verb whose `outcome` feeds `by_route_outcome` and whose `source`
/// feeds `by_source`.
const ROUTE_COMMAND: &str = "route";

/// Command verbs that count as ENGAGING a playbook (actual use), as opposed to
/// merely routing to / inspecting one. A routed conversation "converts" when it
/// also produces one of these turns.
const PLAYBOOK_USE_COMMANDS: &[&str] = &["begin", "snapshot", "complete", "amend"];

/// The `by_source` label for a `route` record whose `source` is empty — an older
/// hook (no `--source`) or a record written before the `source` field existed.
pub const SOURCE_UNKNOWN: &str = "unknown";

/// The `by_call_state` label for a `route` record whose `call_state` is absent —
/// a record written before the field existed.
pub const CALL_STATE_UNKNOWN: &str = "unknown";

/// Derive the bucket key (an ISO date string) for a record's `at` timestamp.
/// Mirrors the usage-time-series bucketer: parses the leading `YYYY-MM-DD`; day
/// granularity returns it verbatim, week granularity returns the Monday of that
/// date's ISO week. A record whose `at` is not a parseable date is skipped.
fn bucket_key(at: &str, granularity: Granularity) -> Option<String> {
    let date_part = at.get(0..10)?;
    let date = NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()?;
    match granularity {
        Granularity::Day => Some(date.format("%Y-%m-%d").to_string()),
        Granularity::Week => {
            let weekday_from_monday = date.weekday().num_days_from_monday();
            let monday = date - chrono::Duration::days(weekday_from_monday as i64);
            Some(monday.format("%Y-%m-%d").to_string())
        }
    }
}

/// Sort a `label -> count` map into a `Vec<LabelCount>` ordered descending by
/// count, then ascending by label.
fn sorted_counts(counts: BTreeMap<String, u64>) -> Vec<LabelCount> {
    let mut out: Vec<LabelCount> = counts
        .into_iter()
        .map(|(label, count)| LabelCount { label, count })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.label.cmp(&b.label)));
    out
}

/// Fold an activity-log record stream into the usage summary.
///
/// `total_turns` counts every record. `by_command` counts per `command`.
/// `by_route_outcome` counts the `outcome` of records whose `command` is
/// `"route"` only. `by_artifact_kind` counts per non-empty `artifact_kind`. Buckets
/// group every record by the period of its (parseable) `at`, carrying the
/// period's total turns and distinct non-None `actor_hash` count. An empty input
/// → a zeroed result.
pub fn fold_activity_summary(
    records: &[ActivityLogRecord],
    granularity: Granularity,
) -> ActivitySummaryResult {
    let mut by_command: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_route_outcome: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_source: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_call_state: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_artifact_kind: BTreeMap<String, u64> = BTreeMap::new();
    // period_start -> total turns
    let mut turns_by_period: BTreeMap<String, u64> = BTreeMap::new();
    // period_start -> set of distinct actor_hash
    let mut actors_by_period: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    // Distinct conversations that routed, and distinct conversations that engaged
    // a playbook. Intersection = converted (honest adoption numerator).
    let mut routed_convos: BTreeSet<String> = BTreeSet::new();
    let mut engaged_convos: BTreeSet<String> = BTreeSet::new();
    // Total playbook-step turns (begin/snapshot/complete/amend) regardless of
    // conversation — the volume of playbook-driving activity.
    let mut playbook_step_turns: u64 = 0;

    let mut total_turns: u64 = 0;
    for record in records {
        total_turns += 1;
        if !record.command.is_empty() {
            *by_command.entry(record.command.clone()).or_insert(0) += 1;
        }
        let is_playbook_step = PLAYBOOK_USE_COMMANDS.contains(&record.command.as_str());
        if is_playbook_step {
            playbook_step_turns += 1;
        }
        if let Some(conv) = &record.conversation_hash {
            if !conv.is_empty() {
                if record.command == ROUTE_COMMAND {
                    routed_convos.insert(conv.clone());
                } else if is_playbook_step {
                    engaged_convos.insert(conv.clone());
                }
            }
        }
        if record.command == ROUTE_COMMAND && !record.outcome.is_empty() {
            *by_route_outcome.entry(record.outcome.clone()).or_insert(0) += 1;
        }
        if record.command == ROUTE_COMMAND {
            // Every route turn is attributed to a harness. An empty source —
            // old hook or pre-`source` record — counts under "unknown" so the
            // total of `by_source` always equals the total of route turns.
            let label = if record.source.is_empty() {
                SOURCE_UNKNOWN.to_string()
            } else {
                record.source.clone()
            };
            *by_source.entry(label).or_insert(0) += 1;
            // Every route turn is classified into a call-state. A record with no
            // `call_state` — written before the field existed — counts under
            // "unknown", so the total of `by_call_state` equals route turns.
            let call_state_label = match &record.call_state {
                Some(s) if !s.is_empty() => s.clone(),
                _ => CALL_STATE_UNKNOWN.to_string(),
            };
            *by_call_state.entry(call_state_label).or_insert(0) += 1;
        }
        if !record.artifact_kind.is_empty() {
            *by_artifact_kind.entry(record.artifact_kind.clone()).or_insert(0) += 1;
        }
        if let Some(key) = bucket_key(&record.at, granularity) {
            *turns_by_period.entry(key.clone()).or_insert(0) += 1;
            if let Some(hash) = &record.actor_hash {
                actors_by_period
                    .entry(key)
                    .or_default()
                    .insert(hash.clone());
            }
        }
    }

    let buckets = turns_by_period
        .into_iter()
        .map(|(period_start, total_turns)| {
            let distinct_actors = actors_by_period
                .get(&period_start)
                .map(|set| set.len() as u64)
                .unwrap_or(0);
            ActivityBucket {
                period_start,
                total_turns,
                distinct_actors,
            }
        })
        .collect();

    ActivitySummaryResult {
        total_turns,
        by_command: sorted_counts(by_command),
        by_route_outcome: sorted_counts(by_route_outcome),
        by_source: sorted_counts(by_source),
        by_artifact_kind: sorted_counts(by_artifact_kind),
        buckets,
        routed_conversations: routed_convos.len() as u64,
        converted_conversations: routed_convos.intersection(&engaged_convos).count() as u64,
        by_call_state: sorted_counts(by_call_state),
        playbook_step_turns,
    }
}

/// Per-kind call counts over the activity log: one count per record with a
/// non-empty `artifact_kind`. This is the SAME fold `fold_activity_summary`
/// applies for `by_artifact_kind`, exposed standalone so the PlaybookActivity owner
/// roll-up can derive its `call_count` from the activity log — making the
/// dashboard's owner totals reconcile with the universal `by_artifact_kind` usage view
/// (instead of the narrower routing-activity sink, which only logs `route` turns).
pub fn artifact_kind_counts(records: &[ActivityLogRecord]) -> BTreeMap<String, u64> {
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    for record in records {
        if !record.artifact_kind.is_empty() {
            *counts.entry(record.artifact_kind.clone()).or_insert(0) += 1;
        }
    }
    counts
}

/// Cross-hearth merge of multiple per-hearth activity-log streams into a single
/// folded summary. Concatenates every hearth's records, then folds the union
/// with [`fold_activity_summary`]. Turn counts SUM across hearths; distinct
/// actors UNION before counting — correct ONLY because the salt is
/// per-deployment, so the same actor hashes identically in every hearth.
pub fn fold_activity_summary_across_hearths(
    streams: &[Vec<ActivityLogRecord>],
    granularity: Granularity,
) -> ActivitySummaryResult {
    let mut all: Vec<ActivityLogRecord> = Vec::new();
    for s in streams {
        all.extend(s.iter().cloned());
    }
    fold_activity_summary(&all, granularity)
}
