//! Step module for `hook_manifest_fold.feature` (core seam).
//!
//! Drives the pure `fold_hook_manifest` fold over a small in-module
//! `PlaybookRegistry` double (machines + their on-disk source ids) and a
//! `PlaybookHookBodyPort` double (the (playbook_id, filename) -> body map). The
//! body port is the SAME trait `begin` uses, so the fold reads bodies via the
//! exact path that serves them — proving the contract body cannot drift.

use anvil_core::domain::begin::PlaybookHookBodyPort;
use anvil_core::domain::hook_manifest::{fold_hook_manifest, ResolvedHook};
use anvil_core::domain::playbook::registry::{PlaybookRegistry, PlaybookSource};
use anvil_core::domain::playbook::types::{PlaybookMachine, RouteConfig, StateDefinition};
use anvil_core::ports::query_port::QueryError;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::collections::BTreeMap;

const MACHINES_KEY: &str = "hmf_machines";
const BODIES_KEY: &str = "hmf_bodies";
const HARD_ENFORCE_KEY: &str = "hmf_hard_enforce";
const RESULT_KEY: &str = "hmf_result";

/// In-module registry double: each machine's `kind` IS its on-disk source id
/// (playbook_id), mirroring the seed-registry convention where the id equals
/// the kind directory under `workflows/`.
struct FixtureRegistry {
    machines: Vec<PlaybookMachine>,
}

impl PlaybookRegistry for FixtureRegistry {
    fn machine_for<'a>(&'a self, kind: &str) -> Option<&'a PlaybookMachine>
    where
        Self: 'a,
    {
        self.machines.iter().find(|m| m.kind == kind)
    }

    fn playbook_id_for(&self, kind: &str) -> Option<String> {
        self.machine_for(kind).map(|m| m.kind.clone())
    }

    fn all_machines<'a>(&'a self) -> Vec<&'a PlaybookMachine>
    where
        Self: 'a,
    {
        self.machines.iter().collect()
    }
}

/// In-module body port double: looks up bodies by (playbook_id, filename).
struct FixtureBodyReader {
    bodies: BTreeMap<(String, String), String>,
}

impl PlaybookHookBodyPort for FixtureBodyReader {
    fn read_playbook_hook_body(
        &self,
        source: &PlaybookSource,
        filename: &str,
    ) -> Result<String, QueryError> {
        self.bodies
            .get(&(source.playbook_id.clone(), filename.to_string()))
            .cloned()
            .ok_or_else(|| QueryError::IoError {
                message: format!(
                    "Hook file '{}' not found for playbook '{}'",
                    filename, source.playbook_id
                ),
            })
    }
}

fn fixture_machine(kind: &str, states: Vec<StateDefinition>) -> PlaybookMachine {
    PlaybookMachine {
        kind: kind.to_string(),
        directory: format!("{}s", kind),
        registry: format!("{}s.md", kind),
        parent_kind: None,
        description: format!("{} fixture", kind),
        route: RouteConfig::default(),
        required_fields: vec![],
        roles: vec!["doer".to_string(), "reviewer".to_string()],
        states,
        transitions: vec![],
        register: anvil_core::domain::playbook::types::Register::Driven,
        ..Default::default()
    }
}

fn hookless_state(name: &str) -> StateDefinition {
    StateDefinition {
        name: name.to_string(),
        role_filters: vec![],
        registry_section: name.to_string(),
        projection_targets: vec![],
        is_review_gate: false,
        is_terminal: false,
        hook: None,
        hooks_by_role: BTreeMap::new(),
        measurement_by_role: BTreeMap::new(),
    }
}

fn col(table: &brine_core::parser::DataTable, name: &str) -> Result<usize, String> {
    table
        .headers
        .iter()
        .position(|h| h == name)
        .ok_or_else(|| format!("Missing '{}' column", name))
}

/// Build/extend the registry + body map from a playbook hook table.
fn ingest_playbook(
    ctx: &mut Context,
    kind: &str,
    table: &brine_core::parser::DataTable,
) -> Result<(), String> {
    let state_col = col(table, "state")?;
    let role_col = col(table, "role")?;
    let file_col = col(table, "filename")?;
    let body_col = col(table, "body")?;

    let mut machines: Vec<PlaybookMachine> = ctx
        .take::<Vec<PlaybookMachine>>(MACHINES_KEY)
        .unwrap_or_default();
    let mut bodies: BTreeMap<(String, String), String> = ctx
        .take::<BTreeMap<(String, String), String>>(BODIES_KEY)
        .unwrap_or_default();

    // Group rows into states (a state may carry multiple role hooks).
    let mut by_state: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for row in &table.rows {
        let state = row[state_col].trim().to_string();
        let role = row[role_col].trim().to_string();
        let filename = row[file_col].trim().to_string();
        let body = row[body_col].trim().to_string();
        by_state
            .entry(state)
            .or_default()
            .insert(role, filename.clone());
        bodies.insert((kind.to_string(), filename), body);
    }

    let states: Vec<StateDefinition> = by_state
        .into_iter()
        .map(|(name, hooks_by_role)| StateDefinition {
            name: name.clone(),
            role_filters: vec![],
            registry_section: name,
            projection_targets: vec![],
            is_review_gate: false,
            is_terminal: false,
            hook: None,
            hooks_by_role,
            measurement_by_role: BTreeMap::new(),
        })
        .collect();

    machines.push(fixture_machine(kind, states));

    ctx.set(MACHINES_KEY, machines);
    ctx.set(BODIES_KEY, bodies);
    Ok(())
}

fn carry(ctx: &mut Context, out: &mut Context) {
    if let Some(m) = ctx.take::<Vec<PlaybookMachine>>(MACHINES_KEY) {
        out.set(MACHINES_KEY, m);
    }
    if let Some(b) = ctx.take::<BTreeMap<(String, String), String>>(BODIES_KEY) {
        out.set(BODIES_KEY, b);
    }
    if let Some(h) = ctx.take::<Vec<String>>(HARD_ENFORCE_KEY) {
        out.set(HARD_ENFORCE_KEY, h);
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a hook manifest registry with playbook {string} hooks:",
            &[],
            &[
                (MACHINES_KEY, "Vec<PlaybookMachine>"),
                (BODIES_KEY, "BTreeMap"),
            ],
            |mut ctx, params| {
                let kind = params.get_string(0).ok_or("Expected playbook kind")?.to_string();
                let table = params.data_table().ok_or("Expected a data table")?;
                ingest_playbook(&mut ctx, &kind, table)?;
                let mut out = Context::new();
                carry(&mut ctx, &mut out);
                Ok(out)
            },
        ),
        step_def(
            "the hook manifest registry has a hookless state {string} on playbook {string}",
            &[(MACHINES_KEY, "Vec<PlaybookMachine>")],
            &[(MACHINES_KEY, "Vec<PlaybookMachine>"), (BODIES_KEY, "BTreeMap")],
            |mut ctx, params| {
                let state = params.get_string(0).ok_or("Expected state")?.to_string();
                let kind = params.get_string(1).ok_or("Expected playbook")?.to_string();
                let mut machines: Vec<PlaybookMachine> = ctx
                    .take::<Vec<PlaybookMachine>>(MACHINES_KEY)
                    .ok_or("No machines in context")?;
                let machine = machines
                    .iter_mut()
                    .find(|m| m.kind == kind)
                    .ok_or_else(|| format!("No machine for kind '{}'", kind))?;
                machine.states.push(hookless_state(&state));
                ctx.set(MACHINES_KEY, machines);
                let mut out = Context::new();
                carry(&mut ctx, &mut out);
                Ok(out)
            },
        ),
        step_def(
            "the hard_enforce policy is {string}",
            &[],
            &[
                (HARD_ENFORCE_KEY, "Vec<String>"),
                (MACHINES_KEY, "Vec<PlaybookMachine>"),
                (BODIES_KEY, "BTreeMap"),
            ],
            |mut ctx, params| {
                let raw = params.get_string(0).unwrap_or_default();
                let hard: Vec<String> = raw
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                let mut out = Context::new();
                out.set(HARD_ENFORCE_KEY, hard);
                carry(&mut ctx, &mut out);
                Ok(out)
            },
        ),
        step_def(
            "fold_hook_manifest is computed",
            &[
                (MACHINES_KEY, "Vec<PlaybookMachine>"),
                (BODIES_KEY, "BTreeMap"),
            ],
            &[(RESULT_KEY, "Vec<ResolvedHook>")],
            |mut ctx, _params| {
                let machines: Vec<PlaybookMachine> = ctx
                    .take::<Vec<PlaybookMachine>>(MACHINES_KEY)
                    .ok_or("No machines in context")?;
                let bodies: BTreeMap<(String, String), String> = ctx
                    .take::<BTreeMap<(String, String), String>>(BODIES_KEY)
                    .unwrap_or_default();
                let hard: Vec<String> =
                    ctx.take::<Vec<String>>(HARD_ENFORCE_KEY).unwrap_or_default();

                let registry = FixtureRegistry { machines };
                let reader = FixtureBodyReader { bodies };
                let result = fold_hook_manifest(&registry, &reader, &hard);

                let mut out = Context::new();
                out.set(RESULT_KEY, result);
                Ok(out)
            },
        ),
        check_def(
            "the hook manifest has {int} hooks",
            &[(RESULT_KEY, "Vec<ResolvedHook>")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let result = ctx
                    .get::<Vec<ResolvedHook>>(RESULT_KEY)
                    .ok_or("No fold result")?;
                if result.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} hooks, got {}: {:?}",
                        expected,
                        result.len(),
                        result
                            .iter()
                            .map(|h| format!("{}:{}:{}", h.artifact_kind, h.state, h.role))
                            .collect::<Vec<_>>()
                    ))
                }
            },
        ),
        check_def(
            "the hook manifest hook {int} is artifact_kind {string} state {string} role {string} gate {string}",
            &[(RESULT_KEY, "Vec<ResolvedHook>")],
            |ctx, params| {
                let idx = params.get_int(0).ok_or("Expected index")? as usize;
                let kind = params.get_string(1).ok_or("Expected kind")?;
                let state = params.get_string(2).ok_or("Expected state")?;
                let role = params.get_string(3).ok_or("Expected role")?;
                let gate = params.get_string(4).ok_or("Expected gate")?;
                let result = ctx
                    .get::<Vec<ResolvedHook>>(RESULT_KEY)
                    .ok_or("No fold result")?;
                let hook = result
                    .get(idx)
                    .ok_or_else(|| format!("No hook at index {}", idx))?;
                if hook.artifact_kind == kind
                    && hook.state == state
                    && hook.role == role
                    && hook.gate == gate
                {
                    Ok(())
                } else {
                    Err(format!(
                        "hook {}: expected ({}, {}, {}, {}), got ({}, {}, {}, {})",
                        idx,
                        kind,
                        state,
                        role,
                        gate,
                        hook.artifact_kind,
                        hook.state,
                        hook.role,
                        hook.gate
                    ))
                }
            },
        ),
        check_def(
            "the hook manifest hook {int} body contains {string}",
            &[(RESULT_KEY, "Vec<ResolvedHook>")],
            |ctx, params| {
                let idx = params.get_int(0).ok_or("Expected index")? as usize;
                let needle = params.get_string(1).ok_or("Expected substring")?;
                let result = ctx
                    .get::<Vec<ResolvedHook>>(RESULT_KEY)
                    .ok_or("No fold result")?;
                let hook = result
                    .get(idx)
                    .ok_or_else(|| format!("No hook at index {}", idx))?;
                if hook.body.contains(needle) {
                    Ok(())
                } else {
                    Err(format!(
                        "hook {} body does not contain '{}': {:?}",
                        idx, needle, hook.body
                    ))
                }
            },
        ),
    ]
}
