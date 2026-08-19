//! Begin-adoption detection — the pure predicate and warning helpers
//! shared by the `complete`/`snapshot` soft-warn (BP2) and (future) the
//! `BeginAdoptionStatus` query RPC (BP3).
//!
//! Putting `has_open_begin` here as the single source of truth guarantees
//! the soft-warn and the query can never disagree (PD-3 rationale).

use crate::domain::shared_types::ActivityEntry;
use crate::domain::status::StatusTransition;
use crate::ports::query_port::{QueryError, QueryPort};

/// Whether `(actor, state)` has an OPEN begin-marker, inferred by the
/// PD-1 close-by-comparison rule (no closing `activity:` entry is ever
/// written).
///
/// Open iff there EXISTS an `activity:` entry
/// `{kind: "begin", actor == A, state == S}` whose `at == T` such that
/// there is NO `transitions:` entry with BOTH `transition.actor == A`
/// AND `transition.at >= T`. Both conditions must hold simultaneously —
/// a later transition by a *different* actor does NOT close A's marker,
/// and the comparison is inclusive (`>=`) so a same-second close still
/// closes correctly (A-2).
///
/// # Monotonicity invariant
///
/// Correctness relies on the engine stamping every `at` with
/// `chrono::Utc::now()` at routing time, so transitions are monotonic and
/// the only transition by `A` with `at >= marker.at` is a genuinely later
/// (closing) one. If back-dated or imported timestamps are ever
/// introduced this predicate must be revisited (PD-1 constraint c).
///
/// Multiple begin-markers for the same `(actor, state)` (A-1) read as
/// open if ANY of them is unclosed — the next transition closes all of
/// them. No per-marker close accounting is required.
///
/// # Adoption transitions never close a begin
///
/// A governance ADOPTION reset (`event_type == "adoption"`) lands the artifact
/// at its machine's initial state and, in the SAME begin call, opens the
/// adopting actor's begin marker. Both the reset transition and the marker are
/// stamped at second granularity, so the reset's `at` is `>=` the marker's `at`
/// by the SAME actor — under the plain close-by-comparison rule the reset would
/// instantly (and wrongly) close the just-opened begin, so an adopted artifact
/// would read as "not begun" the moment it was adopted (breaking mid-playbook
/// resume routing and polluting call-state metrics). Adoption transitions are
/// therefore EXCLUDED from the closing set: only genuine forward doer/reviewer
/// transitions close a begin. This is the semantically honest rule — a reset TO
/// the initial state is not the doer completing work AT it.
pub fn has_open_begin(
    activity: &[ActivityEntry],
    transitions: &[StatusTransition],
    actor: &str,
    state: &str,
) -> bool {
    activity.iter().any(|entry| {
        entry.kind == "begin"
            && entry.actor == actor
            && entry.state == state
            && !transitions.iter().any(|t| {
                !is_adoption_transition(t)
                    && t.actor.as_deref() == Some(actor)
                    && t.at.as_deref().is_some_and(|at| at >= entry.at.as_str())
            })
    })
}

/// Whether a transition is a governance-adoption reset (structural
/// `event_type == "adoption"` discriminator, not `note` prose). Such a
/// transition never closes a begin marker (see [`has_open_begin`]).
fn is_adoption_transition(t: &StatusTransition) -> bool {
    t.event_type.as_deref() == Some("adoption")
}

/// Whether ANY actor has an open begin-marker in `state` — the actor-AGNOSTIC
/// form of [`has_open_begin`], used by the runtime hook gate when the harness
/// does not supply an anvil actor identity. Open iff there exists a begin
/// `activity:` entry in `state` (by any actor `A`) with no later closing
/// transition by that same `A` (the same per-actor close-by-comparison rule, ORed
/// over every distinct begin actor).
pub fn has_any_open_begin(
    activity: &[ActivityEntry],
    transitions: &[StatusTransition],
    state: &str,
) -> bool {
    activity.iter().any(|entry| {
        entry.kind == "begin"
            && entry.state == state
            && !transitions.iter().any(|t| {
                !is_adoption_transition(t)
                    && t.actor.as_deref() == Some(entry.actor.as_str())
                    && t.at.as_deref().is_some_and(|at| at >= entry.at.as_str())
            })
    })
}

/// Return the most recent begin marker's stored conversation id for one
/// artifact. Empty marker conversation ids are ignored. This is intentionally
/// artifact-path keyed, not conversation keyed: complete/snapshot know the run
/// they are closing/tagging and need to inherit that run's begin conversation
/// when the caller omits an override.
pub fn open_begin_conversation_id_for_artifact(
    query: &dyn QueryPort,
    artifact_path: &str,
) -> Result<Option<String>, QueryError> {
    let activity = query.read_activity_entries(artifact_path)?;
    let mut newest: Option<(&str, &str)> = None;
    for entry in &activity {
        if entry.kind != "begin" || entry.conversation_id.trim().is_empty() {
            continue;
        }
        if newest.map(|(at, _)| entry.at.as_str() > at).unwrap_or(true) {
            newest = Some((entry.at.as_str(), entry.conversation_id.as_str()));
        }
    }
    Ok(newest.map(|(_, conversation_id)| conversation_id.to_string()))
}

/// Whether `actor` is the artifact's creating actor — the identity
/// recorded on the artifact's FIRST (creation) transition. The creating
/// actor entered via creation and is exempt from a begin-adoption warning
/// on their first `complete`/`snapshot` (D-D, actor-scoped).
pub fn creating_actor_is(transitions: &[StatusTransition], actor: &str) -> bool {
    transitions
        .first()
        .and_then(|t| t.actor.as_deref())
        .is_some_and(|first| first == actor)
}

/// The pinned begin-adoption warning string. This is the single source of
/// truth — every brine assertion across core/engine/e2e matches this exact
/// string. NEVER inline the format elsewhere; always call this function.
pub fn begin_adoption_warning(actor: &str, artifact: &str, state: &str) -> String {
    format!(
        "begin_adoption: actor {} transitioned {} in state {} without a prior begin",
        actor, artifact, state
    )
}

/// Whether `kind` belongs to the driven register the begin-adoption seam
/// applies to (track / playbook / milestone). This is the explicit kind
/// gate (F-8) — NOT `registry_section_for`, which returns `Some` for free
/// types (decision/learning) that must never be warned (D-F).
pub fn is_driven_register(kind: &str) -> bool {
    matches!(kind, "track" | "workflow" | "milestone")
}
