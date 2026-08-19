//! Compiled-in `PlaybookMachine` literal for the `playbook` kind.
//!
//! The `playbook` kind's own lifecycle per R3.1 (11 states) and R3.2 (14 transitions).
//! This seed is the source of truth for `describe(playbook)`, `begin(artifact_type:
//! "playbook", ...)`, and legacy `workflow` aliases.
//!
//! Unlike the track seed, this seed is fully populated where R3 specifies values.
//! State fields not covered by the interpreter are still populated here because
//! they encode the playbook kind's own known lifecycle (registry sections were
//! added by Phase 2's `registry_section_for` implementation).
//!
//! 11 states:
//!   draft, draft_review, draft_revision,
//!   active, amend, amend_review, amend_revision,
//!   reflecting, reflection_review, reflection_revision,
//!   retired
//!
//! 14 transitions per R3.2:
//!   draft → draft_review (doer)
//!   draft_review → draft_revision (doer)
//!   draft_review → active (reviewer, required_satisfaction: ["satisfied"])
//!   draft_revision → draft_review (doer)
//!   active → amend (doer)
//!   amend → amend_review (doer)
//!   amend_review → amend_revision (doer)
//!   amend_review → active (reviewer, required_satisfaction: ["satisfied"])
//!   amend_revision → amend_review (doer)
//!   active → reflecting (doer)
//!   reflecting → reflection_review (doer)
//!   reflection_review → reflection_revision (doer)
//!   reflection_review → retired (reviewer, required_satisfaction: ["satisfied"])
//!   reflection_revision → reflection_review (doer)
//!
//! Review gates (`is_review_gate: true`) on `draft_review`, `amend_review`,
//! `reflection_review`. Terminal: `retired` only. Role vocabulary: `["doer", "reviewer"]`.

use crate::domain::playbook::types::{
    Access, FieldDescriptor, PlaybookMachine, Register, RoleFilter, RouteConfig, StateDefinition,
    TransitionDefinition,
};

pub fn build() -> PlaybookMachine {
    PlaybookMachine {
        kind: "playbook".to_string(),
        directory: "playbooks".to_string(),
        registry: "playbooks.md".to_string(),
        parent_kind: Some("track".to_string()),
        parent_required: false,
        description: "Definition of another artifact kind's lifecycle (state machine + hooks)."
            .to_string(),
        // Foundation primitive. Must equal what the on-disk machine.yaml (which omits the
        // access block) parses to: the access field's serde default is `Access::foundation`
        // (org "Foundation"), not the derived Access::default. — playbook_seed_yaml_equivalence.
        access: Access::foundation(),
        owner_kit: String::new(),
        visibility: String::new(),
        route: RouteConfig {
            description: Some(
                "Route here when the user wants to DEFINE another artifact kind lifecycle (a playbook artifact under a track). NOT general feature implementation (track)."
                    .to_string(),
            ),
            triggers: vec![
                "define a playbook".to_string(),
                "new artifact kind".to_string(),
                "add a playbook".to_string(),
                "add a lifecycle type".to_string(),
                "define a lifecycle type".to_string(),
                // Legacy READ triggers. These match what a PERSON types, not what
                // Anvil calls the thing, and a person who learned the old word
                // must still land here. Same treatment as the
                // `playbook_generation` / `workflow_generation` kind alias and the
                // `playbook` / `workflow` artifact-kind arms: input recognition
                // stays, output vocabulary moves. Anvil emits neither.
                "define a workflow".to_string(),
                "add a workflow".to_string(),
            ],
        },
        projection_only: false,
        required_fields: vec![
            FieldDescriptor {
                name: "playbook_name".to_string(),
                field_type: "string".to_string(),
                description: "Name of the playbook (the kind it governs)".to_string(),
            },
            FieldDescriptor {
                name: "approver".to_string(),
                field_type: "actor_name".to_string(),
                description: "Human or reviewer authorizing creation".to_string(),
            },
        ],
        roles: vec!["doer".to_string(), "reviewer".to_string()],
        states: vec![
            StateDefinition {
                name: "draft".to_string(),
                role_filters: vec![RoleFilter::DoerActionable],
                registry_section: "draft".to_string(),
                projection_targets: vec![],
                is_review_gate: false,
                is_terminal: false,
                hook: None,
                hooks_by_role: std::collections::BTreeMap::new(),
                measurement_by_role: std::collections::BTreeMap::new(),
            },
            StateDefinition {
                name: "draft_review".to_string(),
                role_filters: vec![RoleFilter::ReviewPending],
                registry_section: "draft".to_string(),
                projection_targets: vec![],
                is_review_gate: true,
                is_terminal: false,
                hook: None,
                hooks_by_role: std::collections::BTreeMap::new(),
                measurement_by_role: std::collections::BTreeMap::new(),
            },
            StateDefinition {
                name: "draft_revision".to_string(),
                role_filters: vec![RoleFilter::DoerActionable],
                registry_section: "draft".to_string(),
                projection_targets: vec![],
                is_review_gate: false,
                is_terminal: false,
                hook: None,
                hooks_by_role: std::collections::BTreeMap::new(),
                measurement_by_role: std::collections::BTreeMap::new(),
            },
            StateDefinition {
                name: "active".to_string(),
                role_filters: vec![RoleFilter::DoerActionable],
                registry_section: "active".to_string(),
                projection_targets: vec![],
                is_review_gate: false,
                is_terminal: false,
                hook: None,
                hooks_by_role: std::collections::BTreeMap::new(),
                measurement_by_role: std::collections::BTreeMap::new(),
            },
            StateDefinition {
                name: "amend".to_string(),
                role_filters: vec![RoleFilter::DoerActionable],
                registry_section: "active".to_string(),
                projection_targets: vec![],
                is_review_gate: false,
                is_terminal: false,
                hook: None,
                hooks_by_role: std::collections::BTreeMap::new(),
                measurement_by_role: std::collections::BTreeMap::new(),
            },
            StateDefinition {
                name: "amend_review".to_string(),
                role_filters: vec![RoleFilter::ReviewPending],
                registry_section: "active".to_string(),
                projection_targets: vec![],
                is_review_gate: true,
                is_terminal: false,
                hook: None,
                hooks_by_role: std::collections::BTreeMap::new(),
                measurement_by_role: std::collections::BTreeMap::new(),
            },
            StateDefinition {
                name: "amend_revision".to_string(),
                role_filters: vec![RoleFilter::DoerActionable],
                registry_section: "active".to_string(),
                projection_targets: vec![],
                is_review_gate: false,
                is_terminal: false,
                hook: None,
                hooks_by_role: std::collections::BTreeMap::new(),
                measurement_by_role: std::collections::BTreeMap::new(),
            },
            StateDefinition {
                name: "reflecting".to_string(),
                role_filters: vec![RoleFilter::DoerActionable],
                registry_section: "reflecting".to_string(),
                projection_targets: vec![],
                is_review_gate: false,
                is_terminal: false,
                hook: None,
                hooks_by_role: std::collections::BTreeMap::new(),
                measurement_by_role: std::collections::BTreeMap::new(),
            },
            StateDefinition {
                name: "reflection_review".to_string(),
                role_filters: vec![RoleFilter::ReviewPending],
                registry_section: "reflecting".to_string(),
                projection_targets: vec![],
                is_review_gate: true,
                is_terminal: false,
                hook: None,
                hooks_by_role: std::collections::BTreeMap::new(),
                measurement_by_role: std::collections::BTreeMap::new(),
            },
            StateDefinition {
                name: "reflection_revision".to_string(),
                role_filters: vec![RoleFilter::DoerActionable],
                registry_section: "reflecting".to_string(),
                projection_targets: vec![],
                is_review_gate: false,
                is_terminal: false,
                hook: None,
                hooks_by_role: std::collections::BTreeMap::new(),
                measurement_by_role: std::collections::BTreeMap::new(),
            },
            StateDefinition {
                name: "retired".to_string(),
                role_filters: vec![RoleFilter::Terminal],
                registry_section: "retired".to_string(),
                projection_targets: vec![],
                is_review_gate: false,
                is_terminal: true,
                hook: None,
                hooks_by_role: std::collections::BTreeMap::new(),
                measurement_by_role: std::collections::BTreeMap::new(),
            },
        ],
        // 14 transitions per R3.2
        transitions: vec![
            // draft → draft_review (doer)
            TransitionDefinition {
                from_state: "draft".to_string(),
                to_state: "draft_review".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: None,
                requires_approver: false,
                hook: None,
            },
            // draft_review → draft_revision (doer)
            TransitionDefinition {
                from_state: "draft_review".to_string(),
                to_state: "draft_revision".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: Some(vec!["needs_revision".to_string()]),
                requires_approver: false,
                hook: None,
            },
            // draft_review → active (reviewer, satisfied)
            TransitionDefinition {
                from_state: "draft_review".to_string(),
                to_state: "active".to_string(),
                required_role: "reviewer".to_string(),
                required_satisfaction: Some(vec!["satisfied".to_string()]),
                requires_approver: true,
                hook: None,
            },
            // draft_revision → draft_review (doer)
            TransitionDefinition {
                from_state: "draft_revision".to_string(),
                to_state: "draft_review".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: None,
                requires_approver: false,
                hook: None,
            },
            // active → amend (doer)
            TransitionDefinition {
                from_state: "active".to_string(),
                to_state: "amend".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: None,
                requires_approver: false,
                hook: None,
            },
            // amend → amend_review (doer)
            TransitionDefinition {
                from_state: "amend".to_string(),
                to_state: "amend_review".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: None,
                requires_approver: false,
                hook: None,
            },
            // amend_review → amend_revision (doer)
            TransitionDefinition {
                from_state: "amend_review".to_string(),
                to_state: "amend_revision".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: Some(vec!["needs_revision".to_string()]),
                requires_approver: false,
                hook: None,
            },
            // amend_review → active (reviewer, satisfied)
            TransitionDefinition {
                from_state: "amend_review".to_string(),
                to_state: "active".to_string(),
                required_role: "reviewer".to_string(),
                required_satisfaction: Some(vec!["satisfied".to_string()]),
                requires_approver: true,
                hook: None,
            },
            // amend_revision → amend_review (doer)
            TransitionDefinition {
                from_state: "amend_revision".to_string(),
                to_state: "amend_review".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: None,
                requires_approver: false,
                hook: None,
            },
            // active → reflecting (doer)
            TransitionDefinition {
                from_state: "active".to_string(),
                to_state: "reflecting".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: None,
                requires_approver: false,
                hook: None,
            },
            // reflecting → reflection_review (doer)
            TransitionDefinition {
                from_state: "reflecting".to_string(),
                to_state: "reflection_review".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: None,
                requires_approver: false,
                hook: None,
            },
            // reflection_review → reflection_revision (doer)
            TransitionDefinition {
                from_state: "reflection_review".to_string(),
                to_state: "reflection_revision".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: Some(vec!["needs_revision".to_string()]),
                requires_approver: false,
                hook: None,
            },
            // reflection_review → retired (reviewer, satisfied)
            TransitionDefinition {
                from_state: "reflection_review".to_string(),
                to_state: "retired".to_string(),
                required_role: "reviewer".to_string(),
                required_satisfaction: Some(vec!["satisfied".to_string()]),
                requires_approver: true,
                hook: None,
            },
            // reflection_revision → reflection_review (doer)
            TransitionDefinition {
                from_state: "reflection_revision".to_string(),
                to_state: "reflection_review".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: None,
                requires_approver: false,
                hook: None,
            },
        ],
        register: Register::Driven,
        success_rubric: None,
        outcome_predicate: None,
    }
}
