//! Compiled-in `PlaybookMachine` literal for the `track` kind.
//!
//! Partial-seed-population discipline (per plan.md Phase 3 "Locked" rule):
//!   - `name` and `is_terminal` are fully populated on every state.
//!   - All other state fields are zero-valued with a comment marking them for
//!     the per-consumer migration that populates them:
//!     - `role_filters: vec![]`
//!     - `registry_section: ""` except for complete-flow registry moves
//!     - `projection_targets: vec![]` except for complete-flow execution labels
//!     - `is_review_gate: false` (true only where the interpreter needs it)
//!     - `hook: None`
//!
//! Transitions fully populate the interpreter-visible axis:
//!   `from_state`, `to_state`, `required_role`
//! Other transition fields are zero-valued:
//!   required_satisfaction: None, requires_approver: false, hook: None
//!
//! These zero-values are INTENTIONAL. Do not fill them in unless you are the
//! migration track that owns the consumer that reads them.
//!
//! Track states — 16 total (13 non-terminal + 3 terminal):
//!   spec, spec_review, spec_revision,
//!   plan, plan_review, plan_revision,
//!   implementing, impl_phase_review, impl_review, impl_revision,
//!   reflecting, reflection_review, reflection_revision,
//!   completed, abandoned, superseded
//!
//! Transitions — mirrors describe::available_actions("track", _) exactly
//! (lines 92–107 of anvil-core/src/domain/describe.rs) per plan canary.
//! Terminal-state exits are intentionally omitted per plan.md Phase 3.

use crate::domain::playbook::types::{
    Access, AnchorRef, EvidenceClass, FieldDescriptor, MeasurementSpec, OutcomePredicate,
    PlaybookMachine, Register, RouteConfig, RubricDimension, StateDefinition, SuccessRubric,
    TransitionDefinition,
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
    // Thin delegator: the zero-obligation default. Keeping this signature and
    // its empty-obligation behavior unchanged means every existing call site
    // (the 12 measured pairs that carry no obligation) is byte-identical.
    measurement_by_role_with_evidence(role, intent, expected_output, success_criteria, Vec::new())
}

fn measurement_by_role_with_evidence(
    role: &str,
    intent: &str,
    expected_output: &str,
    success_criteria: &str,
    evidence_obligation: Vec<EvidenceClass>,
) -> std::collections::BTreeMap<String, MeasurementSpec> {
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        role.to_string(),
        MeasurementSpec {
            intent: intent.to_string(),
            expected_output: expected_output.to_string(),
            success_criteria: Some(success_criteria.to_string()),
            evidence_obligation,
        },
    );
    m
}

pub fn build() -> PlaybookMachine {
    PlaybookMachine {
        kind: "track".to_string(),
        directory: "tracks".to_string(),
        registry: "tracks.md".to_string(),
        parent_kind: Some("proposal".to_string()),
        parent_required: true,
        description: "Concrete implementation of a proposal slice.".to_string(),
        // Foundation primitive. Must equal what the on-disk machine.yaml (which omits the
        // access block) parses to: the access field's serde default is `Access::foundation`
        // (org "Foundation"), NOT the derived Access::default (org ""). Use foundation() so
        // the seed stays structurally equal to the fixture — playbook_seed_yaml_equivalence.
        access: Access::foundation(),
        owner_kit: String::new(),
        visibility: String::new(),
        route: RouteConfig {
            // Fitted to the router's 220-char prompt cap, with the exclusion terms
            // preserved (anvil-hearth 2e6bf98 + c1cd60a, 2026-07-25). Those two commits
            // edited the PRODUCTION hearth copy only, so the seed kept the older 313-char
            // text and `track_copies_parity::production_hearth_copy_equals_seed` went red.
            // The guard's suggested remedy — regenerate the hearth FROM the seed — would
            // have reverted the cap fix on the copy the live engine actually reads. The
            // seed is the source of truth, so the fix is to bring the measured text HERE.
            description: Some(
                "Route here to IMPLEMENT, build, add, wire, fix, or ship a feature, code change, or bug fix — engineering work to do now. NOT a playbook definition (playbook_generation); NOT questions, debugging, status, or discussion."
                    .to_string(),
            ),
            // 6 base triggers + the 5 curated `#003b` recall-tuned triggers,
            // propagated verbatim (and in order) from the production hearth copy
            // by the track-lifecycle satisfaction-encoding reconciliation so all
            // four copies are route-identical. Order is load-bearing: RouteConfig
            // compares triggers as an order-sensitive Vec.
            triggers: vec![
                "new track".to_string(),
                "start a track".to_string(),
                "create a track".to_string(),
                "begin work on a track".to_string(),
                "implement a feature".to_string(),
                "start a feature".to_string(),
                "start a new track".to_string(),
                "begin work on".to_string(),
                "implement the feature".to_string(),
                "add support for".to_string(),
                "start implementing".to_string(),
            ],
        },
        projection_only: false,
        required_fields: vec![
            FieldDescriptor {
                name: "name".to_string(),
                field_type: "string".to_string(),
                description: "Track name".to_string(),
            },
            FieldDescriptor {
                name: "parent_id".to_string(),
                field_type: "artifact_id".to_string(),
                description: "Parent proposal id".to_string(),
            },
            FieldDescriptor {
                name: "approver".to_string(),
                field_type: "actor_name".to_string(),
                description: "Human or reviewer authorizing creation".to_string(),
            },
        ],
        // Roles derived from the action() entries in describe.rs.
        // "reviewer" is the designated review-role; the phase-skill names
        // ("spec", "plan", "implement", "reflect", "complete") are roles the
        // doer uses to signal which lifecycle phase they are in.
        // "doer" added in P1 (hook_content_serving) so hooks_by_role maps with
        // key "doer" pass loader role-key validation in P3/P4 fixtures.
        roles: vec![
            "doer".to_string(),
            "spec".to_string(),
            "plan".to_string(),
            "implement".to_string(),
            "reflect".to_string(),
            "complete".to_string(),
            "review".to_string(),
            "reviewer".to_string(),
        ],
        states: vec![
            // --- Non-terminal states ---
            StateDefinition {
                name: "spec".to_string(),
                // populated when the per-consumer migration lands
                role_filters: vec![],
                registry_section: String::new(),
                projection_targets: vec![],
                is_review_gate: false,
                is_terminal: false,
                hook: None,
                // P5: the (spec, doer) hook serves spec-writing.md. Must stay in
                // lock-step with the on-disk machine.yaml and the equality fixture.
                hooks_by_role: {
                    let mut m = std::collections::BTreeMap::new();
                    m.insert("doer".to_string(), "spec-writing.md".to_string());
                    m
                },
                // M-P3: (spec, doer) measurement. Must stay in lock-step with the
                // on-disk machine.yaml, kit-source machine.yaml, and the equality fixture.
                measurement_by_role: {
                    let mut m = std::collections::BTreeMap::new();
                    m.insert(
                        "doer".to_string(),
                        MeasurementSpec {
                            intent: "Translate the approved proposal into a concrete, testable spec: define what to build and why, with acceptance criteria a reviewer can check.".to_string(),
                            expected_output: "A spec.md stating scope, acceptance criteria as brine-checkable statements, and explicit out-of-scope boundaries.".to_string(),
                            success_criteria: Some(
                                "A `spec.md` states scope, at least 3 acceptance criteria written as falsifiable brine-checkable statements (each naming a threshold, an error code, or a zero-case), and an explicit out-of-scope list naming each exclusion's owning future track.".to_string(),
                            ),
                            // T-ACT-1: spec/doer produces spec.md — a landed,
                            // review-gated artifact whose existence is the proof.
                            evidence_obligation: vec![EvidenceClass::ArtifactOfConsequence],
                        },
                    );
                    m
                },
            },
            StateDefinition {
                name: "spec_review".to_string(),
                role_filters: vec![],
                // F2.2: complete-flow registry/projection routing, mirroring
                // the former engine fallback exactly.
                registry_section: "spec_review".to_string(),
                projection_targets: vec!["Spec Review".to_string()],
                is_review_gate: true,
                is_terminal: false,
                hook: None,
                // P5: the (spec_review, reviewer) hook serves spec-review.md. Must
                // stay in lock-step with the on-disk machine.yaml and the fixture.
                hooks_by_role: {
                    let mut m = std::collections::BTreeMap::new();
                    m.insert("reviewer".to_string(), "spec-review.md".to_string());
                    m
                },
                // M-P3: (spec_review, reviewer) measurement. Must stay in lock-step
                // with the on-disk machine.yaml, kit-source machine.yaml, and fixture.
                measurement_by_role: {
                    let mut m = std::collections::BTreeMap::new();
                    m.insert(
                        "reviewer".to_string(),
                        MeasurementSpec {
                            intent: "Judge whether the spec's acceptance criteria are complete, unambiguous, and faithful to the proposal before plan work begins.".to_string(),
                            expected_output: "A spec.review.md verdict (satisfied or findings) citing each acceptance criterion gap or confirming coverage.".to_string(),
                            success_criteria: Some(
                                "A `spec.review.md` verdict marks satisfied or full_revision and, when full_revision, cites at least 1 specific acceptance-criterion gap by name; zero acceptance criteria are approved unchecked.".to_string(),
                            ),
                            evidence_obligation: Vec::new(),
                        },
                    );
                    m
                },
            },
            StateDefinition {
                name: "spec_revision".to_string(),
                role_filters: vec![],
                registry_section: String::new(),
                projection_targets: vec![],
                is_review_gate: false,
                is_terminal: false,
                hook: None,
                // Slice B: the spec_revision doer hook serves revision-specific
                // context (spec-revision.md), distinct from the spec-creation
                // context (spec-writing.md) and the reviewer context
                // (spec-review.md).
                hooks_by_role: hooks_by_role("doer", "spec-revision.md"),
                measurement_by_role: measurement_by_role(
                    "doer",
                    "Revise spec.md in response to review findings while preserving approved scope and recording explicit dispositions.",
                    "An updated spec.md plus a timestamped spec.review.md response disposing every finding.",
                    "An updated `spec.md` plus a `spec.review.md` response that dispositions every finding (addressed or rejected-with-rationale), leaving zero findings undispositioned.",
                ),
            },
            StateDefinition {
                name: "plan".to_string(),
                role_filters: vec![],
                // F2.2: complete-flow registry/projection routing, mirroring
                // the former engine fallback exactly.
                registry_section: "plan".to_string(),
                projection_targets: vec!["Planned".to_string()],
                is_review_gate: false,
                is_terminal: false,
                hook: None,
                hooks_by_role: hooks_by_role("doer", "plan-writing.md"),
                // T-ACT-1: plan/doer produces plan.md — itself a landed artifact
                // of consequence AND whose success criterion demands at least 5
                // real file:line citations (a genuine verifiable_citation leg).
                measurement_by_role: measurement_by_role_with_evidence(
                    "doer",
                    "Research the codebase and translate the approved spec into a concrete phased implementation plan.",
                    "A plan.md with file-grounded phases, pending tasks, checkpoints, and coverage for every accepted requirement.",
                    "A `plan.md` opens with an audit section citing at least 5 real file:line locations grounding its codebase claims, phases the work with a test named per phase, and covers every accepted spec acceptance criterion.",
                    vec![
                        EvidenceClass::VerifiableCitation,
                        EvidenceClass::ArtifactOfConsequence,
                    ],
                ),
            },
            StateDefinition {
                name: "plan_review".to_string(),
                role_filters: vec![],
                registry_section: String::new(),
                projection_targets: vec![],
                is_review_gate: true,
                is_terminal: false,
                hook: None,
                hooks_by_role: hooks_by_role("reviewer", "plan-review.md"),
                measurement_by_role: measurement_by_role(
                    "reviewer",
                    "Judge whether plan.md fully covers the spec with feasible, well-ordered, codebase-grounded tasks.",
                    "A plan.review.md verdict with findings for coverage, feasibility, ordering, completeness, and architecture gaps.",
                    "A `plan.review.md` verdict marks satisfied or full_revision, and any full_revision cites at least 1 concrete coverage, feasibility, ordering, or architecture gap tied to a specific plan phase.",
                ),
            },
            StateDefinition {
                name: "plan_revision".to_string(),
                role_filters: vec![],
                registry_section: String::new(),
                projection_targets: vec![],
                is_review_gate: false,
                is_terminal: false,
                hook: None,
                hooks_by_role: hooks_by_role("doer", "plan-writing.md"),
                measurement_by_role: measurement_by_role(
                    "doer",
                    "Revise plan.md in response to plan-review findings while preserving spec coverage and task clarity.",
                    "An updated plan.md plus a timestamped plan.review.md response disposing every finding.",
                    "An updated `plan.md` plus a `plan.review.md` response dispositioning every finding, with zero findings left open and spec coverage preserved.",
                ),
            },
            StateDefinition {
                name: "implementing".to_string(),
                role_filters: vec![],
                registry_section: String::new(),
                projection_targets: vec![],
                is_review_gate: false,
                is_terminal: false,
                hook: None,
                hooks_by_role: hooks_by_role("doer", "implementing.md"),
                measurement_by_role: measurement_by_role(
                    "doer",
                    "Execute plan tasks through Brine-first test-driven implementation while keeping plan.md current.",
                    "Passing feature coverage and implementation commits for completed tasks, with plan.md task status updated.",
                    "Every completed plan task passes its brine feature scenario (`brine run .` green) and `plan.md` task-status checkboxes are updated to reflect completed tasks, with zero tasks left silently undone.",
                ),
            },
            StateDefinition {
                name: "impl_phase_review".to_string(),
                role_filters: vec![],
                registry_section: String::new(),
                projection_targets: vec![],
                is_review_gate: true,
                is_terminal: false,
                hook: None,
                hooks_by_role: hooks_by_role("reviewer", "impl-phase-review.md"),
                measurement_by_role: measurement_by_role(
                    "reviewer",
                    "Review completed implementation phases for plan conformance, initiative regressions, and vertical-slice integrity.",
                    "An impl.phase.review.md entry with calibrated findings for the completed phase and any risks to later phases.",
                    "An `impl.phase.review.md` entry marks satisfied or full_revision for the reviewed phase and, when full_revision, names at least 1 specific defect or regression risk tied to a plan phase.",
                ),
            },
            StateDefinition {
                name: "impl_review".to_string(),
                role_filters: vec![],
                registry_section: String::new(),
                projection_targets: vec![],
                is_review_gate: true,
                is_terminal: false,
                hook: None,
                hooks_by_role: hooks_by_role("reviewer", "impl-review.md"),
                measurement_by_role: measurement_by_role(
                    "reviewer",
                    "Perform the final whole-track implementation review against spec, plan, tests, and unresolved findings.",
                    "An impl.review.md verdict covering intent compliance, correctness, test coverage, integration, and AC closure.",
                    "An `impl.review.md` verdict marks satisfied or full_revision covering intent compliance, correctness, test coverage, and acceptance-criteria closure, with every finding SEVERITY-tagged (P1 or nit) and zero acceptance criteria left unaddressed.",
                ),
            },
            StateDefinition {
                name: "impl_revision".to_string(),
                role_filters: vec![],
                registry_section: String::new(),
                projection_targets: vec![],
                is_review_gate: false,
                is_terminal: false,
                hook: None,
                hooks_by_role: hooks_by_role("doer", "implementing.md"),
                measurement_by_role: measurement_by_role(
                    "doer",
                    "Address final implementation review findings through Brine-first changes without skipping explicit dispositions.",
                    "Updated implementation and feature coverage plus a timestamped impl.review.md response disposing every finding.",
                    "Updated implementation with all previously failing brine scenarios now green (`brine run .`), plus a timestamped `impl.review.md` response dispositioning every finding, leaving zero findings open.",
                ),
            },
            StateDefinition {
                name: "reflecting".to_string(),
                role_filters: vec![],
                registry_section: String::new(),
                projection_targets: vec![],
                is_review_gate: false,
                is_terminal: false,
                hook: None,
                hooks_by_role: hooks_by_role("doer", "reflecting.md"),
                measurement_by_role: measurement_by_role(
                    "doer",
                    "Write the semantic reflection capturing what changed in understanding across intent, reality, exceptions, and projections.",
                    "A reflection.md that addresses every required delta lens and records actionable future assumptions.",
                    "A `reflection.md` names what changed in understanding (intent-delta vs reality-delta), owns at least 1 thing that went wrong honestly, and extracts a durable rule — not a restatement of the plan or spec in past tense.",
                ),
            },
            StateDefinition {
                name: "reflection_review".to_string(),
                role_filters: vec![],
                registry_section: String::new(),
                projection_targets: vec![],
                is_review_gate: true,
                is_terminal: false,
                hook: None,
                hooks_by_role: {
                    let mut m = std::collections::BTreeMap::new();
                    m.insert("reviewer".to_string(), "reflection-review.md".to_string());
                    m.insert("complete".to_string(), "complete.md".to_string());
                    m
                },
                measurement_by_role: {
                    let mut m = std::collections::BTreeMap::new();
                    m.insert(
                        "reviewer".to_string(),
                        MeasurementSpec {
                            intent: "Review reflection.md for accurate semantic deltas before its claims feed projections.".to_string(),
                            expected_output: "A reflection.review.md verdict confirming or challenging intent, reality, exception, and projection deltas.".to_string(),
                            success_criteria: Some(
                                "A `reflection.review.md` verdict marks satisfied or full_revision and, when full_revision, names at least 1 specific delta claim (intent, reality, exception, or projection) that the reflection got wrong or omitted.".to_string(),
                            ),
                            evidence_obligation: Vec::new(),
                        },
                    );
                    m.insert(
                        "complete".to_string(),
                        MeasurementSpec {
                            intent: "Verify the artifact is administratively complete after accepted reflection review and prepare terminal closure.".to_string(),
                            expected_output: "A completed transition with required sign-offs verified and projection rebuild obligations identified.".to_string(),
                            success_criteria: Some(
                                "A `completed` transition record with the reflection-review satisfied sign-off verified, zero open findings remaining, and any projection rebuild obligation named explicitly (or `none`).".to_string(),
                            ),
                            evidence_obligation: Vec::new(),
                        },
                    );
                    m
                },
            },
            StateDefinition {
                name: "reflection_revision".to_string(),
                role_filters: vec![],
                registry_section: String::new(),
                projection_targets: vec![],
                is_review_gate: false,
                is_terminal: false,
                hook: None,
                hooks_by_role: hooks_by_role("doer", "reflecting.md"),
                measurement_by_role: measurement_by_role(
                    "doer",
                    "Revise reflection.md in response to reflection-review findings while keeping every delta lens accurate.",
                    "An updated reflection.md plus a timestamped reflection.review.md response disposing every finding.",
                    "An updated `reflection.md` plus a timestamped `reflection.review.md` response dispositioning every finding, with zero delta lenses left inaccurate.",
                ),
            },
            // --- Amend loop (B5b BP4): anchored on `completed`, mirroring the
            // playbook seed's amend-loop structure. Partial-seed discipline: the
            // amend-loop states keep zero-valued role_filters / registry_section /
            // projection_targets like the rest of the track seed. ---
            StateDefinition {
                name: "amend".to_string(),
                role_filters: vec![],
                registry_section: String::new(),
                projection_targets: vec![],
                is_review_gate: false,
                is_terminal: false,
                hook: None,
                hooks_by_role: hooks_by_role("doer", "amend-writing.md"),
                measurement_by_role: measurement_by_role(
                    "doer",
                    "Append an amendment to the correct frozen-artifact amendment log without editing the frozen source.",
                    "A numbered append-only amendment entry with context, amendment text, and rationale in the routed amendments file.",
                    "A numbered append-only entry in the correct `*.amendments.md` file naming the frozen artifact, the amendment text, and a rationale, with the frozen source file itself unedited (zero diff outside the amendments log).",
                ),
            },
            StateDefinition {
                name: "amend_review".to_string(),
                role_filters: vec![],
                registry_section: String::new(),
                projection_targets: vec![],
                is_review_gate: true,
                is_terminal: false,
                hook: None,
                hooks_by_role: hooks_by_role("reviewer", "amend-review.md"),
                measurement_by_role: measurement_by_role(
                    "reviewer",
                    "Review an amendment for necessity, accuracy, scope, append-only discipline, downstream effects, and approval needs.",
                    "A review entry in the phase's existing review document with findings or sign-off for the amendment.",
                    "A review entry appended to the phase's existing review document marks satisfied or full_revision and, when full_revision, names at least 1 necessity, scope, or append-only-discipline gap.",
                ),
            },
            StateDefinition {
                name: "amend_revision".to_string(),
                role_filters: vec![],
                registry_section: String::new(),
                projection_targets: vec![],
                is_review_gate: false,
                is_terminal: false,
                hook: None,
                hooks_by_role: hooks_by_role("doer", "amend-writing.md"),
                measurement_by_role: measurement_by_role(
                    "doer",
                    "Revise an amendment in response to review findings using append-only revision notes.",
                    "An updated amendments log plus a timestamped review response disposing every finding.",
                    "An updated amendments log entry plus a timestamped review response dispositioning every finding, with zero findings left unaddressed and no edits to the frozen source.",
                ),
            },
            // --- Terminal states (is_terminal: true) ---
            // B5b BP4: `completed` is now NON-terminal — a completed track can
            // enter the amend loop via `completed → amend`. `abandoned` and
            // `superseded` remain terminal.
            StateDefinition {
                name: "completed".to_string(),
                // populated when the per-consumer migration lands
                role_filters: vec![],
                registry_section: String::new(),
                projection_targets: vec![],
                is_review_gate: false,
                is_terminal: false,
                hook: None,
                hooks_by_role: std::collections::BTreeMap::new(),
                measurement_by_role: std::collections::BTreeMap::new(),
            },
            StateDefinition {
                name: "abandoned".to_string(),
                // populated when the per-consumer migration lands
                role_filters: vec![],
                registry_section: String::new(),
                projection_targets: vec![],
                is_review_gate: false,
                is_terminal: true,
                hook: None,
                hooks_by_role: std::collections::BTreeMap::new(),
                measurement_by_role: std::collections::BTreeMap::new(),
            },
            StateDefinition {
                name: "superseded".to_string(),
                // populated when the per-consumer migration lands
                role_filters: vec![],
                registry_section: String::new(),
                projection_targets: vec![],
                is_review_gate: false,
                is_terminal: true,
                hook: None,
                hooks_by_role: std::collections::BTreeMap::new(),
                measurement_by_role: std::collections::BTreeMap::new(),
            },
        ],
        // Transitions mirror describe::available_actions("track", state) exactly.
        // Each action("to_state", "role") call in describe.rs becomes one
        // TransitionDefinition here. Ordering within each from_state group is
        // preserved from the describe.rs vec literal order.
        //
        // Fields zero-valued per partial-seed discipline:
        //   required_satisfaction: None
        //   requires_approver: false
        //   hook: None
        transitions: vec![
            // spec → spec_review (role "spec" — doer complete path)
            TransitionDefinition {
                from_state: "spec".to_string(),
                to_state: "spec_review".to_string(),
                required_role: "spec".to_string(),
                required_satisfaction: None,
                requires_approver: false,
                hook: None,
            },
            // spec_review → spec_revision (reviewer drives revision via
            // satisfaction "full_revision" — Slice B). The reviewer call is
            // satisfaction-discriminated; the recorded transition role is
            // "review" (the track reviewer convention in complete.rs).
            TransitionDefinition {
                from_state: "spec_review".to_string(),
                to_state: "spec_revision".to_string(),
                required_role: "reviewer".to_string(),
                required_satisfaction: Some(vec!["full_revision".to_string()]),
                requires_approver: false,
                hook: None,
            },
            // spec_review → plan (role "reviewer", advance via satisfaction
            // "satisfied"). Honest-gate token added by the track-lifecycle
            // satisfaction-encoding reconciliation: `select_edge`'s
            // `by_satisfaction` now matches this edge directly, so the former
            // satisfied-compat Guard 1 (complete.rs:91) is mostly-dead-but-harmless.
            TransitionDefinition {
                from_state: "spec_review".to_string(),
                to_state: "plan".to_string(),
                required_role: "reviewer".to_string(),
                required_satisfaction: Some(vec!["satisfied".to_string()]),
                requires_approver: false,
                hook: None,
            },
            // spec_review → plan (role "reviewer", carry-forward via
            // satisfaction "address_in_next_step" — Slice C). Same destination
            // as the satisfied edge (the reviewer judged the spec non-blocking),
            // but a distinct satisfaction-discriminated edge so the recorded
            // transition carries `satisfaction: address_in_next_step` in its
            // metadata. select_edge matches this by its satisfaction set; the
            // None-satisfaction edge above stays the satisfied-compat target.
            TransitionDefinition {
                from_state: "spec_review".to_string(),
                to_state: "plan".to_string(),
                required_role: "reviewer".to_string(),
                required_satisfaction: Some(vec!["address_in_next_step".to_string()]),
                requires_approver: false,
                hook: None,
            },
            // spec_revision → spec_review (role "spec" — doer complete path,
            // Slice B). The doer's no-satisfaction complete advances revision
            // back to review; the recorded transition role is the edge role
            // "spec".
            TransitionDefinition {
                from_state: "spec_revision".to_string(),
                to_state: "spec_review".to_string(),
                required_role: "spec".to_string(),
                required_satisfaction: None,
                requires_approver: false,
                hook: None,
            },
            // plan → plan_review (role "plan" — doer complete path)
            TransitionDefinition {
                from_state: "plan".to_string(),
                to_state: "plan_review".to_string(),
                required_role: "plan".to_string(),
                required_satisfaction: None,
                requires_approver: false,
                hook: None,
            },
            // plan_review → plan_revision (role "plan", revision via satisfaction
            // "full_revision" — honest-gate token).
            TransitionDefinition {
                from_state: "plan_review".to_string(),
                to_state: "plan_revision".to_string(),
                required_role: "plan".to_string(),
                required_satisfaction: Some(vec!["full_revision".to_string()]),
                requires_approver: false,
                hook: None,
            },
            // plan_review → implementing (role "implement", advance via
            // satisfaction "satisfied" — honest-gate token).
            TransitionDefinition {
                from_state: "plan_review".to_string(),
                to_state: "implementing".to_string(),
                required_role: "implement".to_string(),
                required_satisfaction: Some(vec!["satisfied".to_string()]),
                requires_approver: false,
                hook: None,
            },
            // plan_revision → plan_review (role "reviewer")
            TransitionDefinition {
                from_state: "plan_revision".to_string(),
                to_state: "plan_review".to_string(),
                required_role: "reviewer".to_string(),
                required_satisfaction: None,
                requires_approver: false,
                hook: None,
            },
            // implementing → impl_phase_review (role "reviewer")
            TransitionDefinition {
                from_state: "implementing".to_string(),
                to_state: "impl_phase_review".to_string(),
                required_role: "reviewer".to_string(),
                required_satisfaction: None,
                requires_approver: false,
                hook: None,
            },
            // implementing → impl_review (role "implement" — doer complete path)
            TransitionDefinition {
                from_state: "implementing".to_string(),
                to_state: "impl_review".to_string(),
                required_role: "implement".to_string(),
                required_satisfaction: None,
                requires_approver: false,
                hook: None,
            },
            // impl_phase_review → implementing (role "implement", advance via
            // satisfaction "satisfied" — honest-gate token).
            TransitionDefinition {
                from_state: "impl_phase_review".to_string(),
                to_state: "implementing".to_string(),
                required_role: "implement".to_string(),
                required_satisfaction: Some(vec!["satisfied".to_string()]),
                requires_approver: false,
                hook: None,
            },
            // impl_phase_review → implementing (role "implement", revision via
            // satisfaction "full_revision" — Option B: a phase-review reviewer who
            // finds the phase deficient records an honest `full_revision` verdict
            // that routes back into `implementing` (no new state). Placed in the
            // FORWARD block BEFORE the park edge so the order-sensitive Vec
            // equality against the fixture holds; `dedup_actions` (keyed on
            // to_state+role) collapses this against the `satisfied` edge, so
            // `available_actions` for impl_phase_review still yields 2 actions.
            TransitionDefinition {
                from_state: "impl_phase_review".to_string(),
                to_state: "implementing".to_string(),
                required_role: "implement".to_string(),
                required_satisfaction: Some(vec!["full_revision".to_string()]),
                requires_approver: false,
                hook: None,
            },
            // impl_review → impl_revision (role "implement")
            TransitionDefinition {
                from_state: "impl_review".to_string(),
                to_state: "impl_revision".to_string(),
                required_role: "implement".to_string(),
                required_satisfaction: Some(vec!["full_revision".to_string()]),
                requires_approver: false,
                hook: None,
            },
            // impl_review → reflecting (role "reflect")
            TransitionDefinition {
                from_state: "impl_review".to_string(),
                to_state: "reflecting".to_string(),
                required_role: "reflect".to_string(),
                required_satisfaction: Some(vec!["satisfied".to_string()]),
                requires_approver: false,
                hook: None,
            },
            // impl_revision → impl_review (role "reviewer")
            TransitionDefinition {
                from_state: "impl_revision".to_string(),
                to_state: "impl_review".to_string(),
                required_role: "reviewer".to_string(),
                required_satisfaction: None,
                requires_approver: false,
                hook: None,
            },
            // reflecting → reflection_review (role "reflect" — doer complete path)
            TransitionDefinition {
                from_state: "reflecting".to_string(),
                to_state: "reflection_review".to_string(),
                required_role: "reflect".to_string(),
                required_satisfaction: None,
                requires_approver: false,
                hook: None,
            },
            // reflection_review → reflection_revision (role "reflect")
            TransitionDefinition {
                from_state: "reflection_review".to_string(),
                to_state: "reflection_revision".to_string(),
                required_role: "reflect".to_string(),
                required_satisfaction: Some(vec!["full_revision".to_string()]),
                requires_approver: false,
                hook: None,
            },
            // reflection_review → completed (role "complete")
            TransitionDefinition {
                from_state: "reflection_review".to_string(),
                to_state: "completed".to_string(),
                required_role: "complete".to_string(),
                required_satisfaction: Some(vec!["satisfied".to_string()]),
                requires_approver: false,
                hook: None,
            },
            // reflection_revision → reflection_review (role "reviewer")
            TransitionDefinition {
                from_state: "reflection_revision".to_string(),
                to_state: "reflection_review".to_string(),
                required_role: "reviewer".to_string(),
                required_satisfaction: None,
                requires_approver: false,
                hook: None,
            },
            // --- Amend loop (B5b BP4): mirrors the playbook seed's amend loop,
            // anchored on `completed` instead of `active`. The Amend RPC drives
            // only `completed → amend` (doer); the review-gate transitions are
            // handled by the existing complete/review machinery. ---
            // completed → amend (role "doer")
            TransitionDefinition {
                from_state: "completed".to_string(),
                to_state: "amend".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: None,
                requires_approver: false,
                hook: None,
            },
            // amend → amend_review (role "doer")
            TransitionDefinition {
                from_state: "amend".to_string(),
                to_state: "amend_review".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: None,
                requires_approver: false,
                hook: None,
            },
            // amend_review → completed (role "reviewer")
            TransitionDefinition {
                from_state: "amend_review".to_string(),
                to_state: "completed".to_string(),
                required_role: "reviewer".to_string(),
                required_satisfaction: Some(vec!["satisfied".to_string()]),
                requires_approver: false,
                hook: None,
            },
            // amend_review → amend_revision (role "doer")
            TransitionDefinition {
                from_state: "amend_review".to_string(),
                to_state: "amend_revision".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: Some(vec!["full_revision".to_string()]),
                requires_approver: false,
                hook: None,
            },
            // amend_revision → amend_review (role "doer")
            TransitionDefinition {
                from_state: "amend_revision".to_string(),
                to_state: "amend_review".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: None,
                requires_approver: false,
                hook: None,
            },
            // --- Park (abandon) edges: resume_signal_context_awareness. Every
            // OPEN lifecycle state can be parked to the terminal `abandoned`
            // state, making park a first-class, discoverable `available_actions`
            // action (interpreter-driven) instead of an untyped snapshot escape
            // hatch. These edges are appended AFTER every forward edge so
            // `outgoing_transitions(state).first()` still returns the forward
            // (advance) edge — the resume/advance-action rendering is unchanged.
            //
            // Each abandon edge is satisfaction-gated (`required_satisfaction:
            // Some(["abandoned"])`) so `complete`'s doer-candidate filter
            // (None-satisfaction, non-reviewer edges) NEVER sees it: parking is
            // snapshot-driven only. The `complete` handler rejects any
            // satisfaction outside {"", "satisfied", "full_revision",
            // "address_in_next_step"} before edge selection, so this edge is
            // unreachable via `complete` — auto-parking stays out of scope.
            TransitionDefinition {
                from_state: "spec".to_string(),
                to_state: "abandoned".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: Some(vec!["abandoned".to_string()]),
                requires_approver: false,
                hook: None,
            },
            TransitionDefinition {
                from_state: "spec_review".to_string(),
                to_state: "abandoned".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: Some(vec!["abandoned".to_string()]),
                requires_approver: false,
                hook: None,
            },
            TransitionDefinition {
                from_state: "spec_revision".to_string(),
                to_state: "abandoned".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: Some(vec!["abandoned".to_string()]),
                requires_approver: false,
                hook: None,
            },
            TransitionDefinition {
                from_state: "plan".to_string(),
                to_state: "abandoned".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: Some(vec!["abandoned".to_string()]),
                requires_approver: false,
                hook: None,
            },
            TransitionDefinition {
                from_state: "plan_review".to_string(),
                to_state: "abandoned".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: Some(vec!["abandoned".to_string()]),
                requires_approver: false,
                hook: None,
            },
            TransitionDefinition {
                from_state: "plan_revision".to_string(),
                to_state: "abandoned".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: Some(vec!["abandoned".to_string()]),
                requires_approver: false,
                hook: None,
            },
            TransitionDefinition {
                from_state: "implementing".to_string(),
                to_state: "abandoned".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: Some(vec!["abandoned".to_string()]),
                requires_approver: false,
                hook: None,
            },
            TransitionDefinition {
                from_state: "impl_phase_review".to_string(),
                to_state: "abandoned".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: Some(vec!["abandoned".to_string()]),
                requires_approver: false,
                hook: None,
            },
            TransitionDefinition {
                from_state: "impl_review".to_string(),
                to_state: "abandoned".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: Some(vec!["abandoned".to_string()]),
                requires_approver: false,
                hook: None,
            },
            TransitionDefinition {
                from_state: "impl_revision".to_string(),
                to_state: "abandoned".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: Some(vec!["abandoned".to_string()]),
                requires_approver: false,
                hook: None,
            },
            TransitionDefinition {
                from_state: "reflecting".to_string(),
                to_state: "abandoned".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: Some(vec!["abandoned".to_string()]),
                requires_approver: false,
                hook: None,
            },
            TransitionDefinition {
                from_state: "reflection_review".to_string(),
                to_state: "abandoned".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: Some(vec!["abandoned".to_string()]),
                requires_approver: false,
                hook: None,
            },
            TransitionDefinition {
                from_state: "reflection_revision".to_string(),
                to_state: "abandoned".to_string(),
                required_role: "doer".to_string(),
                required_satisfaction: Some(vec!["abandoned".to_string()]),
                requires_approver: false,
                hook: None,
            },
            // `abandoned` and `superseded` are terminal — no outgoing transitions.
        ],
        register: Register::Driven,
        // success_rubric reconciled from the production hearth copy by the
        // track-lifecycle satisfaction-encoding reconciliation so all four copies
        // carry the same rubric (it previously lived ONLY on the hearth copy).
        success_rubric: Some(SuccessRubric {
            dimensions: vec![
                RubricDimension {
                    dimension: "faithfulness_to_real_process".to_string(),
                    weight: 4,
                    evidence_class: EvidenceClass::ArtifactOfConsequence,
                },
                RubricDimension {
                    dimension: "correctness".to_string(),
                    weight: 3,
                    evidence_class: EvidenceClass::ArtifactOfConsequence,
                },
                RubricDimension {
                    dimension: "research_rigor".to_string(),
                    weight: 2,
                    evidence_class: EvidenceClass::VerifiableCitation,
                },
                RubricDimension {
                    dimension: "forethought".to_string(),
                    weight: 2,
                    evidence_class: EvidenceClass::VerifiableCitation,
                },
            ],
            grader: Some("track_lifecycle_rubric_judge_v1".to_string()),
            lagging_signals: vec![
                "reached_completed_state".to_string(),
                "artifact_git_churn".to_string(),
                "revert_or_rework_mention".to_string(),
                "began_then_abandoned_at_spec".to_string(),
            ],
            anchors: vec![
                AnchorRef {
                    instance: "good-full-lifecycle-driven-end-to-end".to_string(),
                    band: "good".to_string(),
                },
                AnchorRef {
                    instance: "good-audit-first-research-rigor".to_string(),
                    band: "good".to_string(),
                },
                AnchorRef {
                    instance: "good-review-loop-catches-real-defects".to_string(),
                    band: "good".to_string(),
                },
                AnchorRef {
                    instance: "bad-impl-on-stale-branch-base-rework".to_string(),
                    band: "bad".to_string(),
                },
                AnchorRef {
                    instance: "trap-workflow-shaped-work-without-lifecycle".to_string(),
                    band: "trap".to_string(),
                },
                AnchorRef {
                    instance: "trap-begun-then-abandoned-at-spec".to_string(),
                    band: "trap".to_string(),
                },
                AnchorRef {
                    instance: "hidden-virtue-state-field-drift-masks-completion".to_string(),
                    band: "hidden_virtue".to_string(),
                },
            ],
        }),
        outcome_predicate: Some(OutcomePredicate {
            terminal_state: "completed".to_string(),
            check: Some(
                "the track reached completed with a reflection.md recording what shipped and all review gates satisfied".to_string(),
            ),
        }),
    }
}
