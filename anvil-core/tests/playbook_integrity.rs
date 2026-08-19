//! Phase 1 of the atlas measurement surface: the PURE, SHARED integrity fold
//! (`playbook_integrity`) and the per-dir `status.yaml` read (`read_playbook_status`).
//!
//! Plain rust integration tests (precedent: `hearth_lint.rs`)
//! exercising the anvil-core lib fns directly. These primitives are structural
//! folds over an in-memory `PlaybookMachine`, so they are tested here rather than
//! through a brine seam.
//!
//! Honesty constraints under test:
//!   - `rubber_stamp_gates` adopts the FLEET definition: a `*_review` state that is
//!     `is_review_gate:false` OR has ANY exit with `required_satisfaction:null`
//!     (incl. a single-edge gate) is a rubber stamp.
//!   - `grader_declared` reports the NAME only — never "registered"/"calibrated".
//!   - `read_playbook_status` uses PLAIN serde with `#[serde(default)]` and NO
//!     `deny_unknown_fields`, so real status.yaml files (carrying version/kind/
//!     actors/transitions/...) parse; a missing file yields `None`.

use std::collections::BTreeMap;

use anvil_core::domain::playbook::integrity::playbook_integrity;
use anvil_core::domain::playbook::status_read::read_playbook_status;
use anvil_core::domain::playbook::types::{
    AnchorRef, MeasurementSpec, PlaybookMachine, StateDefinition, SuccessRubric,
    TransitionDefinition,
};

fn state(name: &str, is_review_gate: bool, is_terminal: bool, measured: bool) -> StateDefinition {
    let mut measurement_by_role = BTreeMap::new();
    if measured {
        measurement_by_role.insert(
            "doer".to_string(),
            MeasurementSpec {
                intent: "do the thing".into(),
                expected_output: "an artifact".into(),
                success_criteria: None,
                evidence_obligation: Vec::new(),
            },
        );
    }
    StateDefinition {
        name: name.to_string(),
        role_filters: vec![],
        registry_section: "section".into(),
        projection_targets: vec![],
        is_review_gate,
        is_terminal,
        hook: None,
        hooks_by_role: BTreeMap::new(),
        measurement_by_role,
    }
}

fn edge(from: &str, to: &str, satisfaction: Option<Vec<String>>) -> TransitionDefinition {
    TransitionDefinition {
        from_state: from.into(),
        to_state: to.into(),
        required_role: "reviewer".into(),
        required_satisfaction: satisfaction,
        requires_approver: false,
        hook: None,
    }
}

fn machine(states: Vec<StateDefinition>, transitions: Vec<TransitionDefinition>) -> PlaybookMachine {
    PlaybookMachine {
        states,
        transitions,
        ..Default::default()
    }
}

fn satisfied_pair() -> Option<Vec<String>> {
    Some(vec!["satisfied".to_string(), "needs_revision".to_string()])
}

#[test]
fn rubber_stamp_gate_flagged_when_not_a_review_gate() {
    // spec_review LOOKS like a gate but is_review_gate:false → rubber stamp.
    let m = machine(
        vec![
            state("spec", false, false, true),
            state("spec_review", /* is_review_gate = */ false, false, true),
            state("done", false, true, false),
        ],
        vec![
            edge("spec", "spec_review", None),
            edge("spec_review", "done", None),
        ],
    );
    let integrity = playbook_integrity(&m);
    assert_eq!(integrity.rubber_stamp_gates, vec!["spec_review".to_string()]);
    // A machine reaching this fold loaded by definition.
    assert!(integrity.loads);
}

#[test]
fn properly_gated_review_state_is_not_a_rubber_stamp() {
    // is_review_gate:true AND every exit carries a satisfaction token → clean.
    let m = machine(
        vec![
            state("spec", false, false, true),
            state("spec_review", true, false, true),
            state("done", false, true, false),
        ],
        vec![
            edge("spec", "spec_review", None), // entry edge from a NON-gate source: ignored
            edge("spec_review", "done", satisfied_pair()),
            edge("spec_review", "spec", satisfied_pair()), // back-edge also satisfied
        ],
    );
    let integrity = playbook_integrity(&m);
    assert!(
        integrity.rubber_stamp_gates.is_empty(),
        "expected no rubber stamps, got {:?}",
        integrity.rubber_stamp_gates
    );
}

#[test]
fn fleet_definition_flags_null_satisfaction_exit_even_when_review_gate_true() {
    // A single-edge "gate" that claims is_review_gate:true but whose only exit
    // carries required_satisfaction:null — the fleet rubber-stamp shape. (The
    // loader would reject this, but the fold adopts the fleet definition
    // independently, so it must still flag it.)
    let m = machine(
        vec![
            state("plan", false, false, true),
            state("plan_review", /* is_review_gate = */ true, false, true),
            state("done", false, true, false),
        ],
        vec![
            edge("plan", "plan_review", None),
            edge("plan_review", "done", None), // null satisfaction on the ONLY exit
        ],
    );
    let integrity = playbook_integrity(&m);
    assert_eq!(integrity.rubber_stamp_gates, vec!["plan_review".to_string()]);
}

#[test]
fn unmeasured_nonterminal_states_flagged_terminal_excluded() {
    let m = machine(
        vec![
            state("spec", false, false, /* measured = */ true), // measured → clean
            state("in_progress", false, false, /* measured = */ false), // unmeasured non-terminal → flagged
            state("done", false, /* terminal = */ true, /* measured = */ false), // terminal → NOT flagged
        ],
        vec![
            edge("spec", "in_progress", None),
            edge("in_progress", "done", None),
        ],
    );
    let integrity = playbook_integrity(&m);
    assert_eq!(integrity.unmeasured_states, vec!["in_progress".to_string()]);
}

#[test]
fn grader_declared_only_when_rubric_names_a_grader() {
    let mut m = machine(vec![state("s", false, true, false)], vec![]);

    // No rubric → not declared.
    assert!(!playbook_integrity(&m).grader_declared);

    // Rubric present but grader None → not declared.
    m.success_rubric = Some(SuccessRubric {
        grader: None,
        ..Default::default()
    });
    assert!(!playbook_integrity(&m).grader_declared);

    // Rubric NAMES a grader → declared (name only, never "registered").
    m.success_rubric = Some(SuccessRubric {
        grader: Some("rubric-judge-v1".into()),
        ..Default::default()
    });
    assert!(playbook_integrity(&m).grader_declared);
}

#[test]
fn anchors_count_reflects_rubric_anchors() {
    let mut m = machine(vec![state("s", false, true, false)], vec![]);
    assert_eq!(playbook_integrity(&m).anchors_count, 0); // no rubric

    m.success_rubric = Some(SuccessRubric {
        anchors: vec![
            AnchorRef {
                instance: "ex-1".into(),
                band: "good".into(),
            },
            AnchorRef {
                instance: "ex-2".into(),
                band: "mediocre".into(),
            },
        ],
        ..Default::default()
    });
    assert_eq!(playbook_integrity(&m).anchors_count, 2);
}

#[test]
fn read_playbook_status_missing_file_is_none() {
    let tmp = tempfile::TempDir::new().unwrap();
    // Empty dir — no status.yaml (mirrors the 3 live dirs that have none).
    assert_eq!(read_playbook_status(tmp.path()), None);
}

#[test]
fn read_playbook_status_owner_kit_without_state() {
    // Mirrors the real proposal_lifecycle/status.yaml (owner_kit, no `state`).
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("status.yaml"),
        "contributed_by: anvil-kit\nowner_kit: anvil-kit\nvisibility: foundation\norg: Foundation\n",
    )
    .unwrap();
    let status = read_playbook_status(tmp.path()).expect("present status.yaml → Some");
    assert_eq!(status.owner_kit, "anvil-kit");
    assert_eq!(status.state, None);
}

#[test]
fn read_playbook_status_owner_kit_and_state_with_extra_keys() {
    // Mirrors a real track-lifecycle status.yaml: owner_kit + state PLUS the
    // extra keys (version/kind/actors/transitions/...) that `deny_unknown_fields`
    // would choke on — proving the plain-serde shape.
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("status.yaml"),
        concat!(
            "version: 1\n",
            "kind: playbook\n",
            "state: active\n",
            "contributed_by: anvil-kit\n",
            "owner_kit: anvil-kit\n",
            "visibility: foundation\n",
            "org: Foundation\n",
            "actors: {}\n",
            "transitions:\n",
            "  - to: active\n",
            "    at: \"2026-04-22T12:26:55Z\"\n",
            "    actor: Casey-689556\n",
            "    role: bootstrap\n",
        ),
    )
    .unwrap();
    let status = read_playbook_status(tmp.path()).expect("present status.yaml → Some");
    assert_eq!(status.owner_kit, "anvil-kit");
    assert_eq!(status.state.as_deref(), Some("active"));
}
