//! Step module for `hook_serve_seam_parity.feature` (route_response_mirrors_begin
//! Phase 1).
//!
//! Exercises the shared `hook_serve::serve_hook_body` seam directly against the
//! same in-memory query adapter + track registry the begin-create hook scenarios
//! use, proving the seam returns the same resolved + budget-capped,
//! PRE-interpolation body begin reads for a (kind, initial_state, doer).

use anvil_core::domain::playbook::hook_serve::{serve_hook_body, PlaybookHookBodyPort};
use anvil_core::domain::playbook::registry::{PlaybookRegistry, PlaybookSource};
use anvil_core::domain::playbook::types::{PlaybookMachine, StateDefinition};
use anvil_core_hearth::in_memory_query_adapter::InMemoryQueryAdapter;
use anvil_core::ports::query_port::{QueryError, QueryPort};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

const SERVED_KEY: &str = "seam_served_body";
const QUERY_KEY: &str = "begin_query";

/// A minimal single-state track registry: state `spec` declares a `(doer)` hook
/// and the kind resolves to the `20260422T0000_track_lifecycle` playbook id (so
/// the seeded hook body under that id resolves).
struct SeamTrackRegistry {
    machine: PlaybookMachine,
}

impl SeamTrackRegistry {
    fn new(doer_hook: &str) -> Self {
        let mut state = StateDefinition {
            name: "spec".to_string(),
            role_filters: Vec::new(),
            registry_section: "active".to_string(),
            projection_targets: Vec::new(),
            is_review_gate: false,
            is_terminal: false,
            hook: None,
            hooks_by_role: Default::default(),
            measurement_by_role: Default::default(),
        };
        state
            .hooks_by_role
            .insert("doer".to_string(), doer_hook.to_string());
        SeamTrackRegistry {
            machine: PlaybookMachine {
                kind: "track".to_string(),
                directory: "tracks".to_string(),
                registry: "tracks.md".to_string(),
                description: "track".to_string(),
                roles: vec!["doer".to_string(), "reviewer".to_string()],
                states: vec![state],
                ..Default::default()
            },
        }
    }
}

impl PlaybookRegistry for SeamTrackRegistry {
    fn machine_for<'a>(&'a self, kind: &str) -> Option<&'a PlaybookMachine>
    where
        Self: 'a,
    {
        (kind == "track").then_some(&self.machine)
    }

    fn playbook_id_for(&self, kind: &str) -> Option<String> {
        (kind == "track").then(|| "20260422T0000_track_lifecycle".to_string())
    }
}

/// Reader that bridges the seam's `PlaybookHookBodyPort` to the in-memory
/// query adapter (the same body store begin reads from).
struct QueryReader<'a> {
    query: &'a dyn QueryPort,
}

impl PlaybookHookBodyPort for QueryReader<'_> {
    fn read_playbook_hook_body(
        &self,
        source: &PlaybookSource,
        filename: &str,
    ) -> Result<String, QueryError> {
        self.query
            .read_playbook_hook_body(&source.playbook_id, filename)
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "the shared seam serves the track spec doer hook {string}",
            &[(QUERY_KEY, "InMemoryQueryAdapter")],
            &[(QUERY_KEY, "InMemoryQueryAdapter"), (SERVED_KEY, "String")],
            |mut ctx, params| {
                let filename = params.get_string(0).ok_or("Expected filename")?.to_string();
                let query = ctx
                    .take::<InMemoryQueryAdapter>(QUERY_KEY)
                    .ok_or("No begin_query")?;
                let registry = SeamTrackRegistry::new(&filename);
                let reader = QueryReader { query: &query };
                let served = serve_hook_body(&reader, &registry, "track", "spec", "doer")
                    .map_err(|e| format!("seam serve failed: {:?}", e))?;
                let mut out = Context::new();
                out.set(QUERY_KEY, query);
                out.set(SERVED_KEY, served);
                Ok(out)
            },
        ),
        check_def(
            "the served seam body equals {string}",
            &[(SERVED_KEY, "String")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected body")?.to_string();
                let actual = ctx.get::<String>(SERVED_KEY).ok_or("No served body")?;
                if actual == &expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected served body {:?}, got {:?}",
                        expected, actual
                    ))
                }
            },
        ),
        check_def(
            "the served seam body retains the literal placeholder {string}",
            &[(SERVED_KEY, "String")],
            |ctx, params| {
                let placeholder = params
                    .get_string(0)
                    .ok_or("Expected placeholder")?
                    .to_string();
                let actual = ctx.get::<String>(SERVED_KEY).ok_or("No served body")?;
                if actual.contains(&placeholder) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected served body to retain literal {:?}, got {:?}",
                        placeholder, actual
                    ))
                }
            },
        ),
    ]
}
