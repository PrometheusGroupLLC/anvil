//! ActorActivity read-side: per-actor activity folded from the `activity:`
//! begin-markers each artifact's status.yaml carries.
//!
//! This is the LOCAL-DASHBOARD counterpart to the salted `distinct_actors`
//! count the usage-time-series query reports. Where `distinct_actors` reports
//! an anonymized *count* derived from the salted `actor_hash` in the
//! step-measurement sink (the only actor signal that may leave the machine),
//! this query returns the RAW actor NAMES recorded in the begin-markers.
//!
//! CRITICAL — LOCAL-ONLY, NEVER TELEMETRY. The raw actor names this fold
//! returns are served ONLY over the loopback `/ws` bridge and the on-machine
//! gRPC surface to the local dashboard. They MUST NOT be emitted to the
//! telemetry path: the salted `actor_hash` in the step-measurement sink remains
//! the only actor signal that can leave the machine. This fold reads the
//! begin-markers only; it never touches the telemetry emitter or the
//! step-measurement sink.
//!
//! The fold is a pure function over already-read `(artifact_kind, entries)`
//! streams — no filesystem, no ports. The engine walks the hearth's artifact
//! directories, reads each `status.yaml`'s `activity:` section, and hands the
//! vectors here. An empty input yields an empty result (a fresh hearth has no
//! begin-markers yet).

use crate::domain::shared_types::ActivityEntry;
use std::collections::{BTreeMap, BTreeSet};

/// Per-actor activity rolled up across every artifact's begin-markers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorActivityEntry {
    /// The RAW actor name, exactly as recorded in the begin-marker. LOCAL-ONLY.
    pub actor: String,
    /// Total begin-markers attributed to this actor across all artifacts.
    pub begin_count: u64,
    /// The most recent (ISO) `at` timestamp among this actor's begin-markers.
    pub last_active: String,
    /// The distinct artifact kinds (e.g. "track", "milestone") this actor has a
    /// begin-marker on, ordered ascending.
    pub artifact_kinds: Vec<String>,
}

/// The full ActorActivity result: entries ordered by descending begin_count,
/// then ascending actor name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorActivityResult {
    pub entries: Vec<ActorActivityEntry>,
}

/// Internal accumulator: a running roll-up for one actor.
struct Acc {
    begin_count: u64,
    last_active: String,
    artifact_kinds: BTreeSet<String>,
}

/// Fold a set of per-artifact `(artifact_kind, activity entries)` streams into
/// per-actor activity.
///
/// Every begin-marker (an [`ActivityEntry`]) contributes one to its actor's
/// `begin_count`, contributes its artifact's `kind` to that actor's
/// `artifact_kinds` set, and advances `last_active` to the latest `at` seen.
/// `last_active` is compared lexically — ISO-8601 timestamps sort lexically the
/// same as chronologically. An entry with an empty `actor` is skipped (it
/// cannot be attributed). Entries whose `kind` is not "begin" are skipped — only
/// begin-markers count (the "resume" marker is reserved for a later slice).
/// An empty `artifact_kind` is NOT folded into `artifact_kinds`: it
/// contributes no kind entry, but the begin itself is still counted.
///
/// The result is deterministic: entries ordered by descending begin_count, then
/// ascending actor name; each entry's `artifact_kinds` ordered ascending.
pub fn fold_actor_activity(streams: &[(String, Vec<ActivityEntry>)]) -> ActorActivityResult {
    let mut by_actor: BTreeMap<String, Acc> = BTreeMap::new();

    for (artifact_kind, entries) in streams {
        for entry in entries {
            // Only begin-markers count toward actor activity.
            if entry.kind != "begin" {
                continue;
            }
            if entry.actor.trim().is_empty() {
                continue;
            }
            let acc = by_actor.entry(entry.actor.clone()).or_insert_with(|| Acc {
                begin_count: 0,
                last_active: String::new(),
                artifact_kinds: BTreeSet::new(),
            });
            acc.begin_count += 1;
            if !artifact_kind.trim().is_empty() {
                acc.artifact_kinds.insert(artifact_kind.clone());
            }
            // ISO-8601 timestamps sort lexically == chronologically.
            if entry.at > acc.last_active {
                acc.last_active = entry.at.clone();
            }
        }
    }

    let mut entries: Vec<ActorActivityEntry> = by_actor
        .into_iter()
        .map(|(actor, acc)| ActorActivityEntry {
            actor,
            begin_count: acc.begin_count,
            last_active: acc.last_active,
            artifact_kinds: acc.artifact_kinds.into_iter().collect(),
        })
        .collect();

    // Descending begin_count, then ascending actor name.
    entries.sort_by(|a, b| {
        b.begin_count
            .cmp(&a.begin_count)
            .then_with(|| a.actor.cmp(&b.actor))
    });

    ActorActivityResult { entries }
}

/// Cross-hearth merge of multiple per-hearth `(artifact_kind, entries)` streams
/// into a single per-actor roll-up.
///
/// Concatenates every hearth's streams, then folds the union with
/// [`fold_actor_activity`]. begin_counts SUM across hearths and `artifact_kinds`
/// UNION — correct because the actor NAME is the same string everywhere (this is
/// a local query; there is no salt to reconcile).
pub fn fold_actor_activity_across_hearths(
    streams: &[Vec<(String, Vec<ActivityEntry>)>],
) -> ActorActivityResult {
    let mut all: Vec<(String, Vec<ActivityEntry>)> = Vec::new();
    for hearth in streams {
        all.extend(hearth.iter().cloned());
    }
    fold_actor_activity(&all)
}
