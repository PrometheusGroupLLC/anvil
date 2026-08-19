//! Route-response enrichment (route_response_mirrors_begin Phase 2/3).
//!
//! Builds the annotated, budget-respecting payload the engine route RPC carries:
//! per-candidate `intent` / `step_outline` / `why_fits` for a `Candidates`
//! outcome, and the begin-equivalent `guidance` body for a `Single` outcome
//! (served via the SAME `hook_serve` seam begin reads, so the two never drift).
//!
//! Pure: it reads a `&dyn PlaybookRegistry` + a `&dyn PlaybookHookBodyPort` and
//! returns owned data. Enrichment failure (a hook-body read error) is the
//! caller's fail-open concern for the single guidance; candidate annotations
//! never read hook bodies, so they cannot fail-open.

use crate::domain::playbook::hook_serve::{serve_hook_body, PlaybookHookBodyPort};
use crate::domain::playbook::interpreter::state_role_measurement;
use crate::domain::playbook::registry::{PlaybookRegistry, RouteResolution};
use crate::ports::query_port::QueryError;

/// Deterministic ceiling (in bytes) on the assembled candidate-annotation
/// payload for a `Candidates` route response. The single-guidance body is capped
/// by the per-body `HOOK_CONTEXT_BUDGET_BYTES` at the seam; THIS budget bounds
/// the candidate set's summaries (intent + step_outline + why_fits) so a large
/// candidate set can never produce an unbounded payload. When the assembled set
/// exceeds this, the response truncates per the defined fallback: drop
/// `step_outline` first, then collapse to kind + description.
pub const ROUTE_RESPONSE_BUDGET_BYTES: usize = 8_192;

/// One annotated candidate for a `Candidates` route response.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AnnotatedCandidate {
    pub kind: String,
    pub description: String,
    pub required_fields: Vec<String>,
    /// The (initial_state, doer) MeasurementSpec.intent, or the machine
    /// description when the kind declares no such spec.
    pub intent: String,
    /// The machine's state names in order.
    pub step_outline: Vec<String>,
    /// The concrete match signal (matched trigger / overlap term) — cites the
    /// actual signal from the resolution. Empty when the kind is not a matching
    /// candidate (only matching candidates are annotated as matched).
    pub why_fits: String,
}

impl AnnotatedCandidate {
    /// The total byte size this candidate ACTUALLY contributes to the serialized
    /// route response (H2): every field the engine puts on the wire — kind +
    /// description + intent + why_fits + every step_outline entry. The budget is
    /// measured against the real serialized payload, not just the droppable
    /// fields, so the truncation fallback genuinely keeps the total within
    /// `ROUTE_RESPONSE_BUDGET_BYTES`. Kind + description are the irreducible floor
    /// (they remain after the fallback collapses everything else), but they still
    /// count toward the total so the accounting reflects what is sent.
    fn serialized_bytes(&self) -> usize {
        self.kind.len()
            + self.description.len()
            + self.intent.len()
            + self.why_fits.len()
            + self.step_outline.iter().map(String::len).sum::<usize>()
    }
}

/// The initial state of a machine is the first declared state (first-state
/// convention, shared with begin). Returns the state names in declared order.
fn step_outline(machine: &crate::domain::playbook::types::PlaybookMachine) -> Vec<String> {
    machine.states.iter().map(|s| s.name.clone()).collect()
}

/// The candidate's intent: the (initial_state, doer) MeasurementSpec.intent, or
/// the machine description as the fallback when no such spec is declared.
fn candidate_intent(machine: &crate::domain::playbook::types::PlaybookMachine) -> String {
    let initial_state = machine.states.first().map(|s| s.name.as_str());
    initial_state
        .and_then(|state| state_role_measurement(machine, state, "doer"))
        .map(|spec| spec.intent.clone())
        .filter(|intent| !intent.trim().is_empty())
        .unwrap_or_else(|| machine.description.clone())
}

/// Build the annotated candidate set for a route resolution's matching
/// candidates (H3: annotate the MATCHING set, not merely granted — a
/// granted-but-not-matching playbook carries no honest signal and is not
/// surfaced as a matched candidate). Candidates are returned in the resolution's
/// matching order; the budget truncation is applied last by `apply_budget`.
pub fn annotate_candidates(
    registry: &dyn PlaybookRegistry,
    resolution: &RouteResolution,
) -> Vec<AnnotatedCandidate> {
    resolution
        .matching_candidates
        .iter()
        .filter_map(|kind| {
            registry
                .machine_for(kind)
                .map(|machine| AnnotatedCandidate {
                    kind: machine.kind.clone(),
                    description: machine.description.clone(),
                    required_fields: machine
                        .required_fields
                        .iter()
                        .map(|f| f.name.clone())
                        .collect(),
                    intent: candidate_intent(machine),
                    step_outline: step_outline(machine),
                    why_fits: resolution
                        .match_signals
                        .get(kind)
                        .cloned()
                        .unwrap_or_default(),
                })
        })
        .collect()
}

/// Apply the route-response budget to an annotated candidate set. The budget is
/// measured against the bytes ACTUALLY serialized per candidate (H2:
/// `serialized_bytes` = kind + description + intent + step_outline + why_fits).
/// Deterministic, observable fallback when the assembled payload exceeds
/// `ROUTE_RESPONSE_BUDGET_BYTES`: FIRST drop every `step_outline` (the heaviest,
/// least essential annotation); if still over, collapse to kind + description
/// (clear intent + why_fits too) — the irreducible floor. Under budget →
/// returned unchanged.
pub fn apply_budget(mut candidates: Vec<AnnotatedCandidate>) -> Vec<AnnotatedCandidate> {
    let total = |cs: &[AnnotatedCandidate]| {
        cs.iter()
            .map(AnnotatedCandidate::serialized_bytes)
            .sum::<usize>()
    };

    if total(&candidates) <= ROUTE_RESPONSE_BUDGET_BYTES {
        return candidates;
    }
    // Step 1: drop step_outline first.
    for c in &mut candidates {
        c.step_outline.clear();
    }
    if total(&candidates) <= ROUTE_RESPONSE_BUDGET_BYTES {
        return candidates;
    }
    // Step 2: collapse to kind + description (the irreducible floor).
    for c in &mut candidates {
        c.intent.clear();
        c.why_fits.clear();
    }
    candidates
}

/// Serve the begin-equivalent `guidance` body for a SINGLE route resolution: the
/// resolved + budget-capped, PRE-interpolation hook body for the selected kind's
/// initial (state, doer), via the SAME `hook_serve` seam begin reads. Returns an
/// empty string when there is no selected kind or the kind/state declares no hook
/// (absence is not an error). A read failure surfaces as `Err` so the engine can
/// fail open (degrade to the thin response) without turning a successful route
/// into a tool error.
pub fn single_guidance(
    hook_reader: &dyn PlaybookHookBodyPort,
    registry: &dyn PlaybookRegistry,
    resolution: &RouteResolution,
) -> Result<String, QueryError> {
    let Some(kind) = resolution.selected_kind.as_deref() else {
        return Ok(String::new());
    };
    let Some(machine) = registry.machine_for(kind) else {
        return Ok(String::new());
    };
    let Some(initial_state) = machine.states.first().map(|s| s.name.clone()) else {
        return Ok(String::new());
    };
    serve_hook_body(hook_reader, registry, kind, &initial_state, "doer")
}
