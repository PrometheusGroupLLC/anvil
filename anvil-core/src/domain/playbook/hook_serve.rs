//! The shared hook-serving seam.
//!
//! `begin` and `route` both need to serve the SAME machine-declared
//! `(kind, state, role)` hook body — resolved through the `PlaybookRegistry`,
//! read via the hook-body port, and capped to a single deterministic byte
//! budget. This module is that single seam so the two callers can never drift.
//!
//! The seam returns the **pre-interpolation** capped body. `begin` applies its
//! `{{placeholder}}` interpolation AFTER calling the seam (it has create-time
//! field values); `route` has no field values, so it serves the seam's output
//! verbatim (placeholders stay literal). Equality of single-route guidance and
//! begin's served body is asserted against this pre-interpolation body.

use crate::domain::playbook::interpreter::state_role_hook;
use crate::domain::playbook::registry::{PlaybookRegistry, PlaybookSource};
use crate::ports::query_port::QueryError;

/// Deterministic ceiling (in bytes) on a single hook body served into the
/// agent's context window. Hook serving must not re-bloat the window the moment
/// it works, so every served body passes through `cap_hook_body` with this
/// single budget. A generous default — typical authored hook bodies fit
/// comfortably under it; only pathological/runaway bodies are truncated.
pub const HOOK_CONTEXT_BUDGET_BYTES: usize = 16_384;

/// The marker appended verbatim when a hook body is truncated to the budget.
/// Pinned here as the single source of truth so brine assertions and the cap
/// logic can never diverge.
pub const HOOK_TRUNCATION_MARKER: &str = "\n\n[hook content truncated to budget]";

/// The hook-body read port used by the shared seam. Implemented by the
/// engine's `QueryPort`-backed reader (and any test double).
pub trait PlaybookHookBodyPort {
    fn read_playbook_hook_body(
        &self,
        source: &PlaybookSource,
        filename: &str,
    ) -> Result<String, QueryError>;
}

/// Apply the deterministic byte budget (`HOOK_CONTEXT_BUDGET_BYTES`) to a hook
/// body served into context. Under-budget bodies are returned verbatim.
/// Over-budget bodies are truncated at the largest UTF-8 char boundary that
/// keeps the result (truncated prefix + marker) within budget, then the
/// truncation marker is appended. The returned string is always within budget
/// and the truncation is byte-deterministic for a given input.
pub fn cap_hook_body(body: String) -> String {
    if body.len() <= HOOK_CONTEXT_BUDGET_BYTES {
        return body;
    }
    let marker = HOOK_TRUNCATION_MARKER;
    // Reserve room for the marker so the final string stays within budget.
    let prefix_budget = HOOK_CONTEXT_BUDGET_BYTES.saturating_sub(marker.len());
    // Walk back to the nearest char boundary at or below prefix_budget so the
    // truncated slice is valid UTF-8 (deterministic for a given body).
    let mut cut = prefix_budget.min(body.len());
    while cut > 0 && !body.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = body[..cut].to_string();
    out.push_str(marker);
    out
}

/// Resolve the `(kind, state, role)` hook declaration via the registry and read
/// its body via the hook-body port. Returns the body (budget-capped per the
/// single shared budget), or an empty string when the kind is unknown, the
/// state declares no hook for that role, or the kind has no resolvable source
/// (absence is not an error). This is the single PRE-interpolation serving
/// point both `begin` and `route` call, so the budget cap and
/// absence-is-not-an-error semantics are inherited by both.
pub fn serve_hook_body(
    hook_reader: &dyn PlaybookHookBodyPort,
    registry: &dyn PlaybookRegistry,
    kind: &str,
    state: &str,
    role: &str,
) -> Result<String, QueryError> {
    let Some(machine) = registry.machine_for(kind) else {
        return Ok(String::new());
    };
    let Some(selection) = state_role_hook(machine, state, role) else {
        return Ok(String::new());
    };
    let Some(source) = registry.source_for(kind) else {
        return Ok(String::new());
    };
    let body = hook_reader.read_playbook_hook_body(&source, &selection.filename)?;
    Ok(cap_hook_body(body))
}
