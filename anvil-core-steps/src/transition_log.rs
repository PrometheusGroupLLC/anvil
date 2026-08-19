//! Step definitions for the consolidated transition-log resolution seam
//! (`anvil_core::domain::transition_log`).
//!
//! The seam is the single chokepoint every consumer routes through to answer
//! "what state is this artifact in?" and "what is its transition history?".
//! These steps build a `FullStatusYaml` (the parsed legacy status.yaml) and
//! exercise the three seam functions directly — no filesystem adapter is
//! involved:
//!
//! * `resolve_state`       — current state (top-level `state`, else last
//!                            transition's `to`, else None).
//! * `declared_state`      — the raw top-level `state:` field only.
//! * `resolve_transitions` — the ordered transition history.

use anvil_core::domain::status::{FullStatusYaml, StatusTransition};
use anvil_core::domain::transition_log;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

type MaybeState = Option<String>;
type History = Vec<String>;

fn transition_to(to: &str) -> StatusTransition {
    StatusTransition {
        to: to.to_string(),
        at: None,
        actor: None,
        role: None,
        approver: None,
        note: None,
        event_type: None,
        satisfaction: None,
    }
}

/// Build a status from an optional top-level state and a comma-separated,
/// ordered list of transition `to` values. An empty history string means
/// no `transitions:` key at all.
fn build_status(state: Option<String>, history_csv: &str) -> FullStatusYaml {
    let transitions: Option<Vec<StatusTransition>> = if history_csv.trim().is_empty() {
        None
    } else {
        Some(
            history_csv
                .split(',')
                .map(|s| transition_to(s.trim()))
                .collect(),
        )
    };
    FullStatusYaml {
        version: Some(1),
        kind: Some("track".to_string()),
        state,
        origin_turn: None,
        parent_id: None,
        actors: None,
        transitions,
        activity: None,
        contributed_by: None,
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a legacy status with top-level state {string} and transitions {string}",
            &[],
            &[("tl_status", "FullStatusYaml")],
            |_ctx, params| {
                let state = params.get_string(0).ok_or("Expected state")?.to_string();
                let history = params.get_string(1).ok_or("Expected transitions")?;
                let status = build_status(Some(state), history.as_ref());
                let mut out = Context::new();
                out.set("tl_status", status);
                Ok(out)
            },
        ),
        step_def(
            "a legacy status with no top-level state and transitions {string}",
            &[],
            &[("tl_status", "FullStatusYaml")],
            |_ctx, params| {
                let history = params.get_string(0).ok_or("Expected transitions")?;
                let status = build_status(None, history.as_ref());
                let mut out = Context::new();
                out.set("tl_status", status);
                Ok(out)
            },
        ),
        step_def(
            "the transition-log seam resolves the status",
            &[("tl_status", "FullStatusYaml")],
            &[
                ("tl_state", "MaybeState"),
                ("tl_declared", "MaybeState"),
                ("tl_history", "History"),
            ],
            |ctx, _params| {
                let status = ctx
                    .get::<FullStatusYaml>("tl_status")
                    .ok_or("No tl_status")?;
                let state: MaybeState = transition_log::resolve_state(&status);
                let declared: MaybeState = transition_log::declared_state(&status);
                let history: History = transition_log::resolve_transitions(&status)
                    .into_iter()
                    .map(|t| t.to)
                    .collect();
                let mut out = Context::new();
                out.set("tl_state", state);
                out.set("tl_declared", declared);
                out.set("tl_history", history);
                Ok(out)
            },
        ),
        check_def(
            "the seam current state is {string}",
            &[("tl_state", "MaybeState")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?;
                let state = ctx.get::<MaybeState>("tl_state").ok_or("No tl_state")?;
                match state {
                    Some(s) if s == expected.as_ref() as &str => Ok(()),
                    Some(s) => Err(format!(
                        "Expected current state '{}', got '{}'",
                        expected, s
                    )),
                    None => Err(format!("Expected current state '{}', got None", expected)),
                }
            },
        ),
        check_def(
            "the seam declared state is {string}",
            &[("tl_declared", "MaybeState")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?;
                let declared = ctx
                    .get::<MaybeState>("tl_declared")
                    .ok_or("No tl_declared")?;
                match declared {
                    Some(s) if s == expected.as_ref() as &str => Ok(()),
                    Some(s) => Err(format!(
                        "Expected declared state '{}', got '{}'",
                        expected, s
                    )),
                    None => Err(format!("Expected declared state '{}', got None", expected)),
                }
            },
        ),
        check_def(
            "the seam declared state is empty",
            &[("tl_declared", "MaybeState")],
            |ctx, _params| {
                let declared = ctx
                    .get::<MaybeState>("tl_declared")
                    .ok_or("No tl_declared")?;
                match declared {
                    None => Ok(()),
                    Some(s) => Err(format!("Expected no declared state, got '{}'", s)),
                }
            },
        ),
        check_def(
            "the seam history in order is {string}",
            &[("tl_history", "History")],
            |ctx, params| {
                let expected_csv = params.get_string(0).ok_or("Expected history")?;
                let expected: Vec<String> = (expected_csv.as_ref() as &str)
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                let history = ctx.get::<History>("tl_history").ok_or("No tl_history")?;
                if history.as_ref() as &[String] == expected.as_slice() {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected ordered history {:?}, got {:?}",
                        expected, history
                    ))
                }
            },
        ),
        check_def(
            "the seam history is empty",
            &[("tl_history", "History")],
            |ctx, _params| {
                let history = ctx.get::<History>("tl_history").ok_or("No tl_history")?;
                if history.is_empty() {
                    Ok(())
                } else {
                    Err(format!("Expected empty history, got {:?}", history))
                }
            },
        ),
    ]
}
