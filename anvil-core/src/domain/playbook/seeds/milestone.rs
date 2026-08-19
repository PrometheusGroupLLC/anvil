//! Compiled-in `PlaybookMachine` literal for the `milestone` kind.
//!
//! Milestones are free artifacts (`register: free`): begin-able out of
//! band, but excluded from route candidates. The state set mirrors the
//! milestone sections already projected by `snapshot.rs`; no extra lifecycle
//! states are introduced here.

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
    entries: &[(&str, &str, &str, &str)],
) -> std::collections::BTreeMap<String, MeasurementSpec> {
    entries
        .iter()
        .map(|(role, intent, expected_output, success_criteria)| {
            (
                role.to_string(),
                MeasurementSpec {
                    intent: intent.to_string(),
                    expected_output: expected_output.to_string(),
                    success_criteria: Some(success_criteria.to_string()),
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
    measurements: &[(&str, &str, &str, &str)],
) -> StateDefinition {
    StateDefinition {
        name: name.to_string(),
        role_filters: vec![role_filter],
        registry_section: registry_section.to_string(),
        projection_targets: vec!["milestones.md".to_string()],
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
        kind: "milestone".to_string(),
        directory: "milestones".to_string(),
        registry: "milestones.md".to_string(),
        parent_kind: None,
        parent_required: true,
        description: "Delivery-scoped outcome that spans proposals and tracks.".to_string(),
        access: Access::foundation(),
        owner_kit: String::new(),
        visibility: String::new(),
        route: RouteConfig::default(),
        projection_only: false,
        required_fields: vec![FieldDescriptor {
            name: "name".to_string(),
            field_type: "string".to_string(),
            description: "Milestone title".to_string(),
        }],
        roles: vec![
            "doer".to_string(),
            "reviewer".to_string(),
            "amend".to_string(),
            "reflect".to_string(),
        ],
        states: vec![
            state(
                "draft",
                "draft",
                false,
                false,
                RoleFilter::DoerActionable,
                &[("doer", "draft.md")],
                &[(
                    "doer",
                    "Draft the delivery outcome, contributing work, success criteria, and current readiness judgment.",
                    "A definition.md milestone draft ready for draft review.",
                    "A `definition.md` states the delivery outcome as a verifiable end-state, lists every contributing proposal or track by id, declares at least 1 falsifiable success criterion (naming a threshold, artifact, or zero-case), and gives an honest readiness judgment naming what is not yet done.",
                )],
            ),
            state(
                "draft_review",
                "draft",
                true,
                false,
                RoleFilter::ReviewPending,
                &[("reviewer", "draft-review.md")],
                &[(
                    "reviewer",
                    "Review whether the milestone outcome is specific, delivery-scoped, verifiable, and honest about readiness.",
                    "A review.md entry with satisfied sign-off or specific findings.",
                    "A `review.md` verdict addresses all four of outcome specificity, delivery scope, criterion verifiability, and readiness honesty. A satisfied verdict cites the specific `definition.md` evidence clearing each (the named verifiable end-state, the contributing proposal/track ids, at least 1 falsifiable success criterion, and the honest readiness note); a needs_revision verdict names at least 1 specific gap among those four. No verdict is satisfied while any declared success criterion is non-verifiable or any dimension is left unassessed.",
                )],
            ),
            state(
                "draft_revision",
                "draft",
                false,
                false,
                RoleFilter::DoerActionable,
                &[("doer", "milestone-revise.md")],
                &[(
                    "doer",
                    "Revise the milestone draft in response to draft-review findings.",
                    "An updated definition.md plus review.md dispositions for every finding.",
                    "An updated `definition.md` plus a `review.md` response dispositioning every finding (addressed or rejected-with-rationale), leaving zero findings undispositioned.",
                )],
            ),
            state(
                "active",
                "active",
                false,
                false,
                RoleFilter::DoerActionable,
                &[
                    ("doer", "terminal.md"),
                    ("amend", "amend.md"),
                    ("reflect", "reflect.md"),
                ],
                &[
                    (
                        "doer",
                        "Complete, supersede, or abandon an active milestone with human approval.",
                        "A terminal transition preserving the closure rationale.",
                        "A terminal transition to completed, superseded, or abandoned records the closure rationale and carries the required human approver sign-off, with zero terminal moves made without an approver.",
                    ),
                    (
                        "amend",
                        "Append a milestone amendment when the delivery outcome or success criteria change after activation.",
                        "An append-only amendments.md entry ready for amendment review.",
                        "A numbered append-only entry in `amendments.md` names which delivery outcome or success criterion changed, why, and its impact on the milestone's contributing work, with the frozen `definition.md` unedited (zero diff outside `amendments.md`).",
                    ),
                    (
                        "reflect",
                        "Reflect on the milestone's delivery delta and whether success criteria were met.",
                        "A reflection.md artifact ready for reflection review.",
                        "A `reflection.md` states, per declared success criterion, whether it was met with evidence, names the delivery delta between planned and realized outcome, and owns at least 1 criterion that fell short (or states none did, with evidence).",
                    ),
                ],
            ),
            state(
                "amend",
                "active",
                false,
                false,
                RoleFilter::DoerActionable,
                &[("amend", "amend.md")],
                &[(
                    "amend",
                    "Append a milestone amendment when the delivery outcome or success criteria change after activation.",
                    "An append-only amendments.md entry ready for amendment review.",
                    "A numbered append-only entry in `amendments.md` names the changed outcome or criterion, its rationale, and its impact on the milestone's contributing work, with the frozen `definition.md` unedited (zero diff outside `amendments.md`).",
                )],
            ),
            state(
                "amend_review",
                "active",
                true,
                false,
                RoleFilter::ReviewPending,
                &[("reviewer", "amend-review.md")],
                &[(
                    "reviewer",
                    "Review whether the milestone amendment is necessary, scoped, and safe to approve.",
                    "A review.md amendment-review entry with satisfied sign-off or findings.",
                    "A `review.md` amendment-review entry addresses necessity, scope, and append-only discipline. A satisfied verdict cites the `amendments.md` evidence showing the change is necessary, scoped to the stated outcome/criterion, and appended without editing the frozen `definition.md`; a needs_revision verdict names at least 1 specific gap among those three. No verdict is satisfied while the frozen `definition.md` shows a diff or any dimension is left unassessed.",
                )],
            ),
            state(
                "amend_revision",
                "active",
                false,
                false,
                RoleFilter::DoerActionable,
                &[("amend", "amend-revise.md")],
                &[(
                    "amend",
                    "Revise the milestone amendment in response to amendment-review findings.",
                    "Updated amendments.md plus review dispositions.",
                    "An updated `amendments.md` plus a review response dispositioning every finding (addressed or rejected-with-rationale), with zero findings left open and the frozen `definition.md` unedited (zero diff outside `amendments.md`).",
                )],
            ),
            state(
                "reflecting",
                "reflecting",
                false,
                false,
                RoleFilter::DoerActionable,
                &[("reflect", "reflect.md")],
                &[(
                    "reflect",
                    "Write the milestone reflection covering delivery delta, criteria results, and remaining risks.",
                    "A reflection.md artifact ready for reflection review.",
                    "A `reflection.md` reports each declared success criterion's result with evidence, names the delivery delta between planned and realized outcome, and names at least 1 remaining risk to the outcome (or states there are none, backed by evidence); it is not a restatement of the definition in past tense.",
                )],
            ),
            state(
                "reflection_review",
                "reflecting",
                true,
                false,
                RoleFilter::ReviewPending,
                &[("reviewer", "reflection-review.md")],
                &[(
                    "reviewer",
                    "Review whether the milestone reflection accurately captures delivery readiness and unresolved gaps.",
                    "A reflection.review.md entry with satisfied sign-off or findings.",
                    "A `reflection.review.md` verdict assesses every declared success criterion's reported result and the readiness claims. A satisfied verdict cites the `reflection.md` evidence backing each criterion result, the named delivery delta, and the remaining-risk statement; a needs_revision verdict names at least 1 criterion result or readiness claim the reflection got wrong or omitted. No verdict is satisfied while any criterion result lacks evidence or any dimension is left unassessed.",
                )],
            ),
            state(
                "reflection_revision",
                "reflecting",
                false,
                false,
                RoleFilter::DoerActionable,
                &[("reflect", "reflection-revise.md")],
                &[(
                    "reflect",
                    "Revise the milestone reflection in response to reflection-review findings.",
                    "An updated reflection.md plus reflection.review.md dispositions.",
                    "An updated `reflection.md` plus a `reflection.review.md` response dispositioning every finding, with zero criterion-result claims left inaccurate.",
                )],
            ),
            state(
                "completed",
                "completed",
                false,
                true,
                RoleFilter::Terminal,
                &[("doer", "terminal.md")],
                &[],
            ),
            state(
                "superseded",
                "superseded",
                false,
                true,
                RoleFilter::Terminal,
                &[("doer", "terminal.md")],
                &[],
            ),
            state(
                "abandoned",
                "abandoned",
                false,
                true,
                RoleFilter::Terminal,
                &[("doer", "terminal.md")],
                &[],
            ),
        ],
        transitions: vec![
            edge("draft", "draft_review", "doer"),
            review_edge(
                "draft_review",
                "draft_revision",
                "doer",
                "needs_revision",
            ),
            review_edge("draft_review", "active", "reviewer", "satisfied"),
            edge("draft_revision", "draft_review", "doer"),
            edge("active", "amend", "amend"),
            edge("amend", "amend_review", "amend"),
            review_edge(
                "amend_review",
                "amend_revision",
                "amend",
                "needs_revision",
            ),
            review_edge("amend_review", "active", "reviewer", "satisfied"),
            edge("amend_revision", "amend_review", "amend"),
            edge("active", "reflecting", "reflect"),
            edge("reflecting", "reflection_review", "reflect"),
            review_edge(
                "reflection_review",
                "reflection_revision",
                "reflect",
                "needs_revision",
            ),
            review_edge("reflection_review", "active", "reviewer", "satisfied"),
            edge("reflection_revision", "reflection_review", "reflect"),
            gated_edge("active", "completed", "doer"),
            gated_edge("active", "superseded", "doer"),
            gated_edge("active", "abandoned", "doer"),
        ],
        register: Register::Free,
        success_rubric: None,
        // Mirrors playbooks/milestone_lifecycle/machine.yaml (source-tier parity,
        // guarded by seed_outcome_predicate_parity.feature). `completed` is
        // reachable directly from active, so the check claims only what that
        // state guarantees — an approver-signed terminal after draft_review —
        // not reflection evidence, which the graph does not gate before it.
        outcome_predicate: Some(OutcomePredicate {
            terminal_state: "completed".to_string(),
            check: Some(
                "the milestone reached completed via an approver-signed terminal transition that recorded the closure rationale, after its draft_review gate was satisfied — the delivery-scoped outcome was closed as delivered rather than superseded or abandoned"
                    .to_string(),
            ),
        }),
    }
}
