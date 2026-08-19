//! Step module for `route_call_state.feature` (Layer 1 — call-state seam).
//!
//! Exercises the pure `classify_call_state` classifier as the engine uses it:
//! the call-state is a function of the route outcome AND the conversation's
//! open-playbook lookup. The "When" step runs the real
//! `find_open_playbook_run_for_conversation` over an `InMemoryQueryAdapter` (resolved
//! against the real `SeedPlaybookRegistry`, where `track`'s `abandoned` state is
//! terminal and `spec` is not) and feeds its result into `classify_call_state`,
//! so the terminal-exclusion and empty-conversation fallbacks are proven
//! end-to-end alongside the classification mapping.

use anvil_core::domain::playbook::registry::{RouteOutcome, SeedPlaybookRegistry};
use anvil_core::domain::route::{classify_call_state, find_open_playbook_run_for_conversation};
use anvil_core::domain::shared_types::{ActivityEntry, ActivityLog};
use anvil_core::domain::status::{FullStatusYaml, StatusTransition};
use anvil_core_hearth::in_memory_query_adapter::InMemoryQueryAdapter;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

const ADAPTER_KEY: &str = "ccs_adapter";
const OUTCOME_KEY: &str = "ccs_outcome";
const RESULT_KEY: &str = "ccs_result";

fn full_status_with_begin(
    kind: &str,
    state: &str,
    conversation_id: &str,
    begun_at: &str,
) -> FullStatusYaml {
    FullStatusYaml {
        version: Some(1),
        kind: Some(kind.to_string()),
        state: Some(state.to_string()),
        origin_turn: None,
        parent_id: None,
        actors: None,
        transitions: Some(vec![StatusTransition {
            to: state.to_string(),
            at: Some("2026-06-01T00:00:00Z".to_string()),
            actor: Some("Creator-000000".to_string()),
            role: Some("doer".to_string()),
            approver: None,
            note: None,
            event_type: None,
            satisfaction: None,
        }]),
        activity: Some(ActivityLog::new(vec![ActivityEntry {
            kind: "begin".to_string(),
            actor: "Beginner-111111".to_string(),
            state: state.to_string(),
            at: begun_at.to_string(),
            conversation_id: conversation_id.to_string(),
        }])),
        contributed_by: None,
    }
}

fn take_or_new(ctx: &mut Context) -> InMemoryQueryAdapter {
    ctx.take::<InMemoryQueryAdapter>(ADAPTER_KEY)
        .unwrap_or_default()
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a call-state route outcome {string}",
            &[],
            &[(OUTCOME_KEY, "String")],
            |_ctx, params| {
                let outcome = params.get_string(0).ok_or("Expected outcome")?.to_string();
                if outcome != "matched" && outcome != "no_match" {
                    return Err(format!(
                        "Unknown call-state route outcome '{}' (expected 'matched' or 'no_match')",
                        outcome
                    ));
                }
                let mut out = Context::new();
                out.set(OUTCOME_KEY, outcome);
                Ok(out)
            },
        ),
        step_def(
            "a call-state open playbook {string} kind {string} state {string} for conversation {string} begun at {string}",
            &[(OUTCOME_KEY, "String")],
            &[
                (ADAPTER_KEY, "InMemoryQueryAdapter"),
                (OUTCOME_KEY, "String"),
            ],
            |mut ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let kind = params.get_string(1).ok_or("Expected kind")?.to_string();
                let state = params.get_string(2).ok_or("Expected state")?.to_string();
                let conv = params.get_string(3).ok_or("Expected conversation")?.to_string();
                let at = params.get_string(4).ok_or("Expected begun-at")?.to_string();
                let outcome = ctx
                    .get::<String>(OUTCOME_KEY)
                    .cloned()
                    .unwrap_or_default();
                let mut adapter = take_or_new(&mut ctx);
                adapter.with_artifact(&id, &kind, &state);
                adapter.with_status(&id, full_status_with_begin(&kind, &state, &conv, &at));
                let mut out = Context::new();
                out.set(ADAPTER_KEY, adapter);
                out.set(OUTCOME_KEY, outcome);
                Ok(out)
            },
        ),
        step_def(
            "the call-state is classified for conversation {string}",
            &[(OUTCOME_KEY, "String")],
            &[(RESULT_KEY, "String")],
            |mut ctx, params| {
                let conv = params.get_string(0).ok_or("Expected conversation")?.to_string();
                let outcome = match ctx.get::<String>(OUTCOME_KEY).map(|s| s.as_str()) {
                    Some("no_match") => RouteOutcome::NoMatch,
                    Some("matched") => RouteOutcome::Single,
                    other => return Err(format!("No/unknown route outcome seeded: {:?}", other)),
                };
                let adapter = take_or_new(&mut ctx);
                let open = find_open_playbook_run_for_conversation(&adapter, &SeedPlaybookRegistry, &conv)
                    .map_err(|e| format!("open-playbook lookup failed: {}", e))?;
                // No matching set supplied → an open playbook is NEVER relevant, so
                // a matched turn with an open playbook classifies as
                // start_opportunity by construction (used where the open lookup
                // returns None anyway).
                let call_state = classify_call_state(&outcome, open.as_ref(), &[]);
                let mut out = Context::new();
                out.set(RESULT_KEY, call_state.as_str().to_string());
                Ok(out)
            },
        ),
        step_def(
            "the call-state is classified for conversation {string} matching {string}",
            &[(OUTCOME_KEY, "String")],
            &[(RESULT_KEY, "String")],
            |mut ctx, params| {
                let conv = params.get_string(0).ok_or("Expected conversation")?.to_string();
                let matching: Vec<String> = params
                    .get_string(1)
                    .ok_or("Expected matching candidates")?
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                let outcome = match ctx.get::<String>(OUTCOME_KEY).map(|s| s.as_str()) {
                    Some("no_match") => RouteOutcome::NoMatch,
                    Some("matched") => RouteOutcome::Single,
                    other => return Err(format!("No/unknown route outcome seeded: {:?}", other)),
                };
                let adapter = take_or_new(&mut ctx);
                let open = find_open_playbook_run_for_conversation(&adapter, &SeedPlaybookRegistry, &conv)
                    .map_err(|e| format!("open-playbook lookup failed: {}", e))?;
                let call_state = classify_call_state(&outcome, open.as_ref(), &matching);
                let mut out = Context::new();
                out.set(RESULT_KEY, call_state.as_str().to_string());
                Ok(out)
            },
        ),
        check_def(
            "the call-state is {string}",
            &[(RESULT_KEY, "String")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected call-state")?.to_string();
                let actual = ctx.get::<String>(RESULT_KEY).ok_or("No call-state result")?;
                if actual == &expected {
                    Ok(())
                } else {
                    Err(format!("Expected call-state '{}', got '{}'", expected, actual))
                }
            },
        ),
    ]
}
