//! Compiled-in `PlaybookMachine` literal for the `decision` kind.
//!
//! Decisions are free artifacts (`register: free`): begin-able out of band,
//! but excluded from route candidates. The model intentionally mirrors the
//! existing decision skill lifecycle so existing decision artifacts remain
//! valid.

use crate::domain::playbook::types::{
    Access, FieldDescriptor, MeasurementSpec, OutcomePredicate, PlaybookMachine, Register,
    RoleFilter, RouteConfig, StateDefinition, TransitionDefinition,
};

fn hooks_by_role(role: &str, filename: &str) -> std::collections::BTreeMap<String, String> {
    let mut m = std::collections::BTreeMap::new();
    m.insert(role.to_string(), filename.to_string());
    m
}

fn measurement_by_role(
    role: &str,
    intent: &str,
    expected_output: &str,
) -> std::collections::BTreeMap<String, MeasurementSpec> {
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        role.to_string(),
        MeasurementSpec {
            intent: intent.to_string(),
            expected_output: expected_output.to_string(),
            success_criteria: None,
            evidence_obligation: Vec::new(),
        },
    );
    m
}

fn state(
    name: &str,
    registry_section: &str,
    is_review_gate: bool,
    is_terminal: bool,
    role_filter: RoleFilter,
    hook_role: &str,
    hook_file: &str,
    intent: &str,
    expected_output: &str,
) -> StateDefinition {
    StateDefinition {
        name: name.to_string(),
        role_filters: vec![role_filter],
        registry_section: registry_section.to_string(),
        projection_targets: vec!["decisions.md".to_string()],
        is_review_gate,
        is_terminal,
        hook: None,
        hooks_by_role: hooks_by_role(hook_role, hook_file),
        measurement_by_role: measurement_by_role(hook_role, intent, expected_output),
    }
}

fn edge(from: &str, to: &str, role: &str) -> TransitionDefinition {
    TransitionDefinition {
        from_state: from.to_string(),
        to_state: to.to_string(),
        required_role: role.to_string(),
        required_satisfaction: None,
        requires_approver: false,
        hook: None,
    }
}

fn review_edge(from: &str, to: &str, role: &str, satisfaction: &str) -> TransitionDefinition {
    TransitionDefinition {
        from_state: from.to_string(),
        to_state: to.to_string(),
        required_role: role.to_string(),
        required_satisfaction: Some(vec![satisfaction.to_string()]),
        requires_approver: role == "reviewer",
        hook: None,
    }
}

pub fn build() -> PlaybookMachine {
    PlaybookMachine {
        kind: "decision".to_string(),
        directory: "decisions".to_string(),
        registry: "decisions.md".to_string(),
        parent_kind: None,
        parent_required: true,
        description: "Institutional knowledge: question, answer, rationale, and validity conditions.".to_string(),
        access: Access::foundation(),
        owner_kit: String::new(),
        visibility: String::new(),
        route: RouteConfig::default(),
        projection_only: false,
        required_fields: vec![FieldDescriptor {
            name: "name".to_string(),
            field_type: "string".to_string(),
            description: "Decision title".to_string(),
        }],
        roles: vec![
            "doer".to_string(),
            "decide".to_string(),
            "reviewer".to_string(),
            "amend".to_string(),
        ],
        states: vec![
            state(
                "tension",
                "tension",
                false,
                false,
                RoleFilter::DoerActionable,
                "doer",
                "create.md",
                "Record a new unresolved decision question with context, tags, lineage, and related links.",
                "A decision definition.md containing the tension structure and ready for tension review.",
            ),
            state(
                "tension_review",
                "tension",
                true,
                false,
                RoleFilter::ReviewPending,
                "reviewer",
                "decision-tension.md",
                "Review whether the tension is real, well scoped, distinct, and traceable.",
                "A review.md entry with satisfied sign-off or specific findings.",
            ),
            state(
                "tension_revision",
                "tension",
                false,
                false,
                RoleFilter::DoerActionable,
                "doer",
                "revise.md",
                "Revise the tension in response to review findings without freezing the definition.",
                "An updated definition.md plus review.md dispositions for every finding.",
            ),
            state(
                "investigating",
                "investigating",
                false,
                false,
                RoleFilter::DoerActionable,
                "doer",
                "investigate.md",
                "Record the assessment track or deliberation actively investigating this tension.",
                "A transition note linking the investigation source.",
            ),
            state(
                "decided",
                "decided",
                false,
                false,
                RoleFilter::DoerActionable,
                "doer",
                "resolve.md",
                "Append the resolution, rationale, alternatives, validity conditions, investigation, and spawns.",
                "A complete decision definition.md ready for decision review.",
            ),
            state(
                "decision_review",
                "decided",
                true,
                false,
                RoleFilter::ReviewPending,
                "reviewer",
                "decision-resolved.md",
                "Review whether the resolved decision is sound and its validity conditions are actionable.",
                "A review.md entry with satisfied sign-off or specific findings.",
            ),
            state(
                "decision_revision",
                "decided",
                false,
                false,
                RoleFilter::DoerActionable,
                "doer",
                "revise.md",
                "Revise the resolved decision in response to decision-review findings.",
                "An updated definition.md plus review.md dispositions for every finding.",
            ),
            state(
                "amend",
                "decided",
                false,
                false,
                RoleFilter::DoerActionable,
                "doer",
                "amend.md",
                "Append an amendment when validity conditions shift or evidence contradicts the resolution.",
                "An append-only amendments.md entry ready for amendment review.",
            ),
            state(
                "amend_review",
                "decided",
                true,
                false,
                RoleFilter::ReviewPending,
                "reviewer",
                "amend-review.md",
                "Review the decision amendment against the resolved decision criteria and amendment mechanism.",
                "A review.md entry with satisfied sign-off or findings.",
            ),
            state(
                "amend_revision",
                "decided",
                false,
                false,
                RoleFilter::DoerActionable,
                "doer",
                "revise.md",
                "Revise the amendment or apply an approved amendment to definition.md.",
                "Updated amendments.md or definition.md plus review.md dispositions.",
            ),
            state(
                "retired",
                "retired",
                false,
                true,
                RoleFilter::Terminal,
                "doer",
                "retire.md",
                "Record why the decision is no longer relevant.",
                "A retired evidence event or transition note preserving the rationale.",
            ),
        ],
        transitions: vec![
            edge("tension", "tension_review", "decide"),
            edge("tension", "investigating", "decide"),
            edge("tension", "decided", "decide"),
            review_edge("tension_review", "tension_revision", "decide", "needs_revision"),
            review_edge("tension_review", "tension", "reviewer", "satisfied"),
            edge("tension_revision", "tension_review", "decide"),
            edge("investigating", "decided", "decide"),
            edge("decided", "decision_review", "decide"),
            edge("decided", "amend", "amend"),
            edge("decided", "retired", "decide"),
            review_edge("decision_review", "decision_revision", "decide", "needs_revision"),
            review_edge("decision_review", "decided", "reviewer", "satisfied"),
            edge("decision_revision", "decision_review", "decide"),
            edge("amend", "amend_review", "amend"),
            review_edge("amend_review", "amend_revision", "amend", "needs_revision"),
            review_edge("amend_review", "decided", "reviewer", "satisfied"),
            edge("amend_revision", "amend_review", "amend"),
        ],
        register: Register::Free,
        success_rubric: None,
        // Mirrors playbooks/decision_lifecycle/machine.yaml (source-tier parity,
        // guarded by seed_outcome_predicate_parity.feature).
        outcome_predicate: Some(OutcomePredicate {
            terminal_state: "decided".to_string(),
            check: Some(
                "the decision reached decided carrying a recorded answer, rationale, and validity conditions — the tension was resolved into institutional knowledge (decided is reachable directly from tension or investigating, so decision_review being satisfied is NOT implied by this state and is not claimed here)"
                    .to_string(),
            ),
        }),
    }
}
