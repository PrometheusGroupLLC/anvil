//! Step module for the pure-mapping half of `route_rpc_semantic.feature`.
//!
//! Exercises the engine's PURE `apply_semantic_verdict` fold directly — no engine
//! process, no Kiln, no socket. Given a lexical `RouteResolution` and a
//! `RouterVerdict`, it asserts the four mapping cases (Pick∈matching → Single,
//! Pick∉matching → NoMatch, Abstain → NoMatch, Fallback → lexical unchanged).
//!
//! The gate + Kiln-fail-open behavior is exercised end-to-end against a real
//! engine by the RPC scenarios in the same feature (engine.rs steps).

use anvil_core::domain::hooks::route_turn::RouterVerdict;
use anvil_core::domain::playbook::registry::{
    RouteAbstentionReason, RouteCandidateSignal, RouteOutcome, RouteResolution,
};
use anvil_engine::semantic_route::{
    apply_semantic_verdict, apply_semantic_verdict_with_briefs, semantic_route_plan,
    semantic_route_plan_with_brief_cap, SemanticRoutePlan,
};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::collections::BTreeMap;

const LEXICAL_KEY: &str = "srr_lexical";
const RESULT_KEY: &str = "srr_result";
const V1_KEY: &str = "srr_v1_enabled";
const V2_CAP_KEY: &str = "srr_v2_cap";
const PLAN_KEY: &str = "srr_plan";
const REPEATED_PLAN_KEY: &str = "srr_repeated_plan";

fn parse_csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn result(ctx: &Context) -> Result<&RouteResolution, String> {
    ctx.get::<RouteResolution>(RESULT_KEY)
        .ok_or_else(|| "No semantic resolution result".to_string())
}

fn expected_outcome(value: &str) -> Result<RouteOutcome, String> {
    match value {
        "Single" => Ok(RouteOutcome::Single),
        "Candidates" => Ok(RouteOutcome::Candidates),
        "NoMatch" => Ok(RouteOutcome::NoMatch),
        other => Err(format!("Unknown route outcome '{}'", other)),
    }
}

fn parse_granted_overlaps(raw: &str) -> Result<Vec<RouteCandidateSignal>, String> {
    raw.split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let mut fields = entry.split(':');
            let kind = fields.next().ok_or("Expected kind")?.to_string();
            let trigger_tier = fields
                .next()
                .ok_or("Expected trigger tier")?
                .parse::<usize>()
                .map_err(|e| format!("Invalid trigger tier in '{}': {}", entry, e))?;
            let content_overlap = fields
                .next()
                .ok_or("Expected content overlap")?
                .parse::<usize>()
                .map_err(|e| format!("Invalid content overlap in '{}': {}", entry, e))?;
            if fields.next().is_some() {
                return Err(format!("Too many fields in granted overlap '{}'", entry));
            }
            Ok(RouteCandidateSignal {
                kind,
                trigger_tier,
                content_overlap,
            })
        })
        .collect()
}

fn lexical_from_signals(
    signals: Vec<RouteCandidateSignal>,
    matching_candidates: Vec<String>,
) -> RouteResolution {
    let granted_candidates = signals.iter().map(|signal| signal.kind.clone()).collect();
    let outcome = if matching_candidates.is_empty() {
        RouteOutcome::NoMatch
    } else {
        RouteOutcome::Candidates
    };
    RouteResolution {
        granted_candidates,
        full_granted_signals: signals,
        abstention_reason: if outcome == RouteOutcome::NoMatch {
            Some(RouteAbstentionReason::FloorMiss)
        } else {
            None
        },
        matching_candidates,
        selected_kind: None,
        outcome,
        match_signals: BTreeMap::new(),
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "an empty lexical match with granted overlaps {string}",
            &[],
            &[(LEXICAL_KEY, "RouteResolution")],
            |_ctx, params| {
                let signals = parse_granted_overlaps(
                    params.get_string(0).ok_or("Expected granted overlaps")?,
                )?;
                let mut out = Context::new();
                out.set(LEXICAL_KEY, lexical_from_signals(signals, Vec::new()));
                Ok(out)
            },
        ),
        step_def(
            "a lexical match with granted overlaps {string} and matching candidates {string}",
            &[],
            &[(LEXICAL_KEY, "RouteResolution")],
            |_ctx, params| {
                let signals = parse_granted_overlaps(
                    params.get_string(0).ok_or("Expected granted overlaps")?,
                )?;
                let matching = parse_csv(
                    params
                        .get_string(1)
                        .ok_or("Expected matching candidates")?,
                );
                let mut out = Context::new();
                out.set(LEXICAL_KEY, lexical_from_signals(signals, matching));
                Ok(out)
            },
        ),
        step_def(
            "V1 gate breadth is off",
            &[(LEXICAL_KEY, "RouteResolution")],
            &[(LEXICAL_KEY, "RouteResolution"), (V1_KEY, "bool")],
            |ctx, _params| {
                let mut out = Context::new();
                out.set(
                    LEXICAL_KEY,
                    ctx.get::<RouteResolution>(LEXICAL_KEY)
                        .ok_or("No lexical resolution")?
                        .clone(),
                );
                out.set(V1_KEY, false);
                Ok(out)
            },
        ),
        step_def(
            "V1 gate breadth is on",
            &[(LEXICAL_KEY, "RouteResolution")],
            &[(LEXICAL_KEY, "RouteResolution"), (V1_KEY, "bool")],
            |ctx, _params| {
                let mut out = Context::new();
                out.set(
                    LEXICAL_KEY,
                    ctx.get::<RouteResolution>(LEXICAL_KEY)
                        .ok_or("No lexical resolution")?
                        .clone(),
                );
                out.set(V1_KEY, true);
                Ok(out)
            },
        ),
        step_def(
            "V2 brief breadth is off",
            &[(LEXICAL_KEY, "RouteResolution"), (V1_KEY, "bool")],
            &[
                (LEXICAL_KEY, "RouteResolution"),
                (V1_KEY, "bool"),
                (V2_CAP_KEY, "Option<String>"),
            ],
            |ctx, _params| {
                let mut out = Context::new();
                out.set(
                    LEXICAL_KEY,
                    ctx.get::<RouteResolution>(LEXICAL_KEY)
                        .ok_or("No lexical resolution")?
                        .clone(),
                );
                out.set(
                    V1_KEY,
                    *ctx.get::<bool>(V1_KEY).ok_or("No V1 flag state")?,
                );
                out.set(V2_CAP_KEY, None::<String>);
                Ok(out)
            },
        ),
        step_def(
            "V2 brief breadth is on with cap {string}",
            &[(LEXICAL_KEY, "RouteResolution"), (V1_KEY, "bool")],
            &[
                (LEXICAL_KEY, "RouteResolution"),
                (V1_KEY, "bool"),
                (V2_CAP_KEY, "Option<String>"),
            ],
            |ctx, params| {
                let mut out = Context::new();
                out.set(
                    LEXICAL_KEY,
                    ctx.get::<RouteResolution>(LEXICAL_KEY)
                        .ok_or("No lexical resolution")?
                        .clone(),
                );
                out.set(
                    V1_KEY,
                    *ctx.get::<bool>(V1_KEY).ok_or("No V1 flag state")?,
                );
                out.set(
                    V2_CAP_KEY,
                    Some(
                        params
                            .get_string(0)
                            .ok_or("Expected V2 brief cap")?
                            .to_string(),
                    ),
                );
                Ok(out)
            },
        ),
        step_def(
            "semantic brief breadth is evaluated",
            &[
                (LEXICAL_KEY, "RouteResolution"),
                (V1_KEY, "bool"),
                (V2_CAP_KEY, "Option<String>"),
            ],
            &[
                (LEXICAL_KEY, "RouteResolution"),
                (PLAN_KEY, "SemanticRoutePlan"),
                (REPEATED_PLAN_KEY, "SemanticRoutePlan"),
            ],
            |ctx, _params| {
                let lexical = ctx
                    .get::<RouteResolution>(LEXICAL_KEY)
                    .ok_or("No lexical resolution")?;
                let v1_enabled = *ctx.get::<bool>(V1_KEY).ok_or("No V1 flag state")?;
                let cap = ctx
                    .get::<Option<String>>(V2_CAP_KEY)
                    .ok_or("No V2 flag state")?;
                let plan =
                    semantic_route_plan_with_brief_cap(lexical, v1_enabled, cap.as_deref());
                let repeated =
                    semantic_route_plan_with_brief_cap(lexical, v1_enabled, cap.as_deref());
                let mut out = Context::new();
                out.set(LEXICAL_KEY, lexical.clone());
                out.set(PLAN_KEY, plan);
                out.set(REPEATED_PLAN_KEY, repeated);
                Ok(out)
            },
        ),
        step_def(
            "semantic route eligibility is evaluated",
            &[(LEXICAL_KEY, "RouteResolution"), (V1_KEY, "bool")],
            &[
                (LEXICAL_KEY, "RouteResolution"),
                (PLAN_KEY, "SemanticRoutePlan"),
            ],
            |ctx, _params| {
                let lexical = ctx
                    .get::<RouteResolution>(LEXICAL_KEY)
                    .ok_or("No lexical resolution")?;
                let v1_enabled = *ctx.get::<bool>(V1_KEY).ok_or("No V1 flag state")?;
                let mut out = Context::new();
                out.set(LEXICAL_KEY, lexical.clone());
                out.set(PLAN_KEY, semantic_route_plan(lexical, v1_enabled));
                Ok(out)
            },
        ),
        check_def(
            "the semantic router is eligible",
            &[(PLAN_KEY, "SemanticRoutePlan")],
            |ctx, _params| {
                if ctx
                    .get::<SemanticRoutePlan>(PLAN_KEY)
                    .ok_or("No semantic route plan")?
                    .eligible
                {
                    Ok(())
                } else {
                    Err("Expected semantic router to be eligible".to_string())
                }
            },
        ),
        check_def(
            "the semantic router is not eligible",
            &[(PLAN_KEY, "SemanticRoutePlan")],
            |ctx, _params| {
                if !ctx
                    .get::<SemanticRoutePlan>(PLAN_KEY)
                    .ok_or("No semantic route plan")?
                    .eligible
                {
                    Ok(())
                } else {
                    Err("Expected semantic router not to be eligible".to_string())
                }
            },
        ),
        check_def(
            "the semantic briefs are exactly {string}",
            &[(PLAN_KEY, "SemanticRoutePlan")],
            |ctx, params| {
                let expected =
                    parse_csv(params.get_string(0).ok_or("Expected semantic briefs")?);
                let actual = &ctx
                    .get::<SemanticRoutePlan>(PLAN_KEY)
                    .ok_or("No semantic route plan")?
                    .brief_kinds;
                if actual == &expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected semantic briefs {:?}, got {:?}",
                        expected, actual
                    ))
                }
            },
        ),
        check_def(
            "the semantic briefs are empty",
            &[(PLAN_KEY, "SemanticRoutePlan")],
            |ctx, _params| {
                let actual = &ctx
                    .get::<SemanticRoutePlan>(PLAN_KEY)
                    .ok_or("No semantic route plan")?
                    .brief_kinds;
                if actual.is_empty() {
                    Ok(())
                } else {
                    Err(format!("Expected no semantic briefs, got {:?}", actual))
                }
            },
        ),
        step_def(
            "the semantic verdict picks {string} through the planned briefs",
            &[
                (LEXICAL_KEY, "RouteResolution"),
                (PLAN_KEY, "SemanticRoutePlan"),
            ],
            &[
                (PLAN_KEY, "SemanticRoutePlan"),
                (RESULT_KEY, "RouteResolution"),
            ],
            |ctx, params| {
                let lexical = ctx
                    .get::<RouteResolution>(LEXICAL_KEY)
                    .ok_or("No lexical resolution")?
                    .clone();
                let plan = ctx
                    .get::<SemanticRoutePlan>(PLAN_KEY)
                    .ok_or("No semantic route plan")?
                    .clone();
                let kind = params.get_string(0).ok_or("Expected picked kind")?.to_string();
                let result = apply_semantic_verdict_with_briefs(
                    lexical,
                    RouterVerdict::Pick {
                        kind,
                        why: "test verdict".to_string(),
                    },
                    &plan.brief_kinds,
                );
                let mut out = Context::new();
                out.set(PLAN_KEY, plan);
                out.set(RESULT_KEY, result);
                Ok(out)
            },
        ),
        check_def(
            "repeated semantic brief breadth evaluation produces the same briefs",
            &[
                (PLAN_KEY, "SemanticRoutePlan"),
                (REPEATED_PLAN_KEY, "SemanticRoutePlan"),
            ],
            |ctx, _params| {
                let first = ctx
                    .get::<SemanticRoutePlan>(PLAN_KEY)
                    .ok_or("No semantic route plan")?;
                let repeated = ctx
                    .get::<SemanticRoutePlan>(REPEATED_PLAN_KEY)
                    .ok_or("No repeated semantic route plan")?;
                if first.brief_kinds == repeated.brief_kinds {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected deterministic briefs, got {:?} then {:?}",
                        first.brief_kinds, repeated.brief_kinds
                    ))
                }
            },
        ),
        check_def(
            "the route outcome remains {string}",
            &[(PLAN_KEY, "SemanticRoutePlan")],
            |ctx, params| {
                let expected =
                    expected_outcome(params.get_string(0).ok_or("Expected route outcome")?)?;
                let actual = &ctx
                    .get::<SemanticRoutePlan>(PLAN_KEY)
                    .ok_or("No semantic route plan")?
                    .resolution
                    .outcome;
                if actual == &expected {
                    Ok(())
                } else {
                    Err(format!("Expected outcome {:?}, got {:?}", expected, actual))
                }
            },
        ),
        check_def(
            "the route variant is {string} with brief cap {string}",
            &[(PLAN_KEY, "SemanticRoutePlan")],
            |ctx, params| {
                let expected_variant = params.get_string(0).ok_or("Expected route variant")?;
                let expected_cap = params.get_string(1).ok_or("Expected brief cap")?;
                let plan = ctx
                    .get::<SemanticRoutePlan>(PLAN_KEY)
                    .ok_or("No semantic route plan")?;
                if plan.variant == expected_variant && plan.brief_cap == expected_cap {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected variant/cap {}/{}, got {}/{}",
                        expected_variant, expected_cap, plan.variant, plan.brief_cap
                    ))
                }
            },
        ),
        step_def(
            "a lexical route resolution with matching candidates {string}",
            &[],
            &[(LEXICAL_KEY, "RouteResolution")],
            |_ctx, params| {
                let matching =
                    parse_csv(params.get_string(0).ok_or("Expected matching candidates")?);
                // A pre-semantic Candidates resolution: granted == matching, no
                // selected kind, one match signal per matching kind (so the pick
                // path can verify the signal is retained for the chosen kind).
                let match_signals: BTreeMap<String, String> = matching
                    .iter()
                    .map(|k| (k.clone(), format!("lexical signal for {}", k)))
                    .collect();
                let lexical = RouteResolution {
                    granted_candidates: matching.clone(),
                    full_granted_signals: Vec::new(),
                    abstention_reason: None,
                    matching_candidates: matching,
                    selected_kind: None,
                    outcome: RouteOutcome::Candidates,
                    match_signals,
                };
                let mut out = Context::new();
                out.set(LEXICAL_KEY, lexical);
                Ok(out)
            },
        ),
        step_def(
            "a lexical route resolution with granted candidates {string} and matching candidates {string}",
            &[],
            &[(LEXICAL_KEY, "RouteResolution")],
            |_ctx, params| {
                let granted =
                    parse_csv(params.get_string(0).ok_or("Expected granted candidates")?);
                let matching =
                    parse_csv(params.get_string(1).ok_or("Expected matching candidates")?);
                // granted ⊃ matching (the candidate_recall widen lever): the semantic
                // router is fed the GRANTED set, so a Pick of a granted-but-not-matching
                // kind must be honored. One match signal per (narrower) matching kind.
                let match_signals: BTreeMap<String, String> = matching
                    .iter()
                    .map(|k| (k.clone(), format!("lexical signal for {}", k)))
                    .collect();
                let lexical = RouteResolution {
                    granted_candidates: granted,
                    full_granted_signals: Vec::new(),
                    abstention_reason: None,
                    matching_candidates: matching,
                    selected_kind: None,
                    outcome: RouteOutcome::Candidates,
                    match_signals,
                };
                let mut out = Context::new();
                out.set(LEXICAL_KEY, lexical);
                Ok(out)
            },
        ),
        step_def(
            "the semantic verdict is applied as pick {string}",
            &[(LEXICAL_KEY, "RouteResolution")],
            &[(RESULT_KEY, "RouteResolution")],
            |ctx, params| {
                let kind = params
                    .get_string(0)
                    .ok_or("Expected pick kind")?
                    .to_string();
                let lexical = ctx
                    .get::<RouteResolution>(LEXICAL_KEY)
                    .ok_or("No lexical resolution")?
                    .clone();
                let verdict = RouterVerdict::Pick {
                    kind,
                    why: String::new(),
                };
                let mut out = Context::new();
                out.set(RESULT_KEY, apply_semantic_verdict(lexical, verdict));
                Ok(out)
            },
        ),
        step_def(
            "the semantic verdict is applied as abstain",
            &[(LEXICAL_KEY, "RouteResolution")],
            &[(RESULT_KEY, "RouteResolution")],
            |ctx, _params| {
                let lexical = ctx
                    .get::<RouteResolution>(LEXICAL_KEY)
                    .ok_or("No lexical resolution")?
                    .clone();
                let mut out = Context::new();
                out.set(
                    RESULT_KEY,
                    apply_semantic_verdict(lexical, RouterVerdict::Abstain),
                );
                Ok(out)
            },
        ),
        step_def(
            "the semantic verdict is applied as fallback",
            &[(LEXICAL_KEY, "RouteResolution")],
            &[(RESULT_KEY, "RouteResolution")],
            |ctx, _params| {
                let lexical = ctx
                    .get::<RouteResolution>(LEXICAL_KEY)
                    .ok_or("No lexical resolution")?
                    .clone();
                let mut out = Context::new();
                out.set(
                    RESULT_KEY,
                    apply_semantic_verdict(lexical, RouterVerdict::Fallback),
                );
                Ok(out)
            },
        ),
        check_def(
            "the semantic resolution outcome is {string}",
            &[(RESULT_KEY, "RouteResolution")],
            |ctx, params| {
                let expected =
                    expected_outcome(params.get_string(0).ok_or("Expected outcome")?.as_ref())?;
                let actual = &result(&ctx)?.outcome;
                if actual == &expected {
                    Ok(())
                } else {
                    Err(format!("Expected outcome {:?}, got {:?}", expected, actual))
                }
            },
        ),
        check_def(
            "the semantic selected kind is {string}",
            &[(RESULT_KEY, "RouteResolution")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected kind")?.to_string();
                match &result(&ctx)?.selected_kind {
                    Some(kind) if kind == &expected => Ok(()),
                    other => Err(format!(
                        "Expected selected kind {:?}, got {:?}",
                        expected, other
                    )),
                }
            },
        ),
        check_def(
            "the semantic selected kind is unset",
            &[(RESULT_KEY, "RouteResolution")],
            |ctx, _params| match &result(&ctx)?.selected_kind {
                None => Ok(()),
                other => Err(format!("Expected no selected kind, got {:?}", other)),
            },
        ),
        check_def(
            "the semantic matching candidates are exactly {string}",
            &[(RESULT_KEY, "RouteResolution")],
            |ctx, params| {
                let expected =
                    parse_csv(params.get_string(0).ok_or("Expected candidates")?.as_ref());
                let actual = &result(&ctx)?.matching_candidates;
                if actual == &expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected matching candidates {:?}, got {:?}",
                        expected, actual
                    ))
                }
            },
        ),
    ]
}
