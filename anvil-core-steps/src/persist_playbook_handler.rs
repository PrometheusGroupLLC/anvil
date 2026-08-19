//! Step module for `persist_playbook_handler.feature` (track 1a, BP2).
//!
//! Exercises the pure `PersistPlaybookCommandHandler` against a real
//! `HearthPlaybookRegistry` built from a temp owner-home. Proves
//! loader-validate-before-emit, duplicate-kind detection, and the
//! no-partial-write guarantee (typed error → no event). Uses tempfile + the
//! shared minimal/loader-invalid fixtures; never reads a real machine.yaml.

use anvil_core::domain::persist_playbook::{
    PersistPlaybookCommandHandler, PersistPlaybookError, PersistPlaybookOutcome,
    PersistPlaybookRequest,
};
use anvil_core::domain::playbook::hearth_registry::HearthPlaybookRegistry;
use anvil_core::domain::playbook::loader::LoaderEnforcement;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const OWNER_HOME_KEY: &str = "pwh_owner_home";
const OWNER_HOME_HANDLE_KEY: &str = "pwh_owner_home_handle";
const MACHINE_YAML_KEY: &str = "pwh_machine_yaml";
const OUTCOME_KEY: &str = "pwh_outcome";
const ERROR_CODE_KEY: &str = "pwh_error_code";
const ERROR_VARIANT_KEY: &str = "pwh_error_variant";
const ERROR_MESSAGE_KEY: &str = "pwh_error_message";
const EVENT_COUNT_KEY: &str = "pwh_event_count";
const SEEDED_BYTES_KEY: &str = "pwh_seeded_bytes";

/// C-d.1 round 5, M-4: `ScratchDir`, not a bare `TempDir`. The unreadable-entry
/// fixture below chmods a definition directory to `0000`, and a bare `TempDir`
/// would leave a subtree neither `remove_dir_all`, `rm -rf`, nor the age-guarded
/// `find` purge can remove — on every RED, which is exactly when it matters.
type Handle = Arc<Mutex<Option<anvil_test_support::ScratchDir>>>;

fn new_temp() -> Result<(PathBuf, Handle), String> {
    let dir = anvil_test_support::ScratchDir::new()?;
    let path = dir.path().to_path_buf();
    Ok((path, Arc::new(Mutex::new(Some(dir)))))
}

/// Build a `Given` step that seeds `MACHINE_YAML_KEY` from a `kind`-parameterized
/// fixture, carrying the owner-home + handle through unchanged. Shared by the
/// evidence-obligation persist-gate fixtures (T-EEC-1 P3).
fn machine_fixture_step(phrase: &'static str, fixture: fn(&str) -> String) -> StepDef {
    step_def(
        phrase,
        &[
            (OWNER_HOME_KEY, "PathBuf"),
            (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
        ],
        &[
            (OWNER_HOME_KEY, "PathBuf"),
            (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            (MACHINE_YAML_KEY, "String"),
        ],
        move |ctx, params| {
            let kind = params.get_string(0).ok_or("Expected kind")?;
            let owner_home = ctx
                .get::<PathBuf>(OWNER_HOME_KEY)
                .ok_or("No owner_home")?
                .clone();
            let handle = ctx
                .get::<Handle>(OWNER_HOME_HANDLE_KEY)
                .ok_or("No owner_home handle")?
                .clone();
            let mut out = Context::new();
            out.set(OWNER_HOME_KEY, owner_home);
            out.set(OWNER_HOME_HANDLE_KEY, handle);
            out.set(MACHINE_YAML_KEY, fixture(&kind));
            Ok(out)
        },
    )
}

pub fn steps() -> Vec<StepDef> {
    vec![
        machine_fixture_step(
            "a driven measured machine.yaml without an obligation for kind {string}",
            anvil_test_support::machine_yaml_driven_measured_no_obligation,
        ),
        machine_fixture_step(
            "a driven measured machine.yaml with an obligation for kind {string}",
            anvil_test_support::machine_yaml_driven_measured_with_obligation,
        ),
        machine_fixture_step(
            "a free measured machine.yaml declaring an obligation for kind {string}",
            anvil_test_support::machine_yaml_free_measured_with_obligation,
        ),
        machine_fixture_step(
            "a free measured machine.yaml without an obligation for kind {string}",
            anvil_test_support::machine_yaml_free_measured_no_obligation,
        ),
        step_def(
            "the persist handler executes for kind {string} under evidence obligation enforcement {string}",
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (MACHINE_YAML_KEY, "String"),
            ],
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (OUTCOME_KEY, "bool"),
                (EVENT_COUNT_KEY, "usize"),
                (ERROR_CODE_KEY, "String"),
                (ERROR_VARIANT_KEY, "String"),
                (ERROR_MESSAGE_KEY, "String"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let obligation = match params
                    .get_string(1)
                    .ok_or("Expected evidence obligation enforcement state")?
                    .as_ref() as &str
                {
                    "on" => true,
                    "off" => false,
                    other => {
                        return Err(format!(
                            "Expected evidence obligation enforcement 'on' or 'off', got '{}'",
                            other
                        ))
                    }
                };
                let owner_home = ctx
                    .get::<PathBuf>(OWNER_HOME_KEY)
                    .ok_or("No owner_home")?
                    .clone();
                let handle = ctx
                    .get::<Handle>(OWNER_HOME_HANDLE_KEY)
                    .ok_or("No owner_home handle")?
                    .clone();
                let machine_yaml = ctx
                    .get::<String>(MACHINE_YAML_KEY)
                    .ok_or("No machine_yaml")?
                    .clone();

                let registry = HearthPlaybookRegistry::new(owner_home.clone());
                let request = PersistPlaybookRequest {
                    owner_home: owner_home.to_string_lossy().into_owned(),
                    kind,
                    machine_yaml,
                    exemplars: Vec::new(),
                    hooks: vec![(
                        "intent.md".to_string(),
                        "Persist intent hook body.".to_string(),
                    )],
                    actor_name: "Persist-Doer-1".to_string(),
                    actor_type: "agent".to_string(),
                    actor_model: "claude-opus-4-8".to_string(),
                    actor_provider: "anthropic".to_string(),
                };
                // Mirror the engine's persist WRITE-boundary posture: measurement
                // is always enforced here; the obligation leg is flag-gated. This
                // is exactly what the persist RPC threads in P4.
                let result = PersistPlaybookCommandHandler::execute_enforcing_with(
                    &registry,
                    request,
                    LoaderEnforcement {
                        measurement_definition: true,
                        evidence_obligation: obligation,
                    },
                );

                let mut out = Context::new();
                out.set(OWNER_HOME_KEY, owner_home);
                out.set(OWNER_HOME_HANDLE_KEY, handle);
                match result {
                    Ok(outcome) => {
                        let PersistPlaybookOutcome { events, .. } = outcome;
                        out.set(OUTCOME_KEY, true);
                        out.set(EVENT_COUNT_KEY, events.len());
                        out.set(ERROR_CODE_KEY, String::new());
                        out.set(ERROR_VARIANT_KEY, String::new());
                        out.set(ERROR_MESSAGE_KEY, String::new());
                    }
                    Err(e) => {
                        let variant = match &e {
                            PersistPlaybookError::LoaderInvalid(_) => "LoaderInvalid",
                            PersistPlaybookError::DuplicateKind { .. } => "DuplicateKind",
                            PersistPlaybookError::ExistingMachineInvalid { .. } => {
                                "ExistingMachineInvalid"
                            }
                            PersistPlaybookError::KindMismatch { .. } => "KindMismatch",
                            PersistPlaybookError::NonTerminalStateHookless { .. } => {
                                "NonTerminalStateHookless"
                            }
                            PersistPlaybookError::OwnerHomeRequired => "OwnerHomeRequired",
                            PersistPlaybookError::KindRequired => "KindRequired",
                            PersistPlaybookError::MachineYamlRequired => "MachineYamlRequired",
                            PersistPlaybookError::ActorNameRequired => "ActorNameRequired",
                            PersistPlaybookError::ActorParamsRequired { .. } => "ActorParamsRequired",
                            PersistPlaybookError::HearthRegistrationBlocked { .. } => {
                                "HearthRegistrationBlocked"
                            }
                        };
                        out.set(OUTCOME_KEY, false);
                        out.set(EVENT_COUNT_KEY, 0usize);
                        out.set(ERROR_CODE_KEY, e.code().to_string());
                        out.set(ERROR_VARIANT_KEY, variant.to_string());
                        out.set(ERROR_MESSAGE_KEY, e.to_string());
                    }
                }
                Ok(out)
            },
        ),
        step_def(
            "a registry built from an empty temp owner-home",
            &[],
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, _params| {
                let (path, handle) = new_temp()?;
                let mut out = Context::new();
                out.set(OWNER_HOME_KEY, path);
                out.set(OWNER_HOME_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "that owner-home carries definitions under BOTH the legacy and canonical hearth roots",
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, _params| {
                let owner_home = ctx.get::<PathBuf>(OWNER_HOME_KEY).ok_or("No owner_home")?.clone();
                let handle = ctx
                    .get::<Handle>(OWNER_HOME_HANDLE_KEY)
                    .ok_or("No owner_home handle")?
                    .clone();
                // A hearth-LEVEL fault, not an artifact one: only the canonical
                // root is scanned, so the definition under the legacy root is
                // shadowed and the hearth is not the root its definitions will be
                // read from. The seeded kinds are unrelated to the persisted kind
                // so nothing here is a duplicate-kind collision.
                //
                // SHADOW-ONLY, ZERO OVERLAP (C-d.1 round 4, LOW-5). This used to
                // seed the SAME id under both roots — a *collision* fixture — so
                // at this seam the scenario could not tell "block on shadowing"
                // from "block on name collision", which is the exact defect the
                // projection feature's own fixture was condemned for. The two
                // ids are now distinct, so only shadowing can be what blocks.
                for (root, kind) in [
                    ("workflows", "legacy_only_seeded_kind"),
                    ("playbooks", "canonical_only_seeded_kind"),
                ] {
                    let d = owner_home.join(root).join(kind);
                    std::fs::create_dir_all(d.join("hooks"))
                        .map_err(|e| format!("create dir: {}", e))?;
                    std::fs::write(d.join("machine.yaml"), anvil_test_support::conformant_machine_yaml(kind))
                        .map_err(|e| format!("write seed machine: {}", e))?;
                    std::fs::write(d.join("hooks").join("intent.md"), "Persist intent hook body.")
                        .map_err(|e| format!("write seed hook: {}", e))?;
                }
                let mut out = Context::new();
                out.set(OWNER_HOME_KEY, owner_home);
                out.set(OWNER_HOME_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "that owner-home carries a legacy definition directory that cannot be inspected",
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<ScratchDir>>>"),
            ],
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<ScratchDir>>>"),
            ],
            |ctx, _params| {
                let owner_home = ctx
                    .get::<PathBuf>(OWNER_HOME_KEY)
                    .ok_or("No owner_home")?
                    .clone();
                let handle = ctx
                    .get::<Handle>(OWNER_HOME_HANDLE_KEY)
                    .ok_or("No owner_home handle")?
                    .clone();
                // C-d.1 round 5, HIGH-1 AT THE WRITE BOUNDARY. Both roots are
                // present and READABLE; a single legacy definition DIRECTORY is
                // not. Round 4 answered `Ok(NothingToMove)` here — because
                // `{dir}/machine.yaml`'s `is_file()` mapped EACCES to `false`,
                // the shadowed set came back empty, and nothing refused — so
                // `registration_blocked()` was `None` and this handler emitted
                // its event: `persist = Ok(events=1)`, the round-3 signature,
                // intact after the round-4 fix.
                for (root, kind) in [
                    ("workflows", "legacy_only_seeded_kind"),
                    ("playbooks", "canonical_only_seeded_kind"),
                ] {
                    let d = owner_home.join(root).join(kind);
                    std::fs::create_dir_all(d.join("hooks"))
                        .map_err(|e| format!("create dir: {}", e))?;
                    std::fs::write(d.join("machine.yaml"), anvil_test_support::conformant_machine_yaml(kind))
                        .map_err(|e| format!("write seed machine: {}", e))?;
                    std::fs::write(d.join("hooks").join("intent.md"), "Persist intent hook body.")
                        .map_err(|e| format!("write seed hook: {}", e))?;
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(
                        owner_home.join("workflows").join("legacy_only_seeded_kind"),
                        std::fs::Permissions::from_mode(0o000),
                    )
                    .map_err(|e| format!("chmod: {}", e))?;
                }
                #[cfg(not(unix))]
                return Err("this fixture requires unix permissions".to_string());
                let mut out = Context::new();
                out.set(OWNER_HOME_KEY, owner_home);
                out.set(OWNER_HOME_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "a registry built from a temp owner-home already holding kind {string}",
            &[],
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let (path, handle) = new_temp()?;
                // Seed an existing machine.yaml so the registry resolves the kind.
                // Conformant (predicate + intent.md hook) so it is byte-identical
                // to the persist request in the idempotent scenario; the sibling
                // hook file is written so a re-load resolves the reference.
                let kind_dir = path.join("playbooks").join(&kind);
                std::fs::create_dir_all(kind_dir.join("hooks"))
                    .map_err(|e| format!("create dir: {}", e))?;
                std::fs::write(
                    kind_dir.join("machine.yaml"),
                    anvil_test_support::conformant_machine_yaml(&kind),
                )
                .map_err(|e| format!("write seed machine: {}", e))?;
                std::fs::write(
                    kind_dir.join("hooks").join("intent.md"),
                    "Persist intent hook body.",
                )
                .map_err(|e| format!("write seed hook: {}", e))?;
                let mut out = Context::new();
                out.set(OWNER_HOME_KEY, path);
                out.set(OWNER_HOME_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "a registry built from a temp owner-home already holding different content for kind {string}",
            &[],
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let (path, handle) = new_temp()?;
                let kind_dir = path.join("playbooks").join(&kind);
                std::fs::create_dir_all(&kind_dir).map_err(|e| format!("create dir: {}", e))?;
                std::fs::write(
                    kind_dir.join("machine.yaml"),
                    anvil_test_support::different_minimal_machine_yaml(&kind),
                )
                .map_err(|e| format!("write seed machine: {}", e))?;
                let mut out = Context::new();
                out.set(OWNER_HOME_KEY, path);
                out.set(OWNER_HOME_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "a registry built from a temp owner-home already holding semantically equivalent but byte-different content for kind {string}",
            &[],
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (SEEDED_BYTES_KEY, "Vec<u8>"),
            ],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let (path, handle) = new_temp()?;
                let kind_dir = path.join("playbooks").join(&kind);
                std::fs::create_dir_all(kind_dir.join("hooks"))
                    .map_err(|e| format!("create dir: {}", e))?;
                // Seed a comment-prefixed variant of the SAME conformant machine
                // the request carries: byte-different (leading comment) yet
                // semantically IDENTICAL (same predicate + intent.md hook). This
                // is what the scenario claims — the earlier comment-prefixed
                // `minimal_machine_yaml` seed was semantically DIFFERENT (no
                // predicate, no hook) so the scenario did not test equivalence.
                let bytes = format!(
                    "# byte-different but semantically equivalent\n{}",
                    anvil_test_support::conformant_machine_yaml(&kind)
                )
                .into_bytes();
                std::fs::write(kind_dir.join("machine.yaml"), &bytes)
                    .map_err(|e| format!("write seed machine: {}", e))?;
                // The conformant seed references intent.md; write it beside the
                // machine so the preflight re-load resolves the reference and the
                // handler reaches the genuine DuplicateKind branch (not
                // ExistingMachineInvalid on an unknown-hook).
                std::fs::write(
                    kind_dir.join("hooks").join("intent.md"),
                    "Persist intent hook body.",
                )
                .map_err(|e| format!("write seed hook: {}", e))?;
                let mut out = Context::new();
                out.set(OWNER_HOME_KEY, path);
                out.set(OWNER_HOME_HANDLE_KEY, handle);
                out.set(SEEDED_BYTES_KEY, bytes);
                Ok(out)
            },
        ),
        step_def(
            "a registry built from a temp owner-home already holding invalid content for kind {string}",
            &[],
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let (path, handle) = new_temp()?;
                let kind_dir = path.join("playbooks").join(&kind);
                std::fs::create_dir_all(&kind_dir).map_err(|e| format!("create dir: {}", e))?;
                std::fs::write(kind_dir.join("machine.yaml"), "not: [valid\n")
                    .map_err(|e| format!("write invalid seed machine: {}", e))?;
                let mut out = Context::new();
                out.set(OWNER_HOME_KEY, path);
                out.set(OWNER_HOME_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "a minimal valid machine.yaml for kind {string}",
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (MACHINE_YAML_KEY, "String"),
                (SEEDED_BYTES_KEY, "Vec<u8>"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let owner_home = ctx
                    .get::<PathBuf>(OWNER_HOME_KEY)
                    .ok_or("No owner_home")?
                    .clone();
                let handle = ctx
                    .get::<Handle>(OWNER_HOME_HANDLE_KEY)
                    .ok_or("No owner_home handle")?
                    .clone();
                let mut out = Context::new();
                out.set(OWNER_HOME_KEY, owner_home);
                out.set(OWNER_HOME_HANDLE_KEY, handle);
                // WRITE-boundary conformant: outcome_predicate + hooked non-terminal
                // state (references intent.md, which the execute step carries).
                out.set(MACHINE_YAML_KEY, anvil_test_support::conformant_machine_yaml(kind));
                if let Some(seeded) = ctx.get::<Vec<u8>>(SEEDED_BYTES_KEY) {
                    out.set(SEEDED_BYTES_KEY, seeded.clone());
                }
                Ok(out)
            },
        ),
        step_def(
            "a machine.yaml with no outcome_predicate for kind {string}",
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (MACHINE_YAML_KEY, "String"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let owner_home = ctx
                    .get::<PathBuf>(OWNER_HOME_KEY)
                    .ok_or("No owner_home")?
                    .clone();
                let handle = ctx
                    .get::<Handle>(OWNER_HOME_HANDLE_KEY)
                    .ok_or("No owner_home handle")?
                    .clone();
                let mut out = Context::new();
                out.set(OWNER_HOME_KEY, owner_home);
                out.set(OWNER_HOME_HANDLE_KEY, handle);
                // minimal_machine_yaml declares no outcome_predicate.
                out.set(MACHINE_YAML_KEY, anvil_test_support::minimal_machine_yaml(kind));
                Ok(out)
            },
        ),
        step_def(
            "a machine.yaml with an outcome_predicate but a hookless non-terminal state for kind {string}",
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (MACHINE_YAML_KEY, "String"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let owner_home = ctx
                    .get::<PathBuf>(OWNER_HOME_KEY)
                    .ok_or("No owner_home")?
                    .clone();
                let handle = ctx
                    .get::<Handle>(OWNER_HOME_HANDLE_KEY)
                    .ok_or("No owner_home handle")?
                    .clone();
                let mut out = Context::new();
                out.set(OWNER_HOME_KEY, owner_home);
                out.set(OWNER_HOME_HANDLE_KEY, handle);
                out.set(MACHINE_YAML_KEY, anvil_test_support::machine_yaml_predicate_no_hook(kind));
                Ok(out)
            },
        ),
        step_def(
            "a machine.yaml whose initial state carries only a reviewer hook for kind {string}",
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (MACHINE_YAML_KEY, "String"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let owner_home = ctx
                    .get::<PathBuf>(OWNER_HOME_KEY)
                    .ok_or("No owner_home")?
                    .clone();
                let handle = ctx
                    .get::<Handle>(OWNER_HOME_HANDLE_KEY)
                    .ok_or("No owner_home handle")?
                    .clone();
                let mut out = Context::new();
                out.set(OWNER_HOME_KEY, owner_home);
                out.set(OWNER_HOME_HANDLE_KEY, handle);
                out.set(MACHINE_YAML_KEY, anvil_test_support::machine_yaml_reviewer_only_hook(kind));
                Ok(out)
            },
        ),
        step_def(
            "a transitionless event-driven machine.yaml whose non-initial state carries only a custom-role hook for kind {string}",
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (MACHINE_YAML_KEY, "String"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let owner_home = ctx
                    .get::<PathBuf>(OWNER_HOME_KEY)
                    .ok_or("No owner_home")?
                    .clone();
                let handle = ctx
                    .get::<Handle>(OWNER_HOME_HANDLE_KEY)
                    .ok_or("No owner_home handle")?
                    .clone();
                let mut out = Context::new();
                out.set(OWNER_HOME_KEY, owner_home);
                out.set(OWNER_HOME_HANDLE_KEY, handle);
                out.set(
                    MACHINE_YAML_KEY,
                    anvil_test_support::machine_yaml_event_driven_custom_role_hook(kind),
                );
                Ok(out)
            },
        ),
        step_def(
            "a projection-only machine.yaml with transitions whose non-initial state carries only a worker hook for kind {string}",
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (MACHINE_YAML_KEY, "String"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let owner_home = ctx
                    .get::<PathBuf>(OWNER_HOME_KEY)
                    .ok_or("No owner_home")?
                    .clone();
                let handle = ctx
                    .get::<Handle>(OWNER_HOME_HANDLE_KEY)
                    .ok_or("No owner_home handle")?
                    .clone();
                let mut out = Context::new();
                out.set(OWNER_HOME_KEY, owner_home);
                out.set(OWNER_HOME_HANDLE_KEY, handle);
                out.set(
                    MACHINE_YAML_KEY,
                    anvil_test_support::machine_yaml_projection_only_worker_state(kind),
                );
                Ok(out)
            },
        ),
        step_def(
            "a loader-invalid machine.yaml for kind {string}",
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (MACHINE_YAML_KEY, "String"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let owner_home = ctx
                    .get::<PathBuf>(OWNER_HOME_KEY)
                    .ok_or("No owner_home")?
                    .clone();
                let handle = ctx
                    .get::<Handle>(OWNER_HOME_HANDLE_KEY)
                    .ok_or("No owner_home handle")?
                    .clone();
                let mut out = Context::new();
                out.set(OWNER_HOME_KEY, owner_home);
                out.set(OWNER_HOME_HANDLE_KEY, handle);
                out.set(MACHINE_YAML_KEY, anvil_test_support::loader_invalid_machine_yaml(kind));
                Ok(out)
            },
        ),
        step_def(
            "the persist handler executes for kind {string}",
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (MACHINE_YAML_KEY, "String"),
            ],
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (OUTCOME_KEY, "bool"),
                (EVENT_COUNT_KEY, "usize"),
                (ERROR_CODE_KEY, "String"),
                (ERROR_VARIANT_KEY, "String"),
                (ERROR_MESSAGE_KEY, "String"),
                (SEEDED_BYTES_KEY, "Vec<u8>"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let owner_home = ctx
                    .get::<PathBuf>(OWNER_HOME_KEY)
                    .ok_or("No owner_home")?
                    .clone();
                let handle = ctx
                    .get::<Handle>(OWNER_HOME_HANDLE_KEY)
                    .ok_or("No owner_home handle")?
                    .clone();
                let machine_yaml = ctx
                    .get::<String>(MACHINE_YAML_KEY)
                    .ok_or("No machine_yaml")?
                    .clone();

                let registry = HearthPlaybookRegistry::new(owner_home.clone());
                let request = PersistPlaybookRequest {
                    owner_home: owner_home.to_string_lossy().into_owned(),
                    kind,
                    machine_yaml,
                    exemplars: Vec::new(),
                    // Carry the `intent.md` hook the conformant fixtures reference
                    // so the enforcing loader resolves it. Harmless (unreferenced)
                    // for the no-predicate / hookless red fixtures, which fail
                    // before hook resolution matters.
                    hooks: vec![("intent.md".to_string(), "Persist intent hook body.".to_string())],
                    actor_name: "Persist-Doer-1".to_string(),
                    actor_type: "agent".to_string(),
                    actor_model: "claude-opus-4-8".to_string(),
                    actor_provider: "anthropic".to_string(),
                };
                // The persist WRITE boundary: enforcing loader + hook-coverage.
                let result = PersistPlaybookCommandHandler::execute_enforcing(&registry, request);

                let mut out = Context::new();
                out.set(OWNER_HOME_KEY, owner_home);
                out.set(OWNER_HOME_HANDLE_KEY, handle);
                if let Some(seeded) = ctx.get::<Vec<u8>>(SEEDED_BYTES_KEY) {
                    out.set(SEEDED_BYTES_KEY, seeded.clone());
                }
                match result {
                    Ok(outcome) => {
                        let PersistPlaybookOutcome { events, .. } = outcome;
                        out.set(OUTCOME_KEY, true);
                        out.set(EVENT_COUNT_KEY, events.len());
                        out.set(ERROR_CODE_KEY, String::new());
                        out.set(ERROR_VARIANT_KEY, String::new());
                        out.set(ERROR_MESSAGE_KEY, String::new());
                    }
                    Err(e) => {
                        let variant = match &e {
                            PersistPlaybookError::LoaderInvalid(_) => "LoaderInvalid",
                            PersistPlaybookError::DuplicateKind { .. } => "DuplicateKind",
                            PersistPlaybookError::ExistingMachineInvalid { .. } => {
                                "ExistingMachineInvalid"
                            }
                            PersistPlaybookError::KindMismatch { .. } => "KindMismatch",
                            PersistPlaybookError::NonTerminalStateHookless { .. } => {
                                "NonTerminalStateHookless"
                            }
                            PersistPlaybookError::OwnerHomeRequired => "OwnerHomeRequired",
                            PersistPlaybookError::KindRequired => "KindRequired",
                            PersistPlaybookError::MachineYamlRequired => "MachineYamlRequired",
                            PersistPlaybookError::ActorNameRequired => "ActorNameRequired",
                            PersistPlaybookError::ActorParamsRequired { .. } => {
                                "ActorParamsRequired"
                            }
                            PersistPlaybookError::HearthRegistrationBlocked { .. } => {
                                "HearthRegistrationBlocked"
                            }
                        };
                        out.set(OUTCOME_KEY, false);
                        out.set(EVENT_COUNT_KEY, 0usize);
                        out.set(ERROR_CODE_KEY, e.code().to_string());
                        out.set(ERROR_VARIANT_KEY, variant.to_string());
                        out.set(ERROR_MESSAGE_KEY, e.to_string());
                    }
                }
                Ok(out)
            },
        ),
        check_def(
            "the persist handler returns one PlaybookPersisted event",
            &[(OUTCOME_KEY, "bool"), (EVENT_COUNT_KEY, "usize")],
            |ctx, _params| {
                let ok = ctx.get::<bool>(OUTCOME_KEY).ok_or("No outcome")?;
                let count = ctx.get::<usize>(EVENT_COUNT_KEY).ok_or("No event count")?;
                if *ok && *count == 1 {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected ok with 1 event, got ok={} count={}",
                        ok, count
                    ))
                }
            },
        ),
        check_def(
            "the persist handler succeeds",
            &[(OUTCOME_KEY, "bool")],
            |ctx, _params| {
                let ok = ctx.get::<bool>(OUTCOME_KEY).ok_or("No outcome")?;
                if *ok {
                    Ok(())
                } else {
                    let variant = ctx
                        .get::<String>(ERROR_VARIANT_KEY)
                        .map(String::as_str)
                        .unwrap_or("<unknown>");
                    let code = ctx
                        .get::<String>(ERROR_CODE_KEY)
                        .map(String::as_str)
                        .unwrap_or("<unknown>");
                    Err(format!(
                        "Expected successful persist outcome, got {} ({})",
                        variant, code
                    ))
                }
            },
        ),
        check_def(
            "the PlaybookPersisted event carries kind {string}",
            &[(OUTCOME_KEY, "bool")],
            |ctx, _params| {
                // The handler echoes the request kind into the event; success +
                // one-event already proves the carried kind (asserted above). Here
                // we only re-confirm the success path produced an event.
                let ok = ctx.get::<bool>(OUTCOME_KEY).ok_or("No outcome")?;
                if *ok {
                    Ok(())
                } else {
                    Err("Expected a successful persist outcome".to_string())
                }
            },
        ),
        check_def(
            "the persist handler returns a LoaderInvalid error",
            &[(ERROR_VARIANT_KEY, "String")],
            |ctx, _params| {
                let v = ctx
                    .get::<String>(ERROR_VARIANT_KEY)
                    .ok_or("No error variant")?;
                if v == "LoaderInvalid" {
                    Ok(())
                } else {
                    Err(format!("Expected LoaderInvalid, got '{}'", v))
                }
            },
        ),
        check_def(
            "the persist handler returns a HearthRegistrationBlocked error",
            &[(ERROR_VARIANT_KEY, "String")],
            |ctx, _params| {
                let v = ctx
                    .get::<String>(ERROR_VARIANT_KEY)
                    .ok_or("No error variant")?;
                if v == "HearthRegistrationBlocked" {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected HearthRegistrationBlocked, got '{}'. The loader reports this \
                         hearth as blocked; a writer that does not ask is not blocked by the \
                         report.",
                        v
                    ))
                }
            },
        ),
        check_def(
            "the persist handler error names both hearth roots",
            &[(ERROR_MESSAGE_KEY, "String")],
            |ctx, _params| {
                let m = ctx
                    .get::<String>(ERROR_MESSAGE_KEY)
                    .ok_or("No error message")?;
                if !m.contains("workflows") || !m.contains("playbooks") {
                    return Err(format!(
                        "the refusal does not name both roots, so an operator cannot act on it: {m}"
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the persist handler error names the unreadable root",
            &[(ERROR_MESSAGE_KEY, "String")],
            |ctx, _params| {
                let m = ctx
                    .get::<String>(ERROR_MESSAGE_KEY)
                    .ok_or("No error message")?;
                if !m.contains("hearth_root_unreadable") {
                    return Err(format!(
                        "the write boundary refused for the wrong reason, or not at all. A legacy \
                         definition directory that cannot be inspected must refuse AS an \
                         unreadable root — round 4 mapped that EACCES to `false`, concluded the \
                         legacy root held nothing, and let the persist emit its event: {m}"
                    ));
                }
                if !m.contains("legacy_only_seeded_kind") {
                    return Err(format!(
                        "the refusal does not name the entry it could not inspect, so an operator \
                         cannot tell WHICH path to fix: {m}"
                    ));
                }
                Ok(())
            },
        ),
        // C-d.1 round 4 (LOW-3). The operator-facing string at the RPC read
        // `hearth_registration_blocked: … — hearth_directory_collision:
        // hearth_directory_collision: BOTH …`: the load error's Display prefixed
        // a code that its own `detail` — the projection error's Display — already
        // carried. Fixed in code, and pinned here so the fix is not a comment.
        check_def(
            "the persist handler error names the collision code exactly once",
            &[(ERROR_MESSAGE_KEY, "String")],
            |ctx, _params| {
                let m = ctx
                    .get::<String>(ERROR_MESSAGE_KEY)
                    .ok_or("No error message")?;
                let n = m.matches("hearth_directory_collision:").count();
                if n != 1 {
                    return Err(format!(
                        "the operator-facing refusal names the code {n} times, not once: {m}"
                    ));
                }
                Ok(())
            },
        ),
        // C-d.1 round 4 (MEDIUM-3 ii). The assertion that USED to live here —
        // "no machine.yaml for kind X was written under either hearth root" —
        // COULD NOT FAIL. `PersistPlaybookCommandHandler` is a pure CQRS handler
        // that emits events and never touches the filesystem; the
        // `FileSystemArtifactAdapter` is a different seam entirely, so no
        // implementation of the handler could redden a `stat`. It was cited in
        // the record as part of what the scenario proves. It is replaced by the
        // assertion below, which CAN fail and pins the property the code claims:
        // the hearth question is asked BEFORE the request is validated.
        check_def(
            "the refusal names the hearth rather than the request",
            &[(ERROR_MESSAGE_KEY, "String"), (ERROR_VARIANT_KEY, "String")],
            |ctx, _params| {
                let v = ctx
                    .get::<String>(ERROR_VARIANT_KEY)
                    .ok_or("No error variant")?;
                let m = ctx
                    .get::<String>(ERROR_MESSAGE_KEY)
                    .ok_or("No error message")?;
                if v != "HearthRegistrationBlocked" {
                    return Err(format!(
                        "the request carries a SECOND, independent fault and the handler answered \
                         about THAT ({v}: {m}). The hearth check is documented as asked FIRST, \
                         because it is a question about the TARGET: every later validation is \
                         about a write that would produce an artifact nothing serves. Moving the \
                         check below any request guard reds exactly here."
                    ));
                }
                Ok(())
            },
        ),
        step_def(
            "a request carrying no machine.yaml for kind {string}",
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (MACHINE_YAML_KEY, "String"),
            ],
            |ctx, _params| {
                let owner_home = ctx
                    .get::<PathBuf>(OWNER_HOME_KEY)
                    .ok_or("No owner_home")?
                    .clone();
                let handle = ctx
                    .get::<Handle>(OWNER_HOME_HANDLE_KEY)
                    .ok_or("No owner_home handle")?
                    .clone();
                let mut out = Context::new();
                out.set(OWNER_HOME_KEY, owner_home);
                out.set(OWNER_HOME_HANDLE_KEY, handle);
                // An EMPTY machine_yaml is refused by `MachineYamlRequired`, a
                // REQUEST guard that sits below the hearth guard. Which of the
                // two answers comes back is exactly the ordering under test.
                out.set(MACHINE_YAML_KEY, String::new());
                Ok(out)
            },
        ),
        check_def(
            "the persist handler returns a DuplicateKind error",
            &[(ERROR_VARIANT_KEY, "String")],
            |ctx, _params| {
                let v = ctx
                    .get::<String>(ERROR_VARIANT_KEY)
                    .ok_or("No error variant")?;
                if v == "DuplicateKind" {
                    Ok(())
                } else {
                    Err(format!("Expected DuplicateKind, got '{}'", v))
                }
            },
        ),
        check_def(
            "the persist handler returns an ExistingMachineInvalid error",
            &[(ERROR_VARIANT_KEY, "String")],
            |ctx, _params| {
                let v = ctx
                    .get::<String>(ERROR_VARIANT_KEY)
                    .ok_or("No error variant")?;
                if v == "ExistingMachineInvalid" {
                    Ok(())
                } else {
                    Err(format!("Expected ExistingMachineInvalid, got '{}'", v))
                }
            },
        ),
        check_def(
            "the persist handler returns a KindMismatch error",
            &[(ERROR_VARIANT_KEY, "String")],
            |ctx, _params| {
                let v = ctx
                    .get::<String>(ERROR_VARIANT_KEY)
                    .ok_or("No error variant")?;
                if v == "KindMismatch" {
                    Ok(())
                } else {
                    Err(format!("Expected KindMismatch, got '{}'", v))
                }
            },
        ),
        check_def(
            "the persist handler returns a NonTerminalStateHookless error",
            &[(ERROR_VARIANT_KEY, "String")],
            |ctx, _params| {
                let v = ctx
                    .get::<String>(ERROR_VARIANT_KEY)
                    .ok_or("No error variant")?;
                if v == "NonTerminalStateHookless" {
                    Ok(())
                } else {
                    Err(format!("Expected NonTerminalStateHookless, got '{}'", v))
                }
            },
        ),
        check_def(
            "the persist handler error names kind {string}",
            &[(ERROR_MESSAGE_KEY, "String")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected kind")?;
                let message = ctx
                    .get::<String>(ERROR_MESSAGE_KEY)
                    .ok_or("No error message")?;
                if message.contains(&format!("'{}'", expected)) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected error to name kind '{}', got: {}",
                        expected, message
                    ))
                }
            },
        ),
        check_def(
            "the persist handler error names state {string}",
            &[(ERROR_MESSAGE_KEY, "String")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?;
                let message = ctx
                    .get::<String>(ERROR_MESSAGE_KEY)
                    .ok_or("No error message")?;
                if message.contains(&format!("'{}'", expected)) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected error to name state '{}', got: {}",
                        expected, message
                    ))
                }
            },
        ),
        check_def(
            "the persist handler error names role {string}",
            &[(ERROR_MESSAGE_KEY, "String")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected role")?;
                let message = ctx
                    .get::<String>(ERROR_MESSAGE_KEY)
                    .ok_or("No error message")?;
                if message.contains(&format!("'{}'", expected)) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected error to name role '{}', got: {}",
                        expected, message
                    ))
                }
            },
        ),
        check_def(
            "the persist handler error code is {string}",
            &[(ERROR_CODE_KEY, "String")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected code")?;
                let code = ctx.get::<String>(ERROR_CODE_KEY).ok_or("No error code")?;
                if code == expected {
                    Ok(())
                } else {
                    Err(format!("Expected code '{}', got '{}'", expected, code))
                }
            },
        ),
        check_def(
            "the persist handler emits no event",
            &[(EVENT_COUNT_KEY, "usize")],
            |ctx, _params| {
                let count = ctx.get::<usize>(EVENT_COUNT_KEY).ok_or("No event count")?;
                if *count == 0 {
                    Ok(())
                } else {
                    Err(format!("Expected 0 events, got {}", count))
                }
            },
        ),
        check_def(
            "the existing machine.yaml for kind {string} still contains {string}",
            &[(OWNER_HOME_KEY, "PathBuf")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let expected = params.get_string(1).ok_or("Expected text")?;
                let owner_home = ctx.get::<PathBuf>(OWNER_HOME_KEY).ok_or("No owner_home")?;
                let path = owner_home.join("playbooks").join(kind).join("machine.yaml");
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("read {}: {}", path.display(), e))?;
                if content.contains(expected) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} to contain '{}', got:\n{}",
                        path.display(),
                        expected,
                        content
                    ))
                }
            },
        ),
        check_def(
            "the handler target machine.yaml bytes for kind {string} are unchanged",
            &[(OWNER_HOME_KEY, "PathBuf"), (SEEDED_BYTES_KEY, "Vec<u8>")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let owner_home = ctx.get::<PathBuf>(OWNER_HOME_KEY).ok_or("No owner_home")?;
                let expected = ctx.get::<Vec<u8>>(SEEDED_BYTES_KEY).ok_or("No seeded bytes")?;
                let path = owner_home.join("playbooks").join(kind).join("machine.yaml");
                let actual =
                    std::fs::read(&path).map_err(|e| format!("read {}: {}", path.display(), e))?;
                if &actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} bytes to remain unchanged ({} bytes), got {} bytes",
                        path.display(),
                        expected.len(),
                        actual.len()
                    ))
                }
            },
        ),
    ]
}
