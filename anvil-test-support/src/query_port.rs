use crate::{carry_retained_temp_dir, retained_temp_dir, RetainedTempDir};
use anvil_core::domain::begin::{
    reserve_hook_for_open_begin_via_query, BeginCommandHandler, BeginError, BeginOutcome,
    BeginRequest,
};
use anvil_core::domain::begin_adoption::has_open_begin;
use anvil_core::domain::events::Event;
use anvil_core::domain::playbook::composite_registry::CompositePlaybookRegistry;
use anvil_core::domain::playbook::hearth_registry::HearthPlaybookRegistry;
use anvil_core::domain::playbook::registry::{PlaybookRegistry, SeedPlaybookRegistry};
use anvil_core::domain::playbook::types::{PlaybookMachine, RouteConfig, StateDefinition};
use anvil_core::domain::shared_types::{ActivityEntry, ActivityLog};
use anvil_core::domain::status::{FullStatusYaml, StatusTransition};
use anvil_core_hearth::fs_query_adapter::FileSystemQueryAdapter;
use anvil_core_hearth::in_memory_artifact_adapter::InMemoryArtifactAdapter;
use anvil_core_hearth::in_memory_query_adapter::InMemoryQueryAdapter;
use anvil_core::ports::artifact_port::{ArtifactError, ArtifactPort};
use anvil_core::ports::query_port::{QueryError, QueryPort};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// A fixture `PlaybookRegistry` for the begin create-flow hook-serving tests.
///
/// Holds a single owned `track` `PlaybookMachine` whose `spec` state may declare
/// a `hooks_by_role.doer` entry. This lets the in-memory create-flow scenarios
/// exercise hook resolution before the seed declares the doer hook (P5). The
/// `playbook_id_for("track")` answer matches the seed id so body reads hit the
/// seeded `(playbook_id, filename)` pair.
///
/// Also used (via `new_reviewer`) by the P4 reviewer-flow hook-serving tests,
/// where the `spec_review` state may declare a `hooks_by_role.reviewer` entry.
struct FixtureTrackRegistry {
    machine: PlaybookMachine,
}

impl FixtureTrackRegistry {
    /// Build a fixture registry whose `track` machine's `spec` state declares
    /// `hooks_by_role.doer = doer_hook` (when `Some`).
    fn new(doer_hook: Option<&str>) -> Self {
        let mut hooks_by_role: BTreeMap<String, String> = BTreeMap::new();
        if let Some(filename) = doer_hook {
            hooks_by_role.insert("doer".to_string(), filename.to_string());
        }
        let spec_state = StateDefinition {
            name: "spec".to_string(),
            role_filters: vec![],
            registry_section: "Active".to_string(),
            projection_targets: vec![],
            is_review_gate: false,
            is_terminal: false,
            hook: None,
            hooks_by_role,
            measurement_by_role: std::collections::BTreeMap::new(),
        };
        let machine = PlaybookMachine {
            kind: "track".to_string(),
            directory: "tracks".to_string(),
            registry: "tracks.md".to_string(),
            parent_kind: None,
            description: "Begin create-flow hook fixture".to_string(),
            route: RouteConfig::default(),
            required_fields: vec![],
            roles: vec!["doer".to_string()],
            states: vec![spec_state],
            transitions: vec![],
            register: anvil_core::domain::playbook::types::Register::Driven,
            ..Default::default()
        };
        FixtureTrackRegistry { machine }
    }

    /// Build a fixture registry whose `track` machine's `spec_review` state
    /// declares `hooks_by_role.reviewer = reviewer_hook` (when `Some`).
    /// Used by the P4 reviewer-flow hook-serving tests.
    fn new_reviewer(reviewer_hook: Option<&str>) -> Self {
        let mut hooks_by_role: BTreeMap<String, String> = BTreeMap::new();
        if let Some(filename) = reviewer_hook {
            hooks_by_role.insert("reviewer".to_string(), filename.to_string());
        }
        let spec_review_state = StateDefinition {
            name: "spec_review".to_string(),
            role_filters: vec![],
            registry_section: "Active".to_string(),
            projection_targets: vec![],
            is_review_gate: true,
            is_terminal: false,
            hook: None,
            hooks_by_role,
            measurement_by_role: std::collections::BTreeMap::new(),
        };
        let machine = PlaybookMachine {
            kind: "track".to_string(),
            directory: "tracks".to_string(),
            registry: "tracks.md".to_string(),
            parent_kind: None,
            description: "Begin reviewer-flow hook fixture".to_string(),
            route: RouteConfig::default(),
            required_fields: vec![],
            roles: vec!["doer".to_string(), "reviewer".to_string()],
            states: vec![spec_review_state],
            transitions: vec![],
            register: anvil_core::domain::playbook::types::Register::Driven,
            ..Default::default()
        };
        FixtureTrackRegistry { machine }
    }

    fn new_state_role(state: &str, role: &str, hook: &str) -> Self {
        let mut hooks_by_role: BTreeMap<String, String> = BTreeMap::new();
        hooks_by_role.insert(role.to_string(), hook.to_string());
        let state = StateDefinition {
            name: state.to_string(),
            role_filters: vec![],
            registry_section: String::new(),
            projection_targets: vec![],
            is_review_gate: role == "reviewer",
            is_terminal: false,
            hook: None,
            hooks_by_role,
            measurement_by_role: std::collections::BTreeMap::new(),
        };
        let machine = PlaybookMachine {
            kind: "track".to_string(),
            directory: "tracks".to_string(),
            registry: "tracks.md".to_string(),
            parent_kind: None,
            description: "Begin migrated hook fixture".to_string(),
            route: RouteConfig::default(),
            required_fields: vec![],
            roles: vec![
                "doer".to_string(),
                "complete".to_string(),
                "reviewer".to_string(),
            ],
            states: vec![state],
            transitions: vec![],
            register: anvil_core::domain::playbook::types::Register::Driven,
            ..Default::default()
        };
        FixtureTrackRegistry { machine }
    }
}

impl PlaybookRegistry for FixtureTrackRegistry {
    fn machine_for<'a>(&'a self, kind: &str) -> Option<&'a PlaybookMachine>
    where
        Self: 'a,
    {
        match kind {
            "track" => Some(&self.machine),
            _ => None,
        }
    }

    fn playbook_id_for(&self, kind: &str) -> Option<String> {
        match kind {
            "track" => Some("20260422T0000_track_lifecycle".to_string()),
            _ => None,
        }
    }
}

/// Build a minimal FullStatusYaml with just the state field set.
fn minimal_status(kind: &str, state: &str) -> FullStatusYaml {
    FullStatusYaml {
        version: Some(1),
        kind: Some(kind.to_string()),
        state: Some(state.to_string()),
        origin_turn: None,
        parent_id: None,
        actors: None,
        transitions: None,
        activity: None,
        contributed_by: None,
    }
}

/// Parse a Gherkin data table with header row `| kind | actor | state | at |`
/// into a vec of `ActivityEntry`. The header row lands in `table.headers`;
/// data rows in `table.rows`.
fn parse_activity_table(
    table: &brine_core::parser::DataTable,
) -> Result<Vec<ActivityEntry>, String> {
    let cols = &table.headers;
    let idx = |name: &str| cols.iter().position(|h| h.trim() == name);
    let (ik, ia, is, iat) = (
        idx("kind").ok_or("activity table needs a 'kind' column")?,
        idx("actor").ok_or("activity table needs an 'actor' column")?,
        idx("state").ok_or("activity table needs a 'state' column")?,
        idx("at").ok_or("activity table needs an 'at' column")?,
    );
    let mut out = Vec::new();
    for row in &table.rows {
        out.push(ActivityEntry {
            kind: row[ik].trim().to_string(),
            actor: row[ia].trim().to_string(),
            state: row[is].trim().to_string(),
            at: row[iat].trim().to_string(),
            conversation_id: idx("conversation_id")
                .map(|ic| row[ic].trim().to_string())
                .unwrap_or_default(),
        });
    }
    Ok(out)
}

/// Parse a Gherkin data table with header row `| to | actor | at | role |`
/// into a vec of `StatusTransition`. `actor`, `at`, `role` are optional
/// columns; `to` is required.
fn parse_transitions_table(
    table: &brine_core::parser::DataTable,
) -> Result<Vec<StatusTransition>, String> {
    let cols = &table.headers;
    let idx = |name: &str| cols.iter().position(|h| h.trim() == name);
    let ito = idx("to").ok_or("transitions table needs a 'to' column")?;
    let iactor = idx("actor");
    let iat = idx("at");
    let irole = idx("role");
    let cell = |row: &[String], i: Option<usize>| {
        i.and_then(|i| row.get(i))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let mut out = Vec::new();
    for row in &table.rows {
        out.push(StatusTransition {
            to: row[ito].trim().to_string(),
            at: cell(row, iat),
            actor: cell(row, iactor),
            role: cell(row, irole),
            approver: None,
            note: None,
            event_type: None,
            satisfaction: None,
        });
    }
    Ok(out)
}

type QueryOutcome = Result<String, QueryError>;
type ProjectionCheckOutcome = Result<(), QueryError>;
type ArtifactPortOutcome = Result<String, ArtifactError>;

pub fn steps() -> Vec<StepDef> {
    vec![
        // ===== InMemoryQueryAdapter setup =====
        step_def(
            "an in-memory query adapter seeded with artifact {string} kind {string} state {string}",
            &[],
            &[("qp_adapter", "InMemoryQueryAdapter")],
            |_ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let kind = params.get_string(1).ok_or("Expected kind")?.to_string();
                let state = params.get_string(2).ok_or("Expected state")?.to_string();
                let mut adapter = InMemoryQueryAdapter::new();
                adapter.with_artifact(&id, &kind, &state);
                let mut out = Context::new();
                out.set("qp_adapter", adapter);
                Ok(out)
            },
        ),
        // ===== Begin-adoption seeding (activity: + transitions:) =====
        step_def(
            "an in-memory query adapter seeded with artifact {string} kind {string} state {string} with activity:",
            &[],
            &[("qp_adapter", "InMemoryQueryAdapter")],
            |_ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let kind = params.get_string(1).ok_or("Expected kind")?.to_string();
                let state = params.get_string(2).ok_or("Expected state")?.to_string();
                let table = params.data_table().ok_or("Expected activity data table")?;
                let activity = parse_activity_table(table)?;
                let mut adapter = InMemoryQueryAdapter::new();
                adapter.with_artifact(&id, &kind, &state);
                let mut status = minimal_status(&kind, &state);
                status.activity = Some(ActivityLog::new(activity));
                adapter.with_status(&id, status);
                let mut out = Context::new();
                out.set("qp_adapter", adapter);
                Ok(out)
            },
        ),
        step_def(
            "an in-memory query adapter seeded with artifact {string} kind {string} state {string} with transitions:",
            &[],
            &[("qp_adapter", "InMemoryQueryAdapter")],
            |_ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let kind = params.get_string(1).ok_or("Expected kind")?.to_string();
                let state = params.get_string(2).ok_or("Expected state")?.to_string();
                let table = params.data_table().ok_or("Expected transitions data table")?;
                let transitions = parse_transitions_table(table)?;
                let mut adapter = InMemoryQueryAdapter::new();
                adapter.with_artifact(&id, &kind, &state);
                let mut status = minimal_status(&kind, &state);
                status.transitions = Some(transitions);
                adapter.with_status(&id, status);
                let mut out = Context::new();
                out.set("qp_adapter", adapter);
                Ok(out)
            },
        ),
        // ===== Begin-adoption predicate step defs (BP3) =====
        // Adds a single closing transition to the in-memory adapter's status for
        // an already-seeded artifact. This is used in combination with the
        // "with activity:" seeding step to set up the "closed" scenario where
        // has_open_begin should return false.
        step_def(
            "the in-memory query adapter has a closing transition for artifact {string} actor {string} at {string}",
            &[("qp_adapter", "InMemoryQueryAdapter")],
            &[("qp_adapter", "InMemoryQueryAdapter")],
            |mut ctx, params| {
                let artifact_id = params.get_string(0).ok_or("Expected artifact_id")?.to_string();
                let actor = params.get_string(1).ok_or("Expected actor")?.to_string();
                let at = params.get_string(2).ok_or("Expected at")?.to_string();
                let mut adapter = ctx
                    .take::<InMemoryQueryAdapter>("qp_adapter")
                    .ok_or("No qp_adapter")?;
                // Add the transition to the existing status (preserving any
                // activity: already seeded).
                let mut status = adapter
                    .read_artifact_status(&artifact_id)
                    .unwrap_or_else(|_| FullStatusYaml {
                        version: Some(1),
                        kind: None,
                        state: None,
                        origin_turn: None,
                        parent_id: None,
                        actors: None,
                        transitions: None,
                        activity: None,
                        contributed_by: None,
                    });
                let mut transitions = status.transitions.unwrap_or_default();
                transitions.push(StatusTransition {
                    to: "closed".to_string(),
                    at: Some(at),
                    actor: Some(actor),
                    role: None,
                    approver: None,
                    note: None,
                    event_type: None,
                    satisfaction: None,
                });
                status.transitions = Some(transitions);
                adapter.with_status(&artifact_id, status);
                let mut out = Context::new();
                out.set("qp_adapter", adapter);
                Ok(out)
            },
        ),
        // Finding 3 harness: seed an ADOPTION transition (event_type "adoption")
        // by `actor` at `at` on an already-seeded artifact. Unlike an ordinary
        // closing transition, an adoption transition must NOT close a same-actor
        // begin marker — `has_open_begin` skips it — so the adopted artifact reads
        // as still-begun. Mirrors the "closing transition" step but stamps the
        // structural discriminator.
        step_def(
            "the in-memory query adapter has an adoption transition for artifact {string} actor {string} at {string}",
            &[("qp_adapter", "InMemoryQueryAdapter")],
            &[("qp_adapter", "InMemoryQueryAdapter")],
            |mut ctx, params| {
                let artifact_id = params.get_string(0).ok_or("Expected artifact_id")?.to_string();
                let actor = params.get_string(1).ok_or("Expected actor")?.to_string();
                let at = params.get_string(2).ok_or("Expected at")?.to_string();
                let mut adapter = ctx
                    .take::<InMemoryQueryAdapter>("qp_adapter")
                    .ok_or("No qp_adapter")?;
                let mut status = adapter
                    .read_artifact_status(&artifact_id)
                    .unwrap_or_else(|_| minimal_status("track", "spec"));
                let mut transitions = status.transitions.unwrap_or_default();
                transitions.push(StatusTransition {
                    to: "spec".to_string(),
                    at: Some(at),
                    actor: Some(actor),
                    role: Some("spec".to_string()),
                    approver: None,
                    note: Some("adopted into governance from out-of-engine state 'implementing'".to_string()),
                    event_type: Some("adoption".to_string()),
                    satisfaction: None,
                });
                status.transitions = Some(transitions);
                adapter.with_status(&artifact_id, status);
                let mut out = Context::new();
                out.set("qp_adapter", adapter);
                Ok(out)
            },
        ),
        // T4 — checkin re-serve: seed a hook body on the predicate `qp_adapter`.
        step_def(
            "the in-memory query adapter has a reserve hook body for playbook {string} filename {string} with content {string}",
            &[("qp_adapter", "InMemoryQueryAdapter")],
            &[("qp_adapter", "InMemoryQueryAdapter")],
            |mut ctx, params| {
                let playbook_id = params.get_string(0).ok_or("Expected playbook_id")?.to_string();
                let filename = params.get_string(1).ok_or("Expected filename")?.to_string();
                let content = params.get_string(2).ok_or("Expected content")?.to_string();
                let mut adapter = ctx
                    .take::<InMemoryQueryAdapter>("qp_adapter")
                    .ok_or("No qp_adapter")?;
                adapter.with_playbook_hook_body(&playbook_id, &filename, &content);
                let mut out = Context::new();
                out.set("qp_adapter", adapter);
                Ok(out)
            },
        ),
        // T4 — When: evaluate reserve_hook_for_open_begin via the query path with
        // a FixtureTrackRegistry declaring (state, role) → hook.
        step_def(
            "reserve_hook_for_open_begin is evaluated for actor {string} on {string} kind {string} state {string} with a registry declaring state {string} role {string} hook {string}",
            &[("qp_adapter", "InMemoryQueryAdapter")],
            &[("qp_adapter", "InMemoryQueryAdapter"), ("reserve_hook_content", "Result<String, BeginError>")],
            |mut ctx, params| {
                let actor = params.get_string(0).ok_or("Expected actor")?.to_string();
                let artifact_id = params.get_string(1).ok_or("Expected artifact_id")?.to_string();
                let kind = params.get_string(2).ok_or("Expected kind")?.to_string();
                let state = params.get_string(3).ok_or("Expected state")?.to_string();
                let decl_state = params.get_string(4).ok_or("Expected decl state")?.to_string();
                let decl_role = params.get_string(5).ok_or("Expected decl role")?.to_string();
                let hook = params.get_string(6).ok_or("Expected hook")?.to_string();
                let adapter = ctx
                    .take::<InMemoryQueryAdapter>("qp_adapter")
                    .ok_or("No qp_adapter")?;
                let registry = FixtureTrackRegistry::new_state_role(&decl_state, &decl_role, &hook);
                let result = reserve_hook_for_open_begin_via_query(
                    &adapter, &registry, &artifact_id, &kind, &state, &actor,
                );
                let mut out = Context::new();
                out.set("qp_adapter", adapter);
                out.set("reserve_hook_content", result);
                Ok(out)
            },
        ),
        check_def(
            "the reserve hook content contains {string}",
            &[("reserve_hook_content", "Result<String, BeginError>")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                match ctx.get::<Result<String, BeginError>>("reserve_hook_content").ok_or("No reserve_hook_content")? {
                    Ok(s) if s.contains(needle) => Ok(()),
                    Ok(s) => Err(format!("reserve content does not contain '{}'. content: {}", needle, s)),
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the reserve hook content is empty",
            &[("reserve_hook_content", "Result<String, BeginError>")],
            |ctx, _| {
                match ctx.get::<Result<String, BeginError>>("reserve_hook_content").ok_or("No reserve_hook_content")? {
                    Ok(s) if s.is_empty() => Ok(()),
                    Ok(s) => Err(format!("reserve content is not empty: '{}'", s)),
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        // When: evaluate has_open_begin for (actor, state) on the seeded adapter
        step_def(
            "has_open_begin is evaluated for actor {string} state {string} on {string}",
            &[("qp_adapter", "InMemoryQueryAdapter")],
            &[("qp_adapter", "InMemoryQueryAdapter"), ("has_open_begin_result", "bool")],
            |mut ctx, params| {
                let actor = params.get_string(0).ok_or("Expected actor")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let artifact_id = params.get_string(2).ok_or("Expected artifact_id")?.to_string();
                let adapter = ctx
                    .take::<InMemoryQueryAdapter>("qp_adapter")
                    .ok_or("No qp_adapter")?;
                let activity = adapter
                    .read_activity_entries(&artifact_id)
                    .unwrap_or_default();
                let status = adapter
                    .read_artifact_status(&artifact_id)
                    .unwrap_or_else(|_| FullStatusYaml {
                        version: Some(1),
                        kind: None,
                        state: None,
                        origin_turn: None,
                        parent_id: None,
                        actors: None,
                        transitions: None,
                        activity: None,
                        contributed_by: None,
                    });
                let transitions = status.transitions.unwrap_or_default();
                let result = has_open_begin(&activity, &transitions, &actor, &state);
                let mut out = Context::new();
                out.set("qp_adapter", adapter);
                out.set("has_open_begin_result", result);
                Ok(out)
            },
        ),
        // Then: assert the predicate result is true
        check_def(
            "has_open_begin result is true",
            &[("has_open_begin_result", "bool")],
            |ctx, _params| {
                let result = ctx.get::<bool>("has_open_begin_result").ok_or("No has_open_begin_result")?;
                if *result {
                    Ok(())
                } else {
                    Err("Expected has_open_begin to be true, but it was false".to_string())
                }
            },
        ),
        // Then: assert the predicate result is false
        check_def(
            "has_open_begin result is false",
            &[("has_open_begin_result", "bool")],
            |ctx, _params| {
                let result = ctx.get::<bool>("has_open_begin_result").ok_or("No has_open_begin_result")?;
                if !result {
                    Ok(())
                } else {
                    Err("Expected has_open_begin to be false, but it was true".to_string())
                }
            },
        ),
        step_def(
            "the in-memory query adapter has artifact text for {string} filename {string} content {string}",
            &[("qp_adapter", "InMemoryQueryAdapter")],
            &[("qp_adapter", "InMemoryQueryAdapter")],
            |mut ctx, params| {
                let track_path = params.get_string(0).ok_or("Expected track_path")?.to_string();
                let filename = params.get_string(1).ok_or("Expected filename")?.to_string();
                let content = params.get_string(2).ok_or("Expected content")?.to_string();
                let mut adapter = ctx
                    .take::<InMemoryQueryAdapter>("qp_adapter")
                    .ok_or("No qp_adapter")?;
                adapter.with_artifact_text(&track_path, &filename, &content);
                let mut out = Context::new();
                out.set("qp_adapter", adapter);
                Ok(out)
            },
        ),
        step_def(
            "an in-memory query adapter seeded with context file {string} content {string}",
            &[],
            &[("qp_adapter", "InMemoryQueryAdapter")],
            |_ctx, params| {
                let path = params.get_string(0).ok_or("Expected path")?.to_string();
                let content = params.get_string(1).ok_or("Expected content")?.to_string();
                let mut adapter = InMemoryQueryAdapter::new();
                adapter.with_context_file(&path, &content);
                let mut out = Context::new();
                out.set("qp_adapter", adapter);
                Ok(out)
            },
        ),
        step_def(
            "an in-memory query adapter with no artifacts seeded",
            &[],
            &[("qp_adapter", "InMemoryQueryAdapter")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set("qp_adapter", InMemoryQueryAdapter::new());
                Ok(out)
            },
        ),
        step_def(
            "an in-memory query adapter seeded with projection row count for file {string} section {string} track {string} count {int}",
            &[],
            &[("qp_adapter", "InMemoryQueryAdapter")],
            |_ctx, params| {
                let file = params.get_string(0).ok_or("Expected file")?.to_string();
                let section = params.get_string(1).ok_or("Expected section")?.to_string();
                let track = params.get_string(2).ok_or("Expected track")?.to_string();
                let count = params.get_int(3).ok_or("Expected count")? as usize;
                let mut adapter = InMemoryQueryAdapter::new();
                adapter.with_projection_row_count(&file, &section, &track, count);
                let mut out = Context::new();
                out.set("qp_adapter", adapter);
                Ok(out)
            },
        ),

        step_def(
            "an in-memory query adapter seeded with hook body for playbook {string} filename {string} content {string}",
            &[],
            &[("qp_adapter", "InMemoryQueryAdapter")],
            |_ctx, params| {
                let playbook_id = params.get_string(0).ok_or("Expected playbook_id")?.to_string();
                let filename = params.get_string(1).ok_or("Expected filename")?.to_string();
                let content = params.get_string(2).ok_or("Expected content")?.to_string();
                let mut adapter = InMemoryQueryAdapter::new();
                adapter.with_playbook_hook_body(&playbook_id, &filename, &content);
                let mut out = Context::new();
                out.set("qp_adapter", adapter);
                Ok(out)
            },
        ),

        // ===== QueryPort when/then steps =====
        step_def(
            "query_port.read_artifact_kind is called for {string}",
            &[("qp_adapter", "InMemoryQueryAdapter")],
            &[
                ("qp_adapter", "InMemoryQueryAdapter"),
                ("qp_kind_result", "QueryOutcome"),
            ],
            |mut ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let adapter = ctx
                    .take::<InMemoryQueryAdapter>("qp_adapter")
                    .ok_or("No qp_adapter")?;
                let result = adapter.read_artifact_kind(&id);
                let mut out = Context::new();
                out.set("qp_adapter", adapter);
                out.set("qp_kind_result", result);
                Ok(out)
            },
        ),
        step_def(
            "query_port.read_artifact_state is called for {string}",
            &[("qp_adapter", "InMemoryQueryAdapter")],
            &[
                ("qp_adapter", "InMemoryQueryAdapter"),
                ("qp_state_result", "QueryOutcome"),
            ],
            |mut ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let adapter = ctx
                    .take::<InMemoryQueryAdapter>("qp_adapter")
                    .ok_or("No qp_adapter")?;
                let result = adapter.read_artifact_state(&id);
                let mut out = Context::new();
                out.set("qp_adapter", adapter);
                out.set("qp_state_result", result);
                Ok(out)
            },
        ),
        step_def(
            "query_port.read_artifact_text is called for track {string} filename {string}",
            &[("qp_adapter", "InMemoryQueryAdapter")],
            &[
                ("qp_adapter", "InMemoryQueryAdapter"),
                ("qp_text_result", "QueryOutcome"),
            ],
            |mut ctx, params| {
                let track = params.get_string(0).ok_or("Expected track")?.to_string();
                let filename = params.get_string(1).ok_or("Expected filename")?.to_string();
                let adapter = ctx
                    .take::<InMemoryQueryAdapter>("qp_adapter")
                    .ok_or("No qp_adapter")?;
                let result = adapter.read_artifact_text(&track, &filename);
                let mut out = Context::new();
                out.set("qp_adapter", adapter);
                out.set("qp_text_result", result);
                Ok(out)
            },
        ),
        step_def(
            "query_port.read_context_file is called for {string}",
            &[("qp_adapter", "InMemoryQueryAdapter")],
            &[
                ("qp_adapter", "InMemoryQueryAdapter"),
                ("qp_ctx_result", "QueryOutcome"),
            ],
            |mut ctx, params| {
                let path = params.get_string(0).ok_or("Expected path")?.to_string();
                let adapter = ctx
                    .take::<InMemoryQueryAdapter>("qp_adapter")
                    .ok_or("No qp_adapter")?;
                let result = adapter.read_context_file(&path);
                let mut out = Context::new();
                out.set("qp_adapter", adapter);
                out.set("qp_ctx_result", result);
                Ok(out)
            },
        ),
        step_def(
            "query_port.read_playbook_hook_body is called for playbook {string} filename {string}",
            &[("qp_adapter", "InMemoryQueryAdapter")],
            &[
                ("qp_adapter", "InMemoryQueryAdapter"),
                ("qp_hook_body_result", "QueryOutcome"),
            ],
            |mut ctx, params| {
                let playbook_id = params.get_string(0).ok_or("Expected playbook_id")?.to_string();
                let filename = params.get_string(1).ok_or("Expected filename")?.to_string();
                let adapter = ctx
                    .take::<InMemoryQueryAdapter>("qp_adapter")
                    .ok_or("No qp_adapter")?;
                let result = adapter.read_playbook_hook_body(&playbook_id, &filename);
                let mut out = Context::new();
                out.set("qp_adapter", adapter);
                out.set("qp_hook_body_result", result);
                Ok(out)
            },
        ),
        step_def(
            "query_port.check_projection_row_unique is called for file {string} track {string} section {string}",
            &[("qp_adapter", "InMemoryQueryAdapter")],
            &[
                ("qp_adapter", "InMemoryQueryAdapter"),
                ("qp_proj_result", "ProjectionCheckOutcome"),
            ],
            |mut ctx, params| {
                let file = params.get_string(0).ok_or("Expected file")?.to_string();
                let track = params.get_string(1).ok_or("Expected track")?.to_string();
                let section = params.get_string(2).ok_or("Expected section")?.to_string();
                let adapter = ctx
                    .take::<InMemoryQueryAdapter>("qp_adapter")
                    .ok_or("No qp_adapter")?;
                let result = adapter.check_projection_row_unique(&file, &track, &section);
                let mut out = Context::new();
                out.set("qp_adapter", adapter);
                out.set("qp_proj_result", result);
                Ok(out)
            },
        ),

        // ===== QueryPort check steps =====
        check_def(
            "the query port returns kind {string}",
            &[("qp_kind_result", "QueryOutcome")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected kind")?;
                let r = ctx
                    .get::<QueryOutcome>("qp_kind_result")
                    .ok_or("No qp_kind_result")?;
                match r {
                    Ok(k) if k == expected => Ok(()),
                    Ok(k) => Err(format!("Expected kind '{}', got '{}'", expected, k)),
                    Err(e) => Err(format!("Expected kind '{}', got error: {}", expected, e)),
                }
            },
        ),
        check_def(
            "the query port returns state {string}",
            &[("qp_state_result", "QueryOutcome")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?;
                let r = ctx
                    .get::<QueryOutcome>("qp_state_result")
                    .ok_or("No qp_state_result")?;
                match r {
                    Ok(s) if s == expected => Ok(()),
                    Ok(s) => Err(format!("Expected state '{}', got '{}'", expected, s)),
                    Err(e) => Err(format!("Expected state '{}', got error: {}", expected, e)),
                }
            },
        ),
        check_def(
            "the query port returns artifact text containing {string}",
            &[("qp_text_result", "QueryOutcome")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected text")?;
                let r = ctx
                    .get::<QueryOutcome>("qp_text_result")
                    .ok_or("No qp_text_result")?;
                match r {
                    Ok(text) if text.contains(needle) => Ok(()),
                    Ok(text) => Err(format!(
                        "Expected text to contain '{}', got '{}'",
                        needle, text
                    )),
                    Err(e) => Err(format!("Expected text, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the query port returns context text containing {string}",
            &[("qp_ctx_result", "QueryOutcome")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected text")?;
                let r = ctx
                    .get::<QueryOutcome>("qp_ctx_result")
                    .ok_or("No qp_ctx_result")?;
                match r {
                    Ok(text) if text.contains(needle) => Ok(()),
                    Ok(text) => Err(format!(
                        "Expected context to contain '{}', got '{}'",
                        needle, text
                    )),
                    Err(e) => Err(format!("Expected text, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the query port returns hook body containing {string}",
            &[("qp_hook_body_result", "QueryOutcome")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected text")?;
                let r = ctx
                    .get::<QueryOutcome>("qp_hook_body_result")
                    .ok_or("No qp_hook_body_result")?;
                match r {
                    Ok(text) if text.contains(needle) => Ok(()),
                    Ok(text) => Err(format!(
                        "Expected hook body to contain '{}', got '{}'",
                        needle, text
                    )),
                    Err(e) => Err(format!("Expected hook body, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the query port returns a hook body IoError containing {string}",
            &[("qp_hook_body_result", "QueryOutcome")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected text")?;
                let r = ctx
                    .get::<QueryOutcome>("qp_hook_body_result")
                    .ok_or("No qp_hook_body_result")?;
                match r {
                    Err(QueryError::IoError { message }) if message.contains(needle) => Ok(()),
                    Err(QueryError::IoError { message }) => Err(format!(
                        "Expected IoError containing '{}', got IoError: '{}'",
                        needle, message
                    )),
                    Err(e) => Err(format!("Expected IoError containing '{}', got: {}", needle, e)),
                    Ok(text) => Err(format!(
                        "Expected IoError containing '{}', got Ok('{}')",
                        needle, text
                    )),
                }
            },
        ),
        check_def(
            "the query port returns a NotFound error for {string}",
            &[("qp_kind_result", "QueryOutcome")],
            |ctx, params| {
                let expected_id = params.get_string(0).ok_or("Expected id")?;
                let r = ctx
                    .get::<QueryOutcome>("qp_kind_result")
                    .ok_or("No qp_kind_result")?;
                match r {
                    Err(QueryError::NotFound { artifact_id })
                        if artifact_id == expected_id =>
                    {
                        Ok(())
                    }
                    Err(QueryError::NotFound { artifact_id }) => Err(format!(
                        "Expected NotFound for '{}', got NotFound for '{}'",
                        expected_id, artifact_id
                    )),
                    Err(e) => Err(format!(
                        "Expected NotFound for '{}', got: {}",
                        expected_id, e
                    )),
                    Ok(k) => Err(format!(
                        "Expected NotFound for '{}', got Ok('{}')",
                        expected_id, k
                    )),
                }
            },
        ),
        check_def(
            "the query port projection check returns Ok",
            &[("qp_proj_result", "ProjectionCheckOutcome")],
            |ctx, _params| {
                let r = ctx
                    .get::<ProjectionCheckOutcome>("qp_proj_result")
                    .ok_or("No qp_proj_result")?;
                match r {
                    Ok(()) => Ok(()),
                    Err(e) => Err(format!("Expected Ok, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the query port projection check returns an error containing {string}",
            &[("qp_proj_result", "ProjectionCheckOutcome")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected text")?;
                let r = ctx
                    .get::<ProjectionCheckOutcome>("qp_proj_result")
                    .ok_or("No qp_proj_result")?;
                match r {
                    Err(e) => {
                        let msg = e.to_string();
                        if msg.contains(needle) {
                            Ok(())
                        } else {
                            Err(format!(
                                "Expected error containing '{}', got: {}",
                                needle, msg
                            ))
                        }
                    }
                    Ok(()) => Err(format!("Expected error containing '{}', got Ok", needle)),
                }
            },
        ),

        // ===== InMemoryArtifactAdapter setup =====
        step_def(
            "an in-memory artifact adapter with no pre-existing docs",
            &[],
            &[("ap_adapter", "InMemoryArtifactAdapter")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set("ap_adapter", InMemoryArtifactAdapter::new());
                Ok(out)
            },
        ),
        step_def(
            "an in-memory artifact adapter with pre-existing doc at track {string} doc {string}",
            &[],
            &[("ap_adapter", "InMemoryArtifactAdapter")],
            |_ctx, params| {
                let track = params.get_string(0).ok_or("Expected track")?.to_string();
                let doc = params.get_string(1).ok_or("Expected doc")?.to_string();
                let mut adapter = InMemoryArtifactAdapter::new();
                adapter.with_pre_existing_doc(&track, &doc);
                let mut out = Context::new();
                out.set("ap_adapter", adapter);
                Ok(out)
            },
        ),

        // ===== ArtifactPort when steps =====
        step_def(
            "artifact_port.create_review_doc is called for track {string} doc {string} header {string}",
            &[("ap_adapter", "InMemoryArtifactAdapter")],
            &[
                ("ap_adapter", "InMemoryArtifactAdapter"),
                ("ap_result", "ArtifactPortOutcome"),
            ],
            |mut ctx, params| {
                let track = params.get_string(0).ok_or("Expected track")?.to_string();
                let doc = params.get_string(1).ok_or("Expected doc")?.to_string();
                let header = params.get_string(2).ok_or("Expected header")?.to_string();
                let adapter = ctx
                    .take::<InMemoryArtifactAdapter>("ap_adapter")
                    .ok_or("No ap_adapter")?;
                let result = adapter.create_review_doc(&track, &doc, &header);
                let mut out = Context::new();
                out.set("ap_adapter", adapter);
                out.set("ap_result", result);
                Ok(out)
            },
        ),

        // ===== ArtifactPort check steps =====
        check_def(
            "the in-memory artifact adapter recorded {int} created doc",
            &[("ap_adapter", "InMemoryArtifactAdapter")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let adapter = ctx
                    .get::<InMemoryArtifactAdapter>("ap_adapter")
                    .ok_or("No ap_adapter")?;
                let docs = adapter.created_docs.lock().unwrap();
                if docs.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} created docs, got {}",
                        expected,
                        docs.len()
                    ))
                }
            },
        ),
        check_def(
            "the in-memory artifact adapter recorded {int} created docs",
            &[("ap_adapter", "InMemoryArtifactAdapter")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let adapter = ctx
                    .get::<InMemoryArtifactAdapter>("ap_adapter")
                    .ok_or("No ap_adapter")?;
                let docs = adapter.created_docs.lock().unwrap();
                if docs.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} created docs, got {}",
                        expected,
                        docs.len()
                    ))
                }
            },
        ),
        check_def(
            "the in-memory artifact adapter's created doc has track {string} and doc {string}",
            &[("ap_adapter", "InMemoryArtifactAdapter")],
            |ctx, params| {
                let expected_track = params.get_string(0).ok_or("Expected track")?;
                let expected_doc = params.get_string(1).ok_or("Expected doc")?;
                let adapter = ctx
                    .get::<InMemoryArtifactAdapter>("ap_adapter")
                    .ok_or("No ap_adapter")?;
                let docs = adapter.created_docs.lock().unwrap();
                if docs.iter().any(|(t, d, _)| t == expected_track && d == expected_doc) {
                    Ok(())
                } else {
                    Err(format!(
                        "No created doc with track '{}' and doc '{}'. Got: {:?}",
                        expected_track, expected_doc, *docs
                    ))
                }
            },
        ),
        check_def(
            "the artifact port returns a path containing {string}",
            &[("ap_result", "ArtifactPortOutcome")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected text")?;
                let r = ctx
                    .get::<ArtifactPortOutcome>("ap_result")
                    .ok_or("No ap_result")?;
                match r {
                    Ok(path) if path.contains(needle) => Ok(()),
                    Ok(path) => Err(format!(
                        "Expected path to contain '{}', got '{}'",
                        needle, path
                    )),
                    Err(e) => Err(format!("Expected path, got error: {}", e)),
                }
            },
        ),

        // ===== scaffold_track_directory Given/When/Then steps =====
        step_def(
            "an in-memory artifact adapter with pre-existing scaffold for track_name {string}",
            &[],
            &[("ap_adapter", "InMemoryArtifactAdapter")],
            |_ctx, params| {
                let track_name = params.get_string(0).ok_or("Expected track_name")?.to_string();
                let mut adapter = InMemoryArtifactAdapter::new();
                adapter.with_pre_existing_scaffold(&track_name);
                let mut out = Context::new();
                out.set("ap_adapter", adapter);
                Ok(out)
            },
        ),
        step_def(
            "artifact_port.scaffold_track_directory is called with track_name {string} parent_id {string} display_name {string} and status_yaml {string}",
            &[("ap_adapter", "InMemoryArtifactAdapter")],
            &[
                ("ap_adapter", "InMemoryArtifactAdapter"),
                ("ap_result", "ArtifactPortOutcome"),
            ],
            |mut ctx, params| {
                let track_name = params.get_string(0).ok_or("Expected track_name")?.to_string();
                let parent_id = params.get_string(1).ok_or("Expected parent_id")?.to_string();
                let display_name = params.get_string(2).ok_or("Expected display_name")?.to_string();
                let status_yaml = params.get_string(3).ok_or("Expected status_yaml")?.to_string();
                let adapter = ctx
                    .take::<InMemoryArtifactAdapter>("ap_adapter")
                    .ok_or("No ap_adapter")?;
                let result = adapter.scaffold_track_directory(&track_name, &parent_id, &display_name, &status_yaml);
                let mut out = Context::new();
                out.set("ap_adapter", adapter);
                out.set("ap_result", result);
                Ok(out)
            },
        ),
        check_def(
            "the in-memory artifact adapter recorded {int} scaffolded track",
            &[("ap_adapter", "InMemoryArtifactAdapter")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let adapter = ctx
                    .get::<InMemoryArtifactAdapter>("ap_adapter")
                    .ok_or("No ap_adapter")?;
                let tracks = adapter.scaffolded_tracks.lock().unwrap();
                if tracks.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} scaffolded tracks, got {}",
                        expected,
                        tracks.len()
                    ))
                }
            },
        ),
        check_def(
            "the scaffolded track record has track_name {string} and parent_id {string} and display_name {string}",
            &[("ap_adapter", "InMemoryArtifactAdapter")],
            |ctx, params| {
                let expected_track_name = params.get_string(0).ok_or("Expected track_name")?;
                let expected_parent_id = params.get_string(1).ok_or("Expected parent_id")?;
                let expected_display_name = params.get_string(2).ok_or("Expected display_name")?;
                let adapter = ctx
                    .get::<InMemoryArtifactAdapter>("ap_adapter")
                    .ok_or("No ap_adapter")?;
                let tracks = adapter.scaffolded_tracks.lock().unwrap();
                if tracks.iter().any(|r| {
                    r.track_name == expected_track_name
                        && r.parent_id == expected_parent_id
                        && r.display_name == expected_display_name
                }) {
                    Ok(())
                } else {
                    Err(format!(
                        "No scaffolded track with track_name='{}', parent_id='{}', display_name='{}'. Got: {:?}",
                        expected_track_name, expected_parent_id, expected_display_name, *tracks
                    ))
                }
            },
        ),
        check_def(
            "the artifact port returns an IoError with message starting {string}",
            &[("ap_result", "ArtifactPortOutcome")],
            |ctx, params| {
                let prefix = params.get_string(0).ok_or("Expected prefix")?;
                let r = ctx
                    .get::<ArtifactPortOutcome>("ap_result")
                    .ok_or("No ap_result")?;
                match r {
                    Err(ArtifactError::IoError { message }) if message.starts_with(prefix) => Ok(()),
                    Err(ArtifactError::IoError { message }) => Err(format!(
                        "Expected IoError with message starting '{}', got '{}'",
                        prefix, message
                    )),
                    Err(other) => Err(format!(
                        "Expected IoError with message starting '{}', got '{}'",
                        prefix, other
                    )),
                    Ok(path) => Err(format!("Expected IoError, got Ok('{}')", path)),
                }
            },
        ),

        // ===== FileSystemQueryAdapter fidelity steps =====
        step_def(
            "a hearth fixture with a projection file containing {int} rows for track {string} in section {string}",
            &[],
            &[
                ("fs_qp_hearth", "PathBuf"),
                ("fs_qp_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let count = params.get_int(0).ok_or("Expected count")? as usize;
                let track_name = params.get_string(1).ok_or("Expected track")?.to_string();
                let section = params.get_string(2).ok_or("Expected section")?.to_string();

                let (handle, tmp) = retained_temp_dir("anvil-qp-fidelity-")?;
                std::fs::create_dir_all(tmp.join("projections"))
                    .map_err(|e| format!("Failed to create dir: {}", e))?;

                let rows: String = (0..count)
                    .map(|_| format!("| {} | some-proposal |\n", track_name))
                    .collect();
                let content = format!(
                    "---\nincremental_count: 0\n---\n\n## {} ({})\n\n| Track | Proposal |\n|-------|----------|\n{}",
                    section, count, rows
                );

                std::fs::write(tmp.join("projections/execution.md"), &content)
                    .map_err(|e| format!("Failed to write execution.md: {}", e))?;

                let mut out = Context::new();
                out.set("fs_qp_hearth", tmp);
                out.set("fs_qp_hearth_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "fs_query_adapter.check_projection_row_unique is called for file {string} track {string} section {string}",
            &[("fs_qp_hearth", "PathBuf")],
            &[
                ("fs_qp_hearth", "PathBuf"),
                ("fs_qp_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("fs_qp_proj_result", "ProjectionCheckOutcome"),
            ],
            |mut ctx, params| {
                let file = params.get_string(0).ok_or("Expected file")?.to_string();
                let track = params.get_string(1).ok_or("Expected track")?.to_string();
                let section = params.get_string(2).ok_or("Expected section")?.to_string();
                let hearth = ctx
                    .take::<PathBuf>("fs_qp_hearth")
                    .ok_or("No fs_qp_hearth")?;
                let adapter = FileSystemQueryAdapter::new(hearth.clone());
                let result = adapter.check_projection_row_unique(&file, &track, &section);
                let mut out = Context::new();
                out.set("fs_qp_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "fs_qp_hearth_handle");
                out.set("fs_qp_proj_result", result);
                Ok(out)
            },
        ),
        check_def(
            "the fs query adapter returns an error with message {string}",
            &[("fs_qp_proj_result", "ProjectionCheckOutcome")],
            |ctx, params| {
                let expected_msg = params.get_string(0).ok_or("Expected message")?;
                let r = ctx
                    .get::<ProjectionCheckOutcome>("fs_qp_proj_result")
                    .ok_or("No fs_qp_proj_result")?;
                match r {
                    Err(QueryError::ProjectionRowNotFound { message })
                    | Err(QueryError::ProjectionRowAmbiguous { message }) => {
                        if message == expected_msg {
                            Ok(())
                        } else {
                            Err(format!(
                                "Expected message:\n  '{}'\ngot:\n  '{}'",
                                expected_msg, message
                            ))
                        }
                    }
                    Err(e) => Err(format!("Expected projection error, got: {}", e)),
                    Ok(()) => Err(format!(
                        "Expected error with message '{}', got Ok",
                        expected_msg
                    )),
                }
            },
        ),

        // ===== Domain-seam begin: Given adapter setup steps =====
        // These steps produce an InMemoryQueryAdapter + InMemoryArtifactAdapter
        // pair for testing the pure-function BeginCommandHandler directly.

        step_def(
            "an in-memory query adapter with track {string} in state {string}",
            &[],
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            |_ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let mut adapter = InMemoryQueryAdapter::new();
                adapter.with_artifact(&id, "track", &state);
                adapter.with_status(&id, minimal_status("track", &state));
                adapter.with_projection_row_count("projections/execution.md", &state, "review spec strand", 1);
                adapter.with_registry_entry("tracks.md", &id, "review spec strand", "anvil-playbook-engine");
                // P4.4 migration: seed the reviewer hook body via the hook port, not the
                // legacy context-file port. The (spec_review, reviewer) hook is resolved from
                // the registry declaration; absence of context_file seeding is correct.
                adapter.with_playbook_hook_body(
                    "20260422T0000_track_lifecycle",
                    "spec-review.md",
                    "# Forge Review\n\n## Dispatch to criteria\n\n# Spec Review Criteria\n\nreview protocol + criteria",
                );
                let track_path = format!("tracks/{}", id);
                adapter.with_artifact_text(&track_path, "spec.md", "# spec body");
                let mut out = Context::new();
                out.set("begin_query", adapter);
                out.set("begin_artifact", InMemoryArtifactAdapter::new());
                Ok(out)
            },
        ),
        step_def(
            "an in-memory query adapter with track {string} in state {string} seeded with the revision hook",
            &[],
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            |_ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let mut adapter = InMemoryQueryAdapter::new();
                adapter.with_artifact(&id, "track", &state);
                adapter.with_status(&id, minimal_status("track", &state));
                adapter.with_projection_row_count("projections/execution.md", &state, "review spec strand", 1);
                adapter.with_registry_entry("tracks.md", &id, "review spec strand", "anvil-playbook-engine");
                // Slice B: seed the (spec_revision, doer) hook body. The seed
                // machine declares this hook as spec-revision.md; the
                // creator-on-spec_revision begin branch resolves it via the
                // SeedPlaybookRegistry and reads the body from this store.
                adapter.with_playbook_hook_body(
                    "20260422T0000_track_lifecycle",
                    "spec-revision.md",
                    "# Spec Revision Context\n\n## Finding disposition\n\nWill address / Acknowledged, not addressing.\n\nAfter revision, call complete(artifact_path).",
                );
                let track_path = format!("tracks/{}", id);
                adapter.with_artifact_text(&track_path, "spec.md", "# spec body");
                adapter.with_artifact_text(&track_path, "spec.review.md", "# review findings");
                let mut out = Context::new();
                out.set("begin_query", adapter);
                out.set("begin_artifact", InMemoryArtifactAdapter::new());
                Ok(out)
            },
        ),
        step_def(
            "an in-memory query adapter with track {string} in state {string} and spec content {string}",
            &[],
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            |_ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let content = params.get_string(2).ok_or("Expected content")?.to_string();
                let mut adapter = InMemoryQueryAdapter::new();
                adapter.with_artifact(&id, "track", &state);
                adapter.with_status(&id, minimal_status("track", &state));
                adapter.with_projection_row_count("projections/execution.md", &state, "review spec strand", 1);
                adapter.with_registry_entry("tracks.md", &id, "review spec strand", "anvil-playbook-engine");
                // P4.4 migration: seed the reviewer hook body via the hook port, not the
                // legacy context-file port. The (spec_review, reviewer) hook is resolved from
                // the registry declaration; absence of context_file seeding is correct.
                adapter.with_playbook_hook_body(
                    "20260422T0000_track_lifecycle",
                    "spec-review.md",
                    "# Forge Review\n\n## Dispatch to criteria\n\n# Spec Review Criteria\n\nreview protocol + criteria",
                );
                let track_path = format!("tracks/{}", id);
                adapter.with_artifact_text(&track_path, "spec.md", &content);
                let mut out = Context::new();
                out.set("begin_query", adapter);
                out.set("begin_artifact", InMemoryArtifactAdapter::new());
                Ok(out)
            },
        ),
        step_def(
            "an in-memory query adapter with track {string} in state {string} and spec content {string} and pre-existing review doc",
            &[],
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            |_ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let content = params.get_string(2).ok_or("Expected content")?.to_string();
                let mut adapter = InMemoryQueryAdapter::new();
                adapter.with_artifact(&id, "track", &state);
                adapter.with_status(&id, minimal_status("track", &state));
                adapter.with_projection_row_count("projections/execution.md", &state, "review spec strand", 1);
                adapter.with_registry_entry("tracks.md", &id, "review spec strand", "anvil-playbook-engine");
                // P4.4 migration: seed the reviewer hook body via the hook port, not the
                // legacy context-file port. The (spec_review, reviewer) hook is resolved from
                // the registry declaration; absence of context_file seeding is correct.
                adapter.with_playbook_hook_body(
                    "20260422T0000_track_lifecycle",
                    "spec-review.md",
                    "# Forge Review\n\n## Dispatch to criteria\n\n# Spec Review Criteria\n\nreview protocol + criteria",
                );
                let track_path = format!("tracks/{}", id);
                adapter.with_artifact_text(&track_path, "spec.md", &content);
                let mut artifact_adapter = InMemoryArtifactAdapter::new();
                artifact_adapter.with_pre_existing_doc(&track_path, "spec.review.md");
                let mut out = Context::new();
                out.set("begin_query", adapter);
                out.set("begin_artifact", artifact_adapter);
                Ok(out)
            },
        ),
        step_def(
            "an in-memory query adapter with track {string} in state {string} and artifact file {string} content {string}",
            &[],
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            |_ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let filename = params.get_string(2).ok_or("Expected filename")?.to_string();
                let content = params.get_string(3).ok_or("Expected content")?.to_string();
                let mut adapter = InMemoryQueryAdapter::new();
                adapter.with_artifact(&id, "track", &state);
                adapter.with_status(&id, minimal_status("track", &state));
                adapter.with_projection_row_count("projections/execution.md", &state, "review spec strand", 1);
                adapter.with_registry_entry("tracks.md", &id, "review spec strand", "anvil-playbook-engine");
                let track_path = format!("tracks/{}", id);
                adapter.with_artifact_text(&track_path, &filename, &content);
                let mut out = Context::new();
                out.set("begin_query", adapter);
                out.set("begin_artifact", InMemoryArtifactAdapter::new());
                Ok(out)
            },
        ),
        // ===== Adopt out-of-engine artifact steps =====
        // A track already carrying engine transition history (one recorded
        // transition) — i.e. NOT pre-governance, so adoption must be refused.
        step_def(
            "an in-memory query adapter with track {string} in state {string} governed by a prior transition",
            &[],
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            |_ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let mut adapter = InMemoryQueryAdapter::new();
                adapter.with_artifact(&id, "track", &state);
                let mut status = minimal_status("track", &state);
                status.transitions = Some(vec![StatusTransition {
                    to: state.clone(),
                    at: Some("2026-07-16T00:00:00Z".to_string()),
                    actor: Some("Creator-000001".to_string()),
                    role: Some("spec".to_string()),
                    approver: None,
                    note: None,
                    event_type: None,
                    satisfaction: None,
                }]);
                adapter.with_status(&id, status);
                adapter.with_registry_entry("tracks.md", &id, "governed track", "anvil-playbook-engine");
                let track_path = format!("tracks/{}", id);
                adapter.with_artifact_text(&track_path, "spec.md", "# spec body");
                let mut out = Context::new();
                out.set("begin_query", adapter);
                out.set("begin_artifact", InMemoryArtifactAdapter::new());
                Ok(out)
            },
        ),
        // Finding 1 (begin-marker-only): a track with NO recorded transition but
        // an engine begin MARKER present. An engine begin can write only a marker
        // (generic-begin / resume paths), so an empty transition log does NOT
        // prove "never governed" — adoption must still be refused.
        step_def(
            "an in-memory query adapter with track {string} in state {string} carrying only a begin marker",
            &[],
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            |_ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let mut adapter = InMemoryQueryAdapter::new();
                adapter.with_artifact(&id, "track", &state);
                let mut status = minimal_status("track", &state);
                status.activity = Some(ActivityLog::new(vec![ActivityEntry {
                    kind: "begin".to_string(),
                    actor: "Doer-000001".to_string(),
                    state: state.clone(),
                    at: "2026-07-16T00:00:00Z".to_string(),
                    conversation_id: String::new(),
                }]));
                adapter.with_status(&id, status);
                adapter.with_registry_entry("tracks.md", &id, "marker only track", "anvil-playbook-engine");
                let track_path = format!("tracks/{}", id);
                adapter.with_artifact_text(&track_path, "spec.md", "# spec body");
                let mut out = Context::new();
                out.set("begin_query", adapter);
                out.set("begin_artifact", InMemoryArtifactAdapter::new());
                Ok(out)
            },
        ),
        // Finding 1 (fail-closed on damage): a track whose activity log is DEGRADED
        // (a malformed entry was dropped). A dropped entry might have been a begin
        // marker, so the log cannot prove "never governed" — adoption must refuse
        // (AdoptionEvidenceUnreadable), not adopt on incomplete evidence.
        step_def(
            "an in-memory query adapter with track {string} in state {string} with a degraded activity log",
            &[],
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            |_ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let mut adapter = InMemoryQueryAdapter::new();
                adapter.with_artifact(&id, "track", &state);
                let mut status = minimal_status("track", &state);
                status.activity = Some(ActivityLog {
                    entries: vec![],
                    dropped: 1,
                    first_error: Some("activity[0]: malformed begin marker".to_string()),
                });
                adapter.with_status(&id, status);
                adapter.with_registry_entry("tracks.md", &id, "degraded track", "anvil-playbook-engine");
                let track_path = format!("tracks/{}", id);
                adapter.with_artifact_text(&track_path, "spec.md", "# spec body");
                let mut out = Context::new();
                out.set("begin_query", adapter);
                out.set("begin_artifact", InMemoryArtifactAdapter::new());
                Ok(out)
            },
        ),
        // Fail-closed on damaged TRANSITION evidence: a track with NO recorded
        // transition in its parsed status, but whose on-disk `transitions/` event
        // store is damaged (an unreadable / unparseable event). The lenient read
        // would silently drop the damaged event and see an empty history →
        // adoption would wrongly proceed; the STRICT adoption read must refuse
        // (AdoptionEvidenceUnreadable). Modelled via the in-memory adapter's
        // damaged-evidence flag, which only `read_transitions_strict` honors.
        step_def(
            "an in-memory query adapter with track {string} in state {string} with damaged transition evidence",
            &[],
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            |_ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let mut adapter = InMemoryQueryAdapter::new();
                adapter.with_artifact(&id, "track", &state);
                adapter.with_status(&id, minimal_status("track", &state));
                adapter.with_damaged_transition_evidence(&id);
                adapter.with_registry_entry("tracks.md", &id, "torn track", "anvil-playbook-engine");
                let track_path = format!("tracks/{}", id);
                adapter.with_artifact_text(&track_path, "spec.md", "# spec body");
                let mut out = Context::new();
                out.set("begin_query", adapter);
                out.set("begin_artifact", InMemoryArtifactAdapter::new());
                Ok(out)
            },
        ),
        // begin(identifier, adopt: true) against a registry declaring the
        // machine's initial (state, role) → hook. `adopt` is the 3rd string arg
        // ("true"/"false").
        step_def(
            "begin is called via query adapter with identifier {string} session_role {string} adopt {string} and a registry declaring state {string} role {string} hook {string}",
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |mut ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let session_role = params.get_string(1).ok_or("Expected session_role")?.to_string();
                let adopt = params.get_string(2).map(|s| s == "true").unwrap_or(false);
                let state = params.get_string(3).ok_or("Expected state")?.to_string();
                let role = params.get_string(4).ok_or("Expected role")?.to_string();
                let hook = params.get_string(5).ok_or("Expected hook")?.to_string();
                let query = ctx
                    .take::<InMemoryQueryAdapter>("begin_query")
                    .ok_or("No begin_query")?;
                let request = BeginRequest {
                    identifier: id,
                    session_role,
                    adopt,
                    actor_name: "Test-000000".to_string(),
                    actor_type: "agent".to_string(),
                    actor_model: "test-model".to_string(),
                    actor_provider: "test".to_string(),
                    ..Default::default()
                };
                let registry = FixtureTrackRegistry::new_state_role(&state, &role, &hook);
                let outcome = BeginCommandHandler::execute(&query, &registry, request);
                let mut out = Context::new();
                out.set("begin_outcome", outcome);
                out.set("begin_query", query);
                Ok(out)
            },
        ),
        check_def(
            "the handler emitted a ReviewTransition event to state {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?;
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Ok(o) => {
                        if o.events.iter().any(|e| e.transition_target() == Some(expected.as_ref())) {
                            Ok(())
                        } else {
                            Err(format!(
                                "No ReviewTransition to state '{}'; events: {:?}",
                                expected,
                                o.events.iter().map(|e| format!("{:?}", e)).collect::<Vec<_>>()
                            ))
                        }
                    }
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the begin outcome is an AlreadyGoverned error",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, _params| {
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Err(BeginError::AlreadyGoverned { .. }) => Ok(()),
                    Err(e) => Err(format!("Expected AlreadyGoverned, got: {}", e)),
                    Ok(_) => Err("Expected AlreadyGoverned error, got success".to_string()),
                }
            },
        ),
        check_def(
            "the begin outcome is an AdoptionEvidenceUnreadable error",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, _params| {
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Err(BeginError::AdoptionEvidenceUnreadable { .. }) => Ok(()),
                    Err(e) => Err(format!("Expected AdoptionEvidenceUnreadable, got: {}", e)),
                    Ok(_) => Err("Expected AdoptionEvidenceUnreadable error, got success".to_string()),
                }
            },
        ),
        check_def(
            "the handler emitted an ArtifactAdopted event to state {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?;
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Ok(o) => {
                        if o.events.iter().any(|e| e.adoption_target() == Some(expected.as_ref())) {
                            Ok(())
                        } else {
                            Err(format!(
                                "No ArtifactAdopted to state '{}'; events: {:?}",
                                expected,
                                o.events.iter().map(|e| format!("{:?}", e)).collect::<Vec<_>>()
                            ))
                        }
                    }
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        // ===== P4 reviewer hook-serving Given steps =====
        // These Given steps seed a track in spec_review WITHOUT seeding
        // with_context_file("spec-review.md", ...). The P4.2 wiring reads the
        // reviewer hook body via read_playbook_hook_body (from a registry-declared
        // hook), so the context-file seeder is irrelevant — the test controls what
        // body is returned via with_playbook_hook_body.
        step_def(
            "an in-memory query adapter with track {string} in state {string} seeded with reviewer hook and spec content {string}",
            &[],
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            |_ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let content = params.get_string(2).ok_or("Expected spec content")?.to_string();
                let mut adapter = InMemoryQueryAdapter::new();
                adapter.with_artifact(&id, "track", &state);
                adapter.with_status(&id, minimal_status("track", &state));
                adapter.with_projection_row_count("projections/execution.md", &state, "review spec strand", 1);
                adapter.with_registry_entry("tracks.md", &id, "review spec strand", "anvil-playbook-engine");
                let track_path = format!("tracks/{}", id);
                adapter.with_artifact_text(&track_path, "spec.md", &content);
                // NOTE: no with_context_file("spec-review.md", ...) — P4.2 reads
                // the reviewer hook body via read_playbook_hook_body, not read_context_file.
                let mut out = Context::new();
                out.set("begin_query", adapter);
                out.set("begin_artifact", InMemoryArtifactAdapter::new());
                Ok(out)
            },
        ),
        // NOTE: "an in-memory query adapter missing the context file for track {string}
        // in state {string}" step was RETIRED in P4.5 along with
        // begin_error_missing_context_file.feature. The P4.2 change removes the
        // read_context_file("spec-review.md") call from handle_review; the
        // request-time missing-context error path no longer exists. The missing-hook-file
        // case is now caught at LOAD time via playbook_unknown_hook_reference.

        step_def(
            "an in-memory query adapter with proposal {string} in state {string}",
            &[],
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            |_ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let mut adapter = InMemoryQueryAdapter::new();
                adapter.with_artifact(&id, "proposal", &state);
                adapter.with_status(&id, minimal_status("proposal", &state));
                adapter.with_context_file("spec-review.md", "review protocol");
                let mut out = Context::new();
                out.set("begin_query", adapter);
                out.set("begin_artifact", InMemoryArtifactAdapter::new());
                Ok(out)
            },
        ),
        step_def(
            "an empty in-memory query adapter",
            &[],
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            |_ctx, _params| {
                let adapter = InMemoryQueryAdapter::new();
                let mut out = Context::new();
                out.set("begin_query", adapter);
                out.set("begin_artifact", InMemoryArtifactAdapter::new());
                Ok(out)
            },
        ),
        step_def(
            "an in-memory query adapter with parent {string} in state {string}",
            &[],
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            |_ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let mut adapter = InMemoryQueryAdapter::new();
                adapter.with_artifact(&id, "proposal", &state);
                adapter.with_context_file("spec-writing.md", "test");
                let mut out = Context::new();
                out.set("begin_query", adapter);
                out.set("begin_artifact", InMemoryArtifactAdapter::new());
                Ok(out)
            },
        ),
        step_def(
            "an in-memory query adapter with parent {string} in state {string} and spec-writing context {string}",
            &[],
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            |_ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let context = params.get_string(2).ok_or("Expected context")?.to_string();
                let mut adapter = InMemoryQueryAdapter::new();
                adapter.with_artifact(&id, "proposal", &state);
                adapter.with_context_file("spec-writing.md", &context);
                let mut out = Context::new();
                out.set("begin_query", adapter);
                out.set("begin_artifact", InMemoryArtifactAdapter::new());
                Ok(out)
            },
        ),
        step_def(
            "an in-memory query adapter with no parents",
            &[],
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            |_ctx, _params| {
                let mut adapter = InMemoryQueryAdapter::new();
                adapter.with_context_file("spec-writing.md", "test");
                let mut out = Context::new();
                out.set("begin_query", adapter);
                out.set("begin_artifact", InMemoryArtifactAdapter::new());
                Ok(out)
            },
        ),
        step_def(
            "the in-memory query adapter has spec-writing context with the full R6 body",
            &[("begin_query", "InMemoryQueryAdapter"), ("begin_artifact", "InMemoryArtifactAdapter")],
            &[("begin_query", "InMemoryQueryAdapter"), ("begin_artifact", "InMemoryArtifactAdapter")],
            |mut ctx, _params| {
                let body = "\
# Spec Writing Context\n\
\n\
## Actor identity discipline\n\
Carry `actor_name` forward; re-detect runtime `actor_*` per call.\n\
\n\
## Pre-read list\n\
1. PHILOSOPHY.md\n2. CLAUDE.md\n3. forge/projections/\n4. proposal.md\n\
\n\
## Decisions-search step\n\
Scan forge/decisions.md and forge/projections/decisions.md.\n\
\n\
## After the spec is accepted\n\
Review: agent or human-as-reviewer. Next phase: plan or implement.\n\
\n\
## Commit convention\n\
`spec(forge): {track description}`.\n";
                let mut adapter = ctx
                    .take::<InMemoryQueryAdapter>("begin_query")
                    .ok_or("No begin_query")?;
                let artifact_adapter = ctx
                    .take::<InMemoryArtifactAdapter>("begin_artifact")
                    .ok_or("No begin_artifact")?;
                // P3.5: the create flow now serves the (spec, doer) hook body
                // resolved via the registry, not the static spec-writing.md
                // context file. Seed the body under the hook path so the
                // registry-declared doer hook resolves to it.
                adapter.with_playbook_hook_body(
                    "20260422T0000_track_lifecycle",
                    "spec-writing.md",
                    body,
                );
                let mut out = Context::new();
                out.set("begin_query", adapter);
                out.set("begin_artifact", artifact_adapter);
                Ok(out)
            },
        ),
        step_def(
            "the in-memory query adapter has a hook body for playbook {string} filename {string} with content {string}",
            &[("begin_query", "InMemoryQueryAdapter"), ("begin_artifact", "InMemoryArtifactAdapter")],
            &[("begin_query", "InMemoryQueryAdapter"), ("begin_artifact", "InMemoryArtifactAdapter")],
            |mut ctx, params| {
                let playbook_id = params.get_string(0).ok_or("Expected playbook_id")?.to_string();
                let filename = params.get_string(1).ok_or("Expected filename")?.to_string();
                let content = params.get_string(2).ok_or("Expected content")?.to_string();
                let mut adapter = ctx
                    .take::<InMemoryQueryAdapter>("begin_query")
                    .ok_or("No begin_query")?;
                let artifact_adapter = ctx
                    .take::<InMemoryArtifactAdapter>("begin_artifact")
                    .ok_or("No begin_artifact")?;
                adapter.with_playbook_hook_body(&playbook_id, &filename, &content);
                let mut out = Context::new();
                out.set("begin_query", adapter);
                out.set("begin_artifact", artifact_adapter);
                Ok(out)
            },
        ),
        step_def(
            "the in-memory query adapter has a hook body for playbook {string} filename {string} that is over the hook context budget",
            &[("begin_query", "InMemoryQueryAdapter"), ("begin_artifact", "InMemoryArtifactAdapter")],
            &[("begin_query", "InMemoryQueryAdapter"), ("begin_artifact", "InMemoryArtifactAdapter")],
            |mut ctx, params| {
                let playbook_id = params.get_string(0).ok_or("Expected playbook_id")?.to_string();
                let filename = params.get_string(1).ok_or("Expected filename")?.to_string();
                // A body deterministically larger than the budget so the cap fires.
                let content = "X".repeat(anvil_core::domain::begin::HOOK_CONTEXT_BUDGET_BYTES + 4096);
                let mut adapter = ctx
                    .take::<InMemoryQueryAdapter>("begin_query")
                    .ok_or("No begin_query")?;
                let artifact_adapter = ctx
                    .take::<InMemoryArtifactAdapter>("begin_artifact")
                    .ok_or("No begin_artifact")?;
                adapter.with_playbook_hook_body(&playbook_id, &filename, &content);
                let mut out = Context::new();
                out.set("begin_query", adapter);
                out.set("begin_artifact", artifact_adapter);
                Ok(out)
            },
        ),
        check_def(
            "the begin outcome result context_text is within the hook context budget",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, _| {
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Ok(r) if r.result.context_text.len() <= anvil_core::domain::begin::HOOK_CONTEXT_BUDGET_BYTES => Ok(()),
                    Ok(r) => Err(format!(
                        "context_text is {} bytes, over the budget of {} bytes",
                        r.result.context_text.len(),
                        anvil_core::domain::begin::HOOK_CONTEXT_BUDGET_BYTES
                    )),
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the begin outcome result context_text is empty",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, _| {
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Ok(r) if r.result.context_text.is_empty() => Ok(()),
                    Ok(r) => Err(format!(
                        "context_text is not empty: '{}'",
                        r.result.context_text
                    )),
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the begin outcome result context_text contains {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Ok(r) if r.result.context_text.contains(needle) => Ok(()),
                    Ok(r) => Err(format!("context_text does not contain '{}'. context_text: {}", needle, r.result.context_text)),
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the begin outcome result context_text does not contain {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Ok(r) if !r.result.context_text.contains(needle) => Ok(()),
                    Ok(r) => Err(format!(
                        "context_text unexpectedly contains '{}'. context_text: '{}'",
                        needle, r.result.context_text
                    )),
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the begin outcome result measurement_role is {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected measurement_role")?;
                match ctx
                    .get::<Result<BeginOutcome, BeginError>>("begin_outcome")
                    .ok_or("No begin_outcome")?
                {
                    Ok(r) if r.result.measurement_role == *expected => Ok(()),
                    Ok(r) => Err(format!(
                        "Expected measurement_role '{}', got '{}'",
                        expected, r.result.measurement_role
                    )),
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),

        // ===== Domain-seam begin: When steps =====

        step_def(
            "begin is called via query adapter with identifier {string} and session_role {string}",
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |mut ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let session_role = params.get_string(1).ok_or("Expected session_role")?.to_string();
                let query = ctx
                    .take::<InMemoryQueryAdapter>("begin_query")
                    .ok_or("No begin_query")?;
                let artifact_adapter = ctx
                    .take::<InMemoryArtifactAdapter>("begin_artifact")
                    .ok_or("No begin_artifact")?;
                let request = BeginRequest {
                    identifier: id,
                    session_role: session_role.clone(),
                    actor_name: "Test-000000".to_string(),
                    actor_type: "agent".to_string(),
                    actor_model: "test-model".to_string(),
                    actor_provider: "test".to_string(),
                    ..Default::default()
                };
                // P4.4 migration: reviewer scenarios use FixtureTrackRegistry::new_reviewer
                // so that resolve_and_read_hook can find the (spec_review, reviewer) hook
                // declaration and serve the seeded hook body. Error-guard scenarios (not-found,
                // spec_not_ready_for_review, state_not_reviewable) fire before hook resolution
                // so the registry choice does not affect their outcome. Non-reviewer roles
                // (resumer, creator, unknown) bypass hook resolution entirely.
                let outcome = if session_role == "reviewer" {
                    let registry = FixtureTrackRegistry::new_reviewer(Some("spec-review.md"));
                    BeginCommandHandler::execute(&query, &registry, request)
                } else {
                    BeginCommandHandler::execute(&query, &SeedPlaybookRegistry, request)
                };
                // Route ReviewDocCreated events to populate review_doc_path.
                // Do NOT apply ReviewTransition — domain-seam tests assert on
                // the event itself, not its side effects.
                let outcome = outcome.map(|mut o| {
                    for event in &o.events {
                        if let Event::ReviewDocCreated { track_path, doc_name, header } = event {
                            if let Ok(path) = artifact_adapter.create_review_doc(track_path, doc_name, header) {
                                o.result.review_doc_path = path;
                            }
                        }
                    }
                    o
                });
                let mut out = Context::new();
                out.set("begin_outcome", outcome);
                out.set("begin_query", query);
                out.set("begin_artifact", artifact_adapter);
                Ok(out)
            },
        ),
        step_def(
            "begin is called via query adapter with both artifact_type {string} and identifier {string}",
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |mut ctx, params| {
                let at = params.get_string(0).ok_or("Expected artifact_type")?.to_string();
                let id = params.get_string(1).ok_or("Expected id")?.to_string();
                let query = ctx
                    .take::<InMemoryQueryAdapter>("begin_query")
                    .ok_or("No begin_query")?;
                let artifact_adapter = ctx
                    .take::<InMemoryArtifactAdapter>("begin_artifact")
                    .ok_or("No begin_artifact")?;
                let request = BeginRequest {
                    artifact_type: at,
                    identifier: id,
                    session_role: "reviewer".to_string(),
                    actor_name: "Test-000000".to_string(),
                    actor_type: "agent".to_string(),
                    actor_model: "test-model".to_string(),
                    actor_provider: "test".to_string(),
                    ..Default::default()
                };
                let outcome = BeginCommandHandler::execute(&query, &SeedPlaybookRegistry, request);
                let mut out = Context::new();
                out.set("begin_outcome", outcome);
                out.set("begin_query", query);
                out.set("begin_artifact", artifact_adapter);
                Ok(out)
            },
        ),
        step_def(
            "begin is called via query adapter with neither artifact_type nor identifier",
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |mut ctx, _params| {
                let query = ctx
                    .take::<InMemoryQueryAdapter>("begin_query")
                    .ok_or("No begin_query")?;
                let artifact_adapter = ctx
                    .take::<InMemoryArtifactAdapter>("begin_artifact")
                    .ok_or("No begin_artifact")?;
                let request = BeginRequest {
                    session_role: "reviewer".to_string(),
                    actor_name: "Test-000000".to_string(),
                    actor_type: "agent".to_string(),
                    actor_model: "test-model".to_string(),
                    actor_provider: "test".to_string(),
                    ..Default::default()
                };
                let outcome = BeginCommandHandler::execute(&query, &SeedPlaybookRegistry, request);
                let mut out = Context::new();
                out.set("begin_outcome", outcome);
                out.set("begin_query", query);
                out.set("begin_artifact", artifact_adapter);
                Ok(out)
            },
        ),
        step_def(
            "begin is called via query adapter with parent {string}",
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |mut ctx, params| {
                let parent_id = params.get_string(0).ok_or("Expected parent")?.to_string();
                let query = ctx
                    .take::<InMemoryQueryAdapter>("begin_query")
                    .ok_or("No begin_query")?;
                let artifact_adapter = ctx
                    .take::<InMemoryArtifactAdapter>("begin_artifact")
                    .ok_or("No begin_artifact")?;
                let request = BeginRequest {
                    artifact_type: "track".to_string(),
                    parent_id,
                    track_name: "test".to_string(),
                    approver: "mark".to_string(),
                    actor_name: "Test-000000".to_string(),
                    actor_type: "agent".to_string(),
                    actor_model: "test-model".to_string(),
                    actor_provider: "test".to_string(),
                    ..Default::default()
                };
                let outcome = BeginCommandHandler::execute(&query, &SeedPlaybookRegistry, request);
                let mut out = Context::new();
                out.set("begin_outcome", outcome);
                out.set("begin_query", query);
                out.set("begin_artifact", artifact_adapter);
                Ok(out)
            },
        ),
        step_def(
            "begin is called via query adapter with parent {string} and a registry declaring spec doer hook {string}",
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |mut ctx, params| {
                let parent_id = params.get_string(0).ok_or("Expected parent")?.to_string();
                let doer_hook = params.get_string(1).ok_or("Expected doer hook")?.to_string();
                let query = ctx
                    .take::<InMemoryQueryAdapter>("begin_query")
                    .ok_or("No begin_query")?;
                let artifact_adapter = ctx
                    .take::<InMemoryArtifactAdapter>("begin_artifact")
                    .ok_or("No begin_artifact")?;
                let request = BeginRequest {
                    artifact_type: "track".to_string(),
                    parent_id,
                    track_name: "test".to_string(),
                    approver: "mark".to_string(),
                    actor_name: "Test-000000".to_string(),
                    actor_type: "agent".to_string(),
                    actor_model: "test-model".to_string(),
                    actor_provider: "test".to_string(),
                    ..Default::default()
                };
                let registry = FixtureTrackRegistry::new(Some(&doer_hook));
                let outcome = BeginCommandHandler::execute(&query, &registry, request);
                let mut out = Context::new();
                out.set("begin_outcome", outcome);
                out.set("begin_query", query);
                out.set("begin_artifact", artifact_adapter);
                Ok(out)
            },
        ),
        step_def(
            "begin is called via query adapter with parent {string} and a registry declaring no spec doer hook",
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |mut ctx, params| {
                let parent_id = params.get_string(0).ok_or("Expected parent")?.to_string();
                let query = ctx
                    .take::<InMemoryQueryAdapter>("begin_query")
                    .ok_or("No begin_query")?;
                let artifact_adapter = ctx
                    .take::<InMemoryArtifactAdapter>("begin_artifact")
                    .ok_or("No begin_artifact")?;
                let request = BeginRequest {
                    artifact_type: "track".to_string(),
                    parent_id,
                    track_name: "test".to_string(),
                    approver: "mark".to_string(),
                    actor_name: "Test-000000".to_string(),
                    actor_type: "agent".to_string(),
                    actor_model: "test-model".to_string(),
                    actor_provider: "test".to_string(),
                    ..Default::default()
                };
                let registry = FixtureTrackRegistry::new(None);
                let outcome = BeginCommandHandler::execute(&query, &registry, request);
                let mut out = Context::new();
                out.set("begin_outcome", outcome);
                out.set("begin_query", query);
                out.set("begin_artifact", artifact_adapter);
                Ok(out)
            },
        ),
        // ===== P4 reviewer hook-serving When steps =====
        step_def(
            "begin is called via query adapter with identifier {string} and a registry declaring spec_review reviewer hook {string}",
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |mut ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let reviewer_hook = params.get_string(1).ok_or("Expected reviewer hook")?.to_string();
                let query = ctx
                    .take::<InMemoryQueryAdapter>("begin_query")
                    .ok_or("No begin_query")?;
                let artifact_adapter = ctx
                    .take::<InMemoryArtifactAdapter>("begin_artifact")
                    .ok_or("No begin_artifact")?;
                let request = BeginRequest {
                    identifier: id,
                    session_role: "reviewer".to_string(),
                    actor_name: "Test-000000".to_string(),
                    actor_type: "agent".to_string(),
                    actor_model: "test-model".to_string(),
                    actor_provider: "test".to_string(),
                    ..Default::default()
                };
                let registry = FixtureTrackRegistry::new_reviewer(Some(&reviewer_hook));
                let outcome = BeginCommandHandler::execute(&query, &registry, request);
                // Route ReviewDocCreated events to populate review_doc_path.
                let outcome = outcome.map(|mut o| {
                    for event in &o.events {
                        if let Event::ReviewDocCreated { track_path, doc_name, header } = event {
                            if let Ok(path) = artifact_adapter.create_review_doc(track_path, doc_name, header) {
                                o.result.review_doc_path = path;
                            }
                        }
                    }
                    o
                });
                let mut out = Context::new();
                out.set("begin_outcome", outcome);
                out.set("begin_query", query);
                out.set("begin_artifact", artifact_adapter);
                Ok(out)
            },
        ),
        step_def(
            "begin is called via query adapter with identifier {string} and a registry declaring no spec_review reviewer hook",
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |mut ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let query = ctx
                    .take::<InMemoryQueryAdapter>("begin_query")
                    .ok_or("No begin_query")?;
                let artifact_adapter = ctx
                    .take::<InMemoryArtifactAdapter>("begin_artifact")
                    .ok_or("No begin_artifact")?;
                let request = BeginRequest {
                    identifier: id,
                    session_role: "reviewer".to_string(),
                    actor_name: "Test-000000".to_string(),
                    actor_type: "agent".to_string(),
                    actor_model: "test-model".to_string(),
                    actor_provider: "test".to_string(),
                    ..Default::default()
                };
                let registry = FixtureTrackRegistry::new_reviewer(None);
                let outcome = BeginCommandHandler::execute(&query, &registry, request);
                // Route ReviewDocCreated events to populate review_doc_path.
                let outcome = outcome.map(|mut o| {
                    for event in &o.events {
                        if let Event::ReviewDocCreated { track_path, doc_name, header } = event {
                            if let Ok(path) = artifact_adapter.create_review_doc(track_path, doc_name, header) {
                                o.result.review_doc_path = path;
                            }
                        }
                    }
                    o
                });
                let mut out = Context::new();
                out.set("begin_outcome", outcome);
                out.set("begin_query", query);
                out.set("begin_artifact", artifact_adapter);
                Ok(out)
            },
        ),
        step_def(
            "begin is called via query adapter with identifier {string} session_role {string} and a registry declaring state {string} role {string} hook {string}",
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |mut ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let session_role = params.get_string(1).ok_or("Expected session_role")?.to_string();
                let state = params.get_string(2).ok_or("Expected state")?.to_string();
                let role = params.get_string(3).ok_or("Expected role")?.to_string();
                let hook = params.get_string(4).ok_or("Expected hook")?.to_string();
                let query = ctx
                    .take::<InMemoryQueryAdapter>("begin_query")
                    .ok_or("No begin_query")?;
                let artifact_adapter = ctx
                    .take::<InMemoryArtifactAdapter>("begin_artifact")
                    .ok_or("No begin_artifact")?;
                let request = BeginRequest {
                    identifier: id,
                    session_role,
                    actor_name: "Test-000000".to_string(),
                    actor_type: "agent".to_string(),
                    actor_model: "test-model".to_string(),
                    actor_provider: "test".to_string(),
                    ..Default::default()
                };
                let registry = FixtureTrackRegistry::new_state_role(&state, &role, &hook);
                let outcome = BeginCommandHandler::execute(&query, &registry, request);
                let outcome = outcome.map(|mut o| {
                    for event in &o.events {
                        if let Event::ReviewDocCreated { track_path, doc_name, header } = event {
                            if let Ok(path) = artifact_adapter.create_review_doc(track_path, doc_name, header) {
                                o.result.review_doc_path = path;
                            }
                        }
                    }
                    o
                });
                let mut out = Context::new();
                out.set("begin_outcome", outcome);
                out.set("begin_query", query);
                out.set("begin_artifact", artifact_adapter);
                Ok(out)
            },
        ),
        step_def(
            "begin is called via query adapter with artifact_type {string} and parent {string}",
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |mut ctx, params| {
                let artifact_type = params.get_string(0).ok_or("Expected type")?.to_string();
                let parent_id = params.get_string(1).ok_or("Expected parent")?.to_string();
                let query = ctx
                    .take::<InMemoryQueryAdapter>("begin_query")
                    .ok_or("No begin_query")?;
                let artifact_adapter = ctx
                    .take::<InMemoryArtifactAdapter>("begin_artifact")
                    .ok_or("No begin_artifact")?;
                let request = BeginRequest {
                    artifact_type,
                    parent_id,
                    track_name: "test".to_string(),
                    approver: "mark".to_string(),
                    actor_name: "Test-000000".to_string(),
                    actor_type: "agent".to_string(),
                    actor_model: "test-model".to_string(),
                    actor_provider: "test".to_string(),
                    ..Default::default()
                };
                let outcome = BeginCommandHandler::execute(&query, &SeedPlaybookRegistry, request);
                let mut out = Context::new();
                out.set("begin_outcome", outcome);
                out.set("begin_query", query);
                out.set("begin_artifact", artifact_adapter);
                Ok(out)
            },
        ),
        // ===== FD-4 injection-parity proof-gate steps =====
        // These two steps drive `begin` against the REAL `track_seed()`
        // (via `SeedPlaybookRegistry`) with the REAL on-disk hook body copied
        // from `<repo>/playbooks/track_lifecycle/hooks/<file>`. Together they
        // prove that the engine-served hook IS the migrated hook body verbatim
        // at every declared (state, role) seam — the absent piece the synthetic
        // begin_track_lifecycle_migrated_hooks fixtures cannot prove, because
        // those seed a hand-typed marker rather than reading the real file.
        step_def(
            "the in-memory query adapter loads the real track_lifecycle hook body for filename {string}",
            &[("begin_query", "InMemoryQueryAdapter"), ("begin_artifact", "InMemoryArtifactAdapter")],
            &[("begin_query", "InMemoryQueryAdapter"), ("begin_artifact", "InMemoryArtifactAdapter")],
            |mut ctx, params| {
                let filename = params.get_string(0).ok_or("Expected filename")?.to_string();
                let mut adapter = ctx
                    .take::<InMemoryQueryAdapter>("begin_query")
                    .ok_or("No begin_query")?;
                let artifact_adapter = ctx
                    .take::<InMemoryArtifactAdapter>("begin_artifact")
                    .ok_or("No begin_artifact")?;
                // CARGO_MANIFEST_DIR is <repo>/anvil-test-support; the real hooks
                // live at <repo>/playbooks/track_lifecycle/hooks/<filename>.
                let hook_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("..")
                    .join("playbooks")
                    .join("track_lifecycle")
                    .join("hooks")
                    .join(&filename);
                let body = std::fs::read_to_string(&hook_path)
                    .map_err(|e| format!("Failed to read real hook {}: {}", hook_path.display(), e))?;
                // Key the body by the playbook id the SeedPlaybookRegistry maps
                // `track` to, so begin's body read (playbook_id + filename) hits it.
                adapter.with_playbook_hook_body("20260422T0000_track_lifecycle", &filename, &body);
                let mut out = Context::new();
                out.set("begin_query", adapter);
                out.set("begin_artifact", artifact_adapter);
                Ok(out)
            },
        ),
        step_def(
            "begin is called via query adapter with the real track seed for identifier {string} session_role {string}",
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |mut ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let session_role = params.get_string(1).ok_or("Expected session_role")?.to_string();
                let query = ctx
                    .take::<InMemoryQueryAdapter>("begin_query")
                    .ok_or("No begin_query")?;
                let artifact_adapter = ctx
                    .take::<InMemoryArtifactAdapter>("begin_artifact")
                    .ok_or("No begin_artifact")?;
                let request = BeginRequest {
                    identifier: id,
                    session_role,
                    actor_name: "Test-000000".to_string(),
                    actor_type: "agent".to_string(),
                    actor_model: "test-model".to_string(),
                    actor_provider: "test".to_string(),
                    ..Default::default()
                };
                // The REAL seed registry: machine_for("track") == track_seed(),
                // whose hooks_by_role declarations are the proof-gate subject.
                let outcome = BeginCommandHandler::execute(&query, &SeedPlaybookRegistry, request);
                let outcome = outcome.map(|mut o| {
                    for event in &o.events {
                        if let Event::ReviewDocCreated { track_path, doc_name, header } = event {
                            if let Ok(path) = artifact_adapter.create_review_doc(track_path, doc_name, header) {
                                o.result.review_doc_path = path;
                            }
                        }
                    }
                    o
                });
                let mut out = Context::new();
                out.set("begin_outcome", outcome);
                out.set("begin_query", query);
                out.set("begin_artifact", artifact_adapter);
                Ok(out)
            },
        ),
        // Actor identity validation steps (empty field variants)
        step_def(
            "begin is called via query adapter with empty actor_name and parent {string}",
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |mut ctx, params| {
                let parent_id = params.get_string(0).ok_or("Expected parent")?.to_string();
                let query = ctx
                    .take::<InMemoryQueryAdapter>("begin_query")
                    .ok_or("No begin_query")?;
                let artifact_adapter = ctx
                    .take::<InMemoryArtifactAdapter>("begin_artifact")
                    .ok_or("No begin_artifact")?;
                let request = BeginRequest {
                    artifact_type: "track".to_string(),
                    parent_id,
                    track_name: "test".to_string(),
                    approver: "mark".to_string(),
                    actor_name: String::new(),
                    actor_type: "agent".to_string(),
                    actor_model: "test-model".to_string(),
                    actor_provider: "test".to_string(),
                    ..Default::default()
                };
                let outcome = BeginCommandHandler::execute(&query, &SeedPlaybookRegistry, request);
                let mut out = Context::new();
                out.set("begin_outcome", outcome);
                out.set("begin_query", query);
                out.set("begin_artifact", artifact_adapter);
                Ok(out)
            },
        ),
        step_def(
            "begin is called via query adapter with empty {string} and parent {string}",
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |mut ctx, params| {
                let field = params.get_string(0).ok_or("Expected field")?.to_string();
                let parent_id = params.get_string(1).ok_or("Expected parent")?.to_string();
                let query = ctx
                    .take::<InMemoryQueryAdapter>("begin_query")
                    .ok_or("No begin_query")?;
                let artifact_adapter = ctx
                    .take::<InMemoryArtifactAdapter>("begin_artifact")
                    .ok_or("No begin_artifact")?;
                let mut request = BeginRequest {
                    artifact_type: "track".to_string(),
                    parent_id,
                    track_name: "test".to_string(),
                    approver: "mark".to_string(),
                    actor_name: "Test-000000".to_string(),
                    actor_type: "agent".to_string(),
                    actor_model: "test-model".to_string(),
                    actor_provider: "test".to_string(),
                    ..Default::default()
                };
                match field.as_str() {
                    "actor_type" => request.actor_type = String::new(),
                    "actor_model" => request.actor_model = String::new(),
                    "actor_provider" => request.actor_provider = String::new(),
                    _ => return Err(format!("Unknown actor field: '{}'", field)),
                }
                let outcome = BeginCommandHandler::execute(&query, &SeedPlaybookRegistry, request);
                let mut out = Context::new();
                out.set("begin_outcome", outcome);
                out.set("begin_query", query);
                out.set("begin_artifact", artifact_adapter);
                Ok(out)
            },
        ),
        check_def(
            "the begin outcome is an ActorNameRequired error",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, _| {
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Err(BeginError::ActorNameRequired) => Ok(()),
                    other => Err(format!("Expected ActorNameRequired, got {:?}", other)),
                }
            },
        ),
        check_def(
            "the begin outcome is an ActorParamsRequired error for {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected field")?;
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Err(BeginError::ActorParamsRequired { field }) if field == expected => Ok(()),
                    other => Err(format!("Expected ActorParamsRequired for '{}', got {:?}", expected, other)),
                }
            },
        ),
        check_def(
            "the begin outcome is an UnsupportedType error for {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                use anvil_core::domain::checkin::CheckinError;
                let expected = params.get_string(0).ok_or("Expected type")?;
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Err(BeginError::Checkin(CheckinError::UnsupportedType { artifact_type })) if artifact_type == expected => Ok(()),
                    other => Err(format!("Expected UnsupportedType for '{}', got {:?}", expected, other)),
                }
            },
        ),
        check_def(
            "the begin outcome is a ParentNotFound error for {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                use anvil_core::domain::checkin::CheckinError;
                let expected = params.get_string(0).ok_or("Expected parent")?;
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Err(BeginError::Checkin(CheckinError::ParentNotFound { parent_id })) if parent_id == expected => Ok(()),
                    other => Err(format!("Expected ParentNotFound for '{}', got {:?}", expected, other)),
                }
            },
        ),
        check_def(
            "the begin outcome is a ParentNotActive error for {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                use anvil_core::domain::checkin::CheckinError;
                let expected = params.get_string(0).ok_or("Expected parent")?;
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Err(BeginError::Checkin(CheckinError::ParentNotActive { parent_id, .. })) if parent_id == expected => Ok(()),
                    other => Err(format!("Expected ParentNotActive for '{}', got {:?}", expected, other)),
                }
            },
        ),

        // ===== Playbook creation setup =====

        step_def(
            "an in-memory query adapter with track parent {string} in state {string}",
            &[],
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            |_ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let mut adapter = InMemoryQueryAdapter::new();
                adapter.with_artifact(&id, "track", &state);
                let mut out = Context::new();
                out.set("begin_query", adapter);
                out.set("begin_artifact", InMemoryArtifactAdapter::new());
                Ok(out)
            },
        ),
        step_def(
            "begin is called via query adapter with artifact_type {string} and parent {string} and playbook_name {string}",
            &[
                ("begin_query", "InMemoryQueryAdapter"),
                ("begin_artifact", "InMemoryArtifactAdapter"),
            ],
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |mut ctx, params| {
                let artifact_type = params.get_string(0).ok_or("Expected artifact_type")?.to_string();
                let parent_id = params.get_string(1).ok_or("Expected parent")?.to_string();
                let playbook_name = params.get_string(2).ok_or("Expected playbook_name")?.to_string();
                let query = ctx
                    .take::<InMemoryQueryAdapter>("begin_query")
                    .ok_or("No begin_query")?;
                let artifact_adapter = ctx
                    .take::<InMemoryArtifactAdapter>("begin_artifact")
                    .ok_or("No begin_artifact")?;
                let request = BeginRequest {
                    artifact_type,
                    parent_id,
                    playbook_name,
                    approver: "mark".to_string(),
                    actor_name: "Test-000000".to_string(),
                    actor_type: "agent".to_string(),
                    actor_model: "test-model".to_string(),
                    actor_provider: "test".to_string(),
                    ..Default::default()
                };
                let outcome = BeginCommandHandler::execute(&query, &SeedPlaybookRegistry, request);
                let mut out = Context::new();
                out.set("begin_outcome", outcome);
                out.set("begin_query", query);
                out.set("begin_artifact", artifact_adapter);
                Ok(out)
            },
        ),

        // ===== Domain-seam begin: Then steps (event assertions) =====

        check_def(
            "the begin outcome is successful",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, _| {
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Ok(_) => Ok(()),
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the begin outcome emits a PlaybookCreation event",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, _| {
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Ok(o) => {
                        if o.events.iter().any(|e| matches!(e, Event::PlaybookCreation { .. })) {
                            Ok(())
                        } else {
                            Err(format!(
                                "No PlaybookCreation event; events: {:?}",
                                o.events.iter().map(|e| format!("{:?}", e)).collect::<Vec<_>>()
                            ))
                        }
                    }
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the begin outcome PlaybookCreation has playbook_name {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected playbook_name")?;
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Ok(o) => {
                        let found = o.events.iter().find_map(|e| {
                            if let Event::PlaybookCreation { playbook_name, .. } = e {
                                Some(playbook_name.clone())
                            } else {
                                None
                            }
                        });
                        match found {
                            Some(name) if name == *expected => Ok(()),
                            Some(name) => Err(format!("Expected playbook_name '{}', got '{}'", expected, name)),
                            None => Err("No PlaybookCreation event found".to_string()),
                        }
                    }
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the begin outcome PlaybookCreation has parent_id {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected parent_id")?;
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Ok(o) => {
                        let found = o.events.iter().find_map(|e| {
                            if let Event::PlaybookCreation { parent_id, .. } = e {
                                Some(parent_id.clone())
                            } else {
                                None
                            }
                        });
                        match found {
                            Some(pid) if pid == *expected => Ok(()),
                            Some(pid) => Err(format!("Expected parent_id '{}', got '{}'", expected, pid)),
                            None => Err("No PlaybookCreation event found".to_string()),
                        }
                    }
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the begin outcome is a ParentKindInvalid error for {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                use anvil_core::domain::checkin::CheckinError;
                let expected = params.get_string(0).ok_or("Expected parent")?;
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Err(BeginError::Checkin(CheckinError::ParentKindInvalid { parent_id, .. })) if parent_id == expected => Ok(()),
                    other => Err(format!("Expected ParentKindInvalid for '{}', got {:?}", expected, other)),
                }
            },
        ),
        check_def(
            "the handler emitted a ReviewDocCreated event with doc name {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let expected_doc = params.get_string(0).ok_or("Expected doc name")?;
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Ok(o) => {
                        if o.events.iter().any(|e| matches!(e, Event::ReviewDocCreated { doc_name, .. } if doc_name == expected_doc)) {
                            Ok(())
                        } else {
                            Err(format!(
                                "No ReviewDocCreated with doc_name '{}'; events: {:?}",
                                expected_doc,
                                o.events.iter().map(|e| format!("{:?}", e)).collect::<Vec<_>>()
                            ))
                        }
                    }
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the handler emitted a BeginMarkerWritten event with state {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?;
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Ok(o) => {
                        if o.events.iter().any(|e| e.begin_marker_state() == Some(expected.as_ref())) {
                            Ok(())
                        } else {
                            Err(format!(
                                "No BeginMarkerWritten with state '{}'; events: {:?}",
                                expected,
                                o.events.iter().map(|e| format!("{:?}", e)).collect::<Vec<_>>()
                            ))
                        }
                    }
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the handler emitted a BeginMarkerWritten event with kind {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected kind")?;
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Ok(o) => {
                        if o.events.iter().any(|e| e.begin_marker_kind() == Some(expected.as_ref())) {
                            Ok(())
                        } else {
                            Err(format!(
                                "No BeginMarkerWritten with kind '{}'; events: {:?}",
                                expected,
                                o.events.iter().map(|e| format!("{:?}", e)).collect::<Vec<_>>()
                            ))
                        }
                    }
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the handler emitted no ReviewTransition event",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, _params| {
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Ok(o) => {
                        if o.events.iter().any(|e| matches!(e, Event::ReviewTransition { .. })) {
                            Err(format!(
                                "Expected no ReviewTransition event; found one in: {:?}",
                                o.events.iter().map(|e| format!("{:?}", e)).collect::<Vec<_>>()
                            ))
                        } else {
                            Ok(())
                        }
                    }
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the handler emitted no events",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, _params| {
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Ok(o) => {
                        if o.events.is_empty() {
                            Ok(())
                        } else {
                            Err(format!(
                                "Expected no events; found: {:?}",
                                o.events.iter().map(|e| format!("{:?}", e)).collect::<Vec<_>>()
                            ))
                        }
                    }
                    Err(_) => Ok(()), // Error path — no events is structurally guaranteed
                }
            },
        ),
        check_def(
            "the begin outcome result state is {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?;
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Ok(o) if o.result.state == *expected => Ok(()),
                    Ok(o) => Err(format!("Expected state '{}', got '{}'", expected, o.result.state)),
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the begin outcome result artifact_text contains {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected text")?;
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Ok(o) if o.result.artifact_text.contains(expected) => Ok(()),
                    Ok(o) => Err(format!("artifact_text '{}' doesn't contain '{}'", o.result.artifact_text, expected)),
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the begin outcome result review_context_text contains {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected text")?;
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Ok(o) if o.result.review_context_text.contains(expected) => Ok(()),
                    Ok(o) => Err(format!("review_context_text '{}' doesn't contain '{}'", o.result.review_context_text, expected)),
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the begin outcome result review_context_text is empty",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, _| {
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Ok(o) if o.result.review_context_text.is_empty() => Ok(()),
                    Ok(o) => Err(format!(
                        "review_context_text is not empty: '{}'",
                        o.result.review_context_text
                    )),
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the begin outcome result review_doc_path is set",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, _| {
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Ok(o) if !o.result.review_doc_path.is_empty() => Ok(()),
                    Ok(_) => Err("review_doc_path is empty".to_string()),
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the begin outcome result artifact_text is empty",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, _| {
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Ok(o) if o.result.artifact_text.is_empty() => Ok(()),
                    Ok(o) => Err(format!("artifact_text is not empty: '{}'", o.result.artifact_text)),
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the handler did not emit a ReviewDocCreated event",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, _| {
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Ok(o) => {
                        if o.events.iter().any(|e| matches!(e, Event::ReviewDocCreated { .. })) {
                            Err("expected no ReviewDocCreated event, but one was emitted".to_string())
                        } else {
                            Ok(())
                        }
                    }
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the begin outcome is a NotFound error for {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected identifier")?;
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Err(BeginError::NotFound { identifier }) if identifier == expected => Ok(()),
                    other => Err(format!("Expected NotFound for '{}', got {:?}", expected, other)),
                }
            },
        ),
        check_def(
            "the begin outcome is a StateNotReviewable error naming {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected fallback")?;
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Err(BeginError::StateNotReviewable { fallback_skill, .. }) if fallback_skill == expected => Ok(()),
                    other => Err(format!("Expected StateNotReviewable naming '{}', got {:?}", expected, other)),
                }
            },
        ),
        check_def(
            "the begin outcome is a RoleStateMismatch error for role {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected role")?;
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Err(BeginError::RoleStateMismatch { role, .. }) if role == expected => Ok(()),
                    other => Err(format!("Expected RoleStateMismatch for '{}', got {:?}", expected, other)),
                }
            },
        ),
        check_def(
            "the begin outcome is a ModeNotImplemented error naming {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected fallback")?;
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Err(BeginError::ModeNotImplemented { fallback_skill, .. }) if fallback_skill == &expected => Ok(()),
                    other => Err(format!("Expected ModeNotImplemented naming '{}', got {:?}", expected, other)),
                }
            },
        ),
        check_def(
            "the begin outcome is a SessionRequired error",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, _| {
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Err(BeginError::SessionRequired) => Ok(()),
                    other => Err(format!("Expected SessionRequired, got {:?}", other)),
                }
            },
        ),
        check_def(
            "the begin outcome is an InvalidArgument error",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, _| {
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Err(BeginError::InvalidArgument { .. }) => Ok(()),
                    other => Err(format!("Expected InvalidArgument, got {:?}", other)),
                }
            },
        ),
        check_def(
            "the begin outcome is a SpecNotReadyForReview error",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, _| {
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Err(BeginError::SpecNotReadyForReview { .. }) => Ok(()),
                    other => Err(format!("Expected SpecNotReadyForReview, got {:?}", other)),
                }
            },
        ),
        check_def(
            "the begin outcome error message contains {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected text")?;
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Err(e) => {
                        let msg = format!("{}", e);
                        if msg.contains(expected) {
                            Ok(())
                        } else {
                            Err(format!("Error message '{}' doesn't contain '{}'", msg, expected))
                        }
                    }
                    Ok(_) => Err("Expected error, got success".to_string()),
                }
            },
        ),
        check_def(
            "the begin outcome error message does not contain {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let unexpected = params.get_string(0).ok_or("Expected text")?;
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Err(e) => {
                        let msg = format!("{}", e);
                        if msg.contains(unexpected) {
                            Err(format!("Error message '{}' unexpectedly contains '{}'", msg, unexpected))
                        } else {
                            Ok(())
                        }
                    }
                    Ok(_) => Err("Expected error, got success".to_string()),
                }
            },
        ),

        // ===== N1: fs-backed begin steps (real FileSystemQueryAdapter) =====
        // These steps are deliberately distinct from the in-memory begin steps
        // above (which run BeginCommandHandler against InMemoryQueryAdapter, a
        // fallback-less HashMap lookup that never touches
        // FileSystemQueryAdapter::read_artifact_state). They seed a real
        // on-disk hearth and drive begin through the REAL FileSystemQueryAdapter
        // so the stateless-parent fallback at fs_query_adapter.rs is exercised.
        step_def(
            "a begin fs hearth with:",
            &[],
            &[
                ("begin_fs_hearth", "PathBuf"),
                ("begin_fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let (handle, tmp) = retained_temp_dir("anvil-begin-fs-")?;
                std::fs::create_dir_all(&tmp)
                    .map_err(|e| format!("Failed to create hearth: {}", e))?;
                let mut pairs: Vec<(String, String)> = Vec::new();
                if table.headers.len() >= 2 {
                    pairs.push((table.headers[0].clone(), table.headers[1].clone()));
                }
                for row in &table.rows {
                    if row.len() >= 2 {
                        pairs.push((row[0].clone(), row[1].clone()));
                    }
                }
                for (path, content) in pairs {
                    let full = tmp.join(path.trim());
                    if let Some(parent) = full.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| format!("Failed to create dir: {}", e))?;
                    }
                    let content = content.replace("\\n", "\n");
                    std::fs::write(&full, content)
                        .map_err(|e| format!("Failed to write {}: {}", full.display(), e))?;
                }
                let mut out = Context::new();
                out.set("begin_fs_hearth", tmp);
                out.set("begin_fs_hearth_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "begin fs is executed with parent {string}",
            &[("begin_fs_hearth", "PathBuf")],
            &[
                ("begin_fs_hearth", "PathBuf"),
                ("begin_fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("begin_outcome", "Result<BeginOutcome, BeginError>"),
            ],
            |ctx, params| {
                let parent_id = params.get_string(0).ok_or("Expected parent")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>("begin_fs_hearth")
                    .ok_or("No begin_fs_hearth")?
                    .clone();
                let query = FileSystemQueryAdapter::new(hearth.clone());
                let request = BeginRequest {
                    artifact_type: "track".to_string(),
                    parent_id,
                    track_name: "test".to_string(),
                    approver: "mark".to_string(),
                    actor_name: "Test-000000".to_string(),
                    actor_type: "agent".to_string(),
                    actor_model: "test-model".to_string(),
                    actor_provider: "test".to_string(),
                    ..Default::default()
                };
                let outcome = BeginCommandHandler::execute(&query, &SeedPlaybookRegistry, request);
                let mut out = Context::new();
                out.set("begin_fs_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "begin_fs_hearth_handle");
                out.set("begin_outcome", outcome);
                Ok(out)
            },
        ),
        // ===== P5.1: hearth-first hook serving (AC-4) =====
        // Seed a real on-disk hearth whose workflows/{id}/machine.yaml declares
        // the (spec, doer) hook and whose hooks/spec-writing.md holds the body.
        // The begin call resolves the declaration via a CompositePlaybookRegistry
        // (HearthPlaybookRegistry first) and reads the body via the real
        // FileSystemQueryAdapter — both reads come from disk, so an in-process
        // edit to the hooks/ file changes serving with no rebuild.
        step_def(
            "a begin fs hearth declaring a spec doer hook with body:",
            &[],
            &[
                ("begin_fs_hearth", "PathBuf"),
                ("begin_fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let body = params.doc_string().ok_or("Expected doc string")?.to_string();
                let (handle, tmp) = retained_temp_dir("anvil-begin-hearth-first-")?;
                std::fs::create_dir_all(&tmp)
                    .map_err(|e| format!("Failed to create hearth: {}", e))?;

                // Structural hearth signature so fs adapters accept the hearth.
                std::fs::create_dir_all(tmp.join("tracks"))
                    .map_err(|e| format!("Failed to create tracks dir: {}", e))?;
                std::fs::write(tmp.join("tracks.md"), "# Tracks\n")
                    .map_err(|e| format!("Failed to write tracks.md: {}", e))?;

                // Active parent proposal so the create flow passes parent checks.
                let parent_dir = tmp.join("proposals").join("20260601T0000_parent");
                std::fs::create_dir_all(&parent_dir)
                    .map_err(|e| format!("Failed to create parent dir: {}", e))?;
                std::fs::write(
                    parent_dir.join("status.yaml"),
                    "version: 1\nkind: proposal\nstate: active\n",
                )
                .map_err(|e| format!("Failed to write parent status.yaml: {}", e))?;

                // On-disk track playbook declaring the (spec, doer) hook.
                let wf_dir = tmp.join("playbooks").join("20260422T0000_track_lifecycle");
                let hooks_dir = wf_dir.join("hooks");
                std::fs::create_dir_all(&hooks_dir)
                    .map_err(|e| format!("Failed to create hooks dir: {}", e))?;
                let machine_yaml = "\
kind: track
directory: tracks
registry: tracks.md
parent_kind: proposal
description: Track lifecycle (test fixture).
required_fields: []
roles:
  - doer
  - reviewer
states:
  - name: spec
    role_filters: []
    registry_section: \"\"
    projection_targets: []
    is_review_gate: false
    is_terminal: false
    hooks_by_role:
      doer: spec-writing.md
transitions: []
";
                std::fs::write(wf_dir.join("machine.yaml"), machine_yaml)
                    .map_err(|e| format!("Failed to write machine.yaml: {}", e))?;
                std::fs::write(hooks_dir.join("spec-writing.md"), &body)
                    .map_err(|e| format!("Failed to write hook body: {}", e))?;

                let mut out = Context::new();
                out.set("begin_fs_hearth", tmp);
                out.set("begin_fs_hearth_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "the on-disk doer hook body is rewritten to:",
            &[("begin_fs_hearth", "PathBuf")],
            &[
                ("begin_fs_hearth", "PathBuf"),
                ("begin_fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let body = params.doc_string().ok_or("Expected doc string")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>("begin_fs_hearth")
                    .ok_or("No begin_fs_hearth")?
                    .clone();
                let hook_path = hearth
                    .join("playbooks")
                    .join("20260422T0000_track_lifecycle")
                    .join("hooks")
                    .join("spec-writing.md");
                std::fs::write(&hook_path, &body)
                    .map_err(|e| format!("Failed to rewrite hook body: {}", e))?;
                let mut out = Context::new();
                out.set("begin_fs_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "begin_fs_hearth_handle");
                Ok(out)
            },
        ),
        step_def(
            "begin fs hearth-first is executed with parent {string}",
            &[("begin_fs_hearth", "PathBuf")],
            &[
                ("begin_fs_hearth", "PathBuf"),
                ("begin_fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("begin_outcome", "Result<BeginOutcome, BeginError>"),
            ],
            |ctx, params| {
                let parent_id = params.get_string(0).ok_or("Expected parent")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>("begin_fs_hearth")
                    .ok_or("No begin_fs_hearth")?
                    .clone();
                // Build a fresh CompositePlaybookRegistry per call — this IS the
                // always-reload mechanism that makes the in-process edit visible.
                let registry = CompositePlaybookRegistry::new(
                    HearthPlaybookRegistry::new(hearth.clone()),
                    SeedPlaybookRegistry,
                );
                let query = FileSystemQueryAdapter::new(hearth.clone());
                let request = BeginRequest {
                    artifact_type: "track".to_string(),
                    parent_id,
                    track_name: "test".to_string(),
                    approver: "mark".to_string(),
                    actor_name: "Test-000000".to_string(),
                    actor_type: "agent".to_string(),
                    actor_model: "test-model".to_string(),
                    actor_provider: "test".to_string(),
                    ..Default::default()
                };
                let outcome = BeginCommandHandler::execute(&query, &registry, request);
                let mut out = Context::new();
                out.set("begin_fs_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "begin_fs_hearth_handle");
                out.set("begin_outcome", outcome);
                Ok(out)
            },
        ),
        // ===== BP0: domain-kind resolution against a real fs hearth =====
        // Seed an arbitrary hearth file tree from a `| path | content |` table,
        // then exercise read_artifact_kind / read_artifact_state through the
        // REAL FileSystemQueryAdapter so the status.yaml-driven kind resolution
        // and the scan-for-dir discovery (for non-legacy directories) are
        // exercised end-to-end.
        step_def(
            "a domain-kind fs hearth with:",
            &[],
            &[
                ("domain_fs_hearth", "PathBuf"),
                ("domain_fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let (handle, tmp) = retained_temp_dir("anvil-domain-kind-fs-")?;
                std::fs::create_dir_all(&tmp)
                    .map_err(|e| format!("Failed to create hearth: {}", e))?;
                // The header row is `| path | content |` (column names), so do
                // NOT treat it as data — only the data rows carry files.
                for row in &table.rows {
                    if row.len() < 2 {
                        continue;
                    }
                    let full = tmp.join(row[0].trim());
                    if let Some(parent) = full.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| format!("Failed to create dir: {}", e))?;
                    }
                    let content = row[1].replace("\\n", "\n");
                    std::fs::write(&full, content)
                        .map_err(|e| format!("Failed to write {}: {}", full.display(), e))?;
                }
                let mut out = Context::new();
                out.set("domain_fs_hearth", tmp);
                out.set("domain_fs_hearth_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "fs_query_adapter.read_artifact_kind is called for {string}",
            &[("domain_fs_hearth", "PathBuf")],
            &[
                ("domain_fs_hearth", "PathBuf"),
                ("domain_fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("domain_kind_result", "QueryOutcome"),
            ],
            |ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>("domain_fs_hearth")
                    .ok_or("No domain_fs_hearth")?
                    .clone();
                let adapter = FileSystemQueryAdapter::new(hearth.clone());
                let result = adapter.read_artifact_kind(&id);
                let mut out = Context::new();
                out.set("domain_fs_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "domain_fs_hearth_handle");
                out.set("domain_kind_result", result);
                Ok(out)
            },
        ),
        step_def(
            "fs_query_adapter.read_artifact_state is called for {string}",
            &[("domain_fs_hearth", "PathBuf")],
            &[
                ("domain_fs_hearth", "PathBuf"),
                ("domain_fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("domain_state_result", "QueryOutcome"),
            ],
            |ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>("domain_fs_hearth")
                    .ok_or("No domain_fs_hearth")?
                    .clone();
                let adapter = FileSystemQueryAdapter::new(hearth.clone());
                let result = adapter.read_artifact_state(&id);
                let mut out = Context::new();
                out.set("domain_fs_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "domain_fs_hearth_handle");
                out.set("domain_state_result", result);
                Ok(out)
            },
        ),
        check_def(
            "the fs domain query adapter returns kind {string}",
            &[("domain_kind_result", "QueryOutcome")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected kind")?;
                let r = ctx
                    .get::<QueryOutcome>("domain_kind_result")
                    .ok_or("No domain_kind_result")?;
                match r {
                    Ok(k) if k == expected => Ok(()),
                    Ok(k) => Err(format!("Expected kind '{}', got '{}'", expected, k)),
                    Err(e) => Err(format!("Expected kind '{}', got error: {}", expected, e)),
                }
            },
        ),
        check_def(
            "the fs domain query adapter returns state {string}",
            &[("domain_state_result", "QueryOutcome")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?;
                let r = ctx
                    .get::<QueryOutcome>("domain_state_result")
                    .ok_or("No domain_state_result")?;
                match r {
                    Ok(s) if s == expected => Ok(()),
                    Ok(s) => Err(format!("Expected state '{}', got '{}'", expected, s)),
                    Err(e) => Err(format!("Expected state '{}', got error: {}", expected, e)),
                }
            },
        ),
        // ===== BP1: begin create through a CompositePlaybookRegistry over an
        // fs hearth that physically holds the knowledge_lifecycle machine.yaml.
        // knowledge_lifecycle resolves ONLY via HearthPlaybookRegistry (no
        // SeedPlaybookRegistry arm covers it), so this step seeds the playbook
        // dir on disk and drives begin through the real composite. =====
        step_def(
            "a composite begin fs hearth seeded with the knowledge_lifecycle machine",
            &[],
            &[
                ("composite_begin_hearth", "PathBuf"),
                ("composite_begin_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, _params| {
                let (handle, tmp) = seed_knowledge_lifecycle_hearth(None)?;
                let mut out = Context::new();
                out.set("composite_begin_hearth", tmp);
                out.set("composite_begin_hearth_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a composite begin fs hearth seeded with the knowledge_lifecycle machine and an active proposal {string}",
            &[],
            &[
                ("composite_begin_hearth", "PathBuf"),
                ("composite_begin_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let parent = params.get_string(0).ok_or("Expected parent id")?.to_string();
                let (handle, tmp) = seed_knowledge_lifecycle_hearth(Some(&parent))?;
                let mut out = Context::new();
                out.set("composite_begin_hearth", tmp);
                out.set("composite_begin_hearth_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a composite begin fs hearth seeded with an active proposal {string}",
            &[],
            &[
                ("composite_begin_hearth", "PathBuf"),
                ("composite_begin_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let parent = params.get_string(0).ok_or("Expected parent id")?.to_string();
                let (handle, tmp) = retained_temp_dir("anvil-required-fields-")?;
                std::fs::create_dir_all(tmp.join("tracks"))
                    .map_err(|e| format!("Failed to create tracks dir: {}", e))?;
                std::fs::write(tmp.join("tracks.md"), "# Tracks\n")
                    .map_err(|e| format!("Failed to write tracks.md: {}", e))?;
                let parent_dir = tmp.join("proposals").join(&parent);
                std::fs::create_dir_all(&parent_dir)
                    .map_err(|e| format!("Failed to create parent dir: {}", e))?;
                std::fs::write(
                    parent_dir.join("status.yaml"),
                    "version: 1\nkind: proposal\nstate: active\n",
                )
                .map_err(|e| format!("Failed to write parent status.yaml: {}", e))?;
                let mut out = Context::new();
                out.set("composite_begin_hearth", tmp);
                out.set("composite_begin_hearth_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "begin fs composite create is executed for artifact_type {string} with no parent",
            &[("composite_begin_hearth", "PathBuf")],
            &[
                ("composite_begin_hearth", "PathBuf"),
                ("composite_begin_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("begin_outcome", "Result<BeginOutcome, BeginError>"),
            ],
            |ctx, params| {
                let artifact_type = params.get_string(0).ok_or("Expected type")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>("composite_begin_hearth")
                    .ok_or("No composite_begin_hearth")?
                    .clone();
                let outcome = run_composite_begin_create(&hearth, &artifact_type, "");
                let mut out = Context::new();
                out.set("composite_begin_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "composite_begin_hearth_handle");
                out.set("begin_outcome", outcome);
                Ok(out)
            },
        ),
        step_def(
            "begin fs composite create is executed for artifact_type {string} with parent {string} and conversation_id {string}",
            &[("composite_begin_hearth", "PathBuf")],
            &[
                ("composite_begin_hearth", "PathBuf"),
                ("composite_begin_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("begin_outcome", "Result<BeginOutcome, BeginError>"),
            ],
            |ctx, params| {
                let artifact_type = params.get_string(0).ok_or("Expected type")?.to_string();
                let parent = params.get_string(1).ok_or("Expected parent")?.to_string();
                let conversation_id = params.get_string(2).ok_or("Expected conversation_id")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>("composite_begin_hearth")
                    .ok_or("No composite_begin_hearth")?
                    .clone();
                let outcome = run_composite_begin_create_with_conversation(
                    &hearth,
                    &artifact_type,
                    &parent,
                    &conversation_id,
                );
                let mut out = Context::new();
                out.set("composite_begin_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "composite_begin_hearth_handle");
                out.set("begin_outcome", outcome);
                Ok(out)
            },
        ),
        check_def(
            "the begin outcome emits an ArtifactCreation event with conversation_id {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected conversation_id")?;
                let outcome = ctx
                    .get::<Result<BeginOutcome, BeginError>>("begin_outcome")
                    .ok_or("No begin_outcome")?;
                match outcome {
                    Ok(o) => {
                        let found = o
                            .events
                            .iter()
                            .find_map(|e| e.artifact_creation_conversation_id());
                        let expected: &str = &expected;
                        match found {
                            Some(c) if c == expected => Ok(()),
                            Some(c) => Err(format!(
                                "ArtifactCreation conversation_id mismatch: got '{}', expected '{}'",
                                c, expected
                            )),
                            None => Err(format!(
                                "No ArtifactCreation event in outcome; events: {:?}",
                                o.events
                            )),
                        }
                    }
                    Err(e) => Err(format!("Expected ArtifactCreation, got error: {:?}", e)),
                }
            },
        ),
        step_def(
            "begin fs composite create is executed for artifact_type {string} parent {string} track_name {string} approver {string}",
            &[("composite_begin_hearth", "PathBuf")],
            &[
                ("composite_begin_hearth", "PathBuf"),
                ("composite_begin_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("begin_outcome", "Result<BeginOutcome, BeginError>"),
            ],
            |ctx, params| {
                let artifact_type = params.get_string(0).ok_or("Expected artifact_type")?.to_string();
                let parent = params.get_string(1).ok_or("Expected parent")?.to_string();
                let track_name = params.get_string(2).ok_or("Expected track_name")?.to_string();
                let approver = params.get_string(3).ok_or("Expected approver")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>("composite_begin_hearth")
                    .ok_or("No composite_begin_hearth")?
                    .clone();
                let registry = CompositePlaybookRegistry::new(
                    HearthPlaybookRegistry::new(hearth.clone()),
                    SeedPlaybookRegistry,
                );
                let query = FileSystemQueryAdapter::new(hearth.clone());
                let request = BeginRequest {
                    artifact_type,
                    parent_id: parent,
                    track_name,
                    approver,
                    actor_name: "Test-000000".to_string(),
                    actor_type: "agent".to_string(),
                    actor_model: "test-model".to_string(),
                    actor_provider: "test".to_string(),
                    ..Default::default()
                };
                let outcome = BeginCommandHandler::execute(&query, &registry, request);
                let mut out = Context::new();
                out.set("composite_begin_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "composite_begin_hearth_handle");
                out.set("begin_outcome", outcome);
                Ok(out)
            },
        ),
        step_def(
            "begin fs composite create is executed for artifact_type {string} with parent {string}",
            &[("composite_begin_hearth", "PathBuf")],
            &[
                ("composite_begin_hearth", "PathBuf"),
                ("composite_begin_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("begin_outcome", "Result<BeginOutcome, BeginError>"),
            ],
            |ctx, params| {
                let artifact_type = params.get_string(0).ok_or("Expected type")?.to_string();
                let parent = params.get_string(1).ok_or("Expected parent")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>("composite_begin_hearth")
                    .ok_or("No composite_begin_hearth")?
                    .clone();
                let outcome = run_composite_begin_create(&hearth, &artifact_type, &parent);
                let mut out = Context::new();
                out.set("composite_begin_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "composite_begin_hearth_handle");
                out.set("begin_outcome", outcome);
                Ok(out)
            },
        ),
        check_def(
            "the begin outcome emits an ArtifactCreation event with kind {string} state {string} directory {string} registry {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let ek = params.get_string(0).ok_or("Expected kind")?;
                let es = params.get_string(1).ok_or("Expected state")?;
                let ed = params.get_string(2).ok_or("Expected directory")?;
                let er = params.get_string(3).ok_or("Expected registry")?;
                let outcome = ctx
                    .get::<Result<BeginOutcome, BeginError>>("begin_outcome")
                    .ok_or("No begin_outcome")?;
                match outcome {
                    Ok(o) => {
                        let found = o.events.iter().find_map(|e| e.artifact_creation_placement());
                        match found {
                            Some((k, s, d, r)) if k == ek && s == es && d == ed && r == er => Ok(()),
                            Some((k, s, d, r)) => Err(format!(
                                "ArtifactCreation placement mismatch: got kind={} state={} dir={} registry={}, expected kind={} state={} dir={} registry={}",
                                k, s, d, r, ek, es, ed, er
                            )),
                            None => Err(format!(
                                "No ArtifactCreation event in outcome; events: {:?}",
                                o.events
                            )),
                        }
                    }
                    Err(e) => Err(format!("Expected ArtifactCreation, got error: {:?}", e)),
                }
            },
        ),
        // ===== target_owner (Anvil-lane 1b) =====
        step_def(
            "a composite begin fs hearth seeded with a machine requiring target_owner and an active parent {string}",
            &[],
            &[
                ("composite_begin_hearth", "PathBuf"),
                ("composite_begin_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let parent = params.get_string(0).ok_or("Expected parent id")?.to_string();
                let (handle, tmp) = seed_target_owner_required_hearth(&parent)?;
                let mut out = Context::new();
                out.set("composite_begin_hearth", tmp);
                out.set("composite_begin_hearth_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "begin fs composite create is executed for artifact_type {string} parent {string} target_owner {string}",
            &[("composite_begin_hearth", "PathBuf")],
            &[
                ("composite_begin_hearth", "PathBuf"),
                ("composite_begin_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("begin_outcome", "Result<BeginOutcome, BeginError>"),
            ],
            |ctx, params| {
                let artifact_type = params.get_string(0).ok_or("Expected type")?.to_string();
                let parent = params.get_string(1).ok_or("Expected parent")?.to_string();
                let target_owner = params.get_string(2).ok_or("Expected target_owner")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>("composite_begin_hearth")
                    .ok_or("No composite_begin_hearth")?
                    .clone();
                let outcome =
                    run_composite_begin_create_target_owner(&hearth, &artifact_type, &parent, &target_owner);
                let mut out = Context::new();
                out.set("composite_begin_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "composite_begin_hearth_handle");
                out.set("begin_outcome", outcome);
                Ok(out)
            },
        ),
        check_def(
            "the begin outcome is a MissingRequiredField error for {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                use anvil_core::domain::checkin::CheckinError;
                let expected = params.get_string(0).ok_or("Expected field")?;
                match ctx.get::<Result<BeginOutcome, BeginError>>("begin_outcome").ok_or("No begin_outcome")? {
                    Err(BeginError::Checkin(CheckinError::MissingRequiredField { field })) if field == expected => Ok(()),
                    other => Err(format!("Expected MissingRequiredField for '{}', got {:?}", expected, other)),
                }
            },
        ),
        check_def(
            "the begin outcome emits an ArtifactCreation event with status target_owner {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected target_owner")?;
                let outcome = ctx
                    .get::<Result<BeginOutcome, BeginError>>("begin_outcome")
                    .ok_or("No begin_outcome")?;
                match outcome {
                    Ok(o) => {
                        let found = o.events.iter().find_map(|e| e.artifact_creation_target_owner());
                        match found {
                            Some(t) if t == expected => Ok(()),
                            Some(t) => Err(format!(
                                "ArtifactCreation target_owner mismatch: got '{}', expected '{}'",
                                t, expected
                            )),
                            None => Err(format!(
                                "No ArtifactCreation event in outcome; events: {:?}",
                                o.events
                            )),
                        }
                    }
                    Err(e) => Err(format!("Expected ArtifactCreation, got error: {:?}", e)),
                }
            },
        ),
        check_def(
            "no begin artifact status exists under {string}",
            &[("composite_begin_hearth", "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected relative directory")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>("composite_begin_hearth")
                    .ok_or("No composite_begin_hearth")?;
                let dir = hearth.join(rel);
                if !dir.exists() {
                    return Ok(());
                }
                let mut status_paths = Vec::new();
                for entry in std::fs::read_dir(&dir)
                    .map_err(|e| format!("Failed to read {}: {}", dir.display(), e))?
                {
                    let entry = entry.map_err(|e| e.to_string())?;
                    let status = entry.path().join("status.yaml");
                    if status.exists() {
                        status_paths.push(status.display().to_string());
                    }
                }
                if status_paths.is_empty() {
                    Ok(())
                } else {
                    Err(format!("Expected no status.yaml under {}, found {:?}", dir.display(), status_paths))
                }
            },
        ),
        // ===== generic field bag (any machine-declared required field) =====
        step_def(
            "a composite begin fs hearth seeded with a machine requiring generic fields question and requester",
            &[],
            &[
                ("composite_begin_hearth", "PathBuf"),
                ("composite_begin_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, _params| {
                let (handle, tmp) = seed_generic_fields_required_hearth()?;
                let mut out = Context::new();
                out.set("composite_begin_hearth", tmp);
                out.set("composite_begin_hearth_handle", handle);
                Ok(out)
            },
        ),
        // ===== a machine declaring the pre-migration name field =====
        step_def(
            "a composite begin fs hearth seeded with a machine declaring the pre-migration name field",
            &[],
            &[
                ("composite_begin_hearth", "PathBuf"),
                ("composite_begin_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, _params| {
                let (handle, tmp) = seed_legacy_name_field_hearth()?;
                let mut out = Context::new();
                out.set("composite_begin_hearth", tmp);
                out.set("composite_begin_hearth_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "begin fs composite create is executed for artifact_type {string} with the dedicated name {string}",
            &[("composite_begin_hearth", "PathBuf")],
            &[
                ("composite_begin_hearth", "PathBuf"),
                ("composite_begin_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("begin_outcome", "Result<BeginOutcome, BeginError>"),
            ],
            |ctx, params| {
                let artifact_type = params.get_string(0).ok_or("Expected type")?.to_string();
                let name = params.get_string(1).ok_or("Expected name")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>("composite_begin_hearth")
                    .ok_or("No composite_begin_hearth")?
                    .clone();
                let outcome = run_composite_begin_create_named(
                    &hearth,
                    &artifact_type,
                    &name,
                    std::collections::BTreeMap::new(),
                );
                let mut out = Context::new();
                out.set("composite_begin_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "composite_begin_hearth_handle");
                out.set("begin_outcome", outcome);
                Ok(out)
            },
        ),
        step_def(
            "begin fs composite create is executed for artifact_type {string} with the dedicated name {string} and field {string} = {string}",
            &[("composite_begin_hearth", "PathBuf")],
            &[
                ("composite_begin_hearth", "PathBuf"),
                ("composite_begin_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("begin_outcome", "Result<BeginOutcome, BeginError>"),
            ],
            |ctx, params| {
                let artifact_type = params.get_string(0).ok_or("Expected type")?.to_string();
                let name = params.get_string(1).ok_or("Expected name")?.to_string();
                let mut fields = std::collections::BTreeMap::new();
                fields.insert(
                    params.get_string(2).ok_or("k1")?.to_string(),
                    params.get_string(3).ok_or("v1")?.to_string(),
                );
                let hearth = ctx
                    .get::<PathBuf>("composite_begin_hearth")
                    .ok_or("No composite_begin_hearth")?
                    .clone();
                let outcome =
                    run_composite_begin_create_named(&hearth, &artifact_type, &name, fields);
                let mut out = Context::new();
                out.set("composite_begin_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "composite_begin_hearth_handle");
                out.set("begin_outcome", outcome);
                Ok(out)
            },
        ),
        step_def(
            "begin fs composite create is executed for artifact_type {string} with no generic fields",
            &[("composite_begin_hearth", "PathBuf")],
            &[
                ("composite_begin_hearth", "PathBuf"),
                ("composite_begin_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("begin_outcome", "Result<BeginOutcome, BeginError>"),
            ],
            |ctx, params| {
                let artifact_type = params.get_string(0).ok_or("Expected type")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>("composite_begin_hearth")
                    .ok_or("No composite_begin_hearth")?
                    .clone();
                let outcome = run_composite_begin_create_generic_fields(
                    &hearth,
                    &artifact_type,
                    std::collections::BTreeMap::new(),
                );
                let mut out = Context::new();
                out.set("composite_begin_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "composite_begin_hearth_handle");
                out.set("begin_outcome", outcome);
                Ok(out)
            },
        ),
        step_def(
            "begin fs composite create is executed for artifact_type {string} with field {string} = {string} and field {string} = {string}",
            &[("composite_begin_hearth", "PathBuf")],
            &[
                ("composite_begin_hearth", "PathBuf"),
                ("composite_begin_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("begin_outcome", "Result<BeginOutcome, BeginError>"),
            ],
            |ctx, params| {
                let artifact_type = params.get_string(0).ok_or("Expected type")?.to_string();
                let mut fields = std::collections::BTreeMap::new();
                fields.insert(
                    params.get_string(1).ok_or("k1")?.to_string(),
                    params.get_string(2).ok_or("v1")?.to_string(),
                );
                fields.insert(
                    params.get_string(3).ok_or("k2")?.to_string(),
                    params.get_string(4).ok_or("v2")?.to_string(),
                );
                let hearth = ctx
                    .get::<PathBuf>("composite_begin_hearth")
                    .ok_or("No composite_begin_hearth")?
                    .clone();
                let outcome =
                    run_composite_begin_create_generic_fields(&hearth, &artifact_type, fields);
                let mut out = Context::new();
                out.set("composite_begin_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "composite_begin_hearth_handle");
                out.set("begin_outcome", outcome);
                Ok(out)
            },
        ),
        step_def(
            "begin fs composite create is executed for artifact_type {string} with field {string} = {string} and field {string} = {string} and field {string} = {string}",
            &[("composite_begin_hearth", "PathBuf")],
            &[
                ("composite_begin_hearth", "PathBuf"),
                ("composite_begin_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("begin_outcome", "Result<BeginOutcome, BeginError>"),
            ],
            |ctx, params| {
                let artifact_type = params.get_string(0).ok_or("Expected type")?.to_string();
                let mut fields = std::collections::BTreeMap::new();
                fields.insert(
                    params.get_string(1).ok_or("k1")?.to_string(),
                    params.get_string(2).ok_or("v1")?.to_string(),
                );
                fields.insert(
                    params.get_string(3).ok_or("k2")?.to_string(),
                    params.get_string(4).ok_or("v2")?.to_string(),
                );
                fields.insert(
                    params.get_string(5).ok_or("k3")?.to_string(),
                    params.get_string(6).ok_or("v3")?.to_string(),
                );
                let hearth = ctx
                    .get::<PathBuf>("composite_begin_hearth")
                    .ok_or("No composite_begin_hearth")?
                    .clone();
                let outcome =
                    run_composite_begin_create_generic_fields(&hearth, &artifact_type, fields);
                let mut out = Context::new();
                out.set("composite_begin_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "composite_begin_hearth_handle");
                out.set("begin_outcome", outcome);
                Ok(out)
            },
        ),
        check_def(
            "the begin outcome emits an ArtifactCreation event with status field {string} = {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected field key")?;
                let expected = params.get_string(1).ok_or("Expected field value")?;
                let outcome = ctx
                    .get::<Result<BeginOutcome, BeginError>>("begin_outcome")
                    .ok_or("No begin_outcome")?;
                match outcome {
                    Ok(o) => {
                        let status = o
                            .events
                            .iter()
                            .find_map(|e| e.artifact_creation_status())
                            .ok_or_else(|| format!("No ArtifactCreation event; events: {:?}", o.events))?;
                        match status.fields.get(key) {
                            Some(v) if v == expected => Ok(()),
                            Some(v) => Err(format!(
                                "ArtifactCreation status field '{}' mismatch: got '{}', expected '{}'",
                                key, v, expected
                            )),
                            None => Err(format!(
                                "ArtifactCreation status has no field '{}'; fields: {:?}",
                                key, status.fields
                            )),
                        }
                    }
                    Err(e) => Err(format!("Expected ArtifactCreation, got error: {:?}", e)),
                }
            },
        ),
        check_def(
            "the begin outcome ArtifactCreation status has no field {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected field key")?;
                let outcome = ctx
                    .get::<Result<BeginOutcome, BeginError>>("begin_outcome")
                    .ok_or("No begin_outcome")?;
                match outcome {
                    Ok(o) => {
                        let status = o
                            .events
                            .iter()
                            .find_map(|e| e.artifact_creation_status())
                            .ok_or_else(|| format!("No ArtifactCreation event; events: {:?}", o.events))?;
                        if status.fields.contains_key(key) {
                            Err(format!(
                                "Expected no field '{}' on status, but found '{:?}'",
                                key,
                                status.fields.get(key)
                            ))
                        } else {
                            Ok(())
                        }
                    }
                    Err(e) => Err(format!("Expected ArtifactCreation, got error: {:?}", e)),
                }
            },
        ),
        check_def(
            "the begin outcome context_text contains {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let outcome = ctx
                    .get::<Result<BeginOutcome, BeginError>>("begin_outcome")
                    .ok_or("No begin_outcome")?;
                match outcome {
                    Ok(o) if o.result.context_text.contains(needle) => Ok(()),
                    Ok(o) => Err(format!(
                        "context_text does not contain '{}'; context_text: {:?}",
                        needle, o.result.context_text
                    )),
                    Err(e) => Err(format!("Expected success, got error: {:?}", e)),
                }
            },
        ),
        check_def(
            "the begin outcome context_text does not contain {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let outcome = ctx
                    .get::<Result<BeginOutcome, BeginError>>("begin_outcome")
                    .ok_or("No begin_outcome")?;
                match outcome {
                    Ok(o) if !o.result.context_text.contains(needle) => Ok(()),
                    Ok(o) => Err(format!(
                        "context_text unexpectedly contains '{}'; context_text: {:?}",
                        needle, o.result.context_text
                    )),
                    Err(e) => Err(format!("Expected success, got error: {:?}", e)),
                }
            },
        ),
    ]
}

/// Seed a temp fs hearth holding an inline machine that declares two generic
/// required fields (`question`, `requester`) outside the builtin set, plus a
/// (drafting, doer) hook body containing `{{question}}` and `{{requester}}`
/// placeholders so context_text interpolation can be proven. parent_kind is ~
/// (no parent), mirroring directory-less run playbooks.
fn seed_generic_fields_required_hearth() -> Result<(RetainedTempDir, PathBuf), String> {
    let (handle, tmp) = retained_temp_dir("anvil-generic-fields-")?;
    std::fs::create_dir_all(&tmp).map_err(|e| format!("Failed to create hearth: {}", e))?;

    std::fs::create_dir_all(tmp.join("tracks"))
        .map_err(|e| format!("Failed to create tracks dir: {}", e))?;
    std::fs::write(tmp.join("tracks.md"), "# Tracks\n")
        .map_err(|e| format!("Failed to write tracks.md: {}", e))?;

    let wf_dir = tmp.join("playbooks").join("20260606T0000_generic_thing");
    std::fs::create_dir_all(wf_dir.join("hooks"))
        .map_err(|e| format!("Failed to create playbook dir: {}", e))?;
    std::fs::write(
        wf_dir.join("machine.yaml"),
        GENERIC_FIELDS_REQUIRED_MACHINE_YAML,
    )
    .map_err(|e| format!("Failed to write machine.yaml: {}", e))?;
    std::fs::write(
        wf_dir.join("hooks").join("drafting.md"),
        "# Drafting\n\nAnswer the question: {{question}}\nRequested by: {{requester}}\n",
    )
    .map_err(|e| format!("Failed to write hook body: {}", e))?;

    Ok((handle, tmp))
}

/// Run a begin create threading a generic `fields` bag into the domain
/// BeginRequest.
fn run_composite_begin_create_generic_fields(
    hearth: &std::path::Path,
    artifact_type: &str,
    fields: std::collections::BTreeMap<String, String>,
) -> Result<BeginOutcome, BeginError> {
    let registry = CompositePlaybookRegistry::new(
        HearthPlaybookRegistry::new(hearth.to_path_buf()),
        SeedPlaybookRegistry,
    );
    let query = FileSystemQueryAdapter::new(hearth.to_path_buf());
    let request = BeginRequest {
        artifact_type: artifact_type.to_string(),
        actor_name: "Test-000000".to_string(),
        actor_type: "agent".to_string(),
        actor_model: "test-model".to_string(),
        actor_provider: "test".to_string(),
        fields,
        ..Default::default()
    };
    BeginCommandHandler::execute(&query, &registry, request)
}

/// An inline machine declaring `question` + `requester` as required fields
/// (outside the builtin set), with no parent kind and a (drafting, doer) hook.
const GENERIC_FIELDS_REQUIRED_MACHINE_YAML: &str = r#"kind: generic_thing
directory: generic_things
registry: generic_things.md
parent_kind: ~
description: "Inline fixture machine that declares generic required fields."
required_fields:
  - name: question
    field_type: string
    description: The natural-language question to answer.
  - name: requester
    field_type: actor_name
    description: Actor that initiated the run.
roles:
  - doer
  - reviewer
states:
  - name: drafting
    role_filters:
      - doer_actionable
    registry_section: "Drafting"
    projection_targets: []
    is_review_gate: false
    is_terminal: false
    hook: drafting.md
  - name: done
    role_filters:
      - terminal
    registry_section: "Done"
    projection_targets: []
    is_review_gate: false
    is_terminal: true
    hook: ~
transitions:
  - from_state: drafting
    to_state: done
    required_role: doer
    requires_approver: false
"#;

/// Seed a temp fs hearth holding an inline machine that declares the
/// PRE-MIGRATION spelling of the playbook-name field, exactly as the live
/// builder machine in anvil-hearth still does. `machine.yaml` is persisted
/// hearth data this repo reads and does not own, so the name it declares has a
/// dedicated home on the request — it is NOT a generic bag field.
fn seed_legacy_name_field_hearth() -> Result<(RetainedTempDir, PathBuf), String> {
    let (handle, tmp) = retained_temp_dir("anvil-legacy-name-")?;
    std::fs::create_dir_all(&tmp).map_err(|e| format!("Failed to create hearth: {}", e))?;

    std::fs::create_dir_all(tmp.join("tracks"))
        .map_err(|e| format!("Failed to create tracks dir: {}", e))?;
    std::fs::write(tmp.join("tracks.md"), "# Tracks\n")
        .map_err(|e| format!("Failed to write tracks.md: {}", e))?;

    let dir = tmp.join("playbooks").join("20260606T0000_legacy_named");
    std::fs::create_dir_all(dir.join("hooks"))
        .map_err(|e| format!("Failed to create playbook dir: {}", e))?;
    std::fs::write(dir.join("machine.yaml"), LEGACY_NAME_FIELD_MACHINE_YAML)
        .map_err(|e| format!("Failed to write machine.yaml: {}", e))?;
    std::fs::write(
        dir.join("hooks").join("drafting.md"),
        "# Drafting\n\nDefine the playbook named {{workflow_name}}.\n",
    )
    .map_err(|e| format!("Failed to write hook body: {}", e))?;

    Ok((handle, tmp))
}

/// An inline machine declaring the pre-migration name spelling as its only
/// required field, with a hook body interpolating the matching placeholder.
const LEGACY_NAME_FIELD_MACHINE_YAML: &str = r#"kind: legacy_named
directory: legacy_nameds
registry: legacy_nameds.md
parent_kind: ~
description: "Inline fixture machine declaring the pre-migration name field."
required_fields:
  - name: workflow_name
    field_type: string
    description: The name of the playbook being defined.
roles:
  - doer
  - reviewer
states:
  - name: drafting
    role_filters:
      - doer_actionable
    registry_section: "Drafting"
    projection_targets: []
    is_review_gate: false
    is_terminal: false
    hook: drafting.md
  - name: done
    role_filters:
      - terminal
    registry_section: "Done"
    projection_targets: []
    is_review_gate: false
    is_terminal: true
    hook: ~
transitions:
  - from_state: drafting
    to_state: done
    required_role: doer
    requires_approver: false
"#;

/// Run a begin create supplying the dedicated playbook-name request field (and
/// optionally a generic bag), against the legacy-name fixture hearth.
fn run_composite_begin_create_named(
    hearth: &std::path::Path,
    artifact_type: &str,
    playbook_name: &str,
    fields: std::collections::BTreeMap<String, String>,
) -> Result<BeginOutcome, BeginError> {
    let registry = CompositePlaybookRegistry::new(
        HearthPlaybookRegistry::new(hearth.to_path_buf()),
        SeedPlaybookRegistry,
    );
    let query = FileSystemQueryAdapter::new(hearth.to_path_buf());
    let request = BeginRequest {
        artifact_type: artifact_type.to_string(),
        actor_name: "Test-000000".to_string(),
        actor_type: "agent".to_string(),
        actor_model: "test-model".to_string(),
        actor_provider: "test".to_string(),
        playbook_name: playbook_name.to_string(),
        fields,
        ..Default::default()
    };
    BeginCommandHandler::execute(&query, &registry, request)
}

/// Seed a temp fs hearth that physically holds an inline machine declaring a
/// `target_owner` required descriptor (the honest-Red fixture for the
/// machine-driven required check — independent of the real builder machine.yaml).
/// Also seeds an active parent track so the parent_kind: track check passes.
fn seed_target_owner_required_hearth(parent: &str) -> Result<(RetainedTempDir, PathBuf), String> {
    let (handle, tmp) = retained_temp_dir("anvil-target-owner-")?;
    std::fs::create_dir_all(&tmp).map_err(|e| format!("Failed to create hearth: {}", e))?;

    std::fs::create_dir_all(tmp.join("tracks"))
        .map_err(|e| format!("Failed to create tracks dir: {}", e))?;
    std::fs::write(tmp.join("tracks.md"), "# Tracks\n")
        .map_err(|e| format!("Failed to write tracks.md: {}", e))?;

    let wf_dir = tmp.join("playbooks").join("20260606T0000_owned_thing");
    std::fs::create_dir_all(&wf_dir)
        .map_err(|e| format!("Failed to create playbook dir: {}", e))?;
    std::fs::write(
        wf_dir.join("machine.yaml"),
        TARGET_OWNER_REQUIRED_MACHINE_YAML,
    )
    .map_err(|e| format!("Failed to write machine.yaml: {}", e))?;

    // Active parent track (parent_kind: track on the inline machine).
    let parent_dir = tmp.join("tracks").join(parent);
    std::fs::create_dir_all(&parent_dir)
        .map_err(|e| format!("Failed to create parent dir: {}", e))?;
    std::fs::write(
        parent_dir.join("status.yaml"),
        "version: 1\nkind: track\nstate: active\n",
    )
    .map_err(|e| format!("Failed to write parent status.yaml: {}", e))?;

    Ok((handle, tmp))
}

/// Run a begin create threading a `target_owner` into the domain BeginRequest.
fn run_composite_begin_create_target_owner(
    hearth: &std::path::Path,
    artifact_type: &str,
    parent: &str,
    target_owner: &str,
) -> Result<BeginOutcome, BeginError> {
    let registry = CompositePlaybookRegistry::new(
        HearthPlaybookRegistry::new(hearth.to_path_buf()),
        SeedPlaybookRegistry,
    );
    let query = FileSystemQueryAdapter::new(hearth.to_path_buf());
    let request = BeginRequest {
        artifact_type: artifact_type.to_string(),
        parent_id: parent.to_string(),
        track_name: "test topic".to_string(),
        approver: "mark".to_string(),
        actor_name: "Test-000000".to_string(),
        actor_type: "agent".to_string(),
        actor_model: "test-model".to_string(),
        actor_provider: "test".to_string(),
        target_owner: target_owner.to_string(),
        ..Default::default()
    };
    BeginCommandHandler::execute(&query, &registry, request)
}

/// An inline machine declaring `target_owner` as a required field, with a
/// `track` parent kind. Used to capture the honest Red for the machine-driven
/// required check without depending on the real builder machine.yaml.
const TARGET_OWNER_REQUIRED_MACHINE_YAML: &str = r#"kind: owned_thing
directory: owned_things
registry: owned_things.md
parent_kind: track
description: "Inline fixture machine that declares target_owner required."
required_fields:
  - name: target_owner
    field_type: owner_descriptor
    description: Owner-descriptor the artifact is destined for.
roles:
  - doer
  - reviewer
states:
  - name: drafting
    role_filters:
      - doer_actionable
    registry_section: "Drafting"
    projection_targets: []
    is_review_gate: false
    is_terminal: false
    hook: ~
  - name: done
    role_filters:
      - terminal
    registry_section: "Done"
    projection_targets: []
    is_review_gate: false
    is_terminal: true
    hook: ~
transitions:
  - from_state: drafting
    to_state: done
    required_role: doer
    requires_approver: false
"#;

/// Seed a temp fs hearth that physically holds the knowledge_lifecycle
/// machine.yaml under workflows/<id>/ (the only way knowledge_lifecycle
/// resolves — via HearthPlaybookRegistry's filesystem scan). When `parent` is
/// Some, also seed an active proposal under proposals/<parent>/ plus the legacy
/// tracks/ + tracks.md hearth signature so a track create can pass parent checks.
fn seed_knowledge_lifecycle_hearth(
    parent: Option<&str>,
) -> Result<(RetainedTempDir, PathBuf), String> {
    let (handle, tmp) = retained_temp_dir("anvil-composite-begin-")?;
    std::fs::create_dir_all(&tmp).map_err(|e| format!("Failed to create hearth: {}", e))?;

    // Structural hearth signature.
    std::fs::create_dir_all(tmp.join("tracks"))
        .map_err(|e| format!("Failed to create tracks dir: {}", e))?;
    std::fs::write(tmp.join("tracks.md"), "# Tracks\n")
        .map_err(|e| format!("Failed to write tracks.md: {}", e))?;

    // The knowledge_lifecycle playbook on disk.
    let wf_dir = tmp
        .join("playbooks")
        .join("20260529T0409_knowledge_lifecycle");
    std::fs::create_dir_all(&wf_dir)
        .map_err(|e| format!("Failed to create playbook dir: {}", e))?;
    std::fs::write(
        wf_dir.join("machine.yaml"),
        KNOWLEDGE_LIFECYCLE_MACHINE_YAML,
    )
    .map_err(|e| format!("Failed to write machine.yaml: {}", e))?;

    if let Some(parent_id) = parent {
        let parent_dir = tmp.join("proposals").join(parent_id);
        std::fs::create_dir_all(&parent_dir)
            .map_err(|e| format!("Failed to create parent dir: {}", e))?;
        std::fs::write(
            parent_dir.join("status.yaml"),
            "version: 1\nkind: proposal\nstate: active\n",
        )
        .map_err(|e| format!("Failed to write parent status.yaml: {}", e))?;

        // The seed track machine declares a (spec, doer) hook = spec-writing.md
        // which the create flow reads from disk via the conventional playbook id.
        // Seed it so the track create behavior-preservation scenario resolves it.
        let track_wf_hooks = tmp
            .join("playbooks")
            .join("20260422T0000_track_lifecycle")
            .join("hooks");
        std::fs::create_dir_all(&track_wf_hooks)
            .map_err(|e| format!("Failed to create track hooks dir: {}", e))?;
        std::fs::write(
            track_wf_hooks.join("spec-writing.md"),
            "# Spec writing guidance\n",
        )
        .map_err(|e| format!("Failed to write spec-writing.md: {}", e))?;
    }
    Ok((handle, tmp))
}

/// Run a begin create against a CompositePlaybookRegistry built fresh over the
/// seeded hearth (HearthPlaybookRegistry + SeedPlaybookRegistry) and the real
/// FileSystemQueryAdapter.
fn run_composite_begin_create(
    hearth: &std::path::Path,
    artifact_type: &str,
    parent: &str,
) -> Result<BeginOutcome, BeginError> {
    run_composite_begin_create_with_conversation(hearth, artifact_type, parent, "")
}

fn run_composite_begin_create_with_conversation(
    hearth: &std::path::Path,
    artifact_type: &str,
    parent: &str,
    conversation_id: &str,
) -> Result<BeginOutcome, BeginError> {
    let registry = CompositePlaybookRegistry::new(
        HearthPlaybookRegistry::new(hearth.to_path_buf()),
        SeedPlaybookRegistry,
    );
    let query = FileSystemQueryAdapter::new(hearth.to_path_buf());
    let request = BeginRequest {
        artifact_type: artifact_type.to_string(),
        parent_id: parent.to_string(),
        track_name: "test topic".to_string(),
        approver: "mark".to_string(),
        actor_name: "Test-000000".to_string(),
        actor_type: "agent".to_string(),
        actor_model: "test-model".to_string(),
        actor_provider: "test".to_string(),
        conversation_id: conversation_id.to_string(),
        ..Default::default()
    };
    BeginCommandHandler::execute(&query, &registry, request)
}

/// Shared accessor so other step modules (e.g. engine.rs) can seed the same
/// headline machine.yaml into their fixtures.
pub fn knowledge_lifecycle_machine_yaml() -> &'static str {
    KNOWLEDGE_LIFECYCLE_MACHINE_YAML
}

/// The headline knowledge_lifecycle machine, mirrored from
/// anvil-hearth/workflows/20260529T0409_knowledge_lifecycle/machine.yaml so the
/// brine fixtures resolve it without depending on the developer hearth.
const KNOWLEDGE_LIFECYCLE_MACHINE_YAML: &str = r#"kind: knowledge_lifecycle
directory: knowledge
registry: knowledge.md
parent_kind: ~
description: "Governs domain knowledge from ingestion through compilation, validation, and publication."
required_fields: []
roles:
  - doer
  - reviewer
  - ingest
  - organize
  - compile
  - validate
  - publish
states:
  - name: ingesting
    role_filters:
      - doer_actionable
    registry_section: "Ingesting"
    projection_targets: []
    is_review_gate: false
    is_terminal: false
    hook: ~
  - name: ingest_review
    role_filters:
      - review_pending
    registry_section: "Awaiting Ingest Review"
    projection_targets: []
    is_review_gate: true
    is_terminal: false
    hook: ~
  - name: organizing
    role_filters:
      - doer_actionable
    registry_section: "Organizing"
    projection_targets: []
    is_review_gate: false
    is_terminal: false
    hook: ~
  - name: compiling
    role_filters:
      - doer_actionable
    registry_section: "Compiling"
    projection_targets: []
    is_review_gate: false
    is_terminal: false
    hook: ~
  - name: compile_review
    role_filters:
      - review_pending
    registry_section: "Awaiting Compile Review"
    projection_targets: []
    is_review_gate: true
    is_terminal: false
    hook: ~
  - name: validating
    role_filters:
      - doer_actionable
    registry_section: "Validating"
    projection_targets: []
    is_review_gate: false
    is_terminal: false
    hook: ~
  - name: validation_review
    role_filters:
      - review_pending
    registry_section: "Awaiting Validation Review"
    projection_targets: []
    is_review_gate: true
    is_terminal: false
    hook: ~
  - name: published
    role_filters: []
    registry_section: "Published"
    projection_targets: []
    is_review_gate: false
    is_terminal: false
    hook: ~
  - name: rejected
    role_filters:
      - terminal
    registry_section: "Rejected"
    projection_targets: []
    is_review_gate: false
    is_terminal: true
    hook: ~
  - name: archived
    role_filters:
      - terminal
    registry_section: "Archived"
    projection_targets: []
    is_review_gate: false
    is_terminal: true
    hook: ~
transitions:
  - from_state: ingesting
    to_state: ingest_review
    required_role: ingest
    required_satisfaction: ~
    requires_approver: false
  - from_state: ingest_review
    to_state: organizing
    required_role: reviewer
    required_satisfaction:
      - approved
      - approved_with_notes
    requires_approver: true
  - from_state: ingest_review
    to_state: ingesting
    required_role: reviewer
    required_satisfaction:
      - revision_needed
    requires_approver: false
  - from_state: ingest_review
    to_state: rejected
    required_role: reviewer
    required_satisfaction:
      - rejected
    requires_approver: true
  - from_state: organizing
    to_state: compiling
    required_role: organize
    required_satisfaction: ~
    requires_approver: false
  - from_state: compiling
    to_state: compile_review
    required_role: compile
    required_satisfaction: ~
    requires_approver: false
  - from_state: compile_review
    to_state: validating
    required_role: reviewer
    required_satisfaction:
      - approved
      - approved_with_notes
    requires_approver: true
  - from_state: compile_review
    to_state: compiling
    required_role: reviewer
    required_satisfaction:
      - revision_needed
    requires_approver: false
  - from_state: compile_review
    to_state: rejected
    required_role: reviewer
    required_satisfaction:
      - rejected
    requires_approver: true
  - from_state: validating
    to_state: validation_review
    required_role: validate
    required_satisfaction: ~
    requires_approver: false
  - from_state: validation_review
    to_state: published
    required_role: publish
    required_satisfaction:
      - approved
    requires_approver: true
  - from_state: validation_review
    to_state: compiling
    required_role: reviewer
    required_satisfaction:
      - revision_needed
    requires_approver: false
  - from_state: validation_review
    to_state: rejected
    required_role: reviewer
    required_satisfaction:
      - rejected
    requires_approver: true
  - from_state: published
    to_state: archived
    required_role: reviewer
    required_satisfaction: ~
    requires_approver: true
"#;
