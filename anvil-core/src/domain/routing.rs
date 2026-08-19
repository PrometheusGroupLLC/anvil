//! Computes the `execution_route` discriminator for subjects returned by
//! checkin and describe. This tells the agent whether a given subject's
//! action is executed by the engine (`"engine"`) or by a playbook skill
//! (`"fallback:forge:<skill>"`).
//!
//! Per spec §13, the discriminator describes the action available to a
//! session in `role` on this subject:
//! - For an `ActiveArtifact` (filtered_artifact): the action the role
//!   performs on it (reviewer → review; creator looking at a proposal →
//!   create a child track under it, NOT `begin(identifier)` on the
//!   proposal itself).
//! - For an `AvailableArtifactType` (available_type): whether
//!   `begin(type, ...)` is engine-supported.
//! - For an `AvailableAction` (available_action): whether the action is
//!   executed by the engine.
//!
//! As future strands widen engine support, they flip `"fallback:*"`
//! entries here to `"engine"`. No command-file changes required.
//!
//! The discriminator table below is canonical. Keep in sync with
//! `tracks/20260414T0405_review_spec_strand/plan.md` §45.

/// Discriminator subject kinds.
pub const SUBJECT_FILTERED_ARTIFACT: &str = "filtered_artifact";
pub const SUBJECT_AVAILABLE_TYPE: &str = "available_type";
pub const SUBJECT_AVAILABLE_ACTION: &str = "available_action";

/// The default fallback when no more specific mapping applies.
const FALLBACK_REVIEW: &str = "fallback:forge:review";
const ENGINE: &str = "engine";
const TRACK_DOER_RESUME_STATES: &[&str] = &[
    // `spec` is engine-resumable (the retired `forge:spec` skill points at the
    // engine): an adopted-at-initial-state track (or any freshly-created spec
    // track) resumes through the engine's doer path, not a fallback (Finding 5).
    "spec",
    "plan",
    "implementing",
    "reflecting",
    "spec_revision",
    "plan_revision",
    "impl_revision",
    "reflection_revision",
];

/// Compute the `execution_route` discriminator value for a subject.
///
/// `subject_kind`: one of `SUBJECT_FILTERED_ARTIFACT`, `SUBJECT_AVAILABLE_TYPE`,
/// `SUBJECT_AVAILABLE_ACTION`.
/// `artifact_kind`: the artifact kind string (e.g., `"track"`, `"proposal"`).
/// For `available_type`, this is the type name itself.
/// `state`: the artifact's current state (or empty for `available_type`).
/// `role`: for `filtered_artifact`, the session role (creator/resumer/reviewer);
/// for `available_type`, the session role; for `available_action`, the
/// action's required_role.
pub fn compute_execution_route(
    subject_kind: &str,
    artifact_kind: &str,
    state: &str,
    role: &str,
) -> String {
    match subject_kind {
        SUBJECT_FILTERED_ARTIFACT => filtered_artifact_playbook(artifact_kind, state, role),
        SUBJECT_AVAILABLE_TYPE => available_type_playbook(artifact_kind, role),
        SUBJECT_AVAILABLE_ACTION => available_action_playbook(artifact_kind, state, role),
        _ => FALLBACK_REVIEW.to_string(),
    }
}

fn filtered_artifact_playbook(artifact_kind: &str, state: &str, role: &str) -> String {
    match (artifact_kind, state, role) {
        // (track, spec, reviewer) retired per spec R4 — spec → spec_review
        // is now driven by the doer's `complete` call. Reviewers enter on
        // a `spec_review` track for context delivery only.
        ("track", "spec_review", "reviewer") => ENGINE.to_string(),
        // Slice B: doer re-entry on a `spec_revision` track (begin(identifier)
        // delivers revision context) and reviewer entry (the engine returns the
        // spec_not_ready_for_review rejection directly) are both engine-handled.
        // `creator` is the wire-format value for a doer begin session (R4.1).
        ("track", "spec_revision", "creator") => ENGINE.to_string(),
        ("track", "spec_revision", "reviewer") => ENGINE.to_string(),
        ("proposal", _, _) => ENGINE.to_string(),
        ("track", state, "resumer") if TRACK_DOER_RESUME_STATES.contains(&state) => {
            ENGINE.to_string()
        }
        ("track", state, "resumer") => resumer_fallback_for_state(state),
        // playbook kind: all states are engine-handled per R4.3 (compiled-in seed,
        // engine drives transitions via `snapshot`). Wildcard covers all 11 states.
        // See track 20260419T1336 plan.md Phase 3.
        ("workflow", _, _) => ENGINE.to_string(),
        ("milestone", _, _) => ENGINE.to_string(),
        ("decision", _, _) => ENGINE.to_string(),
        ("initiative", _, _) => ENGINE.to_string(),
        ("learning", _, _) => ENGINE.to_string(),
        _ => FALLBACK_REVIEW.to_string(),
    }
}

fn available_type_playbook(type_name: &str, role: &str) -> String {
    match (type_name, role) {
        ("track", "creator") => ENGINE.to_string(),
        ("proposal", _) => ENGINE.to_string(),
        ("milestone", _) => ENGINE.to_string(),
        ("initiative", _) => ENGINE.to_string(),
        ("decision", _) => ENGINE.to_string(),
        ("learning", _) => ENGINE.to_string(),
        // K8 backlog_item genesis is engine-supported (Begin mints the item).
        ("backlog_item", _) => ENGINE.to_string(),
        // "playbook" falls through intentionally to FALLBACK_REVIEW —
        // no `forge:playbook.md` skill exists yet; using FALLBACK_REVIEW
        // routes to the generic `forge:review` skill which handles
        // arbitrary artifacts. When a downstream track authors
        // `forge:playbook.md`, it adds an explicit ("playbook", _) arm.
        // See track 20260419T1336 plan.md M3 resolution.
        _ => FALLBACK_REVIEW.to_string(),
    }
}

fn available_action_playbook(
    artifact_kind: &str,
    state: &str,
    action_required_role: &str,
) -> String {
    match (artifact_kind, state, action_required_role) {
        // Spec-phase complete — doer-side (spec → spec_review) and
        // reviewer-side with satisfied (spec_review → plan).
        ("track", "spec", "spec") => ENGINE.to_string(),
        ("track", "spec_review", "reviewer") => ENGINE.to_string(),
        // Slice B: the doer's complete (spec_revision → spec_review) is the
        // engine-supported action from spec_revision; describe surfaces it with
        // required_role "spec" (the seed edge role).
        ("track", "spec_revision", "spec") => ENGINE.to_string(),
        ("track", state, "plan") if state == "plan" => ENGINE.to_string(),
        ("track", state, "implement") if state == "implementing" => ENGINE.to_string(),
        ("track", state, "reflect") if state == "reflecting" => ENGINE.to_string(),
        ("track", state, "reviewer") if state == "plan_revision" => ENGINE.to_string(),
        ("track", state, "reviewer") if state == "impl_revision" => ENGINE.to_string(),
        ("track", state, "reviewer") if state == "reflection_revision" => ENGINE.to_string(),
        _ => FALLBACK_REVIEW.to_string(),
    }
}

/// Registry-aware discriminator for the DESCRIBE seam (AC7). Identical to
/// `compute_execution_route` for the legacy literal-handled kinds
/// (track / proposal / playbook / free kinds) so their behavior is preserved
/// byte-identically; for an `available_action` on a domain machine kind that the
/// engine now drives, it reports `"engine"`. Catalog and checkin retain the
/// registry-free `compute_execution_route` (they hold no registry); this
/// variant is only wired on the describe path, which already holds a registry.
pub fn compute_execution_route_with_registry(
    subject_kind: &str,
    artifact_kind: &str,
    state: &str,
    role: &str,
    registry: &dyn crate::domain::playbook::registry::PlaybookRegistry,
) -> String {
    // Only the available_action subject gains machine-derived discrimination;
    // the other subject kinds keep the registry-free behavior.
    if subject_kind == SUBJECT_AVAILABLE_ACTION {
        // Legacy literal-handled kinds keep their exact existing discriminators
        // (the `("playbook", _, _) => ENGINE` wildcard is NOT edge-derivable and
        // must stay explicit; the track arms must produce identical results).
        const LEGACY_KINDS: &[&str] = &["track", "workflow"];
        if !LEGACY_KINDS.contains(&artifact_kind) {
            if let Some(machine) = registry.machine_for(artifact_kind) {
                // The engine drives an action from `state` for `role` iff the
                // machine declares an outgoing edge from `state` whose
                // required_role matches the action role.
                let engine_driven = machine
                    .transitions
                    .iter()
                    .any(|t| t.from_state == state && t.required_role == role);
                return if engine_driven {
                    ENGINE.to_string()
                } else {
                    FALLBACK_REVIEW.to_string()
                };
            }
        }
    }
    compute_execution_route(subject_kind, artifact_kind, state, role)
}

/// Maps a doer-actionable state to the lifecycle skill that resumes it.
/// Used for resumer filtered_artifact rows.
fn resumer_fallback_for_state(state: &str) -> String {
    let skill = match state {
        "spec" | "spec_revision" => "spec",
        "plan" | "plan_revision" => "plan",
        "implementing" | "impl_revision" => "implement",
        "reflecting" | "reflection_revision" => "reflect",
        _ => "review",
    };
    format!("fallback:forge:{}", skill)
}
