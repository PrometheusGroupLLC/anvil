//! Step module for `route_handler.feature` (playbook_routing_layer BP2 core seam).
//!
//! Exercises the pure `RouteQueryHandler` over a real in-memory
//! `PlaybookRegistry` (no mocks — a concrete registry with a fixed, ordered set
//! of machines). Proves the typed no_match outcome (AC3-v0) against an empty
//! driven set, the candidate set (AC1), and free exclusion (AC4).

use anvil_core::domain::begin::BeginRequest;
use anvil_core::domain::playbook::registry::{granted_driven_candidates, PlaybookRegistry};
use anvil_core::domain::playbook::types::{
    Access, PlaybookMachine, Register, Role, RouteConfig, Sensitivity, StateDefinition,
    TransitionDefinition,
};
use anvil_core::domain::route::{RouteQueryHandler, RouteRequest, RouteResult};
use anvil_core::domain::shared_types::RequestContext;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::collections::HashMap;

const REGISTRY_MACHINES_KEY: &str = "rh_machines";
const ROUTE_RESULT_KEY: &str = "rh_result";
const ROUTE_REQUEST_KEY: &str = "rh_route_request";
const BEGIN_REQUEST_KEY: &str = "rh_begin_request";
const ACCESS_MACHINES_KEY: &str = "rh_access_machines";
const REQUEST_CONTEXT_KEY: &str = "rh_request_context";
const GRANTED_KINDS_KEY: &str = "rh_granted_kinds";

/// A real ordered registry double for the route handler scenarios.
struct VecRegistry {
    order: Vec<String>,
    map: HashMap<String, PlaybookMachine>,
}

impl PlaybookRegistry for VecRegistry {
    fn machine_for<'a>(&'a self, kind: &str) -> Option<&'a PlaybookMachine>
    where
        Self: 'a,
    {
        self.map.get(kind)
    }

    fn playbook_id_for(&self, kind: &str) -> Option<String> {
        self.map.get(kind).map(|_| format!("{}_playbook", kind))
    }

    fn all_machines<'a>(&'a self) -> Vec<&'a PlaybookMachine>
    where
        Self: 'a,
    {
        self.order.iter().filter_map(|k| self.map.get(k)).collect()
    }
}

fn machine(kind: &str, register: Register) -> PlaybookMachine {
    PlaybookMachine {
        kind: kind.to_string(),
        directory: format!("{}s", kind),
        registry: format!("{}s.md", kind),
        parent_kind: None,
        description: format!("Route handler fixture {}", kind),
        route: RouteConfig::default(),
        required_fields: vec![],
        roles: vec!["doer".to_string()],
        states: vec![StateDefinition {
            name: "active".to_string(),
            role_filters: vec![],
            registry_section: "active".to_string(),
            projection_targets: vec![],
            is_review_gate: false,
            is_terminal: false,
            hook: None,
            hooks_by_role: std::collections::BTreeMap::new(),
            measurement_by_role: std::collections::BTreeMap::new(),
        }],
        transitions: vec![TransitionDefinition {
            from_state: "active".to_string(),
            to_state: "active".to_string(),
            required_role: "doer".to_string(),
            required_satisfaction: None,
            requires_approver: false,
            hook: None,
        }],
        register,
        ..Default::default()
    }
}

fn machine_with_access(kind: &str, access: Access) -> PlaybookMachine {
    PlaybookMachine {
        access,
        ..machine(kind, Register::Driven)
    }
}

/// (kind, register) pairs, encoded so they survive the context as a Vec.
type MachineSpec = Vec<(String, String)>;

#[derive(Debug, Clone)]
struct AccessMachineSpec {
    kind: String,
    access: Access,
}

type AccessMachineSpecs = Vec<AccessMachineSpec>;

fn build_registry(spec: &MachineSpec) -> VecRegistry {
    let mut order = Vec::new();
    let mut map = HashMap::new();
    for (kind, reg) in spec {
        let register = if reg == "free" {
            Register::Free
        } else {
            Register::Driven
        };
        order.push(kind.clone());
        map.insert(kind.clone(), machine(kind, register));
    }
    VecRegistry { order, map }
}

fn build_access_registry(spec: &AccessMachineSpecs) -> VecRegistry {
    let mut order = Vec::new();
    let mut map = HashMap::new();
    for entry in spec {
        order.push(entry.kind.clone());
        map.insert(
            entry.kind.clone(),
            machine_with_access(&entry.kind, entry.access.clone()),
        );
    }
    VecRegistry { order, map }
}

fn parse_role(value: &str) -> Result<Role, String> {
    match value {
        "read" => Ok(Role::Read),
        "write" => Ok(Role::Write),
        "admin" => Ok(Role::Admin),
        other => Err(format!("Unknown role '{}'", other)),
    }
}

fn parse_sensitivity(value: &str) -> Result<Sensitivity, String> {
    match value {
        "public" => Ok(Sensitivity::Public),
        "internal" => Ok(Sensitivity::Internal),
        "confidential" => Ok(Sensitivity::Confidential),
        "phi" => Ok(Sensitivity::Phi),
        other => Err(format!("Unknown sensitivity '{}'", other)),
    }
}

fn parse_space(value: &str) -> Option<String> {
    match value {
        "" | "~" | "none" => None,
        other => Some(other.to_string()),
    }
}

fn parse_org(value: &str) -> String {
    if value == "empty" {
        String::new()
    } else {
        value.to_string()
    }
}

fn ctx_matches(
    ctx: &RequestContext,
    org: &str,
    role: &str,
    clearance: &str,
    space: &str,
) -> Result<(), String> {
    let expected = RequestContext {
        org: org.to_string(),
        role: parse_role(role)?,
        clearance: parse_sensitivity(clearance)?,
        space: parse_space(space),
    };
    if ctx == &expected {
        Ok(())
    } else {
        Err(format!("Expected ctx {:?}, got {:?}", expected, ctx))
    }
}

fn check_granted_kind(
    ctx: &Context,
    params: &brine_runner_rust::registry::Params,
    present: bool,
) -> Result<(), String> {
    let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
    let kinds = ctx
        .get::<Vec<String>>(GRANTED_KINDS_KEY)
        .ok_or("No granted kinds")?;
    let actual = kinds.iter().any(|candidate| candidate == &kind);
    match (present, actual) {
        (true, true) | (false, false) => Ok(()),
        (true, false) => Err(format!("Expected '{}' in granted kinds {:?}", kind, kinds)),
        (false, true) => Err(format!(
            "Expected '{}' absent from granted kinds {:?}",
            kind, kinds
        )),
    }
}

fn carry_access_machines(ctx: &Context, mut out: Context) -> Context {
    if let Some(specs) = ctx.get::<AccessMachineSpecs>(ACCESS_MACHINES_KEY) {
        out.set(ACCESS_MACHINES_KEY, specs.clone());
    }
    out
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a route registry with no driven machines",
            &[],
            &[(REGISTRY_MACHINES_KEY, "MachineSpec")],
            |_ctx, _params| {
                let spec: MachineSpec = vec![];
                let mut out = Context::new();
                out.set(REGISTRY_MACHINES_KEY, spec);
                Ok(out)
            },
        ),
        step_def(
            "a route registry with a driven {string} machine",
            &[],
            &[(REGISTRY_MACHINES_KEY, "MachineSpec")],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let spec: MachineSpec = vec![(kind, "driven".to_string())];
                let mut out = Context::new();
                out.set(REGISTRY_MACHINES_KEY, spec);
                Ok(out)
            },
        ),
        step_def(
            "a route registry with a driven {string} machine and a free {string} machine",
            &[],
            &[(REGISTRY_MACHINES_KEY, "MachineSpec")],
            |_ctx, params| {
                let driven = params.get_string(0).ok_or("Expected driven kind")?.to_string();
                let free = params.get_string(1).ok_or("Expected free kind")?.to_string();
                let spec: MachineSpec =
                    vec![(driven, "driven".to_string()), (free, "free".to_string())];
                let mut out = Context::new();
                out.set(REGISTRY_MACHINES_KEY, spec);
                Ok(out)
            },
        ),
        step_def(
            "the route handler runs with message {string}",
            &[(REGISTRY_MACHINES_KEY, "MachineSpec")],
            &[(ROUTE_RESULT_KEY, "RouteResult")],
            |ctx, params| {
                let message = params.get_string(0).ok_or("Expected message")?.to_string();
                let spec = ctx
                    .get::<MachineSpec>(REGISTRY_MACHINES_KEY)
                    .ok_or("No registry machines")?
                    .clone();
                let registry = build_registry(&spec);
                let request = RouteRequest {
                    message,
                    signal: String::new(),
                    ctx: RequestContext::default_safe(),
                    conversation_id: String::new(),
                };
                let result = RouteQueryHandler::execute(&registry, &request);
                let mut out = Context::new();
                out.set(ROUTE_RESULT_KEY, result);
                Ok(out)
            },
        ),
        step_def(
            "domain route and begin requests are built with org {string} role {string} clearance {string} space {string}",
            &[],
            &[
                (ROUTE_REQUEST_KEY, "RouteRequest"),
                (BEGIN_REQUEST_KEY, "BeginRequest"),
            ],
            |_ctx, params| {
                let org = params.get_string(0).ok_or("Expected org")?.to_string();
                let role = params.get_string(1).ok_or("Expected role")?.to_string();
                let clearance = params.get_string(2).ok_or("Expected clearance")?.to_string();
                let space = params.get_string(3).ok_or("Expected space")?.to_string();
                let ctx_value = RequestContext {
                    org,
                    role: parse_role(&role)?,
                    clearance: parse_sensitivity(&clearance)?,
                    space: parse_space(&space),
                };
                let route_request = RouteRequest {
                    message: "ctx round trip".to_string(),
                    signal: String::new(),
                    ctx: ctx_value.clone(),
                    conversation_id: String::new(),
                };
                let begin_request = BeginRequest {
                    ctx: ctx_value,
                    ..Default::default()
                };
                let mut out = Context::new();
                out.set(ROUTE_REQUEST_KEY, route_request);
                out.set(BEGIN_REQUEST_KEY, begin_request);
                Ok(out)
            },
        ),
        step_def(
            "domain route and begin requests are built without ctx",
            &[],
            &[
                (ROUTE_REQUEST_KEY, "RouteRequest"),
                (BEGIN_REQUEST_KEY, "BeginRequest"),
            ],
            |_ctx, _params| {
                let route_request = RouteRequest {
                    message: "default ctx".to_string(),
                    signal: String::new(),
                    ..Default::default()
                };
                let begin_request = BeginRequest::default();
                let mut out = Context::new();
                out.set(ROUTE_REQUEST_KEY, route_request);
                out.set(BEGIN_REQUEST_KEY, begin_request);
                Ok(out)
            },
        ),
        step_def(
            "a driven access registry with machines:",
            &[],
            &[(ACCESS_MACHINES_KEY, "AccessMachineSpecs")],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                if table.headers != ["kind", "org", "min_role", "sensitivity", "space"] {
                    return Err(format!("Unexpected headers: {:?}", table.headers));
                }
                let mut specs = Vec::new();
                for row in &table.rows {
                    if row.len() != 5 {
                        return Err(format!("Expected 5 columns, got {:?}", row));
                    }
                    specs.push(AccessMachineSpec {
                        kind: row[0].trim().to_string(),
                        access: Access {
                            org: parse_org(row[1].trim()),
                            min_role: parse_role(row[2].trim())?,
                            sensitivity: parse_sensitivity(row[3].trim())?,
                            space: parse_space(row[4].trim()),
                        },
                    });
                }
                let mut out = Context::new();
                out.set(ACCESS_MACHINES_KEY, specs);
                Ok(out)
            },
        ),
        step_def(
            "a request context with org {string} role {string} clearance {string} space {string}",
            &[],
            &[
                (REQUEST_CONTEXT_KEY, "RequestContext"),
                (ROUTE_REQUEST_KEY, "RouteRequest"),
                (BEGIN_REQUEST_KEY, "BeginRequest"),
                (ACCESS_MACHINES_KEY, "AccessMachineSpecs"),
            ],
            |_ctx, params| {
                let org = params.get_string(0).ok_or("Expected org")?.to_string();
                let role = params.get_string(1).ok_or("Expected role")?.to_string();
                let clearance = params.get_string(2).ok_or("Expected clearance")?.to_string();
                let space = params.get_string(3).ok_or("Expected space")?.to_string();
                let ctx_value = RequestContext {
                    org,
                    role: parse_role(&role)?,
                    clearance: parse_sensitivity(&clearance)?,
                    space: parse_space(&space),
                };
                let route_request = RouteRequest {
                    message: "ctx round trip".to_string(),
                    signal: String::new(),
                    ctx: ctx_value.clone(),
                    conversation_id: String::new(),
                };
                let begin_request = BeginRequest {
                    ctx: ctx_value.clone(),
                    ..Default::default()
                };
                let mut out = carry_access_machines(&_ctx, Context::new());
                out.set(REQUEST_CONTEXT_KEY, ctx_value);
                out.set(ROUTE_REQUEST_KEY, route_request);
                out.set(BEGIN_REQUEST_KEY, begin_request);
                Ok(out)
            },
        ),
        step_def(
            "granted driven candidates are selected",
            &[
                (ACCESS_MACHINES_KEY, "AccessMachineSpecs"),
                (REQUEST_CONTEXT_KEY, "RequestContext"),
            ],
            &[(GRANTED_KINDS_KEY, "Vec<String>")],
            |ctx, _params| {
                let specs = ctx
                    .get::<AccessMachineSpecs>(ACCESS_MACHINES_KEY)
                    .ok_or("No access machine specs")?
                    .clone();
                let request_ctx = ctx
                    .get::<RequestContext>(REQUEST_CONTEXT_KEY)
                    .ok_or("No request context")?;
                let registry = build_access_registry(&specs);
                let kinds: Vec<String> = granted_driven_candidates(&registry, request_ctx)
                    .into_iter()
                    .map(|m| m.kind.clone())
                    .collect();
                let mut out = Context::new();
                out.set(GRANTED_KINDS_KEY, kinds);
                Ok(out)
            },
        ),
        check_def(
            "the route result is no_match",
            &[(ROUTE_RESULT_KEY, "RouteResult")],
            |ctx, _params| {
                let result = ctx.get::<RouteResult>(ROUTE_RESULT_KEY).ok_or("No result")?;
                match result {
                    RouteResult::NoMatch { .. } => Ok(()),
                    RouteResult::Candidates(_) => Err("Expected no_match, got candidates".to_string()),
                }
            },
        ),
        check_def(
            "the route result is candidates",
            &[(ROUTE_RESULT_KEY, "RouteResult")],
            |ctx, _params| {
                let result = ctx.get::<RouteResult>(ROUTE_RESULT_KEY).ok_or("No result")?;
                match result {
                    RouteResult::Candidates(_) => Ok(()),
                    RouteResult::NoMatch { .. } => Err("Expected candidates, got no_match".to_string()),
                }
            },
        ),
        check_def(
            "the no_match handoff is {string}",
            &[(ROUTE_RESULT_KEY, "RouteResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected handoff")?;
                let result = ctx.get::<RouteResult>(ROUTE_RESULT_KEY).ok_or("No result")?;
                match result {
                    RouteResult::NoMatch { handoff, .. } if handoff == expected.as_ref() as &str => {
                        Ok(())
                    }
                    RouteResult::NoMatch { handoff, .. } => {
                        Err(format!("Expected handoff '{}', got '{}'", expected, handoff))
                    }
                    RouteResult::Candidates(_) => Err("Expected no_match".to_string()),
                }
            },
        ),
        check_def(
            "the no_match intent echoes {string}",
            &[(ROUTE_RESULT_KEY, "RouteResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected intent")?;
                let result = ctx.get::<RouteResult>(ROUTE_RESULT_KEY).ok_or("No result")?;
                match result {
                    RouteResult::NoMatch { intent, .. } if intent == expected.as_ref() as &str => {
                        Ok(())
                    }
                    RouteResult::NoMatch { intent, .. } => {
                        Err(format!("Expected intent '{}', got '{}'", expected, intent))
                    }
                    RouteResult::Candidates(_) => Err("Expected no_match".to_string()),
                }
            },
        ),
        check_def(
            "the candidate set includes kind {string}",
            &[(ROUTE_RESULT_KEY, "RouteResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let result = ctx.get::<RouteResult>(ROUTE_RESULT_KEY).ok_or("No result")?;
                match result {
                    RouteResult::Candidates(cands) => {
                        if cands.iter().any(|c| c.kind == kind) {
                            Ok(())
                        } else {
                            Err(format!("Expected '{}' in candidates", kind))
                        }
                    }
                    RouteResult::NoMatch { .. } => Err("Expected candidates".to_string()),
                }
            },
        ),
        check_def(
            "the candidate set excludes kind {string}",
            &[(ROUTE_RESULT_KEY, "RouteResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let result = ctx.get::<RouteResult>(ROUTE_RESULT_KEY).ok_or("No result")?;
                match result {
                    RouteResult::Candidates(cands) => {
                        if cands.iter().any(|c| c.kind == kind) {
                            Err(format!("Expected '{}' absent from candidates", kind))
                        } else {
                            Ok(())
                        }
                    }
                    RouteResult::NoMatch { .. } => Err("Expected candidates".to_string()),
                }
            },
        ),
        check_def(
            "the domain route request ctx is org {string} role {string} clearance {string} space {string}",
            &[(ROUTE_REQUEST_KEY, "RouteRequest")],
            |ctx, params| {
                let route_request = ctx.get::<RouteRequest>(ROUTE_REQUEST_KEY).ok_or("No route request")?;
                ctx_matches(
                    &route_request.ctx,
                    params.get_string(0).ok_or("Expected org")?,
                    params.get_string(1).ok_or("Expected role")?,
                    params.get_string(2).ok_or("Expected clearance")?,
                    params.get_string(3).ok_or("Expected space")?,
                )
            },
        ),
        check_def(
            "the domain begin request ctx is org {string} role {string} clearance {string} space {string}",
            &[(BEGIN_REQUEST_KEY, "BeginRequest")],
            |ctx, params| {
                let begin_request = ctx.get::<BeginRequest>(BEGIN_REQUEST_KEY).ok_or("No begin request")?;
                ctx_matches(
                    &begin_request.ctx,
                    params.get_string(0).ok_or("Expected org")?,
                    params.get_string(1).ok_or("Expected role")?,
                    params.get_string(2).ok_or("Expected clearance")?,
                    params.get_string(3).ok_or("Expected space")?,
                )
            },
        ),
        check_def(
            "the granted kinds include kind {string}",
            &[(GRANTED_KINDS_KEY, "Vec<String>")],
            |ctx, params| check_granted_kind(&ctx, params, true),
        ),
        check_def(
            "the granted kinds exclude kind {string}",
            &[(GRANTED_KINDS_KEY, "Vec<String>")],
            |ctx, params| check_granted_kind(&ctx, params, false),
        ),
        check_def(
            "the granted candidate kinds are exactly:",
            &[(GRANTED_KINDS_KEY, "Vec<String>")],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                if table.headers != ["kind"] {
                    return Err(format!("Unexpected headers: {:?}", table.headers));
                }
                let expected: Vec<String> = table
                    .rows
                    .iter()
                    .map(|row| row.first().map(|s| s.trim().to_string()).unwrap_or_default())
                    .collect();
                let actual = ctx.get::<Vec<String>>(GRANTED_KINDS_KEY).ok_or("No granted kinds")?;
                if actual == &expected {
                    Ok(())
                } else {
                    Err(format!("Expected granted kinds {:?}, got {:?}", expected, actual))
                }
            },
        ),
    ]
}
