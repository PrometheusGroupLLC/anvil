//! Domain logic for the `snapshot` engine tool — the authoritative
//! writer for status.yaml transitions, registry mutations, and
//! incremental projection updates.
//!
//! `SnapshotCommandHandler` is a leaf executor, not a domain decision-maker.
//! It is the authoritative mutation-side implementation that
//! `Event::ReviewTransition` (from begin) and `CompleteEvent::TransitionRecorded`
//! (from complete) dispatch into. Its `&dyn SnapshotPort` / `&dyn ActorWritePort`
//! arguments are its defining characteristic and are exempt from the
//! "no `&dyn *WritePort` in domain handler signatures" invariant.
//!
//! `SnapshotPort`'s read methods (`read_artifact_kind`, `read_artifact_state`,
//! `read_artifact_actor_names`) are retained as leaf-executor internals called
//! only from this handler — they are not a domain-handler read surface. See the
//! "Handler shape" amendment in `forge/projections/truth.md` for the canonical
//! scoped exemption.

use crate::domain::begin_adoption::{
    begin_adoption_warning, creating_actor_is, has_open_begin, is_driven_register,
};
use crate::domain::shared_types::{ActorIdentity, TransitionContent};
use crate::ports::actor_write_port::{ActorWriteError, ActorWritePort};
use crate::ports::snapshot_port::SnapshotPort;
use std::fmt;

/// Errors from snapshot operations. The engine layer maps each variant
/// to a structured gRPC `Status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotError {
    InvalidArgument {
        reason: String,
    },
    NotFound {
        artifact_path: String,
    },
    MalformedStatus {
        artifact_path: String,
        message: String,
    },
    IoError {
        message: String,
    },
    /// `actor_name` was absent or empty. The caller must supply a
    /// non-empty actor identity on every snapshot call (spec R3 of the
    /// checkin_backfill_spec_context track).
    ActorNameRequired,
    /// One of the required runtime identity params (`actor_type`,
    /// `actor_model`, `actor_provider`) was empty.
    ActorParamsRequired {
        field: String,
    },
    /// The strict K8 store refused. Carried as its own variant so a backlog
    /// rejection is never flattened into a generic I/O warning (plan Task 4 —
    /// registry failure is CRITICAL for K8, not warn-and-continue).
    BacklogStore {
        message: String,
    },
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapshotError::InvalidArgument { reason } => {
                write!(f, "Invalid argument: {}", reason)
            }
            SnapshotError::NotFound { artifact_path } => {
                write!(f, "Artifact '{}' not found in hearth", artifact_path)
            }
            SnapshotError::MalformedStatus {
                artifact_path,
                message,
            } => {
                write!(
                    f,
                    "Malformed status.yaml for '{}': {}",
                    artifact_path, message
                )
            }
            SnapshotError::IoError { message } => write!(f, "I/O error: {}", message),
            SnapshotError::BacklogStore { message } => {
                write!(f, "Backlog store error: {}", message)
            }
            SnapshotError::ActorNameRequired => {
                write!(f, "actor_name is required and must not be empty")
            }
            SnapshotError::ActorParamsRequired { field } => {
                write!(f, "{} is required and must not be empty", field)
            }
        }
    }
}

impl std::error::Error for SnapshotError {}

/// Request record for a single snapshot command.
#[derive(Debug, Clone, Default)]
pub struct SnapshotRequest {
    // Proto-sourced transition fields
    pub artifact_path: String,
    pub to_state: String,
    pub actor_name: String,
    pub actor_role: String,
    pub approver: String,
    pub note: String,

    // Proto-sourced identity fields (used when seeding the actor)
    pub actor_type: String,
    pub actor_model: String,
    pub actor_provider: String,
    pub actor_context_window: i64,
    pub actor_sdk_version: String,
    pub actor_entrypoint: String,

    // Proto-sourced mode fields
    pub projection_only: bool,
    pub event_type: String,

    // Domain-only (engine-populated, not in proto)
    pub at: String,

    /// Internal opt-in permitting a RESERVED `event_type` (e.g. `"adoption"`)
    /// on a full transition. Set TRUE only by the engine's INTERNAL
    /// governance-adoption route (the `ArtifactAdopted` event handler); a
    /// public snapshot (the snapshot RPC and its MCP passthrough) leaves it
    /// `false`. This is the seam that stops a public caller forging
    /// `event_type: "adoption"` to masquerade a plain transition as a
    /// governance adoption (which would skip begin-marker closure and diverge
    /// the telemetry stream). Not proto-sourced — a proto client cannot set it.
    pub allow_reserved_event_type: bool,

    /// Ordered evidence claims supplied for this lifecycle state entry.
    /// Projection-only requests carry this value through parsing but never
    /// assess or persist it because they are not lifecycle transitions.
    pub claimed_evidence: Vec<crate::domain::shared_types::ClaimedEvidence>,
}

/// Successful snapshot result. `warnings` collects non-critical write
/// failures (registry and projection) per criticality ordering — the
/// overall call is still considered successful if status.yaml updated.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SnapshotResult {
    pub success: bool,
    pub timestamp: String,
    /// The actor name actually used for this transition. When the
    /// request supplies a non-empty `actor_name` it is echoed here;
    /// otherwise the handler's generated name is returned.
    pub actor_name: String,
    pub status_updated: bool,
    pub registry_updated: bool,
    pub projections_updated: Vec<String>,
    pub warnings: Vec<String>,
}

/// The projection targets snapshot knows how to update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionTarget {
    Execution,
    Intent,
    DecisionsCount,
    SparksCount,
    Authoring,
    ArtifactProjection,
}

impl ProjectionTarget {
    pub fn file_name(&self) -> &'static str {
        match self {
            ProjectionTarget::Execution => "execution.md",
            ProjectionTarget::Intent => "intent.md",
            ProjectionTarget::DecisionsCount => "decisions.md",
            ProjectionTarget::SparksCount => "sparks.md",
            ProjectionTarget::Authoring => "authoring.md",
            ProjectionTarget::ArtifactProjection => "projection.md",
        }
    }
}

/// State → section routing for registry files. Consolidated destinations
/// per spec §4. The conditional `conclusion_review → established` edge
/// for learnings is handled inline by the handler when the transition
/// note is `"direct-experience establishment"` — see `execute`.
///
/// Returns `None` for terminal states that don't write to a registry
/// section (e.g., the engine never moves entries into `retired` here;
/// that is covered elsewhere).
pub fn registry_section_for(kind: &str, state: &str) -> Option<&'static str> {
    match (kind, state) {
        // === Tracks === (consolidated per spec §4)
        ("track", "spec") | ("track", "spec_review") | ("track", "spec_revision") => Some("spec"),
        ("track", "plan") | ("track", "plan_review") | ("track", "plan_revision") => Some("plan"),
        ("track", "implementing")
        | ("track", "impl_phase_review")
        | ("track", "impl_review")
        | ("track", "impl_revision") => Some("implementing"),
        ("track", "reflecting")
        | ("track", "reflection_review")
        | ("track", "reflection_revision") => Some("reflecting"),
        ("track", "completed") => Some("completed"),
        ("track", "abandoned") => Some("abandoned"),
        ("track", "superseded") => Some("superseded"),
        ("track", "shelved") => Some("shelved"),
        ("track", "absorbed") => Some("absorbed"),

        // === Proposals === (consolidated per spec §4)
        ("proposal", "vision")
        | ("proposal", "vision_review")
        | ("proposal", "vision_revision") => Some("vision"),
        ("proposal", "draft")
        | ("proposal", "draft_review")
        | ("proposal", "draft_revision")
        | ("proposal", "proposal")
        | ("proposal", "proposal_review")
        | ("proposal", "proposal_revision") => Some("draft"),
        ("proposal", "active")
        | ("proposal", "amend")
        | ("proposal", "amend_review")
        | ("proposal", "amend_revision") => Some("active"),
        ("proposal", "reflecting")
        | ("proposal", "reflection_review")
        | ("proposal", "reflection_revision") => Some("reflecting"),
        ("proposal", "completed") => Some("completed"),
        ("proposal", "superseded") => Some("superseded"),
        ("proposal", "abandoned") => Some("abandoned"),

        // === Milestones === (same consolidation shape as proposals)
        ("milestone", "draft")
        | ("milestone", "draft_review")
        | ("milestone", "draft_revision") => Some("draft"),
        ("milestone", "active")
        | ("milestone", "amend")
        | ("milestone", "amend_review")
        | ("milestone", "amend_revision") => Some("active"),
        ("milestone", "reflecting")
        | ("milestone", "reflection_review")
        | ("milestone", "reflection_revision") => Some("reflecting"),
        ("milestone", "completed") => Some("completed"),
        ("milestone", "superseded") => Some("superseded"),
        ("milestone", "abandoned") => Some("abandoned"),

        // === Initiatives ===
        ("initiative", "draft")
        | ("initiative", "draft_review")
        | ("initiative", "draft_revision") => Some("draft"),
        ("initiative", "active") => Some("active"),
        ("initiative", "promoted") => Some("promoted"),
        ("initiative", "retired") => Some("retired"),

        // === Decisions === (spec §4: tension{,_review,_revision}→tension;
        // investigating→investigating; decided/decision_review/decision_revision/
        // amend*/→decided; retired→retired)
        ("decision", "tension")
        | ("decision", "tension_review")
        | ("decision", "tension_revision") => Some("tension"),
        ("decision", "investigating") => Some("investigating"),
        ("decision", "decided")
        | ("decision", "decision_review")
        | ("decision", "decision_revision")
        | ("decision", "amend")
        | ("decision", "amend_review")
        | ("decision", "amend_revision") => Some("decided"),
        ("decision", "retired") => Some("retired"),

        // === Learnings === (spec §4 consolidation)
        ("learning", "observation")
        | ("learning", "observation_review")
        | ("learning", "observation_revision") => Some("observation"),
        ("learning", "conclusion")
        | ("learning", "conclusion_review")
        | ("learning", "conclusion_revision") => Some("conclusion"),
        ("learning", "established")
        | ("learning", "amend")
        | ("learning", "amend_review")
        | ("learning", "amend_revision") => Some("established"),
        ("learning", "graduated") => Some("graduated"),
        ("learning", "retired") => Some("retired"),

        // === Playbooks === (R3.1 11-state lifecycle per track 20260419T1336)
        ("playbook", "draft") | ("playbook", "draft_review") | ("playbook", "draft_revision") => {
            Some("draft")
        }
        ("playbook", "active")
        | ("playbook", "amend")
        | ("playbook", "amend_review")
        | ("playbook", "amend_revision") => Some("active"),
        ("playbook", "reflecting")
        | ("playbook", "reflection_review")
        | ("playbook", "reflection_revision") => Some("reflecting"),
        ("playbook", "retired") => Some("retired"),

        // === Backlog items === (K8 D2.1: seven states, no review/revision
        // consolidation — each state is its own section of backlog_items.md.)
        ("backlog_item", "candidate") => Some("candidate"),
        ("backlog_item", "ready") => Some("ready"),
        ("backlog_item", "in_flight") => Some("in_flight"),
        ("backlog_item", "parked") => Some("parked"),
        ("backlog_item", "done") => Some("done"),
        ("backlog_item", "superseded") => Some("superseded"),
        ("backlog_item", "aged_out") => Some("aged_out"),

        _ => None,
    }
}

/// The projection targets that should be updated for a given transition.
///
/// - Tracks → execution.md
/// - Proposals/milestones → intent.md
/// - Decisions → decisions.md (count rebuild)
/// - Learnings → no projection (per spec §4)
/// - `*_revision` states → empty (the projection already reflects the
///   mid-review state of the artifact)
/// - Projection-only mode on sparks → sparks.md
pub fn projection_targets_for(
    kind: &str,
    to_state: &str,
    projection_only: bool,
    event_type: &str,
    state_declares_projection: bool,
) -> Vec<ProjectionTarget> {
    if projection_only {
        // Projection-only mode is used by spark capture/annotate events.
        // The only projection target is sparks.md.
        if event_type == "spark" || event_type == "annotation" {
            return vec![ProjectionTarget::SparksCount];
        }
        return Vec::new();
    }

    if to_state.ends_with("_revision") {
        return Vec::new();
    }

    match kind {
        "track" => vec![ProjectionTarget::Execution],
        "playbook_generation" | "workflow_generation" => vec![ProjectionTarget::Authoring],
        "proposal" | "milestone" => vec![ProjectionTarget::Intent],
        "decision" => vec![ProjectionTarget::DecisionsCount],
        _ => {
            if state_declares_projection {
                vec![ProjectionTarget::ArtifactProjection]
            } else {
                Vec::new()
            }
        }
    }
}

/// Derive the artifact id from an artifact path like
/// `"tracks/20260416T0155_deterministic_snapshot_engine"`.
fn artifact_id_from_path(artifact_path: &str) -> &str {
    artifact_path.rsplit('/').next().unwrap_or(artifact_path)
}

/// `event_type` values reserved for the engine's INTERNAL routes. A public
/// snapshot may not set these — see `SnapshotRequest::allow_reserved_event_type`.
/// `"adoption"` is the governance-adoption discriminator that makes
/// `has_open_begin` skip the reset transition; forging it on a public snapshot
/// would let a plain transition masquerade as an adoption.
const RESERVED_EVENT_TYPES: &[&str] = &["adoption"];

/// Whether `event_type` is reserved for an internal route.
fn is_reserved_event_type(event_type: &str) -> bool {
    RESERVED_EVENT_TYPES.contains(&event_type)
}

/// Command handler for the snapshot flow.
pub struct SnapshotCommandHandler;

impl SnapshotCommandHandler {
    /// Validate a snapshot request without reading or mutating its hearth.
    /// Engine-boundary gates call this first so established request errors keep
    /// precedence while the transition itself can still be refused before the
    /// handler's first write.
    pub fn validate(request: &SnapshotRequest) -> Result<(), SnapshotError> {
        if request.artifact_path.is_empty() {
            return Err(SnapshotError::InvalidArgument {
                reason: "artifact_path must not be empty".to_string(),
            });
        }
        if !request.projection_only {
            if request.to_state.is_empty() {
                return Err(SnapshotError::InvalidArgument {
                    reason: "to_state must not be empty".to_string(),
                });
            }
            if request.actor_role.is_empty() {
                return Err(SnapshotError::InvalidArgument {
                    reason: "actor_role must not be empty".to_string(),
                });
            }
        }
        if request.projection_only {
            match request.event_type.as_str() {
                "spark" | "annotation" => {}
                "" => {
                    return Err(SnapshotError::InvalidArgument {
                        reason: "event_type required in projection-only mode".to_string(),
                    });
                }
                other => {
                    return Err(SnapshotError::InvalidArgument {
                        reason: format!("unknown event_type '{}'", other),
                    });
                }
            }
        }
        if !request.projection_only
            && is_reserved_event_type(&request.event_type)
            && !request.allow_reserved_event_type
        {
            return Err(SnapshotError::InvalidArgument {
                reason: format!(
                    "event_type '{}' is reserved for the internal governance-adoption \
                     route and cannot be set on a public snapshot",
                    request.event_type
                ),
            });
        }
        if request.at.is_empty() {
            return Err(SnapshotError::InvalidArgument {
                reason: "at must be populated by the engine".to_string(),
            });
        }
        if !request.projection_only {
            if request.actor_name.is_empty() {
                return Err(SnapshotError::ActorNameRequired);
            }
            for (field, value) in [
                ("actor_type", &request.actor_type),
                ("actor_model", &request.actor_model),
                ("actor_provider", &request.actor_provider),
            ] {
                if value.is_empty() {
                    return Err(SnapshotError::ActorParamsRequired {
                        field: field.to_string(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Exercise every fallible read that precedes the handler's first write.
    /// This lets an engine boundary layer preserve the handler's established
    /// validation/read-error precedence before applying an additional gate.
    /// The handler still repeats these reads while constructing its warning so
    /// this method remains a read-only compatibility seam, not mutable state.
    pub fn validate_before_write(
        port: &dyn SnapshotPort,
        request: &SnapshotRequest,
    ) -> Result<(), SnapshotError> {
        Self::validate(request)?;
        if request.projection_only {
            return Ok(());
        }
        let kind = port.read_artifact_kind(&request.artifact_path)?;
        if is_driven_register(&kind) {
            port.read_artifact_state(&request.artifact_path)?;
            port.read_activity_entries(&request.artifact_path)?;
            port.read_transitions(&request.artifact_path)?;
        }
        Ok(())
    }

    pub fn execute(
        port: &dyn SnapshotPort,
        actor_write_port: &dyn ActorWritePort,
        request: SnapshotRequest,
    ) -> Result<SnapshotResult, SnapshotError> {
        // ---- Input validation ----
        Self::validate(&request)?;

        // ---- Projection-only early path ----
        if request.projection_only {
            let targets = projection_targets_for(
                "", // kind is irrelevant for projection_only = true
                &request.to_state,
                true,
                &request.event_type,
                false,
            );
            let mut warnings: Vec<String> = Vec::new();
            let mut projections_updated: Vec<String> = Vec::new();
            for target in &targets {
                let write_result = match target {
                    ProjectionTarget::SparksCount => port.rebuild_sparks_projection(),
                    ProjectionTarget::DecisionsCount => port.rebuild_decisions_projection(),
                    _ => Ok(()),
                };
                match write_result {
                    Ok(()) => projections_updated.push(target.file_name().to_string()),
                    Err(e) => warnings.push(format!(
                        "projection update failed ({}): {}",
                        target.file_name(),
                        e
                    )),
                }
            }
            return Ok(SnapshotResult {
                success: true,
                timestamp: request.at.clone(),
                actor_name: request.actor_name.clone(),
                status_updated: false,
                registry_updated: false,
                projections_updated,
                warnings,
            });
        }

        // ---- Full transition path ----
        // Read kind from the artifact's status.yaml.
        let kind = port.read_artifact_kind(&request.artifact_path)?;

        // === Begin-adoption soft-warn detection (BP2) ===
        // A pure read over `activity:` + `transitions:`, computed BEFORE the
        // closing transition is appended (warn-before-write) so an actor WITH
        // an open begin-marker correctly reads "open" → no warning. Gated by
        // the explicit driven-register kind match (F-8) — NOT
        // `registry_section_for`, which returns Some for free types.
        // Keyed on the SOURCE state (the state the actor entered), i.e. the
        // artifact's current state before this transition.
        let begin_adoption_warning_text: Option<String> = if is_driven_register(&kind) {
            let source_state = port.read_artifact_state(&request.artifact_path)?;
            let activity = port.read_activity_entries(&request.artifact_path)?;
            let transitions = port.read_transitions(&request.artifact_path)?;
            if has_open_begin(&activity, &transitions, &request.actor_name, &source_state)
                || creating_actor_is(&transitions, &request.actor_name)
            {
                None
            } else {
                Some(begin_adoption_warning(
                    &request.actor_name,
                    &request.artifact_path,
                    &source_state,
                ))
            }
        } else {
            None
        };

        // Validation above guarantees actor_name is non-empty; the
        // engine no longer generates names server-side. The port's
        // generate_actor_name remains for callers that need a fresh
        // name independent of the snapshot flow.
        let resolved_actor_name = request.actor_name.clone();

        // === Criticality 1: status.yaml (fail-fast) ===
        // Apply the uniform actor-write rule so the transition's
        // `actor:` reference has a matching entry in `actors:`.
        let actor_identity = ActorIdentity {
            name: resolved_actor_name.clone(),
            actor_type: request.actor_type.clone(),
            model: request.actor_model.clone(),
            provider: request.actor_provider.clone(),
            context_window: request.actor_context_window,
            sdk_version: request.actor_sdk_version.clone(),
            entrypoint: request.actor_entrypoint.clone(),
            registered_at: request.at.clone(),
        };
        actor_write_port
            .upsert_actor_configuration(&request.artifact_path, &actor_identity)
            .map_err(map_actor_write_error)?;

        let transition = TransitionContent {
            to: request.to_state.clone(),
            at: request.at.clone(),
            actor: resolved_actor_name.clone(),
            role: request.actor_role.clone(),
            approver: if request.approver.is_empty() {
                None
            } else {
                Some(request.approver.clone())
            },
            note: if request.note.is_empty() {
                None
            } else {
                Some(request.note.clone())
            },
            // Snapshot RPC carries no satisfaction metadata (Slice C is wired
            // through the Complete path's CarryForwardWritten + TransitionRecorded
            // events, not the generic snapshot transition).
            satisfaction: None,
            // Thread the request's event-type discriminator onto the persisted
            // transition. Empty (the ordinary case) records as `None` so the
            // event file stays byte-identical; the adoption routing arm sets
            // `"adoption"` so `has_open_begin` can skip the reset transition.
            event_type: if request.event_type.is_empty() {
                None
            } else {
                Some(request.event_type.clone())
            },
        };
        port.append_transition(&request.artifact_path, &transition)?;
        let status_updated = true;

        let mut warnings: Vec<String> = Vec::new();
        if let Some(w) = begin_adoption_warning_text {
            warnings.push(w);
        }

        // === Criticality 2: registry (warn-and-continue) ===
        let registry_updated = match registry_section_for(&kind, &request.to_state) {
            None => {
                // Unmapped (kind, state) — registry skipped with an
                // explicit warning so misconfigured callers can debug.
                warnings.push(format!(
                    "registry update skipped: no mapping for kind='{}' state='{}'",
                    kind, request.to_state
                ));
                false
            }
            Some(target_section) => {
                let registry_file_name = registry_file_for(&kind);
                let artifact_id = artifact_id_from_path(&request.artifact_path);
                let entry_exists = port.registry_entry_exists(registry_file_name, artifact_id);
                match entry_exists {
                    Ok(true) => match port.move_registry_entry(
                        registry_file_name,
                        artifact_id,
                        target_section,
                    ) {
                        Ok(()) => true,
                        Err(e) => {
                            warnings.push(format!("registry update failed: {}", e));
                            false
                        }
                    },
                    Ok(false) => {
                        match port.build_registry_entry_text(
                            &kind,
                            &request.artifact_path,
                            target_section,
                        ) {
                            Ok(entry_text) => match port.create_registry_entry(
                                registry_file_name,
                                artifact_id,
                                &kind,
                                target_section,
                                &entry_text,
                            ) {
                                Ok(()) => true,
                                Err(e) => {
                                    warnings.push(format!("registry update failed: {}", e));
                                    false
                                }
                            },
                            Err(e) => {
                                warnings.push(format!("registry update failed: {}", e));
                                false
                            }
                        }
                    }
                    Err(e) => {
                        warnings.push(format!("registry update failed: {}", e));
                        false
                    }
                }
            }
        };

        // === Criticality 3: projections (warn-and-continue) ===
        let state_declares_projection = port
            .state_declares_projection(&request.artifact_path, &request.to_state)
            .unwrap_or(false);
        let targets = projection_targets_for(
            &kind,
            &request.to_state,
            false,
            &request.event_type,
            state_declares_projection,
        );
        let mut projections_updated: Vec<String> = Vec::new();
        for target in &targets {
            let artifact_id = artifact_id_from_path(&request.artifact_path);
            let write_result = match target {
                ProjectionTarget::Execution => {
                    // execution.md uses title-case projection labels.
                    let section = projection_section_label(&kind, &request.to_state);
                    port.move_execution_row(artifact_id, &section)
                }
                ProjectionTarget::Intent => {
                    let section = projection_section_label(&kind, &request.to_state);
                    port.move_intent_row(artifact_id, &kind, &section)
                }
                ProjectionTarget::DecisionsCount => port.rebuild_decisions_projection(),
                ProjectionTarget::SparksCount => port.rebuild_sparks_projection(),
                ProjectionTarget::Authoring => {
                    let section = projection_section_label(&kind, &request.to_state);
                    port.write_authoring_projection(
                        &request.artifact_path,
                        &section,
                        &request.to_state,
                        &request.at,
                        &request.actor_name,
                        &request.actor_role,
                    )
                }
                ProjectionTarget::ArtifactProjection => {
                    let section = projection_section_label(&kind, &request.to_state);
                    port.write_artifact_projection(
                        &request.artifact_path,
                        &section,
                        &request.to_state,
                        &request.at,
                        &request.actor_name,
                        &request.actor_role,
                    )
                }
            };
            match write_result {
                Ok(()) => projections_updated.push(target.file_name().to_string()),
                Err(e) => warnings.push(format!(
                    "projection update failed ({}): {}",
                    target.file_name(),
                    e
                )),
            }
        }

        Ok(SnapshotResult {
            success: true,
            timestamp: request.at.clone(),
            actor_name: resolved_actor_name,
            status_updated,
            registry_updated,
            projections_updated,
            warnings,
        })
    }
}

fn map_actor_write_error(error: ActorWriteError) -> SnapshotError {
    match error {
        ActorWriteError::IoError { message } => SnapshotError::IoError { message },
        ActorWriteError::MalformedStatus {
            artifact_path,
            message,
        } => SnapshotError::MalformedStatus {
            artifact_path,
            message,
        },
        ActorWriteError::NotFound { artifact_path } => SnapshotError::NotFound { artifact_path },
    }
}

/// The free-kind registry file each artifact kind's rows live in.
pub fn registry_file_for(kind: &str) -> &'static str {
    match kind {
        "track" => "tracks.md",
        "proposal" => "proposals.md",
        "milestone" => "milestones.md",
        "initiative" => "initiatives.md",
        "decision" => "decisions.md",
        "learning" => "learnings.md",
        "playbook" => "playbooks.md",
        "backlog_item" => "backlog_items.md",
        _ => "",
    }
}

/// Title-case projection section label for a state.
///
/// Convention differs by projection:
/// - execution.md (tracks) keeps fine-grained per-state sections —
///   "Spec" and "Spec Review" are separate H2 groups. Return the
///   fine-grained label.
/// - intent.md (proposals, milestones) consolidates by base state
///   under H3 headings — `### Vision`, `### Draft`, `### Active`, etc.
///   Review/revision states map to their base state so the FS adapter's
///   row move lands in an existing section rather than creating a new
///   stray one.
fn projection_section_label(kind: &str, state: &str) -> String {
    match kind {
        "proposal" | "milestone" => consolidated_label(state),
        _ => fine_grained_label(state),
    }
}

/// Track labels — fine-grained, per execution.md's H2 convention.
fn fine_grained_label(state: &str) -> String {
    match state {
        "spec" => "Spec".to_string(),
        "spec_review" => "Spec Review".to_string(),
        "spec_revision" => "Spec Revision".to_string(),
        "plan" => "Planned".to_string(),
        "plan_review" => "Plan Review".to_string(),
        "plan_revision" => "Plan Revision".to_string(),
        "implementing" => "Implementing".to_string(),
        "impl_phase_review" => "Impl Phase Review".to_string(),
        "impl_review" => "Impl Review".to_string(),
        "impl_revision" => "Impl Revision".to_string(),
        "reflecting" => "Reflecting".to_string(),
        "reflection_review" => "Reflection Review".to_string(),
        "reflection_revision" => "Reflection Revision".to_string(),
        "completed" => "Completed".to_string(),
        "abandoned" => "Abandoned".to_string(),
        "superseded" => "Superseded".to_string(),
        "shelved" => "Shelved".to_string(),
        "absorbed" => "Absorbed".to_string(),
        other => title_case_fallback(other),
    }
}

/// Proposal/milestone labels — consolidated base state, per intent.md's
/// H3 convention.
fn consolidated_label(state: &str) -> String {
    match state {
        "vision" | "vision_review" | "vision_revision" => "Vision".to_string(),
        "draft" | "draft_review" | "draft_revision" | "proposal" | "proposal_review"
        | "proposal_revision" => "Draft".to_string(),
        "active" | "amend" | "amend_review" | "amend_revision" => "Active".to_string(),
        "reflecting" | "reflection_review" | "reflection_revision" => "Reflecting".to_string(),
        "completed" => "Completed".to_string(),
        "superseded" => "Superseded".to_string(),
        "abandoned" => "Abandoned".to_string(),
        other => title_case_fallback(other),
    }
}

fn title_case_fallback(state: &str) -> String {
    state
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// K8 governed snapshot path (plan Tasks 4 & 6).
//
// This is the ONLY route from a public request to a K8 lifecycle write. It
// reads strictly, prepares under the printed row/role/guard rules, journals one
// compound capability, and consumes it once. `append_transition` rejects a
// backlog_item outright, so `SnapshotCommandHandler::execute`, Complete, Amend,
// adoption, and any other internal caller cannot reach K8 bytes another way.
// ---------------------------------------------------------------------------

use crate::domain::backlog_item::{
    prepare_backlog_transition, BacklogItemError, DriverRole as K8Role,
    PreparedBacklogTransition, State as K8State, TransitionOrigin,
};
use crate::domain::shared_types::ActorIdentity as K8Actor;
use crate::domain::backlog_manifest::{build_transition_commit, BacklogRegistryRow};
use crate::ports::backlog_item_port::PreparedBacklogCommit;

fn k8_domain_error(e: BacklogItemError) -> SnapshotError {
    SnapshotError::BacklogStore {
        message: e.to_string(),
    }
}

/// The registry projection rows implied by a full item set, with `moved`
/// overriding one row's section.
fn k8_registry_rows(
    items: &[crate::ports::backlog_item_port::LoadedBacklogItem],
    moved: Option<(&str, &str)>,
) -> Vec<BacklogRegistryRow> {
    items
        .iter()
        .map(|l| {
            let section = match moved {
                Some((id, to)) if id == l.id => to.to_string(),
                _ => l.item.state.as_str().to_string(),
            };
            BacklogRegistryRow {
                id: l.id.clone(),
                title: l.item.title.clone(),
                section,
            }
        })
        .collect()
}

/// Prepare one governed K8 transition against strict live revisions.
///
/// Returns a consume-once compound capability. NOTHING is written here: a
/// rejection at any step leaves every K8 file byte-identical.
#[allow(clippy::too_many_arguments)]
pub fn prepare_k8_transition(
    port: &dyn SnapshotPort,
    bi_id: &str,
    from: K8State,
    to: K8State,
    role: K8Role,
    actor: &K8Actor,
    at: &str,
    approver: Option<&str>,
    origin: TransitionOrigin,
    operation_id: &str,
    hi_res_prefix: &str,
    random_suffix: &str,
) -> Result<(PreparedBacklogTransition, PreparedBacklogCommit), SnapshotError> {
    let source = port.read_backlog_item(bi_id)?;
    // A rank-sensitive or #5 guard is NEVER decided from a one-item read.
    let context = port
        .read_backlog_transition_context(&source.item.business_node_id)
        .ok();

    let prepared = prepare_backlog_transition(
        &source.item,
        &source.history,
        from,
        to,
        role,
        actor,
        at,
        origin,
        context.as_ref(),
        approver,
    )
    .map_err(k8_domain_error)?;

    let all = port.read_backlog_items()?;
    let derived_sources: Vec<_> = prepared
        .derived_rank
        .iter()
        .filter(|d| d.bi_id != source.id)
        .filter_map(|d| all.iter().find(|l| l.id == d.bi_id).cloned())
        .collect();
    let rows = k8_registry_rows(&all, Some((&source.id, to.as_str())));
    let registry_old_hash = context.as_ref().and_then(|c| c.registry_hash.clone());

    let commit = build_transition_commit(
        operation_id,
        &prepared,
        &source,
        &derived_sources,
        &rows,
        registry_old_hash,
        hi_res_prefix,
        random_suffix,
    )
    .map_err(|e| SnapshotError::BacklogStore {
        message: e.to_string(),
    })?;
    Ok((prepared, commit))
}

/// Consume a prepared K8 capability through the privileged compound writer.
pub fn commit_k8_transition(
    port: &dyn SnapshotPort,
    prepared: PreparedBacklogCommit,
) -> Result<(), SnapshotError> {
    port.commit_backlog_transition(prepared)
}
