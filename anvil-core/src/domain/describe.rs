use crate::domain::available_artifact_types;
use crate::domain::playbook::interpreter;
use crate::domain::playbook::registry::PlaybookRegistry;
use crate::domain::routing::{compute_execution_route_with_registry, SUBJECT_AVAILABLE_ACTION};
use crate::ports::describe_port::DescribePort;
use std::fmt;

/// Request to describe an artifact type or instance.
#[derive(Debug, Clone)]
pub struct DescribeRequest {
    pub identifier: String,
}

/// Result of a describe query.
#[derive(Debug, Clone)]
pub enum DescribeResult {
    TypeInfo {
        name: String,
        description: String,
        required_fields: Vec<String>,
        parent_type: String,
    },
    InstanceInfo {
        id: String,
        artifact_type: String,
        state: String,
        transition_count: usize,
        last_transition: Option<TransitionInfo>,
        available_actions: Vec<AvailableAction>,
    },
}

/// Summary of a single transition.
#[derive(Debug, Clone)]
pub struct TransitionInfo {
    pub to: String,
    pub at: String,
    pub actor: String,
    pub role: String,
}

/// An action available from the current state.
#[derive(Debug, Clone)]
pub struct AvailableAction {
    pub action: String,
    pub required_role: String,
    /// "engine" if this action is executed by begin(identifier); otherwise
    /// "fallback:forge:<skill>" naming the forge skill to invoke instead.
    pub execution_route: String,
}

/// Errors from describe operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DescribeError {
    UnknownIdentifier { identifier: String },
    IoError { message: String },
}

impl fmt::Display for DescribeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DescribeError::UnknownIdentifier { identifier } => {
                write!(f, "Unknown type or artifact: '{}'", identifier)
            }
            DescribeError::IoError { message } => {
                write!(f, "I/O error: {}", message)
            }
        }
    }
}

impl std::error::Error for DescribeError {}

/// Known artifact type names for type-level dispatch.
const KNOWN_TYPES: &[&str] = &[
    "proposal",
    "track",
    "milestone",
    "initiative",
    "decision",
    "learning",
    "playbook",
    "backlog_item",
];

/// The artifact type names `describe` recognizes for a type-level query.
pub fn known_types() -> &'static [&'static str] {
    KNOWN_TYPES
}

/// Required fields for creating each artifact type.
pub fn required_fields_for_type(type_name: &str) -> Vec<String> {
    match type_name {
        "track" => vec![
            "name".to_string(),
            "parent_id".to_string(),
            "approver".to_string(),
        ],
        "proposal" => vec!["name".to_string()],
        "milestone" => vec!["name".to_string()],
        "initiative" => vec!["name".to_string()],
        "decision" => vec!["name".to_string()],
        "learning" => vec!["name".to_string()],
        "playbook" => vec![
            "playbook_name".to_string(),
            "parent_id".to_string(),
            "approver".to_string(),
        ],
        // K8 genesis takes exactly one closed field `item` (the genesis input
        // object); the engine mints id/state/history and never accepts them here.
        "backlog_item" => vec!["item".to_string()],
        _ => Vec::new(),
    }
}

/// Available actions from a given state for engine-driven lifecycles.
///
/// The `track` arm is playbook-driven via the `PlaybookRegistry` (Phase 5 cutover):
/// it calls `registry.machine_for("track")` and passes the result to the interpreter.
/// If the registry returns `None` (e.g., missing or malformed machine.yaml), the
/// function returns an empty vec — same behaviour as an unknown kind.
///
/// AC12.a.2 note: `registry` is a parameter (`&dyn PlaybookRegistry`), not a
/// struct field on `DescribeQueryHandler`. This keeps the handler a pure function
/// and avoids `Box<dyn PlaybookRegistry>` field lifetime complexity. The engine
/// composition root constructs the registry per call and passes it through.
pub fn available_actions(
    registry: &dyn PlaybookRegistry,
    kind: &str,
    state: &str,
) -> Vec<AvailableAction> {
    match kind {
        // Track lifecycle — playbook-driven via registry (R12.2–12.5).
        // `registry.machine_for("track")` resolves the on-disk machine.yaml;
        // interpreter queries it for outgoing transitions. `None` → empty vec.
        "track" => registry
            .machine_for("track")
            .map(|m| dedup_actions(interpreter::outgoing_transitions(m, state)))
            .unwrap_or_default(),
        // Free lifecycle kinds now playbook-driven via registry. They are
        // excluded from route candidates, but describe still surfaces their
        // engine-driven instance actions.
        "decision" | "initiative" | "milestone" | "proposal" | "learning" => registry
            .machine_for(kind)
            .map(|m| dedup_actions(interpreter::outgoing_transitions(m, state)))
            .unwrap_or_default(),
        // Any other (domain machine) kind: if it resolves to a machine
        // (knowledge_lifecycle), surface its machine-declared outgoing
        // transitions from `state` — the same interpreter-driven path the track
        // arm uses. Unresolved kinds yield no actions.
        other => registry
            .machine_for(other)
            .map(|m| dedup_actions(interpreter::outgoing_transitions(m, state)))
            .unwrap_or_default(),
    }
}

/// Map outgoing transitions to available actions, collapsing duplicates that
/// share the same `(to_state, required_role)`. Slice C introduced a second
/// `spec_review → plan` reviewer edge (satisfaction-discriminated for the
/// carry-forward path); satisfaction is not part of the `AvailableAction`
/// shape, so the two plan edges must surface as ONE action. Order-preserving:
/// the first occurrence wins.
fn dedup_actions(transitions: Vec<interpreter::OutgoingTransition>) -> Vec<AvailableAction> {
    let mut seen: Vec<(String, String)> = Vec::new();
    let mut out: Vec<AvailableAction> = Vec::new();
    for t in transitions {
        let key = (t.to_state.clone(), t.required_role.clone());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(action(&t.to_state, &t.required_role));
    }
    out
}

fn action(action: &str, role: &str) -> AvailableAction {
    AvailableAction {
        action: action.to_string(),
        required_role: role.to_string(),
        // Filled in by `available_actions` caller once the artifact kind +
        // state context is known. The raw `action()` constructor leaves it
        // empty for call sites that assemble fixtures in tests.
        execution_route: String::new(),
    }
}

/// Query handler for describe.
///
/// `execute` is a pure function: it reads via `DescribePort`, consults the
/// `PlaybookRegistry` to resolve available actions, and returns a value. No I/O,
/// no global state, no mutation — CQRS handler shape per truth.md.
///
/// The registry is passed as a parameter (not held as a struct field) so the
/// engine composition root controls construction per-call. Phase 6 will pass a
/// `CompositePlaybookRegistry`; Phase 5 passes a `SeedPlaybookRegistry` inline.
pub struct DescribeQueryHandler;

impl DescribeQueryHandler {
    /// Execute a describe query.
    ///
    /// `registry` is resolved by the caller; `port` provides artifact instance
    /// data. The `registry` parameter replaces the previous direct seed call —
    /// any `PlaybookRegistry` implementation is accepted.
    pub fn execute(
        port: &dyn DescribePort,
        registry: &dyn PlaybookRegistry,
        request: DescribeRequest,
    ) -> Result<DescribeResult, DescribeError> {
        // Try type-level first
        if KNOWN_TYPES.contains(&request.identifier.as_str()) {
            return Self::describe_type(port, &request.identifier);
        }

        // Try instance-level
        Self::describe_instance(port, registry, &request.identifier)
    }

    fn describe_type(
        port: &dyn DescribePort,
        type_name: &str,
    ) -> Result<DescribeResult, DescribeError> {
        let type_info = available_artifact_types()
            .into_iter()
            .find(|t| t.name == type_name)
            .ok_or_else(|| DescribeError::UnknownIdentifier {
                identifier: type_name.to_string(),
            })?;

        let _ = port; // port not needed for type-level (schema is hardcoded)

        Ok(DescribeResult::TypeInfo {
            name: type_info.name,
            description: type_info.description,
            required_fields: required_fields_for_type(type_name),
            parent_type: type_info.requires_parent,
        })
    }

    fn describe_instance(
        port: &dyn DescribePort,
        registry: &dyn PlaybookRegistry,
        artifact_id: &str,
    ) -> Result<DescribeResult, DescribeError> {
        let instance = port.read_instance(artifact_id)?;

        let actions = available_actions(registry, &instance.kind, &instance.state)
            .into_iter()
            .map(|mut a| {
                a.execution_route = compute_execution_route_with_registry(
                    SUBJECT_AVAILABLE_ACTION,
                    &instance.kind,
                    &instance.state,
                    &a.required_role,
                    registry,
                );
                a
            })
            .collect();

        Ok(DescribeResult::InstanceInfo {
            id: artifact_id.to_string(),
            artifact_type: instance.kind,
            state: instance.state,
            transition_count: instance.transition_count,
            last_transition: instance.last_transition,
            available_actions: actions,
        })
    }
}
