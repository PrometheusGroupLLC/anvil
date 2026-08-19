//! `AmendCommandHandler` — the pure CQRS handler for the B5b Amend surface.
//!
//! Mirrors `complete.rs::CompleteCommandHandler::execute`: a pure function that
//! reads via `QueryPort` (+ a `&dyn PlaybookRegistry`), validates the candidate
//! op, and emits `Vec<AmendEvent>`. No I/O; the engine routes the events to
//! ports (BP3).
//!
//! ## Two kind namespaces (HIGH-3)
//!
//! - `request.kind` is the AMENDMENT kind (one of the 8 content schemas) — used
//!   for `schema_for_kind` content validation.
//! - `query.read_artifact_kind(artifact_path)` is the artifact's stored
//!   LIFECYCLE kind — used for the machine lookup that drives state.
//! - `query.read_artifact_state(artifact_path)` is the current state — used for
//!   the `completed`-check + `amend` idempotency (drives transitions in BP4).
//!
//! ## Op-log-only validation (D-1, Q-B)
//!
//! The base is an EMPTY `ArtifactDocument { kind: request.kind, elements: vec![] }`.
//! The persisted op log is replayed in `ordered()` order, the candidate op is
//! pushed, and `apply(schema, &base, &log)` re-validates the whole log (AC-2
//! atomicity). An op targeting an element never introduced by a prior `Add` in
//! the same log yields `amendment_unknown_element`; an op outside the schema
//! yields the matching B5a typed code.

use crate::domain::amend_events::AmendEvent;
use crate::domain::amendment::{
    apply, schema_for_kind, AddAnchor, AmendmentError, AmendmentOp, ArtifactDocument, OpKind,
};
use crate::domain::playbook::interpreter::outgoing_transitions;
use crate::domain::playbook::registry::PlaybookRegistry;
use crate::domain::shared_types::ActorIdentity;
use crate::ports::query_port::{QueryError, QueryPort};
use std::fmt;

/// Errors returned by `AmendCommandHandler::execute`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AmendError {
    ArtifactPathRequired,
    TargetDocumentRequired,
    ActorNameRequired,
    ActorParamsRequired {
        field: String,
    },
    /// `request.kind` is not one of the 8 amendment kinds.
    UnknownAmendmentKind {
        kind: String,
    },
    /// `op_kind` could not be parsed to a known `OpKind`.
    UnknownOpKind {
        op_kind: String,
    },
    /// `anchor` could not be parsed to a known `AddAnchor`.
    UnknownAnchor {
        anchor: String,
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
    /// A B5a schema/application error, wrapped so its `code()` propagates.
    Amendment(AmendmentError),
}

impl AmendError {
    /// The stable snake_case error code for this variant. For `Amendment`,
    /// delegates to the wrapped B5a code.
    pub fn code(&self) -> &str {
        match self {
            AmendError::ArtifactPathRequired => "artifact_path_required",
            AmendError::TargetDocumentRequired => "target_document_required",
            AmendError::ActorNameRequired => "actor_name_required",
            AmendError::ActorParamsRequired { .. } => "actor_params_required",
            AmendError::UnknownAmendmentKind { .. } => "amend_unknown_amendment_kind",
            AmendError::UnknownOpKind { .. } => "amend_unknown_op_kind",
            AmendError::UnknownAnchor { .. } => "amend_unknown_anchor",
            AmendError::NotFound { .. } => "not_found",
            AmendError::MalformedStatus { .. } => "malformed_status",
            AmendError::IoError { .. } => "io_error",
            AmendError::Amendment(e) => e.code(),
        }
    }
}

impl fmt::Display for AmendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AmendError::ArtifactPathRequired => {
                write!(f, "artifact_path_required: artifact_path must not be empty")
            }
            AmendError::TargetDocumentRequired => write!(
                f,
                "target_document_required: target_document must not be empty"
            ),
            AmendError::ActorNameRequired => {
                write!(f, "actor_name_required: actor_name must not be empty")
            }
            AmendError::ActorParamsRequired { field } => {
                write!(f, "actor_params_required: {} must not be empty", field)
            }
            AmendError::UnknownAmendmentKind { kind } => write!(
                f,
                "amend_unknown_amendment_kind: '{}' is not one of the 8 amendment kinds",
                kind
            ),
            AmendError::UnknownOpKind { op_kind } => write!(
                f,
                "amend_unknown_op_kind: '{}' is not a recognized op_kind",
                op_kind
            ),
            AmendError::UnknownAnchor { anchor } => write!(
                f,
                "amend_unknown_anchor: '{}' is not a recognized anchor",
                anchor
            ),
            AmendError::NotFound { artifact_path } => {
                write!(f, "Artifact '{}' not found in hearth", artifact_path)
            }
            AmendError::MalformedStatus {
                artifact_path,
                message,
            } => write!(
                f,
                "Malformed status.yaml for '{}': {}",
                artifact_path, message
            ),
            AmendError::IoError { message } => write!(f, "I/O error: {}", message),
            AmendError::Amendment(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for AmendError {}

/// Engine-stamped request to amend an artifact's document op log.
#[derive(Debug, Clone, Default)]
pub struct AmendRequest {
    /// Relative path to the artifact (e.g. `tracks/<id>`).
    pub artifact_path: String,
    /// The AMENDMENT kind (one of 8) — used for `schema_for_kind`.
    pub kind: String,
    /// The frozen document being amended (e.g. `"spec"`, `"plan"`).
    pub target_document: String,
    /// The element id the op targets (or mints, for Add).
    pub target_id: String,
    /// The op kind string (`add`/`revise`/`retire`/`reorder`).
    pub op_kind: String,
    /// The op body payload (empty → None).
    pub body: String,
    /// The new element kind for an Add (empty → None).
    pub new_kind: String,
    /// The positioning anchor (empty → None).
    pub anchor: String,
    pub actor_name: String,
    pub actor_type: String,
    pub actor_model: String,
    pub actor_provider: String,
    pub actor_context_window: i64,
    pub actor_sdk_version: String,
    pub actor_entrypoint: String,
    /// Engine-stamped accept timestamp (like complete's `chrono::Utc::now()`).
    pub at: String,
}

/// The pure result summary of an Amend call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AmendResult {
    /// The recorded op's stable id.
    pub op_id: String,
    /// Populated only when a state transition was driven (BP4); `None` otherwise.
    pub new_state: Option<String>,
}

/// The pure outcome of an Amend call: a result summary + the events to route.
#[derive(Debug, Clone)]
pub struct AmendOutcome {
    pub result: AmendResult,
    pub events: Vec<AmendEvent>,
}

pub struct AmendCommandHandler;

/// Compute the op_id: `op-{compact_at}-{seq}` where `compact_at` strips `-` and
/// `:` from the ISO timestamp (keeping `T` and the `Z` suffix), and `seq` is the
/// 0-indexed position in the op log before the push. Example: at
/// `2026-06-04T21:15:00Z` with seq 0 → `op-20260604T211500Z-0`.
pub fn compute_op_id(at: &str, seq: u64) -> String {
    let compact = at.replace(['-', ':'], "");
    format!("op-{}-{}", compact, seq)
}

impl AmendCommandHandler {
    pub fn execute(
        query: &dyn QueryPort,
        registry: &dyn PlaybookRegistry,
        request: AmendRequest,
    ) -> Result<AmendOutcome, AmendError> {
        // ---- 1. Argument validation (mirror complete) ----------------------
        if request.artifact_path.is_empty() {
            return Err(AmendError::ArtifactPathRequired);
        }
        if request.target_document.is_empty() {
            return Err(AmendError::TargetDocumentRequired);
        }
        if request.actor_name.is_empty() {
            return Err(AmendError::ActorNameRequired);
        }
        for (field, value) in [
            ("actor_type", &request.actor_type),
            ("actor_model", &request.actor_model),
            ("actor_provider", &request.actor_provider),
        ] {
            if value.is_empty() {
                return Err(AmendError::ActorParamsRequired {
                    field: field.to_string(),
                });
            }
        }

        // ---- 2. Resolve the content schema (amendment kind namespace) -------
        let schema = schema_for_kind(&request.kind).ok_or(AmendError::UnknownAmendmentKind {
            kind: request.kind.clone(),
        })?;

        // ---- 3. Read lifecycle kind + current state (machine namespace) -----
        let lifecycle_kind = query
            .read_artifact_kind(&request.artifact_path)
            .map_err(map_query_error)?;
        let current_state = query
            .read_artifact_state(&request.artifact_path)
            .map_err(map_query_error)?;

        // K8 bypass closure (plan Task 6): `Amend` is NOT a backlog writer.
        // Rejected before its op log so no amendment op is ever recorded
        // against a backlog_item.
        if lifecycle_kind == "backlog_item" {
            return Err(AmendError::MalformedStatus {
                artifact_path: request.artifact_path.clone(),
                message: format!(
                    "a backlog_item (state '{current_state}') is never amended: post-genesis \
                     K8 state moves only through a prepared K8 transition"
                ),
            });
        }

        // ---- 4. Build the candidate op -------------------------------------
        let op_kind = parse_op_kind(&request.op_kind)?;
        let anchor = parse_anchor(&request.anchor)?;
        let candidate = AmendmentOp {
            target_id: request.target_id.clone(),
            kind: op_kind,
            body: if request.body.is_empty() {
                None
            } else {
                Some(request.body.clone())
            },
            new_kind: if request.new_kind.is_empty() {
                None
            } else {
                Some(request.new_kind.clone())
            },
            anchor,
        };

        // ---- 5. Q-B whole-log re-validation --------------------------------
        // Empty base + replay the persisted log + push the candidate, then call
        // apply (validate-all-then-apply atomicity).
        let base = ArtifactDocument {
            kind: request.kind.clone(),
            elements: vec![],
        };
        let mut log = query
            .read_op_log(&request.artifact_path, &request.target_document)
            .map_err(map_query_error)?;
        let seq = log.len() as u64;
        let op_id = compute_op_id(&request.at, seq);
        log.push(op_id.clone(), request.at.clone(), candidate);
        apply(schema, &base, &log).map_err(AmendError::Amendment)?;

        // The recorded entry is the last one pushed (carries the stamped fields).
        let entry = log
            .entries()
            .last()
            .expect("log has at least the candidate op")
            .clone();

        // ---- 6. Emit events ------------------------------------------------
        let identity = ActorIdentity {
            name: request.actor_name.clone(),
            actor_type: request.actor_type.clone(),
            model: request.actor_model.clone(),
            provider: request.actor_provider.clone(),
            context_window: request.actor_context_window,
            sdk_version: request.actor_sdk_version.clone(),
            entrypoint: request.actor_entrypoint.clone(),
            registered_at: request.at.clone(),
        };
        let mut events: Vec<AmendEvent> = Vec::new();
        events.push(AmendEvent::OpRecorded {
            artifact_path: request.artifact_path.clone(),
            target_document: request.target_document.clone(),
            entry,
        });
        events.push(AmendEvent::ActorUpserted {
            artifact_path: request.artifact_path.clone(),
            identity,
        });

        // ---- 7. State-driving check (machine namespace; BP4 drives track) ---
        // Emit TransitionRecorded only when the artifact's lifecycle-kind machine
        // declares a `<current_state> → amend` edge AND the artifact is in that
        // source state. Idempotent: already-in-`amend` records the op only.
        let new_state = if current_state == "amend" {
            // Already amending: record op only, echo the current state.
            Some("amend".to_string())
        } else if let Some(machine) = registry.machine_for(&lifecycle_kind) {
            if outgoing_transitions(machine, &current_state)
                .iter()
                .any(|t| t.to_state == "amend")
            {
                events.push(AmendEvent::TransitionRecorded {
                    artifact_path: request.artifact_path.clone(),
                    to_state: "amend".to_string(),
                    at: request.at.clone(),
                    role: "doer".to_string(),
                    actor_name: request.actor_name.clone(),
                });
                Some("amend".to_string())
            } else {
                None
            }
        } else {
            None
        };

        Ok(AmendOutcome {
            result: AmendResult { op_id, new_state },
            events,
        })
    }
}

fn parse_op_kind(s: &str) -> Result<OpKind, AmendError> {
    match s {
        "add" => Ok(OpKind::Add),
        "revise" => Ok(OpKind::Revise),
        "retire" => Ok(OpKind::Retire),
        "reorder" => Ok(OpKind::Reorder),
        other => Err(AmendError::UnknownOpKind {
            op_kind: other.to_string(),
        }),
    }
}

/// Parse an anchor string. Empty → `None` (no anchor). Recognized forms:
/// `at_start`, `at_end`, `after:<id>`, `before:<id>`.
fn parse_anchor(s: &str) -> Result<Option<AddAnchor>, AmendError> {
    if s.is_empty() {
        return Ok(None);
    }
    if s == "at_start" {
        return Ok(Some(AddAnchor::AtStart));
    }
    if s == "at_end" {
        return Ok(Some(AddAnchor::AtEnd));
    }
    if let Some(id) = s.strip_prefix("after:") {
        return Ok(Some(AddAnchor::After(id.to_string())));
    }
    if let Some(id) = s.strip_prefix("before:") {
        return Ok(Some(AddAnchor::Before(id.to_string())));
    }
    Err(AmendError::UnknownAnchor {
        anchor: s.to_string(),
    })
}

fn map_query_error(e: QueryError) -> AmendError {
    match e {
        QueryError::NotFound { artifact_id } => AmendError::NotFound {
            artifact_path: artifact_id,
        },
        QueryError::MalformedStatus {
            artifact_id,
            message,
        } => AmendError::MalformedStatus {
            artifact_path: artifact_id,
            message,
        },
        QueryError::IoError { message } => AmendError::IoError { message },
        QueryError::ProjectionRowNotFound { message } => AmendError::IoError { message },
        QueryError::ProjectionRowAmbiguous { message } => AmendError::IoError { message },
        // The amend path never issues a strict adoption read; surface it as an
        // I/O failure with the detail preserved to keep the match exhaustive.
        QueryError::AdoptionEvidenceUnreadable {
            artifact_id,
            detail,
        } => AmendError::IoError {
            message: format!("adoption evidence unreadable for '{}': {}", artifact_id, detail),
        },
    }
}
