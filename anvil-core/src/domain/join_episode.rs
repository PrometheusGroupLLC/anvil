//! The joined-episode fold: a delivery row that carried a guidance kind, paired
//! with the begin of that kind in the same conversation and the same hearth.
//!
//! This is the unit three other tracks consume. The coverage report is its
//! first consumer, not its only output.
//!
//! ## Pure, and a read-only registry is the single exception
//!
//! No filesystem, no I/O, no write port. The caller reads both sinks through
//! ports and hands the vectors in; the two key-epoch values are resolved by the
//! ENGINE and handed in, so the salt never reaches `anvil-core`.
//!
//! [`PlaybookRegistry`] is here for one reason: terminality is PER KIND. The
//! flat terminal-state list in `domain/mod.rs` holds `"completed"`, while the
//! `track` machine declares `completed` with `is_terminal: false` — so a
//! flat-list implementation folds green on synthesized vectors and mislabels
//! every track run on the real hearth. Terminality is [`state_is_terminal`] and
//! nothing else; this module names that list nowhere, by rule.
//!
//! ## Unjoinable rows are COUNTED, never dropped
//!
//! Every bucket carrying "no", "absent", "pre" or "superseded" in its name is a
//! reported count inside its denominator. A sentinel row and a pre-migration row
//! are episodes with a declared reason, not absences — a report that improves
//! its coverage number by excluding its own rows is the exact failure this fold
//! exists to make impossible.
//!
//! ## Nothing pools, and the fold re-sorts nothing
//!
//! `per_hearth` is a list in INPUT order and there is no fleet total. The engine
//! owns the ordering (canonical-path ascending); a fold that re-sorted would
//! make that ordering unfixable at its own seam.

use crate::domain::playbook::registry::PlaybookRegistry;
use crate::domain::route::state_is_terminal;
use crate::domain::telemetry_salt::{UNKNOWN_CONVERSATION_HASH, UNKNOWN_KEY_EPOCH};
use crate::domain::usage_timeseries::{bucket_key, Granularity};
use crate::ports::activity_log_port::ActivityLogRecord;
use crate::ports::delivery_log_port::DeliveryLogRecord;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// Versions the JOIN — the match rule, the unjoin taxonomy, the terminality
/// source, the episode denominator and the exclusion set. Consumers cite this
/// constant verbatim and declare no second version of the join.
pub const JOIN_FILTER_VERSION: &str = "join-episode-v1";

/// Outcome literals meaning the engine was never successfully asked. Both the
/// live spelling and the legacy one: `engine_call_failed` is no longer emitted
/// but is already on disk in the hundreds, and omitting it would make two
/// instruments disagree by exactly those rows while each reports a closed
/// partition. Deliberately NOT `engine_call_failed_unclassified`, which is a
/// DELIVERED no-single-kind row and belongs to the opposite bucket — one
/// literal is a prefix of the other.
const NO_ENGINE_ANSWER_OUTCOMES: &[&str] = &[
    "engine_unreachable",
    "engine_timeout",
    "engine_rpc_error",
    "engine_call_failed",
];

/// One hearth's already-read rows. The caller measured `read_defects` and
/// `activity_rows_scanned` while reading; a pure fold cannot re-derive either
/// from the retained slices, and a report may not carry a number nobody
/// measured.
pub struct HearthJoinInput<'a> {
    pub hearth_label: String,
    pub delivery: &'a [DeliveryLogRecord],
    pub activity: &'a [ActivityLogRecord],
    pub hearth_salt_file_epoch: Option<String>,
    pub read_defects: u64,
    pub activity_rows_scanned: u64,
}

/// RFC3339 bounds, `window_start` inclusive and `window_end` exclusive; empty
/// means unbounded. `key_epoch` is the fingerprint of the DEPLOYMENT salt, one
/// per report.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JoinOptions {
    pub window_start: String,
    pub window_end: String,
    pub project_label: Option<String>,
    pub key_epoch: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TerminalStatus {
    NotJoined,
    NotYetTerminal,
    ReachedTerminal,
    UnknownRunState,
}

/// Causes, not one bucket. Two deliveries against one begin is a DIFFERENT fact
/// from a delivery nobody acted on, and collapsing them would make
/// double-suggestion read as non-adoption.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum UnjoinReason {
    NoConversationKey,
    PreMigrationRow,
    ConversationAbsentFromBeginSide,
    NoBeginOfKindInConversation,
    SupersededByLaterDeliveryOfKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum BeginUnjoinReason {
    NoConversationKey,
    ConversationAbsentFromDeliverySide,
    NoPriorDeliveryOfKind,
    NoUnconsumedPriorDeliveryOfKind,
}

/// Four states and never a boolean: `NoLocalEvidence` is NOT "no conflict".
/// Most hearths hold no salt file at all, and a `false` there would read as
/// "reconciled" when nothing was reconciled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum KeyEpochReconciliation {
    NoEffectiveSalt,
    NoLocalEvidence,
    MatchesEffective,
    DiffersFromEffective,
}

/// `last_transition_at` is `None` if and only if `terminal == UnknownRunState`.
/// A run that began and never moved carries `Some(begin_at)`, because the begin
/// row IS a run-bearing transition record — reporting it absent would push every
/// never-moved run into the unknown bucket and empty the stalled bucket of
/// exactly the runs it exists to find.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchedBegin {
    pub playbook_run_id: String,
    pub begin_at: String,
    pub artifact_kind: String,
    pub last_transition_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryEpisode {
    pub hearth_label: String,
    pub conversation_hash: String,
    pub guidance_kind: String,
    pub delivery_at: String,
    pub source: String,
    pub project_label: String,
    pub delivery_outcome: String,
    pub resume_source: String,
    pub matched_begin: Option<MatchedBegin>,
    pub terminal: TerminalStatus,
    /// `None` exactly when `matched_begin` is `Some`.
    pub unjoin_reason: Option<UnjoinReason>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmatchedBegin {
    pub hearth_label: String,
    pub conversation_hash: String,
    pub artifact_kind: String,
    pub playbook_run_id: String,
    pub begin_at: String,
    pub reason: BeginUnjoinReason,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct UnjoinCounts {
    pub no_conversation_key: u64,
    pub pre_migration_row: u64,
    pub conversation_absent_from_begin_side: u64,
    pub no_begin_of_kind_in_conversation: u64,
    pub superseded_by_later_delivery_of_kind: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct BeginUnjoinCounts {
    pub no_conversation_key: u64,
    pub conversation_absent_from_delivery_side: u64,
    pub no_prior_delivery_of_kind: u64,
    pub no_unconsumed_prior_delivery_of_kind: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TerminalCounts {
    pub not_joined: u64,
    pub not_yet_terminal: u64,
    pub reached_terminal: u64,
    pub unknown_run_state: u64,
}

/// One hearth's counts. There is no sibling holding a fleet total and none may
/// be added.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HearthJoinCoverage {
    pub hearth_label: String,
    pub window_start: String,
    pub window_end: String,
    pub delivery_rows_read: u64,
    pub read_defects: u64,
    pub activity_rows_scanned: u64,
    pub activity_rows_retained: u64,
    pub begin_rows_read: u64,
    /// Delivery rows in the window carrying a NON-EMPTY `guidance_kind`.
    pub episode_denominator: u64,
    pub begin_denominator: u64,
    /// Excluded from the episode denominator and reported beside it.
    pub menu_delivered: u64,
    pub nothing_delivered: u64,
    pub no_engine_answer: u64,
    pub joined: u64,
    pub unjoin: UnjoinCounts,
    pub begin_unjoin: BeginUnjoinCounts,
    pub terminal: TerminalCounts,
    pub key_epoch: String,
    pub hearth_salt_file_epoch: Option<String>,
    pub key_epoch_reconciliation: KeyEpochReconciliation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinEpisodeSet {
    pub episodes: Vec<DeliveryEpisode>,
    pub unmatched_begins: Vec<UnmatchedBegin>,
    pub per_hearth: Vec<HearthJoinCoverage>,
    pub filter_version: &'static str,
}

/// The serializable report. `per_hearth` is a list; there is no `total_coverage`
/// key and none may be added.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JoinCoverageReport {
    pub per_hearth: Vec<HearthJoinCoverage>,
    pub filter_version: String,
}

/// The reduce-at-ingest predicate. Passed as an ARGUMENT to
/// `read_activity_log_where`, never baked into the reader — other consumers pass
/// their own supersets and their own reducers through the same single parser.
pub fn is_join_relevant(record: &ActivityLogRecord) -> bool {
    record.command == "begin"
        || (!record.to_state.is_empty()
            && record
                .playbook_run_id
                .as_deref()
                .is_some_and(|id| !id.is_empty()))
}

fn in_window(at: &str, options: &JoinOptions) -> bool {
    (options.window_start.is_empty() || at >= options.window_start.as_str())
        && (options.window_end.is_empty() || at < options.window_end.as_str())
}

fn reconcile(key_epoch: &str, local: Option<&str>) -> KeyEpochReconciliation {
    // Evaluated in variant order: an absent deployment salt is decided before
    // local evidence is even consulted.
    if key_epoch.is_empty() || key_epoch == UNKNOWN_KEY_EPOCH {
        KeyEpochReconciliation::NoEffectiveSalt
    } else {
        match local {
            None => KeyEpochReconciliation::NoLocalEvidence,
            Some(l) if l == key_epoch => KeyEpochReconciliation::MatchesEffective,
            Some(_) => KeyEpochReconciliation::DiffersFromEffective,
        }
    }
}

/// The record that supplies BOTH `terminal` and `last_transition_at` — one
/// selection, two outputs, so the two can never describe different transitions.
/// Candidates come from this hearth's own slice, which is what keeps a
/// `playbook_run_id` (not globally unique) from crossing hearths. Transitions
/// are NOT window-filtered: whether the run reached a terminal state is a fact
/// about the run, not about the reporting window.
fn select_transition(
    activity: &[ActivityLogRecord],
    run_id: &str,
    kind: &str,
    registry: &dyn PlaybookRegistry,
) -> (Option<String>, TerminalStatus) {
    if run_id.is_empty() {
        return (None, TerminalStatus::UnknownRunState);
    }
    let mut best: Option<&ActivityLogRecord> = None;
    for record in activity {
        if record.to_state.is_empty() {
            continue;
        }
        if record.playbook_run_id.as_deref().unwrap_or("") != run_id {
            continue;
        }
        best = match best {
            None => Some(record),
            Some(current) if record.at >= current.at => Some(record),
            keep => keep,
        };
    }
    match best {
        None => (None, TerminalStatus::UnknownRunState),
        Some(record) => (
            Some(record.at.clone()),
            if state_is_terminal(registry, kind, &record.to_state) {
                TerminalStatus::ReachedTerminal
            } else {
                TerminalStatus::NotYetTerminal
            },
        ),
    }
}

/// Fold already-read rows into the joined-episode vector and its per-hearth
/// counts. Input order is output order.
pub fn fold_join_episodes(
    inputs: &[HearthJoinInput<'_>],
    registry: &dyn PlaybookRegistry,
    options: &JoinOptions,
) -> JoinEpisodeSet {
    let mut episodes = Vec::new();
    let mut unmatched_begins = Vec::new();
    let mut per_hearth = Vec::new();
    for input in inputs {
        let (eps, begins, coverage) = fold_one_hearth(input, registry, options);
        episodes.extend(eps);
        unmatched_begins.extend(begins);
        per_hearth.push(coverage);
    }
    JoinEpisodeSet {
        episodes,
        unmatched_begins,
        per_hearth,
        filter_version: JOIN_FILTER_VERSION,
    }
}

#[allow(clippy::type_complexity)]
fn fold_one_hearth(
    input: &HearthJoinInput<'_>,
    registry: &dyn PlaybookRegistry,
    options: &JoinOptions,
) -> (Vec<DeliveryEpisode>, Vec<UnmatchedBegin>, HearthJoinCoverage) {
    let project_filter = options.project_label.as_deref().filter(|p| !p.is_empty());
    let mut menu_delivered = 0u64;
    let mut nothing_delivered = 0u64;
    let mut no_engine_answer = 0u64;

    // ── episodes, in delivery append order ─────────────────────────────────
    let mut eps: Vec<DeliveryEpisode> = Vec::new();
    // Parallel to `eps`: the read-side flag that separates a row written before
    // the instrument from a row whose engine could not answer. Both carry the
    // sentinel; they are different facts and different buckets.
    let mut pre_migration: Vec<bool> = Vec::new();
    for record in input.delivery {
        if !in_window(&record.at, options) {
            continue;
        }
        if project_filter.is_some_and(|p| p != record.project_label) {
            continue;
        }
        if record.guidance_kind.is_empty() {
            if NO_ENGINE_ANSWER_OUTCOMES.contains(&record.outcome.as_str()) {
                no_engine_answer += 1;
            } else if record.guidance_produced {
                menu_delivered += 1;
            } else {
                nothing_delivered += 1;
            }
            continue;
        }
        eps.push(DeliveryEpisode {
            hearth_label: input.hearth_label.clone(),
            conversation_hash: record.conversation_hash.clone(),
            guidance_kind: record.guidance_kind.clone(),
            delivery_at: record.at.clone(),
            source: record.source.clone(),
            project_label: record.project_label.clone(),
            delivery_outcome: record.outcome.clone(),
            resume_source: record.resume_source.clone(),
            matched_begin: None,
            terminal: TerminalStatus::NotJoined,
            unjoin_reason: None,
        });
        pre_migration.push(record.pre_migration);
    }

    // ── begins, processed in ascending (begin_at, playbook_run_id) ──────────
    let begins: Vec<&ActivityLogRecord> = input
        .activity
        .iter()
        .filter(|r| r.command == "begin" && in_window(&r.at, options))
        .filter(|r| {
            project_filter.is_none_or(|p| p == r.project_label.as_deref().unwrap_or(""))
        })
        .collect();
    let mut order: Vec<usize> = (0..begins.len()).collect();
    order.sort_by(|a, b| {
        let (x, y) = (begins[*a], begins[*b]);
        x.at
            .cmp(&y.at)
            .then_with(|| x.playbook_run_id.cmp(&y.playbook_run_id))
    });

    let mut consumed = vec![false; eps.len()];
    let mut matched: Vec<Option<usize>> = vec![None; begins.len()];
    let mut begin_reason: Vec<Option<BeginUnjoinReason>> = vec![None; begins.len()];
    for &bi in &order {
        let begin = begins[bi];
        let hash = begin.conversation_hash.clone().unwrap_or_default();
        if hash.is_empty() || hash == UNKNOWN_CONVERSATION_HASH {
            begin_reason[bi] = Some(BeginUnjoinReason::NoConversationKey);
            continue;
        }
        let mut best: Option<usize> = None;
        let mut any_hash = false;
        let mut any_prior_of_kind = false;
        for (ei, episode) in eps.iter().enumerate() {
            if episode.conversation_hash != hash {
                continue;
            }
            any_hash = true;
            if episode.guidance_kind != begin.artifact_kind || episode.delivery_at >= begin.at {
                continue;
            }
            any_prior_of_kind = true;
            if consumed[ei] {
                continue;
            }
            // Most recent prior; a `delivery_at` tie breaks toward the later
            // append-order row.
            best = match best {
                Some(b)
                    if (eps[b].delivery_at.as_str(), b) > (episode.delivery_at.as_str(), ei) =>
                {
                    Some(b)
                }
                _ => Some(ei),
            };
        }
        match best {
            Some(ei) => {
                consumed[ei] = true;
                matched[bi] = Some(ei);
            }
            None if !any_hash => {
                begin_reason[bi] = Some(BeginUnjoinReason::ConversationAbsentFromDeliverySide)
            }
            None if !any_prior_of_kind => {
                begin_reason[bi] = Some(BeginUnjoinReason::NoPriorDeliveryOfKind)
            }
            None => begin_reason[bi] = Some(BeginUnjoinReason::NoUnconsumedPriorDeliveryOfKind),
        }
    }

    // ── the terminal leg, for matched episodes only ─────────────────────────
    for (bi, slot) in matched.iter().enumerate() {
        let Some(ei) = slot else { continue };
        let begin = begins[bi];
        let run_id = begin.playbook_run_id.clone().unwrap_or_default();
        let (last_transition_at, terminal) =
            select_transition(input.activity, &run_id, &begin.artifact_kind, registry);
        eps[*ei].matched_begin = Some(MatchedBegin {
            playbook_run_id: run_id,
            begin_at: begin.at.clone(),
            artifact_kind: begin.artifact_kind.clone(),
            last_transition_at,
        });
        eps[*ei].terminal = terminal;
    }

    // ── unjoin reasons, computed before assignment so the whole vector is
    //    visible while each row's cause is decided ────────────────────────────
    let begin_hashes: BTreeSet<&str> = begins
        .iter()
        .filter_map(|r| r.conversation_hash.as_deref())
        .filter(|h| !h.is_empty() && *h != UNKNOWN_CONVERSATION_HASH)
        .collect();
    let reasons: Vec<Option<UnjoinReason>> = eps
        .iter()
        .enumerate()
        .map(|(ei, episode)| {
            if episode.matched_begin.is_some() {
                return None;
            }
            Some(if episode.conversation_hash == UNKNOWN_CONVERSATION_HASH {
                if pre_migration[ei] {
                    UnjoinReason::PreMigrationRow
                } else {
                    UnjoinReason::NoConversationKey
                }
            } else if !begin_hashes.contains(episode.conversation_hash.as_str()) {
                UnjoinReason::ConversationAbsentFromBeginSide
            } else if eps.iter().enumerate().any(|(ej, other)| {
                other.matched_begin.is_some()
                    && other.conversation_hash == episode.conversation_hash
                    && other.guidance_kind == episode.guidance_kind
                    && (other.delivery_at.as_str(), ej) > (episode.delivery_at.as_str(), ei)
            }) {
                UnjoinReason::SupersededByLaterDeliveryOfKind
            } else {
                UnjoinReason::NoBeginOfKindInConversation
            })
        })
        .collect();

    let mut unjoin = UnjoinCounts::default();
    for (episode, reason) in eps.iter_mut().zip(reasons) {
        episode.unjoin_reason = reason;
        match reason {
            None => {}
            Some(UnjoinReason::NoConversationKey) => unjoin.no_conversation_key += 1,
            Some(UnjoinReason::PreMigrationRow) => unjoin.pre_migration_row += 1,
            Some(UnjoinReason::ConversationAbsentFromBeginSide) => {
                unjoin.conversation_absent_from_begin_side += 1
            }
            Some(UnjoinReason::NoBeginOfKindInConversation) => {
                unjoin.no_begin_of_kind_in_conversation += 1
            }
            Some(UnjoinReason::SupersededByLaterDeliveryOfKind) => {
                unjoin.superseded_by_later_delivery_of_kind += 1
            }
        }
    }

    let mut begin_unjoin = BeginUnjoinCounts::default();
    let mut unmatched: Vec<UnmatchedBegin> = Vec::new();
    for (bi, begin) in begins.iter().enumerate() {
        let Some(reason) = begin_reason[bi] else {
            continue;
        };
        match reason {
            BeginUnjoinReason::NoConversationKey => begin_unjoin.no_conversation_key += 1,
            BeginUnjoinReason::ConversationAbsentFromDeliverySide => {
                begin_unjoin.conversation_absent_from_delivery_side += 1
            }
            BeginUnjoinReason::NoPriorDeliveryOfKind => begin_unjoin.no_prior_delivery_of_kind += 1,
            BeginUnjoinReason::NoUnconsumedPriorDeliveryOfKind => {
                begin_unjoin.no_unconsumed_prior_delivery_of_kind += 1
            }
        }
        unmatched.push(UnmatchedBegin {
            hearth_label: input.hearth_label.clone(),
            conversation_hash: begin.conversation_hash.clone().unwrap_or_default(),
            artifact_kind: begin.artifact_kind.clone(),
            playbook_run_id: begin.playbook_run_id.clone().unwrap_or_default(),
            begin_at: begin.at.clone(),
            reason,
        });
    }

    let mut terminal = TerminalCounts::default();
    for episode in &eps {
        match episode.terminal {
            TerminalStatus::NotJoined => terminal.not_joined += 1,
            TerminalStatus::NotYetTerminal => terminal.not_yet_terminal += 1,
            TerminalStatus::ReachedTerminal => terminal.reached_terminal += 1,
            TerminalStatus::UnknownRunState => terminal.unknown_run_state += 1,
        }
    }

    let coverage = HearthJoinCoverage {
        hearth_label: input.hearth_label.clone(),
        window_start: options.window_start.clone(),
        window_end: options.window_end.clone(),
        delivery_rows_read: input.delivery.len() as u64,
        read_defects: input.read_defects,
        activity_rows_scanned: input.activity_rows_scanned,
        activity_rows_retained: input.activity.len() as u64,
        begin_rows_read: begins.len() as u64,
        episode_denominator: eps.len() as u64,
        begin_denominator: begins.len() as u64,
        menu_delivered,
        nothing_delivered,
        no_engine_answer,
        joined: matched.iter().filter(|m| m.is_some()).count() as u64,
        unjoin,
        begin_unjoin,
        terminal,
        key_epoch: options.key_epoch.clone(),
        hearth_salt_file_epoch: input.hearth_salt_file_epoch.clone(),
        key_epoch_reconciliation: reconcile(
            &options.key_epoch,
            input.hearth_salt_file_epoch.as_deref(),
        ),
    };
    (eps, unmatched, coverage)
}

/// Reads the folded set and nothing else — no matching happens here, which is
/// what makes "nobody re-implements matching" structural.
pub fn fold_join_coverage(set: &JoinEpisodeSet) -> JoinCoverageReport {
    JoinCoverageReport {
        per_hearth: set.per_hearth.clone(),
        filter_version: set.filter_version.to_string(),
    }
}

/// Period granularity is the EXISTING [`Granularity`]; no second week rule is
/// minted here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpisodeGrouping {
    Hearth,
    HearthKind,
    HearthPeriod(Granularity),
    HearthKindPeriod(Granularity),
    HearthRun,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct EpisodeGroupKey {
    pub hearth_label: String,
    pub guidance_kind: Option<String>,
    pub period_start: Option<String>,
    pub playbook_run_id: Option<String>,
}

/// `n` is a REQUIRED field, so a consumer cannot render a cell without its
/// count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EpisodeGroup {
    pub key: EpisodeGroupKey,
    pub n: usize,
    pub episode_indices: Vec<usize>,
}

/// Group the episode vector. Suppresses NOTHING — a group of one is returned as
/// a group of one — and orders deterministically by key.
pub fn group_episodes(set: &JoinEpisodeSet, grouping: EpisodeGrouping) -> Vec<EpisodeGroup> {
    let mut buckets: BTreeMap<EpisodeGroupKey, Vec<usize>> = BTreeMap::new();
    for (index, episode) in set.episodes.iter().enumerate() {
        let mut key = EpisodeGroupKey {
            hearth_label: episode.hearth_label.clone(),
            guidance_kind: None,
            period_start: None,
            playbook_run_id: None,
        };
        match grouping {
            EpisodeGrouping::Hearth => {}
            EpisodeGrouping::HearthKind => {
                key.guidance_kind = Some(episode.guidance_kind.clone());
            }
            EpisodeGrouping::HearthPeriod(granularity) => {
                let Some(period) = bucket_key(&episode.delivery_at, granularity) else {
                    continue;
                };
                key.period_start = Some(period);
            }
            EpisodeGrouping::HearthKindPeriod(granularity) => {
                let Some(period) = bucket_key(&episode.delivery_at, granularity) else {
                    continue;
                };
                key.guidance_kind = Some(episode.guidance_kind.clone());
                key.period_start = Some(period);
            }
            EpisodeGrouping::HearthRun => {
                let Some(matched) = &episode.matched_begin else {
                    continue;
                };
                key.playbook_run_id = Some(matched.playbook_run_id.clone());
            }
        }
        buckets.entry(key).or_default().push(index);
    }
    buckets
        .into_iter()
        .map(|(key, episode_indices)| EpisodeGroup {
            key,
            n: episode_indices.len(),
            episode_indices,
        })
        .collect()
}
