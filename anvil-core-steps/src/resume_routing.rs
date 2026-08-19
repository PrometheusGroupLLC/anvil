//! Step module for `resume_routing.feature` (resume_aware_routing — Phase 2/3
//! pure core, complementary to `route_resolution`'s fixture-registry lookup
//! coverage).
//!
//! Exercises the pure resume seam in `anvil_core::domain::route` against an
//! `InMemoryQueryAdapter` seeded with `activity:` begin markers carrying a
//! `conversation_id`, resolving terminality against the real
//! `SeedPlaybookRegistry` (track machine). The `is_continuation_token` predicate
//! is covered here too, alongside the open-playbook lookup's multiplicity and
//! terminal-exclusion behavior.

use anvil_core::domain::playbook::registry::SeedPlaybookRegistry;
use anvil_core::domain::route::{find_open_playbook_run_for_conversation, is_continuation_token};
use anvil_core::domain::shared_types::{ActivityEntry, ActivityLog};
use anvil_core::domain::status::{FullStatusYaml, StatusTransition};
use anvil_core_hearth::in_memory_query_adapter::InMemoryQueryAdapter;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

const ADAPTER_KEY: &str = "resume_adapter";
const TOKEN_RESULT_KEY: &str = "resume_token_result";
const LOOKUP_RESULT_KEY: &str = "resume_lookup_result";

/// (artifact_id, kind, state) of the open playbook the lookup returned, or
/// `("", "", "")` for "no open playbook".
type LookupTriple = (String, String, String);

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

fn seed_artifact(ctx: &mut Context, id: &str, kind: &str, state: &str, conv: &str, at: &str) {
    let mut adapter = take_or_new(ctx);
    adapter.with_artifact(id, kind, state);
    adapter.with_status(id, full_status_with_begin(kind, state, conv, at));
    ctx.set(ADAPTER_KEY, adapter);
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "the continuation predicate is evaluated for {string}",
            &[],
            &[(TOKEN_RESULT_KEY, "bool")],
            |_ctx, params| {
                let msg = params.get_string(0).ok_or("Expected message")?.to_string();
                let mut out = Context::new();
                out.set(TOKEN_RESULT_KEY, is_continuation_token(&msg));
                Ok(out)
            },
        ),
        check_def(
            "the evaluated message is a continuation token",
            &[(TOKEN_RESULT_KEY, "bool")],
            |ctx, _params| {
                let r = ctx.get::<bool>(TOKEN_RESULT_KEY).ok_or("No token result")?;
                if *r {
                    Ok(())
                } else {
                    Err("Expected a continuation token, got non-token".to_string())
                }
            },
        ),
        check_def(
            "the evaluated message is not a continuation token",
            &[(TOKEN_RESULT_KEY, "bool")],
            |ctx, _params| {
                let r = ctx.get::<bool>(TOKEN_RESULT_KEY).ok_or("No token result")?;
                if *r {
                    Err("Expected a non-token, got a continuation token".to_string())
                } else {
                    Ok(())
                }
            },
        ),
        step_def(
            "a resume adapter with track {string} kind {string} state {string} conversation_id {string} begun at {string}",
            &[],
            &[(ADAPTER_KEY, "InMemoryQueryAdapter")],
            |mut ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let kind = params.get_string(1).ok_or("Expected kind")?.to_string();
                let state = params.get_string(2).ok_or("Expected state")?.to_string();
                let conv = params.get_string(3).ok_or("Expected conversation_id")?.to_string();
                let at = params.get_string(4).ok_or("Expected begun-at")?.to_string();
                seed_artifact(&mut ctx, &id, &kind, &state, &conv, &at);
                let adapter = take_or_new(&mut ctx);
                let mut out = Context::new();
                out.set(ADAPTER_KEY, adapter);
                Ok(out)
            },
        ),
        step_def(
            "the open playbook is looked up for conversation_id {string}",
            &[(ADAPTER_KEY, "InMemoryQueryAdapter")],
            &[
                (ADAPTER_KEY, "InMemoryQueryAdapter"),
                (LOOKUP_RESULT_KEY, "LookupTriple"),
            ],
            |mut ctx, params| {
                let conv = params.get_string(0).ok_or("Expected conversation_id")?.to_string();
                let adapter = take_or_new(&mut ctx);
                let found = find_open_playbook_run_for_conversation(&adapter, &SeedPlaybookRegistry, &conv)
                    .map_err(|e| format!("lookup failed: {}", e))?;
                let triple: LookupTriple = match found {
                    Some(w) => (w.artifact_id, w.kind, w.state),
                    None => (String::new(), String::new(), String::new()),
                };
                let mut out = Context::new();
                out.set(ADAPTER_KEY, adapter);
                out.set(LOOKUP_RESULT_KEY, triple);
                Ok(out)
            },
        ),
        check_def(
            "the resume lookup returns artifact {string} kind {string} state {string}",
            &[(LOOKUP_RESULT_KEY, "LookupTriple")],
            |ctx, params| {
                let eid = params.get_string(0).ok_or("Expected id")?.to_string();
                let ek = params.get_string(1).ok_or("Expected kind")?.to_string();
                let es = params.get_string(2).ok_or("Expected state")?.to_string();
                let (id, k, s) = ctx.get::<LookupTriple>(LOOKUP_RESULT_KEY).ok_or("No lookup result")?;
                if id == &eid && k == &ek && s == &es {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected ({}, {}, {}), got ({}, {}, {})",
                        eid, ek, es, id, k, s
                    ))
                }
            },
        ),
        check_def(
            "the resume lookup returns no open playbook",
            &[(LOOKUP_RESULT_KEY, "LookupTriple")],
            |ctx, _params| {
                let (id, _, _) = ctx.get::<LookupTriple>(LOOKUP_RESULT_KEY).ok_or("No lookup result")?;
                if id.is_empty() {
                    Ok(())
                } else {
                    Err(format!("Expected no open playbook, got artifact '{}'", id))
                }
            },
        ),
    ]
}
