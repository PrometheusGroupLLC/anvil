//! Shared protobuf-to-domain mapping for lifecycle requests carrying evidence.
//!
//! All public lifecycle RPC boundaries use these mappers. References remain
//! opaque strings: only the class token is interpreted, while the reference is
//! copied verbatim and never included in mapping errors.

use anvil_core::domain::begin::BeginRequest as DomainBeginRequest;
use anvil_core::domain::complete::CompleteRequest as DomainCompleteRequest;
use anvil_core::domain::playbook::types::EvidenceClass;
use anvil_core::domain::shared_types::{ClaimedEvidence as DomainClaimedEvidence, RequestContext};
use anvil_core::domain::snapshot::SnapshotRequest as DomainSnapshotRequest;

use crate::proto::{
    BeginRequest as ProtoBeginRequest, ClaimedEvidence as ProtoClaimedEvidence,
    CompleteRequest as ProtoCompleteRequest, SnapshotRequest as ProtoSnapshotRequest,
};

/// One mapped lifecycle request plus the typed claims retained for the
/// post-handler evidence emitter introduced in P2.
#[derive(Debug, Clone)]
pub struct MappedLifecycleRequest<T> {
    pub domain_request: T,
    pub claimed_evidence: Vec<DomainClaimedEvidence>,
}

/// Map ordered protobuf claims to the typed domain carrier.
pub fn claimed_evidence_to_domain(
    claims: Vec<ProtoClaimedEvidence>,
) -> Result<Vec<DomainClaimedEvidence>, String> {
    claims
        .into_iter()
        .map(|claim| {
            let class = match claim.class.as_str() {
                "artifact_of_consequence" => EvidenceClass::ArtifactOfConsequence,
                "verifiable_citation" => EvidenceClass::VerifiableCitation,
                "self_description" => EvidenceClass::SelfDescription,
                unknown => {
                    return Err(format!(
                        "claimed_evidence_class_unknown: '{}' is not an EvidenceClass token",
                        unknown
                    ))
                }
            };
            Ok(DomainClaimedEvidence {
                class,
                reference: claim.reference,
            })
        })
        .collect()
}

/// Map a public Begin RPC request to its domain request while retaining an
/// ordered typed copy of the claims for the later emitter.
pub fn begin_request_to_domain(
    request: ProtoBeginRequest,
    ctx: RequestContext,
    actor_name: String,
) -> Result<MappedLifecycleRequest<DomainBeginRequest>, String> {
    let claimed_evidence = claimed_evidence_to_domain(request.claimed_evidence)?;
    let domain_request = DomainBeginRequest {
        ctx,
        artifact_type: request.artifact_type,
        parent_id: request.parent_id,
        track_name: request.track_name,
        playbook_name: request.playbook_name,
        target_owner: request.target_owner,
        fields: request.create_fields.into_iter().collect(),
        approver: request.approver,
        actor_name,
        actor_type: request.actor_type,
        actor_model: request.actor_model,
        actor_provider: request.actor_provider,
        actor_context_window: request.actor_context_window,
        actor_sdk_version: request.actor_sdk_version,
        actor_entrypoint: request.actor_entrypoint,
        identifier: request.identifier,
        session_role: request.session_role,
        adopt: request.adopt,
        conversation_id: request.conversation_id,
        rd_turn_id: request.rd_turn_id,
        rd_input: request.rd_input,
        rd_candidate_set: request.rd_candidate_set,
        rd_selected: request.rd_selected,
        rd_confidence: request.rd_confidence,
        claimed_evidence: claimed_evidence.clone(),
    };
    Ok(MappedLifecycleRequest {
        domain_request,
        claimed_evidence,
    })
}

/// Map a public Snapshot RPC request to its domain request while retaining an
/// ordered typed copy of the claims for the later emitter.
pub fn snapshot_request_to_domain(
    request: ProtoSnapshotRequest,
    actor_name: String,
    at: String,
) -> Result<MappedLifecycleRequest<DomainSnapshotRequest>, String> {
    let claimed_evidence = claimed_evidence_to_domain(request.claimed_evidence)?;
    let domain_request = DomainSnapshotRequest {
        artifact_path: request.artifact_path,
        to_state: request.to_state,
        actor_name,
        actor_role: request.actor_role,
        approver: request.approver,
        note: request.note,
        actor_type: request.actor_type,
        actor_model: request.actor_model,
        actor_provider: request.actor_provider,
        actor_context_window: request.actor_context_window,
        actor_sdk_version: request.actor_sdk_version,
        actor_entrypoint: request.actor_entrypoint,
        projection_only: request.projection_only,
        event_type: request.event_type,
        // Public snapshot RPC: a proto client cannot set this. A forged
        // reserved event_type (for example, `adoption`) remains rejected by
        // the domain handler.
        allow_reserved_event_type: false,
        at,
        claimed_evidence: claimed_evidence.clone(),
    };
    Ok(MappedLifecycleRequest {
        domain_request,
        claimed_evidence,
    })
}

/// Map a public Complete RPC request to its domain request while retaining an
/// ordered typed copy of the claims for the later emitter.
pub fn complete_request_to_domain(
    request: ProtoCompleteRequest,
    actor_name: String,
    at: String,
) -> Result<MappedLifecycleRequest<DomainCompleteRequest>, String> {
    let claimed_evidence = claimed_evidence_to_domain(request.claimed_evidence)?;
    let domain_request = DomainCompleteRequest {
        artifact_path: request.artifact_path,
        actor_name,
        actor_type: request.actor_type,
        actor_model: request.actor_model,
        actor_provider: request.actor_provider,
        actor_context_window: request.actor_context_window,
        actor_sdk_version: request.actor_sdk_version,
        actor_entrypoint: request.actor_entrypoint,
        satisfaction: request.satisfaction,
        approver: request.approver,
        note: request.note,
        at,
        reflection_notes: request.reflection_notes,
        findings: request.findings,
        claimed_evidence: claimed_evidence.clone(),
    };
    Ok(MappedLifecycleRequest {
        domain_request,
        claimed_evidence,
    })
}
