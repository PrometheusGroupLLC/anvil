//! Computes the `execution_route` discriminator for subjects returned by
//! checkin and describe. This tells the agent who executes a given subject's
//! action.
//!
//! There are exactly two honest answers:
//!
//! - `"engine"` — the engine executes it, and serves the state's hook content
//!   into the conversation.
//! - `"none"` — no action exists for this role on this subject (a terminal
//!   artifact, an unrecognized subject).
//!
//! There used to be a third shape: a fallback string naming an external skill
//! for the caller to go invoke. That answer is gone, along with the skills it
//! addressed. The addresses named nothing, and nothing ever checked — a caller
//! received a bare string and went looking for a file that had been deleted.
//! A track sitting at a review gate could have every one of its available
//! actions collapse onto that single dead address and be left with no
//! executable path at all.
//!
//! The engine does not hand out addresses. It serves content.
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

/// Discriminator subject kinds.
pub const SUBJECT_FILTERED_ARTIFACT: &str = "filtered_artifact";
pub const SUBJECT_AVAILABLE_TYPE: &str = "available_type";
pub const SUBJECT_AVAILABLE_ACTION: &str = "available_action";

/// No action exists for this role on this subject.
///
/// This is a positive assertion, not an absence. It is deliberately not the
/// empty string: an empty string is indistinguishable from an unset field once
/// serialized, which would make a wire bug read exactly like a correct terminal
/// answer — the same species of invisibility as the unchecked skill address
/// this constant replaced.
const NONE: &str = "none";
const ENGINE: &str = "engine";

const TRACK_DOER_RESUME_STATES: &[&str] = &[
    // `spec` is engine-resumable: an adopted-at-initial-state track (or any
    // freshly-created spec track) resumes through the engine's doer path
    // (Finding 5).
    "spec",
    "plan",
    "implementing",
    "reflecting",
    "spec_revision",
    "plan_revision",
    "impl_revision",
    "reflection_revision",
];

/// The review-gate states of the track machine — those carrying
/// `is_review_gate: true` in `playbooks/track_lifecycle/machine.yaml`.
///
/// Every one of these declares a reviewer hook in `hooks_by_role`, and every
/// one of those hook files ships. The engine has always held this content; it
/// simply declined to admit it and named a skill instead.
///
/// This list duplicates a fact the machine already states. That duplication is
/// deliberate and confined: `compute_execution_route` holds no registry and so
/// cannot read `is_review_gate` at all. The registry-aware path below derives
/// the same fact from the machine rather than trusting this list, and a parity
/// test holds the two in agreement.
const TRACK_REVIEW_GATE_STATES: &[&str] = &[
    "spec_review",
    "plan_review",
    "impl_phase_review",
    "impl_review",
    "reflection_review",
    "amend_review",
];

/// Whether `state` is a review gate of the track machine.
fn is_track_review_gate(state: &str) -> bool {
    TRACK_REVIEW_GATE_STATES.contains(&state)
}

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
        _ => NONE.to_string(),
    }
}

fn filtered_artifact_playbook(artifact_kind: &str, state: &str, role: &str) -> String {
    match (artifact_kind, state, role) {
        // A reviewer at a review gate is engine-driven: the engine serves the
        // gate's `hooks_by_role` reviewer content directly, and
        // `track_review_files_for_state` resolves the review documents for all
        // six gates.
        //
        // Deliberately NOT role-agnostic. A resumer (doer) at a review gate
        // falls through to `none`, because `begin` genuinely refuses that
        // combination — the ball is in the reviewer's court and there is no
        // doer action pending. Claiming `engine` here would re-commit the exact
        // defect this module was cleaned up to remove: a discriminator
        // promising an execution path that the executing call then refuses.
        // The executable path at a gate is carried by `available_actions`,
        // every one of which is engine-executed (see below).
        ("track", state, "reviewer") if is_track_review_gate(state) => ENGINE.to_string(),
        // Doer re-entry on a `spec_revision` track (begin(identifier) delivers
        // revision context). `creator` is the wire-format value for a doer
        // begin session (R4.1).
        ("track", "spec_revision", "creator") => ENGINE.to_string(),
        // Reviewer entry on a `spec_revision` track: the engine returns the
        // spec_not_ready_for_review rejection directly, so it is engine-handled.
        ("track", "spec_revision", "reviewer") => ENGINE.to_string(),
        ("proposal", _, _) => ENGINE.to_string(),
        ("track", state, "resumer") if TRACK_DOER_RESUME_STATES.contains(&state) => {
            ENGINE.to_string()
        }
        // playbook kind: all states are engine-handled per R4.3 (compiled-in
        // seed, engine drives transitions via `snapshot`).
        ("workflow", _, _) => ENGINE.to_string(),
        ("milestone", _, _) => ENGINE.to_string(),
        ("decision", _, _) => ENGINE.to_string(),
        ("initiative", _, _) => ENGINE.to_string(),
        ("learning", _, _) => ENGINE.to_string(),
        // Terminal track states (completed / abandoned / superseded) and any
        // unrecognized kind land here. There is genuinely no action to take.
        _ => NONE.to_string(),
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
        _ => NONE.to_string(),
    }
}

fn available_action_playbook(
    artifact_kind: &str,
    state: &str,
    action_required_role: &str,
) -> String {
    match (artifact_kind, state, action_required_role) {
        // Every action out of a review gate is engine-executed, whatever role
        // the edge requires.
        //
        // This arm is the fix for the defect that motivated removing the
        // fallback scheme. A review gate's outgoing edges carry the
        // DESTINATION doer role, not `reviewer` — `plan_review` declares three
        // edges requiring `plan`, `implement` and `doer`. The arms below match
        // on the doer STATE (`state == "plan"`, `state == "implementing"`), so
        // none of them matched a track sitting in `plan_review`, and all three
        // of its actions fell to the default. The track was told, three times,
        // to go invoke a skill that did not exist.
        ("track", state, _) if is_track_review_gate(state) => ENGINE.to_string(),
        // Spec-phase complete — doer-side (spec → spec_review).
        ("track", "spec", "spec") => ENGINE.to_string(),
        // The doer's complete (spec_revision → spec_review); describe surfaces
        // it with required_role "spec" (the seed edge role).
        ("track", "spec_revision", "spec") => ENGINE.to_string(),
        ("track", state, "plan") if state == "plan" => ENGINE.to_string(),
        ("track", state, "implement") if state == "implementing" => ENGINE.to_string(),
        ("track", state, "reflect") if state == "reflecting" => ENGINE.to_string(),
        ("track", state, "reviewer") if state == "plan_revision" => ENGINE.to_string(),
        ("track", state, "reviewer") if state == "impl_revision" => ENGINE.to_string(),
        ("track", state, "reviewer") if state == "reflection_revision" => ENGINE.to_string(),
        _ => NONE.to_string(),
    }
}

/// Registry-aware discriminator for the DESCRIBE seam (AC7). Identical to
/// `compute_execution_route` for the legacy literal-handled kinds
/// (track / proposal / playbook / free kinds); for an `available_action` on a
/// domain machine kind that the engine now drives, it reports `"engine"`.
/// Catalog and checkin retain the registry-free `compute_execution_route`
/// (they hold no registry); this variant is only wired on the describe path,
/// which already holds a registry.
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
                    NONE.to_string()
                };
            }
        }
    }
    compute_execution_route(subject_kind, artifact_kind, state, role)
}
