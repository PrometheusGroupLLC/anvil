//! Lenient deserialization mirror for the event/step/queue-driven machine
//! schema (#34).
//!
//! Some registered playbooks (e.g. `import_transaction_history`,
//! `extract_document`) use an EVENT/STEP/QUEUE-DRIVEN schema rather than the
//! standard transition-graph schema that [`PlaybookMachine`] models. Their
//! top-level keys include `name`, `version`, `anvil_kind`, `trigger`,
//! `mcp_tool_dependencies`, and `steps`; their states carry an `on:` event map
//! and a `terminal:` flag instead of (or in addition to) the standard
//! transition graph. They are driven by an external queue + events, NOT by the
//! begin/complete doer/reviewer lifecycle.
//!
//! Because [`PlaybookMachine`] carries `#[serde(deny_unknown_fields)]` to keep
//! the standard schema strict, those files cannot deserialize through it — they
//! would be dropped into `invalid_artifacts` and excluded from the registry,
//! catalog, routing, and the dashboard. This module provides a SEPARATE mirror,
//! used by the loader ONLY when a file declares the event-driven schema. The
//! standard struct (and its strictness) is left completely unchanged.
//!
//! This mirror itself ALSO carries `#[serde(deny_unknown_fields)]`: it names the
//! complete event-driven + standard field set, so a genuinely-unknown key still
//! fails to parse on BOTH schemas. The relaxation is precise — it accepts the
//! legitimate event-driven variant, not arbitrary keys.
//!
//! The conversion maps the event-driven fields onto a [`PlaybookMachine`] so
//! these playbooks register as valid machines:
//! - `anvil_kind` (or `kind`) -> `kind`
//! - `description` -> `description`
//! - states (name + terminal flag + hooks/measurement) -> `states`
//! - `transitions` -> `transitions` when a hybrid machine declares them;
//!   otherwise an EMPTY list — the marker that exempts a machine from
//!   contiguity validation and the begin/complete-driven, machine-derived
//!   `next_step`.
//!
//! The event-only fields (`trigger`, `steps`, `mcp_tool_dependencies`, per-state
//! `on:`) are accepted (so parsing succeeds) but are not folded into the
//! begin/complete lifecycle, which never drives these machines.

use serde::Deserialize;

use super::types::{
    Access, MeasurementSpec, OutcomePredicate, PlaybookMachine, RoleFilter, StateDefinition,
    TransitionDefinition,
};

/// Mirror of a machine.yaml declaring the event-driven schema.
///
/// Carries `deny_unknown_fields`, naming the full event-driven + standard field
/// set so unknown keys still fail. Only the fields needed to build a registrable
/// [`PlaybookMachine`] are consumed; the rest are accepted-and-ignored.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventDrivenMachine {
    /// Standard `kind`, or the event-driven `anvil_kind` alias. Required for a
    /// registrable machine; absence yields `into_machine() == None`.
    #[serde(default, alias = "anvil_kind")]
    kind: Option<String>,
    /// Human-readable name (event-driven schema). Accepted, not consumed.
    #[serde(default)]
    name: Option<String>,
    /// Declared schema version (event-driven schema). Accepted, not consumed.
    #[serde(default)]
    version: Option<String>,
    /// One-line description surfaced via `available_types`.
    #[serde(default)]
    description: String,
    /// Optional parent kind, if the event-driven schema declares one.
    #[serde(default)]
    parent_kind: Option<String>,
    /// Trigger declaration (e.g. `{kind: pending_queue, poll_tool: ...}`).
    /// Accepted, not consumed — driven by its own queue/event mechanism.
    #[serde(default)]
    trigger: Option<serde_yaml::Value>,
    /// MCP tools the playbook consumes. Accepted as free-form, not consumed.
    #[serde(default)]
    mcp_tool_dependencies: Vec<serde_yaml::Value>,
    /// Step list (with `next:` pointers). Accepted as free-form, not consumed.
    #[serde(default)]
    steps: Vec<serde_yaml::Value>,
    /// Role names, if the machine ALSO declares a standard role list (hybrid
    /// machines like `extract_document` carry both schemas). When omitted, the
    /// role set is derived from the states' per-role hook/measurement keys so
    /// per-role-hook cross-reference validation passes.
    #[serde(default)]
    roles: Vec<String>,
    /// State definitions, in the event-driven shape (see [`EventState`]).
    #[serde(default)]
    states: Vec<EventState>,
    /// Standard transition graph, if the machine ALSO declares one (hybrid
    /// machines carry both an event/step schema and a `transitions:` graph).
    /// Event-only machines omit this; it stays empty, keeping them exempt from
    /// contiguity and the begin/complete-driven next_step.
    #[serde(default)]
    transitions: Vec<TransitionDefinition>,
    /// The machine's outcome predicate (the checkable "did the world-change
    /// happen" fact). Event-driven machines carry it at the top level exactly
    /// like standard machines; without this field the lenient mirror's
    /// `deny_unknown_fields` would reject a declared `outcome_predicate:` block
    /// AND `into_machine` would null it — so an event-driven machine could never
    /// satisfy the measurement-enforcement DEFINE block (which requires a
    /// non-blank `terminal_state`), structurally excluding the whole event-driven
    /// schema class from the enforcing registry. Optional: a machine that omits
    /// it stays `None`, unchanged.
    #[serde(default)]
    outcome_predicate: Option<OutcomePredicate>,
}

/// Mirror of an event-driven state. Carries `deny_unknown_fields` over the union
/// of event-driven and standard state keys.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct EventState {
    name: String,
    /// Terminal flag — accepts either `is_terminal:` or `terminal:`.
    #[serde(default, alias = "terminal")]
    is_terminal: bool,
    /// Event → next-state map (event-driven schema). Accepted, not consumed by
    /// the begin/complete lifecycle.
    #[serde(default)]
    on: std::collections::BTreeMap<String, String>,
    /// Optional registry section (event-driven states usually omit it).
    #[serde(default)]
    registry_section: String,
    /// Role-based filters, if declared.
    #[serde(default)]
    role_filters: Vec<RoleFilter>,
    /// Projection targets, if declared.
    #[serde(default)]
    projection_targets: Vec<String>,
    /// Review-gate flag, if declared.
    #[serde(default)]
    is_review_gate: bool,
    /// Optional role-agnostic hook filename.
    #[serde(default)]
    hook: Option<String>,
    /// Per-role hook filenames.
    #[serde(default)]
    hooks_by_role: std::collections::BTreeMap<String, String>,
    /// Per-role step-measurement specs.
    #[serde(default)]
    measurement_by_role: std::collections::BTreeMap<String, MeasurementSpec>,
}

impl EventDrivenMachine {
    /// Convert into a registrable [`PlaybookMachine`], or `None` when the
    /// identity (`kind`/`anvil_kind`) is absent.
    ///
    /// The resulting machine carries an EMPTY `transitions` list for event-only
    /// machines (no standard transition graph) — the marker that keeps it OUT
    /// of the begin/complete lifecycle: excluded from routing (no
    /// `route.triggers`), exempt from contiguity validation (empty
    /// transitions), and exempt from the machine-derived `next_step`.
    ///
    /// `roles` is taken verbatim when declared (hybrid machines), otherwise
    /// derived from the union of every state's per-role hook/measurement keys so
    /// the loader's per-role-hook cross-reference validation passes.
    pub fn into_machine(self) -> Option<PlaybookMachine> {
        let _ = (
            &self.name,
            &self.version,
            &self.trigger,
            &self.mcp_tool_dependencies,
            &self.steps,
        );
        let kind = self.kind.filter(|k| !k.is_empty())?;

        let roles = if self.roles.is_empty() {
            derive_roles(&self.states)
        } else {
            self.roles
        };

        let states = self
            .states
            .into_iter()
            .map(|s| StateDefinition {
                name: s.name,
                role_filters: s.role_filters,
                registry_section: s.registry_section,
                projection_targets: s.projection_targets,
                is_review_gate: s.is_review_gate,
                is_terminal: s.is_terminal,
                hook: s.hook,
                hooks_by_role: s.hooks_by_role,
                measurement_by_role: s.measurement_by_role,
            })
            .collect();

        Some(PlaybookMachine {
            kind,
            directory: String::new(),
            registry: String::new(),
            parent_kind: self.parent_kind,
            parent_required: true,
            description: self.description,
            access: Access::foundation(),
            owner_kit: String::new(),
            visibility: String::new(),
            route: Default::default(),
            projection_only: false,
            required_fields: Vec::new(),
            roles,
            states,
            transitions: self.transitions,
            register: Default::default(),
            success_rubric: None,
            outcome_predicate: self.outcome_predicate,
        })
    }
}

/// Derive the role set from the union of every state's per-role hook and
/// measurement keys, preserving first-seen order. Event-driven machines often
/// declare `hooks_by_role`/`measurement_by_role` without a top-level `roles:`
/// list; the loader's per-role-hook validation needs those roles declared.
fn derive_roles(states: &[EventState]) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut roles = Vec::new();
    for state in states {
        for role in state
            .hooks_by_role
            .keys()
            .chain(state.measurement_by_role.keys())
        {
            if seen.insert(role.clone()) {
                roles.push(role.clone());
            }
        }
    }
    roles
}
