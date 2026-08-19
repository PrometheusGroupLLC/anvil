//! Step module for `hooks_route_turn.feature` (core seam).
//!
//! Drives the PURE route-turn decision: distill outcome → guidance text, and
//! extract the user message from a harness user-prompt event JSON. No process,
//! no socket — the stdin/gRPC plumbing + fail-open live in the real binary
//! (proven in the engine seam).

use anvil_core::domain::hooks::route_turn::{
    build_router_prompt, extract_conversation_id, extract_transcript_context, extract_user_message,
    format_guidance, format_park_hint, guidance_kind_of, narrow_candidates, parse_router_decision,
    parse_router_kind, shape_guidance_for_source,
    CandidateBrief, InProgressSignal, ResumeWire, RouteCandidateWire, RouteTurnOutcome,
    RouterVerdict,
};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

const OUTCOME_KEY: &str = "rt_outcome";
const GUIDANCE_KEY: &str = "rt_guidance";
const PAYLOAD_KEY: &str = "rt_payload";
const MESSAGE_KEY: &str = "rt_message";
const NO_MESSAGE_KEY: &str = "rt_no_message";
const NARROWED_KEY: &str = "rt_narrowed";
const PROMPT_KEY: &str = "rt_router_prompt";
const REPLY_KEY: &str = "rt_router_reply";
const PARSED_KIND_KEY: &str = "rt_parsed_kind";
const PARSED_WHY_KEY: &str = "rt_parsed_why";
const NO_KIND_KEY: &str = "rt_no_kind";
const CONV_KEY: &str = "rt_conversation_id";
const NO_CONV_KEY: &str = "rt_no_conversation_id";
// context_aware_routing keys.
const REC_CTX_KEY: &str = "rt_recent_context";
const IP_KEY: &str = "rt_in_progress";
const TAIL_KEY: &str = "rt_transcript_tail";
// The delivered kind projected off the outcome, for the delivery row.
const GUIDANCE_KIND_KEY: &str = "rt_guidance_kind";

/// Parse an in-progress spec cell into an [`InProgressSignal`]:
/// `none` | `open:<kind>` | `work-without-begin`.
fn parse_in_progress_spec(spec: &str) -> Result<InProgressSignal, String> {
    let spec = spec.trim();
    if spec == "none" {
        Ok(InProgressSignal::None)
    } else if spec == "work-without-begin" {
        Ok(InProgressSignal::WorkWithoutBegin)
    } else if let Some(kind) = spec.strip_prefix("open:") {
        Ok(InProgressSignal::OpenPlaybookRun {
            kind: kind.trim().to_string(),
        })
    } else {
        Err(format!("unknown in-progress spec '{}'", spec))
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ---- Given: a distilled outcome ----
        step_def(
            "a route-turn outcome single with kind {string}",
            &[],
            &[(OUTCOME_KEY, "RouteTurnOutcome")],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let mut out = Context::new();
                out.set(
                    OUTCOME_KEY,
                    RouteTurnOutcome::Single {
                        kind,
                        why: String::new(),
                        description: String::new(),
                        required_fields: Vec::new(),
                        guidance: String::new(),
                    },
                );
                Ok(out)
            },
        ),
        step_def(
            "a route-turn outcome single with kind {string} guidance {string}",
            &[],
            &[(OUTCOME_KEY, "RouteTurnOutcome")],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let guidance = params.get_string(1).ok_or("Expected guidance")?.to_string();
                let mut out = Context::new();
                out.set(
                    OUTCOME_KEY,
                    RouteTurnOutcome::Single {
                        kind,
                        why: String::new(),
                        description: "a purpose".to_string(),
                        required_fields: Vec::new(),
                        guidance,
                    },
                );
                Ok(out)
            },
        ),
        step_def(
            "a route-turn outcome single with kind {string} purpose {string} required fields {string}",
            &[],
            &[(OUTCOME_KEY, "RouteTurnOutcome")],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let description = params.get_string(1).ok_or("Expected purpose")?.to_string();
                let raw = params.get_string(2).ok_or("Expected required fields")?;
                let required_fields: Vec<String> = raw
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                let mut out = Context::new();
                out.set(
                    OUTCOME_KEY,
                    RouteTurnOutcome::Single {
                        kind,
                        why: String::new(),
                        description,
                        required_fields,
                        guidance: String::new(),
                    },
                );
                Ok(out)
            },
        ),
        step_def(
            "the extracted conversation id is used with a route-turn outcome single with kind {string} purpose {string} required fields {string}",
            &[(CONV_KEY, "String")],
            &[(OUTCOME_KEY, "RouteTurnOutcome"), (CONV_KEY, "String")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let description = params.get_string(1).ok_or("Expected purpose")?.to_string();
                let raw = params.get_string(2).ok_or("Expected required fields")?;
                let required_fields: Vec<String> = raw
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                let conversation_id = ctx
                    .get::<String>(CONV_KEY)
                    .ok_or("No conversation id extracted")?
                    .clone();
                let mut out = Context::new();
                out.set(
                    OUTCOME_KEY,
                    RouteTurnOutcome::Single {
                        kind,
                        why: String::new(),
                        description,
                        required_fields,
                        guidance: String::new(),
                    },
                );
                out.set(CONV_KEY, conversation_id);
                Ok(out)
            },
        ),
        step_def(
            "a route-turn outcome candidates with kinds {string}",
            &[],
            &[(OUTCOME_KEY, "RouteTurnOutcome")],
            |_ctx, params| {
                let raw = params.get_string(0).ok_or("Expected kinds")?;
                let candidates: Vec<CandidateBrief> = raw
                    .split(',')
                    .map(|s| CandidateBrief {
                        kind: s.trim().to_string(),
                        description: String::new(),
                        ..Default::default()
                    })
                    .collect();
                let mut out = Context::new();
                out.set(OUTCOME_KEY, RouteTurnOutcome::Candidates { candidates });
                Ok(out)
            },
        ),
        step_def(
            "a route-turn outcome candidates with kinds and purposes {string}",
            &[],
            &[(OUTCOME_KEY, "RouteTurnOutcome")],
            |_ctx, params| {
                // Each candidate is "kind|purpose", candidates separated by ";".
                let raw = params.get_string(0).ok_or("Expected candidates")?;
                let candidates: Vec<CandidateBrief> = raw
                    .split(';')
                    .filter_map(|entry| {
                        let (kind, desc) = entry.split_once('|')?;
                        Some(CandidateBrief {
                            kind: kind.trim().to_string(),
                            description: desc.trim().to_string(),
                            ..Default::default()
                        })
                    })
                    .collect();
                let mut out = Context::new();
                out.set(OUTCOME_KEY, RouteTurnOutcome::Candidates { candidates });
                Ok(out)
            },
        ),
        step_def(
            "a route-turn outcome candidates with kinds purposes and route triggers {string}",
            &[],
            &[(OUTCOME_KEY, "RouteTurnOutcome")],
            |_ctx, params| {
                // Each candidate is "kind|purpose|trigger1,trigger2", candidates separated by ";".
                let raw = params.get_string(0).ok_or("Expected candidates")?;
                let candidates: Vec<CandidateBrief> = raw
                    .split(';')
                    .filter_map(|entry| {
                        let mut parts = entry.splitn(3, '|');
                        let kind = parts.next()?;
                        let desc = parts.next()?;
                        let triggers = parts.next().unwrap_or_default();
                        let route_triggers = triggers
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        Some(CandidateBrief {
                            kind: kind.trim().to_string(),
                            description: desc.trim().to_string(),
                            route_triggers,
                            ..Default::default()
                        })
                    })
                    .collect();
                let mut out = Context::new();
                out.set(OUTCOME_KEY, RouteTurnOutcome::Candidates { candidates });
                Ok(out)
            },
        ),
        step_def(
            "a route-turn outcome candidates with kinds purposes and required fields {string}",
            &[],
            &[(OUTCOME_KEY, "RouteTurnOutcome")],
            |_ctx, params| {
                // Each candidate is "kind|purpose|field1,field2", candidates separated by ";".
                let raw = params.get_string(0).ok_or("Expected candidates")?;
                let candidates: Vec<CandidateBrief> = raw
                    .split(';')
                    .filter_map(|entry| {
                        let mut parts = entry.splitn(3, '|');
                        let kind = parts.next()?;
                        let desc = parts.next()?;
                        let fields = parts.next().unwrap_or_default();
                        let required_fields = fields
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        Some(CandidateBrief {
                            kind: kind.trim().to_string(),
                            description: desc.trim().to_string(),
                            required_fields,
                            ..Default::default()
                        })
                    })
                    .collect();
                let mut out = Context::new();
                out.set(OUTCOME_KEY, RouteTurnOutcome::Candidates { candidates });
                Ok(out)
            },
        ),
        step_def(
            "a route-turn outcome candidates annotated kind {string} intent {string} steps {string} why {string}",
            &[],
            &[(OUTCOME_KEY, "RouteTurnOutcome")],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let intent = params.get_string(1).ok_or("Expected intent")?.to_string();
                let steps_raw = params.get_string(2).ok_or("Expected steps")?;
                let why = params.get_string(3).ok_or("Expected why")?.to_string();
                let step_outline: Vec<String> = steps_raw
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                let candidates = vec![CandidateBrief {
                    kind,
                    description: "a purpose".to_string(),
                    route_triggers: Vec::new(),
                    required_fields: Vec::new(),
                    intent,
                    step_outline,
                    why_fits: why,
                }];
                let mut out = Context::new();
                out.set(OUTCOME_KEY, RouteTurnOutcome::Candidates { candidates });
                Ok(out)
            },
        ),
        step_def(
            "a candidates route response with granted candidate kinds {string} and matching candidate kinds {string}",
            &[],
            &[(OUTCOME_KEY, "RouteTurnOutcome")],
            |_ctx, params| {
                let granted_raw = params.get_string(0).ok_or("Expected granted kinds")?;
                let matching_raw = params.get_string(1).ok_or("Expected matching kinds")?;
                let candidates: Vec<RouteCandidateWire> = granted_raw
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|kind| RouteCandidateWire {
                        kind: kind.to_string(),
                        description: format!("Purpose for {}.", kind),
                        route_triggers: vec![format!("trigger {}", kind)],
                        required_fields: vec![format!("field_{}", kind)],
                        intent: format!("Intent for {}", kind),
                        step_outline: vec![
                            format!("step_{}_one", kind),
                            format!("step_{}_two", kind),
                        ],
                        why_fits: format!("why {} fits", kind),
                    })
                    .collect();
                let matching_candidates: Vec<String> = matching_raw
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                let mut out = Context::new();
                out.set(
                    OUTCOME_KEY,
                    RouteTurnOutcome::from_route_response(
                        "candidates",
                        "",
                        "",
                        &candidates,
                        &matching_candidates,
                        &ResumeWire::default(),
                    ),
                );
                Ok(out)
            },
        ),
        step_def(
            "a single route response with kind {string} why {string}",
            &[],
            &[(OUTCOME_KEY, "RouteTurnOutcome")],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let why = params.get_string(1).ok_or("Expected why")?.to_string();
                let candidates = vec![RouteCandidateWire {
                    kind: kind.clone(),
                    description: format!("Purpose for {}.", kind),
                    route_triggers: vec![format!("trigger {}", kind)],
                    required_fields: vec![format!("field_{}", kind)],
                    intent: String::new(),
                    step_outline: Vec::new(),
                    why_fits: String::new(),
                }];
                let matching_candidates = vec![kind.clone()];
                let mut out = Context::new();
                out.set(
                    OUTCOME_KEY,
                    RouteTurnOutcome::from_route_response_with_why(
                        "single",
                        &kind,
                        "",
                        &why,
                        &candidates,
                        &matching_candidates,
                        &ResumeWire::default(),
                    ),
                );
                Ok(out)
            },
        ),
        step_def(
            "a route-turn outcome no_match",
            &[],
            &[(OUTCOME_KEY, "RouteTurnOutcome")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set(OUTCOME_KEY, RouteTurnOutcome::NoMatch);
                Ok(out)
            },
        ),
        // ---- Given: user-prompt event payloads ----
        step_def(
            "a user-prompt event JSON with prompt {string}",
            &[],
            &[(PAYLOAD_KEY, "String")],
            |_ctx, params| {
                let prompt = params.get_string(0).ok_or("Expected prompt")?;
                let json = serde_json::json!({ "prompt": prompt });
                let mut out = Context::new();
                out.set(PAYLOAD_KEY, json.to_string());
                Ok(out)
            },
        ),
        step_def(
            "a Hermes pre_llm_call event JSON whose last user message is {string}",
            &[],
            &[(PAYLOAD_KEY, "String")],
            |_ctx, params| {
                let msg = params.get_string(0).ok_or("Expected message")?;
                let json = serde_json::json!({
                    "messages": [
                        { "role": "system", "content": "you are helpful" },
                        { "role": "user", "content": "earlier turn" },
                        { "role": "assistant", "content": "ok" },
                        { "role": "user", "content": msg },
                    ]
                });
                let mut out = Context::new();
                out.set(PAYLOAD_KEY, json.to_string());
                Ok(out)
            },
        ),
        step_def(
            "a PreToolUse Task event JSON whose subagent prompt is {string}",
            &[],
            &[(PAYLOAD_KEY, "String")],
            |_ctx, params| {
                let prompt = params.get_string(0).ok_or("Expected prompt")?;
                // Claude Code PreToolUse payload for a subagent dispatch.
                let json = serde_json::json!({
                    "tool_name": "Task",
                    "tool_input": { "description": "do a thing", "prompt": prompt, "subagent_type": "general" }
                });
                let mut out = Context::new();
                out.set(PAYLOAD_KEY, json.to_string());
                Ok(out)
            },
        ),
        step_def(
            "a user-prompt event JSON that carries no prompt",
            &[],
            &[(PAYLOAD_KEY, "String")],
            |_ctx, _params| {
                let json = serde_json::json!({ "unrelated": "noise" });
                let mut out = Context::new();
                out.set(PAYLOAD_KEY, json.to_string());
                Ok(out)
            },
        ),
        // resume_aware_routing H2 — conversation/session id extraction payloads.
        step_def(
            "a Claude Code UserPromptSubmit event JSON with session_id {string} and prompt {string}",
            &[],
            &[(PAYLOAD_KEY, "String")],
            |_ctx, params| {
                let session_id = params.get_string(0).ok_or("Expected session_id")?;
                let prompt = params.get_string(1).ok_or("Expected prompt")?;
                let json = serde_json::json!({
                    "hook_event_name": "UserPromptSubmit",
                    "session_id": session_id,
                    "prompt": prompt,
                });
                let mut out = Context::new();
                out.set(PAYLOAD_KEY, json.to_string());
                Ok(out)
            },
        ),
        step_def(
            "a user-prompt event JSON that carries no session id",
            &[],
            &[(PAYLOAD_KEY, "String")],
            |_ctx, _params| {
                let json = serde_json::json!({ "prompt": "go" });
                let mut out = Context::new();
                out.set(PAYLOAD_KEY, json.to_string());
                Ok(out)
            },
        ),
        step_def(
            "a route-turn conversation id {string}",
            &[(OUTCOME_KEY, "RouteTurnOutcome")],
            &[(OUTCOME_KEY, "RouteTurnOutcome"), (CONV_KEY, "String")],
            |ctx, params| {
                let conversation_id = params
                    .get_string(0)
                    .ok_or("Expected conversation id")?
                    .to_string();
                let outcome = ctx
                    .get::<RouteTurnOutcome>(OUTCOME_KEY)
                    .ok_or("No outcome")?
                    .clone();
                let mut out = Context::new();
                out.set(OUTCOME_KEY, outcome);
                out.set(CONV_KEY, conversation_id);
                Ok(out)
            },
        ),
        // ---- When ----
        step_def(
            "the route-turn guidance is formatted",
            &[(OUTCOME_KEY, "RouteTurnOutcome")],
            &[(GUIDANCE_KEY, "String")],
            |ctx, _params| {
                let outcome = ctx
                    .get::<RouteTurnOutcome>(OUTCOME_KEY)
                    .ok_or("No outcome")?;
                let conversation_id = ctx
                    .get::<String>(CONV_KEY)
                    .map(|s| s.as_str())
                    .unwrap_or("");
                let mut out = Context::new();
                out.set(GUIDANCE_KEY, format_guidance(outcome, conversation_id));
                Ok(out)
            },
        ),
        step_def(
            "the narrowed route-turn guidance is formatted",
            &[(NARROWED_KEY, "RouteTurnOutcome")],
            &[(GUIDANCE_KEY, "String")],
            |ctx, _params| {
                let outcome = ctx
                    .get::<RouteTurnOutcome>(NARROWED_KEY)
                    .ok_or("No narrowed outcome")?;
                let conversation_id = ctx
                    .get::<String>(CONV_KEY)
                    .map(|s| s.as_str())
                    .unwrap_or("");
                let mut out = Context::new();
                out.set(GUIDANCE_KEY, format_guidance(outcome, conversation_id));
                Ok(out)
            },
        ),
        // harness-shaped stdout — Kiln needs JSON `additionalContext`, others raw.
        step_def(
            "the route-turn guidance is shaped for source {string}",
            &[(GUIDANCE_KEY, "String")],
            &[(GUIDANCE_KEY, "String")],
            |ctx, params| {
                let source = params.get_string(0).ok_or("Expected source")?;
                let guidance = ctx.get::<String>(GUIDANCE_KEY).ok_or("No guidance")?;
                let mut out = Context::new();
                out.set(GUIDANCE_KEY, shape_guidance_for_source(guidance, &source));
                Ok(out)
            },
        ),
        // resume-signal context-awareness — the PARK HINT render (format_park_hint).
        step_def(
            "the route-turn park hint is formatted for artifact {string} kind {string} state {string} park action {string}",
            &[],
            &[(GUIDANCE_KEY, "String")],
            |_ctx, params| {
                let artifact_id = params.get_string(0).ok_or("Expected artifact id")?.to_string();
                let kind = params.get_string(1).ok_or("Expected kind")?.to_string();
                let state = params.get_string(2).ok_or("Expected state")?.to_string();
                let park_action = params.get_string(3).ok_or("Expected park action")?.to_string();
                let mut out = Context::new();
                out.set(
                    GUIDANCE_KEY,
                    format_park_hint(&artifact_id, &kind, &state, &park_action),
                );
                Ok(out)
            },
        ),
        step_def(
            "the route-turn user message is extracted",
            &[(PAYLOAD_KEY, "String")],
            &[(MESSAGE_KEY, "String"), (NO_MESSAGE_KEY, "bool")],
            |ctx, _params| {
                let payload = ctx.get::<String>(PAYLOAD_KEY).ok_or("No payload")?;
                let json: serde_json::Value =
                    serde_json::from_str(payload).map_err(|e| format!("parse: {}", e))?;
                let mut out = Context::new();
                match extract_user_message(&json) {
                    Some(msg) => out.set(MESSAGE_KEY, msg),
                    None => out.set(NO_MESSAGE_KEY, true),
                }
                Ok(out)
            },
        ),
        step_def(
            "the route-turn conversation id is extracted",
            &[(PAYLOAD_KEY, "String")],
            &[(CONV_KEY, "String"), (NO_CONV_KEY, "bool")],
            |ctx, _params| {
                let payload = ctx.get::<String>(PAYLOAD_KEY).ok_or("No payload")?;
                let json: serde_json::Value =
                    serde_json::from_str(payload).map_err(|e| format!("parse: {}", e))?;
                let mut out = Context::new();
                match extract_conversation_id(&json) {
                    Some(id) => out.set(CONV_KEY, id),
                    None => out.set(NO_CONV_KEY, true),
                }
                Ok(out)
            },
        ),
        check_def(
            "the extracted conversation id is {string}",
            &[(CONV_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected conversation id")?;
                let got = ctx.get::<String>(CONV_KEY).ok_or("No conversation id extracted")?;
                if got == want.as_ref() as &str {
                    Ok(())
                } else {
                    Err(format!("conversation id: expected '{}', got '{}'", want, got))
                }
            },
        ),
        check_def(
            "no route-turn conversation id is extracted",
            &[(NO_CONV_KEY, "bool")],
            |ctx, _params| {
                if ctx.get::<bool>(NO_CONV_KEY).copied().unwrap_or(false) {
                    Ok(())
                } else {
                    Err("expected no conversation id, but one was extracted".to_string())
                }
            },
        ),
        // ---- Then ----
        check_def(
            "the route-turn guidance is JSON with additionalContext containing {string}",
            &[(GUIDANCE_KEY, "String")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected substring")?;
                let raw = ctx.get::<String>(GUIDANCE_KEY).ok_or("No guidance")?;
                let parsed: serde_json::Value = serde_json::from_str(raw)
                    .map_err(|e| format!("guidance is not JSON ({}) in:\n{}", e, raw))?;
                let ctx_str = parsed
                    .get("additionalContext")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| format!("no string additionalContext field in:\n{}", raw))?;
                if ctx_str.contains(needle.as_ref() as &str) {
                    Ok(())
                } else {
                    Err(format!("additionalContext missing '{}' in:\n{}", needle, ctx_str))
                }
            },
        ),
        check_def(
            "the route-turn guidance is not JSON",
            &[(GUIDANCE_KEY, "String")],
            |ctx, _params| {
                let raw = ctx.get::<String>(GUIDANCE_KEY).ok_or("No guidance")?;
                match serde_json::from_str::<serde_json::Value>(raw) {
                    Err(_) => Ok(()),
                    Ok(_) => Err(format!("expected raw (non-JSON) guidance, got JSON:\n{}", raw)),
                }
            },
        ),
        check_def(
            "the route-turn outcome is candidates with kinds {string}",
            &[(OUTCOME_KEY, "RouteTurnOutcome")],
            |ctx, params| {
                let raw = params.get_string(0).ok_or("Expected kinds")?;
                let want: Vec<String> = raw
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                let outcome = ctx
                    .get::<RouteTurnOutcome>(OUTCOME_KEY)
                    .ok_or("No route-turn outcome")?;
                match outcome {
                    RouteTurnOutcome::Candidates { candidates } => {
                        let got: Vec<String> =
                            candidates.iter().map(|c| c.kind.clone()).collect();
                        if got == want {
                            Ok(())
                        } else {
                            Err(format!("expected candidates {:?}, got {:?}", want, got))
                        }
                    }
                    other => Err(format!("expected Candidates, got {:?}", other)),
                }
            },
        ),
        check_def(
            "the route-turn guidance contains {string}",
            &[(GUIDANCE_KEY, "String")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let guidance = ctx.get::<String>(GUIDANCE_KEY).ok_or("No guidance")?;
                if guidance.contains(needle.as_ref() as &str) {
                    Ok(())
                } else {
                    Err(format!(
                        "guidance '{}' does not contain '{}'",
                        guidance, needle
                    ))
                }
            },
        ),
        check_def(
            "the route-turn guidance does not contain {string}",
            &[(GUIDANCE_KEY, "String")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let guidance = ctx.get::<String>(GUIDANCE_KEY).ok_or("No guidance")?;
                if guidance.contains(needle.as_ref() as &str) {
                    Err(format!("guidance '{}' unexpectedly contains '{}'", guidance, needle))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the route-turn guidance starts with {string}",
            &[(GUIDANCE_KEY, "String")],
            |ctx, params| {
                let prefix = params.get_string(0).ok_or("Expected prefix")?;
                let guidance = ctx.get::<String>(GUIDANCE_KEY).ok_or("No guidance")?;
                if guidance.starts_with(prefix.as_ref() as &str) {
                    Ok(())
                } else {
                    Err(format!(
                        "guidance '{}' does not start with '{}'",
                        guidance, prefix
                    ))
                }
            },
        ),
        check_def(
            "the route-turn guidance is empty",
            &[(GUIDANCE_KEY, "String")],
            |ctx, _params| {
                let guidance = ctx.get::<String>(GUIDANCE_KEY).ok_or("No guidance")?;
                if guidance.is_empty() {
                    Ok(())
                } else {
                    Err(format!("expected empty guidance, got '{}'", guidance))
                }
            },
        ),
        check_def(
            "the extracted user message is {string}",
            &[(MESSAGE_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected message")?;
                let got = ctx
                    .get::<String>(MESSAGE_KEY)
                    .ok_or("No message extracted")?;
                if got == want {
                    Ok(())
                } else {
                    Err(format!("message: expected '{}', got '{}'", want, got))
                }
            },
        ),
        check_def(
            "no route-turn user message is extracted",
            &[(NO_MESSAGE_KEY, "bool")],
            |ctx, _params| {
                if ctx.get::<bool>(NO_MESSAGE_KEY).copied().unwrap_or(false) {
                    Ok(())
                } else {
                    Err("expected no message, but one was extracted".to_string())
                }
            },
        ),
        // ---- LLM ROUTER: pure narrowing (Pick / Abstain / Fallback) ----
        step_def(
            "the route-turn outcome is narrowed with router verdict pick {string}",
            &[(OUTCOME_KEY, "RouteTurnOutcome")],
            &[(NARROWED_KEY, "RouteTurnOutcome")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let outcome = ctx
                    .get::<RouteTurnOutcome>(OUTCOME_KEY)
                    .ok_or("No outcome")?
                    .clone();
                let mut out = Context::new();
                out.set(
                    NARROWED_KEY,
                    narrow_candidates(outcome, &RouterVerdict::Pick { kind, why: String::new() }),
                );
                Ok(out)
            },
        ),
        step_def(
            "the route-turn outcome is narrowed with router verdict pick {string} why {string}",
            &[(OUTCOME_KEY, "RouteTurnOutcome")],
            &[(NARROWED_KEY, "RouteTurnOutcome")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let why = params.get_string(1).ok_or("Expected why")?.to_string();
                let outcome = ctx
                    .get::<RouteTurnOutcome>(OUTCOME_KEY)
                    .ok_or("No outcome")?
                    .clone();
                let mut out = Context::new();
                out.set(NARROWED_KEY, narrow_candidates(outcome, &RouterVerdict::Pick { kind, why }));
                Ok(out)
            },
        ),
        step_def(
            "the route-turn outcome is narrowed with router verdict abstain",
            &[(OUTCOME_KEY, "RouteTurnOutcome")],
            &[(NARROWED_KEY, "RouteTurnOutcome")],
            |ctx, _params| {
                let outcome = ctx
                    .get::<RouteTurnOutcome>(OUTCOME_KEY)
                    .ok_or("No outcome")?
                    .clone();
                let mut out = Context::new();
                out.set(
                    NARROWED_KEY,
                    narrow_candidates(outcome, &RouterVerdict::Abstain),
                );
                Ok(out)
            },
        ),
        step_def(
            "the route-turn outcome is narrowed with router verdict fallback",
            &[(OUTCOME_KEY, "RouteTurnOutcome")],
            &[(NARROWED_KEY, "RouteTurnOutcome")],
            |ctx, _params| {
                let outcome = ctx
                    .get::<RouteTurnOutcome>(OUTCOME_KEY)
                    .ok_or("No outcome")?
                    .clone();
                let mut out = Context::new();
                out.set(
                    NARROWED_KEY,
                    narrow_candidates(outcome, &RouterVerdict::Fallback),
                );
                Ok(out)
            },
        ),
        check_def(
            "the narrowed outcome is single with kind {string}",
            &[(NARROWED_KEY, "RouteTurnOutcome")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected kind")?;
                let outcome = ctx
                    .get::<RouteTurnOutcome>(NARROWED_KEY)
                    .ok_or("No narrowed outcome")?;
                match outcome {
                    RouteTurnOutcome::Single { kind, .. } if kind == want.as_ref() as &str => {
                        Ok(())
                    }
                    other => Err(format!("expected Single({}), got {:?}", want, other)),
                }
            },
        ),
        check_def(
            "the narrowed outcome is no_match",
            &[(NARROWED_KEY, "RouteTurnOutcome")],
            |ctx, _params| {
                let outcome = ctx
                    .get::<RouteTurnOutcome>(NARROWED_KEY)
                    .ok_or("No narrowed outcome")?;
                match outcome {
                    RouteTurnOutcome::NoMatch => Ok(()),
                    other => Err(format!("expected NoMatch, got {:?}", other)),
                }
            },
        ),
        check_def(
            "the narrowed outcome is candidates with kinds {string}",
            &[(NARROWED_KEY, "RouteTurnOutcome")],
            |ctx, params| {
                let raw = params.get_string(0).ok_or("Expected kinds")?;
                let want: Vec<String> = raw
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                let outcome = ctx
                    .get::<RouteTurnOutcome>(NARROWED_KEY)
                    .ok_or("No narrowed outcome")?;
                match outcome {
                    RouteTurnOutcome::Candidates { candidates } => {
                        let got: Vec<String> =
                            candidates.iter().map(|c| c.kind.clone()).collect();
                        if got == want {
                            Ok(())
                        } else {
                            Err(format!("expected candidates {:?}, got {:?}", want, got))
                        }
                    }
                    other => Err(format!("expected Candidates, got {:?}", other)),
                }
            },
        ),
        // ---- LLM ROUTER: pure prompt build + reply parse ----
        step_def(
            "the router prompt is built from the candidates",
            &[(OUTCOME_KEY, "RouteTurnOutcome")],
            &[(PROMPT_KEY, "String")],
            |ctx, _params| {
                let outcome = ctx
                    .get::<RouteTurnOutcome>(OUTCOME_KEY)
                    .ok_or("No outcome")?;
                let candidates = match outcome {
                    RouteTurnOutcome::Candidates { candidates } => candidates.clone(),
                    other => return Err(format!("expected Candidates, got {:?}", other)),
                };
                let mut out = Context::new();
                out.set(
                    PROMPT_KEY,
                    build_router_prompt("", "", &InProgressSignal::None, &candidates),
                );
                Ok(out)
            },
        ),
        check_def(
            "the router prompt contains {string}",
            &[(PROMPT_KEY, "String")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let prompt = ctx.get::<String>(PROMPT_KEY).ok_or("No prompt")?;
                if prompt.contains(needle.as_ref() as &str) {
                    Ok(())
                } else {
                    Err(format!("prompt '{}' does not contain '{}'", prompt, needle))
                }
            },
        ),
        check_def(
            "the router prompt does not contain {string}",
            &[(PROMPT_KEY, "String")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let prompt = ctx.get::<String>(PROMPT_KEY).ok_or("No prompt")?;
                if prompt.contains(needle.as_ref() as &str) {
                    Err(format!("prompt unexpectedly contains '{}'", needle))
                } else {
                    Ok(())
                }
            },
        ),
        // ---- context_aware_routing: context-aware prompt build ----
        // Recent context, current turn, and the in-progress signal are passed as
        // params (not separate context keys) so the single candidate-set context
        // dependency threads cleanly through brine's linear planner. The
        // in-progress spec is "none" | "open:<kind>" | "work-without-begin".
        step_def(
            "the context-aware router prompt is built with recent context {string} current turn {string} and in-progress {string}",
            &[(OUTCOME_KEY, "RouteTurnOutcome")],
            &[(PROMPT_KEY, "String")],
            |ctx, params| {
                let outcome = ctx.get::<RouteTurnOutcome>(OUTCOME_KEY).ok_or("No outcome")?;
                let candidates = match outcome {
                    RouteTurnOutcome::Candidates { candidates } => candidates.clone(),
                    other => return Err(format!("expected Candidates, got {:?}", other)),
                };
                // `\n` in the cell is a literal escape; unescape to real newlines.
                let recent = params
                    .get_string(0)
                    .ok_or("Expected recent context")?
                    .replace("\\n", "\n");
                let current = params.get_string(1).ok_or("Expected current turn")?.to_string();
                let ip_spec = params.get_string(2).ok_or("Expected in-progress spec")?;
                let in_progress = parse_in_progress_spec(&ip_spec)?;
                let mut out = Context::new();
                out.set(
                    PROMPT_KEY,
                    build_router_prompt(&current, &recent, &in_progress, &candidates),
                );
                Ok(out)
            },
        ),
        // ---- context_aware_routing: transcript tail → context extraction ----
        step_def(
            "a transcript tail:",
            &[],
            &[(TAIL_KEY, "String")],
            |_ctx, params| {
                let tail = params.doc_string().ok_or("Expected a transcript tail doc string")?;
                let mut out = Context::new();
                out.set(TAIL_KEY, tail.to_string());
                Ok(out)
            },
        ),
        step_def(
            "the transcript context is extracted",
            &[(TAIL_KEY, "String")],
            &[(REC_CTX_KEY, "String"), (IP_KEY, "InProgressSignal")],
            |ctx, _params| {
                let tail = ctx.get::<String>(TAIL_KEY).ok_or("No transcript tail")?;
                let context = extract_transcript_context(tail);
                let mut out = Context::new();
                out.set(REC_CTX_KEY, context.recent_context);
                out.set(IP_KEY, context.in_progress);
                Ok(out)
            },
        ),
        check_def(
            "the recent context contains {string}",
            &[(REC_CTX_KEY, "String")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let recent = ctx.get::<String>(REC_CTX_KEY).ok_or("No recent context")?;
                if recent.contains(needle.as_ref() as &str) {
                    Ok(())
                } else {
                    Err(format!(
                        "recent context '{}' does not contain '{}'",
                        recent, needle
                    ))
                }
            },
        ),
        check_def(
            "the recent context is empty",
            &[(REC_CTX_KEY, "String")],
            |ctx, _params| {
                let recent = ctx.get::<String>(REC_CTX_KEY).ok_or("No recent context")?;
                if recent.trim().is_empty() {
                    Ok(())
                } else {
                    Err(format!("expected empty recent context, got '{}'", recent))
                }
            },
        ),
        check_def(
            "the in-progress signal is open playbook kind {string}",
            &[(IP_KEY, "InProgressSignal")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let signal = ctx.get::<InProgressSignal>(IP_KEY).ok_or("No in-progress")?;
                match signal {
                    InProgressSignal::OpenPlaybookRun { kind: got } if got == kind.as_ref() as &str => {
                        Ok(())
                    }
                    other => Err(format!(
                        "expected OpenPlaybookRun {{ kind: {} }}, got {:?}",
                        kind, other
                    )),
                }
            },
        ),
        check_def(
            "the in-progress signal is work without begin",
            &[(IP_KEY, "InProgressSignal")],
            |ctx, _params| {
                let signal = ctx.get::<InProgressSignal>(IP_KEY).ok_or("No in-progress")?;
                match signal {
                    InProgressSignal::WorkWithoutBegin => Ok(()),
                    other => Err(format!("expected WorkWithoutBegin, got {:?}", other)),
                }
            },
        ),
        check_def(
            "the in-progress signal is none",
            &[(IP_KEY, "InProgressSignal")],
            |ctx, _params| {
                let signal = ctx.get::<InProgressSignal>(IP_KEY).ok_or("No in-progress")?;
                match signal {
                    InProgressSignal::None => Ok(()),
                    other => Err(format!("expected None, got {:?}", other)),
                }
            },
        ),
        step_def(
            "a local router reply {string}",
            &[],
            &[(REPLY_KEY, "String")],
            |_ctx, params| {
                let reply = params.get_string(0).ok_or("Expected reply")?.to_string();
                let mut out = Context::new();
                out.set(REPLY_KEY, reply);
                Ok(out)
            },
        ),
        step_def(
            "the router reply is parsed",
            &[(REPLY_KEY, "String")],
            &[(PARSED_KIND_KEY, "String"), (NO_KIND_KEY, "bool")],
            |ctx, _params| {
                let reply = ctx.get::<String>(REPLY_KEY).ok_or("No reply")?;
                let mut out = Context::new();
                match parse_router_kind(reply) {
                    Some(kind) => out.set(PARSED_KIND_KEY, kind),
                    None => out.set(NO_KIND_KEY, true),
                }
                Ok(out)
            },
        ),
        step_def(
            "the router decision reply is parsed",
            &[(REPLY_KEY, "String")],
            &[
                (PARSED_KIND_KEY, "String"),
                (PARSED_WHY_KEY, "String"),
                (NO_KIND_KEY, "bool"),
            ],
            |ctx, _params| {
                let reply = ctx.get::<String>(REPLY_KEY).ok_or("No reply")?;
                let mut out = Context::new();
                match parse_router_decision(reply) {
                    Some(decision) => {
                        out.set(PARSED_KIND_KEY, decision.kind);
                        out.set(PARSED_WHY_KEY, decision.why);
                    }
                    None => out.set(NO_KIND_KEY, true),
                }
                Ok(out)
            },
        ),
        check_def(
            "the parsed router kind is {string}",
            &[(PARSED_KIND_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected kind")?;
                let got = ctx.get::<String>(PARSED_KIND_KEY).ok_or("No kind parsed")?;
                if got == want.as_ref() as &str {
                    Ok(())
                } else {
                    Err(format!("kind: expected '{}', got '{}'", want, got))
                }
            },
        ),
        check_def(
            "the parsed router why is {string}",
            &[(PARSED_WHY_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected why")?;
                let got = ctx.get::<String>(PARSED_WHY_KEY).ok_or("No why parsed")?;
                if got == want.as_ref() as &str {
                    Ok(())
                } else {
                    Err(format!("why: expected '{}', got '{}'", want, got))
                }
            },
        ),
        check_def(
            "no router kind is parsed",
            &[(NO_KIND_KEY, "bool")],
            |ctx, _params| {
                if ctx.get::<bool>(NO_KIND_KEY).copied().unwrap_or(false) {
                    Ok(())
                } else {
                    Err("expected no kind, but one was parsed".to_string())
                }
            },
        ),
        // ---- resume_aware_routing: resume outcome render ----
        step_def(
            "a route-turn outcome resume for kind {string} state {string} artifact {string} guidance {string} advance {string}",
            &[],
            &[(OUTCOME_KEY, "RouteTurnOutcome")],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let artifact_id = params.get_string(2).ok_or("Expected artifact")?.to_string();
                let guidance = params.get_string(3).ok_or("Expected guidance")?.to_string();
                let advance_action = params.get_string(4).ok_or("Expected advance")?.to_string();
                let mut out = Context::new();
                out.set(
                    OUTCOME_KEY,
                    RouteTurnOutcome::Resume {
                        artifact_id,
                        kind,
                        state,
                        guidance,
                        advance_action,
                    },
                );
                Ok(out)
            },
        ),
        // ---- the delivered kind: what the row records about this turn ----
        step_def(
            "the guidance kind is projected",
            &[(OUTCOME_KEY, "RouteTurnOutcome")],
            &[(GUIDANCE_KIND_KEY, "String")],
            |ctx, _params| {
                let outcome = ctx
                    .get::<RouteTurnOutcome>(OUTCOME_KEY)
                    .ok_or("No outcome")?;
                let mut out = Context::new();
                out.set(GUIDANCE_KIND_KEY, guidance_kind_of(outcome).to_string());
                Ok(out)
            },
        ),
        check_def(
            "the guidance kind is {string}",
            &[(GUIDANCE_KIND_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected kind")?;
                let got = ctx
                    .get::<String>(GUIDANCE_KIND_KEY)
                    .ok_or("No guidance kind")?;
                if got == want {
                    Ok(())
                } else {
                    Err(format!("guidance kind: expected '{}', got '{}'", want, got))
                }
            },
        ),
        check_def(
            "the guidance kind is empty",
            &[(GUIDANCE_KIND_KEY, "String")],
            |ctx, _params| {
                let got = ctx
                    .get::<String>(GUIDANCE_KIND_KEY)
                    .ok_or("No guidance kind")?;
                if got.is_empty() {
                    Ok(())
                } else {
                    Err(format!("expected no guidance kind, got '{}'", got))
                }
            },
        ),
    ]
}
