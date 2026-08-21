//! Domain handler for the `begin` operation.
//!
//! ## CQRS shape
//!
//! `BeginCommandHandler::execute` is a **pure function**: it reads state
//! via `QueryPort` and returns a `BeginOutcome` containing domain
//! `Event`s. It never calls mutation methods directly. All mutations are
//! an engine-layer concern.
//!
//! ```text
//!                     ┌─────────────────────────────────┐
//!  BeginRequest ──────► BeginCommandHandler::execute     │
//!                     │  reads via QueryPort             │
//!                     │  returns BeginOutcome { events } │
//!                     └──────────────┬──────────────────┘
//!                                    │ engine routes each event:
//!                       ┌────────────┼──────────────────────┐
//!                       ▼            ▼                       ▼
//!               ReviewTransition  ReviewDocCreated     TrackCreation
//!               SnapshotCmd-       ArtifactPort::       ArtifactPort::
//!               Handler            create_review_doc    scaffold_track_
//!               (state+registry    (idempotent file     directory, then
//!               +projection)       write)               SnapshotCmdHandler
//! ```
//!
//! ### The three-port triad
//!
//! | Port | Concern | Examples |
//! |------|---------|----------|
//! | `QueryPort` | Reads only; used by the handler | `read_artifact_state`, `read_artifact_text`, `read_registry_entry` |
//! | `SnapshotPort` (via `SnapshotCommandHandler`) | State bookkeeping | Append transition, move registry entry, upsert actor, move projection row |
//! | `ArtifactPort` | Artifact-file creation | `create_review_doc`, `scaffold_track_directory` |
//!
//! ### Adding a new handler
//!
//! Follow the same shape:
//! 1. Accept `&dyn QueryPort` (plus any other read-only ports needed).
//! 2. Perform all reads up front, fail fast on validation errors.
//! 3. Collect mutations into `Vec<Event>` — one variant per distinct
//!    mutation concern.
//! 4. Return `Ok(Outcome { result, events })` — no port mutation calls
//!    inside the handler.
//! 5. Wire event routing in `anvil-engine/src/main.rs`: match on each
//!    variant and dispatch to the appropriate port or command handler.
//!
//! Mutation methods on ports called directly from a handler are an
//! anti-pattern — they bypass the engine's lock-coordination and
//! observability layer.

use crate::domain::playbook::types::EvidenceClass;
use crate::domain::checkin::{validate_artifact_type, validate_parent_state, CheckinError};
use crate::domain::events::Event;
use crate::domain::playbook::hook_serve::serve_hook_body;
use crate::domain::playbook::registry::{granted, PlaybookRegistry, PlaybookSource};
use crate::domain::shared_types::{ActorIdentity, RequestContext, StatusContent};
use crate::ports::query_port::QueryError;
use crate::ports::query_port::QueryPort;
use std::fmt;

// Re-export the shared hook-serving seam symbols so existing call-sites that
// reference them via `begin::` (hook_manifest, query_port steps, engine reader)
// keep compiling unchanged. The single definitions live in `hook_serve`.
pub use crate::domain::playbook::hook_serve::{
    cap_hook_body, PlaybookHookBodyPort, HOOK_CONTEXT_BUDGET_BYTES, HOOK_TRUNCATION_MARKER,
};

/// Errors from `begin` operations. Distinguishes the create-flow
/// validation errors (wrapped from `CheckinError`) from the review-flow
/// taxonomy that names a fallback skill where applicable. The engine
/// layer maps each variant to a structured gRPC `Status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BeginError {
    /// `begin(identifier)` called without a prior `checkin` — no session
    /// role available.
    SessionRequired,
    /// The current session role is recognized but its identifier-mode is
    /// not yet implemented in the engine.
    ModeNotImplemented {
        mode: String,
        /// Whether ADOPTION is a genuine remedy for THIS artifact — computed at
        /// construction (a registered machine exists for the kind AND the
        /// artifact would pass the governed check). Only then does the message
        /// advertise `adopt: true`; otherwise the remedy would be bait that the
        /// adoption path immediately rejects (Finding 4).
        adoptable: bool,
    },
    /// The session role cannot perform any identifier-mode action. E.g.,
    /// creator with an identifier — creator operates on artifact types.
    RoleStateMismatch {
        role: String,
        artifact_kind: String,
        state: String,
    },
    /// The (artifact_kind, state, role) combination is reviewable in
    /// principle but not yet engine-supported.
    StateNotReviewable {
        artifact_kind: String,
        state: String,
        /// Whether ADOPTION is a genuine remedy for THIS artifact — see
        /// `ModeNotImplemented::adoptable` (Finding 4).
        adoptable: bool,
    },
    /// Both `artifact_type` and `identifier` set, both empty, or other
    /// shape-level argument errors.
    InvalidArgument { reason: String },
    /// The named identifier doesn't exist in the hearth.
    NotFound { identifier: String },
    /// I/O failure while reading or writing hearth state.
    IoError { message: String },
    /// A status.yaml file is malformed.
    MalformedStatus {
        artifact_id: String,
        message: String,
    },
    /// Create-flow validation error (unsupported artifact type, parent
    /// not active, etc.). Phase 4 leaves create-flow error messaging
    /// unchanged.
    Checkin(CheckinError),
    /// Routed begin selected one kind but attempted to begin a different
    /// artifact type. Rejected before any create event is emitted.
    RoutedSelectionMismatch {
        selected: String,
        artifact_type: String,
    },
    /// Routed begin attempted to create a kind that is not a driven playbook
    /// candidate (`register: free` or otherwise not driven).
    NotDrivenCandidate { kind: String },
    /// The caller's access context is not granted for the requested playbook
    /// machine. Carries `access_denied` in its Display for feature assertions.
    AccessDenied { kind: String },
    /// `actor_name` was absent or empty. The caller must supply a
    /// non-empty actor identity on every begin call (spec R2/R3 of the
    /// checkin_backfill_spec_context track).
    ActorNameRequired,
    /// One of the required runtime identity params (`actor_type`,
    /// `actor_model`, `actor_provider`) was empty. Identifies which
    /// field is missing for diagnostics.
    ActorParamsRequired { field: String },
    /// `begin(identifier, reviewer)` called on a track that is not yet
    /// submitted for review. Two cases share this code:
    /// - `spec`: the doer's `complete` has not yet advanced the track to
    ///   `spec_review` (Slice A cutover).
    /// - `spec_revision`: the track is in revision and the doer's `complete`
    ///   has not yet advanced it back to `spec_review` (Slice B).
    /// The `state` discriminates the two for the Display message; both carry
    /// the `spec_not_ready_for_review` error-code substring for feature
    /// assertion.
    SpecNotReadyForReview { state: String },
    /// `begin(identifier, adopt: true)` was called on an artifact that already
    /// has recorded engine transition history. Adoption resets an artifact to
    /// its machine's initial state and is therefore only valid for artifacts
    /// authored OUTSIDE the engine (no governance history); refusing here
    /// protects an in-flight, engine-governed artifact from a destructive
    /// state reset. Carries the `already_governed` error-code substring.
    AlreadyGoverned { identifier: String, state: String },
    /// `begin(identifier, adopt: true)` could not PROVE the artifact is
    /// pre-governance because an evidence channel is damaged (e.g. a degraded
    /// `activity:` log whose dropped entries might have included a begin
    /// marker). Adoption fails CLOSED here — refusing rather than risk resetting
    /// an artifact that is actually governed. Carries the
    /// `adoption_evidence_unreadable` error-code substring and names the anomaly.
    AdoptionEvidenceUnreadable { identifier: String, detail: String },
}

impl fmt::Display for BeginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BeginError::SessionRequired => write!(
                f,
                "No active session. Call checkin first to register your identity and intent."
            ),
            BeginError::ModeNotImplemented { mode, adoptable } => {
                if *adoptable {
                    write!(
                        f,
                        "Mode '{}' on identifier is not yet engine-supported. If this artifact \
                         was authored outside the engine, call begin(identifier, adopt: true) to \
                         take it back to its initial state and drive it through every phase and \
                         review gate properly.",
                        mode
                    )
                } else {
                    write!(
                        f,
                        "Mode '{}' on identifier is not yet engine-supported.",
                        mode
                    )
                }
            }
            BeginError::RoleStateMismatch {
                role,
                artifact_kind,
                state,
            } => write!(
                f,
                "Session role '{}' cannot act on a {} in state '{}' via begin(identifier).",
                role, artifact_kind, state
            ),
            BeginError::StateNotReviewable {
                artifact_kind,
                state,
                adoptable,
            } => {
                if *adoptable {
                    write!(
                        f,
                        "({}, {}, reviewer) is not engine-supported. If this artifact was \
                         authored outside the engine, call begin(identifier, adopt: true) to take \
                         it back to its initial state and drive it through every phase and review \
                         gate properly.",
                        artifact_kind, state
                    )
                } else {
                    write!(
                        f,
                        "({}, {}, reviewer) is not engine-supported.",
                        artifact_kind, state
                    )
                }
            }
            BeginError::InvalidArgument { reason } => write!(f, "Invalid argument: {}", reason),
            BeginError::NotFound { identifier } => {
                write!(f, "Artifact '{}' not found in the hearth.", identifier)
            }
            BeginError::IoError { message } => write!(f, "I/O error: {}", message),
            BeginError::MalformedStatus {
                artifact_id,
                message,
            } => write!(
                f,
                "Malformed status.yaml for '{}': {}",
                artifact_id, message
            ),
            BeginError::Checkin(e) => write!(f, "{}", e),
            BeginError::RoutedSelectionMismatch {
                selected,
                artifact_type,
            } => write!(
                f,
                "routed selection mismatch: selected '{}' but began '{}'",
                selected, artifact_type
            ),
            BeginError::NotDrivenCandidate { kind } => {
                write!(f, "not a driven candidate: '{}'", kind)
            }
            BeginError::AccessDenied { kind } => {
                write!(f, "access_denied: begin is not granted for kind '{}'", kind)
            }
            BeginError::ActorNameRequired => {
                write!(f, "actor_name is required and must not be empty")
            }
            BeginError::ActorParamsRequired { field } => {
                write!(f, "{} is required and must not be empty", field)
            }
            BeginError::SpecNotReadyForReview { state } if state == "spec_revision" => write!(
                f,
                "spec_not_ready_for_review: the spec is in revision and not yet submitted \
                 for re-review; wait for the doer's complete call before beginning review."
            ),
            BeginError::AlreadyGoverned { identifier, state } => write!(
                f,
                "already_governed: '{}' already carries engine governance evidence (a recorded \
                 transition or a begin marker; state '{}'); adoption applies only to artifacts \
                 authored outside the engine. Resume it with a plain begin(identifier) instead.",
                identifier, state
            ),
            BeginError::AdoptionEvidenceUnreadable { identifier, detail } => write!(
                f,
                "adoption_evidence_unreadable: refusing to adopt '{}' because its governance \
                 evidence cannot be read cleanly ({}). Adoption fails closed on damaged evidence \
                 to avoid resetting an artifact that may already be governed; repair the artifact \
                 and retry.",
                identifier, detail
            ),
            BeginError::SpecNotReadyForReview { .. } => write!(
                f,
                "spec_not_ready_for_review: this track is still in `spec`. The doer must \
                 call `complete` (no satisfaction) to advance it to `spec_review` before \
                 a reviewer can enter. If you are the doer resuming work, invoke \
                 `begin(identifier)` as a resumer and the engine will serve the spec \
                 doer context."
            ),
        }
    }
}

impl std::error::Error for BeginError {}

impl From<CheckinError> for BeginError {
    fn from(e: CheckinError) -> Self {
        match e {
            CheckinError::IoError { message } => BeginError::IoError { message },
            CheckinError::MalformedStatus {
                artifact_id,
                message,
            } => BeginError::MalformedStatus {
                artifact_id,
                message,
            },
            other => BeginError::Checkin(other),
        }
    }
}

/// Request to begin work. Either creation mode (set `artifact_type` +
/// parent fields) or resume/review mode (set `identifier` + `session_role`).
/// Dispatch validates exactly one mode is selected.
#[derive(Debug, Clone, Default)]
pub struct BeginRequest {
    /// Caller access context. BP5 will consume this for begin-time enforcement.
    pub ctx: RequestContext,

    // Create-mode fields
    pub artifact_type: String,
    pub parent_id: String,
    pub track_name: String,
    /// Name field for playbook creation (parallel to `track_name` for tracks).
    /// Kept as `playbook_name` on the Rust/proto carrier for wire compatibility.
    /// Per plan §Phase2 R10.1(b) naming convention.
    pub playbook_name: String,
    /// Owner descriptor (kit id or user/Space) the created instance is destined
    /// for. Optional at the wire level; required-ness is machine-driven (a
    /// machine declaring a `target_owner` descriptor in its required_fields).
    /// Recorded verbatim on the created instance's status. (Anvil-lane 1b.)
    pub target_owner: String,
    /// Generic field bag for ANY machine-declared required field outside the
    /// builtin set (name, parent_id, approver, playbook_name and the
    /// pre-migration `workflow_name` a live machine.yaml still declares,
    /// target_owner).
    /// BTreeMap for deterministic key order. Wire-optional; required-ness is
    /// machine-driven. Declared values are recorded on the created instance's
    /// status and interpolated into the first hook's context_text.
    pub fields: std::collections::BTreeMap<String, String>,
    pub approver: String,
    pub actor_name: String,
    pub actor_type: String,
    pub actor_model: String,
    pub actor_provider: String,
    pub actor_context_window: i64,
    pub actor_sdk_version: String,
    pub actor_entrypoint: String,

    // Resume/review-mode fields
    pub identifier: String,
    pub session_role: String,

    /// Explicit opt-in to ADOPT an out-of-engine artifact. When set on an
    /// `identifier` begin, the engine takes the (pre-governance) artifact back
    /// to its machine's initial state and drives it through the full playbook.
    /// Default `false` leaves the adoption path inert (no reset, no adoption
    /// event): an on-disk artifact and a genuinely-resumable one are
    /// indistinguishable by history alone (both may lack recorded transitions),
    /// so adoption must be caller-declared, never inferred.
    pub adopt: bool,

    /// The originating conversation id (resume-aware routing). Recorded on the
    /// durable open-begin marker so a later continuation message can resolve the
    /// conversation's open playbook. Distinct from `rd_turn_id` (the
    /// routing-decision turn id). Empty when the surface omits it (back-compat —
    /// the marker then records an empty conversation_id).
    pub conversation_id: String,

    // Routing-decision measurement fields. Empty rd_selected means direct begin.
    pub rd_turn_id: String,
    pub rd_input: String,
    pub rd_candidate_set: String,
    pub rd_selected: String,
    pub rd_confidence: String,

    /// Ordered evidence claims supplied for this lifecycle state entry.
    /// Empty for legacy callers and internal non-public begin paths.
    pub claimed_evidence: Vec<crate::domain::shared_types::ClaimedEvidence>,
}

/// Result of a successful begin.
/// Create-flow responses populate `track_path`, `state`, `context_text`;
/// review-flow responses populate all fields including `artifact_text`,
/// `review_context_text`, `review_doc_path`.
#[derive(Debug, Clone, Default)]
pub struct BeginResult {
    pub track_path: String,
    pub state: String,
    pub context_text: String,
    pub artifact_text: String,
    pub review_context_text: String,
    pub review_doc_path: String,
    pub measurement_kind: String,
    pub measurement_role: String,
    pub intent: String,
    pub expected_output: String,
    pub playbook_id: String,
}

/// The output of a successful begin operation: a result payload plus a
/// list of domain events for the engine layer to route. The result fields
/// may be partially populated by the pure handler and completed by the
/// engine layer (e.g., `review_doc_path` is filled in by the engine after
/// routing `Event::ReviewDocCreated` to `ArtifactPort`).
#[derive(Debug, Clone, Default)]
pub struct BeginOutcome {
    pub result: BeginResult,
    pub events: Vec<Event>,
}

/// Command handler for the begin flow. Pure function: reads via
/// `QueryPort`, emits `BeginOutcome` with domain events. No mutation calls.
pub struct BeginCommandHandler;

struct QueryPortHookBodyReader<'a> {
    query: &'a dyn QueryPort,
}

impl PlaybookHookBodyPort for QueryPortHookBodyReader<'_> {
    fn read_playbook_hook_body(
        &self,
        source: &PlaybookSource,
        filename: &str,
    ) -> Result<String, QueryError> {
        self.query
            .read_playbook_hook_body(&source.playbook_id, filename)
    }
}

impl BeginCommandHandler {
    pub fn execute(
        query: &dyn QueryPort,
        registry: &dyn PlaybookRegistry,
        request: BeginRequest,
    ) -> Result<BeginOutcome, BeginError> {
        validate_actor_identity(&request)?;
        let hook_reader = QueryPortHookBodyReader { query };
        dispatch(query, &hook_reader, registry, request)
    }

    pub fn execute_with_hook_reader(
        query: &dyn QueryPort,
        hook_reader: &dyn PlaybookHookBodyPort,
        registry: &dyn PlaybookRegistry,
        request: BeginRequest,
    ) -> Result<BeginOutcome, BeginError> {
        validate_actor_identity(&request)?;
        dispatch(query, hook_reader, registry, request)
    }
}

/// Resolve the `(kind, state, role)` hook declaration via the registry and
/// read its body via the query port. Returns the body (budget-capped, per the
/// shared `hook_serve` seam), or an empty string when the state declares no hook
/// for that role (absence is not an error — AC-2). Used by both the create flow
/// (→ `context_text`) and the review flow (→ `review_context_text`) and the
/// checkin re-serve; only the destination field differs by call-site, so channel
/// mapping stays in the calling handler.
///
/// This is a thin `BeginError`-mapping wrapper over `hook_serve::serve_hook_body`
/// — the SAME seam `route` calls — so single-route guidance and begin's served
/// body can never drift. begin applies `interpolate_create_fields` AFTER this
/// returns; route serves the pre-interpolation body verbatim.
fn resolve_and_read_hook(
    query: &dyn QueryPort,
    hook_reader: &dyn PlaybookHookBodyPort,
    registry: &dyn PlaybookRegistry,
    kind: &str,
    state: &str,
    role: &str,
) -> Result<String, BeginError> {
    let _ = query;
    serve_hook_body(hook_reader, registry, kind, state, role).map_err(BeginError::from)
}

/// T4 — checkin re-serve. For a single artifact the checking-in actor may have
/// an open begin on, re-serve the current `(state, role)` hook content so a
/// compacted/resumed session re-warms its standing context. Returns an empty
/// string (not an error) when the actor has NO open begin on this artifact in
/// its current state, or when the resolved state declares no hook — re-serve is
/// purely additive and never blocks checkin.
///
/// Role resolution: `activity:` begin-markers record actor + state but not
/// role, so re-serve tries the declared roles in a deterministic priority order
/// (`doer`, then `reviewer`, then `complete`) and serves the first hook the
/// state declares for that role. This reuses the SAME `resolve_and_read_hook`
/// path as `begin` (so the budget cap and absence-is-not-an-error semantics are
/// inherited), never duplicating the read/cap logic.
pub fn reserve_hook_for_open_begin(
    query: &dyn QueryPort,
    hook_reader: &dyn PlaybookHookBodyPort,
    registry: &dyn PlaybookRegistry,
    artifact_id: &str,
    kind: &str,
    state: &str,
    actor: &str,
) -> Result<String, BeginError> {
    let activity = query.read_activity_entries(artifact_id)?;
    let transitions = query.read_transitions(artifact_id)?;
    if !crate::domain::begin_adoption::has_open_begin(&activity, &transitions, actor, state) {
        return Ok(String::new());
    }
    for role in ["doer", "reviewer", "complete"] {
        let body = resolve_and_read_hook(query, hook_reader, registry, kind, state, role)?;
        if !body.is_empty() {
            return Ok(body);
        }
    }
    Ok(String::new())
}

/// Engine-facing entry to `reserve_hook_for_open_begin` that wires the default
/// `QueryPort`-backed hook reader (mirroring `BeginCommandHandler::execute`).
pub fn reserve_hook_for_open_begin_via_query(
    query: &dyn QueryPort,
    registry: &dyn PlaybookRegistry,
    artifact_id: &str,
    kind: &str,
    state: &str,
    actor: &str,
) -> Result<String, BeginError> {
    let hook_reader = QueryPortHookBodyReader { query };
    reserve_hook_for_open_begin(
        query,
        &hook_reader,
        registry,
        artifact_id,
        kind,
        state,
        actor,
    )
}

fn routed_origin_turn(request: &BeginRequest) -> Option<&str> {
    if request.rd_selected.is_empty() || request.rd_turn_id.is_empty() {
        None
    } else {
        Some(request.rd_turn_id.as_str())
    }
}

fn origin_turn_hit(
    query: &dyn QueryPort,
    hook_reader: &dyn PlaybookHookBodyPort,
    registry: &dyn PlaybookRegistry,
    kind: &str,
    origin_turn: Option<&str>,
) -> Result<Option<BeginOutcome>, BeginError> {
    let Some(origin_turn) = origin_turn else {
        return Ok(None);
    };
    let Some(existing) = query.find_artifact_by_kind_origin_turn(kind, origin_turn)? else {
        return Ok(None);
    };
    let context_text =
        resolve_and_read_hook(query, hook_reader, registry, kind, &existing.state, "doer")?;
    Ok(Some(BeginOutcome {
        result: BeginResult {
            track_path: existing.artifact_path,
            state: existing.state,
            context_text,
            measurement_kind: kind.to_string(),
            measurement_role: "doer".to_string(),
            ..Default::default()
        },
        events: Vec::new(),
    }))
}

/// Validate that the caller supplied the full required actor identity.
/// Per spec R2 of the checkin_backfill_spec_context track, `actor_name`
/// is required-non-empty in all modes; the runtime params (`actor_type`,
/// `actor_model`, `actor_provider`) are required-non-empty as well.
/// Validation runs before any port call so failures produce no
/// filesystem side effects.
fn validate_actor_identity(request: &BeginRequest) -> Result<(), BeginError> {
    if request.actor_name.is_empty() {
        return Err(BeginError::ActorNameRequired);
    }
    for (field, value) in [
        ("actor_type", &request.actor_type),
        ("actor_model", &request.actor_model),
        ("actor_provider", &request.actor_provider),
    ] {
        if value.is_empty() {
            return Err(BeginError::ActorParamsRequired {
                field: field.to_string(),
            });
        }
    }
    Ok(())
}

/// Dispatch a begin request to either the create path or the review path
/// based on which identifying field is set.
fn dispatch(
    query: &dyn QueryPort,
    hook_reader: &dyn PlaybookHookBodyPort,
    registry: &dyn PlaybookRegistry,
    request: BeginRequest,
) -> Result<BeginOutcome, BeginError> {
    let has_type = !request.artifact_type.is_empty();
    let has_identifier = !request.identifier.is_empty();
    match (has_type, has_identifier) {
        (true, false) => handle_create(query, hook_reader, registry, request),
        (false, true) => handle_review(query, hook_reader, registry, request),
        (true, true) => Err(BeginError::InvalidArgument {
            reason: "both 'artifact_type' and 'identifier' set; exactly one must be supplied"
                .to_string(),
        }),
        (false, false) => Err(BeginError::InvalidArgument {
            reason: "neither 'artifact_type' nor 'identifier' set; exactly one must be supplied"
                .to_string(),
        }),
    }
}

/// Creation flow — pure read + emit. Reads the parent state and context
/// file via `QueryPort`, then emits the appropriate creation event carrying
/// all the data the engine shim needs. No mutation calls in this handler.
fn handle_create(
    query: &dyn QueryPort,
    hook_reader: &dyn PlaybookHookBodyPort,
    registry: &dyn PlaybookRegistry,
    request: BeginRequest,
) -> Result<BeginOutcome, BeginError> {
    validate_artifact_type(&request.artifact_type, registry)?;

    // The `playbook` create flow keeps its dedicated arm. (Q-C.)
    if request.artifact_type == "playbook" {
        let machine = registry
            .machine_for(&request.artifact_type)
            .ok_or_else(|| {
                BeginError::Checkin(CheckinError::UnsupportedType {
                    artifact_type: request.artifact_type.clone(),
                })
            })?;
        if !request.rd_selected.is_empty() {
            if request.rd_selected != request.artifact_type {
                return Err(BeginError::RoutedSelectionMismatch {
                    selected: request.rd_selected.clone(),
                    artifact_type: request.artifact_type.clone(),
                });
            }
            if !machine.is_driven() {
                return Err(BeginError::NotDrivenCandidate {
                    kind: request.artifact_type.clone(),
                });
            }
        }
        if !granted(&request.ctx, &machine.access) {
            return Err(BeginError::AccessDenied {
                kind: machine.kind.clone(),
            });
        }
        if let Some(outcome) = origin_turn_hit(
            query,
            hook_reader,
            registry,
            &request.artifact_type,
            routed_origin_turn(&request),
        )? {
            return Ok(outcome);
        }

        if machine.parent_required || !request.parent_id.is_empty() {
            // Optional-parent playbook creation treats a non-empty parent_id as
            // provenance: it must exist, but its kind/state are not gates.
            let parent_state = query.read_artifact_state(&request.parent_id).map_err(|e| {
                use crate::ports::query_port::QueryError;
                match e {
                    QueryError::NotFound { .. } => {
                        BeginError::Checkin(CheckinError::ParentNotFound {
                            parent_id: request.parent_id.clone(),
                        })
                    }
                    other => BeginError::from(other),
                }
            })?;
            let parent_kind = query
                .read_artifact_kind(&request.parent_id)
                .map_err(BeginError::from)?;
            if machine.parent_required {
                validate_parent_state(&request.parent_id, Some(&parent_state), &parent_kind)?;
                if parent_kind != "track" {
                    return Err(BeginError::Checkin(CheckinError::ParentKindInvalid {
                        parent_id: request.parent_id.clone(),
                        expected_kind: "track".to_string(),
                        actual_kind: parent_kind,
                    }));
                }
            }
        }
        return handle_create_playbook(query, hook_reader, registry, request);
    }

    // Resolve the machine for this kind. `validate_artifact_type` already
    // guaranteed it resolves, so this unwrap is total — but resolve the
    // owned fields up front so we don't hold a borrow across the handler.
    let machine = registry
        .machine_for(&request.artifact_type)
        .ok_or_else(|| {
            BeginError::Checkin(CheckinError::UnsupportedType {
                artifact_type: request.artifact_type.clone(),
            })
        })?;
    if !request.rd_selected.is_empty() {
        if request.rd_selected != request.artifact_type {
            return Err(BeginError::RoutedSelectionMismatch {
                selected: request.rd_selected.clone(),
                artifact_type: request.artifact_type.clone(),
            });
        }
        if !machine.is_driven() {
            return Err(BeginError::NotDrivenCandidate {
                kind: request.artifact_type.clone(),
            });
        }
    }
    let kind = machine.kind.clone();
    if !granted(&request.ctx, &machine.access) {
        return Err(BeginError::AccessDenied { kind });
    }
    // K8 genesis (plan Task 5) branches BEFORE generic field collection: the
    // one closed `item` object is decoded and validated in this pure handler,
    // the id is minted server-side, and a TYPED creation event is emitted. The
    // generic timestamped scaffold is never reached, so the result path is
    // always exactly `backlog_items/bi_...` and no candidate -> candidate
    // Snapshot is dispatched.
    if kind == "backlog_item" {
        return handle_create_backlog_item(request);
    }
    if let Some(outcome) = origin_turn_hit(
        query,
        hook_reader,
        registry,
        &kind,
        routed_origin_turn(&request),
    )? {
        return Ok(outcome);
    }
    let directory = machine.directory.clone();
    let registry_file = machine.registry.clone();
    let parent_kind = machine.parent_kind.clone();
    // Initial state = first declared state (first-state convention, Q-B).
    let initial_state = machine
        .states
        .first()
        .map(|s| s.name.clone())
        .ok_or_else(|| {
            BeginError::Checkin(CheckinError::UnsupportedType {
                artifact_type: request.artifact_type.clone(),
            })
        })?;

    let missing_required_fields = missing_create_required_fields(machine, &request);
    if !missing_required_fields.is_empty() {
        return Err(BeginError::Checkin(CheckinError::MissingRequiredField {
            field: missing_required_fields.join(","),
        }));
    }

    if machine.projection_only {
        return handle_projection_only_create(
            query,
            hook_reader,
            registry,
            &request,
            &kind,
            &initial_state,
        );
    }

    // Parent handling is conditional on the machine's `parent_kind`. When the
    // machine declares a parent kind (track → proposal), enforce parent exists
    // + active + kind matches. When `None` (knowledge_lifecycle), no parent is
    // required and no parent read runs.
    let parent_field = match &parent_kind {
        Some(expected_kind) if machine.parent_required => {
            let parent_state = query.read_artifact_state(&request.parent_id).map_err(|e| {
                use crate::ports::query_port::QueryError;
                match e {
                    QueryError::NotFound { .. } => {
                        BeginError::Checkin(CheckinError::ParentNotFound {
                            parent_id: request.parent_id.clone(),
                        })
                    }
                    other => BeginError::from(other),
                }
            })?;
            let actual_parent_kind = query
                .read_artifact_kind(&request.parent_id)
                .map_err(BeginError::from)?;
            validate_parent_state(&request.parent_id, Some(&parent_state), &actual_parent_kind)?;
            if &actual_parent_kind != expected_kind {
                return Err(BeginError::Checkin(CheckinError::ParentKindInvalid {
                    parent_id: request.parent_id.clone(),
                    expected_kind: expected_kind.clone(),
                    actual_kind: actual_parent_kind,
                }));
            }
            request.parent_id.clone()
        }
        Some(_) if !request.parent_id.is_empty() => {
            query.read_artifact_state(&request.parent_id).map_err(|e| {
                use crate::ports::query_port::QueryError;
                match e {
                    QueryError::NotFound { .. } => {
                        BeginError::Checkin(CheckinError::ParentNotFound {
                            parent_id: request.parent_id.clone(),
                        })
                    }
                    other => BeginError::from(other),
                }
            })?;
            request.parent_id.clone()
        }
        Some(_) => String::new(),
        None => String::new(),
    };

    // The creation/seed transition role: `spec` for the track encoding (its
    // first working edge role), `doer` for domain machines (create-time
    // bookkeeping; AC1 does not constrain this for knowledge).
    let creation_role = if kind == "track" {
        "spec".to_string()
    } else {
        "doer".to_string()
    };
    let scaffold_files = initial_scaffold_files(&kind, &request.track_name);

    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    // Record only the machine's declared generic required fields (those outside
    // the builtin set) on the status. Unrelated bag entries are dropped, and
    // builtins (name/parent_id/approver/target_owner) keep their dedicated
    // homes rather than duplicating into `fields`.
    let declared_fields = collect_declared_generic_fields(machine, &request);

    let status = StatusContent {
        version: 1,
        kind: kind.clone(),
        state: initial_state.clone(),
        parent_id: parent_field.clone(),
        actor_name: request.actor_name.clone(),
        actor_type: request.actor_type.clone(),
        actor_model: request.actor_model.clone(),
        actor_provider: request.actor_provider.clone(),
        actor_context_window: request.actor_context_window,
        actor_sdk_version: request.actor_sdk_version.clone(),
        actor_entrypoint: request.actor_entrypoint.clone(),
        actor_registered_at: now.clone(),
        origin_turn: routed_origin_turn(&request).unwrap_or("").to_string(),
        transition_to: initial_state.clone(),
        transition_at: now,
        transition_role: creation_role.clone(),
        transition_approver: request.approver.clone(),
        target_owner: request.target_owner.clone(),
        fields: declared_fields,
    };

    // Serve the playbook-declared (initial_state, doer) hook body into
    // context_text. The declaration lives on the resolved machine; absence of
    // a declared hook yields an empty context_text (AC-2), not an error.
    let context_text = resolve_and_read_hook(
        query,
        hook_reader,
        registry,
        &request.artifact_type,
        &initial_state,
        "doer",
    )?;
    // Interpolate `{{key}}` placeholders in the served hook body with the
    // supplied create-time field values (generic bag + builtins).
    let context_text = interpolate_create_fields(&context_text, &request, &status);

    // track_path is not yet known at this point — the engine shim derives it
    // after routing the ArtifactCreation event.
    let result = BeginResult {
        state: initial_state.clone(),
        context_text,
        ..Default::default()
    };

    let actor = ActorIdentity {
        name: request.actor_name.clone(),
        actor_type: request.actor_type.clone(),
        model: request.actor_model.clone(),
        provider: request.actor_provider.clone(),
        context_window: request.actor_context_window,
        sdk_version: request.actor_sdk_version.clone(),
        entrypoint: request.actor_entrypoint.clone(),
        registered_at: status.actor_registered_at.clone(),
    };

    let event = Event::ArtifactCreation {
        track_name: request.track_name.clone(),
        parent_id: parent_field,
        display_name: request.track_name.clone(),
        actor,
        approver: request.approver.clone(),
        status,
        directory,
        registry_file,
        scaffold_files,
        creation_role,
        conversation_id: request.conversation_id.clone(),
    };

    Ok(BeginOutcome {
        result,
        events: vec![event],
    })
}

fn handle_projection_only_create(
    query: &dyn QueryPort,
    hook_reader: &dyn PlaybookHookBodyPort,
    registry: &dyn PlaybookRegistry,
    request: &BeginRequest,
    kind: &str,
    initial_state: &str,
) -> Result<BeginOutcome, BeginError> {
    let context_text =
        resolve_and_read_hook(query, hook_reader, registry, kind, initial_state, "doer")?;
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let actor = ActorIdentity {
        name: request.actor_name.clone(),
        actor_type: request.actor_type.clone(),
        model: request.actor_model.clone(),
        provider: request.actor_provider.clone(),
        context_window: request.actor_context_window,
        sdk_version: request.actor_sdk_version.clone(),
        entrypoint: request.actor_entrypoint.clone(),
        registered_at: now,
    };
    Ok(BeginOutcome {
        result: BeginResult {
            track_path: "sparks/sparks.md".to_string(),
            state: initial_state.to_string(),
            context_text,
            measurement_kind: kind.to_string(),
            measurement_role: "doer".to_string(),
            ..Default::default()
        },
        events: vec![Event::ProjectionOnlySnapshot {
            artifact_path: "sparks/sparks.md".to_string(),
            event_type: kind.to_string(),
            body: request.track_name.clone(),
            actor,
        }],
    })
}

fn initial_scaffold_files(kind: &str, display_name: &str) -> Vec<(String, String)> {
    match kind {
        "track" => vec![("spec.md".to_string(), format!("# {}\n", display_name))],
        "playbook_generation" | "workflow_generation" => vec![
            (
                "research.md".to_string(),
                format!("# Research: {}\n", display_name),
            ),
            (
                "proposal.md".to_string(),
                format!("# Proposal: {}\n", display_name),
            ),
            ("plan.md".to_string(), format!("# Plan: {}\n", display_name)),
            ("draft.md".to_string(), format!("# Draft: {}\n", display_name)),
            ("amendments.md".to_string(), String::new()),
            ("machine.yaml".to_string(), String::new()),
            ("reflection.md".to_string(), String::new()),
            (
                "authoring.md".to_string(),
                format!(
                    "# Authoring Projection\n\nCurrent phase: Gathering\nCurrent state: gathering\n\n## History\n"
                ),
            ),
        ],
        "decision" => vec![
            (
                "definition.md".to_string(),
                format!(
                    "# Decision: {}\n\n## Question\n\n\n## Context\n\n\n## Domain tags\n\n\n## Lineage\n\n\n## Related\n\n",
                    display_name
                ),
            ),
            ("evidence.md".to_string(), String::new()),
            ("review.md".to_string(), String::new()),
            ("amendments.md".to_string(), String::new()),
        ],
        "learning" => vec![
            (
                "definition.md".to_string(),
                format!(
                    "# Learning: {}\n\n## Observation\n\n\n## Context\n\n\n## Source\n\n\n## Domain tags\n\n",
                    display_name
                ),
            ),
            ("evidence.md".to_string(), String::new()),
            ("review.md".to_string(), String::new()),
        ],
        "initiative" => vec![
            (
                "definition.md".to_string(),
                format!("# Initiative: {}\n", display_name),
            ),
            ("evidence.md".to_string(), String::new()),
            ("review.md".to_string(), String::new()),
        ],
        "proposal" => vec![
            (
                "definition.md".to_string(),
                format!("# Proposal: {}\n", display_name),
            ),
            ("evidence.md".to_string(), String::new()),
            ("review.md".to_string(), String::new()),
            ("amendments.md".to_string(), String::new()),
        ],
        "milestone" => vec![
            (
                "definition.md".to_string(),
                format!("# Milestone: {}\n", display_name),
            ),
            ("evidence.md".to_string(), String::new()),
            ("review.md".to_string(), String::new()),
            ("amendments.md".to_string(), String::new()),
        ],
        _ => Vec::new(),
    }
}

fn missing_create_required_fields(
    machine: &crate::domain::playbook::types::PlaybookMachine,
    request: &BeginRequest,
) -> Vec<String> {
    machine
        .required_fields
        .iter()
        .filter_map(|field| {
            if create_required_value(field.name.as_str(), request).is_empty() {
                Some(field.name.clone())
            } else {
                None
            }
        })
        .collect()
}

/// The builtin required-field names that have dedicated homes on the request /
/// status and are therefore NOT recorded in the generic `fields` bag.
///
/// These are names a MACHINE.YAML declares — persisted hearth data this repo
/// reads and does not own. `workflow_name` is the pre-migration spelling the
/// live builder machine still declares; it resolves the SAME dedicated
/// `playbook_name` value (see `create_required_value`), so it is one home for
/// one value, not a second code path. Dropping it moves the field into the
/// generic bag, where nothing ever reads it — see
/// `begin_create_legacy_name_field.feature`.
const BUILTIN_REQUIRED_FIELDS: &[&str] = &[
    "name",
    "track_name",
    "playbook_name",
    "workflow_name",
    "parent_id",
    "approver",
    "target_owner",
];

/// Collect the generic (non-builtin) required fields the machine declares,
/// reading each value from the request's field bag. Builtins are excluded
/// (they live on dedicated request/status fields). Unrelated bag entries the
/// caller may have supplied are dropped — only declared fields land here.
fn collect_declared_generic_fields(
    machine: &crate::domain::playbook::types::PlaybookMachine,
    request: &BeginRequest,
) -> std::collections::BTreeMap<String, String> {
    machine
        .required_fields
        .iter()
        .filter(|f| !BUILTIN_REQUIRED_FIELDS.contains(&f.name.as_str()))
        .filter_map(|f| {
            request
                .fields
                .get(&f.name)
                .map(|v| (f.name.clone(), v.clone()))
        })
        .collect()
}

/// Replace `{{key}}` placeholders in a hook body with create-time field values.
/// Resolves builtins (track_name, parent_id, approver, target_owner, etc.) and
/// every recorded generic field on the status. Unknown placeholders are left
/// untouched. Minimal mustache-style substitution — no conditionals or loops.
fn interpolate_create_fields(body: &str, request: &BeginRequest, status: &StatusContent) -> String {
    if !body.contains("{{") {
        return body.to_string();
    }
    let mut values: std::collections::BTreeMap<&str, &str> = std::collections::BTreeMap::new();
    // Builtins.
    values.insert("track_name", request.track_name.as_str());
    values.insert("name", request.track_name.as_str());
    values.insert("playbook_name", request.playbook_name.as_str());
    // The pre-migration spelling a live machine.yaml still declares, resolving
    // the SAME dedicated value — see BUILTIN_REQUIRED_FIELDS.
    values.insert("workflow_name", request.playbook_name.as_str());
    values.insert("parent_id", request.parent_id.as_str());
    values.insert("approver", request.approver.as_str());
    values.insert("target_owner", request.target_owner.as_str());
    // Recorded generic fields (declared + supplied).
    for (k, v) in &status.fields {
        values.insert(k.as_str(), v.as_str());
    }
    let mut out = body.to_string();
    for (key, value) in values {
        if value.is_empty() {
            continue;
        }
        out = out.replace(&format!("{{{{{}}}}}", key), value);
    }
    out
}

fn create_required_value<'a>(field: &str, request: &'a BeginRequest) -> &'a str {
    match field {
        "name" | "track_name" => &request.track_name,
        "playbook_name" | "workflow_name" => &request.playbook_name,
        "parent_id" => &request.parent_id,
        "approver" => &request.approver,
        "target_owner" => &request.target_owner,
        // Any non-builtin required field is satisfied from the generic field bag.
        _ => request.fields.get(field).map(String::as_str).unwrap_or(""),
    }
}

/// Playbook creation flow — pure read + emit. Validates parent kind (must be
/// "track") + parent state (active-only rule, same as track-under-proposal).
/// Emits `Event::PlaybookCreation`. No mutation calls.
///
/// Called from `handle_create` after parent-state and parent-kind checks pass.
fn handle_create_playbook(
    _query: &dyn QueryPort,
    _hook_reader: &dyn PlaybookHookBodyPort,
    registry: &dyn PlaybookRegistry,
    request: BeginRequest,
) -> Result<BeginOutcome, BeginError> {
    let machine = registry.machine_for("playbook").ok_or_else(|| {
        BeginError::Checkin(CheckinError::UnsupportedType {
            artifact_type: "playbook".to_string(),
        })
    })?;
    if !granted(&request.ctx, &machine.access) {
        return Err(BeginError::AccessDenied {
            kind: machine.kind.clone(),
        });
    }

    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let status = StatusContent {
        version: 1,
        kind: "playbook".to_string(),
        state: "draft".to_string(),
        parent_id: request.parent_id.clone(), // unified parent_id key; any provenance kind
        actor_name: request.actor_name.clone(),
        actor_type: request.actor_type.clone(),
        actor_model: request.actor_model.clone(),
        actor_provider: request.actor_provider.clone(),
        actor_context_window: request.actor_context_window,
        actor_sdk_version: request.actor_sdk_version.clone(),
        actor_entrypoint: request.actor_entrypoint.clone(),
        actor_registered_at: now.clone(),
        origin_turn: routed_origin_turn(&request).unwrap_or("").to_string(),
        transition_to: "draft".to_string(),
        transition_at: now,
        transition_role: "doer".to_string(),
        transition_approver: request.approver.clone(),
        // Playbook creation does not carry a target_owner.
        target_owner: String::new(),
        // Playbook creation carries no generic fields.
        fields: std::collections::BTreeMap::new(),
    };

    let result = BeginResult {
        state: "draft".to_string(),
        ..Default::default()
    };

    let actor = ActorIdentity {
        name: request.actor_name.clone(),
        actor_type: request.actor_type.clone(),
        model: request.actor_model.clone(),
        provider: request.actor_provider.clone(),
        context_window: request.actor_context_window,
        sdk_version: request.actor_sdk_version.clone(),
        entrypoint: request.actor_entrypoint.clone(),
        registered_at: status.actor_registered_at.clone(),
    };

    let event = Event::PlaybookCreation {
        playbook_name: request.playbook_name.clone(),
        parent_id: request.parent_id.clone(),
        actor,
        approver: request.approver.clone(),
        status,
    };

    Ok(BeginOutcome {
        result,
        events: vec![event],
    })
}

fn track_review_files_for_state(state: &str) -> Option<(&'static str, &'static str)> {
    match state {
        "spec_review" => Some(("spec.md", "spec.review.md")),
        "plan_review" => Some(("plan.md", "plan.review.md")),
        "impl_phase_review" => Some(("plan.md", "impl.phase.review.md")),
        "impl_review" => Some(("plan.md", "impl.review.md")),
        "reflection_review" => Some(("reflection.md", "reflection.review.md")),
        "amend_review" => Some(("spec.amendments.md", "spec.review.md")),
        _ => None,
    }
}

fn track_doer_resume_state(state: &str) -> bool {
    matches!(
        state,
        // `spec` is a doer-resume state so an ADOPTED track (reset to its
        // initial `spec` state) is resumable if its adopting session is
        // interrupted: begin(identifier, resumer) re-serves the spec doer hook
        // and re-opens the begin, instead of stranding on a dead external
        // fallback (Finding 5). A freshly-created spec track is
        // equally resumable — the spec doer just continues authoring.
        "spec"
            | "plan"
            | "implementing"
            | "reflecting"
            | "spec_revision"
            | "plan_revision"
            | "impl_revision"
            | "reflection_revision"
    )
}

/// Slice C carry-forward delimiter (R4.1). Used by both the composer here and
/// the feature-step assertions; changing it is a deliberate feature change.
const CARRY_FORWARD_DELIMITER: &str = "## Carry-forward findings from spec review";

/// On a `plan`-state track, append the carry-forward findings section to
/// `preamble` when `carry-forward.md` is present (Slice C, R4). Returns
/// `preamble` unchanged when the file is absent (R4.2 — absence is not an
/// error) or the state is not `plan`. Reads the file fresh via QueryPort
/// (R4.5 — no cache); never deletes or marks it consumed (R4.4).
///
/// Concatenation rule (R4.3 lock):
/// - both preamble + section non-empty → `preamble + "\n\n" + section`
/// - only preamble → `preamble`
/// - only section → `section` (no leading blank lines)
/// - both empty → `""`
fn append_carry_forward_section(
    query: &dyn QueryPort,
    identifier: &str,
    state: &str,
    preamble: String,
) -> Result<String, BeginError> {
    if state != "plan" {
        return Ok(preamble);
    }
    let artifact_path = format!("tracks/{}", identifier);
    let Some(body) = query
        .read_carry_forward_if_present(&artifact_path)
        .map_err(BeginError::from)?
    else {
        return Ok(preamble);
    };
    let findings = extract_carry_forward_findings(&body);
    let section = format!("{}\n\n{}", CARRY_FORWARD_DELIMITER, findings);
    Ok(match (preamble.is_empty(), section.is_empty()) {
        (false, false) => format!("{}\n\n{}", preamble, section),
        (false, true) => preamble,
        (true, false) => section,
        (true, true) => String::new(),
    })
}

/// Extract the verbatim findings bytes from a rendered `carry-forward.md` body.
/// The file format (locked in complete.rs::render_carry_forward) is:
/// frontmatter (`---` … `---`), blank line, `# Carry-forward from spec review`,
/// blank line, `_Reviewer: … · …_`, blank line, then the verbatim findings.
/// This skips past the second `---` line (closing the engine frontmatter at the
/// top — anchored by position, not by re-scanning), then past the header block,
/// and returns the remainder unchanged.
fn extract_carry_forward_findings(body: &str) -> String {
    // Split into lines preserving the ability to rejoin the remainder verbatim.
    // We locate the byte offset after the structural prefix and slice the rest.
    let mut rest = body;
    // 1. Opening `---\n`.
    if let Some(after) = rest.strip_prefix("---\n") {
        // 2. Up to and including the closing `---\n` (the FIRST one on a line
        //    by itself after line 1 — anchors to the engine's frontmatter).
        if let Some(pos) = after.find("\n---\n") {
            rest = &after[pos + "\n---\n".len()..];
        } else {
            // Malformed frontmatter: return the whole body verbatim rather than
            // dropping content.
            return body.to_string();
        }
    } else {
        return body.to_string();
    }
    // 3. Skip a single leading blank line, the header line, a blank line, the
    //    reviewer-signature line, and a blank line — each only if present, so a
    //    manually-edited file degrades gracefully.
    rest = rest.strip_prefix('\n').unwrap_or(rest);
    rest = strip_line_prefix(rest, "# Carry-forward from spec review");
    rest = rest.strip_prefix('\n').unwrap_or(rest);
    rest = strip_signature_line(rest);
    rest = rest.strip_prefix('\n').unwrap_or(rest);
    rest.to_string()
}

/// Strip `line` plus its trailing `\n` from the front of `s` when present.
fn strip_line_prefix<'a>(s: &'a str, line: &str) -> &'a str {
    s.strip_prefix(line)
        .and_then(|r| r.strip_prefix('\n'))
        .unwrap_or(s)
}

/// Strip a leading `_Reviewer: … _` signature line (plus trailing `\n`).
fn strip_signature_line(s: &str) -> &str {
    if s.starts_with("_Reviewer:") {
        if let Some(nl) = s.find('\n') {
            return &s[nl + 1..];
        }
    }
    s
}

fn begin_marker_for(track_path: String, request: &BeginRequest, state: &str) -> Event {
    Event::BeginMarkerWritten {
        artifact_path: track_path,
        kind: "begin".to_string(),
        actor: request.actor_name.clone(),
        state: state.to_string(),
        at: String::new(),
        conversation_id: request.conversation_id.clone(),
    }
}

/// Slice B — doer (`creator` wire role) re-entering a `spec_revision` track via
/// `begin(identifier)`. Delivers revision context from the machine-declared
/// `(spec_revision, doer)` hook (`spec-revision.md`); computes `review_doc_path`
/// deterministically (`<track_path>/spec.review.md`) without validating the
/// file; leaves `artifact_text` empty (the doer reads `spec.md` /
/// `spec.review.md` directly). Records NO state transition — only a begin
/// marker. Mirrors the resumer/doer branches' "context-only, no transition"
/// shape.
fn handle_spec_revision_doer(
    query: &dyn QueryPort,
    hook_reader: &dyn PlaybookHookBodyPort,
    registry: &dyn PlaybookRegistry,
    request: &BeginRequest,
    state: &str,
) -> Result<BeginOutcome, BeginError> {
    let track_path = format!("tracks/{}", request.identifier);
    // Revision context: the (spec_revision, doer) hook body (spec-revision.md).
    let context_text = resolve_and_read_hook(query, hook_reader, registry, "track", state, "doer")?;
    // review_doc_path is a computed path (R2.5): deterministic from the track
    // path; existence is not validated.
    let review_doc_path = format!("{}/spec.review.md", track_path);
    Ok(BeginOutcome {
        result: BeginResult {
            track_path: track_path.clone(),
            state: state.to_string(),
            context_text,
            // artifact_text intentionally left empty (R2.4); the shim omits the
            // JSON key when empty.
            review_doc_path,
            measurement_kind: "track".to_string(),
            measurement_role: "doer".to_string(),
            ..Default::default()
        },
        events: vec![begin_marker_for(track_path, request, state)],
    })
}

/// Identifier flow — serves machine-declared hook context for an existing
/// artifact. Reviewers receive `review_context_text`; doer-like roles receive
/// `context_text`. No lifecycle transition is recorded.
///
/// Errors are mapped to the dedicated taxonomy:
/// - `SessionRequired` when no session_role was sent
/// - `ModeNotImplemented` when session_role is recognized but its
///   identifier-mode isn't yet engine-supported (e.g., `resumer`)
/// - `RoleStateMismatch` when the role can't act on identifiers
///   (e.g., `creator` operates on artifact types, not ids)
/// - `NotFound` when the identifier doesn't exist in the hearth
/// - `SpecNotReadyForReview` for (track, spec, reviewer) — cutover to
///   doer-`complete`-drives-spec-review per spec R4.1/R4.3
/// - `StateNotReviewable` for any reviewer combo outside the engine's
///   supported set
///
/// ## Adoption of out-of-engine artifacts (`adopt: true`)
///
/// Nick's requirement: "if something is created out of engine, set up the
/// engine to have it go back to the beginning and do the playbook properly."
/// ADOPTION is restart-at-initial-state: an artifact that exists on disk but
/// was never created via the engine (no recorded transition history) is taken
/// back to its machine's INITIAL state and driven through EVERY phase and
/// review gate. The hand-made files are the doer's raw material — each phase's
/// doer refines them and each review gate genuinely evaluates them (already-good
/// work passes quickly through real gates, not state-mapping).
///
/// - **Trigger.** An explicit `adopt: true` on `begin(identifier)`. This is the
///   smallest sound MCP-surface change: adoption is fully gated behind the flag,
///   so a plain (non-adopt) begin takes NONE of the adoption path (the reset,
///   the adoption event, the preamble). It is not byte-identical to the
///   pre-adoption engine — the spec-resume / spec-revision handling was
///   broadened alongside — but adoption adds no behavior to a begin that does
///   not set the flag. An implicit trigger is impossible because an on-disk hand-made
///   artifact and a legitimately-resumable one are indistinguishable by history
///   alone (both may have zero recorded transitions), so an implicit reset would
///   silently clobber in-flight resumes. Adoption must be caller-declared.
/// - **Discovery.** The engine's non-drivable outcomes (`ModeNotImplemented`,
///   `StateNotReviewable` — the `fallback:*` paths) name `adopt: true` in their
///   error messages, so a session that hits a fallback tag is told how to adopt.
/// - **Adoption event.** The reset is recorded as an appended `ReviewTransition`
///   to the initial state carrying a note `"adopted into governance from
///   out-of-engine state '<prior>'"`. It is an append to the event-sourced
///   transition log (CQRS: events append, projections follow) — never a rewrite
///   or deletion of the hand-made files, so pre-governance history is preserved.
///   A `BeginMarkerWritten` opens the adopting actor's begin at the initial
///   state so their first `complete` is not flagged as un-begun.
/// - **Guard.** Adoption is refused (`AlreadyGoverned`) for any artifact that
///   already carries engine transition history — you cannot "adopt" (destroy the
///   progress of) an in-flight, engine-governed artifact.
/// - **Fallback-tag semantics.** `execution_route` is computed purely from
///   `(kind, state, role)`. Once adopted, the folded state becomes the machine's
///   initial state (which routing already maps to `engine` for driven kinds), so
///   the discriminator flips from `fallback:*` to `engine` on the next
///   catalog/checkin with no routing change — the tag clears because the
///   artifact is now genuinely engine-driven.
/// - **Doer/reviewer context.** The served `context_text` prepends an adoption
///   preamble that surfaces "existing pre-governance files present — read,
///   evaluate, and refine them" ahead of the initial state's doer hook, so both
///   doers and (via subsequent phases) reviewers evaluate the existing content.
fn handle_adoption(
    query: &dyn QueryPort,
    hook_reader: &dyn PlaybookHookBodyPort,
    registry: &dyn PlaybookRegistry,
    request: &BeginRequest,
    kind: &str,
    prior_state: &str,
) -> Result<BeginOutcome, BeginError> {
    // Adoption only applies to a driven (machine-resolvable) kind — the engine
    // must know the machine to know its initial state.
    let Some(machine) = registry.machine_for(kind) else {
        return Err(BeginError::InvalidArgument {
            reason: format!(
                "adopt requires a driven artifact kind; '{}' has no registered machine",
                kind
            ),
        });
    };

    // A projection-only kind (e.g. spark) has no per-artifact state machine to
    // reset TO — its "state" is a fold over shared projection rows, not an
    // initial-state edge. Adoption is meaningless for it; reject explicitly
    // rather than fabricating a bogus initial-state transition.
    if machine.projection_only {
        return Err(BeginError::InvalidArgument {
            reason: format!(
                "adopt does not apply to projection-only kind '{}'; it has no per-artifact \
                 initial state to reset to",
                kind
            ),
        });
    }

    // Guard: adoption resets an artifact to its machine's initial state, so it is
    // ONLY valid for a genuinely pre-governance artifact — one the engine has
    // never touched. "Governed" is ANY of three evidence channels, so a reset can
    // never clobber in-flight engine work:
    //   (a) a recorded transition (the create/seed transition, or any later one);
    //   (b) an OPEN or CLOSED begin marker (an engine begin can write ONLY a
    //       marker with no transition — generic-begin / track-resume paths — so
    //       an empty transition log does NOT prove "never governed");
    //   (c) — folded into (a): the creation transition is engine-creation evidence.
    // The evidence reads must also FAIL CLOSED on damage: a degraded activity log
    // (`dropped > 0`) means a begin marker may have been silently lost, so we
    // cannot prove the artifact is ungoverned — refuse with a distinct anomaly
    // error rather than adopt (and risk clobbering) on incomplete evidence. The
    // read errors (I/O, malformed status) already surface as `Err` and propagate,
    // which is likewise fail-closed.
    // STRICT read: damaged transition evidence (an unreadable `transitions/`
    // dir or an unparseable event) surfaces as AdoptionEvidenceUnreadable and
    // fails closed here — the lenient read would silently drop the very event
    // that proves this artifact is governed, letting the reset clobber it.
    let transitions = query
        .read_transitions_strict(&request.identifier)
        .map_err(BeginError::from)?;
    let activity = query
        .read_activity_log(&request.identifier)
        .map_err(BeginError::from)?;
    if activity.is_degraded() {
        return Err(BeginError::AdoptionEvidenceUnreadable {
            identifier: request.identifier.clone(),
            detail: format!(
                "activity log degraded ({} malformed entr{} dropped) — cannot prove the \
                 artifact carries no engine begin marker",
                activity.dropped,
                if activity.dropped == 1 { "y" } else { "ies" },
            ),
        });
    }
    let has_begin_marker = activity.entries.iter().any(|e| e.kind == "begin");
    if !transitions.is_empty() || has_begin_marker {
        return Err(BeginError::AlreadyGoverned {
            identifier: request.identifier.clone(),
            state: prior_state.to_string(),
        });
    }

    // Initial state = first declared state (first-state convention, matching the
    // create flow). The creation/seed role mirrors handle_create: `spec` for the
    // track encoding (its first working edge role), `doer` for domain machines.
    let initial_state = machine
        .states
        .first()
        .map(|s| s.name.clone())
        .ok_or_else(|| BeginError::InvalidArgument {
            reason: format!("machine for '{}' declares no states", kind),
        })?;
    let creation_role = if kind == "track" { "spec" } else { "doer" };

    // Resolve the artifact's canonical path from the machine's directory (the id
    // may or may not already carry the directory prefix — mirror handle_review).
    let artifact_id = request
        .identifier
        .rsplit('/')
        .next()
        .unwrap_or(&request.identifier);
    let artifact_path = if request
        .identifier
        .starts_with(&format!("{}/", machine.directory))
    {
        request.identifier.clone()
    } else {
        format!("{}/{}", machine.directory, artifact_id)
    };

    // Serve the initial state's doer hook and prepend the adoption preamble so
    // the doer treats the existing pre-governance files as raw material.
    let hook_body =
        resolve_and_read_hook(query, hook_reader, registry, kind, &initial_state, "doer")?;
    let context_text = format!(
        "{}\n\n{}",
        adoption_preamble(prior_state, &initial_state),
        hook_body
    );

    let actor = crate::domain::shared_types::ActorIdentity {
        name: request.actor_name.clone(),
        actor_type: request.actor_type.clone(),
        model: request.actor_model.clone(),
        provider: request.actor_provider.clone(),
        context_window: request.actor_context_window,
        sdk_version: request.actor_sdk_version.clone(),
        entrypoint: request.actor_entrypoint.clone(),
        registered_at: String::new(),
    };
    let adoption_transition = Event::ArtifactAdopted {
        track_path: artifact_path.clone(),
        to_state: initial_state.clone(),
        actor,
        role: creation_role.to_string(),
        note: Some(format!(
            "adopted into governance from out-of-engine state '{}'",
            prior_state
        )),
    };
    let marker = Event::BeginMarkerWritten {
        artifact_path: artifact_path.clone(),
        kind: "begin".to_string(),
        actor: request.actor_name.clone(),
        state: initial_state.clone(),
        at: String::new(),
        conversation_id: request.conversation_id.clone(),
    };

    Ok(BeginOutcome {
        result: BeginResult {
            track_path: artifact_path,
            state: initial_state,
            context_text,
            measurement_kind: kind.to_string(),
            measurement_role: "doer".to_string(),
            ..Default::default()
        },
        events: vec![adoption_transition, marker],
    })
}

/// The adoption-context preamble prepended to the initial-state doer hook. It
/// tells the doer the artifact was authored outside the engine, names the prior
/// (pre-governance) state, and directs them to evaluate/refine — not discard —
/// the existing files.
fn adoption_preamble(prior_state: &str, initial_state: &str) -> String {
    format!(
        "ADOPTION: this artifact was authored outside the engine (pre-governance \
         state '{}'). Its existing files are present on disk — read, evaluate, and \
         refine them as the raw material for this phase; do not discard or ignore \
         them. The artifact has been reset to the machine's initial state '{}' so it \
         is driven through every phase and its review gates properly. Prior \
         pre-governance history is preserved; this adoption is recorded as an \
         appended transition event.",
        prior_state, initial_state
    )
}

/// Whether `begin(identifier, adopt: true)` would ACTUALLY be accepted for this
/// artifact — the same conditions `handle_adoption` enforces. Used by the
/// fallback-error paths (`ModeNotImplemented` / `StateNotReviewable`) to decide
/// whether advertising `adopt: true` is a genuine remedy or bait: the message
/// offers adoption ONLY when a real reset would succeed (Finding 4). Any read
/// failure or damage resolves to `false` — an unofferable remedy is safer than a
/// misleading one (the actual adopt call would fail closed anyway).
fn artifact_is_adoptable(
    query: &dyn QueryPort,
    registry: &dyn PlaybookRegistry,
    identifier: &str,
    kind: &str,
) -> bool {
    let Some(machine) = registry.machine_for(kind) else {
        return false;
    };
    if machine.projection_only {
        return false;
    }
    // K8 bypass closure (plan Task 6): a `backlog_item` is NEVER adoptable. An
    // adoption reset writes a state transition through the generic path, which
    // would move K8 bytes without a prepared capability.
    if kind == "backlog_item" {
        return false;
    }
    // STRICT read (same fail-closed policy as `handle_adoption`): damaged
    // transition evidence resolves to `false` here — an unofferable remedy is
    // safer than advertising an adoption the actual call would refuse.
    let Ok(transitions) = query.read_transitions_strict(identifier) else {
        return false;
    };
    if !transitions.is_empty() {
        return false;
    }
    let Ok(activity) = query.read_activity_log(identifier) else {
        return false;
    };
    if activity.is_degraded() {
        return false;
    }
    !activity.entries.iter().any(|e| e.kind == "begin")
}

fn handle_review(
    query: &dyn QueryPort,
    hook_reader: &dyn PlaybookHookBodyPort,
    registry: &dyn PlaybookRegistry,
    request: BeginRequest,
) -> Result<BeginOutcome, BeginError> {
    if request.session_role.is_empty() {
        return Err(BeginError::SessionRequired);
    }

    // Adoption opt-in (see `handle_adoption`): take an out-of-engine artifact
    // back to its machine's initial state and drive it through the full
    // playbook. Handled before the role-specific dispatch so it works whatever
    // role the caller checked in as.
    if request.adopt {
        let kind = match query.read_artifact_kind(&request.identifier) {
            Ok(k) => k,
            Err(e) => {
                use crate::ports::query_port::QueryError;
                return match e {
                    QueryError::NotFound { .. } => Err(BeginError::NotFound {
                        identifier: request.identifier.clone(),
                    }),
                    other => Err(BeginError::from(other)),
                };
            }
        };
        let state = query
            .read_artifact_state(&request.identifier)
            .map_err(BeginError::from)?;
        return handle_adoption(query, hook_reader, registry, &request, &kind, &state);
    }

    match request.session_role.as_str() {
        "reviewer" | "doer" | "complete" => {}
        "resumer" => {}
        "creator" => {
            // Creator role looks at artifact ids by mistake — they should
            // be calling begin(artifact_type, ...). The kind/state come
            // from the artifact for context if it exists; if it doesn't,
            // fall back to "unknown" — the role mismatch is the real
            // issue.
            let (artifact_kind, state) = match query.read_artifact_kind(&request.identifier) {
                Ok(kind) => {
                    // Fold the event store for the displayed state (R9), not the
                    // stale raw `state:` field.
                    let state = query
                        .read_artifact_state(&request.identifier)
                        .unwrap_or_default();
                    (kind, state)
                }
                Err(_) => ("unknown".to_string(), String::new()),
            };
            // Slice B: a `creator` (doer) session re-entering a `spec_revision`
            // track is a legitimate revision re-entry, not a role mismatch. Take
            // the revision-context branch; all other `creator`-on-identifier
            // cases keep the RoleStateMismatch signal (e.g. `creator` on `spec`).
            if artifact_kind == "track" && state == "spec_revision" {
                return handle_spec_revision_doer(query, hook_reader, registry, &request, &state);
            }
            return Err(BeginError::RoleStateMismatch {
                role: "creator".to_string(),
                artifact_kind,
                state,
            });
        }
        other => {
            return Err(BeginError::InvalidArgument {
                reason: format!("unknown session_role '{}'", other),
            });
        }
    }

    // Session role is recognized from here on.
    let kind = match query.read_artifact_kind(&request.identifier) {
        Ok(k) => k,
        Err(e) => {
            use crate::ports::query_port::QueryError;
            return match e {
                QueryError::NotFound { .. } => Err(BeginError::NotFound {
                    identifier: request.identifier.clone(),
                }),
                other => Err(BeginError::from(other)),
            };
        }
    };
    // Resolve current state through the folding seam (per-file transition event
    // store merged with any legacy array), NOT the raw top-level `state:` field
    // — that field is no longer rewritten on a transition (event-store upcast),
    // so reading it directly would see a stale state and misroute resumer/
    // reviewer begins.
    let state = query
        .read_artifact_state(&request.identifier)
        .map_err(BeginError::from)?;

    if kind != "track" {
        // An engine-driven (machine-resolvable) playbook can be RESUMED by the
        // engine itself: re-read the current state and re-enter the doer hook.
        // The old blanket resumer rejection named a skill that did not exist,
        // and engine-driven kinds need no external fallback at all. Resumer
        // therefore maps to the doer context.
        let Some(machine) = registry.machine_for(&kind) else {
            // No registered machine ⇒ adoption cannot resolve an initial state
            // either, so it is not a genuine remedy here.
            return Err(BeginError::StateNotReviewable {
                artifact_kind: kind,
                state,
                adoptable: false,
            });
        };
        let hook_role = match request.session_role.as_str() {
            "reviewer" => "reviewer",
            "doer" | "complete" | "resumer" => "doer",
            _ => {
                let adoptable =
                    artifact_is_adoptable(query, registry, &request.identifier, &kind);
                return Err(BeginError::StateNotReviewable {
                    artifact_kind: kind,
                    state,
                    adoptable,
                });
            }
        };
        let context_text =
            resolve_and_read_hook(query, hook_reader, registry, &kind, &state, hook_role)?;
        let artifact_id = request
            .identifier
            .rsplit('/')
            .next()
            .unwrap_or(&request.identifier);
        let artifact_path = if request
            .identifier
            .starts_with(&format!("{}/", machine.directory))
        {
            request.identifier.clone()
        } else {
            format!("{}/{}", machine.directory, artifact_id)
        };
        let result = if request.session_role == "reviewer" {
            BeginResult {
                track_path: artifact_path.clone(),
                state: state.clone(),
                review_context_text: context_text,
                measurement_kind: kind.clone(),
                measurement_role: "reviewer".to_string(),
                ..Default::default()
            }
        } else {
            BeginResult {
                track_path: artifact_path.clone(),
                state: state.clone(),
                context_text,
                measurement_kind: kind.clone(),
                measurement_role: "doer".to_string(),
                ..Default::default()
            }
        };
        return Ok(BeginOutcome {
            result,
            events: vec![Event::BeginMarkerWritten {
                artifact_path,
                kind: "begin".to_string(),
                actor: request.actor_name.clone(),
                state: state.clone(),
                at: String::new(),
                conversation_id: request.conversation_id.clone(),
            }],
        });
    }

    // Slice A cutover: (track, spec, reviewer) is no longer engine-driven.
    // The spec -> spec_review transition is driven by the doer's complete call.
    if request.session_role == "reviewer" && state == "spec" {
        return Err(BeginError::SpecNotReadyForReview {
            state: "spec".to_string(),
        });
    }

    // Slice B: (track, spec_revision, reviewer) — the track is in revision and
    // not yet submitted for re-review. The reviewer must wait for the doer's
    // complete call. Same error code as the `spec` case, distinct message.
    if request.session_role == "reviewer" && state == "spec_revision" {
        return Err(BeginError::SpecNotReadyForReview {
            state: "spec_revision".to_string(),
        });
    }

    let track_path = format!("tracks/{}", request.identifier);

    if request.session_role == "resumer" {
        if !track_doer_resume_state(&state) {
            let adoptable = artifact_is_adoptable(query, registry, &request.identifier, "track");
            return Err(BeginError::ModeNotImplemented {
                mode: "resumer".to_string(),
                adoptable,
            });
        }
        let preamble =
            resolve_and_read_hook(query, hook_reader, registry, "track", &state, "doer")?;
        // Slice C consumer side (R4): on a `plan`-state track, append the
        // carry-forward findings section to context_text when carry-forward.md
        // is present. Read fresh from disk every call (R4.5 — no cache); the
        // file is never deleted or marked consumed (R4.4) so re-entry re-delivers.
        let context_text =
            append_carry_forward_section(query, &request.identifier, &state, preamble)?;
        return Ok(BeginOutcome {
            result: BeginResult {
                track_path: track_path.clone(),
                state: state.clone(),
                context_text,
                measurement_kind: "track".to_string(),
                measurement_role: "doer".to_string(),
                ..Default::default()
            },
            events: vec![begin_marker_for(track_path, &request, &state)],
        });
    }

    if request.session_role == "doer" || request.session_role == "complete" {
        let context_text = resolve_and_read_hook(
            query,
            hook_reader,
            registry,
            "track",
            &state,
            &request.session_role,
        )?;
        return Ok(BeginOutcome {
            result: BeginResult {
                track_path: track_path.clone(),
                state: state.clone(),
                context_text,
                measurement_kind: "track".to_string(),
                measurement_role: request.session_role.clone(),
                ..Default::default()
            },
            events: vec![begin_marker_for(track_path, &request, &state)],
        });
    }

    if request.session_role != "reviewer" {
        let adoptable = artifact_is_adoptable(query, registry, &request.identifier, "track");
        return Err(BeginError::StateNotReviewable {
            artifact_kind: "track".to_string(),
            state,
            adoptable,
        });
    }

    let Some((artifact_filename, review_doc_name)) = track_review_files_for_state(&state) else {
        let adoptable = artifact_is_adoptable(query, registry, &request.identifier, "track");
        return Err(BeginError::StateNotReviewable {
            artifact_kind: "track".to_string(),
            state,
            adoptable,
        });
    };

    // Resolve the track's human-readable name from the authoritative
    // registry entry rather than a slug heuristic. Per Phase-3 L1 of
    // the review-spec-strand track and the field report in
    // supporting/reviewer_context_gaps.md, the heuristic collides when
    // two tracks share a display name and produces opaque "row not
    // found" errors. Registry lookup keys on the full identifier,
    // which is unique by construction.
    let registry_entry = query
        .read_registry_entry("tracks.md", &request.identifier)
        .map_err(BeginError::from)?;
    let track_name = registry_entry.track_name;

    // Context delivery: read the phase artifact and review context, then
    // idempotently scaffold the phase review doc if absent. No transition is
    // recorded, no actor is seeded, no registry/projection mutations.
    //
    // Serve the playbook-declared (spec_review, reviewer) hook body into
    // review_context_text. The declaration lives on the resolved machine;
    // absence of a declared hook yields an empty review_context_text (AC-2),
    // not an error. Uses the same resolve_and_read_hook helper as the create
    // flow — only the destination field (review_context_text) and (state, role)
    // differ.
    let review_context_text =
        resolve_and_read_hook(query, hook_reader, registry, "track", &state, "reviewer")?;
    let artifact_text = query
        .read_artifact_text(&track_path, artifact_filename)
        .map_err(BeginError::from)?;

    let header = format!("# Review: {}\n\n## Round 1\n", track_name);

    // review_doc_path is populated by the engine layer after routing
    // Event::ReviewDocCreated to ArtifactPort.
    let result = BeginResult {
        track_path: track_path.clone(),
        state: state.clone(),
        context_text: String::new(),
        artifact_text,
        review_context_text,
        review_doc_path: String::new(),
        measurement_kind: "track".to_string(),
        measurement_role: request.session_role.clone(),
        intent: String::new(),
        expected_output: String::new(),
        playbook_id: String::new(),
    };

    // The begin-marker is an additive `activity:` append recording that
    // this actor entered the artifact in this state. It carries an empty
    // `at` — the engine stamps it at routing time (mirroring how
    // ReviewTransition defers `at` stamping). It is NOT a state transition.
    let begin_marker = begin_marker_for(track_path.clone(), &request, &state);

    let review_doc = Event::ReviewDocCreated {
        track_path,
        doc_name: review_doc_name.to_string(),
        header,
    };

    Ok(BeginOutcome {
        result,
        events: vec![review_doc, begin_marker],
    })
}

// ---------------------------------------------------------------------------
// K8 typed genesis (plan Task 5).
// ---------------------------------------------------------------------------

/// Decode, validate, and mint one K8 genesis. Every engine-owned field
/// (`backlog_item_id`, `state`, `rank`, `execution_binding`, `outcome_binding`,
/// `exit`, `history`) is ABSENT from the closed `BacklogGenesisInput`, so
/// `deny_unknown_fields` rejects a caller attempt to supply one before a byte
/// is written. Raw JSON never reaches `status.fields`.
pub fn build_backlog_genesis(
    item_json: &str,
    actor: &crate::domain::shared_types::ActorIdentity,
    at: &str,
) -> Result<(crate::domain::backlog_item::BacklogItem, crate::domain::backlog_item::HistoryEntry), BeginError>
{
    use crate::domain::backlog_item as k8;
    let input: k8::BacklogGenesisInput =
        serde_json::from_str(item_json).map_err(|e| {
            let detail = e.to_string();
            // Name the engine-owned key explicitly so the caller learns WHY,
            // rather than seeing a bare serde message.
            let engine_owned = [
                "backlog_item_id",
                "state",
                "rank",
                "execution_binding",
                "outcome_binding",
                "exit",
                "history",
            ]
            .iter()
            .find(|k| detail.contains(*k));
            match engine_owned {
                Some(key) => BeginError::Checkin(CheckinError::MissingRequiredField {
                    field: format!(
                        "item: `{key}` is engine_owned and may not be supplied by a caller"
                    ),
                }),
                None => BeginError::Checkin(CheckinError::MissingRequiredField {
                    field: format!("item: {detail}"),
                }),
            }
        })?;

    k8::validate_genesis_input(&input).map_err(|e| {
        BeginError::Checkin(CheckinError::MissingRequiredField {
            field: format!("item: {e}"),
        })
    })?;

    let item = k8::BacklogItem {
        backlog_item_id: k8::mint_backlog_item_id(),
        business_node_id: input.business_node_id,
        title: input.title,
        description: input.description,
        action_class: input.action_class,
        effort_class: input.effort_class,
        state: k8::State::Candidate,
        intake: input.intake,
        rank: None,
        playbook_binding: input.playbook_binding,
        origin_binding: input.origin_binding,
        execution_binding: None,
        outcome_binding: None,
        exit: None,
    };
    k8::validate_item_semantics(&item).map_err(|e| {
        BeginError::Checkin(CheckinError::MissingRequiredField {
            field: format!("item: {e}"),
        })
    })?;
    k8::validate_required_by_state(&item, k8::State::Candidate).map_err(|e| {
        BeginError::Checkin(CheckinError::MissingRequiredField {
            field: format!("item: {e}"),
        })
    })?;

    // The ONLY history entry genesis writes. Never a genesis `state_change`.
    let created = k8::HistoryEntry {
        seq: 0,
        actor: actor.name.clone(),
        role: k8::DriverRole::Intake,
        at: at.to_string(),
        kind: k8::HistoryKind::Created,
        from_state: None,
        to_state: Some(k8::State::Candidate),
        payload: None,
        note: None,
    };
    Ok((item, created))
}

/// The exact `status.yaml` bytes a freshly published K8 item carries.
pub fn render_backlog_genesis_status(
    item: &crate::domain::backlog_item::BacklogItem,
    actor: &crate::domain::shared_types::ActorIdentity,
) -> String {
    format!(
        "version: 1\nkind: backlog_item\nstate: candidate\nid: {}\nactors:\n  {}:\n    type: {}\n    configurations:\n      - at: \"{}\"\n        model: {}\n        provider: {}\n        details:\n          context_window: {}\n          sdk_version: \"{}\"\n          entrypoint: {}\n",
        item.backlog_item_id,
        actor.name,
        actor.actor_type,
        actor.registered_at,
        actor.model,
        actor.provider,
        actor.context_window,
        actor.sdk_version,
        actor.entrypoint,
    )
}

fn handle_create_backlog_item(request: BeginRequest) -> Result<BeginOutcome, BeginError> {
    // Exactly `create_fields["item"]` — no other generic field participates.
    let item_json = request.fields.get("item").ok_or_else(|| {
        BeginError::Checkin(CheckinError::MissingRequiredField {
            field: "item".to_string(),
        })
    })?;
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let actor = ActorIdentity {
        name: request.actor_name.clone(),
        actor_type: request.actor_type.clone(),
        model: request.actor_model.clone(),
        provider: request.actor_provider.clone(),
        context_window: request.actor_context_window,
        sdk_version: request.actor_sdk_version.clone(),
        entrypoint: request.actor_entrypoint.clone(),
        registered_at: now.clone(),
    };
    let (item, created) = build_backlog_genesis(item_json, &actor, &now)?;
    let status_bytes = render_backlog_genesis_status(&item, &actor);
    let result = BeginResult {
        state: "candidate".to_string(),
        ..Default::default()
    };
    Ok(BeginOutcome {
        result,
        events: vec![Event::BacklogItemCreation {
            item: Box::new(item),
            created,
            actor,
            status_bytes,
            conversation_id: request.conversation_id.clone(),
        }],
    })
}

// ===========================================================================
// artifact_of_record — the ONE authority for "which file, and what does it look
// like when freshly scaffolded" (transition_carries_step_evidence_status,
// plan phase 1).
//
// Derived from `initial_scaffold_files`, never copied beside it. A copy is how a
// byte comparison silently stops matching after someone edits the bootstrap
// format: the check keeps passing while seeing nothing, which is precisely the
// class of failure the sibling track exists to detect.
// ===========================================================================

/// The artifact of record for a kind's INITIAL state, with the exact bytes
/// bootstrap writes for it.
///
/// `Some` only when the kind scaffolds exactly ONE principal artifact. Multi-file
/// kinds return `None` — choosing which of `research.md` / `proposal.md` /
/// `plan.md` / `draft.md` is "the" artifact for a generation workflow is a policy
/// decision, and inventing it inside an implementation task is how undocumented
/// conventions get born. Non-initial states return `None` too: this seam is
/// honest about covering only the state bootstrap actually writes.
///
/// `display_name` is required because every scaffold body embeds it. Without it
/// the function cannot produce the byte-identical placeholder the whole
/// assessment rests on.
pub fn artifact_of_record(
    kind: &str,
    state: &str,
    display_name: &str,
) -> Option<(String, Vec<u8>)> {
    if state != initial_state_for_kind(kind)? {
        return None;
    }
    let mut files = initial_scaffold_files(kind, display_name);
    // Ancillary empty files (machine.yaml, amendments.md) are scaffolded too;
    // "exactly one principal artifact" means exactly one with CONTENT.
    files.retain(|(_, body)| !body.is_empty());
    if files.len() != 1 {
        return None;
    }
    let (path, body) = files.remove(0);
    Some((path, body.into_bytes()))
}

/// The state a freshly-begun artifact of this kind lands in. Only kinds whose
/// initial state is unambiguous are declared; everything else declines, which
/// keeps `artifact_of_record` from guessing.
fn initial_state_for_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "track" => Some("spec"),
        _ => None,
    }
}

// ===========================================================================
// Transition evidence assessment (transition_carries_step_evidence_status,
// plan phase 3). Two independent questions, deliberately kept apart:
//   1. what was CLAIMED on the call
//   2. what the artifact of record LOOKED LIKE on disk
// The consumer's predicate is their conjunction, and collapsing them into one
// value is what made four spec revisions unable to express it.
// ===========================================================================

/// What the call claimed. Ordered, first match wins.
///
/// `self_described` exists because `SelfDescription` is the `#[default]`
/// evidence class, documented as "the weakest evidence: the actor's own
/// description of what they did". Folding it into `claimed` would let the
/// DEFAULT VALUE certify the work.
pub fn classify_claim(
    is_begin: bool,
    has_artifact_of_record: bool,
    classes: &[EvidenceClass],
) -> &'static str {
    // 1. A begin is entering the state, not leaving it — the actor cannot have
    //    authored the artifact of a state they are only now arriving in. An
    //    earlier design assessed the state being ENTERED and would have accused
    //    every legitimate new run at creation.
    if is_begin {
        return "pending";
    }
    // 2. Nothing to have produced.
    if !has_artifact_of_record {
        return "not_applicable";
    }
    // 3. Any strong claim wins over a mixed set.
    if classes
        .iter()
        .any(|c| matches!(c, EvidenceClass::ArtifactOfConsequence | EvidenceClass::VerifiableCitation))
    {
        return "claimed";
    }
    // 4. Claims exist but every one is the weakest kind.
    if !classes.is_empty() {
        return "self_described";
    }
    // 5. No claim at all. The ONLY state that can warn — and only in
    //    conjunction with the artifact assessment below.
    "unclaimed"
}

/// What the artifact of record looked like. `missing` — never `absent`, which
/// is in the downstream scorer's FORBIDDEN_VALUES.
pub fn classify_artifact(
    artifact: Option<&ArtifactOnDisk>,
) -> &'static str {
    match artifact {
        None => "not_applicable",
        Some(ArtifactOnDisk::Missing) => "missing",
        Some(ArtifactOnDisk::Placeholder) => "placeholder",
        Some(ArtifactOnDisk::Substantive) => "substantive",
    }
}

/// The on-disk reading, kept as a type so the caller cannot pass a bare bool and
/// silently lose the missing/placeholder distinction — they warn alike but they
/// are different facts and the sweep reports them separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactOnDisk {
    Missing,
    Placeholder,
    Substantive,
}

/// Compare a state's artifact against the bytes bootstrap would have written.
/// A byte comparison, derived from the writer via `artifact_of_record` — not a
/// heuristic about length or emptiness, which is what makes it survive an author
/// who writes a genuinely short spec.
pub fn read_artifact_on_disk(
    artifact_dir: &std::path::Path,
    kind: &str,
    state: &str,
    display_name: &str,
) -> Option<ArtifactOnDisk> {
    let (rel, placeholder) = artifact_of_record(kind, state, display_name)?;
    let path = artifact_dir.join(rel);
    match std::fs::read(&path) {
        Err(_) => Some(ArtifactOnDisk::Missing),
        Ok(bytes) if bytes == placeholder => Some(ArtifactOnDisk::Placeholder),
        Ok(_) => Some(ArtifactOnDisk::Substantive),
    }
}

/// The two-condition warning gate. Both, never one — that is what separates the
/// 8 genuine abandonments from the 73 unclaimed-but-real completions measured on
/// the live fleet, a ~9:1 false-positive rate if either half were dropped.
///
/// `self_described` never warns: a weak claim is not an absent one, and warning
/// on it would recreate the false-positive population in a new place.
pub fn warns(claim_status: &str, artifact_assessment: &str) -> bool {
    claim_status == "unclaimed"
        && matches!(artifact_assessment, "placeholder" | "missing")
}

/// Whether `bytes` is an UNTOUCHED bootstrap scaffold for this kind+state,
/// without needing the original display name.
///
/// The display name is not available at the transition emit site (status.yaml
/// does not carry it, and reading the registry from there would be a heavy,
/// fragile dependency). So instead of comparing against ONE expected string,
/// this tests membership in the IMAGE of the bootstrap function: render the
/// template with a sentinel, split it at the sentinel, and a file matches iff it
/// has that exact prefix and suffix.
///
/// That is not a heuristic about length or emptiness — it is derived from the
/// writer, so a change to the bootstrap format changes this predicate with it.
/// A genuinely short spec someone actually wrote ("# Foo\n\nDo the thing.\n")
/// fails the suffix check and reads as substantive, which is the case that must
/// never warn.
pub fn is_bootstrap_placeholder(kind: &str, state: &str, bytes: &[u8]) -> bool {
    const SENTINEL: &str = "\u{1}ANVIL_PLACEHOLDER_SENTINEL\u{1}";
    let Some((_, template)) = artifact_of_record(kind, state, SENTINEL) else {
        return false;
    };
    let template = String::from_utf8_lossy(&template).to_string();
    let Some(idx) = template.find(SENTINEL) else {
        // A template that does not embed the display name is a fixed string:
        // compare it whole.
        return bytes == template.as_bytes();
    };
    let (prefix, rest) = template.split_at(idx);
    let suffix = &rest[SENTINEL.len()..];
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    text.len() >= prefix.len() + suffix.len()
        && text.starts_with(prefix)
        && text.ends_with(suffix)
        // The body between prefix and suffix is the display name, which is one
        // line. A multi-line body means somebody wrote something.
        && !text[prefix.len()..text.len() - suffix.len()].contains('\n')
}

/// Read a state's artifact of record and classify it, using the shape-derived
/// placeholder predicate so no display name is needed at the call site.
pub fn read_artifact_on_disk_by_shape(
    artifact_dir: &std::path::Path,
    kind: &str,
    state: &str,
) -> Option<ArtifactOnDisk> {
    const SENTINEL: &str = "\u{1}ANVIL_PLACEHOLDER_SENTINEL\u{1}";
    let (rel, _) = artifact_of_record(kind, state, SENTINEL)?;
    let path = artifact_dir.join(rel);
    match std::fs::read(&path) {
        Err(_) => Some(ArtifactOnDisk::Missing),
        Ok(bytes) if is_bootstrap_placeholder(kind, state, &bytes) => {
            Some(ArtifactOnDisk::Placeholder)
        }
        Ok(_) => Some(ArtifactOnDisk::Substantive),
    }
}
