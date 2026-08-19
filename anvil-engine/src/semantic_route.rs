//! The SEMANTIC ROUTE RPC lever (dark-launch) — the engine's Route RPC can serve
//! the Kiln router's verdict instead of the pure lexical `resolve_route` result,
//! gated behind `ANVIL_SEMANTIC_ROUTE_RPC` (default OFF) and always failing OPEN
//! to the lexical resolution.
//!
//! This module holds only the PURE, engine-binary-only pieces: the dark-launch
//! gate and the verdict→resolution fold. The time-boxed Kiln HTTP call lives in
//! [`crate::kiln_router`]; the async-boundary wiring (offloading the blocking
//! call off the async worker) lives in `main.rs`. Nothing here touches
//! `anvil-core` domain logic — it consumes anvil-core's `RouteResolution` /
//! `RouterVerdict` value types only, so the core stays LLM-free.

use anvil_core::domain::hooks::route_turn::RouterVerdict;
use anvil_core::domain::playbook::registry::{
    RouteAbstentionReason, RouteOutcome, RouteResolution,
};
use std::collections::BTreeMap;

/// The dark-launch gate for the semantic Route RPC. Resolved with the SAME
/// precedence as the other router knobs (ENV `ANVIL_SEMANTIC_ROUTE_RPC` >
/// `~/.anvil/router.json` `semantic_route_rpc` > built-in default), so it honors
/// the hermetic-test `ANVIL_ROUTER_CONFIG_FILE` override too. Distinct from the
/// hook's `ANVIL_ROUTER_ENABLED` kill switch: this gates ONLY the RPC's semantic
/// path. Default OFF — enabled ONLY when the resolved value is `on` / `1` / `true`.
pub fn semantic_route_rpc_enabled() -> bool {
    match crate::kiln_router::router_config("ANVIL_SEMANTIC_ROUTE_RPC", "semantic_route_rpc") {
        Some(v) => {
            let v = v.trim();
            v.eq_ignore_ascii_case("on") || v.eq_ignore_ascii_case("true") || v == "1"
        }
        None => false,
    }
}

/// The V1 semantic-gate breadth experiment. Uses the router configuration
/// precedence and is default OFF.
pub fn router_v1_gate_breadth_enabled() -> bool {
    match crate::kiln_router::router_config(
        "ANVIL_ROUTER_V1_GATE_BREADTH",
        "router_v1_gate_breadth",
    ) {
        Some(v) => {
            let v = v.trim();
            v.eq_ignore_ascii_case("on") || v.eq_ignore_ascii_case("true") || v == "1"
        }
        None => false,
    }
}

/// The configured V2 brief cap. Unset means the experiment is off and all
/// granted briefs retain their existing order.
pub fn router_v2_brief_cap() -> Option<String> {
    crate::kiln_router::router_config("ANVIL_ROUTER_V2_BRIEF_CAP", "router_v2_brief_cap")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticRoutePlan {
    pub eligible: bool,
    pub brief_kinds: Vec<String>,
    pub variant: String,
    pub brief_cap: String,
    pub invalid_brief_cap: Option<String>,
    pub resolution: RouteResolution,
}

/// Decide whether the semantic router may run without changing the lexical
/// resolution. Existing non-empty matches remain eligible. V1 additionally
/// admits an empty match only when a full-set granted signal has non-zero
/// content overlap.
pub fn semantic_route_plan(
    resolution: &RouteResolution,
    v1_gate_breadth_enabled: bool,
) -> SemanticRoutePlan {
    semantic_route_plan_with_brief_cap(resolution, v1_gate_breadth_enabled, None)
}

/// Plan semantic eligibility and the briefs visible to the model. V2 changes
/// only brief ordering/breadth; eligibility remains entirely owned by the
/// existing lexical/V1 gate.
pub fn semantic_route_plan_with_brief_cap(
    resolution: &RouteResolution,
    v1_gate_breadth_enabled: bool,
    configured_cap: Option<&str>,
) -> SemanticRoutePlan {
    let eligible = !resolution.matching_candidates.is_empty()
        || (v1_gate_breadth_enabled
            && resolution.abstention_reason == Some(RouteAbstentionReason::FloorMiss)
            && resolution
                .full_granted_signals
                .iter()
                .any(|signal| signal.content_overlap >= 1));
    let parsed_cap = configured_cap.and_then(|raw| raw.trim().parse::<usize>().ok());
    let valid_cap = parsed_cap.filter(|cap| *cap > 0);
    let invalid_brief_cap = configured_cap
        .filter(|_| valid_cap.is_none())
        .map(ToString::to_string);
    let v2_enabled = configured_cap.is_some();
    let mut brief_kinds = if eligible {
        resolution.granted_candidates.clone()
    } else {
        Vec::new()
    };
    if eligible && valid_cap.is_some() {
        let mut signals = resolution.full_granted_signals.clone();
        signals.sort_by(|a, b| {
            b.trigger_tier
                .cmp(&a.trigger_tier)
                .then_with(|| b.content_overlap.cmp(&a.content_overlap))
                .then_with(|| a.kind.cmp(&b.kind))
        });
        brief_kinds = signals.into_iter().map(|signal| signal.kind).collect();
        brief_kinds.truncate(valid_cap.expect("checked above"));
    }
    let variant = match (v1_gate_breadth_enabled, v2_enabled) {
        (false, false) => "control",
        (true, false) => "v1_gate_breadth",
        (false, true) => "v2_brief_breadth",
        (true, true) => "v1_gate_breadth+v2_brief_breadth",
    };
    SemanticRoutePlan {
        eligible,
        brief_kinds,
        variant: variant.to_string(),
        brief_cap: valid_cap
            .map(|cap| cap.to_string())
            .unwrap_or_else(|| "all".to_string()),
        invalid_brief_cap,
        resolution: resolution.clone(),
    }
}

/// Fold the Kiln router's verdict into the lexical resolution (PURE, directly
/// unit-testable). The mapping:
///
/// - `Pick{kind}` where `kind` ∈ the authorized brief set → `Single` on that kind
///   (`selected_kind = kind`, `matching_candidates = [kind]`), retaining only that
///   kind's match signal.
/// - `Pick{kind}` where `kind` ∉ the authorized brief set → `NoMatch` (never honor
///   a kind the model was not shown).
/// - `Abstain` → `NoMatch` (the precision win).
/// - `Fallback` → the lexical `resolution` UNCHANGED (fail-open). The RPC is a
///   shared front door and must NOT degrade to "never route" on a Kiln blip — the
///   deliberate divergence from the hook, which goes silent on `Fallback`.
///
/// `granted_candidates` is always preserved (telemetry parity); only the matching
/// set / selected kind / outcome / signals are narrowed.
pub fn apply_semantic_verdict(
    resolution: RouteResolution,
    verdict: RouterVerdict,
) -> RouteResolution {
    let authorized_kinds = resolution.granted_candidates.clone();
    apply_semantic_verdict_with_briefs(resolution, verdict, &authorized_kinds)
}

/// Fold a semantic verdict while treating the exact briefs shown to the model
/// as the authorization boundary.
pub fn apply_semantic_verdict_with_briefs(
    resolution: RouteResolution,
    verdict: RouterVerdict,
    authorized_kinds: &[String],
) -> RouteResolution {
    match verdict {
        // Fail-open: keep today's lexical behavior on any Kiln miss.
        RouterVerdict::Fallback => resolution,
        RouterVerdict::Abstain => no_match(resolution),
        RouterVerdict::Pick { kind, .. } => {
            let kind = kind.trim().to_string();
            // Validate against the exact brief set fed to the router. In control/V1
            // this is the granted set; in V2 it is the capped subset.
            if authorized_kinds.iter().any(|k| k == &kind) {
                let match_signals: BTreeMap<String, String> = resolution
                    .match_signals
                    .into_iter()
                    .filter(|(k, _)| k == &kind)
                    .collect();
                RouteResolution {
                    granted_candidates: resolution.granted_candidates,
                    full_granted_signals: resolution.full_granted_signals,
                    abstention_reason: None,
                    matching_candidates: vec![kind.clone()],
                    selected_kind: Some(kind),
                    outcome: RouteOutcome::Single,
                    match_signals,
                }
            } else {
                no_match(resolution)
            }
        }
    }
}

/// Collapse a resolution to a floor-driven `NoMatch`, preserving only the granted
/// set (telemetry parity) and clearing the matching set / selected kind / signals.
fn no_match(resolution: RouteResolution) -> RouteResolution {
    RouteResolution {
        granted_candidates: resolution.granted_candidates,
        full_granted_signals: resolution.full_granted_signals,
        abstention_reason: resolution.abstention_reason,
        matching_candidates: Vec::new(),
        selected_kind: None,
        outcome: RouteOutcome::NoMatch,
        match_signals: BTreeMap::new(),
    }
}
