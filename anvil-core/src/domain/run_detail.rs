//! A run's own record: the steps it took, the actors who took them, and the
//! runs that were started from inside it.
//!
//! ## Why this exists as a fold rather than as a query each surface writes
//!
//! Every surface that shows someone "what happened in this run" needs the same
//! four things — the ordered steps, who took each one, who approved the ones
//! that needed approving, and the work that was spawned from inside it. The
//! moment two surfaces each derive that for themselves they begin to disagree,
//! and the disagreement is invisible: both are reading the same files, so both
//! look right. The engine therefore reads the artifacts and hands the already-
//! read records HERE, and this single fold decides the shape. The gRPC
//! `RunDetail` RPC and the `/ws` `run_detail` JSON-RPC method both go through
//! it, so neither can drift from the other.
//!
//! ## What is authoritative, and what this fold is NOT allowed to re-derive
//!
//! An artifact's history lives in the per-file event store under
//! `<artifact_dir>/transitions/`, merged with the legacy `status.yaml`
//! `transitions:` array. That merge is already implemented, once, in
//! [`crate::domain::transition_log`] — `read_event_files` +
//! `resolve_transitions_with_events`. This module deliberately does NOT fold
//! event files. It takes the ALREADY-FOLDED `Vec<StatusTransition>` as input
//! and only shapes it. A second fold here would be a second opinion about what
//! happened, which is the exact failure the module exists to prevent.
//!
//! ## THERE IS NO COST FIGURE IN ANVIL, SO THIS RECORD CARRIES NONE
//!
//! Nothing in this engine records what a step cost. There is no spend field, no
//! token count, no price, and no autonomy rung anywhere on an artifact — not in
//! `status.yaml`, not in a transition event, not in the step-measurement sink.
//! A `cost: 0.0` on [`RunStep`] would therefore not be a missing number: it
//! would be a FABRICATED one, and it would read to a person as "this step was
//! free". The types below carry no such field, and
//! `anvil-core/features/run_detail.feature` asserts the absence structurally
//! (it serializes a folded record and fails on any cost-shaped key) so a future
//! addition cannot slip in unnoticed.
//!
//! CORRECTED 2026-08-14, AND THE REFUSAL STANDS. "Nothing in this engine
//! records what a step cost" is broader than what is true: a per-run
//! `total_cost_usd` and `total_tokens` are parsed from temper's per-instance
//! scorecards in `anvil-engine/src/scorecard_reader.rs`, both `#[serde(default)]`,
//! so an absent key already becomes a `0.0` inside this process. They are
//! dropped before any summary and have never reached a wire response. This
//! record still carries none. The correction matters because the overclaim
//! would tell a reader that a cost figure must be invented from nothing, when
//! the real question is whether a figure this engine RECEIVES from a caller may
//! be shown as if this engine measured it. See
//! [`crate::domain::autonomy_evidence`].
//!
//! ## Absent is empty, never a placeholder
//!
//! A transition record carries `at`, `actor`, `role`, `approver` and `note`
//! optionally: a step nobody had to approve simply has no approver. Every such
//! field flattens to the EMPTY STRING here. It never becomes "unknown",
//! "n/a", "system", or any other invented value — an empty string is a surface's
//! cue to render nothing, whereas a placeholder is a claim.

use crate::domain::status::StatusTransition;
use serde::Serialize;
use std::collections::HashMap;

/// One step a run took, exactly as the folded transition record carries it.
///
/// Every field is a plain `String` and EMPTY means the record does not carry
/// it (see the module doc). There is no cost field — see the module doc for
/// why that absence is deliberate.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RunStep {
    /// The state the run moved INTO.
    pub to_state: String,
    /// When it happened, as recorded. Empty when the record carries no time.
    pub at: String,
    /// The actor who took the step. Empty when the record names none.
    pub actor: String,
    /// The part that actor was playing (doer, reviewer, …). Empty when absent.
    pub role: String,
    /// The person who approved the step. EMPTY for the ordinary case — most
    /// steps require no approval, and saying so is different from naming
    /// somebody.
    pub approver: String,
    /// The note recorded with the step. Empty when none was written.
    pub note: String,
    /// The reviewer verdict recorded on the step. EMPTY for the ordinary case —
    /// most steps carry no verdict, and saying so is different from claiming an
    /// approval nobody rendered.
    ///
    /// It is here because "what it got wrong, and WHO caught it" needs the
    /// verdict and the actor on the same row, and this record is the one every
    /// surface reads a run's history from. Carrying the actor without the
    /// verdict is what left every catch invisible.
    pub verdict: String,
}

/// One actor from a run's `actors:` table.
///
/// `model` / `provider` are read from the actor's MOST RECENT `configurations`
/// entry — the table appends a new entry whenever an agent returns under a
/// different configuration, so the last one is the configuration that is
/// current. A human actor carries no configurations at all, and a table may
/// simply not record them; in both cases these are empty, never guessed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RunActor {
    /// The actor's name, as keyed in the `actors:` table.
    pub name: String,
    /// The table's `type` for this actor (e.g. `agent`, `human`). Empty when
    /// the table does not say.
    pub actor_type: String,
    /// The model from the most recent configuration. Empty when the table
    /// carries no configuration (a human, or an unconfigured agent).
    pub model: String,
    /// The provider from the most recent configuration. Empty when absent.
    pub provider: String,
}

/// One run in the record: the run that was asked for, or a run started from
/// inside it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RunNode {
    /// The artifact directory id — the run's identity.
    pub instance_id: String,
    /// The artifact's `kind` from `status.yaml`.
    pub kind: String,
    /// The FOLDED current state (status.yaml header + transition events),
    /// resolved by the engine before it hands the input here.
    pub state: String,
    /// The on-disk artifact directory, so a surface can open the real work.
    pub artifact_dir: String,
    /// The run this one was started from inside. EMPTY for the run that was
    /// asked for — it is the root of this record and is nested under nothing.
    pub parent_instance_id: String,
    /// `0` for the run that was asked for, `1` for a run started from inside
    /// it.
    pub depth: u32,
    /// The run's own steps, oldest first.
    pub steps: Vec<RunStep>,
    /// The run's own actors, ordered by name so the record is stable across
    /// reads (the `actors:` table is a map and has no inherent order).
    pub actors: Vec<RunActor>,
}

/// One artifact the engine has ALREADY read off disk, ready to be shaped.
///
/// The engine owns every syscall: it locates the artifact directory, parses
/// `status.yaml`, and folds the transition event store through
/// [`crate::domain::transition_log`]. This fold performs no I/O, so it is
/// deterministic and testable against literal inputs.
#[derive(Debug, Clone, Default)]
pub struct RunDetailInput {
    pub instance_id: String,
    pub kind: String,
    /// The FOLDED current state, resolved by the caller.
    pub state: String,
    pub artifact_dir: String,
    /// The artifact's `parent_id` from `status.yaml`. Used by the caller to
    /// decide which artifacts are children; carried here for completeness.
    pub parent_id: String,
    /// The ALREADY-FOLDED ordered history — never re-derived here.
    pub transitions: Vec<StatusTransition>,
    /// The raw `actors:` table, exactly as `status.yaml` carries it.
    pub actors: Option<HashMap<String, serde_yaml::Value>>,
}

/// Flatten the folded transition history into the run's steps, preserving the
/// fold's order (oldest first). Optional fields flatten to the empty string.
pub fn fold_run_steps(transitions: &[StatusTransition]) -> Vec<RunStep> {
    transitions
        .iter()
        .map(|t| RunStep {
            to_state: t.to.clone(),
            at: t.at.clone().unwrap_or_default(),
            actor: t.actor.clone().unwrap_or_default(),
            role: t.role.clone().unwrap_or_default(),
            approver: t.approver.clone().unwrap_or_default(),
            note: t.note.clone().unwrap_or_default(),
            verdict: t.satisfaction.clone().unwrap_or_default(),
        })
        .collect()
}

/// Read one actor's `type` and its most recent `model` / `provider`.
///
/// `configurations` is an append-ordered list, so the LAST entry is the current
/// configuration. Anything the table does not carry comes back empty — this
/// function never substitutes a default model or provider.
fn fold_one_actor(name: &str, value: &serde_yaml::Value) -> RunActor {
    let latest = value
        .get("configurations")
        .and_then(|c| c.as_sequence())
        .and_then(|s| s.last());
    let field = |key: &str| {
        latest
            .and_then(|entry| entry.get(key))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    };
    RunActor {
        name: name.to_string(),
        actor_type: value
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        model: field("model"),
        provider: field("provider"),
    }
}

/// Fold an `actors:` table into an ordered actor list.
///
/// Ordered by NAME: the table is a map, so iteration order is arbitrary, and a
/// record whose actors reshuffle between two reads of the same unchanged
/// artifact would read as churn that did not happen.
pub fn fold_run_actors(actors: Option<&HashMap<String, serde_yaml::Value>>) -> Vec<RunActor> {
    let Some(table) = actors else {
        return Vec::new();
    };
    let mut out: Vec<RunActor> = table
        .iter()
        .map(|(name, value)| fold_one_actor(name, value))
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The key a nested run is ordered by: when its first step happened, then its
/// own id.
///
/// A run that has taken NO step yet has no time to be ordered by, so it sorts
/// first (the empty string precedes every timestamp) with its id breaking the
/// tie. Any position for it is arbitrary; what matters is that the position is
/// the SAME on every read of an unchanged hearth.
fn child_order_key(input: &RunDetailInput) -> (String, String) {
    let first_at = input
        .transitions
        .first()
        .and_then(|t| t.at.clone())
        .unwrap_or_default();
    (first_at, input.instance_id.clone())
}

/// Shape one run's record: the run that was asked for FIRST, then every run
/// started from inside it.
///
/// * `root` is at depth `0` with an EMPTY `parent_instance_id` — it is what was
///   asked for, and it is nested under nothing.
/// * every element of `children` is at depth `1`, carries `root`'s id as its
///   parent, and carries its OWN steps and actors.
/// * children are ordered by their first step's time, ascending, ties broken by
///   `instance_id` — the order in which the work was started.
///
/// SELECTION IS THE CALLER'S, SHAPING IS THIS FUNCTION'S. The engine decides
/// which artifacts are children (those whose `status.yaml` `parent_id` is the
/// root's id) because that decision needs the filesystem; this fold takes the
/// set as given and never re-filters it. Which set the engine actually passes is
/// pinned where it can be observed — against a real hearth, in
/// `anvil-engine/features/run_detail_rpc.feature`.
pub fn fold_run_detail(root: &RunDetailInput, children: &[RunDetailInput]) -> Vec<RunNode> {
    let node = |input: &RunDetailInput, parent: &str, depth: u32| RunNode {
        instance_id: input.instance_id.clone(),
        kind: input.kind.clone(),
        state: input.state.clone(),
        artifact_dir: input.artifact_dir.clone(),
        parent_instance_id: parent.to_string(),
        depth,
        steps: fold_run_steps(&input.transitions),
        actors: fold_run_actors(input.actors.as_ref()),
    };

    let mut ordered: Vec<&RunDetailInput> = children.iter().collect();
    ordered.sort_by_key(|input| child_order_key(input));

    let mut nodes = vec![node(root, "", 0)];
    nodes.extend(
        ordered
            .into_iter()
            .map(|input| node(input, &root.instance_id, 1)),
    );
    nodes
}
