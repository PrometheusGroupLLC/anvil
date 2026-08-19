//! Compiled-in `PlaybookMachine` literal for the `proposal` kind.
//!
//! Proposals are free artifacts (`register: free`): begin-able out of
//! band, but excluded from route candidates. The state set mirrors the
//! proposal sections already projected by `snapshot.rs`; no extra lifecycle
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
        projection_targets: vec!["proposals.md".to_string()],
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
        kind: "proposal".to_string(),
        directory: "proposals".to_string(),
        registry: "proposals.md".to_string(),
        parent_kind: None,
        parent_required: true,
        description: "Strategic direction and approach that can spawn implementation tracks."
            .to_string(),
        access: Access::foundation(),
        owner_kit: String::new(),
        visibility: String::new(),
        route: RouteConfig::default(),
        projection_only: false,
        required_fields: vec![FieldDescriptor {
            name: "name".to_string(),
            field_type: "string".to_string(),
            description: "Proposal title".to_string(),
        }],
        roles: vec![
            "doer".to_string(),
            "envision".to_string(),
            "propose".to_string(),
            "reviewer".to_string(),
            "amend".to_string(),
            "reflect".to_string(),
        ],
        states: vec![
            state(
                "vision",
                "vision",
                false,
                false,
                RoleFilter::DoerActionable,
                &[("doer", "envision.md"), ("envision", "envision.md")],
                &[
                    (
                        "doer",
                        "Frame the problem, desired future state, urgency, and relationship to existing work without committing to an approach.",
                        "A vision.md direction artifact ready for vision review.",
                        "A `vision.md` frames the problem, the desired future state, and the urgency, names its relationship to existing proposals or tracks, and commits to NO implementation approach (zero solution or architecture choices stated).",
                    ),
                    (
                        "envision",
                        "Frame the problem, desired future state, urgency, and relationship to existing work without committing to an approach.",
                        "A vision.md direction artifact ready for vision review.",
                        "A `vision.md` frames the problem, the desired future state, and the urgency, names its relationship to existing proposals or tracks, and commits to NO implementation approach (zero solution or architecture choices stated).",
                    ),
                ],
            ),
            state(
                "vision_review",
                "vision",
                true,
                false,
                RoleFilter::ReviewPending,
                &[("reviewer", "vision-review.md")],
                &[(
                    "reviewer",
                    "Review whether the direction is worth pursuing, the problem is real, and the desired future state is compelling.",
                    "A vision.review.md entry with satisfied sign-off or specific findings.",
                    "A `vision.review.md` verdict addresses problem reality, future-state clarity, and urgency. A satisfied verdict cites the `vision.md` evidence establishing each (a real named problem, a clear desired future state, a stated urgency) and confirms no implementation approach was committed; a needs_revision verdict names at least 1 specific gap among those three. No direction is approved without a stated problem or with any dimension left unassessed.",
                )],
            ),
            state(
                "vision_revision",
                "vision",
                false,
                false,
                RoleFilter::DoerActionable,
                &[("doer", "vision-revise.md"), ("envision", "vision-revise.md")],
                &[
                    (
                        "doer",
                        "Revise the vision in response to vision-review findings.",
                        "An updated vision.md plus vision.review.md dispositions for every finding.",
                        "An updated `vision.md` plus a `vision.review.md` response dispositioning every finding (addressed or rejected-with-rationale), leaving zero findings undispositioned and the no-approach boundary intact.",
                    ),
                    (
                        "envision",
                        "Revise the vision in response to vision-review findings.",
                        "An updated vision.md plus vision.review.md dispositions for every finding.",
                        "An updated `vision.md` plus a `vision.review.md` response dispositioning every finding (addressed or rejected-with-rationale), leaving zero findings undispositioned and the no-approach boundary intact.",
                    ),
                ],
            ),
            state(
                "draft",
                "draft",
                false,
                false,
                RoleFilter::DoerActionable,
                &[("doer", "propose.md"), ("propose", "propose.md")],
                &[
                    (
                        "doer",
                        "Research the approved vision and draft the concrete proposal approach.",
                        "A proposal.md approach artifact ready for draft review.",
                        "A `proposal.md` states the concrete approach grounded in the approved vision, cites the research that informed it, and names the tracks the approach would spawn; the approach is falsifiable, not a restatement of the vision.",
                    ),
                    (
                        "propose",
                        "Research the approved vision and draft the concrete proposal approach.",
                        "A proposal.md approach artifact ready for draft review.",
                        "A `proposal.md` states the concrete approach grounded in the approved vision, cites the research that informed it, and names the tracks the approach would spawn; the approach is falsifiable, not a restatement of the vision.",
                    ),
                ],
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
                    "Review the draft proposal approach for feasibility, scope, and fit with the approved vision.",
                    "A proposal.review.md entry with satisfied sign-off or specific findings.",
                    "A `proposal.review.md` verdict addresses feasibility, scope, and vision-fit of the drafted approach. A satisfied verdict cites the `proposal.md` evidence clearing each (a feasible approach, a bounded scope, and fidelity to the approved vision); a needs_revision verdict names at least 1 specific gap among those three tied to the drafted approach. No verdict is satisfied without that cited evidence or with any dimension left unassessed.",
                )],
            ),
            state(
                "draft_revision",
                "draft",
                false,
                false,
                RoleFilter::DoerActionable,
                &[("doer", "proposal-revise.md"), ("propose", "proposal-revise.md")],
                &[
                    (
                        "doer",
                        "Revise the draft proposal in response to draft-review findings.",
                        "An updated proposal.md plus proposal.review.md dispositions for every finding.",
                        "An updated `proposal.md` plus a `proposal.review.md` response dispositioning every draft-review finding, leaving zero findings undispositioned and approved vision scope preserved.",
                    ),
                    (
                        "propose",
                        "Revise the draft proposal in response to draft-review findings.",
                        "An updated proposal.md plus proposal.review.md dispositions for every finding.",
                        "An updated `proposal.md` plus a `proposal.review.md` response dispositioning every draft-review finding, leaving zero findings undispositioned and approved vision scope preserved.",
                    ),
                ],
            ),
            state(
                "proposal",
                "draft",
                false,
                false,
                RoleFilter::DoerActionable,
                &[("doer", "propose.md"), ("propose", "propose.md")],
                &[
                    (
                        "doer",
                        "Finalize the proposal approach for full proposal review.",
                        "A proposal.md artifact ready for proposal review.",
                        "A `proposal.md` finalizes the approach with every open draft-review concern resolved and the tracks-to-spawn and success signals named, ready for activation review with zero unresolved TODOs.",
                    ),
                    (
                        "propose",
                        "Finalize the proposal approach for full proposal review.",
                        "A proposal.md artifact ready for proposal review.",
                        "A `proposal.md` finalizes the approach with every open draft-review concern resolved and the tracks-to-spawn and success signals named, ready for activation review with zero unresolved TODOs.",
                    ),
                ],
            ),
            state(
                "proposal_review",
                "draft",
                true,
                false,
                RoleFilter::ReviewPending,
                &[("reviewer", "proposal-review.md")],
                &[(
                    "reviewer",
                    "Review whether the proposal approach is sound enough to activate.",
                    "A proposal.review.md entry with satisfied sign-off or specific findings.",
                    "A `proposal.review.md` verdict assesses approach soundness and activation readiness. A satisfied verdict cites the `proposal.md` evidence that the approach is sound and activation-ready (open draft-review concerns resolved, tracks-to-spawn and success signals named); a needs_revision verdict names at least 1 concrete soundness or activation-readiness gap. No proposal is activated with an unresolved approach or with any dimension left unassessed.",
                )],
            ),
            state(
                "proposal_revision",
                "draft",
                false,
                false,
                RoleFilter::DoerActionable,
                &[("doer", "proposal-revise.md"), ("propose", "proposal-revise.md")],
                &[
                    (
                        "doer",
                        "Revise the proposal in response to proposal-review findings.",
                        "An updated proposal.md plus proposal.review.md dispositions for every finding.",
                        "An updated `proposal.md` plus a `proposal.review.md` response dispositioning every proposal-review finding, leaving zero findings undispositioned.",
                    ),
                    (
                        "propose",
                        "Revise the proposal in response to proposal-review findings.",
                        "An updated proposal.md plus proposal.review.md dispositions for every finding.",
                        "An updated `proposal.md` plus a `proposal.review.md` response dispositioning every proposal-review finding, leaving zero findings undispositioned.",
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
                    ("doer", "terminal.md"),
                    ("amend", "amend.md"),
                    ("reflect", "reflect.md"),
                ],
                &[
                    (
                        "doer",
                        "Close, supersede, or abandon an active proposal with human approval.",
                        "A terminal transition preserving the closure rationale.",
                        "A terminal transition to completed, superseded, or abandoned records the closure rationale and carries the required human approver sign-off, with zero terminal moves made without an approver.",
                    ),
                    (
                        "amend",
                        "Append a proposal amendment when active execution changes proposal-level intent.",
                        "An append-only proposal.amendments.md entry ready for amendment review.",
                        "A numbered append-only entry in `proposal.amendments.md` names how proposal-level intent changed, why, and its impact on the tracks the proposal governs, with the frozen `proposal.md` unedited (zero diff outside `proposal.amendments.md`).",
                    ),
                    (
                        "reflect",
                        "Reflect on the active proposal's realized semantic delta.",
                        "A reflection.md artifact ready for reflection review.",
                        "A `reflection.md` names the semantic delta between the proposal's intended direction and what its linked tracks realized, and owns at least 1 assumption that proved wrong (or states none did, backed by evidence).",
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
                    "Append a proposal amendment when active execution changes proposal-level intent.",
                    "An append-only proposal.amendments.md entry ready for amendment review.",
                    "A numbered append-only entry in `proposal.amendments.md` names how proposal-level intent changed, why, and its impact on the tracks the proposal governs, with the frozen `proposal.md` unedited (zero diff outside `proposal.amendments.md`).",
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
                    "Review whether the proposal amendment changes proposal-level intent and is safe to approve.",
                    "A proposal.review.md amendment-review entry with satisfied sign-off or findings.",
                    "A `proposal.review.md` amendment-review entry addresses necessity, scope, and append-only discipline. A satisfied verdict cites the `proposal.amendments.md` evidence showing the change is necessary, scoped to proposal-level intent, and appended without editing the frozen `proposal.md`; a needs_revision verdict names at least 1 specific gap among those three. No verdict is satisfied while the frozen `proposal.md` shows a diff or any dimension is left unassessed.",
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
                    "Revise the proposal amendment in response to amendment-review findings.",
                    "Updated proposal.amendments.md plus review dispositions.",
                    "An updated `proposal.amendments.md` plus a review response dispositioning every finding (addressed or rejected-with-rationale), with zero findings open and the frozen `proposal.md` unedited (zero diff outside `proposal.amendments.md`).",
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
                    "Write the proposal reflection covering semantic delta and lessons from linked tracks.",
                    "A reflection.md artifact ready for reflection review.",
                    "A `reflection.md` states the realized-vs-intended semantic delta, extracts at least 1 durable lesson from the linked tracks, and is not a restatement of the proposal in past tense.",
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
                    "Review whether the proposal reflection accurately captures realized intent and remaining deltas.",
                    "A reflection.review.md entry with satisfied sign-off or findings.",
                    "A `reflection.review.md` verdict assesses the realized-vs-intended semantic delta and the extracted lessons. A satisfied verdict cites the `reflection.md` evidence backing the stated delta and at least 1 durable lesson; a needs_revision verdict names at least 1 realized-intent or remaining-delta claim the reflection got wrong or omitted. No verdict is satisfied while a delta claim lacks evidence or any dimension is left unassessed.",
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
                    "Revise the proposal reflection in response to reflection-review findings.",
                    "An updated reflection.md plus reflection.review.md dispositions.",
                    "An updated `reflection.md` plus a `reflection.review.md` response dispositioning every finding, with zero delta claims left inaccurate.",
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
            edge("vision", "vision_review", "envision"),
            review_edge(
                "vision_review",
                "vision_revision",
                "envision",
                "needs_revision",
            ),
            review_edge("vision_review", "draft", "reviewer", "satisfied"),
            edge("vision_revision", "vision_review", "envision"),
            edge("draft", "draft_review", "propose"),
            review_edge(
                "draft_review",
                "draft_revision",
                "propose",
                "needs_revision",
            ),
            review_edge("draft_review", "proposal", "reviewer", "satisfied"),
            edge("draft_revision", "draft_review", "propose"),
            edge("proposal", "proposal_review", "propose"),
            review_edge(
                "proposal_review",
                "proposal_revision",
                "propose",
                "needs_revision",
            ),
            review_edge("proposal_review", "active", "reviewer", "satisfied"),
            edge("proposal_revision", "proposal_review", "propose"),
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
        // Mirrors playbooks/proposal_lifecycle/machine.yaml (source-tier parity,
        // guarded by seed_outcome_predicate_parity.feature). `active` is
        // reachable only via an approver-signed proposal_review satisfied.
        outcome_predicate: Some(OutcomePredicate {
            terminal_state: "active".to_string(),
            check: Some(
                "the proposal reached active with an approver-signed proposal_review satisfied, so it is a governing direction that can spawn implementation tracks rather than an unreviewed draft"
                    .to_string(),
            ),
        }),
    }
}
