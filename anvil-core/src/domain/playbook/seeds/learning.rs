//! Compiled-in `PlaybookMachine` literal for the `learning` kind.
//!
//! Learnings are free artifacts (`register: free`): begin-able out of band,
//! but excluded from route candidates. The model mirrors the existing learning
//! skill lifecycle (observation → conclusion → established, with terminal paths
//! graduated / retired) so existing learning artifacts remain valid. It is a
//! one-for-one analogue of the decision lifecycle (seeds/decision.rs):
//! observation* mirrors tension*, conclusion* mirrors the resolved-review
//! states, established mirrors decided (amend loop), and graduated/retired are
//! the terminal exits. Learnings carry no projection target (spec §4).

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
        projection_targets: Vec::new(),
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
        kind: "learning".to_string(),
        directory: "learnings".to_string(),
        registry: "learnings.md".to_string(),
        parent_kind: None,
        parent_required: true,
        description: "Knowledge that matures from raw observation through interpreted conclusion to validated established knowledge.".to_string(),
        access: Access::foundation(),
        owner_kit: String::new(),
        visibility: String::new(),
        route: RouteConfig::default(),
        projection_only: false,
        required_fields: vec![FieldDescriptor {
            name: "name".to_string(),
            field_type: "string".to_string(),
            description: "Learning title".to_string(),
        }],
        roles: vec![
            "doer".to_string(),
            "learn".to_string(),
            "reviewer".to_string(),
            "amend".to_string(),
        ],
        states: vec![
            state(
                "observation",
                "observation",
                false,
                false,
                RoleFilter::DoerActionable,
                "doer",
                "capture.md",
                "Record a new observation with context, source, and domain tags.",
                "A learning definition.md containing the observation structure and ready for observation review.",
            ),
            state(
                "observation_review",
                "observation",
                true,
                false,
                RoleFilter::ReviewPending,
                "reviewer",
                "observation.md",
                "Review whether the observation is real, well scoped, distinct, and traceable.",
                "A review.md entry with satisfied sign-off or specific findings.",
            ),
            state(
                "observation_revision",
                "observation",
                false,
                false,
                RoleFilter::DoerActionable,
                "doer",
                "revise.md",
                "Revise the observation in response to review findings without freezing the definition.",
                "An updated definition.md plus review.md dispositions for every finding.",
            ),
            state(
                "conclusion",
                "conclusion",
                false,
                false,
                RoleFilter::DoerActionable,
                "doer",
                "interpret.md",
                "Append the interpretation, implications, scope, and supporting evidence for the observation.",
                "A complete learning definition.md ready for conclusion review.",
            ),
            state(
                "conclusion_review",
                "conclusion",
                true,
                false,
                RoleFilter::ReviewPending,
                "reviewer",
                "conclusion.md",
                "Review whether the interpreted conclusion is sound and its scope is actionable.",
                "A review.md entry with satisfied sign-off or specific findings.",
            ),
            state(
                "conclusion_revision",
                "conclusion",
                false,
                false,
                RoleFilter::DoerActionable,
                "doer",
                "revise.md",
                "Revise the conclusion in response to conclusion-review findings.",
                "An updated definition.md plus review.md dispositions for every finding.",
            ),
            state(
                "established",
                "established",
                false,
                false,
                RoleFilter::DoerActionable,
                "doer",
                "establish.md",
                "Establish the conclusion as validated standing knowledge once cross-context evidence supports it.",
                "An established learning definition.md with validity conditions and supporting evidence.",
            ),
            state(
                "amend",
                "established",
                false,
                false,
                RoleFilter::DoerActionable,
                "doer",
                "amend.md",
                "Append an amendment when validity conditions shift or evidence contradicts the established learning.",
                "An append-only amendments.md entry ready for amendment review.",
            ),
            state(
                "amend_review",
                "established",
                true,
                false,
                RoleFilter::ReviewPending,
                "reviewer",
                "amend-review.md",
                "Review the learning amendment against the established learning criteria and amendment mechanism.",
                "A review.md entry with satisfied sign-off or findings.",
            ),
            state(
                "amend_revision",
                "established",
                false,
                false,
                RoleFilter::DoerActionable,
                "doer",
                "revise.md",
                "Revise the amendment or apply an approved amendment to definition.md.",
                "Updated amendments.md or definition.md plus review.md dispositions.",
            ),
            state(
                "graduated",
                "graduated",
                false,
                true,
                RoleFilter::Terminal,
                "doer",
                "graduate.md",
                "Record the initiative or decision that absorbed this established learning.",
                "A graduated evidence event or transition note linking the absorbing artifact.",
            ),
            state(
                "retired",
                "retired",
                false,
                true,
                RoleFilter::Terminal,
                "doer",
                "retire.md",
                "Record why the learning is no longer relevant.",
                "A retired evidence event or transition note preserving the rationale.",
            ),
        ],
        transitions: vec![
            edge("observation", "observation_review", "learn"),
            edge("observation", "conclusion", "learn"),
            review_edge(
                "observation_review",
                "observation_revision",
                "learn",
                "needs_revision",
            ),
            review_edge("observation_review", "observation", "reviewer", "satisfied"),
            edge("observation_revision", "observation_review", "learn"),
            edge("conclusion", "conclusion_review", "learn"),
            review_edge(
                "conclusion_review",
                "conclusion_revision",
                "learn",
                "needs_revision",
            ),
            review_edge("conclusion_review", "established", "reviewer", "satisfied"),
            edge("conclusion_revision", "conclusion_review", "learn"),
            edge("established", "amend", "amend"),
            edge("established", "graduated", "learn"),
            edge("established", "retired", "learn"),
            edge("amend", "amend_review", "amend"),
            review_edge("amend_review", "amend_revision", "amend", "needs_revision"),
            review_edge("amend_review", "established", "reviewer", "satisfied"),
            edge("amend_revision", "amend_review", "amend"),
        ],
        register: Register::Free,
        success_rubric: None,
        // Mirrors playbooks/learning_lifecycle/machine.yaml (source-tier parity,
        // guarded by seed_outcome_predicate_parity.feature).
        outcome_predicate: Some(OutcomePredicate {
            terminal_state: "established".to_string(),
            check: Some(
                "the learning reached established with a conclusion_review satisfied, promoting a raw observation into validated institutional knowledge"
                    .to_string(),
            ),
        }),
    }
}
