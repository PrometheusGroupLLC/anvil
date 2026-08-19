//! Step module for `internal_identifier_boundary.feature`.
//!
//! B-i's semantic assertion (`spec.md:767` forbids a count-only rename). What is
//! asserted is the PHASE BOUNDARY: the internal type renamed by B-i.1 must not
//! have moved any of the `playbook_*` wire codes its `code()` emits, because
//! those are frozen byte-verbatim until B-w.6's versioned error cutover under
//! NG-PROTO-VNEXT.
//!
//! Nothing here reads source text. Scenario 1 and 2 construct real
//! `PlaybookLoadError` variants and call the real `code()`; scenario 3 builds a
//! real `TempDir` hearth carrying only a legacy `workflows/` directory and
//! constructs the real `HearthPlaybookRegistry` against it.

use anvil_core::domain::playbook::hearth_registry::HearthPlaybookRegistry;
use anvil_core::domain::playbook::load_error::PlaybookLoadError;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const CODES_KEY: &str = "iib_codes";
const HEARTH_PATH_KEY: &str = "iib_hearth_path";
const HEARTH_HANDLE_KEY: &str = "iib_hearth_handle";

/// EVERY code `PlaybookLoadError::code()` can emit, paired with the variant that
/// emits it. The pairing is the assertion: a renamed code reds by VALUE, and a
/// new variant fails to compile in the exhaustive `mirrored_code()` match below
/// until it is added here.
///
/// Round 5 falsified the earlier version of that second claim: `all_variants()`
/// is a `vec![...]`, so adding a 19th variant did NOT break it, and the seam
/// stayed green while `code()` emitted 19 codes and `ALL_CODES` pinned 18 —
/// round-4 MEDIUM-4's defect class reintroduced at n+1. `mirrored_code()` is the
/// repair: an EXHAUSTIVE `match` that a new variant cannot compile past.
///
/// SCOPING DECISION (round-4 MEDIUM-4). `code()` has **18** arms, not 17. The
/// 18th, `hook_path_invalid`, is deliberately NOT `playbook_`-prefixed: it names
/// a malformed path, not a playbook concept, so this migration has nothing to
/// rename in it. Rather than scope the frozen set to the 17 `playbook_`-prefixed
/// codes and leave the 18th unguarded, the set is **extended to all 18** with the
/// exception named. That is the stronger choice: B-i must move NO code, not
/// merely no `playbook_`-prefixed code, and pinning 18 of 18 makes the claim
/// true about the type instead of true about a subset.
const ALL_CODES: &[(&str, &str)] = &[
    ("YamlParseError", "playbook_yaml_parse_error"),
    ("MissingRequiredKey", "playbook_missing_required_key"),
    ("UnknownRoleReference", "playbook_unknown_role_reference"),
    ("UnknownStateReference", "playbook_unknown_state_reference"),
    ("ReviewGateMissingSatisfaction", "playbook_review_gate_missing_satisfaction"),
    // The deliberate non-`playbook_`-prefixed code. See the scoping decision above.
    ("HookPathInvalid", "hook_path_invalid"),
    ("UnknownHookReference", "playbook_unknown_hook_reference"),
    ("UnknownRoleKeyReference", "playbook_unknown_role_key"),
    ("DuplicateKindRegistration", "playbook_duplicate_kind_registration"),
    ("UnreachableState", "playbook_unreachable_state"),
    ("DeadEndState", "playbook_dead_end_state"),
    ("NoTerminalReachable", "playbook_no_terminal_reachable"),
    ("UnknownQualityDimension", "playbook_unknown_quality_dimension"),
    ("InvalidRubricWeight", "playbook_invalid_rubric_weight"),
    ("OutcomePredicateUnknownState", "playbook_outcome_predicate_unknown_state"),
    ("MeasurementDefinitionMissing", "playbook_measurement_definition_missing"),
    ("EvidenceObligationMissing", "playbook_evidence_obligation_missing"),
    ("EvidenceObligationOnFreeRegister", "playbook_evidence_obligation_on_free_register"),
];

/// One real constructed variant per `code()` arm — EXHAUSTIVE, in the same order
/// as `ALL_CODES`. Round 4 falsified the previous 3-variant sample: renaming
/// `playbook_no_terminal_reachable` passed every gate this phase runs.
fn all_variants() -> Vec<PlaybookLoadError> {
    let s = |v: &str| v.to_string();
    vec![
        PlaybookLoadError::YamlParseError { artifact_id: s("f"), line: 1, column: 1, message: s("f") },
        PlaybookLoadError::MissingRequiredKey { artifact_id: s("f"), key_path: s("kind") },
        PlaybookLoadError::UnknownRoleReference { artifact_id: s("f"), from_state: s("a"), to_state: s("b"), role: s("doer") },
        PlaybookLoadError::UnknownStateReference { artifact_id: s("f"), from_state: s("a"), to_state: s("b"), unknown_state: s("b") },
        PlaybookLoadError::ReviewGateMissingSatisfaction { artifact_id: s("f"), state: s("a"), to_state: s("b") },
        PlaybookLoadError::HookPathInvalid { artifact_id: s("f"), context: s("state:a"), filename: s("../x.md") },
        PlaybookLoadError::UnknownHookReference { artifact_id: s("f"), context: s("state:a"), filename: s("x.md") },
        PlaybookLoadError::UnknownRoleKeyReference { artifact_id: s("f"), context: s("state:a"), role: s("doer") },
        PlaybookLoadError::DuplicateKindRegistration { artifact_ids: s("f,g"), kind: s("track") },
        PlaybookLoadError::UnreachableState { artifact_id: s("f"), initial_state: s("a"), unreachable_state: s("z") },
        PlaybookLoadError::DeadEndState { artifact_id: s("f"), state: s("z") },
        PlaybookLoadError::NoTerminalReachable { artifact_id: s("f"), state: s("a") },
        PlaybookLoadError::UnknownQualityDimension { artifact_id: s("f"), dimension: s("d") },
        PlaybookLoadError::InvalidRubricWeight { artifact_id: s("f"), dimension: s("d") },
        PlaybookLoadError::OutcomePredicateUnknownState { artifact_id: s("f"), terminal_state: s("z") },
        PlaybookLoadError::MeasurementDefinitionMissing { artifact_id: s("f"), detail: s("d") },
        PlaybookLoadError::EvidenceObligationMissing { artifact_id: s("f"), detail: s("d") },
        PlaybookLoadError::EvidenceObligationOnFreeRegister { artifact_id: s("f"), detail: s("d") },
    ]
}

/// An exhaustive `match` mirroring `PlaybookLoadError::code()` arm for arm.
///
/// THIS FUNCTION EXISTS TO FAIL TO COMPILE. Rust requires a `match` over an enum
/// to be exhaustive, so adding a variant to `PlaybookLoadError` breaks the build
/// HERE, forcing whoever adds it to also pin its code in `ALL_CODES` and
/// construct it in `all_variants()`. Without it the seam's stated invariant
/// ("every emitted code is pinned") silently becomes false about the type again
/// the moment a 19th variant lands.
///
/// It deliberately duplicates production's `code()` rather than calling it: a
/// mirror that delegated would pass regardless of what production emits, which
/// is precisely the unfailable shape this seam was rewritten to remove. The
/// zip check below compares the two, so a drift between mirror and production
/// reds by value.
fn mirrored_code(e: &PlaybookLoadError) -> &'static str {
    match e {
        PlaybookLoadError::HearthDirectoryMoveFailed { .. } => "hearth_directory_move_failed",
        PlaybookLoadError::HearthDirectoryCollision { .. } => "hearth_directory_collision",
        PlaybookLoadError::HearthRootUnreadable { .. } => "hearth_root_unreadable",
        PlaybookLoadError::YamlParseError { .. } => "playbook_yaml_parse_error",
        PlaybookLoadError::MissingRequiredKey { .. } => "playbook_missing_required_key",
        PlaybookLoadError::UnknownRoleReference { .. } => "playbook_unknown_role_reference",
        PlaybookLoadError::UnknownStateReference { .. } => "playbook_unknown_state_reference",
        PlaybookLoadError::ReviewGateMissingSatisfaction { .. } => {
            "playbook_review_gate_missing_satisfaction"
        }
        PlaybookLoadError::HookPathInvalid { .. } => "hook_path_invalid",
        PlaybookLoadError::UnknownHookReference { .. } => "playbook_unknown_hook_reference",
        PlaybookLoadError::UnknownRoleKeyReference { .. } => "playbook_unknown_role_key",
        PlaybookLoadError::DuplicateKindRegistration { .. } => {
            "playbook_duplicate_kind_registration"
        }
        PlaybookLoadError::UnreachableState { .. } => "playbook_unreachable_state",
        PlaybookLoadError::DeadEndState { .. } => "playbook_dead_end_state",
        PlaybookLoadError::NoTerminalReachable { .. } => "playbook_no_terminal_reachable",
        PlaybookLoadError::UnknownQualityDimension { .. } => "playbook_unknown_quality_dimension",
        PlaybookLoadError::InvalidRubricWeight { .. } => "playbook_invalid_rubric_weight",
        PlaybookLoadError::OutcomePredicateUnknownState { .. } => {
            "playbook_outcome_predicate_unknown_state"
        }
        PlaybookLoadError::MeasurementDefinitionMissing { .. } => {
            "playbook_measurement_definition_missing"
        }
        PlaybookLoadError::EvidenceObligationMissing { .. } => {
            "playbook_evidence_obligation_missing"
        }
        PlaybookLoadError::EvidenceObligationOnFreeRegister { .. } => {
            "playbook_evidence_obligation_on_free_register"
        }
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "the playbook load-error family",
            &[],
            &[(CODES_KEY, "list")],
            |_ctx, _params| {
                let codes: Vec<String> = all_variants()
                    .iter()
                    .map(|e| e.code().to_string())
                    .collect();
                if codes.is_empty() {
                    return Err("no load-error variants constructed".to_string());
                }
                let mut out = Context::new();
                out.set(CODES_KEY, codes);
                Ok(out)
            },
        ),
        check_def(
            "the unknown-hook-reference code is exactly {string}",
            &[(CODES_KEY, "list")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected code")?.to_string();
                let got = PlaybookLoadError::UnknownHookReference {
                    artifact_id: "fixture".into(),
                    context: "state:draft".into(),
                    filename: "draft.md".into(),
                }
                .code();
                if got == want {
                    Ok(())
                } else {
                    Err(format!("expected `{want}`, got `{got}`"))
                }
            },
        ),
        check_def(
            "the yaml-parse code is exactly {string}",
            &[(CODES_KEY, "list")],
            |ctx, params| {
                let _ = ctx;
                let want = params.get_string(0).ok_or("Expected code")?.to_string();
                let got = PlaybookLoadError::YamlParseError {
                    artifact_id: "fixture".into(),
                    line: 1,
                    column: 1,
                    message: "fixture".into(),
                }
                .code();
                if got == want {
                    Ok(())
                } else {
                    Err(format!("expected `{want}`, got `{got}`"))
                }
            },
        ),
        check_def(
            "every variant emits exactly its pinned code",
            &[(CODES_KEY, "list")],
            |ctx, _params| {
                let _ = ctx;
                let got = all_variants();
                if got.len() != ALL_CODES.len() {
                    return Err(format!(
                        "all_variants() constructs {} variants but ALL_CODES pins {}. \
                         Every code()-emitting arm must be constructed here.",
                        got.len(),
                        ALL_CODES.len()
                    ));
                }
                let mut failures = Vec::new();
                for (variant, (name, want)) in got.iter().zip(ALL_CODES.iter()) {
                    let actual = variant.code();
                    if actual != *want {
                        failures.push(format!("{name}: expected `{want}`, got `{actual}`"));
                    }
                    // The mirror is exhaustive, so it is what forces a NEW variant
                    // into this list; comparing it to production catches a drift
                    // between the two.
                    let mirrored = mirrored_code(variant);
                    if mirrored != actual {
                        failures.push(format!(
                            "{name}: production code() emits `{actual}` but the exhaustive \
                             mirror emits `{mirrored}` — the mirror has drifted"
                        ));
                    }
                }
                if failures.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "{} wire code(s) moved. Changing a wire code is B-w.6's, under \
                         NG-PROTO-VNEXT — not B-i's:\n  - {}",
                        failures.len(),
                        failures.join("\n  - ")
                    ))
                }
            },
        ),
        check_def(
            "exactly 18 codes are pinned, 17 of them playbook-prefixed",
            &[],
            |_ctx, _params| {
                if ALL_CODES.len() != 18 {
                    return Err(format!("ALL_CODES pins {}, expected 18", ALL_CODES.len()));
                }
                let prefixed = ALL_CODES
                    .iter()
                    .filter(|(_, c)| c.starts_with("playbook_"))
                    .count();
                if prefixed != 17 {
                    return Err(format!(
                        "{prefixed} codes are `playbook_`-prefixed, expected 17; the one \
                         deliberate exception is `hook_path_invalid`"
                    ));
                }
                let exception: Vec<&str> = ALL_CODES
                    .iter()
                    .filter(|(_, c)| !c.starts_with("playbook_"))
                    .map(|(_, c)| *c)
                    .collect();
                if exception != vec!["hook_path_invalid"] {
                    return Err(format!(
                        "the non-`playbook_` code set is {exception:?}, expected \
                         exactly [\"hook_path_invalid\"]"
                    ));
                }
                Ok(())
            },
        ),
        step_def(
            "a hearth whose only definition directory is a legacy workflows dir",
            &[],
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, _params| {
                let temp = tempfile::TempDir::new()
                    .map_err(|e| format!("failed to create temp hearth: {e}"))?;
                let root = temp.path().to_path_buf();
                let legacy = root.join("workflows/track_lifecycle");
                std::fs::create_dir_all(&legacy)
                    .map_err(|e| format!("failed to create legacy dir: {e}"))?;
                std::fs::write(legacy.join("machine.yaml"), "kind: track\n")
                    .map_err(|e| format!("failed to write machine.yaml: {e}"))?;

                let handle: Arc<Mutex<Option<tempfile::TempDir>>> =
                    Arc::new(Mutex::new(Some(temp)));
                let mut out = Context::new();
                out.set(HEARTH_PATH_KEY, root);
                out.set(HEARTH_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "the hearth playbook registry is constructed against it",
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, _params| {
                let root = ctx
                    .get::<PathBuf>(HEARTH_PATH_KEY)
                    .ok_or("No hearth path")?
                    .clone();
                let handle = ctx
                    .get::<Arc<Mutex<Option<tempfile::TempDir>>>>(HEARTH_HANDLE_KEY)
                    .ok_or("No hearth handle")?
                    .clone();
                let _registry = HearthPlaybookRegistry::new(root.clone());
                let mut out = Context::new();
                out.set(HEARTH_PATH_KEY, root);
                out.set(HEARTH_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        check_def(
            "the legacy directory has been renamed to the canonical playbooks directory",
            &[(HEARTH_PATH_KEY, "PathBuf")],
            |ctx, _params| {
                let root = ctx.get::<PathBuf>(HEARTH_PATH_KEY).ok_or("No hearth path")?;
                if !root.join("playbooks/track_lifecycle/machine.yaml").is_file() {
                    return Err(format!(
                        "canonical playbooks/track_lifecycle/machine.yaml missing under {}",
                        root.display()
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "no definition is scanned from the legacy location",
            &[(HEARTH_PATH_KEY, "PathBuf")],
            |ctx, _params| {
                let root = ctx.get::<PathBuf>(HEARTH_PATH_KEY).ok_or("No hearth path")?;
                if root.join("workflows").exists() {
                    return Err(format!(
                        "legacy workflows/ still exists under {} — the registry renames it, \
                         it does not scan it in place",
                        root.display()
                    ));
                }
                Ok(())
            },
        ),
    ]
}
