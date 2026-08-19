//! Playbook atlas fold — the honest per-kind measurement surface backing the
//! in-app Atlas.
//!
//! `fold_playbook_atlas` walks a CONCRETE `HearthPlaybookRegistry` (not the
//! composite `&dyn PlaybookRegistry`) because the two things the Atlas needs most
//! — the invalid-artifact diagnostics (`invalid_artifacts()`) and the per-dir
//! `status.yaml` (owner_kit / lifecycle state) — are only reachable through the
//! concrete hearth registry. The documented trade-off (a foreign hearth lacking a
//! seed-only kind's dir would miss it) is acceptable for a LOCAL-hearth
//! measurement surface: the live hearth carries every kind as a directory on
//! disk, so the atlas sees the full fleet.
//!
//! Honesty constraints (from the 2026-07-06 fleet measurement review):
//!   - Integrity is the SHARED `playbook_integrity` fold — the SAME rules
//!     `hearth_lint` uses, so the lint gate and the app can never drift.
//!   - INVALID machines are INCLUDED as degenerate entries (`loads == false`)
//!     carrying their load error, so breakage is SHOWN, not hidden.
//!   - The calibration badge is only `Seed` (a rubric with >=1 dimension) or
//!     `None` (no rubric / empty dimensions). `calibrated`/`refused` have no
//!     source yet and are NEVER emitted.
//!   - `grader_declared` (via `playbook_integrity`) is provenance ONLY — never
//!     "registered" or "calibrated".

use std::path::Path;

use crate::domain::playbook::hearth_registry::HearthPlaybookRegistry;
use crate::domain::playbook::integrity::{playbook_integrity, Integrity};
use crate::domain::playbook::load_error::PlaybookLoadError;
use crate::domain::playbook::registry::PlaybookRegistry;
use crate::domain::playbook::status_read::read_playbook_status;
use crate::domain::playbook::types::{EvidenceClass, PlaybookMachine, Register, SuccessRubric};

/// The calibration badge for a playbook. Only these two variants are emitted
/// today; `calibrated`/`refused` are RESERVED wire values with no source yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtlasCalibration {
    /// A `success_rubric` with >=1 dimension: declared but uncalibrated.
    Seed,
    /// No `success_rubric`, or a rubric with no dimensions.
    None,
}

impl AtlasCalibration {
    /// The serialized badge token the wire carries.
    pub fn as_str(&self) -> &'static str {
        match self {
            AtlasCalibration::Seed => "seed",
            AtlasCalibration::None => "none",
        }
    }
}

/// One Atlas sidebar row: either a valid (loaded) machine or an invalid one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtlasEntry {
    Valid(AtlasValidEntry),
    Invalid(AtlasInvalidEntry),
}

impl AtlasEntry {
    /// The display / sort key: `kind` for a valid entry, `artifact_id` for an
    /// invalid one.
    pub fn key(&self) -> &str {
        match self {
            AtlasEntry::Valid(e) => &e.kind,
            AtlasEntry::Invalid(e) => &e.artifact_id,
        }
    }
}

/// A registry-resolved (loaded) playbook's atlas row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtlasValidEntry {
    pub kind: String,
    /// The on-disk playbook directory id.
    pub artifact_id: String,
    /// Owning kit from the dir's `status.yaml`; `""` when absent/omitted.
    pub owner_kit: String,
    /// Lifecycle state from the dir's `status.yaml`; `None` when absent/omitted.
    pub state: Option<String>,
    /// The machine's register (`Driven`/`Free`).
    pub register: Register,
    /// The shared honest-integrity fold.
    pub integrity: Integrity,
    /// The calibration badge (`Seed`/`None`).
    pub calibration: AtlasCalibration,
    /// Declared state count.
    pub state_count: usize,
    /// Declared transition (edge) count.
    pub edge_count: usize,
}

/// An invalid (unloadable) playbook's degenerate atlas row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtlasInvalidEntry {
    /// The on-disk artifact id from the load error.
    pub artifact_id: String,
    /// Owning kit from the dir's `status.yaml`; `""` when absent/omitted.
    pub owner_kit: String,
    /// Lifecycle state from the dir's `status.yaml`; `None` when absent/omitted.
    pub state: Option<String>,
    /// The stable snake_case load-error code.
    pub error_code: String,
    /// The human-readable load-error message.
    pub error_message: String,
}

/// The whole Atlas list payload.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlaybookAtlas {
    /// One entry per registry-resolved kind PLUS one per invalid machine, sorted
    /// by the display key.
    pub entries: Vec<AtlasEntry>,
}

/// Fold a hearth's concrete registry into the Atlas list payload.
///
/// Iterates `all_machines()` (valid) + `invalid_artifacts()` (degenerate),
/// reading each machine's dir `status.yaml` for `owner_kit`/`state`, and folds
/// each valid machine through the shared `playbook_integrity`.
pub fn fold_playbook_atlas(
    hearth_path: &Path,
    registry: &HearthPlaybookRegistry,
) -> PlaybookAtlas {
    let playbooks_root = hearth_path.join("playbooks");

    let mut entries: Vec<AtlasEntry> = Vec::new();

    for machine in registry.all_machines() {
        let kind = machine.kind.clone();
        let artifact_id = registry.playbook_id_for(&kind).unwrap_or_else(|| kind.clone());
        let status = read_playbook_status(&playbooks_root.join(&artifact_id));
        let owner_kit = status
            .as_ref()
            .map(|s| s.owner_kit.clone())
            .unwrap_or_default();
        let state = status.and_then(|s| s.state);

        entries.push(AtlasEntry::Valid(AtlasValidEntry {
            kind,
            artifact_id,
            owner_kit,
            state,
            register: machine.register,
            integrity: playbook_integrity(machine),
            calibration: calibration_for(machine),
            state_count: machine.states.len(),
            edge_count: machine.transitions.len(),
        }));
    }

    for error in registry.invalid_artifacts() {
        let artifact_id = error_artifact_id(error);
        // A duplicate-kind error carries a comma-joined id pair; read status from
        // the first id's dir so a broken machine still slots into a kit column.
        let status_id = artifact_id.split(',').next().unwrap_or(&artifact_id);
        let status = read_playbook_status(&playbooks_root.join(status_id));
        let owner_kit = status
            .as_ref()
            .map(|s| s.owner_kit.clone())
            .unwrap_or_default();
        let state = status.and_then(|s| s.state);

        entries.push(AtlasEntry::Invalid(AtlasInvalidEntry {
            artifact_id,
            owner_kit,
            state,
            error_code: error.code().to_string(),
            error_message: error.to_string(),
        }));
    }

    // C-d.1 round 4. `excluded_directories()` had NO consumer anywhere: the
    // loader recorded every directory under the canonical root it resolved
    // nothing from, with the comment "Recorded, not swallowed... The projection
    // states it" — and the projection has no production consumer either, so the
    // record was made and then surfaced nowhere. A directory sitting in
    // `playbooks/` that serves no kind is exactly what an atlas exists to show:
    // without it the operator sees a directory on disk and an atlas that does
    // not mention it, which is the "concluded it was never there" failure this
    // whole surface was built to end.
    //
    // Directories that ALSO produced a load error are already above; only the
    // ones with no error of their own are added, so nothing is listed twice.
    //
    // C-d.1 round 5, L-2: this de-dupe is over DIRECTORY IDS, and it used to be
    // built from `e.key()` — which is the `kind` for a Valid entry and the
    // `artifact_id` for an Invalid one. Those are different namespaces, so a
    // directory under `playbooks/` whose basename happened to equal some other
    // machine's governed kind was silently suppressed from the atlas: precisely
    // the "the operator concludes it was never there" failure this wiring exists
    // to end, reintroduced by the code that ends it.
    let already: std::collections::BTreeSet<String> = entries
        .iter()
        .flat_map(|e| match e {
            AtlasEntry::Valid(v) => vec![v.artifact_id.clone()],
            // A duplicate-kind error carries a comma-joined id PAIR.
            AtlasEntry::Invalid(i) => i
                .artifact_id
                .split(',')
                .map(|s| s.to_string())
                .collect::<Vec<String>>(),
        })
        .collect();
    for excluded in registry.excluded_directories() {
        if already.contains(&excluded.id) {
            continue;
        }
        let status = read_playbook_status(&playbooks_root.join(&excluded.id));
        let owner_kit = status
            .as_ref()
            .map(|s| s.owner_kit.clone())
            .unwrap_or_default();
        let state = status.and_then(|s| s.state);
        entries.push(AtlasEntry::Invalid(AtlasInvalidEntry {
            artifact_id: excluded.id.clone(),
            owner_kit,
            state,
            error_code: "playbook_directory_not_loaded".to_string(),
            error_message: excluded.reason.clone(),
        }));
    }

    entries.sort_by(|a, b| a.key().cmp(b.key()));

    PlaybookAtlas { entries }
}

/// The full per-kind detail: states, edges, and success rubric.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AtlasDetail {
    pub kind: String,
    pub states: Vec<AtlasState>,
    pub edges: Vec<AtlasEdge>,
    pub success_rubric: Option<AtlasRubric>,
}

/// One state in the per-kind flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtlasState {
    pub name: String,
    pub is_review_gate: bool,
    pub is_terminal: bool,
    /// True when the state declares any `measurement_by_role` spec.
    pub has_measurement: bool,
    /// True when the state declares a `hook` or any `hooks_by_role` entry.
    pub has_hook: bool,
}

/// One transition in the per-kind flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtlasEdge {
    pub from: String,
    pub to: String,
    pub required_role: String,
    /// Empty when the edge declares no `required_satisfaction` (null).
    pub required_satisfaction: Vec<String>,
}

/// The per-kind success-rubric summary.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AtlasRubric {
    pub dimensions: Vec<AtlasRubricDimension>,
    pub grader_declared: bool,
    pub anchors_count: usize,
    pub lagging_signals: Vec<String>,
}

/// One weighted rubric dimension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtlasRubricDimension {
    pub dimension: String,
    pub weight: u32,
    /// The serialized `EvidenceClass`.
    pub evidence_class: String,
}

/// Fold one registry-resolved kind into its full detail. Returns `None` when the
/// kind is not a valid (loaded) machine — the atlas list still carries invalid
/// kinds, but detail is only meaningful for a machine that loaded.
pub fn fold_playbook_atlas_detail(
    registry: &HearthPlaybookRegistry,
    kind: &str,
) -> Option<AtlasDetail> {
    let machine = registry.machine_for(kind)?;

    let states = machine
        .states
        .iter()
        .map(|s| AtlasState {
            name: s.name.clone(),
            is_review_gate: s.is_review_gate,
            is_terminal: s.is_terminal,
            has_measurement: !s.measurement_by_role.is_empty(),
            has_hook: s.hook.is_some() || !s.hooks_by_role.is_empty(),
        })
        .collect();

    let edges = machine
        .transitions
        .iter()
        .map(|t| AtlasEdge {
            from: t.from_state.clone(),
            to: t.to_state.clone(),
            required_role: t.required_role.clone(),
            required_satisfaction: t.required_satisfaction.clone().unwrap_or_default(),
        })
        .collect();

    Some(AtlasDetail {
        kind: machine.kind.clone(),
        states,
        edges,
        success_rubric: machine.success_rubric.as_ref().map(atlas_rubric),
    })
}

/// The calibration badge: `Seed` when a rubric declares >=1 dimension, else
/// `None`.
fn calibration_for(machine: &PlaybookMachine) -> AtlasCalibration {
    match machine.success_rubric.as_ref() {
        Some(rubric) if !rubric.dimensions.is_empty() => AtlasCalibration::Seed,
        _ => AtlasCalibration::None,
    }
}

/// Summarize a `SuccessRubric` into the atlas rubric view.
fn atlas_rubric(rubric: &SuccessRubric) -> AtlasRubric {
    AtlasRubric {
        dimensions: rubric
            .dimensions
            .iter()
            .map(|d| AtlasRubricDimension {
                dimension: d.dimension.clone(),
                weight: d.weight,
                evidence_class: evidence_class_str(d.evidence_class).to_string(),
            })
            .collect(),
        grader_declared: rubric.grader.is_some(),
        anchors_count: rubric.anchors.len(),
        lagging_signals: rubric.lagging_signals.clone(),
    }
}

/// The serialized snake_case token for an `EvidenceClass`.
fn evidence_class_str(class: EvidenceClass) -> &'static str {
    match class {
        EvidenceClass::ArtifactOfConsequence => "artifact_of_consequence",
        EvidenceClass::VerifiableCitation => "verifiable_citation",
        EvidenceClass::SelfDescription => "self_description",
    }
}

/// Extract the on-disk artifact id from a load error. Every variant carries an
/// `artifact_id` EXCEPT `DuplicateKindRegistration`, which carries `artifact_ids`
/// (plural, comma-joined) — return the whole joined string so the duplicate still
/// slots into a kit column instead of an empty id.
fn error_artifact_id(error: &PlaybookLoadError) -> String {
    match error {
        // C9: a hearth-directory failure belongs to no single artifact. Returning
        // an empty id would silently slot it into an "(unassigned)" column as if
        // it were an artifact whose owner could not be read; naming it keeps the
        // atlas honest about what kind of failure it is.
        PlaybookLoadError::HearthDirectoryMoveFailed { .. } => {
            "(hearth directory move)".to_string()
        }
        PlaybookLoadError::HearthDirectoryCollision { .. } => {
            "(hearth directory collision)".to_string()
        }
        PlaybookLoadError::HearthRootUnreadable { .. } => {
            "(hearth root unreadable)".to_string()
        }
        PlaybookLoadError::YamlParseError { artifact_id, .. }
        | PlaybookLoadError::MissingRequiredKey { artifact_id, .. }
        | PlaybookLoadError::UnknownRoleReference { artifact_id, .. }
        | PlaybookLoadError::UnknownStateReference { artifact_id, .. }
        | PlaybookLoadError::ReviewGateMissingSatisfaction { artifact_id, .. }
        | PlaybookLoadError::HookPathInvalid { artifact_id, .. }
        | PlaybookLoadError::UnknownHookReference { artifact_id, .. }
        | PlaybookLoadError::UnknownRoleKeyReference { artifact_id, .. }
        | PlaybookLoadError::UnreachableState { artifact_id, .. }
        | PlaybookLoadError::DeadEndState { artifact_id, .. }
        | PlaybookLoadError::NoTerminalReachable { artifact_id, .. }
        | PlaybookLoadError::UnknownQualityDimension { artifact_id, .. }
        | PlaybookLoadError::InvalidRubricWeight { artifact_id, .. }
        | PlaybookLoadError::OutcomePredicateUnknownState { artifact_id, .. }
        | PlaybookLoadError::MeasurementDefinitionMissing { artifact_id, .. }
        | PlaybookLoadError::EvidenceObligationMissing { artifact_id, .. }
        | PlaybookLoadError::EvidenceObligationOnFreeRegister { artifact_id, .. } => {
            artifact_id.clone()
        }
        PlaybookLoadError::DuplicateKindRegistration { artifact_ids, .. } => artifact_ids.clone(),
    }
}
