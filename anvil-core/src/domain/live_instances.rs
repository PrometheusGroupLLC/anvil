//! Live-instances read-side: the ACTIVELY-IN-PROGRESS artifact instances in a
//! hearth, each carrying its folded current state, the RAW actor name +
//! latest timestamp from that instance's `activity:` begin-markers, and its
//! §0-derived activity detail (action count, artifact directory).
//!
//! CRITICAL — LOCAL-ONLY, NEVER TELEMETRY. Like [`crate::domain::actor_activity`],
//! the actor names this fold surfaces are the RAW names recorded in the
//! begin-markers — the SAME LOCAL-ONLY signal the ActorActivity query reads. They
//! are served ONLY over the loopback `/ws` bridge and the on-machine gRPC surface
//! to the local dashboard. They MUST NOT reach the telemetry path: the salted
//! `actor_hash` in the step-measurement sink remains the only actor signal that
//! may leave the machine. This fold NEVER reads that salted `actor_hash` (the
//! dishonesty the Atlas exists to expose) — the live actor comes from the
//! begin-markers only. `action_count` is a plain COUNT of §0 events, never their
//! prose content.
//!
//! ## "Live" vs merely "open"
//!
//! Every non-terminal artifact is "open", but most open artifacts are DORMANT —
//! a proposal sitting in `active`, a track `shelved` — not someone's live work.
//! Rendering all of them as "live agents" is dishonest. An instance counts as
//! LIVE when EITHER:
//!   - its most-recent §0 `StepMeasurementEvent` (the temper-consumed stream) is
//!     within [`LIVE_WINDOW_HOURS`] of `now`, OR
//!   - it carries a RAW actor from an `activity:` begin-marker (already
//!     filtered to non-terminal by the engine before this fold runs).
//! An open instance satisfying NEITHER is DORMANT — excluded from
//! `LiveInstances::instances` and counted (not silently dropped) in
//! `LiveInstances::idle_count`.
//!
//! The fold is a pure function over already-read [`LiveInstanceInput`] records
//! and an injected `now` (determinism — no `SystemTime::now()`/`Utc::now()`
//! inside the fold). The engine owns the I/O: it reads the catalog active-set
//! (already non-terminal via the hardcoded terminal-state filter), reaches each
//! instance's status.yaml for the specific `kind` + the `activity:` begin-markers,
//! folds the current state over the transition events, reads the §0 stream for
//! the instance's action count + most-recent timestamp, resolves the on-disk
//! artifact directory, and applies the registry-aware `state_is_terminal`
//! belt-and-suspenders filter BEFORE handing the open set here. An empty input
//! yields an empty result.

use crate::domain::shared_types::ActivityEntry;
use chrono::{DateTime, Utc};

/// An instance counts as LIVE when its most-recent §0 event is within this
/// many hours of `now` (the OTHER liveness path — a raw actor present — has no
/// window; see the module doc).
pub const LIVE_WINDOW_HOURS: i64 = 24;

/// One open (non-terminal) instance's already-read inputs. The engine assembles
/// these from the catalog active-artifacts joined with each instance's
/// status.yaml (`kind` + `activity:` begin-markers), its folded current state,
/// its §0-stream activity (count + most-recent timestamp), and its on-disk
/// artifact directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveInstanceInput {
    /// The artifact directory id (the instance id).
    pub instance_id: String,
    /// The instance's specific playbook kind (status.yaml `kind`).
    pub kind: String,
    /// The FOLDED current state (status.yaml + transition events).
    pub state: String,
    /// The instance's `activity:` begin-markers (LOCAL-ONLY). The RAW actor +
    /// latest timestamp are joined from here.
    pub activity: Vec<ActivityEntry>,
    /// Count of §0 `StepMeasurementEvent`s recorded for this instance in the
    /// temper-consumed stream. `0` when temper has none (no temper home
    /// resolvable, no events file for the kind, or none carry this
    /// instance's `workflow_id`) — fail-open, never an error.
    pub action_count: u32,
    /// The most-recent §0 event's `at` (RFC3339), empty when `action_count`
    /// is `0`.
    pub last_step0_at: String,
    /// The on-disk artifact directory (e.g. `<hearth>/tracks/<instance_id>`),
    /// so the UI can point at / open the real artifact. Empty if
    /// unresolvable (never fabricated).
    pub artifact_dir: String,
}

/// One LIVE instance in the live view (see the module doc for the live/idle
/// split).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveInstance {
    /// The artifact directory id.
    pub instance_id: String,
    /// The instance's specific playbook kind.
    pub kind: String,
    /// The folded current state.
    pub state: String,
    /// The RAW actor name from the latest begin-marker. LOCAL-ONLY. Empty when
    /// the instance carries no begin-marker (honest — not fabricated).
    pub actor: String,
    /// The latest begin-marker's ISO timestamp. Empty when the instance carries
    /// no begin-marker.
    pub at: String,
    /// The last transition's `to_state` — the SAME value as `state`, surfaced
    /// under the clearer "current step" name the UI renders.
    pub current_step: String,
    /// Count of §0 events recorded for this instance. `0` when temper has none.
    pub action_count: u32,
    /// The on-disk artifact directory, so the UI can link to the real artifact.
    pub artifact_dir: String,
}

/// The full live-instances result: only the instances that are ACTIVELY live
/// (see the module doc), ordered ascending by `instance_id`. `idle_count` is
/// the honest count of OPEN-but-dormant instances excluded from `instances` —
/// surfaced as a number, never rendered as fake "live agents".
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LiveInstances {
    pub instances: Vec<LiveInstance>,
    pub idle_count: u32,
}

/// True when `last_step0_at` (RFC3339) parses and is within [`LIVE_WINDOW_HOURS`]
/// of `now`. An empty or unparseable timestamp is never "recent" (honest — no
/// signal, not a wildcard).
fn step0_is_recent(last_step0_at: &str, now: DateTime<Utc>) -> bool {
    if last_step0_at.trim().is_empty() {
        return false;
    }
    match DateTime::parse_from_rfc3339(last_step0_at) {
        Ok(at) => {
            let at_utc = at.with_timezone(&Utc);
            let age = now.signed_duration_since(at_utc);
            age >= chrono::Duration::zero() && age <= chrono::Duration::hours(LIVE_WINDOW_HOURS)
        }
        Err(_) => false,
    }
}

/// Fold the already-open instance inputs into the live view, splitting
/// genuinely LIVE instances from merely-open DORMANT ones (see the module
/// doc). `now` is INJECTED for determinism — this fold never reads the clock.
///
/// Each input's RAW `actor` + `at` are joined from the LATEST begin-marker in
/// its `activity:` log — a begin-marker is the latest by its `at` (ISO-8601
/// timestamps sort lexically the same as chronologically); markers whose
/// `kind` is not "begin" or whose `actor` is blank are ignored.
///
/// An instance is LIVE when its `last_step0_at` is within [`LIVE_WINDOW_HOURS`]
/// of `now`, OR it resolved a non-empty `actor`. Otherwise it is DORMANT:
/// excluded from `instances`, counted in `idle_count`.
///
/// The result is deterministic: instances ordered ascending by `instance_id`.
pub fn fold_live_instances(inputs: &[LiveInstanceInput], now: DateTime<Utc>) -> LiveInstances {
    let mut instances: Vec<LiveInstance> = Vec::new();
    let mut idle_count: u32 = 0;

    for input in inputs {
        let latest = input
            .activity
            .iter()
            .filter(|e| e.kind == "begin" && !e.actor.trim().is_empty())
            .max_by(|a, b| a.at.cmp(&b.at));
        let (actor, at) = match latest {
            Some(entry) => (entry.actor.clone(), entry.at.clone()),
            None => (String::new(), String::new()),
        };

        let is_live = step0_is_recent(&input.last_step0_at, now) || !actor.trim().is_empty();

        if is_live {
            instances.push(LiveInstance {
                instance_id: input.instance_id.clone(),
                kind: input.kind.clone(),
                state: input.state.clone(),
                actor,
                at,
                current_step: input.state.clone(),
                action_count: input.action_count,
                artifact_dir: input.artifact_dir.clone(),
            });
        } else {
            idle_count += 1;
        }
    }

    instances.sort_by(|a, b| a.instance_id.cmp(&b.instance_id));

    LiveInstances {
        instances,
        idle_count,
    }
}
