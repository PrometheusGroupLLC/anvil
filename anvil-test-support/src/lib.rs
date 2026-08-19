//! Shared test support for the anvil workspace.
//!
//! Hosts step-definition modules and the brine-runner harness so each crate's
//! `tests/brine_runner.rs` stays a thin entry point.
//!
//! Consumers depend on this crate as a `[dev-dependency]`; re-exports below
//! make it the single contract surface so consumer `tests/brine_runner.rs`
//! files can compile using only `use anvil_test_support::*;` without direct
//! `brine_*` imports.

pub use brine_runner_rust::context::Context;

/// This crate's own directory, resolved at ITS compile time.
///
/// Step modules that were split out into a sibling steps crate must use this to
/// reach `anvil-test-support/fixtures`: their own `env!("CARGO_MANIFEST_DIR")`
/// now names THEIR crate, so a fixture path built from it silently points at a
/// directory that does not exist. That is how the consumer-seam split broke 45
/// fixture-loading scenarios while every crate still compiled.
pub const TEST_SUPPORT_DIR: &str = env!("CARGO_MANIFEST_DIR");
use std::sync::{Arc, Mutex};

pub type RetainedTempDir = Arc<Mutex<Option<tempfile::TempDir>>>;

/// Parse an artifact type out of a Gherkin data-table cell.
///
/// ONE copy. `checkin.rs` and `hearth.rs` each carried a byte-identical
/// hand-rolled match, which is how the blanket rename could damage the same
/// parse twice and how the damage could sit behind two identical warnings.
///
/// The input is a feature file's own prose, not persisted hearth bytes, so the
/// spellings accepted here are exactly the canonical ones a feature author may
/// write today. The `workflow` read alias that `ArtifactType`'s serde derive
/// keeps is for `status.yaml` written before the migration — a table cell is
/// never that, and carrying the retired spelling here would be a second name
/// for one thing with nothing on the other end of it.
pub fn parse_artifact_type(s: &str) -> Result<anvil_core::domain::ArtifactType, String> {
    use anvil_core::domain::ArtifactType;
    match s.trim() {
        "proposal" => Ok(ArtifactType::Proposal),
        "track" => Ok(ArtifactType::Track),
        "milestone" => Ok(ArtifactType::Milestone),
        "initiative" => Ok(ArtifactType::Initiative),
        "decision" => Ok(ArtifactType::Decision),
        "learning" => Ok(ArtifactType::Learning),
        "playbook" => Ok(ArtifactType::Playbook),
        _ => Err(format!("Unknown artifact type: '{}'", s)),
    }
}

/// Restore removable permissions over a whole scratch subtree, TOP-DOWN.
///
/// C-d.1 round 5, M-4. Fixtures that construct an unreadable or untraversable
/// directory (`0300`, `0600`, `0400`, `0000`) leave scratch that `remove_dir_all`
/// — and therefore `TempDir`'s own `Drop` — CANNOT remove, and that an ordinary
/// `rm -rf` sweep and the age-guarded `find` purge cannot remove either. Against
/// this project's history of Brine scratch filling the disk, a fixture that leaks
/// an unremovable directory is a defect.
///
/// Round 4 restored the modes from the scenario's later CHECK steps, so the
/// restore ran only on the GREEN path: a red at the first `Then` skipped it and
/// leaked. This runs from `Drop`, which is the only place that runs on every
/// path — pass, fail, panic, or early return.
///
/// TOP-DOWN and chmod-BEFORE-descend, because a `0300` directory cannot be
/// enumerated until it has been made readable.
pub fn make_removable(root: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Best-effort by construction: this runs during `Drop`, where there is
        // nothing to return an error to and the only alternative to continuing
        // is leaking more. It is not a decision predicate.
        let meta = match std::fs::symlink_metadata(root) {
            Ok(m) => m,
            Err(_) => return,
        };
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return;
        }
        let _ = std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o755));
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };
        for entry in entries.flatten() {
            make_removable(&entry.path());
        }
    }
    #[cfg(not(unix))]
    let _ = root;
}

/// A scratch directory that CANNOT leak an unremovable subtree.
///
/// Wraps `TempDir` and restores removable modes from `Drop` — which runs before
/// the wrapped `TempDir`'s own `Drop` (a struct's `Drop::drop` runs before its
/// fields are dropped), so the cleanup that follows always succeeds. Fixtures
/// that chmod anything MUST use this instead of a bare `TempDir`; see
/// [`make_removable`].
pub struct ScratchDir {
    dir: tempfile::TempDir,
}

impl ScratchDir {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            dir: tempfile::TempDir::new().map_err(|e| format!("temp dir: {e}"))?,
        })
    }

    pub fn with_prefix(prefix: &str) -> Result<Self, String> {
        Ok(Self {
            dir: tempfile::Builder::new()
                .prefix(prefix)
                .tempdir()
                .map_err(|e| format!("temp dir {prefix}: {e}"))?,
        })
    }

    pub fn path(&self) -> &std::path::Path {
        self.dir.path()
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        make_removable(self.dir.path());
    }
}

pub fn retained_temp_dir(prefix: &str) -> Result<(RetainedTempDir, std::path::PathBuf), String> {
    let dir = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir()
        .map_err(|e| format!("temp dir {}: {}", prefix, e))?;
    let path = dir.path().to_path_buf();
    Ok((Arc::new(Mutex::new(Some(dir))), path))
}

pub fn carry_retained_temp_dir(ctx: &Context, out: &mut Context, key: &str) {
    if let Some(handle) = ctx.get::<RetainedTempDir>(key) {
        out.set(key, Arc::clone(handle));
    }
}

/// Minimal valid `machine.yaml` for a given kind — a throwaway fixture for
/// persist-primitive features (track 1a). Mirrors the loader's minimal contract
/// (no hook references) so it loader-validates cleanly. NEVER read the real
/// builder/Lore machine.yaml in tests; synthesize via this helper instead.
pub fn minimal_machine_yaml(kind: &str) -> String {
    format!(
        r#"kind: {kind}
directory: {kind}s
registry: {kind}s.md
description: "Minimal {kind} machine for testing."
required_fields: []
roles:
  - doer
  - reviewer
states:
  - name: active
    role_filters: []
    registry_section: active
    projection_targets: []
    is_review_gate: false
    is_terminal: false
  - name: completed
    role_filters: []
    registry_section: completed
    projection_targets: []
    is_review_gate: false
    is_terminal: true
transitions:
  - from_state: active
    to_state: completed
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
"#,
        kind = kind
    )
}

/// Same shape as `minimal_machine_yaml`, but semantically different while
/// preserving the same kind. Used to prove same-kind/different-content
/// collisions are still rejected after idempotent same-content retries.
pub fn different_minimal_machine_yaml(kind: &str) -> String {
    minimal_machine_yaml(kind).replace(
        &format!("Minimal {kind} machine for testing."),
        &format!("Different {kind} machine for collision testing."),
    )
}

/// A minimal valid `machine.yaml` whose `active` state declares a `hook`
/// reference to `hook_filename`. Used to prove the persist primitive carries,
/// validates-against, and writes hook files. When the persist request carries a
/// matching hook the machine loader-validates cleanly; when it does not, the
/// loader rejects it with `playbook_unknown_hook_reference`.
///
/// Conformant at the persist WRITE boundary: it declares an `outcome_predicate`
/// (so `load_from_yaml_enforcing` accepts it) and its lone non-terminal state
/// carries a hook (so the hook-coverage gate accepts it). Persisting it via
/// `execute_enforcing` therefore succeeds when the referenced hook is carried.
pub fn machine_yaml_with_hook(kind: &str, hook_filename: &str) -> String {
    format!(
        r#"kind: {kind}
directory: {kind}s
registry: {kind}s.md
description: "Hook-bearing {kind} machine for testing."
required_fields: []
roles:
  - doer
  - reviewer
states:
  - name: active
    role_filters: []
    registry_section: active
    projection_targets: []
    is_review_gate: false
    is_terminal: false
    hook: {hook_filename}
  - name: completed
    role_filters: []
    registry_section: completed
    projection_targets: []
    is_review_gate: false
    is_terminal: true
transitions:
  - from_state: active
    to_state: completed
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
outcome_predicate:
  terminal_state: completed
"#,
        kind = kind,
        hook_filename = hook_filename
    )
}

/// A persist-WRITE-boundary-conformant `machine.yaml`: an `outcome_predicate`
/// AND a begin hook on the lone non-terminal state (`active` -> `intent.md`).
/// This is what `execute_enforcing` accepts. Callers must carry a hook named
/// `intent.md` in the persist request so the loader resolves the reference.
pub fn conformant_machine_yaml(kind: &str) -> String {
    machine_yaml_with_hook(kind, "intent.md")
}

/// Byte-different sibling of [`conformant_machine_yaml`] for the same kind —
/// still WRITE-boundary conformant (predicate + hook) but with a different
/// description, so a second persist of the same kind is a genuine
/// same-kind/different-content duplicate rather than an idempotent no-op.
pub fn different_conformant_machine_yaml(kind: &str) -> String {
    conformant_machine_yaml(kind).replace(
        &format!("Hook-bearing {kind} machine for testing."),
        &format!("Different hook-bearing {kind} machine for collision testing."),
    )
}

/// A `machine.yaml` that declares an `outcome_predicate` (so it clears the
/// measurement gate) but leaves its non-terminal `active` state hookless — the
/// exact shape the persist WRITE boundary refuses with
/// `playbook_non_terminal_state_hookless`.
pub fn machine_yaml_predicate_no_hook(kind: &str) -> String {
    format!(
        r#"kind: {kind}
directory: {kind}s
registry: {kind}s.md
description: "Predicate-bearing hookless {kind} machine for testing."
required_fields: []
roles:
  - doer
  - reviewer
states:
  - name: active
    role_filters: []
    registry_section: active
    projection_targets: []
    is_review_gate: false
    is_terminal: false
  - name: completed
    role_filters: []
    registry_section: completed
    projection_targets: []
    is_review_gate: false
    is_terminal: true
transitions:
  - from_state: active
    to_state: completed
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
outcome_predicate:
  terminal_state: completed
"#,
        kind = kind
    )
}

/// A `machine.yaml` that clears the measurement gate (declares an
/// `outcome_predicate`) and whose lone non-terminal `active` state declares a
/// hook ONLY under `hooks_by_role.reviewer` — no role-agnostic `hook` and no
/// `doer` entry — while NOT being a review gate. The interpreter serves the
/// `doer` hook at the initial state on create, so this machine would serve a
/// BLIND doer begin: the persist WRITE boundary refuses it with
/// `playbook_non_terminal_state_hookless` naming the `doer` role. The reviewer
/// hook references `intent.md` (the same file the conformant fixtures carry) so
/// the loader resolves the reference and the hook-coverage gate — not the
/// loader — is what rejects it.
pub fn machine_yaml_reviewer_only_hook(kind: &str) -> String {
    format!(
        r#"kind: {kind}
directory: {kind}s
registry: {kind}s.md
description: "Reviewer-only-hook {kind} machine for testing."
required_fields: []
roles:
  - doer
  - reviewer
states:
  - name: active
    role_filters: []
    registry_section: active
    projection_targets: []
    is_review_gate: false
    is_terminal: false
    hooks_by_role:
      reviewer: intent.md
  - name: completed
    role_filters: []
    registry_section: completed
    projection_targets: []
    is_review_gate: false
    is_terminal: true
transitions:
  - from_state: active
    to_state: completed
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
outcome_predicate:
  terminal_state: completed
"#,
        kind = kind
    )
}

/// A transitionless event-driven `machine.yaml` — the `anvil_kind` + `trigger`
/// + per-state `on:` shape the loader routes through the event-driven mirror.
/// It declares an EMPTY standard `transitions:` graph, which is the exact marker
/// the loader and interpreter use to classify a machine as OUTSIDE the
/// begin/complete progression (`validate_contiguity` exempts a machine with
/// `transitions.is_empty()`; the begin/complete-driven `next_step` never runs).
/// Its initial `active` state carries a role-agnostic begin hook (which resolves
/// for the `doer` served at create), but its NON-initial `processing` state
/// carries ONLY a custom-role hook (`hooks_by_role.worker`) — no doer, no
/// reviewer, no role-agnostic hook. An event/queue mechanism (not begin/complete)
/// drives that state, so it may carry any custom-role hook or none per its own
/// contract; the persist WRITE boundary must ACCEPT this machine. Both hook
/// references point at `intent.md` (the file the persist request carries) so the
/// loader resolves them and the hook-COVERAGE gate — not the loader — is what
/// the scenario exercises. Declares an `outcome_predicate` so it clears the
/// enforcing measurement gate too.
pub fn machine_yaml_event_driven_custom_role_hook(kind: &str) -> String {
    format!(
        r#"name: Event-driven {kind} worker
version: "1"
anvil_kind: {kind}
description: "Event-driven transitionless {kind} machine for testing."
trigger:
  kind: pending_queue
  poll_tool: list_pending
states:
  - name: active
    is_terminal: false
    on:
      item_ready: processing
    hook: intent.md
  - name: processing
    is_terminal: false
    on:
      done: completed
    hooks_by_role:
      worker: intent.md
  - name: completed
    is_terminal: true
outcome_predicate:
  terminal_state: completed
"#,
        kind = kind
    )
}

/// A projection-only `machine.yaml` that DOES declare a standard `transitions:`
/// graph (`captured -> processing -> completed`) — the exact shape the
/// role-coverage gate previously mis-handled. Because `projection_only: true`,
/// `begin` serves ONLY the initial `captured` state's `doer` hook and emits a
/// `ProjectionOnlySnapshot` (see `begin.rs` `handle_projection_only_create`); it
/// creates NO per-instance progression, so the non-initial `processing` state is
/// never entered via begin. `processing` therefore legitimately carries ONLY a
/// custom-role hook (`hooks_by_role.worker`) — no doer, no reviewer, no
/// role-agnostic hook. The initial `captured` state DOES carry a role-agnostic
/// hook (which resolves for the `doer` served at create). Both hook references
/// point at `intent.md` (the file the persist request carries) so the loader
/// resolves them and the hook-COVERAGE gate — not the loader — is what the
/// scenario exercises. Declares an `outcome_predicate` so it clears the enforcing
/// measurement gate too. A transitionless-only exemption would falsely reject
/// this machine for `processing`'s missing doer hook; the fix additionally
/// exempts non-initial states when `projection_only`.
pub fn machine_yaml_projection_only_worker_state(kind: &str) -> String {
    format!(
        r#"kind: {kind}
directory: {kind}s
registry: {kind}s.md
description: "Projection-only {kind} machine with transitions for testing."
projection_only: true
required_fields: []
roles:
  - doer
  - reviewer
  - worker
states:
  - name: captured
    role_filters: []
    registry_section: captured
    projection_targets: []
    is_review_gate: false
    is_terminal: false
    hook: intent.md
  - name: processing
    role_filters: []
    registry_section: processing
    projection_targets: []
    is_review_gate: false
    is_terminal: false
    hooks_by_role:
      worker: intent.md
  - name: completed
    role_filters: []
    registry_section: completed
    projection_targets: []
    is_review_gate: false
    is_terminal: true
transitions:
  - from_state: captured
    to_state: processing
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
  - from_state: processing
    to_state: completed
    required_role: worker
    required_satisfaction: ~
    requires_approver: false
outcome_predicate:
  terminal_state: completed
"#,
        kind = kind
    )
}

/// A persist-WRITE-boundary-conformant DRIVEN `machine.yaml` carrying a MEASURED
/// initial step — `outcome_predicate` + per-step `success_criteria` (so the
/// always-on measurement gate accepts it) + a begin hook on the measured
/// non-terminal state (so the hook-coverage gate accepts it) — but with NO
/// `evidence_obligation` on that measured pair. Under
/// `LoaderEnforcement { measurement_definition: true, evidence_obligation: false }`
/// it persists; under `evidence_obligation: true` it is refused with
/// `playbook_evidence_obligation_missing` (T-EEC-1 C10/C11 persist legs). The
/// caller must carry an `intent.md` hook in the persist request.
pub fn machine_yaml_driven_measured_no_obligation(kind: &str) -> String {
    format!(
        r#"kind: {kind}
directory: {kind}s
registry: {kind}s.md
description: "Driven measured {kind} machine without an obligation for testing."
required_fields: []
roles:
  - doer
  - reviewer
states:
  - name: active
    role_filters: []
    registry_section: active
    projection_targets: []
    is_review_gate: false
    is_terminal: false
    hook: intent.md
    measurement_by_role:
      doer:
        intent: Do the measured work.
        expected_output: A work artifact.
        success_criteria: The work artifact is complete and correct.
  - name: completed
    role_filters: []
    registry_section: completed
    projection_targets: []
    is_review_gate: false
    is_terminal: true
transitions:
  - from_state: active
    to_state: completed
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
outcome_predicate:
  terminal_state: completed
"#,
        kind = kind
    )
}

/// Sibling of [`machine_yaml_driven_measured_no_obligation`] whose measured pair
/// DOES declare an `evidence_obligation`. This is the DRIVEN pass-case: under
/// `evidence_obligation: true` it clears the obligation gate and persists
/// (T-EEC-1 C13 persist leg).
pub fn machine_yaml_driven_measured_with_obligation(kind: &str) -> String {
    machine_yaml_driven_measured_no_obligation(kind).replace(
        "        success_criteria: The work artifact is complete and correct.\n",
        "        success_criteria: The work artifact is complete and correct.\n        evidence_obligation: [artifact_of_consequence]\n",
    )
}

/// A persist-WRITE-boundary-conformant FREE `machine.yaml` (`register: free`)
/// carrying a MEASURED initial step that is measurement-conformant
/// (`success_criteria` + `outcome_predicate` + begin hook) AND declares an
/// `evidence_obligation`. Measurement passes, so under
/// `evidence_obligation: true` the refusal is the FREE-register obligation error
/// (`playbook_evidence_obligation_on_free_register`), not a measurement error
/// (T-EEC-1 C17 persist leg). The caller must carry an `intent.md` hook.
pub fn machine_yaml_free_measured_with_obligation(kind: &str) -> String {
    format!(
        r#"kind: {kind}
directory: {kind}s
registry: {kind}s.md
description: "Free measured {kind} machine declaring an obligation for testing."
required_fields: []
roles:
  - doer
  - reviewer
register: free
states:
  - name: active
    role_filters: []
    registry_section: active
    projection_targets: []
    is_review_gate: false
    is_terminal: false
    hook: intent.md
    measurement_by_role:
      doer:
        intent: Do the measured work.
        expected_output: A work artifact.
        success_criteria: The work artifact is complete and correct.
        evidence_obligation: [self_description]
  - name: completed
    role_filters: []
    registry_section: completed
    projection_targets: []
    is_review_gate: false
    is_terminal: true
transitions:
  - from_state: active
    to_state: completed
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
outcome_predicate:
  terminal_state: completed
"#,
        kind = kind
    )
}

/// Sibling of [`machine_yaml_free_measured_with_obligation`] whose measured pair
/// declares NO `evidence_obligation`. A FREE machine that never declares an
/// obligation is never refused, regardless of the obligation gate (T-EEC-1 C18
/// persist leg).
pub fn machine_yaml_free_measured_no_obligation(kind: &str) -> String {
    machine_yaml_free_measured_with_obligation(kind)
        .replace("        evidence_obligation: [self_description]\n", "")
}

/// A deliberately loader-invalid `machine.yaml`: a transition references an
/// undeclared role. Used to prove loader-validate-before-write (no partial write).
pub fn loader_invalid_machine_yaml(kind: &str) -> String {
    format!(
        r#"kind: {kind}
directory: {kind}s
registry: {kind}s.md
description: "Loader-invalid {kind} machine for testing."
required_fields: []
roles:
  - doer
states:
  - name: active
    role_filters: []
    registry_section: active
    projection_targets: []
    is_review_gate: false
    is_terminal: false
  - name: completed
    role_filters: []
    registry_section: completed
    projection_targets: []
    is_review_gate: false
    is_terminal: true
transitions:
  - from_state: active
    to_state: completed
    required_role: ghost_role
    required_satisfaction: ~
    requires_approver: false
"#,
        kind = kind
    )
}

/// Wrap a message as a gRPC request that names the Brine harness as its surface.
///
/// The engine refuses a state-changing call that does not name the program
/// making it. The harness is a REAL caller of this engine, so it names itself —
/// `test-harness` is a member of the engine's closed surface set rather than a
/// lie. Having the harness send `cli` or `mcp` would corrupt the very
/// measurement the seam exists to produce: every scenario in the suite would
/// then look like traffic from a shipped surface.
///
/// Every `tonic::Request::new` in the harness goes through here, so a scenario
/// cannot be written that silently bypasses the seam.
pub fn surfaced<T>(message: T) -> tonic::Request<T> {
    // The ONE place in the harness that may call `tonic::Request::new` directly:
    // everything else goes through this function, and this function is what
    // puts the surface on. Routing it through itself is an infinite recursion
    // that presents as a stack overflow inside an unrelated engine-start step.
    let mut request = tonic::Request::new(message);
    request.metadata_mut().insert(
        anvil_engine::command_seam::SURFACE_METADATA_KEY,
        tonic::metadata::MetadataValue::from_static("test-harness"),
    );
    request
}

pub mod backlog_item;
pub mod builder;
pub mod checkin;
pub mod engine;
pub mod harness;
pub mod hearth;
pub mod query_port;
pub mod snapshot;
pub mod spark_lifecycle;

pub use brine_runner_rust::registry::{async_step_def, check_def, step_def, StepDef};
