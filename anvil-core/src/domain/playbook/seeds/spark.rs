//! Compiled-in `PlaybookMachine` literal for the `spark` kind.
//!
//! Sparks are free and projection-only: `begin(artifact_type: "spark")`
//! captures an out-of-band idea into the sparks event log and projection
//! without scaffolding an artifact directory or status.yaml.

use crate::domain::playbook::types::{
    Access, FieldDescriptor, MeasurementSpec, OutcomePredicate, PlaybookMachine, Register,
    RoleFilter, RouteConfig, StateDefinition,
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
    success_criteria: &str,
) -> std::collections::BTreeMap<String, MeasurementSpec> {
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        role.to_string(),
        MeasurementSpec {
            intent: intent.to_string(),
            expected_output: expected_output.to_string(),
            success_criteria: Some(success_criteria.to_string()),
            evidence_obligation: Vec::new(),
        },
    );
    m
}

pub fn build() -> PlaybookMachine {
    PlaybookMachine {
        kind: "spark".to_string(),
        directory: "sparks".to_string(),
        registry: "sparks.md".to_string(),
        parent_kind: None,
        parent_required: true,
        description: "Projection-only capture of an out-of-band idea.".to_string(),
        access: Access::foundation(),
        owner_kit: String::new(),
        visibility: String::new(),
        route: RouteConfig::default(),
        projection_only: true,
        required_fields: vec![FieldDescriptor {
            name: "name".to_string(),
            field_type: "string".to_string(),
            description: "Spark body or idea text".to_string(),
        }],
        roles: vec!["doer".to_string()],
        states: vec![StateDefinition {
            name: "captured".to_string(),
            role_filters: vec![RoleFilter::DoerActionable],
            registry_section: "captured".to_string(),
            projection_targets: vec!["sparks.md".to_string()],
            is_review_gate: false,
            // Spark is fire-and-forget: `captured` is its single, final resting
            // state with no follow-on transition. It is terminal so the
            // contiguity gate accepts it (a non-terminal state must have an
            // outgoing transition; capture has none by design).
            is_terminal: true,
            hook: None,
            hooks_by_role: hooks_by_role("doer", "capture.md"),
            measurement_by_role: measurement_by_role(
                "doer",
                "Capture the spark as an out-of-band idea without creating a lifecycle artifact.",
                "An appended spark event in sparks/sparks.md and an updated sparks projection.",
                "A single spark entry is appended to `sparks/sparks.md` carrying the idea text verbatim, with zero lifecycle artifact (no proposal, track, or decision) created and zero edits to any other artifact's files — capture stays fire-and-forget.",
            ),
        }],
        transitions: Vec::new(),
        register: Register::Free,
        success_rubric: None,
        // Mirrors playbooks/spark_lifecycle/machine.yaml (source-tier parity,
        // guarded by seed_outcome_predicate_parity.feature). Each spark capture
        // is folded under its own unique instance id (see the engine's begin
        // emit path), so `captured` reach is a per-capture checkable fact.
        outcome_predicate: Some(OutcomePredicate {
            terminal_state: "captured".to_string(),
            check: Some(
                "the spark reached captured as an appended sparks.md entry with no lifecycle artifact spawned"
                    .to_string(),
            ),
        }),
    }
}
