//! Named error codes for playbook machine.yaml validation.
//!
//! Each variant mirrors one error code from the error-code table in plan.md.
//! `Display` produces the snake_case code as a substring, consistent with the
//! `spec_not_ready_for_review` pattern used elsewhere in the engine.

use std::fmt;

/// An error returned when loading or validating a `machine.yaml` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaybookLoadError {
    /// The top-level `workflows/` -> `playbooks/` hearth directory move failed.
    ///
    /// C9. This is NOT a machine.yaml validation error — it is a hearth-level
    /// failure carried in the same channel so a caller sees it where it already
    /// looks. It used to be an `eprintln!` and nothing else, which meant a failed
    /// move was invisible to every consumer of this type.
    HearthDirectoryMoveFailed {
        detail: String,
    },
    /// BOTH `playbooks/` and `playbooks/` exist at the top of the hearth.
    ///
    /// C-d.1. Also not a machine.yaml validation error. The scan reads the
    /// canonical root only, so definitions under the legacy root were dropped
    /// with no rename, no error and no log line — the silent-drop this variant
    /// exists to make loud. Carried in the same channel as the move failure so
    /// a caller sees it where it already looks.
    HearthDirectoryCollision {
        detail: String,
    },
    /// A hearth ROOT exists and could not be ENUMERATED (or is not a directory).
    ///
    /// C-d.1 round 4. Also not a machine.yaml validation error. Every other
    /// `read_dir` failure used to return an empty listing, so an unreadable root
    /// produced an EMPTY REGISTRY with no error at all — and an empty registry
    /// is exactly what a hearth with no definitions produces, so the two states
    /// were indistinguishable and a writer was cleared to write into the second
    /// when it was in fact the first. It BLOCKS registration: the loaded set is
    /// not the set on disk.
    HearthRootUnreadable {
        detail: String,
    },
    /// The file is not valid YAML. Carries the parser's line/column pointer.
    ///
    /// Code: `playbook_yaml_parse_error`
    YamlParseError {
        artifact_id: String,
        line: u64,
        column: u64,
        message: String,
    },

    /// A required top-level or nested key per R2.4 is absent.
    ///
    /// Code: `playbook_missing_required_key`
    MissingRequiredKey {
        artifact_id: String,
        /// Key path, e.g. `"kind"` or `"states[2].registry_section"`.
        key_path: String,
    },

    /// A transition's `required_role` names a role not in the top-level `roles` list.
    ///
    /// Code: `playbook_unknown_role_reference`
    UnknownRoleReference {
        artifact_id: String,
        from_state: String,
        to_state: String,
        role: String,
    },

    /// A transition's `from_state` or `to_state` names a state not in `states`.
    ///
    /// Code: `playbook_unknown_state_reference`
    UnknownStateReference {
        artifact_id: String,
        from_state: String,
        to_state: String,
        unknown_state: String,
    },

    /// A state with `is_review_gate: true` has an outgoing transition with
    /// null or missing `required_satisfaction`.
    ///
    /// Code: `playbook_review_gate_missing_satisfaction`
    ReviewGateMissingSatisfaction {
        artifact_id: String,
        /// The review-gate state name.
        state: String,
        /// The target state of the offending transition.
        to_state: String,
    },

    /// A state or transition's `hook` field names an unsafe path rather than a
    /// direct Markdown filename under `hooks/`.
    ///
    /// Code: `hook_path_invalid`
    HookPathInvalid {
        artifact_id: String,
        /// `"state:X"` or `"transition:X→Y"`
        context: String,
        filename: String,
    },

    /// A state or transition's `hook` field names a file not present under `hooks/`.
    ///
    /// Code: `playbook_unknown_hook_reference`
    UnknownHookReference {
        artifact_id: String,
        /// `"state:X"` or `"transition:X→Y"`
        context: String,
        filename: String,
    },

    /// A `hooks_by_role` map key names a role not in the top-level `roles` list.
    ///
    /// Code: `playbook_unknown_role_key`
    UnknownRoleKeyReference {
        artifact_id: String,
        /// `"state:X:role:Y"` — the context string identifying the offending key.
        context: String,
        /// The role name that is not in the `roles` list.
        role: String,
    },

    /// Two playbook artifacts declare the same top-level `kind` value.
    ///
    /// Code: `playbook_duplicate_kind_registration`
    DuplicateKindRegistration {
        /// Both artifact ids, separated by `,`.
        artifact_ids: String,
        kind: String,
    },

    /// A declared state is not reachable from the machine's initial state
    /// (the first declared state). A playbook can never enter such a state.
    ///
    /// Code: `playbook_unreachable_state`
    UnreachableState {
        artifact_id: String,
        /// The initial (first declared) state the reachability scan started from.
        initial_state: String,
        /// The state that cannot be reached.
        unreachable_state: String,
    },

    /// A non-terminal state has zero outgoing transitions — the playbook
    /// reaches it but can never continue (a dead-end).
    ///
    /// Code: `playbook_dead_end_state`
    DeadEndState {
        artifact_id: String,
        /// The non-terminal state with no outgoing transitions.
        state: String,
    },

    /// A `success_rubric` dimension names a value not in the shared quality
    /// vocabulary (`types::QUALITY_DIMENSIONS`). Rubrics must draw from the
    /// shared vocab so cross-playbook scores stay comparable.
    ///
    /// Code: `playbook_unknown_quality_dimension`
    UnknownQualityDimension {
        artifact_id: String,
        /// The dimension identifier that is not in the shared vocabulary.
        dimension: String,
    },

    /// A `success_rubric` dimension carries a non-well-formed (zero) weight.
    ///
    /// Code: `playbook_invalid_rubric_weight`
    InvalidRubricWeight {
        artifact_id: String,
        /// The dimension whose weight is invalid.
        dimension: String,
    },

    /// From some state, no terminal state is reachable via transitions — the
    /// playbook can enter a trap from which it can never complete.
    ///
    /// Code: `playbook_no_terminal_reachable`
    NoTerminalReachable {
        artifact_id: String,
        /// A state from which no terminal state is reachable.
        state: String,
    },

    /// An `outcome_predicate.terminal_state` names a state not declared in the
    /// machine's `states` list. The predicate's checkable fact ("this state was
    /// reached") is only meaningful for a state the machine actually has.
    ///
    /// Code: `playbook_outcome_predicate_unknown_state`
    OutcomePredicateUnknownState {
        artifact_id: String,
        /// The terminal_state value that is not a declared state.
        terminal_state: String,
    },

    /// The machine fails the measurement-definition enforcement check: either
    /// a state declaring a `measurement_by_role` entry has no (or blank)
    /// `success_criteria` on that entry, or the machine has no
    /// `outcome_predicate` with a non-blank `terminal_state`. States with NO
    /// `measurement_by_role` entries at all are exempt — automated/routing
    /// machines legitimately have no per-step measurement.
    ///
    /// Emitted ONLY by the enforcing entry points
    /// (`loader::validate_with_id_enforcing` / `loader::load_from_yaml_enforcing`)
    /// — the default (non-enforcing) load path never emits this variant. This
    /// is the loader-side twin of the generator's `GenerateError::VacuousSuccessCriteria`
    /// / `GenerateError::MissingOutcomePredicate` — dark-gated behind the
    /// engine's `ANVIL_ENFORCE_MEASUREMENT_DEFINITION` flag until the fleet is
    /// backfilled.
    ///
    /// Code: `playbook_measurement_definition_missing`
    MeasurementDefinitionMissing {
        artifact_id: String,
        /// Human-readable detail naming the offending state/role, or noting
        /// the missing/blank outcome_predicate.
        detail: String,
    },

    /// A DRIVEN machine has a measured `(state, role)` step that declares no
    /// `evidence_obligation`. Every DRIVEN measured step must name its required
    /// evidence classes (T-EEC-1). States carrying NO `measurement_by_role`
    /// entry are exempt (the same exemption as the measurement-definition gate).
    /// The detail names the playbook kind and offending state + role.
    ///
    /// Emitted ONLY when obligation enforcement is on
    /// (`LoaderEnforcement::evidence_obligation`) — the default load path never
    /// emits this variant. Dark-gated behind the engine's
    /// `ANVIL_ENFORCE_EVIDENCE_OBLIGATION` flag until the fleet is backfilled.
    ///
    /// Code: `playbook_evidence_obligation_missing`
    EvidenceObligationMissing {
        artifact_id: String,
        /// Human-readable detail naming the playbook kind and offending state
        /// + role.
        detail: String,
    },

    /// A FREE-registered machine declares an `evidence_obligation` on a measured
    /// step. FREE kinds are measured downstream only and never carry an
    /// obligation (T-EEC-1). Emitted ONLY when obligation enforcement is on;
    /// dark-gated the same way as [`EvidenceObligationMissing`]. The detail
    /// names the FREE playbook kind (and may also identify the declaring pair).
    ///
    /// Code: `playbook_evidence_obligation_on_free_register`
    EvidenceObligationOnFreeRegister {
        artifact_id: String,
        /// Human-readable detail naming the FREE playbook kind and declaring
        /// state + role.
        detail: String,
    },
}

impl PlaybookLoadError {
    /// Returns the stable snake_case error code for this error variant.
    pub fn code(&self) -> &'static str {
        match self {
            // C9. Deliberately NOT prefixed `playbook_`: it names a hearth
            // DIRECTORY operation, not a machine validation, and the whole point
            // of this track is that the two are different things.
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
            PlaybookLoadError::UnknownQualityDimension { .. } => {
                "playbook_unknown_quality_dimension"
            }
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
}

/// Render `code: detail` WITHOUT doubling a code the detail already carries.
///
/// C-d.1 round 4 (LOW-3). The hearth-level variants carry a `detail` that is the
/// projection error's own `Display`, which opens with the same code — so the
/// operator saw `hearth_directory_collision: hearth_directory_collision: BOTH …`
/// at the RPC, through the persist error's own wrapper.
fn write_coded(f: &mut fmt::Formatter<'_>, code: &str, detail: &str) -> fmt::Result {
    if detail.starts_with(code) {
        write!(f, "{detail}")
    } else {
        write!(f, "{code}: {detail}")
    }
}

impl fmt::Display for PlaybookLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // C-d.1 round 4 (LOW-3): `detail` is the projection error's own
            // Display, which ALREADY opens with this code, so the unconditional
            // prefix produced `hearth_directory_collision:
            // hearth_directory_collision: BOTH ...` at the RPC — operator-facing.
            // Prefix only when the detail does not already carry the code.
            PlaybookLoadError::HearthDirectoryMoveFailed { detail } => {
                write_coded(f, "hearth_directory_move_failed", detail)
            }
            PlaybookLoadError::HearthDirectoryCollision { detail } => {
                write_coded(f, "hearth_directory_collision", detail)
            }
            PlaybookLoadError::HearthRootUnreadable { detail } => {
                write_coded(f, "hearth_root_unreadable", detail)
            }
            PlaybookLoadError::YamlParseError {
                artifact_id,
                line,
                column,
                message,
            } => write!(
                f,
                "playbook_yaml_parse_error: artifact '{}' at {}:{} — {}",
                artifact_id, line, column, message
            ),
            PlaybookLoadError::MissingRequiredKey {
                artifact_id,
                key_path,
            } => write!(
                f,
                "playbook_missing_required_key: artifact '{}' missing key '{}'",
                artifact_id, key_path
            ),
            PlaybookLoadError::UnknownRoleReference {
                artifact_id,
                from_state,
                to_state,
                role,
            } => write!(
                f,
                "playbook_unknown_role_reference: artifact '{}' transition '{}→{}' references undeclared role '{}'",
                artifact_id, from_state, to_state, role
            ),
            PlaybookLoadError::UnknownStateReference {
                artifact_id,
                from_state,
                to_state,
                unknown_state,
            } => write!(
                f,
                "playbook_unknown_state_reference: artifact '{}' transition '{}→{}' references undeclared state '{}'",
                artifact_id, from_state, to_state, unknown_state
            ),
            PlaybookLoadError::ReviewGateMissingSatisfaction {
                artifact_id,
                state,
                to_state,
            } => write!(
                f,
                "playbook_review_gate_missing_satisfaction: artifact '{}' review gate '{}' transition to '{}' has no required_satisfaction",
                artifact_id, state, to_state
            ),
            PlaybookLoadError::HookPathInvalid {
                artifact_id,
                context,
                filename,
            } => write!(
                f,
                "hook_path_invalid: artifact '{}' {} references invalid hook path '{}'",
                artifact_id, context, filename
            ),
            PlaybookLoadError::UnknownHookReference {
                artifact_id,
                context,
                filename,
            } => write!(
                f,
                "playbook_unknown_hook_reference: artifact '{}' {} references missing hook file '{}'",
                artifact_id, context, filename
            ),
            PlaybookLoadError::UnknownRoleKeyReference {
                artifact_id,
                context,
                role,
            } => write!(
                f,
                "playbook_unknown_role_key: artifact '{}' {} references undeclared role '{}'",
                artifact_id, context, role
            ),
            PlaybookLoadError::DuplicateKindRegistration { artifact_ids, kind } => write!(
                f,
                "playbook_duplicate_kind_registration: kind '{}' declared by multiple artifacts: {}",
                kind, artifact_ids
            ),
            PlaybookLoadError::UnreachableState {
                artifact_id,
                initial_state,
                unreachable_state,
            } => write!(
                f,
                "playbook_unreachable_state: artifact '{}' state '{}' is not reachable from initial state '{}'",
                artifact_id, unreachable_state, initial_state
            ),
            PlaybookLoadError::DeadEndState { artifact_id, state } => write!(
                f,
                "playbook_dead_end_state: artifact '{}' non-terminal state '{}' has no outgoing transitions",
                artifact_id, state
            ),
            PlaybookLoadError::NoTerminalReachable { artifact_id, state } => write!(
                f,
                "playbook_no_terminal_reachable: artifact '{}' state '{}' cannot reach any terminal state",
                artifact_id, state
            ),
            PlaybookLoadError::UnknownQualityDimension {
                artifact_id,
                dimension,
            } => write!(
                f,
                "playbook_unknown_quality_dimension: artifact '{}' success_rubric references unknown quality dimension '{}'",
                artifact_id, dimension
            ),
            PlaybookLoadError::InvalidRubricWeight {
                artifact_id,
                dimension,
            } => write!(
                f,
                "playbook_invalid_rubric_weight: artifact '{}' success_rubric dimension '{}' has a non-positive weight",
                artifact_id, dimension
            ),
            PlaybookLoadError::OutcomePredicateUnknownState {
                artifact_id,
                terminal_state,
            } => write!(
                f,
                "playbook_outcome_predicate_unknown_state: artifact '{}' outcome_predicate.terminal_state '{}' is not a declared state",
                artifact_id, terminal_state
            ),
            PlaybookLoadError::MeasurementDefinitionMissing {
                artifact_id,
                detail,
            } => write!(
                f,
                "playbook_measurement_definition_missing: artifact '{}' {}",
                artifact_id, detail
            ),
            PlaybookLoadError::EvidenceObligationMissing {
                artifact_id,
                detail,
            } => write!(
                f,
                "playbook_evidence_obligation_missing: artifact '{}' {}",
                artifact_id, detail
            ),
            PlaybookLoadError::EvidenceObligationOnFreeRegister {
                artifact_id,
                detail,
            } => write!(
                f,
                "playbook_evidence_obligation_on_free_register: artifact '{}' {}",
                artifact_id, detail
            ),
        }
    }
}

impl std::error::Error for PlaybookLoadError {}

impl PlaybookLoadError {
    /// Convert this error to an `InvalidArtifact` entry suitable for inclusion
    /// in `CatalogResult::invalid_artifacts`.
    ///
    /// The `artifact_id` parameter overrides the id embedded in the error when
    /// the caller knows the correct artifact identity (e.g., for duplicate-kind
    /// errors where both ids are available).
    pub fn to_invalid_artifact(&self, artifact_id: &str) -> crate::domain::InvalidArtifact {
        use std::collections::BTreeMap;
        let mut params: BTreeMap<String, String> = BTreeMap::new();

        match self {
            // C9: carries `detail` (the paths + cause), not an artifact_id — the
            // failure is the hearth's, not any one definition's.
            PlaybookLoadError::HearthDirectoryMoveFailed { detail } => {
                params.insert("detail".to_string(), detail.clone());
            }
            PlaybookLoadError::HearthDirectoryCollision { detail }
            | PlaybookLoadError::HearthRootUnreadable { detail } => {
                params.insert("detail".to_string(), detail.clone());
            }
            PlaybookLoadError::YamlParseError { line, column, .. } => {
                params.insert("artifact_id".to_string(), artifact_id.to_string());
                params.insert("line".to_string(), line.to_string());
                params.insert("column".to_string(), column.to_string());
            }
            PlaybookLoadError::MissingRequiredKey { key_path, .. } => {
                params.insert("artifact_id".to_string(), artifact_id.to_string());
                params.insert("key_path".to_string(), key_path.clone());
            }
            PlaybookLoadError::UnknownRoleReference {
                from_state,
                to_state,
                role,
                ..
            } => {
                params.insert("artifact_id".to_string(), artifact_id.to_string());
                params.insert("from_state".to_string(), from_state.clone());
                params.insert("to_state".to_string(), to_state.clone());
                params.insert("role".to_string(), role.clone());
            }
            PlaybookLoadError::UnknownStateReference {
                from_state,
                to_state,
                unknown_state,
                ..
            } => {
                params.insert("artifact_id".to_string(), artifact_id.to_string());
                params.insert("from_state".to_string(), from_state.clone());
                params.insert("to_state".to_string(), to_state.clone());
                params.insert("unknown_state".to_string(), unknown_state.clone());
            }
            PlaybookLoadError::ReviewGateMissingSatisfaction {
                state, to_state, ..
            } => {
                params.insert("artifact_id".to_string(), artifact_id.to_string());
                params.insert("state".to_string(), state.clone());
                params.insert("to_state".to_string(), to_state.clone());
            }
            PlaybookLoadError::HookPathInvalid {
                context, filename, ..
            } => {
                params.insert("artifact_id".to_string(), artifact_id.to_string());
                params.insert("context".to_string(), context.clone());
                params.insert("filename".to_string(), filename.clone());
            }
            PlaybookLoadError::UnknownHookReference {
                context, filename, ..
            } => {
                params.insert("artifact_id".to_string(), artifact_id.to_string());
                params.insert("context".to_string(), context.clone());
                params.insert("filename".to_string(), filename.clone());
            }
            PlaybookLoadError::UnknownRoleKeyReference { context, role, .. } => {
                params.insert("artifact_id".to_string(), artifact_id.to_string());
                params.insert("context".to_string(), context.clone());
                params.insert("role".to_string(), role.clone());
            }
            PlaybookLoadError::DuplicateKindRegistration { artifact_ids, kind } => {
                params.insert("artifact_ids".to_string(), artifact_ids.clone());
                params.insert("kind".to_string(), kind.clone());
            }
            PlaybookLoadError::UnreachableState {
                initial_state,
                unreachable_state,
                ..
            } => {
                params.insert("artifact_id".to_string(), artifact_id.to_string());
                params.insert("initial_state".to_string(), initial_state.clone());
                params.insert("unreachable_state".to_string(), unreachable_state.clone());
            }
            PlaybookLoadError::DeadEndState { state, .. } => {
                params.insert("artifact_id".to_string(), artifact_id.to_string());
                params.insert("state".to_string(), state.clone());
            }
            PlaybookLoadError::NoTerminalReachable { state, .. } => {
                params.insert("artifact_id".to_string(), artifact_id.to_string());
                params.insert("state".to_string(), state.clone());
            }
            PlaybookLoadError::UnknownQualityDimension { dimension, .. } => {
                params.insert("artifact_id".to_string(), artifact_id.to_string());
                params.insert("dimension".to_string(), dimension.clone());
            }
            PlaybookLoadError::InvalidRubricWeight { dimension, .. } => {
                params.insert("artifact_id".to_string(), artifact_id.to_string());
                params.insert("dimension".to_string(), dimension.clone());
            }
            PlaybookLoadError::OutcomePredicateUnknownState { terminal_state, .. } => {
                params.insert("artifact_id".to_string(), artifact_id.to_string());
                params.insert("terminal_state".to_string(), terminal_state.clone());
            }
            PlaybookLoadError::MeasurementDefinitionMissing { detail, .. } => {
                params.insert("artifact_id".to_string(), artifact_id.to_string());
                params.insert("detail".to_string(), detail.clone());
            }
            PlaybookLoadError::EvidenceObligationMissing { detail, .. } => {
                params.insert("artifact_id".to_string(), artifact_id.to_string());
                params.insert("detail".to_string(), detail.clone());
            }
            PlaybookLoadError::EvidenceObligationOnFreeRegister { detail, .. } => {
                params.insert("artifact_id".to_string(), artifact_id.to_string());
                params.insert("detail".to_string(), detail.clone());
            }
        }

        crate::domain::InvalidArtifact {
            id: artifact_id.to_string(),
            code: self.code().to_string(),
            message: self.to_string(),
            params,
        }
    }
}

impl PlaybookLoadError {
    /// The on-disk artifact id(s) this error attributes to.
    ///
    /// The engine uses this to decide whether a load error belongs to the kind a
    /// caller selected. Two variants attribute to NO artifact, and that is the
    /// load-bearing claim here: a hearth-DIRECTORY failure (a failed move, or a
    /// legacy/canonical collision) is a property of the hearth, not of any one
    /// definition. Attributing it to an artifact would blame a definition for a
    /// directory's problem; attributing it to the empty string would slot it
    /// into an "(unassigned)" bucket as if its owner were merely unreadable.
    /// The empty vec is NOT a silent drop — the error still surfaces through
    /// `invalid_artifacts()` and, for the blocking pair, through
    /// `HearthPlaybookRegistry::registration_blocked()`.
    pub fn artifact_ids(&self) -> Vec<String> {
        match self {
            PlaybookLoadError::HearthDirectoryMoveFailed { .. }
            | PlaybookLoadError::HearthDirectoryCollision { .. }
            | PlaybookLoadError::HearthRootUnreadable { .. } => Vec::new(),
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
                vec![artifact_id.clone()]
            }
            PlaybookLoadError::DuplicateKindRegistration { artifact_ids, .. } => artifact_ids
                .split(',')
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(ToString::to_string)
                .collect(),
        }
    }
}
