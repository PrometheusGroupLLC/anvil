use crate::domain::begin_adoption::{
    begin_adoption_warning, creating_actor_is, has_open_begin, is_driven_register,
};
use crate::domain::complete_events::CompleteEvent;
use crate::domain::playbook::interpreter::outgoing_transitions;
use crate::domain::playbook::registry::PlaybookRegistry;
use crate::domain::playbook::types::PlaybookMachine;
use crate::domain::shared_types::ActorIdentity;
use crate::ports::query_port::{QueryError, QueryPort};
use std::fmt;

/// The outgoing edge the unified selector chose for a complete call.
struct SelectedEdge {
    to_state: String,
    required_role: String,
    #[allow(dead_code)]
    requires_approver: bool,
}

/// Is this role the reviewer axis? A `required_role` is a reviewer role iff it
/// is literally `"reviewer"` (Q-F). Every working/doer edge in both the track
/// and knowledge machines uses a non-`"reviewer"` role; the lone wrinkle
/// (`validation_review → published` with `required_role: publish`) is
/// satisfaction-discriminated, so selection never gates on this for the reviewer
/// path — role classification is used ONLY by the two track-gated guards.
fn is_reviewer_role(role: &str) -> bool {
    role == "reviewer"
}

/// True when `state_name` is a declared-terminal state of the machine
/// (the universal lifecycle exits `abandoned`/`superseded`, or any state
/// marked `is_terminal`). Used to keep terminal exits OUT of the doer-complete
/// forward-candidate set: a `complete` means "I finished this pass, advance me
/// FORWARD" — it must never resolve to a lifecycle-exit edge (those are taken
/// explicitly via `snapshot`). Absent state → not terminal (defensive).
fn is_terminal_state(machine: &PlaybookMachine, state_name: &str) -> bool {
    machine
        .states
        .iter()
        .find(|s| s.name == state_name)
        .map(|s| s.is_terminal)
        .unwrap_or(false)
}

/// The unified edge-selector (Option A). Given the resolved machine, the
/// from_state, and the supplied satisfaction (empty = doer call, non-empty =
/// reviewer call), select the single outgoing edge to drive. `is_track` gates
/// the two compatibility guards so the track encoding behaves byte-identically
/// while domain machines are driven purely by their declared satisfaction sets.
fn select_edge(
    machine: &PlaybookMachine,
    from_state: &str,
    satisfaction: &str,
    is_track: bool,
) -> Result<SelectedEdge, CompleteError> {
    let outs = outgoing_transitions(machine, from_state);

    if satisfaction.is_empty() {
        // ---- Doer call ----
        // Guard 2 (M2): preserve the track's existing doer rejection from a
        // review state. For the track encoding, doer-complete from `spec_review`
        // returns the SAME message as the legacy complete.rs:279.
        if is_track && from_state == "spec_review" {
            return Err(CompleteError::WrongStateForComplete {
                current_state: from_state.to_string(),
                valid_actions: "On a 'spec_review' track, the valid complete actions are reviewer-style with satisfaction: \"satisfied\" or satisfaction: \"full_revision\".".to_string(),
            });
        }
        // Candidate set: None-satisfaction, non-reviewer-role edges.
        let candidates: Vec<&_> = outs
            .iter()
            .filter(|e| e.required_satisfaction.is_none() && !is_reviewer_role(&e.required_role))
            .collect();
        // When more than one candidate exists, the ambiguity is almost always
        // the universal lifecycle exits (abandoned/superseded) sharing the
        // doer's None-satisfaction author edges with the single FORWARD edge.
        // A `complete` advances forward, never to a terminal exit — so drop
        // terminal-target edges and re-check for a unique forward candidate.
        // Single-candidate states are unaffected (byte-identical behavior),
        // including the rare state whose sole forward edge is itself terminal.
        let forward: Vec<&_> = if candidates.len() > 1 {
            candidates
                .iter()
                .copied()
                .filter(|e| !is_terminal_state(machine, &e.to_state))
                .collect()
        } else {
            candidates
        };
        match forward.as_slice() {
            [edge] => Ok(SelectedEdge {
                to_state: edge.to_state.clone(),
                required_role: edge.required_role.clone(),
                requires_approver: edge.requires_approver,
            }),
            _ => Err(wrong_state_doer(from_state, is_track)),
        }
    } else {
        // ---- Reviewer call ---- (satisfaction already validated by caller)
        // Primary: edges whose required_satisfaction set contains S.
        let by_satisfaction: Vec<&_> = outs
            .iter()
            .filter(|e| {
                e.required_satisfaction
                    .as_ref()
                    .map(|set| set.iter().any(|v| v == satisfaction))
                    .unwrap_or(false)
            })
            .collect();
        if let [edge] = by_satisfaction.as_slice() {
            return Ok(SelectedEdge {
                to_state: edge.to_state.clone(),
                required_role: edge.required_role.clone(),
                requires_approver: edge.requires_approver,
            });
        }
        // Guard 1 (M1): track satisfied-compat fallback. When no
        // satisfaction-set edge matched AND S == "satisfied" AND this is the
        // track encoding, select the unique None-satisfaction reviewer-role
        // edge (`spec_review → plan`). The is_track gate stops "satisfied" from
        // firing a None-satisfaction reviewer edge on a domain machine.
        if is_track && satisfaction == "satisfied" {
            let reviewer_none: Vec<&_> = outs
                .iter()
                .filter(|e| e.required_satisfaction.is_none() && is_reviewer_role(&e.required_role))
                .collect();
            if let [edge] = reviewer_none.as_slice() {
                return Ok(SelectedEdge {
                    to_state: edge.to_state.clone(),
                    required_role: edge.required_role.clone(),
                    requires_approver: edge.requires_approver,
                });
            }
        }
        Err(wrong_state_reviewer(from_state, is_track))
    }
}

/// The doer-call no-candidate error. Preserve the track's existing message for
/// `(track, spec)`-style no-op; a generic message names the from_state for
/// domain machines.
fn wrong_state_doer(from_state: &str, is_track: bool) -> CompleteError {
    if is_track {
        CompleteError::WrongStateForComplete {
            current_state: from_state.to_string(),
            valid_actions: "No engine-supported complete action exists from this state in Slice A."
                .to_string(),
        }
    } else {
        CompleteError::WrongStateForComplete {
            current_state: from_state.to_string(),
            valid_actions: format!(
                "No machine-declared doer-complete edge exists from state '{}'.",
                from_state
            ),
        }
    }
}

/// The reviewer-call no-match error.
fn wrong_state_reviewer(from_state: &str, is_track: bool) -> CompleteError {
    if is_track {
        // Preserve the legacy track reviewer messages byte-identically.
        // From `spec` (a working state) a reviewer must wait for the doer's
        // complete first; from any other non-`spec_review` track state, the
        // generic Slice-A message.
        if from_state == "spec" {
            return CompleteError::WrongStateForComplete {
                current_state: from_state.to_string(),
                valid_actions: "On a 'spec' track, the valid complete action is doer-style (no satisfaction). The reviewer must wait for the doer's complete call before entering as reviewer.".to_string(),
            };
        }
        if from_state == "spec_revision" {
            // R6.3: a reviewer-style complete (e.g. satisfaction:"full_revision")
            // on a track already in revision. The valid action here is the doer's
            // complete with no satisfaction (advances spec_revision → spec_review).
            return CompleteError::WrongStateForComplete {
                current_state: from_state.to_string(),
                valid_actions: "On a 'spec_revision' track, the valid complete action is doer-style (no satisfaction). Wait for the doer's complete call to advance revision back to spec_review.".to_string(),
            };
        }
        CompleteError::WrongStateForComplete {
            current_state: from_state.to_string(),
            valid_actions:
                "No engine-supported reviewer-complete action exists from this state in Slice A."
                    .to_string(),
        }
    } else {
        CompleteError::WrongStateForComplete {
            current_state: from_state.to_string(),
            valid_actions: format!(
                "No machine-declared reviewer-complete edge matched from state '{}'.",
                from_state
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompleteError {
    ArtifactPathRequired,
    ActorNameRequired,
    ActorParamsRequired {
        field: String,
    },
    WrongStateForComplete {
        current_state: String,
        valid_actions: String,
    },
    SatisfactionOutOfScope {
        value: String,
    },
    /// `complete(satisfaction: "address_in_next_step")` was called with an empty
    /// or absent `findings` field. Carry-forward acceptance must carry the
    /// reviewer's verbatim observations (Slice C, R1.2 / R5.2). Raised before
    /// any state read or mutation.
    FindingsRequiredForAddressInNextStep,
    SatisfactionUnknown {
        value: String,
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
    ReflectionWriteFailed {
        path: String,
        io_error: String,
    },
}

impl fmt::Display for CompleteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompleteError::ArtifactPathRequired => write!(f, "artifact_path_required: artifact_path must not be empty"),
            CompleteError::ActorNameRequired => write!(f, "actor_name_required: actor_name must not be empty"),
            CompleteError::ActorParamsRequired { field } => write!(f, "actor_params_required: {} must not be empty", field),
            CompleteError::WrongStateForComplete { current_state, valid_actions } => write!(f, "wrong_state_for_complete: complete is not valid from state '{}'. {}", current_state, valid_actions),
            CompleteError::SatisfactionOutOfScope { value } => write!(f, "satisfaction_out_of_scope: satisfaction '{}' is not a supported reviewer satisfaction. Slice A shipped `satisfied`; Slice B shipped `full_revision`; Slice C shipped `address_in_next_step`. Currently-supported values: [\"\", \"satisfied\", \"full_revision\", \"address_in_next_step\"].", value),
            CompleteError::FindingsRequiredForAddressInNextStep => write!(f, "findings_required_for_address_in_next_step: the `findings` field must be a non-empty string when satisfaction is \"address_in_next_step\" — the reviewer's carry-forward observations cannot be empty."),
            CompleteError::SatisfactionUnknown { value } => write!(f, "satisfaction_unknown: '{}' is not a recognized satisfaction value. Known values: \"\", \"satisfied\", \"full_revision\", \"address_in_next_step\".", value),
            CompleteError::NotFound { artifact_path } => write!(f, "Artifact '{}' not found in hearth", artifact_path),
            CompleteError::MalformedStatus { artifact_path, message } => write!(f, "Malformed status.yaml for '{}': {}", artifact_path, message),
            CompleteError::IoError { message } => write!(f, "I/O error: {}", message),
            CompleteError::ReflectionWriteFailed { path, io_error } => write!(f, "reflection_write_failed: failed to write reflection file '{}': {}", path, io_error),
        }
    }
}

impl std::error::Error for CompleteError {}

#[derive(Debug, Clone, Default)]
pub struct CompleteRequest {
    pub artifact_path: String,
    pub actor_name: String,
    pub actor_type: String,
    pub actor_model: String,
    pub actor_provider: String,
    pub actor_context_window: i64,
    pub actor_sdk_version: String,
    pub actor_entrypoint: String,
    pub satisfaction: String,
    pub approver: String,
    pub note: String,
    pub at: String,
    pub reflection_notes: String,
    /// Verbatim reviewer findings carried forward to the next phase. Required
    /// (non-empty) when `satisfaction == "address_in_next_step"`; ignored for
    /// every other satisfaction value (Slice C, R1.1 / R1.3).
    pub findings: String,
    /// Ordered evidence claims supplied for this lifecycle completion.
    /// Empty for legacy callers.
    pub claimed_evidence: Vec<crate::domain::shared_types::ClaimedEvidence>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompleteResult {
    pub new_state: String,
    /// Required role of the exact machine edge selected by this complete call.
    /// Kept distinct from the legacy persisted transition-role spelling so
    /// emitters can resolve the selected step without re-running edge choice.
    pub selected_required_role: String,
    pub transition_at: String,
    pub artifact_path: String,
    pub warnings: Vec<String>,
    pub reflection_path: String,
    /// Absolute path to the `carry-forward.md` file written on the
    /// `complete(satisfaction: "address_in_next_step")` success path (Slice C).
    /// Empty string on every other path. Populated by the engine/event-router
    /// after routing the `CarryForwardWritten` event.
    pub carry_forward_path: String,
    /// The state the artifact was in before the transition (pre-transition state).
    /// Populated from the `state` read in execute_doer / execute_reviewer.
    /// Used by the engine emit layer (P3) to resolve the measurement spec.
    pub from_state: String,
    /// The artifact kind (e.g. "track"). Populated from the `kind` read in
    /// execute_doer / execute_reviewer. Used by the engine emit layer (P3)
    /// to resolve the playbook_id without a post-routing re-read.
    pub kind: String,
}

#[derive(Debug, Clone)]
pub struct CompleteOutcome {
    pub result: CompleteResult,
    pub events: Vec<CompleteEvent>,
}

pub struct CompleteCommandHandler;

pub fn compute_reflection_filename(at: &str, actor_name: &str) -> String {
    let compact = at.replace(['-', ':', ' '], "");
    format!("{}-{}.md", compact, actor_name)
}

pub fn render_reflection_body(
    source_state: &str,
    actor_name: &str,
    at: &str,
    satisfaction: Option<&str>,
    notes: &str,
) -> String {
    let trimmed = notes.trim();
    let mut result = String::new();
    result.push_str("---\n");
    result.push_str(&format!("source_state: {}\n", source_state));
    result.push_str(&format!("actor: {}\n", actor_name));
    result.push_str(&format!("at: {}\n", at));
    if let Some(sat) = satisfaction {
        result.push_str(&format!("satisfaction: {}\n", sat));
    }
    result.push_str("---\n");
    result.push('\n');
    result.push_str(trimmed);
    if !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

impl CompleteCommandHandler {
    pub fn execute(
        query: &dyn QueryPort,
        registry: &dyn PlaybookRegistry,
        request: CompleteRequest,
    ) -> Result<CompleteOutcome, CompleteError> {
        if request.artifact_path.is_empty() {
            return Err(CompleteError::ArtifactPathRequired);
        }
        if request.actor_name.is_empty() {
            return Err(CompleteError::ActorNameRequired);
        }
        for (field, value) in [
            ("actor_type", &request.actor_type),
            ("actor_model", &request.actor_model),
            ("actor_provider", &request.actor_provider),
        ] {
            if value.is_empty() {
                return Err(CompleteError::ActorParamsRequired {
                    field: field.to_string(),
                });
            }
        }

        // Read kind + state up front; resolve the machine. The satisfaction
        // whitelist is now applied AFTER the kind read so it can be
        // kind-conditional (track keeps the frozen "" | "satisfied" contract;
        // domain machines validate against their own satisfaction vocabulary).
        let kind = query
            .read_artifact_kind(&request.artifact_path)
            .map_err(map_query_error)?;
        let state = query
            .read_artifact_state(&request.artifact_path)
            .map_err(map_query_error)?;
        // K8 bypass closure (plan Task 6): `Complete` is NOT a backlog writer.
        // Rejected here — before reflection, carry-forward, actor, or transition
        // events — so no side effect precedes the refusal. The store's
        // `append_transition` rejection is defense in depth, not the only guard.
        if kind == "backlog_item" {
            return Err(CompleteError::WrongStateForComplete {
                current_state: state.clone(),
                valid_actions: "A backlog_item never completes: post-genesis K8 state moves \
                                only through a prepared K8 transition."
                    .to_string(),
            });
        }
        let machine =
            registry
                .machine_for(&kind)
                .ok_or_else(|| CompleteError::WrongStateForComplete {
                    current_state: state.clone(),
                    valid_actions: format!("No playbook machine resolves for kind '{}'.", kind),
                })?;
        let is_track = machine.kind == "track";

        // Kind-conditional satisfaction validation.
        let satisfaction = request.satisfaction.as_str();
        validate_satisfaction(satisfaction, machine, is_track)?;

        // Slice C: `address_in_next_step` REQUIRES non-empty `findings`. The
        // findings carry the reviewer's verbatim carry-forward observations; an
        // empty value is rejected before any transition or file write (R5.2).
        if satisfaction == "address_in_next_step" && request.findings.is_empty() {
            return Err(CompleteError::FindingsRequiredForAddressInNextStep);
        }

        // Unified edge selection (Option A) — doer vs reviewer is decided by the
        // CALL (empty vs non-empty satisfaction), never by the edge's role.
        let edge = select_edge(machine, &state, satisfaction, is_track)?;

        // [BP8] A kind counts as driven for the begin-adoption soft-warn if it is
        // in the legacy driven register OR it resolves to a machine (it is now
        // engine-driven). We reached here only because the machine resolved, so
        // the kind is driven — but keep the legacy register in the predicate for
        // intent clarity.
        let driven = is_driven_register(&kind) || registry.machine_for(&kind).is_some();

        Self::execute_selected(query, request, kind, state, edge, driven)
    }

    /// Build the complete outcome from the selected edge. Shared by the doer and
    /// reviewer paths — the only differences (the transition role, whether the
    /// reflection carries a satisfaction, the registry-driven warnings state) are
    /// derived from the selected edge / the call.
    fn execute_selected(
        query: &dyn QueryPort,
        request: CompleteRequest,
        kind: String,
        state: String,
        edge: SelectedEdge,
        driven: bool,
    ) -> Result<CompleteOutcome, CompleteError> {
        let satisfaction = request.satisfaction.as_str();
        let is_reviewer_call = !satisfaction.is_empty();
        let at = request.at.clone();
        let identity = ActorIdentity {
            name: request.actor_name.clone(),
            actor_type: request.actor_type.clone(),
            model: request.actor_model.clone(),
            provider: request.actor_provider.clone(),
            context_window: request.actor_context_window,
            sdk_version: request.actor_sdk_version.clone(),
            entrypoint: request.actor_entrypoint.clone(),
            registered_at: at.clone(),
        };
        let mut events: Vec<CompleteEvent> = Vec::new();
        events.push(CompleteEvent::ActorUpserted {
            artifact_path: request.artifact_path.clone(),
            identity,
        });

        // Source state for the reflection is the machine-derived from_state
        // (drop the hardcoded "spec"/"spec_review" literals).
        let source_state = state.as_str();
        if !request.reflection_notes.trim().is_empty() {
            let filename = compute_reflection_filename(&at, &request.actor_name);
            let reflection_satisfaction = if is_reviewer_call {
                Some(request.satisfaction.as_str())
            } else {
                None
            };
            let body = render_reflection_body(
                source_state,
                &request.actor_name,
                &at,
                reflection_satisfaction,
                &request.reflection_notes,
            );
            events.push(CompleteEvent::ReflectionWritten {
                artifact_path: request.artifact_path.clone(),
                source_state: source_state.to_string(),
                filename,
                body,
            });
        }

        // Track reviewer advances keep the legacy recorded role "review".
        // Track doer advances record the selected edge role so plan/implement/
        // reflect completions preserve the machine's phase role.
        let transition_role = if kind == "track" {
            if is_reviewer_call {
                "review".to_string()
            } else {
                edge.required_role.clone()
            }
        } else {
            edge.required_role.clone()
        };
        // The reviewer call carries the approver (when supplied); the doer call
        // records no approver (preserving the legacy doer behavior).
        let approver = if is_reviewer_call && !request.approver.is_empty() {
            Some(request.approver.clone())
        } else {
            None
        };

        // Slice C carry-forward path: `complete(satisfaction: "address_in_next_step")`.
        // Emit the CarryForwardWritten event (carry-forward.md is a primary
        // artifact, written before the status/registry/projection transition)
        // and record `satisfaction: address_in_next_step` on the transition
        // metadata so the audit trail distinguishes carry-forward from outright
        // acceptance (R2.2 / R3). For every other path, no carry-forward event
        // and `satisfaction: None` (the YAML emitter omits the line).
        let is_carry_forward = satisfaction == "address_in_next_step";
        let transition_satisfaction = if is_carry_forward {
            // carry-forward.md is written before the transition (R2.4).
            let body = render_carry_forward(&request.actor_name, &at, &request.findings);
            events.push(CompleteEvent::CarryForwardWritten {
                artifact_path: request.artifact_path.clone(),
                body,
            });
            Some("address_in_next_step".to_string())
        } else {
            None
        };

        events.push(CompleteEvent::TransitionRecorded {
            artifact_path: request.artifact_path.clone(),
            to_state: edge.to_state.clone(),
            at: at.clone(),
            role: transition_role,
            approver,
            note: if request.note.is_empty() {
                None
            } else {
                Some(request.note.clone())
            },
            actor_name: request.actor_name.clone(),
            satisfaction: transition_satisfaction,
        });

        // Detection reads pre-dispatch QueryPort state: the TransitionRecorded
        // event is pushed above but not yet written to disk, so an actor WITH a
        // matching open marker correctly reads as open (warn-before-write).
        let warnings = detect_begin_adoption_warning(
            query,
            driven,
            &request.artifact_path,
            &request.actor_name,
            &state,
        )?
        .into_iter()
        .collect();

        Ok(CompleteOutcome {
            result: CompleteResult {
                new_state: edge.to_state,
                selected_required_role: edge.required_role,
                transition_at: at,
                artifact_path: request.artifact_path.clone(),
                warnings,
                reflection_path: String::new(),
                carry_forward_path: String::new(),
                from_state: state,
                kind,
            },
            events,
        })
    }
}

/// Render the Slice C `carry-forward.md` body per the locked file format:
///
/// ```text
/// ---
/// findings_from: spec_review
/// satisfied_by: <reviewer>
/// at: <iso8601>
/// satisfaction: address_in_next_step
/// ---
///
/// # Carry-forward from spec review
///
/// _Reviewer: <reviewer> · <iso8601>_
///
/// <verbatim findings text>
/// ```
///
/// Verbatim preservation: `findings` bytes are emitted unchanged after the blank
/// line following the reviewer-signature header — no trimming, no normalization,
/// no trailing-newline addition. The single `at` value feeds both the
/// frontmatter `at:` field and the header signature line, so the file and the
/// status.yaml transition agree on when `complete` fired.
fn render_carry_forward(reviewer: &str, at: &str, findings: &str) -> String {
    let mut s = String::new();
    s.push_str("---\n");
    s.push_str("findings_from: spec_review\n");
    s.push_str(&format!("satisfied_by: {}\n", reviewer));
    s.push_str(&format!("at: {}\n", at));
    s.push_str("satisfaction: address_in_next_step\n");
    s.push_str("---\n");
    s.push('\n');
    s.push_str("# Carry-forward from spec review\n");
    s.push('\n');
    s.push_str(&format!("_Reviewer: {} · {}_\n", reviewer, at));
    s.push('\n');
    s.push_str(findings);
    s
}

/// Kind-conditional satisfaction validation. The track encoding keeps the
/// EXACT frozen whitelist + error contract; non-track machines validate the
/// supplied value against the UNION of `required_satisfaction` across all the
/// machine's transitions (empty is always valid — the doer call).
fn validate_satisfaction(
    satisfaction: &str,
    machine: &PlaybookMachine,
    is_track: bool,
) -> Result<(), CompleteError> {
    if is_track {
        return match satisfaction {
            // Slice B lifts the `full_revision` scope guard (spec_review →
            // spec_revision). Slice C lifts the `address_in_next_step` scope
            // guard (spec_review → plan, carry-forward). No track satisfaction
            // is "future" after Slice C.
            "" | "satisfied" | "full_revision" | "address_in_next_step" => Ok(()),
            other => Err(CompleteError::SatisfactionUnknown {
                value: other.to_string(),
            }),
        };
    }
    // Non-track: the empty satisfaction is the doer call (always valid). A
    // non-empty value must appear in the machine's satisfaction vocabulary.
    if satisfaction.is_empty() {
        return Ok(());
    }
    let in_vocabulary = machine.transitions.iter().any(|t| {
        t.required_satisfaction
            .as_ref()
            .map(|set| set.iter().any(|v| v == satisfaction))
            .unwrap_or(false)
    });
    if in_vocabulary {
        Ok(())
    } else {
        Err(CompleteError::SatisfactionUnknown {
            value: satisfaction.to_string(),
        })
    }
}

/// Begin-adoption soft-warn detection (BP2). A pure read over `activity:`
/// + `transitions:`: if the (actor, artifact, current_state) key has no
/// OPEN begin-marker AND the actor is not the creating actor AND the kind
/// is in the driven register, return the pinned warning. The detection is
/// non-blocking — the transition still records.
///
/// Runs warn-before-write: the closing transition isn't appended yet at
/// detection time, so an actor WITH an open marker correctly reads open.
fn detect_begin_adoption_warning(
    query: &dyn QueryPort,
    driven: bool,
    artifact_path: &str,
    actor: &str,
    current_state: &str,
) -> Result<Option<String>, CompleteError> {
    if !driven {
        return Ok(None);
    }
    // Fold the per-file transition event store (creation event + every
    // subsequent event), not just the legacy array — otherwise the
    // creating-actor exemption and open-begin comparison miss event-sourced
    // history.
    let transitions = query
        .read_transitions(artifact_path)
        .map_err(map_query_error)?;
    let activity = query
        .read_activity_entries(artifact_path)
        .map_err(map_query_error)?;
    if has_open_begin(&activity, &transitions, actor, current_state)
        || creating_actor_is(&transitions, actor)
    {
        Ok(None)
    } else {
        Ok(Some(begin_adoption_warning(
            actor,
            artifact_path,
            current_state,
        )))
    }
}

fn map_query_error(e: QueryError) -> CompleteError {
    match e {
        QueryError::NotFound { artifact_id } => CompleteError::NotFound {
            artifact_path: artifact_id,
        },
        QueryError::MalformedStatus {
            artifact_id,
            message,
        } => CompleteError::MalformedStatus {
            artifact_path: artifact_id,
            message,
        },
        QueryError::IoError { message } => CompleteError::IoError { message },
        QueryError::ProjectionRowNotFound { message } => CompleteError::IoError { message },
        QueryError::ProjectionRowAmbiguous { message } => CompleteError::IoError { message },
        // The complete path never issues a strict adoption read, so this cannot
        // arise here; surface it as an I/O failure with the detail preserved to
        // keep the match exhaustive.
        QueryError::AdoptionEvidenceUnreadable {
            artifact_id,
            detail,
        } => CompleteError::IoError {
            message: format!("adoption evidence unreadable for '{}': {}", artifact_id, detail),
        },
    }
}

#[cfg(test)]
mod doer_complete_terminal_exclusion_tests {
    use super::*;

    /// A machine shaped like kit_generation's `intent` doer state: one FORWARD
    /// None-satisfaction author edge (intent -> intent_review) plus the two
    /// universal lifecycle exits (abandoned/superseded, also author/None). This
    /// is the 2026-07-07 misfit: three None-sat author candidates made
    /// doer-`complete` ambiguous and it fell through to "No machine-declared
    /// doer-complete edge exists", forcing snapshot-only operation.
    fn intent_shaped_machine() -> PlaybookMachine {
        let yaml = r#"
kind: kit_generation
directory: kit_generations
registry: kit_generations.md
description: test
roles: [author, reviewer]
states:
  - {name: intent, registry_section: "", is_review_gate: false, is_terminal: false}
  - {name: intent_review, registry_section: "", is_review_gate: true, is_terminal: false}
  - {name: abandoned, registry_section: "", is_review_gate: false, is_terminal: true}
  - {name: superseded, registry_section: "", is_review_gate: false, is_terminal: true}
transitions:
  - {from_state: intent, to_state: intent_review, required_role: author, requires_approver: false}
  - {from_state: intent, to_state: abandoned, required_role: author, requires_approver: false}
  - {from_state: intent, to_state: superseded, required_role: author, requires_approver: false}
"#;
        serde_yaml::from_str(yaml).expect("test machine parses")
    }

    #[test]
    fn doer_complete_advances_forward_past_lifecycle_exits() {
        let m = intent_shaped_machine();
        // Doer call (empty satisfaction) from `intent`: the two terminal exits
        // (abandoned/superseded) are excluded, leaving the unique forward edge.
        let edge = select_edge(&m, "intent", "", false).expect("forward edge selected");
        assert_eq!(edge.to_state, "intent_review");
        assert_eq!(edge.required_role, "author");
    }

    #[test]
    fn single_forward_candidate_is_unaffected() {
        // A state with exactly one None-sat author edge behaves identically
        // (the >1 exclusion path never runs) — even when that sole edge is
        // itself terminal.
        let yaml = r#"
kind: k
directory: d
registry: r.md
description: t
roles: [author]
states:
  - {name: only, registry_section: "", is_review_gate: false, is_terminal: false}
  - {name: done, registry_section: "", is_review_gate: false, is_terminal: true}
transitions:
  - {from_state: only, to_state: done, required_role: author, requires_approver: false}
"#;
        let m: PlaybookMachine = serde_yaml::from_str(yaml).unwrap();
        let edge = select_edge(&m, "only", "", false).expect("sole edge selected even if terminal");
        assert_eq!(edge.to_state, "done");
    }

    #[test]
    fn no_forward_candidate_still_errors() {
        // A doer state whose only None-sat author edges are BOTH terminal
        // exits has no forward edge — must still error (never silently pick a
        // lifecycle exit).
        let yaml = r#"
kind: k
directory: d
registry: r.md
description: t
roles: [author]
states:
  - {name: s, registry_section: "", is_review_gate: false, is_terminal: false}
  - {name: abandoned, registry_section: "", is_review_gate: false, is_terminal: true}
  - {name: superseded, registry_section: "", is_review_gate: false, is_terminal: true}
transitions:
  - {from_state: s, to_state: abandoned, required_role: author, requires_approver: false}
  - {from_state: s, to_state: superseded, required_role: author, requires_approver: false}
"#;
        let m: PlaybookMachine = serde_yaml::from_str(yaml).unwrap();
        assert!(select_edge(&m, "s", "", false).is_err());
    }
}
