//! Step definitions for the shared `FullStatusYaml::resolved_state()` accessor.
//!
//! These steps construct a `FullStatusYaml` directly (the canonical status
//! type) and exercise the accessor in isolation — no filesystem adapter is
//! involved. The accessor is the single fallback definition that all four
//! filesystem read sites route through.

use anvil_core::domain::status::{FullStatusYaml, StatusTransition};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

type ResolvedState = Option<String>;

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

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a status with top-level state {string} and last transition to {string}",
            &[],
            &[("full_status", "FullStatusYaml")],
            |_ctx, params| {
                let state = params.get_string(0).ok_or("Expected state")?.to_string();
                let last_to = params
                    .get_string(1)
                    .ok_or("Expected transition to")?
                    .to_string();
                let status = FullStatusYaml {
                    version: Some(1),
                    kind: Some("track".to_string()),
                    state: Some(state),
                    origin_turn: None,
                    parent_id: None,
                    actors: None,
                    transitions: Some(vec![transition_to("spec"), transition_to(&last_to)]),
                    activity: None,
                    contributed_by: None,
                };
                let mut out = Context::new();
                out.set("full_status", status);
                Ok(out)
            },
        ),
        step_def(
            "a status with no top-level state and last transition to {string}",
            &[],
            &[("full_status", "FullStatusYaml")],
            |_ctx, params| {
                let last_to = params
                    .get_string(0)
                    .ok_or("Expected transition to")?
                    .to_string();
                let status = FullStatusYaml {
                    version: Some(1),
                    kind: Some("decision".to_string()),
                    state: None,
                    origin_turn: None,
                    parent_id: None,
                    actors: None,
                    transitions: Some(vec![transition_to("tension"), transition_to(&last_to)]),
                    activity: None,
                    contributed_by: None,
                };
                let mut out = Context::new();
                out.set("full_status", status);
                Ok(out)
            },
        ),
        step_def(
            "a status with no top-level state and no transitions",
            &[],
            &[("full_status", "FullStatusYaml")],
            |_ctx, _params| {
                let status = FullStatusYaml {
                    version: Some(1),
                    kind: Some("decision".to_string()),
                    state: None,
                    origin_turn: None,
                    parent_id: None,
                    actors: None,
                    transitions: None,
                    activity: None,
                    contributed_by: None,
                };
                let mut out = Context::new();
                out.set("full_status", status);
                Ok(out)
            },
        ),
        step_def(
            "resolved_state is computed",
            &[("full_status", "FullStatusYaml")],
            &[("resolved_state", "ResolvedState")],
            |ctx, _params| {
                let status = ctx
                    .get::<FullStatusYaml>("full_status")
                    .ok_or("No full_status")?;
                let resolved: ResolvedState = status.resolved_state();
                let mut out = Context::new();
                out.set("resolved_state", resolved);
                Ok(out)
            },
        ),
        check_def(
            "the resolved state is {string}",
            &[("resolved_state", "ResolvedState")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?;
                let resolved = ctx
                    .get::<ResolvedState>("resolved_state")
                    .ok_or("No resolved_state")?;
                match resolved {
                    Some(s) if s == expected.as_ref() as &str => Ok(()),
                    Some(s) => Err(format!(
                        "Expected resolved state '{}', got '{}'",
                        expected, s
                    )),
                    None => Err(format!(
                        "Expected resolved state '{}', got None (unresolvable)",
                        expected
                    )),
                }
            },
        ),
        check_def(
            "the resolved state is unresolvable",
            &[("resolved_state", "ResolvedState")],
            |ctx, _params| {
                let resolved = ctx
                    .get::<ResolvedState>("resolved_state")
                    .ok_or("No resolved_state")?;
                match resolved {
                    None => Ok(()),
                    Some(s) => Err(format!("Expected unresolvable (None), got '{}'", s)),
                }
            },
        ),
    ]
}
