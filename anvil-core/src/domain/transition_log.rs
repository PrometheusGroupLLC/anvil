//! The single resolution seam for an artifact's current state and its
//! ordered transition history.
//!
//! Every consumer that needs "what state is this artifact in?" or "what is
//! its transition history?" routes through these pure functions instead of
//! inlining `status.state` / `status.transitions.last()` / ad-hoc folds.
//! Today the seam reads the legacy parsed `status.yaml` (`FullStatusYaml`).
//! Concentrating the logic here is the whole point: a later phase can swap
//! the underlying source (e.g. an event-file log) without touching any
//! consumer, because no consumer inlines the resolution.
//!
//! Three distinct reads are exposed because consumers genuinely need
//! different things — and the differences are load-bearing behavior that
//! must be preserved exactly:
//!
//! * [`resolve_state`] — the artifact's *current* state. Top-level `state`
//!   if present, else the last transition's `to`, else `None`. This is the
//!   denormalized-cache fold the filesystem read sites have always used.
//! * [`declared_state`] — the *raw* top-level `state:` field only, with NO
//!   transition fallback. Used where a consumer historically read the field
//!   directly (and so must keep that exact, narrower semantics).
//! * [`resolve_transitions`] — the ordered transition history, oldest →
//!   newest, as recorded. Used by begin-adoption detection.

use crate::domain::status::{FullStatusYaml, StatusTransition};
use crate::ports::transition_event_write_port::{TransitionRecord, TRANSITIONS_DIR};
use std::path::Path;

/// Resolve the artifact's current state.
///
/// The top-level `state` field is a denormalized cache of the last
/// transition's `to` — the writer keeps them equal on every transition.
/// When the top-level field is absent (e.g. a hand-created artifact, or one
/// transitioned before the writer inserted the field), the last transition's
/// `to` reproduces exactly what the writer would have written. State is
/// unresolvable only when there is neither a top-level `state` nor any
/// transitions.
pub fn resolve_state(status: &FullStatusYaml) -> Option<String> {
    status.state.clone().or_else(|| {
        status
            .transitions
            .as_ref()
            .and_then(|t| t.last())
            .map(|t| t.to.clone())
    })
}

/// The raw top-level `state:` field, with NO fallback to the transition log.
///
/// This is deliberately narrower than [`resolve_state`]: callers that have
/// always read `status.state` directly (and so treat a missing field as
/// "empty", never reaching back to the last transition) route here to keep
/// that exact semantics.
pub fn declared_state(status: &FullStatusYaml) -> Option<String> {
    status.state.clone()
}

/// The ordered transition history, oldest → newest, exactly as recorded.
/// Absent history resolves to an empty list.
pub fn resolve_transitions(status: &FullStatusYaml) -> Vec<StatusTransition> {
    status.transitions.clone().unwrap_or_default()
}

// === Event-store fold (Phase 2) ============================================
//
// Transitions are written one-file-per-event under `<artifact>/transitions/`.
// The fold MERGES the legacy `status.yaml` array with the per-file event
// records, ordered by embedded timestamp (then a stable filename/uuid tiebreak
// surfaced as the record's content), and dedupes exact duplicates. This
// guarantees no history loss for a legacy artifact that gains a new event, and
// is read-order-independent (R3/R4). Phase 3 tightens this to "non-empty event
// dir authoritative + legacy seeding".

/// An event record paired with its on-disk filename — the filename is the
/// stable tiebreak when two records carry the same `at`.
#[derive(Debug, Clone)]
pub struct EventFile {
    pub file_name: String,
    pub record: TransitionRecord,
}

/// Read every event file under `<artifact_dir>/transitions/`, returning them
/// paired with their filenames. Absent directory → empty. Read order is
/// filesystem-arbitrary — the fold sorts.
///
/// # C-d.1 round 8, H-1 — this function carried the whole class, one frame
/// below a 249-cell matrix that could not see it
///
/// It held the round-3, round-4 and round-5 spellings verbatim:
///
/// ```text
/// Err(_) => return Vec::new(),                    // round 3 — Err emptied
/// for entry in entries.flatten()                  // the per-ENTRY error, dropped
/// if !path.is_file() { continue; }                // round 4 — the per-entry stat
/// let Ok(content) = read_to_string(..) else {..}  // round 5
/// ```
///
/// `resolve_state_with_events` and `resolve_transitions_with_events` sit
/// directly above it, and `read_artifact_state`, `read_transitions`,
/// `list_artifacts` and `find_artifact_by_kind_origin_turn` on BOTH
/// `fs_query_adapter` and `fs_snapshot_adapter` go through those. **Measured on
/// unmutated `63df2ff`, three events on disk:** `transitions/` at `0600` gave
/// `Ok(1 transitions)` and — worse — `Ok(implementing)` for an artifact that is
/// `reviewing`. Not a refusal and not an emptying: a **confidently wrong state**,
/// folded from a stale earlier event because the newest one could not be read.
/// Everything downstream that gates on state then gates on the wrong one.
///
/// The matrix could not reach it because its fixture never created the
/// directory, so the `Err(_)` arm ran on a `NotFound` — where emptying is the
/// CORRECT answer — in all 249 cells and passed 249 times.
///
/// # What is fallible now, and what is deliberately still tolerated
///
/// * an unreadable/unlistable `transitions/` directory → `Err`
/// * an entry the kernel cannot hand back → `Err`
/// * an entry whose kind cannot be inspected → `Err`
/// * an event file that cannot be READ → `Err`
/// * an event file that does not PARSE → **still skipped, deliberately.** A
///   corrupt sibling must not sink the fold, and "I could not open it" is a
///   different fact from "this is not a transition record". That is the same
///   split the strict reader makes, and it is the one this class is about: no
///   unreadable input may be read as an empty one.
///
/// KNOWN LIMITATION (impl-review MEDIUM-2), now narrowed to what it actually
/// covers: a silently skipped *unparseable* newest event still resolves the
/// artifact to a stale earlier state. That arises from disk corruption or
/// hand-editing, never from the engine's own writes (serde + atomic
/// temp+rename). The PERMISSION route into the same outcome — which is what
/// C-d.1 exists to close and which the limitation note predated and did not
/// cover — is closed here.
pub fn read_event_files(artifact_dir: &Path) -> Result<Vec<EventFile>, TransitionEvidenceError> {
    let events_dir = artifact_dir.join(TRANSITIONS_DIR);
    let entries = match std::fs::read_dir(&events_dir) {
        Ok(e) => e,
        // Absence is legitimately-empty history, NOT damage. Every other error
        // means "I could not look", which is a different fact.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(TransitionEvidenceError::UnreadableDir {
                path: events_dir.display().to_string(),
                detail: e.to_string(),
            })
        }
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| TransitionEvidenceError::UnreadableDir {
            path: events_dir.display().to_string(),
            detail: e.to_string(),
        })?;
        let path = entry.path();
        // `Path::is_file()` maps EACCES onto `false`, which is the round-4
        // lexeme: at 0600 the directory lists and the per-entry `stat` is what
        // fails, so every event `continue`d away and the history came back
        // "empty". `node_kind` answers with a type that cannot express that.
        let kind = crate::domain::playbook::fs_probe::node_kind(&path).map_err(|e| {
            TransitionEvidenceError::UnreadableFile {
                path: path.display().to_string(),
                detail: e.to_string(),
            }
        })?;
        if kind != crate::domain::playbook::fs_probe::NodeKind::File {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().to_string();
        if !file_name.ends_with(".yaml") {
            continue;
        }
        let content = std::fs::read_to_string(&path).map_err(|e| {
            TransitionEvidenceError::UnreadableFile {
                path: path.display().to_string(),
                detail: e.to_string(),
            }
        })?;
        let Ok(record) = serde_yaml::from_str::<TransitionRecord>(&content) else {
            continue;
        };
        out.push(EventFile { file_name, record });
    }
    Ok(out)
}

/// Damage encountered while reading the transition event store under a STRICT
/// (fail-closed) policy.
///
/// The lenient [`read_event_files`] deliberately tolerates every one of these —
/// a corrupt sibling must not sink a status fold. The ADOPTION path cannot
/// afford that tolerance: a damaged event is EXACTLY what would let a reset
/// clobber a governed artifact whose only surviving governance evidence is
/// unreadable. Adoption therefore reads strictly and refuses on any of these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionEvidenceError {
    /// The `transitions/` directory exists but could not be enumerated
    /// (permission, I/O). An ABSENT directory is not this error — it is a
    /// legitimately empty history and reads back as `Ok(vec![])`.
    UnreadableDir { path: String, detail: String },
    /// An individual event file could not be read from disk.
    UnreadableFile { path: String, detail: String },
    /// An event file was read but is not a parseable `TransitionRecord`.
    InvalidEvent { path: String, detail: String },
}

impl std::fmt::Display for TransitionEvidenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransitionEvidenceError::UnreadableDir { path, detail } => {
                write!(f, "transitions directory '{}' unreadable: {}", path, detail)
            }
            TransitionEvidenceError::UnreadableFile { path, detail } => {
                write!(f, "transition event file '{}' unreadable: {}", path, detail)
            }
            TransitionEvidenceError::InvalidEvent { path, detail } => {
                write!(f, "transition event file '{}' is not parseable: {}", path, detail)
            }
        }
    }
}

impl std::error::Error for TransitionEvidenceError {}

/// STRICT counterpart to [`read_event_files`] for the adoption evidence read.
///
/// An ABSENT `transitions/` directory is legitimately-empty history → `Ok`.
/// ANY OTHER damage — an unreadable directory, a directory-entry read error, an
/// unreadable event file, or an unparseable event — fails CLOSED with a
/// [`TransitionEvidenceError`] instead of being silently skipped. This is the
/// whole point: the lenient reader could drop the one event that proves an
/// artifact is governed, letting adoption reset it; the strict reader refuses so
/// that cannot happen.
pub fn read_event_files_strict(
    artifact_dir: &Path,
) -> Result<Vec<EventFile>, TransitionEvidenceError> {
    let events_dir = artifact_dir.join(TRANSITIONS_DIR);
    let entries = match std::fs::read_dir(&events_dir) {
        Ok(e) => e,
        // Absence is legitimately-empty history, NOT damage.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(TransitionEvidenceError::UnreadableDir {
                path: events_dir.display().to_string(),
                detail: e.to_string(),
            })
        }
    };
    let mut out = Vec::new();
    for entry in entries {
        // A directory-entry read error is unreadable evidence — fail closed
        // rather than truncating the log (the lenient reader `.flatten()`s
        // these away).
        let entry = entry.map_err(|e| TransitionEvidenceError::UnreadableDir {
            path: events_dir.display().to_string(),
            detail: e.to_string(),
        })?;
        let path = entry.path();
        // ── C-d.1 round 8, H-2: THE FAIL-CLOSED READER FAILED OPEN HERE ──
        //
        // This function exists so an adoption reset cannot proceed over an
        // artifact whose governance evidence merely could not be read. It fixed
        // the `read_dir` swallow and it fixed the `.flatten()`. It left
        // `if !path.is_file() { continue; }` — the round-4 lexeme — and that one
        // line defeated the whole guarantee at exactly ONE mode.
        //
        // Measured on unmutated `63df2ff`, two events on disk:
        //
        //   transitions/ 0300  ->  AdoptionEvidenceUnreadable   (refuses)
        //   transitions/ 0000  ->  AdoptionEvidenceUnreadable   (refuses)
        //   transitions/ 0600  ->  Ok(0 transitions)   *** FAILS OPEN ***
        //
        // At 0600 `read_dir` succeeds and the entry iteration succeeds; the
        // per-entry `stat` fails EACCES; `is_file()` answers `false`; every
        // event is skipped; the function returns "the complete governance
        // history is empty". `begin.rs`'s `handle_adoption` then reads
        // `transitions.is_empty()` as "never governed", `AlreadyGoverned` does
        // not fire, and the reset clobbers a governed artifact — the exact
        // outcome the comment above that call says this read exists to prevent.
        //
        // 0300 and 0000 refusing is what proves 0600 was a swallow and not a
        // uniform failure.
        let kind = crate::domain::playbook::fs_probe::node_kind(&path).map_err(|e| {
            TransitionEvidenceError::UnreadableFile {
                path: path.display().to_string(),
                detail: e.to_string(),
            }
        })?;
        if kind != crate::domain::playbook::fs_probe::NodeKind::File {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().to_string();
        if !file_name.ends_with(".yaml") {
            continue;
        }
        let content = std::fs::read_to_string(&path).map_err(|e| {
            TransitionEvidenceError::UnreadableFile {
                path: path.display().to_string(),
                detail: e.to_string(),
            }
        })?;
        let record = serde_yaml::from_str::<TransitionRecord>(&content).map_err(|e| {
            TransitionEvidenceError::InvalidEvent {
                path: path.display().to_string(),
                detail: e.to_string(),
            }
        })?;
        out.push(EventFile { file_name, record });
    }
    Ok(out)
}

/// A normalized transition usable in the fold, sourced from either the legacy
/// array or an event file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FoldEntry {
    to: String,
    at: String,
    actor: String,
    role: String,
    approver: Option<String>,
    note: Option<String>,
    /// True for entries sourced from the legacy `status.yaml` array (false for
    /// per-file events). Used by the dedup: a legacy entry is dropped only when
    /// an EVENT carries identical content (the legacy/event overlap), but two
    /// distinct events are NEVER deduped against each other — two legitimate
    /// transitions to the same state in the same second by the same actor are
    /// separate events and must both survive.
    is_legacy: bool,
    /// Stable tiebreak when two entries share the same `at`. Event files use
    /// their filename (which leads with a high-resolution write-time prefix, so
    /// equal-`at` events sort in causal write order); legacy-array entries use
    /// their array index (rendered so they sort before event files at equal
    /// timestamps — legacy is "older").
    tiebreak: String,
    /// Machine-readable event-type discriminator carried through the fold so
    /// `has_open_begin` can skip an adoption transition. `None` for ordinary
    /// transitions.
    event_type: Option<String>,
    /// The reviewer verdict carried through the fold. Written on the event
    /// record since the carry-forward slice and dropped here until now — the
    /// step that recorded a verdict and the actor who recorded it are the same
    /// step, so folding one without the other left every catch invisible to
    /// every reader. `None` for a step nobody reviewed, and for every legacy
    /// array row (the legacy shape has no such column).
    satisfaction: Option<String>,
}

fn legacy_to_fold(t: &StatusTransition, index: usize) -> FoldEntry {
    FoldEntry {
        to: t.to.clone(),
        at: t.at.clone().unwrap_or_default(),
        actor: t.actor.clone().unwrap_or_default(),
        role: t.role.clone().unwrap_or_default(),
        approver: t.approver.clone(),
        note: t.note.clone(),
        is_legacy: true,
        // Legacy entries sort before event files at an equal `at` (the empty
        // string sorts before any non-empty filename), with index keeping
        // intra-array order stable.
        tiebreak: format!("\x00legacy:{:08}", index),
        event_type: t.event_type.clone(),
        // The legacy `status.yaml` array predates reviewer verdicts and does
        // not carry the column, so in practice this is `None` — but it is
        // CARRIED, not dropped. A fold that discarded it to make a comment true
        // would delete a recorded verdict to preserve a claim about the data,
        // and `transition_verdict_fold.feature` pins the carry with a legacy row
        // that does have one rather than resting on serde's default.
        satisfaction: t.satisfaction.clone(),
    }
}

fn event_to_fold(ef: &EventFile) -> FoldEntry {
    FoldEntry {
        to: ef.record.to.clone(),
        at: ef.record.at.clone(),
        actor: ef.record.actor.clone(),
        role: ef.record.role.clone(),
        approver: ef.record.approver.clone(),
        note: ef.record.note.clone(),
        is_legacy: false,
        tiebreak: ef.file_name.clone(),
        event_type: ef.record.event_type.clone(),
        satisfaction: ef.record.satisfaction.clone(),
    }
}

fn fold_to_status_transition(e: &FoldEntry) -> StatusTransition {
    StatusTransition {
        to: e.to.clone(),
        at: if e.at.is_empty() {
            None
        } else {
            Some(e.at.clone())
        },
        actor: if e.actor.is_empty() {
            None
        } else {
            Some(e.actor.clone())
        },
        role: if e.role.is_empty() {
            None
        } else {
            Some(e.role.clone())
        },
        approver: e.approver.clone(),
        note: e.note.clone(),
        event_type: e.event_type.clone(),
        // An empty verdict is NO verdict. `Some("")` would satisfy a reader
        // that only asks whether the field is present, and every unreviewed
        // step in the product would then read as a verdict nobody rendered.
        satisfaction: e
            .satisfaction
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string),
    }
}

/// Fold the legacy array and the event files into one ordered history,
/// oldest → newest, deduping exact (to, at, actor, role) duplicates. The
/// result is independent of the filesystem read order of the event files.
///
/// ORDERING (impl-review MEDIUM-1): the primary key is the record's `at`
/// (second granularity); the per-event filename (nanosecond write-time prefix +
/// actor + random) is the deterministic tiebreak. Two DIFFERENT actors
/// transitioning the same artifact within the same wall-clock second therefore
/// resolve LAST-WRITER-WINS by write-time order — accepted semantics: both
/// transitions were valid from the prior state, BOTH event files are preserved
/// (zero history loss), and the projected "current state" is deterministic given
/// the files on disk. Carrying sub-second precision in `at` itself (so the primary
/// key disambiguates without relying on the tiebreak) is a future hardening for
/// the multi-actor production target; it is not required for conflict-freedom.
pub fn fold_transitions(
    legacy: &[StatusTransition],
    events: &[EventFile],
) -> Vec<StatusTransition> {
    let mut entries: Vec<FoldEntry> = Vec::new();
    for (i, t) in legacy.iter().enumerate() {
        entries.push(legacy_to_fold(t, i));
    }
    for ef in events {
        entries.push(event_to_fold(ef));
    }

    entries.sort_by(|a, b| a.at.cmp(&b.at).then(a.tiebreak.cmp(&b.tiebreak)));

    // Dedup ONLY the legacy/event overlap: drop a LEGACY entry whose identical
    // content (to, at, actor, role) was also written as an EVENT (e.g. when a
    // legacy array is later seeded into the event dir). Two distinct EVENTS are
    // never collapsed — back-to-back transitions to the same state in the same
    // second by the same actor are legitimately separate and must both survive,
    // or the fold would lose causal history (and the latest-state read).
    let event_keys: Vec<(String, String, String, String)> = entries
        .iter()
        .filter(|e| !e.is_legacy)
        .map(|e| (e.to.clone(), e.at.clone(), e.actor.clone(), e.role.clone()))
        .collect();
    let deduped: Vec<FoldEntry> = entries
        .into_iter()
        .filter(|e| {
            if !e.is_legacy {
                return true;
            }
            let key = (e.to.clone(), e.at.clone(), e.actor.clone(), e.role.clone());
            !event_keys.contains(&key)
        })
        .collect();

    deduped.iter().map(fold_to_status_transition).collect()
}

/// The folded current state (R3): the `to` of the latest entry in the folded
/// history, falling back to the top-level `state:` field ONLY when there is no
/// transition at all (legacy or event).
///
/// This deliberately INVERTS the legacy precedence (which preferred the
/// denormalized top-level `state:` cache), and the inversion stays load-bearing
/// even now that the write path re-projects that header on every transition
/// (`anvil_core_hearth::status_header`). The header is a PROJECTION, not a source: a
/// hand-edit, a botched git merge, or an interrupted write can leave it
/// disagreeing with the events, and the events are the evidence. Trusting the
/// header would make exactly those cases silently authoritative — instead they
/// are detected and repaired (`reconcile_status_header`). The top-level field
/// survives only as a dual-read fallback for a legacy artifact that has a
/// `state:` but no transition history at all.
pub fn fold_state(status: &FullStatusYaml, events: &[EventFile]) -> Option<String> {
    let legacy = resolve_transitions(status);
    let history = fold_transitions(&legacy, events);
    history
        .last()
        .map(|t| t.to.clone())
        .or_else(|| status.state.clone())
}

/// Convenience: read the event files under `artifact_dir` and fold them with
/// the parsed status to resolve the current state. Used by every fs read site
/// so the dual-read merge is single-sourced.
///
/// **C-d.1 round 8, H-1.** This returns a `Result` now. The old signature could
/// not tell "there are no events" from "I could not read the events", and every
/// caller therefore reported the second as the first — including
/// `read_artifact_state`, which answered `Ok(implementing)` for an artifact that
/// is `reviewing`. The `Option` that remains is the honest one: `None` means the
/// artifact genuinely resolves to no state (no `state:` and no transition), and
/// that is still `MalformedStatus` at the callers.
pub fn resolve_state_with_events(
    status: &FullStatusYaml,
    artifact_dir: &Path,
) -> Result<Option<String>, TransitionEvidenceError> {
    Ok(fold_state(status, &read_event_files(artifact_dir)?))
}

/// Convenience: read the event files under `artifact_dir` and fold them with
/// the parsed status to resolve the ordered history.
pub fn resolve_transitions_with_events(
    status: &FullStatusYaml,
    artifact_dir: &Path,
) -> Result<Vec<StatusTransition>, TransitionEvidenceError> {
    Ok(fold_transitions(
        &resolve_transitions(status),
        &read_event_files(artifact_dir)?,
    ))
}

/// STRICT counterpart to [`resolve_transitions_with_events`] for the adoption
/// path: reads the on-disk event store via [`read_event_files_strict`] (fail
/// closed on damage) and folds it with the parsed status. Used ONLY where a
/// silent skip would be unsafe — an adoption reset must see the COMPLETE
/// governance history or refuse.
pub fn resolve_transitions_with_events_strict(
    status: &FullStatusYaml,
    artifact_dir: &Path,
) -> Result<Vec<StatusTransition>, TransitionEvidenceError> {
    let events = read_event_files_strict(artifact_dir)?;
    Ok(fold_transitions(&resolve_transitions(status), &events))
}
