pub mod activity_summary;
pub mod actor_activity;
pub mod actor_configuration;
pub mod amend;
pub mod amend_events;
pub mod amendment;
pub mod autonomy_evidence;
pub mod backlog_item;
pub mod backlog_manifest;
pub mod begin;
pub mod begin_adoption;
pub mod catalog;
pub mod change_record;
pub mod checkin;
pub mod complete;
pub mod complete_events;
pub mod content_hash;
pub mod describe;
pub mod enforcement_bundle_check;
pub mod events;
pub mod hook_manifest;
pub mod hooks;
pub mod join_episode;
pub mod live_instances;
pub mod merge_check;
pub mod outcome_predicate_fold;
pub mod persist_playbook;
pub mod persist_playbook_events;
pub mod playbook;
pub mod playbook_version;
pub mod route;
pub mod route_response;
pub mod run_detail;
pub mod routing;
pub mod shared_types;
pub mod snapshot;
pub mod status;
pub mod status_header;
pub mod step_two_by_two;
pub mod survivor_outcome;
pub mod telemetry_salt;
pub mod transition_log;
pub mod usage_timeseries;
pub mod artifact_activity;
pub mod playbook_run_fidelity;

use serde::{Deserialize, Serialize};

/// Summary of an artifact in the hearth.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactSummary {
    pub id: String,
    pub artifact_type: ArtifactType,
    pub state: String,
    pub summary: String,
    /// Populated by the checkin flow based on session role. Empty string
    /// when the summary is not being surfaced to an agent (e.g., catalog
    /// reads not associated with a role).
    #[serde(default)]
    pub execution_route: String,
}

/// The kinds of artifacts in the forge lifecycle.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactType {
    Proposal,
    Track,
    Milestone,
    Initiative,
    Decision,
    Learning,
    /// Playbook defines another kind's lifecycle (state machine + hooks).
    #[serde(rename = "playbook", alias = "workflow")]
    Playbook,
    /// K8 backlog item — a Free, parent-less data-artifact lifecycle.
    #[serde(rename = "backlog_item")]
    BacklogItem,
}

impl ArtifactType {
    /// The canonical string representation of this artifact type.
    pub fn as_str(&self) -> &'static str {
        match self {
            ArtifactType::Proposal => "proposal",
            ArtifactType::Track => "track",
            ArtifactType::Milestone => "milestone",
            ArtifactType::Initiative => "initiative",
            ArtifactType::Decision => "decision",
            ArtifactType::Learning => "learning",
            ArtifactType::Playbook => "playbook",
            ArtifactType::BacklogItem => "backlog_item",
        }
    }

    /// The directory name under the hearth for this artifact type.
    pub fn directory_name(&self) -> &'static str {
        match self {
            ArtifactType::Proposal => "proposals",
            ArtifactType::Track => "tracks",
            ArtifactType::Milestone => "milestones",
            ArtifactType::Initiative => "initiatives",
            ArtifactType::Decision => "decisions",
            ArtifactType::Learning => "learnings",
            ArtifactType::Playbook => "playbooks",
            ArtifactType::BacklogItem => "backlog_items",
        }
    }

    /// The registry file name for this artifact type.
    pub fn registry_file(&self) -> &'static str {
        match self {
            ArtifactType::Proposal => "proposals.md",
            ArtifactType::Track => "tracks.md",
            ArtifactType::Milestone => "milestones.md",
            ArtifactType::Initiative => "initiatives.md",
            ArtifactType::Decision => "decisions.md",
            ArtifactType::Learning => "learnings.md",
            ArtifactType::Playbook => "playbooks.md",
            ArtifactType::BacklogItem => "backlog_items.md",
        }
    }
}

/// All artifact types that the hearth reader scans.
pub const ALL_ARTIFACT_TYPES: &[ArtifactType] = &[
    ArtifactType::Proposal,
    ArtifactType::Track,
    ArtifactType::Milestone,
    ArtifactType::Initiative,
    ArtifactType::Decision,
    ArtifactType::Learning,
    ArtifactType::Playbook,
    ArtifactType::BacklogItem,
];

/// An artifact type available for creation in the playbook.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AvailableArtifactType {
    pub name: String,
    pub description: String,
    /// The parent artifact type required for creation, if any.
    /// Empty string if no parent required.
    pub requires_parent: String,
    /// Populated by the checkin flow based on session role.
    #[serde(default)]
    pub execution_route: String,
}

/// The artifact types available in the forge lifecycle.
/// `execution_route` is left empty here — the checkin flow populates it
/// based on session role via `compute_execution_route`.
pub fn available_artifact_types() -> Vec<AvailableArtifactType> {
    vec![
        AvailableArtifactType {
            name: "proposal".to_string(),
            description: "Strategic direction that spawns tracks. Begins in the vision phase."
                .to_string(),
            requires_parent: String::new(),
            execution_route: String::new(),
        },
        AvailableArtifactType {
            name: "track".to_string(),
            description: "Concrete implementation of a proposal slice.".to_string(),
            requires_parent: "proposal".to_string(),
            execution_route: String::new(),
        },
        AvailableArtifactType {
            name: "milestone".to_string(),
            description: "Delivery-scoped outcome spanning multiple proposals.".to_string(),
            requires_parent: String::new(),
            execution_route: String::new(),
        },
        AvailableArtifactType {
            name: "initiative".to_string(),
            description: "Cross-cutting implementation expectation with evidence.".to_string(),
            requires_parent: String::new(),
            execution_route: String::new(),
        },
        AvailableArtifactType {
            name: "decision".to_string(),
            description: "Institutional knowledge — what was decided, what was rejected, and why."
                .to_string(),
            requires_parent: String::new(),
            execution_route: String::new(),
        },
        AvailableArtifactType {
            name: "playbook".to_string(),
            description: "Definition of another artifact kind's lifecycle (state machine + hooks)."
                .to_string(),
            requires_parent: "track".to_string(),
            execution_route: String::new(),
        },
        AvailableArtifactType {
            name: "backlog_item".to_string(),
            description: "K8 backlog item — a governed, parent-less data-artifact lifecycle."
                .to_string(),
            requires_parent: String::new(),
            execution_route: String::new(),
        },
    ]
}

/// A playbook artifact that failed validation during catalog loading.
///
/// Invalid artifacts appear in `CatalogResult::invalid_artifacts` instead
/// of `active_artifacts`. One entry per malformed artifact; one artifact's
/// failure does not short-circuit other artifacts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InvalidArtifact {
    /// The artifact directory name (id).
    pub id: String,
    /// Stable snake_case error code from spec R5.2 (e.g., `"playbook_yaml_parse_error"`).
    pub code: String,
    /// Human-readable message summarizing the error.
    pub message: String,
    /// Code-specific diagnostic parameters (e.g., `{"line": "3", "column": "5"}`).
    pub params: std::collections::BTreeMap<String, String>,
}

/// The result of a catalog query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogResult {
    pub active_artifacts: Vec<ArtifactSummary>,
    pub available_types: Vec<AvailableArtifactType>,
    /// Playbook artifacts that failed machine.yaml validation.
    /// Malformed artifacts appear here and are absent from `active_artifacts`.
    #[serde(default)]
    pub invalid_artifacts: Vec<InvalidArtifact>,
}

/// States that indicate an artifact is no longer active.
pub const TERMINAL_STATES: &[&str] = &["completed", "superseded", "abandoned", "retired"];

/// States where a doer can continue work (revision states + authoring states).
pub const DOER_ACTIONABLE_STATES: &[&str] = &[
    "spec",
    "plan",
    "implementing",
    "spec_revision",
    "plan_revision",
    "impl_revision",
    "reflecting",
    "reflection_revision",
];

/// States where a reviewer can act (all *_review states across artifact kinds).
pub const REVIEW_PENDING_STATES: &[&str] = &[
    "spec_review",
    "plan_review",
    "impl_phase_review",
    "impl_review",
    "reflection_review",
    "draft_review",
    "amend_review",
    "tension_review",
    "decision_review",
    "vision_review",
    "proposal_review",
];

/// Doer-produced states whose next action requires the reviewer role.
/// The reviewer filter returns artifacts in REVIEW_PENDING_STATES plus these.
/// Currently limited to `spec` per spec §6; broader states added by their
/// respective review strands.
pub const REVIEW_AWAITING_STATES: &[&str] = &["spec"];

/// States where a creator can create child artifacts under a required parent.
/// Parent-required machines use the proposal/active rule.
pub const CREATOR_PARENT_STATES: &[&str] = &["active"];

/// Returns true if the given state is terminal.
pub fn is_terminal_state(state: &str) -> bool {
    TERMINAL_STATES.contains(&state)
}

/// The three terminal states of the K8 `backlog_item` lifecycle. `done` and
/// `aged_out` are K8-only names, so they are NOT in the cross-kind
/// [`TERMINAL_STATES`] list; asking the shared list about them would report a
/// finished backlog item as active.
pub const BACKLOG_ITEM_TERMINAL_STATES: &[&str] = &["done", "superseded", "aged_out"];

/// Returns true if `state` is terminal FOR `kind`.
///
/// Terminality is a per-lifecycle fact: K8 finishes at `done`, `superseded`, or
/// `aged_out`, while every other kind keeps the cross-kind
/// [`is_terminal_state`] answer unchanged. Callers that filter a mixed artifact
/// list (the catalog scan) must use this, not the kind-blind variant.
pub fn is_terminal_state_for_kind(kind: &str, state: &str) -> bool {
    match kind {
        "backlog_item" => BACKLOG_ITEM_TERMINAL_STATES.contains(&state),
        _ => is_terminal_state(state),
    }
}

/// Errors from hearth operations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum HearthError {
    /// The hearth directory does not exist or is not accessible.
    HearthNotFound { path: String, message: String },
    /// A status.yaml file is malformed or missing required fields.
    MalformedStatus {
        artifact_id: String,
        message: String,
    },
    /// A generic I/O or filesystem error.
    IoError { message: String },
}

impl std::fmt::Display for HearthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HearthError::HearthNotFound { path, message } => {
                write!(f, "Hearth not found at '{}': {}", path, message)
            }
            HearthError::MalformedStatus {
                artifact_id,
                message,
            } => {
                write!(
                    f,
                    "Malformed status.yaml for '{}': {}",
                    artifact_id, message
                )
            }
            HearthError::IoError { message } => {
                write!(f, "I/O error: {}", message)
            }
        }
    }
}

impl std::error::Error for HearthError {}
