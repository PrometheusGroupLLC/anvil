//! Step module for track_lifecycle_evidence_obligation_declared.feature
//! (T-ACT-1).
//!
//! Asserts the REAL production `track_seed()` — the canonical source of the
//! four synchronized `track_lifecycle` machine.yaml copies — carries the two
//! declared evidence obligations (`spec`/doer and `plan`/doer) and nothing
//! else, that declaring them re-hashes the machine's `playbook_version`, that
//! the declared obligation is sufficient for `assess_evidence_obligation` to
//! emit a non-`None` `Absent` assessment with no claims, and that the
//! record-mode shipping posture (flag OFF) loads the machine with its
//! obligations intact while the (unused) flag-ON authoring gate deliberately
//! rejects the intentionally-partial 2-of-17 declaration.
//!
//! The `Given the track seed` step is reused from `playbook_seeds` (context key
//! `ws_seed`, typed `PlaybookMachine`); this module only adds new check steps
//! that read that key or reach for `track_seed()` directly.

use anvil_core::domain::playbook::evidence_obligation::assess_evidence_obligation;
use anvil_core::domain::playbook::loader::{
    load_from_yaml_with, validate_evidence_obligation, LoaderEnforcement,
};
use anvil_core::domain::playbook::seeds::track_seed;
use anvil_core::domain::playbook::types::{EvidenceClass, PlaybookMachine};
use anvil_core::domain::playbook_version::machine_content_version;
use brine_runner_rust::registry::{check_def, StepDef};

/// Context key the shared `playbook_seeds` Given populates with the track seed.
const SEED_KEY: &str = "ws_seed";

/// Snake_case name of an `EvidenceClass`, matching the YAML/serde vocabulary.
fn class_name(class: EvidenceClass) -> &'static str {
    match class {
        EvidenceClass::ArtifactOfConsequence => "artifact_of_consequence",
        EvidenceClass::VerifiableCitation => "verifiable_citation",
        EvidenceClass::SelfDescription => "self_description",
    }
}

/// Parse a comma-separated class list like `verifiable_citation, artifact_of_consequence`.
fn parse_expected(list: &str) -> Vec<String> {
    list.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// The on-disk track_lifecycle hooks/ listing the loader validates references
/// against — mirrors the regen examples and the parity guard.
fn hook_files() -> Vec<String> {
    [
        "spec-writing.md",
        "spec-review.md",
        "spec-revision.md",
        "plan-writing.md",
        "plan-review.md",
        "implementing.md",
        "impl-phase-review.md",
        "impl-review.md",
        "reflecting.md",
        "reflection-review.md",
        "amend-writing.md",
        "amend-review.md",
        "complete.md",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// The snake_case obligation on `machine.states[state].measurement_by_role[role]`.
fn obligation_of<'a>(
    machine: &'a PlaybookMachine,
    state: &str,
    role: &str,
) -> Result<&'a Vec<EvidenceClass>, String> {
    let state_def = machine
        .states
        .iter()
        .find(|s| s.name == state)
        .ok_or_else(|| format!("no state '{}' in track seed", state))?;
    let spec = state_def.measurement_by_role.get(role).ok_or_else(|| {
        format!(
            "no measurement_by_role entry for state '{}' role '{}'",
            state, role
        )
    })?;
    Ok(&spec.evidence_obligation)
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ===== S1 — exact declared classes (zero-case on the other class) =====
        check_def(
            "the track seed state {string} role {string} declares evidence classes {string}",
            &[(SEED_KEY, "PlaybookMachine")],
            |ctx, params| {
                let state = params.get_string(0).ok_or("Expected state name")?;
                let role = params.get_string(1).ok_or("Expected role name")?;
                let expected = parse_expected(&params.get_string(2).ok_or("Expected class list")?);
                let machine = ctx.get::<PlaybookMachine>(SEED_KEY).ok_or("No track seed")?;
                let actual: Vec<String> = obligation_of(machine, &state, &role)?
                    .iter()
                    .map(|c| class_name(*c).to_string())
                    .collect();
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "state '{}' role '{}' obligation: expected {:?} but got {:?}",
                        state, role, expected, actual
                    ))
                }
            },
        ),
        // ===== S2 — the 15-pair zero-case (each measured pair stays empty) =====
        check_def(
            "the track seed state {string} role {string} has an empty evidence obligation",
            &[(SEED_KEY, "PlaybookMachine")],
            |ctx, params| {
                let state = params.get_string(0).ok_or("Expected state name")?;
                let role = params.get_string(1).ok_or("Expected role name")?;
                let machine = ctx.get::<PlaybookMachine>(SEED_KEY).ok_or("No track seed")?;
                let actual = obligation_of(machine, &state, &role)?;
                if actual.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "state '{}' role '{}' was expected to carry NO obligation but declares {:?}",
                        state,
                        role,
                        actual.iter().map(|c| class_name(*c)).collect::<Vec<_>>()
                    ))
                }
            },
        ),
        // ===== S3 — the 3 unmeasured states carry no measurement entry =====
        check_def(
            "the track seed state {string} has no measurement entry",
            &[(SEED_KEY, "PlaybookMachine")],
            |ctx, params| {
                let state = params.get_string(0).ok_or("Expected state name")?;
                let machine = ctx.get::<PlaybookMachine>(SEED_KEY).ok_or("No track seed")?;
                let state_def = machine
                    .states
                    .iter()
                    .find(|s| s.name == state)
                    .ok_or_else(|| format!("no state '{}' in track seed", state))?;
                if state_def.measurement_by_role.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "state '{}' was expected to have an empty measurement_by_role map but has roles {:?}",
                        state,
                        state_def.measurement_by_role.keys().collect::<Vec<_>>()
                    ))
                }
            },
        ),
        // ===== S4 — assess_evidence_obligation returns Some(Absent), not None =====
        check_def(
            "assessing the track seed state {string} role {string} against no claims yields status {string} and missing classes {string}",
            &[(SEED_KEY, "PlaybookMachine")],
            |ctx, params| {
                let state = params.get_string(0).ok_or("Expected state name")?;
                let role = params.get_string(1).ok_or("Expected role name")?;
                let expected_status = params.get_string(2).ok_or("Expected status token")?;
                let expected_missing =
                    parse_expected(&params.get_string(3).ok_or("Expected missing class list")?);
                let machine = ctx.get::<PlaybookMachine>(SEED_KEY).ok_or("No track seed")?;
                let obligation = obligation_of(machine, &state, &role)?;
                let assessment = assess_evidence_obligation(obligation, &[]).ok_or_else(|| {
                    format!(
                        "assess_evidence_obligation returned None for state '{}' role '{}' — the declaration is not being observed",
                        state, role
                    )
                })?;
                if assessment.status.as_str() != expected_status {
                    return Err(format!(
                        "expected status '{}' but got '{}'",
                        expected_status,
                        assessment.status.as_str()
                    ));
                }
                let actual_missing: Vec<String> = assessment
                    .missing_classes
                    .iter()
                    .map(|c| class_name(*c).to_string())
                    .collect();
                if actual_missing == expected_missing {
                    Ok(())
                } else {
                    Err(format!(
                        "expected missing classes {:?} but got {:?}",
                        expected_missing, actual_missing
                    ))
                }
            },
        ),
        // ===== S5 — declaring the two obligations changes machine_content_version =====
        check_def(
            "declaring the two obligations changes the track machine content version",
            &[],
            |_ctx, _params| {
                // Clone B is the post-P0 seed (obligations declared).
                let clone_b = track_seed().clone();
                // Clone A resets the two declared obligations to empty (pre-P0 shape),
                // proving it is exactly these two fields that re-hash the machine.
                let mut clone_a = track_seed().clone();
                for state_def in clone_a.states.iter_mut() {
                    if state_def.name == "spec" || state_def.name == "plan" {
                        if let Some(spec) = state_def.measurement_by_role.get_mut("doer") {
                            spec.evidence_obligation = Vec::new();
                        }
                    }
                }
                let va = machine_content_version(&clone_a)
                    .ok_or("machine_content_version(pre-P0 clone) returned None")?;
                let vb = machine_content_version(&clone_b)
                    .ok_or("machine_content_version(post-P0 seed) returned None")?;
                if va.is_empty() || vb.is_empty() {
                    return Err(format!("empty version(s): A='{}' B='{}'", va, vb));
                }
                if va == vb {
                    Err(format!(
                        "expected declaring the obligations to change the content version, but both hashed to '{}'",
                        va
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        // ===== S6a — flag-OFF (record-mode) loads the machine, obligations intact =====
        check_def(
            "with evidence-obligation enforcement off the track seed loads and retains both declared obligations",
            &[],
            |_ctx, _params| {
                let seed = track_seed().clone();
                let yaml = serde_yaml::to_string(&seed)
                    .map_err(|e| format!("serialize track seed to yaml: {}", e))?;
                let machine = load_from_yaml_with(
                    "track_lifecycle_machine.yaml",
                    &yaml,
                    &hook_files(),
                    LoaderEnforcement::OFF,
                )
                .map_err(|e| {
                    format!(
                        "track machine failed to load with enforcement OFF (expected a clean load): {}",
                        e
                    )
                })?;
                let spec_doer: Vec<&str> = obligation_of(&machine, "spec", "doer")?
                    .iter()
                    .map(|c| class_name(*c))
                    .collect();
                let plan_doer: Vec<&str> = obligation_of(&machine, "plan", "doer")?
                    .iter()
                    .map(|c| class_name(*c))
                    .collect();
                if spec_doer != vec!["artifact_of_consequence"] {
                    return Err(format!(
                        "spec/doer obligation not retained through OFF load: {:?}",
                        spec_doer
                    ));
                }
                if plan_doer != vec!["verifiable_citation", "artifact_of_consequence"] {
                    return Err(format!(
                        "plan/doer obligation not retained through OFF load: {:?}",
                        plan_doer
                    ));
                }
                Ok(())
            },
        ),
        // ===== S6b — flag-ON deliberately rejects the partial 2-of-17 declaration =====
        check_def(
            "validating the track seed under evidence-obligation enforcement returns error code {string}",
            &[],
            |_ctx, params| {
                let expected_code = params.get_string(0).ok_or("Expected error code")?;
                match validate_evidence_obligation(track_seed(), "track_lifecycle") {
                    Ok(()) => Err(
                        "expected validate_evidence_obligation to REJECT the partial 2-of-17 declaration, but it returned Ok(())"
                            .to_string(),
                    ),
                    Err(e) => {
                        if e.code() == expected_code {
                            Ok(())
                        } else {
                            Err(format!(
                                "expected error code '{}' but got '{}' ({})",
                                expected_code,
                                e.code(),
                                e
                            ))
                        }
                    }
                }
            },
        ),
        // ===== S6b (AC12) — track is Driven, so the FREE-register branch is unreachable =====
        check_def(
            "the track seed is register driven",
            &[],
            |_ctx, _params| {
                if track_seed().is_driven() {
                    Ok(())
                } else {
                    Err("track seed is not register driven — the free-register obligation branch would become reachable".to_string())
                }
            },
        ),
    ]
}
