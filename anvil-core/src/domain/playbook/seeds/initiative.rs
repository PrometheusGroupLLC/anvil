//! Compiled-in `PlaybookMachine` literal for the `initiative` kind.
//!
//! Initiatives are free artifacts (`register: free`): begin-able out of
//! band, but excluded from route candidates. `log` and `reflect` modes append
//! to the initiative directory with no state change; both are modeled as
//! ungated self-edges on `active`/`promoted` (role `log`/`reflect`) so every
//! initiative state stays within the set `snapshot.rs` already projects. That
//! avoids a new registry section or projection gap.

use crate::domain::playbook::types::{
    Access, FieldDescriptor, MeasurementSpec, OutcomePredicate, PlaybookMachine, Register,
    RoleFilter, RouteConfig, StateDefinition, TransitionDefinition,
};

fn role_map(entries: &[(&str, &str)]) -> std::collections::BTreeMap<String, String> {
    entries
        .iter()
        .map(|(role, filename)| (role.to_string(), filename.to_string()))
        .collect()
}

fn measurement_map(
    entries: &[(&str, &str, &str)],
) -> std::collections::BTreeMap<String, MeasurementSpec> {
    entries
        .iter()
        .map(|(role, intent, expected_output)| {
            (
                role.to_string(),
                MeasurementSpec {
                    intent: intent.to_string(),
                    expected_output: expected_output.to_string(),
                    success_criteria: None,
                    evidence_obligation: Vec::new(),
                },
            )
        })
        .collect()
}

fn state(
    name: &str,
    registry_section: &str,
    is_review_gate: bool,
    is_terminal: bool,
    role_filter: RoleFilter,
    hooks: &[(&str, &str)],
    measurements: &[(&str, &str, &str)],
) -> StateDefinition {
    StateDefinition {
        name: name.to_string(),
        role_filters: vec![role_filter],
        registry_section: registry_section.to_string(),
        projection_targets: vec!["initiatives.md".to_string()],
        is_review_gate,
        is_terminal,
        hook: None,
        hooks_by_role: role_map(hooks),
        measurement_by_role: measurement_map(measurements),
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

fn gated_edge(from: &str, to: &str, role: &str) -> TransitionDefinition {
    TransitionDefinition {
        from_state: from.to_string(),
        to_state: to.to_string(),
        required_role: role.to_string(),
        required_satisfaction: None,
        requires_approver: true,
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
        kind: "initiative".to_string(),
        directory: "initiatives".to_string(),
        registry: "initiatives.md".to_string(),
        parent_kind: None,
        parent_required: true,
        description: "Create, manage, and track initiatives — cross-cutting implementation expectations with evidence.".to_string(),
        access: Access::foundation(),
        owner_kit: String::new(),
        visibility: String::new(),
        route: RouteConfig::default(),
        projection_only: false,
        required_fields: vec![FieldDescriptor {
            name: "name".to_string(),
            field_type: "string".to_string(),
            description: "Initiative title".to_string(),
        }],
        roles: vec![
            "doer".to_string(),
            "draft".to_string(),
            "reviewer".to_string(),
            "promote".to_string(),
            "demote".to_string(),
            "retire".to_string(),
            "log".to_string(),
            "reflect".to_string(),
        ],
        states: vec![
            state(
                "draft",
                "draft",
                false,
                false,
                RoleFilter::DoerActionable,
                &[("doer", "draft.md"), ("draft", "draft.md")],
                &[
                    (
                        "doer",
                        "Draft a new initiative definition with pattern, rationale, guidance, status, source, and related rule context.",
                        "A definition.md for a distinct initiative, ready for draft review.",
                    ),
                    (
                        "draft",
                        "Draft a new initiative definition with pattern, rationale, guidance, status, source, and related rule context.",
                        "A definition.md for a distinct initiative, ready for draft review.",
                    ),
                ],
            ),
            state(
                "draft_review",
                "draft",
                true,
                false,
                RoleFilter::ReviewPending,
                &[("reviewer", "draft.md")],
                &[(
                    "reviewer",
                    "Review whether the initiative definition is accurate, valid, well-scoped, and enforceable.",
                    "A review.md entry with satisfied sign-off or specific findings.",
                )],
            ),
            state(
                "draft_revision",
                "draft",
                false,
                false,
                RoleFilter::DoerActionable,
                &[("doer", "revise.md"), ("draft", "revise.md")],
                &[
                    (
                        "doer",
                        "Revise the initiative definition in response to draft-review findings.",
                        "An updated definition.md plus review.md dispositions for every finding.",
                    ),
                    (
                        "draft",
                        "Revise the initiative definition in response to draft-review findings.",
                        "An updated definition.md plus review.md dispositions for every finding.",
                    ),
                ],
            ),
            state(
                "active",
                "active",
                false,
                false,
                RoleFilter::DoerActionable,
                &[
                    ("promote", "promote-demote.md"),
                    ("retire", "retire.md"),
                    ("log", "log.md"),
                    ("reflect", "reflect.md"),
                ],
                &[
                    (
                        "promote",
                        "Promote an active initiative to a CLAUDE.md rule when evidence supports convergence.",
                        "Updated definition/evidence and CLAUDE.md rule context for the promoted initiative.",
                    ),
                    (
                        "retire",
                        "Retire an active initiative when it is no longer enforced.",
                        "Updated definition.md and evidence.md preserving the retirement rationale.",
                    ),
                    (
                        "log",
                        "Append out-of-band evidence without changing initiative state.",
                        "An evidence.md entry recording advance, regress, exception-proposed, or exception-approved.",
                    ),
                    (
                        "reflect",
                        "Append convergence synthesis without changing initiative state.",
                        "A timestamped reflection.md synthesis entry covering convergence, definition effectiveness, exceptions, and lifecycle readiness.",
                    ),
                ],
            ),
            state(
                "promoted",
                "promoted",
                false,
                false,
                RoleFilter::DoerActionable,
                &[
                    ("demote", "promote-demote.md"),
                    ("retire", "retire.md"),
                    ("log", "log.md"),
                ],
                &[
                    (
                        "demote",
                        "Demote a promoted initiative back to active when the CLAUDE.md rule no longer fits.",
                        "Updated definition/evidence and CLAUDE.md rule context for the demoted initiative.",
                    ),
                    (
                        "retire",
                        "Retire a promoted initiative when it is no longer enforced.",
                        "Updated definition.md, evidence.md, and CLAUDE.md rule context preserving the retirement rationale.",
                    ),
                    (
                        "log",
                        "Append out-of-band evidence without changing initiative state.",
                        "An evidence.md entry recording advance, regress, exception-proposed, or exception-approved.",
                    ),
                ],
            ),
            state(
                "retired",
                "retired",
                false,
                true,
                RoleFilter::Terminal,
                &[("doer", "retire.md"), ("retire", "retire.md")],
                &[],
            ),
        ],
        transitions: vec![
            edge("draft", "draft_review", "draft"),
            review_edge("draft_review", "draft_revision", "draft", "needs_revision"),
            review_edge("draft_review", "active", "reviewer", "satisfied"),
            edge("draft_revision", "draft_review", "draft"),
            gated_edge("active", "promoted", "promote"),
            gated_edge("promoted", "active", "demote"),
            gated_edge("active", "retired", "retire"),
            gated_edge("promoted", "retired", "retire"),
            edge("active", "active", "log"),
            edge("promoted", "promoted", "log"),
            edge("active", "active", "reflect"),
        ],
        register: Register::Free,
        success_rubric: None,
        // Mirrors playbooks/initiative_lifecycle/machine.yaml (source-tier
        // parity, guarded by seed_outcome_predicate_parity.feature).
        outcome_predicate: Some(OutcomePredicate {
            terminal_state: "active".to_string(),
            check: Some(
                "the initiative reached active with an approver-signed draft_review satisfied, so it is a tracked cross-cutting expectation rather than an unreviewed draft"
                    .to_string(),
            ),
        }),
    }
}
