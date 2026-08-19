//! Machine-derived `next_step` generator.
//!
//! `next_step_for(machine, state)` synthesizes engine-native guidance for ANY
//! playbook by reading the OUTGOING transitions of `state` from the machine —
//! never a hardcoded per-playbook string table. This makes the engine's
//! `begin`/`complete` next_step generic across every registered playbook
//! (track, proposal, decision, learning, measurement, knowledge, lore_query,
//! and any hearth- or kit-authored machine), not just `track_lifecycle`.
//!
//! The guidance is ALWAYS skill-free: it directs the actor to call the engine's
//! `complete`/`snapshot`/`begin` RPCs, never a retired forge skill. The engine
//! drives the lifecycle via the pull model — each `complete` advances the state
//! so the next phase's hook is served on the next `begin`.
//!
//! Pure function: no I/O, no global state. Mirrors `interpreter.rs`.

use crate::domain::playbook::interpreter::outgoing_transitions;
use crate::domain::playbook::types::PlaybookMachine;

/// Synthesize the `next_step` guidance for `state` in `machine`.
///
/// Behavior, derived entirely from the machine:
/// - **Terminal state** (declared `is_terminal`, or a state with no outgoing
///   transitions): a short "this artifact is complete" note.
/// - **Review-gate state**, or any state whose outgoing transitions carry
///   `required_satisfaction`: enumerate the satisfaction options and the
///   `to_state` each advances to, directing a `complete(..., satisfaction: ...)`.
/// - **Doer state with one plain outgoing transition** (no
///   `required_satisfaction`): direct the doer to write the artifact then call
///   `complete` to advance to the single `to_state`; the engine serves the next
///   phase's hook on the next `begin`. Do NOT invoke a skill.
/// - **Doer state with multiple plain transitions**: list the reachable
///   `to_state`s and note that `complete`/`snapshot` advances.
///
/// Returns a NON-EMPTY string for every non-terminal state of every playbook.
pub fn next_step_for(machine: &PlaybookMachine, state: &str) -> String {
    let outgoing = outgoing_transitions(machine, state);

    // Terminal: declared is_terminal OR no outgoing transitions at all.
    let is_terminal_state = machine
        .states
        .iter()
        .find(|s| s.name == state)
        .map(|s| s.is_terminal)
        .unwrap_or(false);
    if is_terminal_state || outgoing.is_empty() {
        return format!(
            "This artifact has reached its terminal state ({state}); no further \
             playbook steps remain. It is complete."
        );
    }

    let is_review_gate = machine
        .states
        .iter()
        .find(|s| s.name == state)
        .map(|s| s.is_review_gate)
        .unwrap_or(false);

    // resume-signal context-awareness: the machine-declared `→ abandoned` PARK
    // edge is a snapshot-driven escape hatch, NOT part of the forward
    // complete-flow. Exclude it from the next-step guidance so it does not (a)
    // flip a plain doer state into the satisfaction/review-gate shape, nor (b)
    // pollute the enumerated verdicts / reachable targets. Park stays discoverable
    // via `available_actions`; it just isn't the "what to do next" instruction.
    let forward: Vec<_> = outgoing
        .iter()
        .filter(|t| t.to_state != "abandoned")
        .cloned()
        .collect();

    // Any FORWARD transition carrying required_satisfaction makes this a
    // satisfaction-gated state (the review-gate shape), even if the state flag
    // is not set — the edge selector keys on satisfaction.
    let has_satisfaction = forward.iter().any(|t| t.required_satisfaction.is_some());

    if is_review_gate || has_satisfaction {
        // Enumerate each satisfaction value → to_state across the gated edges.
        let mut options: Vec<String> = Vec::new();
        for transition in &forward {
            if let Some(satisfactions) = &transition.required_satisfaction {
                for satisfaction in satisfactions {
                    options.push(format!(
                        "`\"{}\"` \u{2192} {}",
                        satisfaction, transition.to_state
                    ));
                }
            }
        }
        if options.is_empty() {
            // Defensive: a review gate with no satisfaction edges — list targets.
            return plain_transition_guidance(&forward, state);
        }
        return format!(
            "Review context delivered for {state}. Write your review, then call \
             `complete(artifact_path, satisfaction: \"<value>\", actor_*)` to record \
             your verdict \u{2014} {}. Check the `execution_route` field on the \
             available action to confirm engine routing before calling.",
            options.join(", ")
        );
    }

    // Non-review-gate doer state (forward edges only — park excluded).
    if forward.len() == 1 {
        let to_state = &forward[0].to_state;
        return format!(
            "Context delivered for {state}. Write this state's artifact, then call \
             `complete(artifact_path, actor_*)` (no satisfaction) to advance to \
             {to_state} \u{2014} the engine serves the next phase's hook on the next \
             `begin(identifier)`. Do NOT invoke a skill; the engine drives the \
             lifecycle. Check the `execution_route` field on the available action \
             to confirm engine routing before calling."
        );
    }

    plain_transition_guidance(&forward, state)
}

/// Guidance for a state with multiple plain (non-satisfaction) transitions:
/// list the reachable `to_state`s and note that `complete`/`snapshot` advances.
fn plain_transition_guidance(
    outgoing: &[crate::domain::playbook::interpreter::OutgoingTransition],
    state: &str,
) -> String {
    let targets: Vec<String> = outgoing.iter().map(|t| t.to_state.clone()).collect();
    format!(
        "Context delivered for {state}. Write this state's artifact, then advance with \
         `complete(artifact_path, actor_*)` (or `snapshot` for a chosen target) \u{2014} \
         reachable states: {}. The engine serves the next phase's hook on the next \
         `begin(identifier)`. Do NOT invoke a skill; the engine drives the lifecycle. \
         Check the `execution_route` field on the available action to confirm engine \
         routing before calling.",
        targets.join(", ")
    )
}
