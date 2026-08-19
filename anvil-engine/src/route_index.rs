//! Engine-local in-memory state for the open-marker index and the re-nudge
//! dedup (track open_marker_index_and_nudge_dedup).
//!
//! Both structures are OPTIMIZATIONS layered over the durable markers, never a
//! second source of truth:
//!
//!   * [`OpenMarkerIndex`] caches, per `(hearth, conversation_id)`, the CANDIDATE
//!     artifact ids that carry an open begin marker so the route handler can
//!     confirm openness by reading only those candidates instead of enumerating
//!     every artifact. The cache holds CANDIDATES, not verdicts — the route
//!     handler re-confirms each candidate fresh (open begin marker +
//!     non-terminal current state) at lookup time, so staleness can never yield
//!     a wrong answer. A miss FAILS OPEN to the full scan (which also rebuilds
//!     the entry).
//!
//!   * [`NudgeDedup`] suppresses a repeat check-in / reminder nudge for a
//!     conversation when one was emitted within [`NUDGE_DEDUP_WINDOW`] AND
//!     nothing changed since (no begin / snapshot / complete on that
//!     conversation's open playbook). A state change or a continuation token
//!     re-arms it. It NEVER suppresses the first nudge — default to nudging when
//!     unsure (a wrong suppression hides a dangling instance; a missed
//!     suppression is merely a little noise).
//!
//! Both survive engine restart by being rebuildable / re-armable: the index
//! lazily rebuilds from the durable begin markers on a miss, and the dedup
//! simply re-arms (every conversation nudges once more after a restart, which is
//! harmless).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// How long a check-in / reminder nudge for one conversation suppresses a
/// repeat nudge, absent any intervening state change. Tunable post-deploy with
/// the `*_suppressed` telemetry markers (spec out-of-scope: empirical tuning).
pub const NUDGE_DEDUP_WINDOW: Duration = Duration::from_secs(10 * 60);

/// Per-`(hearth, conversation_id)` candidate open-marker index.
#[derive(Default)]
pub struct OpenMarkerIndex {
    inner: RwLock<HashMap<(PathBuf, String), Vec<String>>>,
}

impl OpenMarkerIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// The cached candidate artifact ids for `(hearth, conversation_id)`, or
    /// `None` when the conversation is COLD (never indexed) — the caller must
    /// then fail open to the full scan and call [`Self::populate`] with the
    /// scan's confirmed candidate set. A poisoned lock reads as cold (fail-open).
    pub fn candidates(&self, hearth: &Path, conversation_id: &str) -> Option<Vec<String>> {
        let guard = self.inner.read().ok()?;
        guard
            .get(&(hearth.to_path_buf(), conversation_id.to_string()))
            .cloned()
    }

    /// Replace the cached candidate set for `(hearth, conversation_id)` — used
    /// after a full scan rebuilt the truth, so a subsequent lookup can take the
    /// cheap candidate path. A poisoned lock is a no-op (fail-open: the next
    /// lookup just rebuilds again).
    pub fn populate(&self, hearth: &Path, conversation_id: &str, candidates: Vec<String>) {
        if let Ok(mut guard) = self.inner.write() {
            guard.insert(
                (hearth.to_path_buf(), conversation_id.to_string()),
                candidates,
            );
        }
    }

    /// Add `artifact_id` as a candidate for `(hearth, conversation_id)` — called
    /// on `begin` so a freshly-begun playbook is discoverable WITHOUT a scan.
    /// Idempotent. A poisoned lock is a no-op (the next miss rebuilds via scan).
    pub fn record_begin(&self, hearth: &Path, conversation_id: &str, artifact_id: &str) {
        if conversation_id.trim().is_empty() {
            return;
        }
        if let Ok(mut guard) = self.inner.write() {
            let entry = guard
                .entry((hearth.to_path_buf(), conversation_id.to_string()))
                .or_default();
            if !entry.iter().any(|id| id == artifact_id) {
                entry.push(artifact_id.to_string());
            }
        }
    }

    /// Drop the cached candidate set for `(hearth, conversation_id)` — called on
    /// a `snapshot` / `complete` that may have moved the playbook to a terminal
    /// state. The next lookup is then a cold miss → full scan → repopulate, so a
    /// completed playbook is never served from a stale candidate list. (Re-read
    /// confirmation already prevents a wrong verdict; this just keeps the cache
    /// from growing stale.) A poisoned lock is a no-op.
    pub fn invalidate(&self, hearth: &Path, conversation_id: &str) {
        if conversation_id.trim().is_empty() {
            return;
        }
        if let Ok(mut guard) = self.inner.write() {
            guard.remove(&(hearth.to_path_buf(), conversation_id.to_string()));
        }
    }
}

/// After this many CONSECUTIVE unrelated route turns for one open artifact
/// (matched-other or idle-no_match, with no intervening begin/snapshot/complete/
/// continuation-token rearm), the resume reminder for that artifact goes QUIET
/// and the park hint is surfaced ONCE (resume-signal context-awareness, P2).
///
/// Chosen as 2: the real specimen (12 context-blind resume nudges for one open
/// track while the agent built unrelated features) shows a single tangential turn
/// is NOT enough to conclude the agent moved on — but two consecutive unrelated
/// turns with no lifecycle progress on the open artifact is a clear "moved on"
/// signal. So turns 1..2 still surface the reminder (fail toward reminding), and
/// the 3rd (`> 2`) is the first quieted turn — the one that surfaces the park
/// affordance instead of nagging.
pub const UNRELATED_QUIET_THRESHOLD: u32 = 2;

/// One open artifact's last-nudge bookkeeping.
#[derive(Clone, Copy)]
struct NudgeState {
    /// When the last check-in / reminder nudge fired.
    last_nudge_at: Instant,
    /// The artifact's state generation AT the moment of the last nudge.
    nudged_generation: u64,
}

/// Per-open-artifact re-nudge dedup keyed by `(hearth, conversation_id,
/// artifact_id)`.
///
/// The artifact id in the key is essential (resume-signal context-awareness P2):
/// `find_open_playbook_run_for_conversation` can return DIFFERENT open artifacts
/// across turns (most-recently-begun), and two open tracks of the same kind in
/// one conversation must keep INDEPENDENT counters + park-hint history. A
/// conversation-only key would conflate them and re-arm the whole conversation on
/// any one artifact's lifecycle event.
type ArtifactKey = (PathBuf, String, String);

/// Per-`(hearth, conversation_id, artifact_id)` re-nudge dedup + sticky
/// unrelated-turn suppression.
#[derive(Default)]
pub struct NudgeDedup {
    /// Open artifact → last-nudge bookkeeping (time-window dedup for the
    /// no_match-while-open reminder).
    nudges: RwLock<HashMap<ArtifactKey, NudgeState>>,
    /// Open artifact → monotonic state generation. Bumped on every begin /
    /// snapshot / complete / continuation-token resume for that artifact. A
    /// nudge is suppressed only when the CURRENT generation equals the
    /// generation recorded at the last nudge (i.e. nothing changed since).
    generations: RwLock<HashMap<ArtifactKey, u64>>,
    /// Open artifact → count of CONSECUTIVE unrelated route turns (matched-other
    /// or idle-no_match) with no intervening lifecycle/continuation rearm. Reset
    /// to 0 on any `rearm` (a real change) — including a related resume turn.
    unrelated_counts: RwLock<HashMap<ArtifactKey, u32>>,
    /// Open artifact → whether the park hint has already been surfaced for the
    /// CURRENT sticky-suppression episode. Cleared on `rearm` so a re-armed
    /// artifact that later moves on again surfaces the hint afresh.
    park_hint_surfaced: RwLock<HashMap<ArtifactKey, bool>>,
}

impl NudgeDedup {
    pub fn new() -> Self {
        Self::default()
    }

    fn key(hearth: &Path, conversation_id: &str, artifact_id: &str) -> ArtifactKey {
        (
            hearth.to_path_buf(),
            conversation_id.to_string(),
            artifact_id.to_string(),
        )
    }

    fn current_generation(&self, hearth: &Path, conversation_id: &str, artifact_id: &str) -> u64 {
        self.generations
            .read()
            .ok()
            .and_then(|g| g.get(&Self::key(hearth, conversation_id, artifact_id)).copied())
            .unwrap_or(0)
    }

    /// Re-arm the open artifact — a begin / snapshot / complete / continuation-
    /// token resume (or a fresh RELATED resume turn) happened, so the NEXT nudge
    /// must NOT be suppressed and the sticky unrelated-turn suppression resets:
    /// bump the state generation, reset the consecutive-unrelated counter, and
    /// clear the surface-once park-hint flag. A poisoned lock is a no-op (fail
    /// toward nudging). An empty conversation or artifact id is ignored (never
    /// mid-playbook).
    pub fn rearm(&self, hearth: &Path, conversation_id: &str, artifact_id: &str) {
        if conversation_id.trim().is_empty() || artifact_id.trim().is_empty() {
            return;
        }
        let key = Self::key(hearth, conversation_id, artifact_id);
        if let Ok(mut g) = self.generations.write() {
            let entry = g.entry(key.clone()).or_insert(0);
            *entry = entry.wrapping_add(1);
        }
        if let Ok(mut c) = self.unrelated_counts.write() {
            c.insert(key.clone(), 0);
        }
        if let Ok(mut p) = self.park_hint_surfaced.write() {
            p.insert(key, false);
        }
    }

    /// Record one CONSECUTIVE unrelated route turn for the open artifact and
    /// return the new running count. The caller decides suppression by comparing
    /// against [`UNRELATED_QUIET_THRESHOLD`] (quiet when the returned count
    /// exceeds it). A poisoned lock returns 0 (fail toward NOT quieting — never a
    /// wrong suppression). An empty conversation/artifact id returns 0.
    pub fn record_unrelated(&self, hearth: &Path, conversation_id: &str, artifact_id: &str) -> u32 {
        if conversation_id.trim().is_empty() || artifact_id.trim().is_empty() {
            return 0;
        }
        let key = Self::key(hearth, conversation_id, artifact_id);
        if let Ok(mut c) = self.unrelated_counts.write() {
            let entry = c.entry(key).or_insert(0);
            *entry = entry.saturating_add(1);
            *entry
        } else {
            0
        }
    }

    /// Surface-once gate for the park hint: returns `true` the FIRST time it is
    /// called for the open artifact within the current sticky-suppression episode
    /// (and records that it has surfaced), `false` on every subsequent call until
    /// the next `rearm`. A poisoned lock returns `false` (never surface twice). An
    /// empty conversation/artifact id returns `false`.
    pub fn take_park_hint_once(
        &self,
        hearth: &Path,
        conversation_id: &str,
        artifact_id: &str,
    ) -> bool {
        if conversation_id.trim().is_empty() || artifact_id.trim().is_empty() {
            return false;
        }
        let key = Self::key(hearth, conversation_id, artifact_id);
        let Ok(mut p) = self.park_hint_surfaced.write() else {
            return false;
        };
        let already = p.get(&key).copied().unwrap_or(false);
        if already {
            false
        } else {
            p.insert(key, true);
            true
        }
    }

    /// Decide whether a check-in / reminder nudge should fire for this open
    /// artifact RIGHT NOW. Returns `true` to nudge (and records this nudge),
    /// `false` to suppress.
    ///
    /// Suppress iff a prior nudge fired within [`NUDGE_DEDUP_WINDOW`] AND the
    /// artifact's state generation has NOT advanced since that nudge. The first
    /// nudge for an artifact (no recorded prior) ALWAYS fires. On a fire, the
    /// current `(at, generation)` is recorded so the next call can compare. A
    /// poisoned lock fails toward nudging (returns `true` without recording —
    /// never a wrong suppression).
    pub fn should_nudge(&self, hearth: &Path, conversation_id: &str, artifact_id: &str) -> bool {
        if conversation_id.trim().is_empty() || artifact_id.trim().is_empty() {
            // Empty conversation/artifact can never be deduped (it can never be
            // mid-playbook anyway); always allow, never record.
            return true;
        }
        let key = Self::key(hearth, conversation_id, artifact_id);
        let current_gen = self.current_generation(hearth, conversation_id, artifact_id);
        let now = Instant::now();

        let Ok(mut nudges) = self.nudges.write() else {
            return true;
        };
        let suppress = match nudges.get(&key) {
            Some(prev) => {
                now.duration_since(prev.last_nudge_at) < NUDGE_DEDUP_WINDOW
                    && prev.nudged_generation == current_gen
            }
            None => false,
        };
        if suppress {
            return false;
        }
        nudges.insert(
            key,
            NudgeState {
                last_nudge_at: now,
                nudged_generation: current_gen,
            },
        );
        true
    }
}
