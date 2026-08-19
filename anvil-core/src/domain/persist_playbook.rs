//! Pure CQRS handler for the persist-generated-playbook primitive (track 1a).
//!
//! `PersistPlaybookCommandHandler::execute` loader-validates a generated machine
//! FIRST, checks the exact owner-home target file as a fast path, and only on
//! all-pass emits a single `PlaybookPersisted` event. The emitted event is the
//! write trigger routed by the engine to the port. If the same kind already
//! exists with byte-identical `machine.yaml` content, the handler treats the
//! request as already persisted and returns success with NO event. A loader-invalid
//! machine, same-kind/different-content preflight collision, or
//! present-but-invalid existing target returns a typed error and emits NO event.
//! The filesystem write boundary remains authoritative for races after this
//! preflight: it must create the target exclusively and compare-on-exists before
//! any success is reported.
//!
//! SCOPE CAVEAT (acknowledged): `execute_enforcing`'s measurement + hook-coverage
//! gates are an RPC-INGRESS gate on the `persist_playbook` seam, NOT a universal
//! write boundary for the hearth. Foundry kit-registration copies and manual
//! authoring write `machine.yaml` files directly and bypass this handler
//! entirely; for those paths the enforcing loader at registration
//! (`HearthPlaybookRegistry` construction with measurement enforcement) remains
//! the gate. This handler only guarantees that a machine persisted THROUGH the
//! RPC cannot land predicate-less or serve a blind begin.
//!
//! KNOWN SERVE-SIDE TOLERANCE (out of this gate's scope): the persist
//! hook-coverage gate mirrors the role `begin` serves per state — a review gate
//! is served the `reviewer` hook — but the route-resume path
//! (`resume_response_for_open_playbook_run`, engine `main.rs`) re-serves the
//! `(state, doer)` hook fail-open (`serve_hook_body(..., "doer")` →
//! `.unwrap_or_default()`). The existing corpus's review gates are reviewer-only
//! by house convention, so resuming AT a review gate yields blank doer resume
//! guidance. This is a serve-side fail-open tolerance, NOT a persist-time defect:
//! the gate does NOT force review gates to carry a doer hook (that would reject
//! the entire conformant corpus), and the resume serve is fail-open by design.

use crate::domain::persist_playbook_events::PersistPlaybookEvent;
use crate::domain::playbook::candidate::GeneratedExemplarFile;
use crate::domain::playbook::interpreter::state_role_hook;
use crate::domain::playbook::load_error::PlaybookLoadError;
use crate::domain::playbook::fs_probe;
use crate::domain::playbook::loader::{load_from_yaml, load_from_yaml_with, LoaderEnforcement};
use crate::domain::playbook::registry::PlaybookRegistry;
use std::fmt;
use std::path::Path;

/// Engine-stamped request to persist a generated playbook to an owner-home.
#[derive(Debug, Clone, Default)]
pub struct PersistPlaybookRequest {
    /// Absolute path to the target owner-home (the write + dup-check target).
    pub owner_home: String,
    /// The playbook kind (dir name under `<owner_home>/playbooks/`).
    /// Legacy `<owner_home>/workflows/` remains readable.
    pub kind: String,
    /// The generated `machine.yaml` text.
    pub machine_yaml: String,
    pub exemplars: Vec<GeneratedExemplarFile>,
    /// Hook files (filename, content) written beside `machine.yaml` under
    /// `hooks/`. The machine is loader-validated against these filenames, so a
    /// machine referencing a hook NOT carried here is rejected. Empty = today's
    /// machine.yaml-only behavior.
    pub hooks: Vec<(String, String)>,
    pub actor_name: String,
    pub actor_type: String,
    pub actor_model: String,
    pub actor_provider: String,
}

/// The result of a successful persist command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistPlaybookResult {
    pub kind: String,
}

/// The outcome of `PersistPlaybookCommandHandler::execute`: the result plus the
/// events the engine must route.
#[derive(Debug, Clone)]
pub struct PersistPlaybookOutcome {
    pub result: PersistPlaybookResult,
    pub events: Vec<PersistPlaybookEvent>,
}

/// Typed errors from the persist handler. Each carries a stable snake_case
/// `code()`; `LoaderInvalid` delegates to the wrapped `PlaybookLoadError::code()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersistPlaybookError {
    OwnerHomeRequired,
    KindRequired,
    MachineYamlRequired,
    ActorNameRequired,
    ActorParamsRequired {
        field: String,
    },
    /// The generated machine failed the loader (parse or cross-reference).
    LoaderInvalid(PlaybookLoadError),
    /// The request kind and declared machine.yaml kind differ.
    KindMismatch {
        argument_kind: String,
        declared_kind: String,
    },
    /// A different machine with this kind already resolves at the owner-home.
    DuplicateKind {
        kind: String,
        existing_id: String,
    },
    /// The exact target file exists but cannot be read or loaded as the same
    /// playbook machine. Fail closed so retries never overwrite an unknown
    /// existing declaration.
    ExistingMachineInvalid {
        kind: String,
        path: String,
        message: String,
    },
    /// A state the interpreter would serve resolves NO begin hook for the role
    /// it would serve there. Emitted ONLY at the WRITE boundary
    /// (`execute_enforcing`) and resolved with the interpreter's own
    /// `state_role_hook` (`hooks_by_role[role]` → role-agnostic `hook` fallback),
    /// so this gate and `begin` agree exactly: the initial state's `doer` hook is
    /// served on every create/adopt (incl. a projection-only single-state
    /// machine), a review gate serves the `reviewer` hook, and every other
    /// working state serves the `doer` hook. A state that declares only a
    /// role-mismatched `hooks_by_role` entry (e.g. reviewer-only where the doer
    /// is served) therefore fails closed here rather than masking a blind begin.
    /// The loaded corpus and the generator's hookless-by-construction terminal
    /// persist (plain `execute`) are exempt. A machine OUTSIDE the per-instance
    /// begin/complete progression is only checked at its INITIAL state: its
    /// non-initial states are driven by the machine's own queue/event/projection
    /// contract and may carry any custom-role hook or none. Two exemption classes
    /// qualify: (1) an empty transition graph (`transitions.is_empty()` —
    /// event/step/queue-driven, e.g. `import_transaction_history`); and (2) a
    /// projection-only machine (`projection_only`), whose `begin` serves only the
    /// initial doer hook and emits a snapshot with NO per-instance progression,
    /// EVEN when it declares a full transition graph.
    NonTerminalStateHookless {
        kind: String,
        state: String,
        role: String,
    },
    /// The TARGET hearth reports registration as blocked (C9): a failed
    /// top-level `workflows/` -> `playbooks/` move, or both roots present. The
    /// canonical root is not the root this hearth's definitions are read from,
    /// so a definition written here would be served by nothing. Refused before
    /// any validation, and before any write.
    HearthRegistrationBlocked {
        detail: String,
    },
}

impl PersistPlaybookError {
    /// The stable snake_case error code for this variant. For `LoaderInvalid`,
    /// delegates to the wrapped `PlaybookLoadError` code.
    pub fn code(&self) -> &str {
        match self {
            PersistPlaybookError::OwnerHomeRequired => "owner_home_required",
            PersistPlaybookError::KindRequired => "kind_required",
            PersistPlaybookError::MachineYamlRequired => "machine_yaml_required",
            PersistPlaybookError::ActorNameRequired => "actor_name_required",
            PersistPlaybookError::ActorParamsRequired { .. } => "actor_params_required",
            PersistPlaybookError::LoaderInvalid(e) => e.code(),
            PersistPlaybookError::KindMismatch { .. } => "kind_mismatch",
            PersistPlaybookError::DuplicateKind { .. } => "playbook_duplicate_kind_registration",
            PersistPlaybookError::ExistingMachineInvalid { .. } => {
                "playbook_existing_machine_invalid"
            }
            PersistPlaybookError::NonTerminalStateHookless { .. } => {
                "playbook_non_terminal_state_hookless"
            }
            PersistPlaybookError::HearthRegistrationBlocked { .. } => "hearth_registration_blocked",
        }
    }
}

impl fmt::Display for PersistPlaybookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PersistPlaybookError::OwnerHomeRequired => {
                write!(f, "owner_home_required: owner_home must not be empty")
            }
            PersistPlaybookError::KindRequired => {
                write!(f, "kind_required: kind must not be empty")
            }
            PersistPlaybookError::MachineYamlRequired => {
                write!(f, "machine_yaml_required: machine_yaml must not be empty")
            }
            PersistPlaybookError::ActorNameRequired => {
                write!(f, "actor_name_required: actor_name must not be empty")
            }
            PersistPlaybookError::ActorParamsRequired { field } => {
                write!(f, "actor_params_required: {} must not be empty", field)
            }
            PersistPlaybookError::LoaderInvalid(e) => write!(f, "{}", e),
            PersistPlaybookError::KindMismatch {
                argument_kind,
                declared_kind,
            } => write!(
                f,
                "kind_mismatch: argument kind '{}' does not match machine.yaml kind '{}'",
                argument_kind, declared_kind
            ),
            PersistPlaybookError::DuplicateKind { kind, existing_id } => write!(
                f,
                "playbook_duplicate_kind_registration: kind '{}' already resolves at the owner-home (existing: {})",
                kind, existing_id
            ),
            PersistPlaybookError::ExistingMachineInvalid {
                kind,
                path,
                message,
            } => write!(
                f,
                "playbook_existing_machine_invalid: existing machine for kind '{}' at '{}' could not be loaded: {}",
                kind, path, message
            ),
            PersistPlaybookError::NonTerminalStateHookless { kind, state, role } => write!(
                f,
                "playbook_non_terminal_state_hookless: kind '{}' state '{}' serves no '{}' begin hook (state_role_hook resolves neither hooks_by_role['{}'] nor a role-agnostic `hook`)",
                kind, state, role, role
            ),
            PersistPlaybookError::HearthRegistrationBlocked { detail } => write!(
                f,
                "hearth_registration_blocked: the target hearth may not be registered into — {}",
                detail
            ),
        }
    }
}

impl std::error::Error for PersistPlaybookError {}

pub struct PersistPlaybookCommandHandler;

impl PersistPlaybookCommandHandler {
    /// Validate the request + machine, detect duplicate kinds via `registry`, and
    /// emit a single `PlaybookPersisted` event on success.
    ///
    /// Order (CONFIRMED no-partial-write + retry-safe terminal persist):
    /// (a) required-field guards;
    /// (b) loader-validate via `load_from_yaml` → `LoaderInvalid` on error;
    /// (c) byte-exact fast-path check of `<owner_home>/playbooks/<kind>/machine.yaml`
    ///     and legacy `<owner_home>/workflows/<kind>/machine.yaml`;
    /// (d) byte-identical existing content returns success with zero events;
    /// (e) only on fresh all-pass, emit `PlaybookPersisted` for the filesystem
    ///     write boundary to finalize via exclusive create + compare-on-exists.
    pub fn execute(
        registry: &dyn PlaybookRegistry,
        request: PersistPlaybookRequest,
    ) -> Result<PersistPlaybookOutcome, PersistPlaybookError> {
        Self::execute_impl(registry, request, LoaderEnforcement::OFF)
    }

    /// Write-boundary variant of [`execute`]. The persist WRITE boundary should
    /// always refuse new junk, regardless of the runtime measurement dark-gate:
    ///
    /// - loader-validate via `load_from_yaml_enforcing` — a missing/blank
    ///   `outcome_predicate` or a measured state without `success_criteria`
    ///   fails closed as `LoaderInvalid(MeasurementDefinitionMissing)`
    ///   (`playbook_measurement_definition_missing`);
    /// - every non-terminal state must serve a begin hook, else
    ///   `NonTerminalStateHookless` (`playbook_non_terminal_state_hookless`).
    ///
    /// The runtime dark-gate protects the LOADED corpus; this guards the WRITE.
    /// The generator's terminal persist (which emits candidate-threaded,
    /// hookless-by-construction machines) stays on plain [`execute`].
    pub fn execute_enforcing(
        registry: &dyn PlaybookRegistry,
        request: PersistPlaybookRequest,
    ) -> Result<PersistPlaybookOutcome, PersistPlaybookError> {
        Self::execute_impl(registry, request, LoaderEnforcement::MEASUREMENT)
    }

    /// Evidence-obligation-aware variant of [`execute_enforcing`] (T-EEC-1 P3).
    ///
    /// Threads a full [`LoaderEnforcement`] to the loader-validate step so the
    /// persist WRITE boundary can run the evidence-obligation leg IN ADDITION to
    /// the always-on measurement-definition gate. The engine's persist RPC calls
    /// this with `{ measurement_definition: true, evidence_obligation:
    /// enforce_evidence_obligation() }` (P4) — so with the obligation flag OFF
    /// this reduces EXACTLY to [`execute_enforcing`] (obligation leg skipped),
    /// preserving today's behavior; with it ON, a DRIVEN measured `(state, role)`
    /// lacking an obligation is refused with
    /// `playbook_evidence_obligation_missing`, and a FREE-registered machine
    /// declaring one is refused with `playbook_evidence_obligation_on_free_register`,
    /// both surfaced through the existing `LoaderInvalid` wrapper.
    ///
    /// The obligation errors ride the loader's `LoaderEnforcement`; the
    /// hook-coverage WRITE-boundary gate remains keyed to the measurement leg
    /// (always on at persist), so it is unchanged from `execute_enforcing`.
    pub fn execute_enforcing_with(
        registry: &dyn PlaybookRegistry,
        request: PersistPlaybookRequest,
        enforcement: LoaderEnforcement,
    ) -> Result<PersistPlaybookOutcome, PersistPlaybookError> {
        Self::execute_impl(registry, request, enforcement)
    }

    fn execute_impl(
        registry: &dyn PlaybookRegistry,
        request: PersistPlaybookRequest,
        enforcement: LoaderEnforcement,
    ) -> Result<PersistPlaybookOutcome, PersistPlaybookError> {
        if request.owner_home.is_empty() {
            return Err(PersistPlaybookError::OwnerHomeRequired);
        }
        // ASKED FIRST, because it is a question about the TARGET rather than the
        // request: if the canonical root is not the root this hearth's
        // definitions are read from, every later validation is about a write
        // that produces an artifact nothing serves. C9 named this check in a doc
        // comment for a whole track before any writer made it.
        if let Some(detail) = registry.registration_blocked_detail() {
            return Err(PersistPlaybookError::HearthRegistrationBlocked { detail });
        }
        if request.kind.is_empty() {
            return Err(PersistPlaybookError::KindRequired);
        }
        if request.machine_yaml.is_empty() {
            return Err(PersistPlaybookError::MachineYamlRequired);
        }
        if request.actor_name.is_empty() {
            return Err(PersistPlaybookError::ActorNameRequired);
        }
        for (field, value) in [
            ("actor_type", &request.actor_type),
            ("actor_model", &request.actor_model),
            ("actor_provider", &request.actor_provider),
        ] {
            if value.is_empty() {
                return Err(PersistPlaybookError::ActorParamsRequired {
                    field: field.to_string(),
                });
            }
        }

        // (b) Loader-validate FIRST — the validate-before-write gate. Returns
        // BEFORE any event, so an invalid machine never reaches the port.
        //
        // The machine is validated against the SUPPLIED hook filenames: a
        // machine declaring `hook: intent.md` with a carried intent.md hook is
        // valid; a machine referencing a hook NOT carried here fails closed with
        // `playbook_unknown_hook_reference`. Empty hooks reproduce the prior
        // machine.yaml-only behavior exactly.
        let hook_filenames: Vec<String> =
            request.hooks.iter().map(|(name, _)| name.clone()).collect();
        // The selected authoring gates run at the WRITE boundary via the loader.
        // `execute` passes `LoaderEnforcement::OFF` (plain load — the generator's
        // terminal persist); `execute_enforcing` passes `MEASUREMENT` (a
        // missing/blank outcome_predicate or a measured state without
        // success_criteria fails closed with
        // `playbook_measurement_definition_missing`); `execute_enforcing_with`
        // additionally runs the evidence-obligation leg when selected. `OFF` and
        // `MEASUREMENT` reduce byte-for-byte to the prior `load_from_yaml` /
        // `load_from_yaml_enforcing` calls.
        let machine =
            load_from_yaml_with(&request.kind, &request.machine_yaml, &hook_filenames, enforcement)
                .map_err(PersistPlaybookError::LoaderInvalid)?;
        // Graph contiguity gate: a machine that parses but can't flow
        // (unreachable state, non-terminal dead-end, terminal-unreachable trap)
        // never persists — fail closed before any event, like the loader gate.
        crate::domain::playbook::loader::validate_contiguity(&machine, &request.kind)
            .map_err(PersistPlaybookError::LoaderInvalid)?;
        if machine.kind != request.kind {
            return Err(PersistPlaybookError::KindMismatch {
                argument_kind: request.kind.clone(),
                declared_kind: machine.kind,
            });
        }

        // Hook-coverage gate (WRITE boundary only): every state the interpreter
        // would serve must resolve a begin hook for the ROLE it would serve
        // there. Resolve with the interpreter's own `state_role_hook`
        // (`hooks_by_role[role]` → role-agnostic `hook` fallback) rather than
        // re-inspecting the raw fields, so this gate and `begin` agree exactly —
        // a state that declares only a role-mismatched `hooks_by_role` entry
        // (e.g. reviewer-only where the doer is served) no longer masks a blind
        // begin. The served role mirrors `begin` (begin.rs handle_create /
        // handle_projection_only_create / handle_adoption / handle_review):
        //   - the initial state is always entered as the `doer` on create/adopt
        //     — INCLUDING a projection-only single-state (possibly terminal)
        //     machine, whose create path still serves the doer hook;
        //   - a review gate is entered as the `reviewer`;
        //   - every other working state is entered as the `doer`.
        // Scoped to the WRITE boundary — the loaded corpus and the generator's
        // hookless-by-construction terminal persist are exempt.
        //
        // A machine is OUTSIDE the per-instance begin/complete progression when
        // EITHER exemption class holds, read straight from the parsed machine:
        //   1. `transitions.is_empty()` — an EMPTY standard transition graph.
        //      This is EXACTLY how the loader and interpreter classify these:
        //      `validate_contiguity` exempts a transitionless machine, and the
        //      begin/complete-driven `next_step` never runs for it (see loader.rs
        //      + event_driven.rs). These are the event/step/queue-driven
        //      playbooks (e.g. `import_transaction_history`).
        //   2. `projection_only` — a projection-only machine. `begin` routes it
        //      to `handle_projection_only_create` (begin.rs), which serves ONLY
        //      the initial state's `doer` hook and emits a
        //      `ProjectionOnlySnapshot`; it creates NO per-instance progression,
        //      so the machine's NON-initial states are never entered via begin,
        //      EVEN when it declares a full `transitions:` graph (the graph is
        //      authoring/reference metadata, not a per-instance lifecycle here).
        //      Without this class a projection-only machine WITH transitions —
        //      e.g. `captured(doer) -> processing(worker-only) -> completed` —
        //      would be falsely rejected for `processing`'s missing doer hook.
        // For BOTH classes an external queue/event/projection mechanism — not
        // begin/complete — governs the NON-initial states, so each may carry any
        // custom-role hook (e.g. `worker: process.md`) or none per its own
        // contract, and the persist gate must NOT impose the doer/reviewer role
        // requirement on them. Only the INITIAL state is still served through
        // begin: the create path (begin.rs handle_create /
        // handle_projection_only_create) always serves it as the `doer`, so its
        // doer hook requirement is retained. Machines with a real transition
        // graph that are NOT projection-only keep the full per-served-state check.
        //
        // The hook-coverage gate is a WRITE-boundary concern tied to the
        // measurement leg (always on at the persist RPC); it is intentionally
        // independent of the evidence-obligation leg, so `execute` (OFF) skips it
        // and both `execute_enforcing` / `execute_enforcing_with` run it exactly
        // as before.
        if enforcement.measurement_definition {
            let outside_begin_progression =
                machine.transitions.is_empty() || machine.projection_only;
            let initial_state_name = machine.states.first().map(|s| s.name.as_str());
            for state in &machine.states {
                let is_initial = Some(state.name.as_str()) == initial_state_name;
                // Terminal states are never begun EXCEPT a (projection-only)
                // initial state, whose create path still serves the doer hook.
                if state.is_terminal && !is_initial {
                    continue;
                }
                // Non-initial states of a transitionless machine are never
                // entered via begin — exempt them; their own queue/event
                // contract governs their hooks, so requiring the served-role
                // begin hook here would falsely reject a valid custom-role state.
                if outside_begin_progression && !is_initial {
                    continue;
                }
                let served_role = if is_initial {
                    "doer"
                } else if state.is_review_gate {
                    "reviewer"
                } else {
                    "doer"
                };
                if state_role_hook(&machine, &state.name, served_role).is_none() {
                    return Err(PersistPlaybookError::NonTerminalStateHookless {
                        kind: request.kind.clone(),
                        state: state.name.clone(),
                        role: served_role.to_string(),
                    });
                }
            }
        }

        // (c) Byte-exact duplicate-kind fast path at the exact target path.
        // `HearthPlaybookRegistry::machine_for` intentionally omits invalid
        // files, so checking only the registry would treat present-but-invalid
        // content as absent and overwrite it. File presence is therefore the
        // first branch: absent persists, byte-identical is idempotent, valid
        // byte-different or invalid fails closed with no event. Compare the
        // existing bytes to request.machine_yaml.as_bytes() because those are
        // the exact bytes carried by PlaybookPersisted and written by the
        // adapter. A concurrent writer can still create the file after this
        // branch; the filesystem adapter's exclusive create + compare-on-exists
        // is the authoritative gate for that write-boundary race.
        let canonical_target_path = Path::new(&request.owner_home)
            .join("playbooks")
            .join(&request.kind)
            .join("machine.yaml");
        let legacy_target_path = Path::new(&request.owner_home)
            .join("workflows")
            .join(&request.kind)
            .join("machine.yaml");
        let target_path = if canonical_target_path.try_exists().map_err(|e| {
            PersistPlaybookError::ExistingMachineInvalid {
                kind: request.kind.clone(),
                path: canonical_target_path.display().to_string(),
                message: e.to_string(),
            }
        })? {
            Some(canonical_target_path)
        } else if legacy_target_path.try_exists().map_err(|e| {
            PersistPlaybookError::ExistingMachineInvalid {
                kind: request.kind.clone(),
                path: legacy_target_path.display().to_string(),
                message: e.to_string(),
            }
        })? {
            Some(legacy_target_path)
        } else {
            None
        };
        if let Some(target_path) = target_path {
            let target_path_text = target_path.display().to_string();
            let existing_bytes = std::fs::read(&target_path).map_err(|e| {
                PersistPlaybookError::ExistingMachineInvalid {
                    kind: request.kind.clone(),
                    path: target_path_text.clone(),
                    message: e.to_string(),
                }
            })?;
            let desired_bytes = request.machine_yaml.as_bytes();
            if existing_bytes == desired_bytes {
                return Ok(PersistPlaybookOutcome {
                    result: PersistPlaybookResult {
                        kind: request.kind.clone(),
                    },
                    events: Vec::new(),
                });
            }
            let existing_yaml = std::str::from_utf8(&existing_bytes).map_err(|e| {
                PersistPlaybookError::ExistingMachineInvalid {
                    kind: request.kind.clone(),
                    path: target_path_text.clone(),
                    message: e.to_string(),
                }
            })?;
            // Validate the existing target against the hook filenames actually
            // present in its sibling `hooks/` dir — NOT an empty slice. A
            // previously-persisted hook-bearing machine references its own hook
            // files, so loading it with no hook filenames would mis-report it as
            // `ExistingMachineInvalid` (unknown-hook) instead of a genuine
            // same-kind duplicate. The write adapter persists those hook files
            // beside `machine.yaml`, so the listing reproduces the load context.
            let existing_hooks_dir = target_path
                .parent()
                .map(|parent| parent.join("hooks"))
                .unwrap_or_else(|| Path::new("hooks").to_path_buf());
            // C-d.1 round 6, M-1. Was `list_hook_filenames`, which swallowed a
            // read failure into an empty listing — so an unreadable `hooks/`
            // beside the existing machine made this preflight report the
            // EXISTING machine invalid for referencing hooks it correctly
            // references. Refusing on the read failure names the real fault.
            let existing_hook_filenames = fs_probe::list_hook_files(&existing_hooks_dir).map_err(
                |e| PersistPlaybookError::ExistingMachineInvalid {
                    kind: request.kind.clone(),
                    path: target_path_text.clone(),
                    message: format!(
                        "the existing machine's hooks directory {} could not be listed: {e}. \
                         An unreadable hook directory is not an empty one, and reading it as \
                         empty reports a valid machine as invalid.",
                        existing_hooks_dir.display()
                    ),
                },
            )?;
            load_from_yaml(&request.kind, existing_yaml, &existing_hook_filenames).map_err(|e| {
                PersistPlaybookError::ExistingMachineInvalid {
                    kind: request.kind.clone(),
                    path: target_path_text.clone(),
                    message: e.to_string(),
                }
            })?;
            let existing_id = registry
                .playbook_id_for(&request.kind)
                .unwrap_or_else(|| request.kind.clone());
            return Err(PersistPlaybookError::DuplicateKind {
                kind: request.kind.clone(),
                existing_id,
            });
        }

        // (d) All-pass — emit the single persist event LAST.
        Ok(PersistPlaybookOutcome {
            result: PersistPlaybookResult {
                kind: request.kind.clone(),
            },
            events: vec![PersistPlaybookEvent::PlaybookPersisted {
                owner_home: request.owner_home,
                kind: request.kind,
                machine_yaml: request.machine_yaml,
                hooks: request.hooks,
                exemplars: request.exemplars,
            }],
        })
    }
}
