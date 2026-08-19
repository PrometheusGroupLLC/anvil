use crate::domain::routing::{
    compute_execution_route, SUBJECT_AVAILABLE_TYPE, SUBJECT_FILTERED_ARTIFACT,
};
use crate::domain::{
    available_artifact_types, ArtifactSummary, ArtifactType, AvailableArtifactType,
    CREATOR_PARENT_STATES, DOER_ACTIONABLE_STATES, REVIEW_AWAITING_STATES, REVIEW_PENDING_STATES,
};
use crate::ports::checkin_query_port::CheckinQueryPort;
use rand::Rng;
use std::fmt;

/// Errors from checkin and begin operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckinError {
    UnsupportedType {
        artifact_type: String,
    },
    UnsupportedRole {
        role: String,
    },
    ParentNotFound {
        parent_id: String,
    },
    ParentNotActive {
        parent_id: String,
        current_state: String,
    },
    /// The parent artifact exists and is active, but its kind is not valid
    /// for the requested artifact type. E.g., playbook requires a track parent;
    /// a proposal parent is rejected even when active.
    ParentKindInvalid {
        parent_id: String,
        expected_kind: String,
        actual_kind: String,
    },
    IoError {
        message: String,
    },
    MalformedStatus {
        artifact_id: String,
        message: String,
    },
    /// A field declared required by the resolved machine's `required_fields`
    /// was not provided on the create request. Carries the field name. Display
    /// embeds the stable assertable substring `missing_required_field`.
    /// (Anvil-lane 1b — machine-driven required check.)
    MissingRequiredField {
        field: String,
    },
}

impl fmt::Display for CheckinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckinError::UnsupportedType { artifact_type } => {
                write!(
                    f,
                    "Unsupported artifact type: '{}'. Only 'track' is currently supported.",
                    artifact_type
                )
            }
            CheckinError::UnsupportedRole { role } => {
                write!(
                    f,
                    "Unsupported role: '{}'. Supported roles: 'creator', 'resumer', 'reviewer'.",
                    role
                )
            }
            CheckinError::ParentNotFound { parent_id } => {
                write!(f, "Parent artifact not found: '{}'", parent_id)
            }
            CheckinError::ParentNotActive {
                parent_id,
                current_state,
            } => {
                write!(
                    f,
                    "Parent artifact '{}' is not active (current state: '{}')",
                    parent_id, current_state
                )
            }
            CheckinError::ParentKindInvalid {
                parent_id,
                expected_kind,
                actual_kind,
            } => {
                write!(
                    f,
                    "Parent artifact '{}' has kind '{}', expected '{}'",
                    parent_id, actual_kind, expected_kind
                )
            }
            CheckinError::IoError { message } => {
                write!(f, "I/O error: {}", message)
            }
            CheckinError::MalformedStatus {
                artifact_id,
                message,
            } => {
                write!(
                    f,
                    "Malformed status.yaml for '{}': {}",
                    artifact_id, message
                )
            }
            CheckinError::MissingRequiredField { field } => {
                write!(
                    f,
                    "missing_required_field: required field '{}' was not provided",
                    field
                )
            }
        }
    }
}

impl std::error::Error for CheckinError {}

/// Validate that the artifact type is supported by resolving it against the
/// playbook registry: a type is supported iff a `PlaybookMachine` resolves for
/// it. This drives ANY registry-resolved machine (track, playbook, knowledge,
/// etc.) rather than a hardcoded `track | playbook` list.
pub fn validate_artifact_type(
    artifact_type: &str,
    registry: &dyn crate::domain::playbook::registry::PlaybookRegistry,
) -> Result<(), CheckinError> {
    if registry.machine_for(artifact_type).is_some() {
        Ok(())
    } else {
        Err(CheckinError::UnsupportedType {
            artifact_type: artifact_type.to_string(),
        })
    }
}

/// Validate that a required parent artifact is in an eligible state to create
/// a child under it. Required-parent creation uses the proposal/active rule.
pub fn validate_parent_state(
    parent_id: &str,
    state: Option<&str>,
    _parent_kind: &str,
) -> Result<(), CheckinError> {
    match state {
        None => Err(CheckinError::ParentNotFound {
            parent_id: parent_id.to_string(),
        }),
        Some(current) if CREATOR_PARENT_STATES.contains(&current) => Ok(()),
        Some(current) => Err(CheckinError::ParentNotActive {
            parent_id: parent_id.to_string(),
            current_state: current.to_string(),
        }),
    }
}

/// Generate an actor name from a word list.
/// Format: Word-NNNNNN (proper noun + hyphen + 6-digit zero-padded random number).
pub fn generate_actor_name(word_list: &[String]) -> String {
    let mut rng = rand::thread_rng();
    let word = &word_list[rng.gen_range(0..word_list.len())];
    let suffix: u32 = rng.gen_range(0..1_000_000);
    format!("{}-{:06}", word, suffix)
}

/// Derive a human-readable proposal name from a proposal directory id.
/// E.g., "20260411T2021_anvil_workflow_engine" -> "anvil-workflow-engine"
pub fn derive_proposal_name(proposal_id: &str) -> String {
    let parts: Vec<&str> = proposal_id.split('_').collect();
    if parts.len() > 1
        && parts[0].len() >= 8
        && parts[0].chars().all(|c| c.is_ascii_digit() || c == 'T')
    {
        parts[1..].join("-")
    } else {
        proposal_id.replace('_', "-")
    }
}

// === Checkin query handler ===

/// The role the agent declares when checking in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckinRole {
    Creator,
    Resumer,
    Reviewer,
}

/// Validate and parse a role string into a CheckinRole.
pub fn validate_checkin_role(role: &str) -> Result<CheckinRole, CheckinError> {
    match role {
        "creator" => Ok(CheckinRole::Creator),
        "resumer" => Ok(CheckinRole::Resumer),
        "reviewer" => Ok(CheckinRole::Reviewer),
        _ => Err(CheckinError::UnsupportedRole {
            role: role.to_string(),
        }),
    }
}

/// Request for the checkin query (identity + filtered discovery).
#[derive(Debug, Clone)]
pub struct CheckinQueryRequest {
    pub role: String,
    pub actor_type: String,
    pub actor_model: String,
    pub actor_provider: String,
}

/// Result of the checkin query.
#[derive(Debug, Clone)]
pub struct CheckinQueryResult {
    pub actor_name: String,
    pub filtered_artifacts: Vec<ArtifactSummary>,
    pub available_types: Vec<AvailableArtifactType>,
    /// T4 — re-served hook content for any artifact the checking-in actor has an
    /// OPEN begin on, so a compacted/resumed session re-warms its standing
    /// context. The discovery-only `CheckinQueryHandler` leaves this empty (it
    /// has no `QueryPort`/registry access); the engine layer fills it via
    /// `begin::reserve_hook_for_open_begin_via_query` over the resolved
    /// artifacts, reusing the same budget-capped serve path as `begin`. Empty
    /// when the actor has no open begin (re-serve is purely additive).
    pub context: String,
}

/// Query handler for the checkin flow.
pub struct CheckinQueryHandler;

impl CheckinQueryHandler {
    pub fn execute(
        port: &dyn CheckinQueryPort,
        request: CheckinQueryRequest,
    ) -> Result<CheckinQueryResult, CheckinError> {
        let role = validate_checkin_role(&request.role)?;

        let word_list = port.load_word_list()?;
        let actor_name = generate_actor_name(&word_list);

        let all_artifacts = port.list_artifacts().map_err(|e| CheckinError::IoError {
            message: e.to_string(),
        })?;

        let role_str = role_to_string(&role);
        let filtered_artifacts: Vec<ArtifactSummary> = filter_by_role(&role, &all_artifacts)
            .into_iter()
            .map(|mut a| {
                a.execution_route = compute_execution_route(
                    SUBJECT_FILTERED_ARTIFACT,
                    a.artifact_type.as_str(),
                    &a.state,
                    &role_str,
                );
                a
            })
            .collect();

        let available_types = match role {
            CheckinRole::Creator => available_artifact_types()
                .into_iter()
                .map(|mut t| {
                    t.execution_route =
                        compute_execution_route(SUBJECT_AVAILABLE_TYPE, &t.name, "", &role_str);
                    t
                })
                .collect(),
            _ => Vec::new(),
        };

        Ok(CheckinQueryResult {
            actor_name,
            filtered_artifacts,
            available_types,
            // Discovery-only handler: re-serve is an engine-layer concern (it
            // needs QueryPort + registry). Left empty here.
            context: String::new(),
        })
    }
}

fn role_to_string(role: &CheckinRole) -> String {
    match role {
        CheckinRole::Creator => "creator",
        CheckinRole::Resumer => "resumer",
        CheckinRole::Reviewer => "reviewer",
    }
    .to_string()
}

/// Filter artifacts based on the declared role.
/// Reviewer returns `REVIEW_PENDING_STATES ∪ REVIEW_AWAITING_STATES`, but
/// only for artifact kinds whose awaiting state is engine-supported in the
/// current strand — `REVIEW_AWAITING_STATES = ["spec"]` applies to tracks
/// only. Proposals in `spec` or similar would be surfaced separately when
/// their review strand ships.
fn filter_by_role(role: &CheckinRole, artifacts: &[ArtifactSummary]) -> Vec<ArtifactSummary> {
    match role {
        CheckinRole::Creator => artifacts
            .iter()
            .filter(|a| CREATOR_PARENT_STATES.contains(&a.state.as_str()))
            .cloned()
            .collect(),
        CheckinRole::Resumer => artifacts
            .iter()
            .filter(|a| DOER_ACTIONABLE_STATES.contains(&a.state.as_str()))
            .cloned()
            .collect(),
        CheckinRole::Reviewer => artifacts
            .iter()
            .filter(|a| {
                REVIEW_PENDING_STATES.contains(&a.state.as_str())
                    || (REVIEW_AWAITING_STATES.contains(&a.state.as_str())
                        && a.artifact_type == ArtifactType::Track)
            })
            .cloned()
            .collect(),
    }
}
