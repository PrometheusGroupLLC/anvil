use anvil_core::domain::actor_activity::{fold_actor_activity, fold_actor_activity_across_hearths};
use anvil_core::domain::amend::{
    AmendCommandHandler, AmendError, AmendRequest as DomainAmendRequest,
};
use anvil_core::domain::begin::{
    BeginCommandHandler, BeginError, BeginRequest as DomainBeginRequest, PlaybookHookBodyPort,
};
use anvil_core::domain::begin_adoption::{has_open_begin, open_begin_conversation_id_for_artifact};
use anvil_core::domain::catalog::CatalogQueryHandler;
use anvil_core::domain::checkin::{CheckinError, CheckinQueryHandler, CheckinQueryRequest};
use anvil_core::domain::complete::{CompleteCommandHandler, CompleteError};
use anvil_core::domain::describe::{
    DescribeError, DescribeQueryHandler, DescribeRequest as DomainDescribeRequest, DescribeResult,
};
use anvil_core::domain::events::Event;
use anvil_core::domain::hook_manifest::fold_hook_manifest;
use anvil_core::domain::hooks::route_turn::{CandidateBrief, InProgressSignal, RouterVerdict};
use anvil_core::domain::merge_check::{
    arms_merge_check, classify_reference, merge_check_verdict, CodeClaim,
};
use anvil_core::domain::persist_playbook::{
    PersistPlaybookCommandHandler, PersistPlaybookError,
    PersistPlaybookRequest as DomainPersistPlaybookRequest,
};
use anvil_core::domain::playbook::candidate::{
    CandidatePlaybook as DomainCandidatePlaybook, ProposedState as DomainProposedState,
};
use anvil_core::domain::playbook::composite_registry::CompositePlaybookRegistry;
use anvil_core::domain::playbook::evidence_obligation::{
    assess_evidence_obligation, obligation_satisfied,
};
use anvil_core::domain::playbook::generate::{
    generate as generate_candidate_playbook,
    generate_enforcing as generate_candidate_playbook_enforcing,
};
use anvil_core::domain::playbook::hearth_registry::HearthPlaybookRegistry;
use anvil_core::domain::playbook::hook_serve::serve_hook_body;
use anvil_core::domain::playbook::load_error::PlaybookLoadError;
use anvil_core::domain::playbook::loader::{validate_evidence_obligation, LoaderEnforcement};
use anvil_core::domain::playbook::registry::{
    resolve_route, PlaybookRegistry, PlaybookSource, RouteOutcome, SeedPlaybookRegistry,
};
use anvil_core::domain::playbook::types::{PlaybookMachine, Role, Sensitivity};
use anvil_core::domain::route::{
    abandon_action_for, advance_action_for, classify_call_state, CallState,
    find_open_playbook_run_for_conversation, find_open_playbook_run_indexed, is_continuation_token,
    open_playbook_run_is_relevant, OpenPlaybookRun, CANDIDATE_PLAYBOOK_INTAKE,
};
use anvil_core::domain::route_response::{annotate_candidates, apply_budget, single_guidance};
use anvil_core::domain::shared_types::ActivityEntry;
use anvil_core::domain::shared_types::{ActorIdentity, ClaimedEvidence, RequestContext};
use anvil_core::domain::snapshot::{
    SnapshotCommandHandler, SnapshotError, SnapshotRequest as DomainSnapshotRequest,
};
use anvil_core::domain::status::FullStatusYaml;
use anvil_core::domain::telemetry_salt::UNKNOWN_CONVERSATION_HASH;
use anvil_core::domain::usage_timeseries::{fold_playbook_step_volume, Granularity};
use anvil_core::domain::artifact_activity::ArtifactActivityQuery;
use anvil_core_hearth::fs_activity_log_adapter::FileSystemActivityLogAdapter;
use anvil_core_hearth::fs_activity_write_adapter::FileSystemActivityWriteAdapter;
use anvil_core_hearth::fs_actor_write_adapter::FileSystemActorWriteAdapter;
use anvil_core_hearth::fs_artifact_adapter::FileSystemArtifactAdapter;
use anvil_core_hearth::fs_checkin_query_adapter::FileSystemCheckinQueryAdapter;
use anvil_core_hearth::fs_describe_adapter::FileSystemDescribeAdapter;
use anvil_core_hearth::fs_hearth_reader::FileSystemHearthReader;
use anvil_core_hearth::fs_owner_resolver::FileSystemOwnerResolver;
use anvil_core_hearth::fs_query_adapter::FileSystemQueryAdapter;
use anvil_core_hearth::fs_reflection_write_adapter::FileSystemReflectionWriteAdapter;
use anvil_core_hearth::fs_review_verdict_adapter::FileSystemReviewVerdictAdapter;
use anvil_core_hearth::fs_routing_activity_adapter::FileSystemRoutingActivityAdapter;
use anvil_core_hearth::fs_backlog_item_adapter::FileSystemBacklogItemAdapter;
use anvil_core_hearth::fs_snapshot_adapter::FileSystemSnapshotAdapter;
use anvil_core_hearth::fs_step_measurement_adapter::FileSystemStepMeasurementAdapter;
use anvil_core_hearth::fs_transition_measurement_adapter::FileSystemTransitionMeasurementAdapter;
use anvil_core_hearth::fs_playbook_measurement_adapter::FileSystemPlaybookMeasurementAdapter;
use anvil_core_hearth::hearth_locks::HearthLocks;
use anvil_core::ports::activity_log_port::{
    ActivityLogReadPort, ActivityLogRecord, ActivityLogWritePort,
};
use anvil_core::ports::artifact_port::{ArtifactError, ArtifactPort};
use anvil_core::ports::query_port::QueryError;
use anvil_core::ports::query_port::QueryPort;
use anvil_core::ports::review_verdict_port::{
    ReviewVerdictRecord, ReviewVerdictWritePort, REVIEW_VERDICT_KIND,
};
use anvil_core::ports::routing_activity_port::{RoutingActivityRecord, RoutingActivityWritePort};
use anvil_core::ports::session_verifier::{SessionMode, VerifiedSession};
use anvil_core::ports::step_measurement_port::{
    StepEvidenceRecord, StepMeasurementReadPort, StepMeasurementRecord, STEP_MEASUREMENT_KIND,
};
use anvil_core::ports::transition_measurement_port::{
    TransitionMeasurementRecord, TransitionMeasurementWritePort, TRANSITION_MEASUREMENT_KIND,
};
use anvil_core::ports::playbook_measurement_port::{
    PlaybookMeasurementReadPort, PlaybookMeasurementRecord, PlaybookMeasurementWritePort,
    PLAYBOOK_MEASUREMENT_KIND,
};
use anvil_engine::claimed_evidence::{
    begin_request_to_domain, complete_request_to_domain, snapshot_request_to_domain,
    MappedLifecycleRequest,
};
use anvil_engine::step_measurement_dispatcher::StepMeasurementDispatcher;
use anvil_engine::proto::anvil_service_server::{AnvilService, AnvilServiceServer};
use anvil_engine::proto::{
    ActivitySummaryRequest, ActivitySummaryResponse, ActorActivityRequest, ActorActivityResponse,
    AmendRequest, AmendResponse, BeginAdoptionStatusRequest, BeginAdoptionStatusResponse,
    BeginRequest, BeginResponse, CandidateMeta, CatalogRequest, CatalogResponse, CheckinRequest,
    CheckinResponse, CompleteRequest, CompleteResponse, DescribeRequest, DescribeResponse,
    HealthCheckRequest, HealthCheckResponse, HookManifestRequest, HookManifestResponse,
    IntakeCandidatePlaybookRequest, IntakeCandidatePlaybookResponse, ParkHint,
    PersistPlaybookRequest, PersistPlaybookResponse, RouteRequest, RouteResponse, SnapshotRequest,
    SnapshotResponse, UsageTimeSeriesRequest, UsageTimeSeriesResponse, PlaybookActivityRequest,
    PlaybookActivityResponse, PlaybookFidelityRequest, PlaybookFidelityResponse,
    PlaybookOwnerGroup, PlaybookStepVolumeRequest, PlaybookStepVolumeResponse,
};
use anvil_engine::kiln_router;
use anvil_engine::semantic_route::{
    apply_semantic_verdict_with_briefs, router_v1_gate_breadth_enabled,
    router_v2_brief_cap, semantic_route_plan_with_brief_cap, semantic_route_rpc_enabled,
};
use anvil_engine::session::{EngineVerifier, VerificationCache};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use tonic::{Request, Response, Status};

mod publication_log;
mod route_index;
mod scorecard_reader;
mod step0_activity_index;
mod telemetry_salt;
mod ws_bridge;

use anvil_core::domain::autonomy_evidence::{
    count_revision_cycles,
    grade_cleanliness_score,
    CLEANLINESS_DIMENSION,
    CLEANLINESS_GRADER,
};
use anvil_core::ports::playbook_measurement_port::DimensionScore;
use anvil_engine::command_seam::{self, CommandSeam};
use publication_log::PublicationLog;
use route_index::{NudgeDedup, OpenMarkerIndex};

const ENGINE_PARENT_PID_ENV: &str = "ANVIL_ENGINE_PARENT_PID";

/// The kit's expected JWT audience under Foundry (spec / J5).
const EXPECTED_AUDIENCE: &str = "foundry-mcp:anvil-kit";

/// The operating mode the engine was started in, decided ONCE at startup from
/// the engine's own environment (D1): a present (non-whitespace) at-spawn
/// `FOUNDRY_SESSION_TOKEN` means Foundry; absent or whitespace-only means
/// Standalone (spec Req 1 — the trim is applied at startup, in
/// `detect_mode_and_verifier`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum EngineMode {
    Standalone,
    Foundry,
}

#[derive(Clone)]
struct HearthPolicy {
    permitted_roots: Vec<PathBuf>,
}

impl HearthPolicy {
    fn new(
        default: Option<&Path>,
        global_playbooks_hearth: Option<&Path>,
        configured_roots: Vec<PathBuf>,
    ) -> Result<Self, String> {
        let mut permitted_roots = Vec::new();
        for root in configured_roots {
            permitted_roots.push(canonicalize_startup_path("--permitted-root", &root)?);
        }
        if let Some(default) = default {
            permitted_roots.push(canonicalize_startup_path("--hearth", default)?);
        }
        if let Some(global) = global_playbooks_hearth {
            permitted_roots.push(canonicalize_startup_path(
                "--global-playbooks-hearth",
                global,
            )?);
        }
        Ok(Self { permitted_roots })
    }

    fn permits(&self, canonical_hearth: &Path) -> bool {
        self.permitted_roots
            .iter()
            .any(|root| canonical_hearth.starts_with(root))
    }
}

/// Resolve the target hearth for an RPC.
///
/// Empty request paths use the optional spawn default. Non-empty request paths
/// are canonicalized and validated as hearths (`tracks/` + `tracks.md`). Both
/// paths are checked against the fail-closed permitted-root policy. Returns the
/// canonical path used as adapter base, `resolved_hearth` echo, and
/// `HearthLocks` key.
fn resolve_hearth(
    req_hearth_path: &str,
    default: Option<&Path>,
    policy: &HearthPolicy,
) -> Result<PathBuf, Status> {
    let using_default = req_hearth_path.is_empty();
    let candidate = if using_default {
        default.ok_or_else(|| {
            // Self-heal guidance (typed precondition error, NOT a silent default):
            // no hearth_path was supplied and the engine has no default hearth, so
            // attribution cannot be charged to any project. Instruct the caller how
            // to resolve and resupply the hearth instead of corrupting the global
            // hearth with a source-less turn.
            Status::failed_precondition(
                "no hearth resolved — pass hearth_path. Find it in `.hearth` at your project root (a file containing `path: <hearth-dir>`), or in the sibling `*-hearth` repo's `hearth.yaml`. If neither exists, create `.hearth` pointing at the project's hearth.",
            )
        })?
    } else {
        Path::new(req_hearth_path)
    };

    let canonical = std::fs::canonicalize(candidate).map_err(|_| {
        Status::invalid_argument(format!(
            "hearth_path does not resolve to an existing path: '{}'",
            candidate.display()
        ))
    })?;

    if !canonical.is_dir() {
        return Err(Status::invalid_argument(format!(
            "hearth_path is not a directory: '{}'",
            canonical.display()
        )));
    }

    if !using_default {
        // Hearth predicate (Req 4): a directory containing tracks/ AND tracks.md.
        let has_tracks_dir = canonical.join("tracks").is_dir();
        let has_registry = canonical.join("tracks.md").is_file();
        if !has_tracks_dir || !has_registry {
            return Err(Status::invalid_argument(format!(
                "path is not a valid hearth (missing tracks/ dir or tracks.md registry): '{}'",
                canonical.display()
            )));
        }
    }

    if !policy.permits(&canonical) {
        return Err(Status::permission_denied(format!(
            "hearth_not_permitted: hearth_path is outside permitted roots: '{}'",
            canonical.display()
        )));
    }

    Ok(canonical)
}

fn canonicalize_startup_path(label: &str, path: &Path) -> Result<PathBuf, String> {
    std::fs::canonicalize(path).map_err(|e| {
        format!(
            "{} '{}' could not be canonicalized: {}",
            label,
            path.display(),
            e
        )
    })
}

fn canonicalize_startup_hearth(label: &str, path: &Path) -> Result<PathBuf, String> {
    let canonical = canonicalize_startup_path(label, path)?;
    if !canonical.is_dir() {
        return Err(format!(
            "{} '{}' is not a directory",
            label,
            canonical.display()
        ));
    }
    if !canonical.join("tracks").is_dir() || !canonical.join("tracks.md").is_file() {
        return Err(format!(
            "{} '{}' is not a valid hearth (missing tracks/ dir or tracks.md registry)",
            label,
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn playbook_source_tier<'a>(
    source: Option<&PlaybookSource>,
    request_hearth: &Path,
    global_playbooks_hearth: Option<&Path>,
) -> &'a str {
    match source.and_then(|s| s.hearth.as_deref()) {
        Some(hearth) if hearth == request_hearth => "request",
        Some(hearth) if global_playbooks_hearth.is_some_and(|global| global == hearth) => "global",
        Some(_) => "hearth",
        None => "seed",
    }
}

fn proto_request_context(
    ctx_org: &str,
    ctx_space: &str,
    ctx_role: &str,
    ctx_clearance: &str,
) -> RequestContext {
    if ctx_org.is_empty() && ctx_space.is_empty() && ctx_role.is_empty() && ctx_clearance.is_empty()
    {
        return RequestContext::default_safe();
    }

    RequestContext {
        org: ctx_org.to_string(),
        space: if ctx_space.is_empty() {
            None
        } else {
            Some(ctx_space.to_string())
        },
        role: parse_context_role(ctx_role),
        clearance: parse_context_clearance(ctx_clearance),
    }
}

fn parse_context_role(value: &str) -> Role {
    match value.trim().to_ascii_lowercase().as_str() {
        "write" => Role::Write,
        "admin" => Role::Admin,
        _ => Role::Read,
    }
}

fn parse_context_clearance(value: &str) -> Sensitivity {
    match value.trim().to_ascii_lowercase().as_str() {
        "public" => Sensitivity::Public,
        "confidential" => Sensitivity::Confidential,
        "phi" => Sensitivity::Phi,
        _ => Sensitivity::Internal,
    }
}

struct AnvilServer {
    hearth_path: Option<PathBuf>,
    global_playbooks_hearth: Option<PathBuf>,
    hearth_policy: HearthPolicy,
    /// One write lock per canonical hearth, held for the process lifetime
    /// (spec Req 5, N3). ALL write paths — begin, snapshot, complete — obtain
    /// their guard from this single instance, keyed by the call's resolved
    /// canonical hearth, so same-hearth writes serialize and different-hearth
    /// writes never block. Replaces the former single process-wide
    /// `snapshot_lock`.
    hearth_locks: HearthLocks,
    /// The session-enforcement mode decided once at startup from the engine's
    /// own env (D1). Independent of `HearthLocks` — gates every RPC before any
    /// hearth resolution or lock acquisition.
    mode: EngineMode,
    /// Per-token verification cache backed by a CONCRETE `EngineVerifier`
    /// (J4 — enum-dispatch, NOT a type parameter on `AnvilServer`). The
    /// `expires_at`-keyed cache bounds revocation latency to the token's
    /// remaining lifetime (spec Req 6 / G1 — see `session.rs` for the bound).
    verification_cache: VerificationCache<EngineVerifier>,
    /// One bounded, non-blocking queue for every durable lean step row. Its
    /// single OS-thread consumer preserves accepted-row order without allowing
    /// a slow or failing filesystem sink to delay lifecycle RPCs.
    step_measurement_dispatcher: StepMeasurementDispatcher,
    /// Best-effort Crucible CQRS publication-log sink. `Some` only on a real
    /// engine data dir (or with `$FOUNDRY_PUBLICATION_LOG_DIR` set); `None`
    /// keeps anvil's own tests inert. Teed off the dispatch loops — never fatal.
    publication_log: Option<PublicationLog>,
    /// Per-`(hearth, conversation_id)` candidate open-marker index — the cheap
    /// lookup that lets the route handler resolve a conversation's open playbook
    /// by reading only candidate artifacts instead of enumerating the whole
    /// hearth. An optimization over the scan, never a source of truth:
    /// candidates are re-confirmed fresh at lookup time, and a miss fails open to
    /// the scan (which rebuilds the entry). See `route_index`.
    open_marker_index: OpenMarkerIndex,
    /// Per-`(hearth, conversation_id)` re-nudge dedup — suppresses a repeat
    /// check-in / reminder nudge within `NUDGE_DEDUP_WINDOW` when nothing changed
    /// since the last nudge. Re-armed by begin / snapshot / complete /
    /// continuation-token resume. Never suppresses the first nudge. See
    /// `route_index`.
    nudge_dedup: NudgeDedup,
}

/// Opaque lifetime guard for Brine's real in-process gRPC server.
///
/// This lives beside the private production server so the fail-open proof can
/// exercise the actual lifecycle handlers without widening `AnvilServer`'s
/// production visibility. Dropping the guard requests graceful shutdown.
#[cfg(test)]
pub(crate) struct TestAnvilServerHandle {
    port: u16,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    _task: tokio::task::JoinHandle<()>,
}

#[cfg(test)]
impl TestAnvilServerHandle {
    pub(crate) fn port(&self) -> u16 {
        self.port
    }
}

#[cfg(test)]
impl Drop for TestAnvilServerHandle {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}

/// Bind the real private `AnvilServer` to an ephemeral loopback port while
/// replacing only its durable step-measurement writer.
///
/// The production lifecycle handlers, hearth adapters, registry resolution,
/// and tonic service are unchanged. The injected writer is consumed by the
/// same bounded `StepMeasurementDispatcher` that production uses, which lets a
/// deterministic parking writer prove that sink slowness cannot delay an RPC.
#[cfg(test)]
pub(crate) async fn spawn_test_server_with_step_measurement_writer(
    hearth: PathBuf,
    writer: std::sync::Arc<dyn anvil_core::ports::step_measurement_port::StepMeasurementWritePort>,
) -> Result<TestAnvilServerHandle, String> {
    let hearth = canonicalize_startup_path("test hearth", &hearth)?;
    let hearth_policy = HearthPolicy::new(Some(&hearth), None, Vec::new())?;
    let server = AnvilServer {
        hearth_path: Some(hearth),
        global_playbooks_hearth: None,
        hearth_policy,
        hearth_locks: HearthLocks::new(),
        mode: EngineMode::Standalone,
        verification_cache: VerificationCache::new(EngineVerifier::Standalone),
        step_measurement_dispatcher: StepMeasurementDispatcher::with_writer(writer, 64),
        publication_log: None,
        open_marker_index: OpenMarkerIndex::new(),
        nudge_dedup: NudgeDedup::new(),
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| format!("bind in-process anvil test server: {}", error))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("read in-process anvil test address: {}", error))?
        .port();
    let app = tonic::service::Routes::new(AnvilServiceServer::new(server)).into_axum_router();
    let (shutdown, shutdown_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let result = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await;
        if let Err(error) = result {
            tracing::warn!(
                error = %error,
                "in-process anvil test server stopped with an error"
            );
        }
    });

    Ok(TestAnvilServerHandle {
        port,
        shutdown: Some(shutdown),
        _task: task,
    })
}

/// The run's revision cycles, read at emit from the artifact's OWN transition
/// store.
///
/// THE COUNTING IS NOT DONE HERE. `count_revision_cycles` in
/// `anvil_core::domain::autonomy_evidence` is the one place that turns a history
/// into a number of corrections; this function only supplies the history. Two
/// loops over the same rule is how two surfaces come to report different numbers
/// for the same run while both look right.
///
/// The SOURCE differs from `playbook_run_fidelity`'s deliberately: that fold
/// reads the hearth activity log because it answers "every instance", this reads
/// the artifact's own event store because it answers "this run", and for one run
/// the events are the evidence. They can disagree, and the read fold reports the
/// disagreement in both directions rather than preferring one.
///
/// `None` when the artifact directory was not supplied, could not be read, or
/// could not be folded. A run whose history could not be read is UNGRADED — it
/// is never graded clean, because "I could not look" and "nothing sent this back"
/// are different facts and collapsing them is how an unreadable history comes to
/// present as a perfect record.
fn revision_cycles_at_emit(artifact_dir: Option<&Path>) -> Option<u64> {
    let dir = artifact_dir?;
    let text = std::fs::read_to_string(dir.join("status.yaml")).ok()?;
    let status: anvil_core::domain::status::FullStatusYaml = serde_yaml::from_str(&text).ok()?;
    let history =
        anvil_core::domain::transition_log::resolve_transitions_with_events(&status, dir).ok()?;
    Some(count_revision_cycles(&history))
}

#[allow(clippy::too_many_arguments)]
fn append_playbook_measurement_for_machine(
    resolved: &Path,
    machine: Option<&PlaybookMachine>,
    playbook_run_id: &str,
    // MERGE RESOLUTION: origin/main introduced this helper with the retired
    // `artifact_kind` name; this branch's rename applies to it too. The field it
    // populates is `PlaybookMeasurementRecord::artifact_kind`.
    artifact_kind: &str,
    terminal_state: &str,
    at: &str,
    conversation_hash: Option<String>,
    project_label: Option<String>,
    // The run's own artifact directory, so the cleanliness grader can read the
    // history it grades. `None` leaves the record UNGRADED.
    artifact_dir: Option<&Path>,
) {
    let resolved_state =
        machine.and_then(|machine| machine.states.iter().find(|state| state.name == terminal_state));
    let terminal_reached = resolved_state.map(|state| state.is_terminal).unwrap_or(false);
    let resolution_gap = machine.is_none() || resolved_state.is_none();
    if !terminal_reached && !resolution_gap {
        return;
    }
    let playbook_version =
        machine.and_then(anvil_core::domain::playbook_version::machine_content_version);
    let adapter = FileSystemPlaybookMeasurementAdapter::new(resolved);
    match adapter.read_playbook_measurements() {
        Ok(records) => {
            if !playbook_run_id.is_empty()
                && records
                    .iter()
                    .any(|record| record.playbook_run_id.as_deref() == Some(playbook_run_id))
            {
                return;
            }
        }
        Err(error) => {
            tracing::warn!(
                outcome = "playbook_measurement_read_failed",
                error = %error,
                "playbook-measurement sink read failed before append (non-fatal)"
            );
        }
    }
    // UNGRADED unless BOTH inputs are real: the machine had to resolve, and the
    // history had to be readable.
    //
    // The resolution gap is the one that mattered and was missed. This function
    // also writes a durable coverage-gap record for a run whose MACHINE could not
    // be resolved, and `revision_cycles_at_emit` needs no machine — it only reads
    // `status.yaml`. So a coverage-gap run used to come out graded `0.0` by a
    // grader that could not tell whether the run had reached anything, which is
    // exactly the "I could not look" / "this was not clean" collapse the grader
    // is supposed to refuse. `terminal_reached` is `false` for such a record by
    // construction, so the zero looked entirely plausible.
    let cleanliness = if resolution_gap {
        None
    } else {
        revision_cycles_at_emit(artifact_dir)
            .map(|cycles| grade_cleanliness_score(terminal_reached, cycles))
    };
    let record = PlaybookMeasurementRecord {
        kind: PLAYBOOK_MEASUREMENT_KIND.to_string(),
        artifact_kind: artifact_kind.to_string(),
        playbook_run_id: Some(if playbook_run_id.is_empty() {
            "unknown_playbook_run".to_string()
        } else {
            playbook_run_id.to_string()
        }),
        terminal_state: terminal_state.to_string(),
        terminal_reached,
        outcome: if resolution_gap {
            "measurement_unknown".to_string()
        } else {
            "terminal_reached".to_string()
        },
        success: terminal_reached,
        // THE FLOOR AND THE QUALITY ARE TWO DIFFERENT FACTS, AND UNTIL NOW ONLY
        // ONE OF THEM WAS RECORDED. `success` above is the completion floor —
        // "it finished" — and the port has always said so. The quality vector
        // beside it shipped EMPTY with no grader, so every reader that wanted
        // "did it finish WELL" had nothing but the floor to read, and a run sent
        // back three times presented identically to one that was never touched.
        //
        // The grader is `run_cleanliness/v1`: reached the end AND was never sent
        // back. It does not invent a definition — it is the predicate
        // `playbook_run_fidelity` already computes and `step_two_by_two`'s
        // one-shot axis already uses, over the one shared revision-state rule.
        //
        // A SCORE AND NOTHING ELSE. The attribution — which step, which moment,
        // who caught it — stays on the read fold. This sink states that it never
        // carries raw actor identity, so no catcher's name may be copied here.
        //
        // ABSENT WHEN UNGRADED. `quality_grader` is the discriminator, never the
        // score: an unclean run scores a real `0.0`, so a reader keying on
        // "non-zero" could not tell it from a record nothing ever graded.
        quality_dimension_scores: cleanliness
            .map(|score| {
                vec![DimensionScore {
                    dimension: CLEANLINESS_DIMENSION.to_string(),
                    score,
                }]
            })
            .unwrap_or_default(),
        quality_overall: cleanliness,
        quality_grader: cleanliness.map(|_| CLEANLINESS_GRADER.to_string()),
        quality_signal: anvil_core::ports::playbook_measurement_port::QualitySignal::Leading,
        at: at.to_string(),
        conversation_hash: Some(
            conversation_hash.unwrap_or_else(|| UNKNOWN_CONVERSATION_HASH.to_string()),
        ),
        project_label: Some(project_label.unwrap_or_else(|| "unknown_project_label".to_string())),
        playbook_version,
    };
    if let Err(error) = adapter.append_playbook_measurement(&record) {
        tracing::warn!(
            outcome = "playbook_measurement_append_failed",
            error = %error,
            "playbook-measurement sink append failed (non-fatal)"
        );
    }
}

#[cfg(test)]
pub(crate) fn emit_unresolved_playbook_measurement_for_test(resolved: &Path, artifact_dir: &Path) {
    append_playbook_measurement_for_machine(
        resolved,
        None,
        "unresolved-playbook-run",
        "missing_playbook",
        "unknown_terminal",
        "2026-07-28T00:00:00Z",
        Some("unresolved-conversation-hash".to_string()),
        Some("unresolved-project".to_string()),
        // THE ARTIFACT DIRECTORY IS THE CALLER'S AND IT IS A REAL, READABLE ONE.
        //
        // Two earlier spellings of this call were both unfalsifiable, in
        // opposite ways, and a mutant caught the second. The first passed
        // `None`, so the scenario proved only that a `None` argument yields a
        // `None` grade — a property of this call site. The second passed the
        // HEARTH ROOT, which has no `status.yaml`, so the grade came back
        // absent because the history was unreadable and removing the
        // resolution-gap gate entirely left the scenario green.
        //
        // The caller now supplies an artifact directory whose history reads
        // cleanly and contains a correction. So the ONLY reason this record can
        // come back ungraded is that the machine did not resolve — which is the
        // thing the scenario is named for, and a grader that ignored the gap
        // would score it a definite `0.0` and go red.
        Some(artifact_dir),
    );
}

struct RequestSessionRegistry {
    registry: Box<dyn PlaybookRegistry>,
    request_invalid: Vec<PlaybookLoadError>,
    global_invalid: Vec<PlaybookLoadError>,
}

#[derive(Default)]
struct BeginResponseMeasurement {
    intent: String,
    expected_output: String,
    playbook_id: String,
}

fn resolve_step_evidence(
    machine: &PlaybookMachine,
    state: &str,
    role: &str,
    claimed_evidence: &[ClaimedEvidence],
) -> Option<StepEvidenceRecord> {
    use anvil_core::domain::playbook::interpreter::state_role_measurement;
    use anvil_core::domain::playbook_version::machine_content_version;

    if !machine.is_driven() {
        return None;
    }
    let measurement = state_role_measurement(machine, state, role)?;
    if measurement.evidence_obligation.is_empty() {
        return None;
    }

    let claimed_classes = claimed_evidence
        .iter()
        .map(|claim| claim.class)
        .collect::<Vec<_>>();
    let assessment =
        assess_evidence_obligation(&measurement.evidence_obligation, &claimed_classes)?;
    let playbook_version = machine_content_version(machine)?;

    Some(StepEvidenceRecord {
        assessment,
        claimed_evidence: claimed_evidence.to_vec(),
        playbook_version,
    })
}

const CLAIMED_EVIDENCE_UNSATISFIED: &str = "playbook_evidence_obligation_unsatisfied";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaimedEvidenceGateError {
    Unsatisfied,
}

fn claimed_evidence_gate_error_to_status(error: ClaimedEvidenceGateError) -> Status {
    match error {
        ClaimedEvidenceGateError::Unsatisfied => {
            Status::failed_precondition(CLAIMED_EVIDENCE_UNSATISFIED)
        }
    }
}

/// Run the Phase-3 completeness preflight against the exact transition step.
/// FREE machines and steps without an obligation are deliberately outside the
/// gate. Satisfaction delegates to T-EEC-1's canonical strength predicate; the
/// gate never depends on the optional playbook-version stamp used by telemetry.
fn enforce_claimed_evidence_preflight(
    enabled: bool,
    registry: &dyn PlaybookRegistry,
    kind: &str,
    state: &str,
    role: &str,
    claimed_evidence: &[ClaimedEvidence],
) -> Result<(), ClaimedEvidenceGateError> {
    if !enabled {
        return Ok(());
    }
    let Some(machine) = registry.machine_for(kind) else {
        return Ok(());
    };
    if !machine.is_driven() {
        return Ok(());
    }
    use anvil_core::domain::playbook::interpreter::state_role_measurement;
    let Some(measurement) = state_role_measurement(machine, state, role) else {
        return Ok(());
    };
    if measurement.evidence_obligation.is_empty() {
        return Ok(());
    }
    let claimed_classes = claimed_evidence
        .iter()
        .map(|claim| claim.class)
        .collect::<Vec<_>>();
    if obligation_satisfied(&measurement.evidence_obligation, &claimed_classes) {
        Ok(())
    } else {
        Err(ClaimedEvidenceGateError::Unsatisfied)
    }
}

/// The completion merge check.
///
/// `completed` asserts that work SHIPPED. This is the only place the engine
/// makes that assertion checkable: when the transition lands in `completed` and
/// the caller presented claimed evidence that cites code, every cited commit
/// must be an ancestor of its repository's `origin/main` and every cited path
/// must exist. Refusal happens BEFORE the first hearth write, so a refused
/// completion leaves the artifact byte-for-byte where it was.
///
/// Honest cases pass by construction, and each one is a deliberate exclusion
/// rather than an oversight:
///
/// - a transition to any state other than `completed` — a track parked at a
///   gate is not claiming to have shipped, and its code is SUPPOSED to be on a
///   branch;
/// - a completion presenting no claims, or only non-code claims — a
///   document-only track cites no code;
/// - `projection_only` snapshots, which record no lifecycle progress.
///
/// What it therefore does NOT do: it cannot catch a completion that presents
/// NO evidence at all. Requiring evidence is the playbook's
/// `evidence_obligation` lever, not this one; this gate judges what was
/// claimed, and refuses when the claim is false.
fn enforce_completion_merge_check(
    to_state: &str,
    hearth: &Path,
    claimed_evidence: &[ClaimedEvidence],
) -> Result<(), Status> {
    if !arms_merge_check(to_state) {
        return Ok(());
    }
    let classified: Vec<(String, CodeClaim)> = claimed_evidence
        .iter()
        .map(|claim| {
            (
                claim.reference.clone(),
                classify_reference(&claim.reference),
            )
        })
        .collect();
    if !classified.iter().any(|(_, claim)| claim.is_code()) {
        return Ok(());
    }
    use anvil_core_hearth::git_code_evidence_adapter::GitCodeEvidenceAdapter;
    use anvil_core::ports::code_evidence_port::CodeEvidencePort;
    let resolver = GitCodeEvidenceAdapter::new(hearth.to_path_buf(), |key| {
        std::env::var(key).ok()
    });
    let resolved: Vec<(String, _)> = classified
        .into_iter()
        .map(|(reference, claim)| {
            let resolution = resolver.resolve(&claim);
            (reference, resolution)
        })
        .collect();
    match merge_check_verdict(&resolved) {
        Ok(()) => Ok(()),
        Err(refusal) => {
            tracing::warn!(
                outcome = "completion_merge_check_refused",
                refused = refusal.refused.len(),
                code_claims_examined = refusal.code_claims_examined,
                "completion cited code that is not on origin/main"
            );
            Err(Status::failed_precondition(refusal.to_string()))
        }
    }
}

/// Resolve whether the P4 transition gate may enforce for this request. The
/// lane-local flag ships OFF and MUST NOT be activated until D-privacy decision
/// `20260622T1940_step_measurement_emit_privacy` reaches `decided`. The runtime
/// decision check is retained as a second guard: an unresolved or unreadable
/// decision keeps even an accidentally enabled lane fail-open.
fn claimed_evidence_gate_active(
    request_hearth: &Path,
    global_playbooks_hearth: Option<&Path>,
) -> bool {
    if !anvil_engine::engine_flags::claimed_evidence_gate_enabled_for_hearth(
        request_hearth,
        |key| std::env::var(key).ok(),
    ) {
        return false;
    }
    let mut decision_hearths = vec![request_hearth];
    if let Some(global) = global_playbooks_hearth.filter(|global| *global != request_hearth) {
        decision_hearths.push(global);
    }
    privacy_decision_is_decided(&decision_hearths)
}

fn required_role_for_transition<'a>(
    machine: &'a PlaybookMachine,
    from_state: &str,
    to_state: &str,
) -> Option<&'a str> {
    machine
        .transitions
        .iter()
        .find(|transition| {
            transition.from_state == from_state && transition.to_state == to_state
        })
        .map(|transition| transition.required_role.as_str())
}

fn completed_step_measurement_role(
    machine: &PlaybookMachine,
    from_state: &str,
    selected_required_role: &str,
    fallback_role: &str,
) -> String {
    use anvil_core::domain::playbook::interpreter::state_role_measurement;

    if !selected_required_role.is_empty()
        && state_role_measurement(machine, from_state, selected_required_role).is_some()
    {
        selected_required_role.to_string()
    } else {
        fallback_role.to_string()
    }
}

/// Thin delegate to `PlaybookLoadError::artifact_ids`.
///
/// The match USED TO LIVE HERE, in a private fn inside the engine binary, which
/// meant its hearth-directory arm was reachable from no feature at all: an
/// independent review replaced that arm with a sentinel id and the full engine
/// suite stayed 523/523 green. The match now lives on the type in `anvil-core`,
/// where `anvil-core/features/playbook_registry_projection.feature` exercises it
/// directly. This wrapper stays so the call sites below read unchanged.
fn playbook_load_error_artifact_ids(error: &PlaybookLoadError) -> Vec<String> {
    error.artifact_ids()
}

fn playbook_load_error_matches_selected(
    error: &PlaybookLoadError,
    selected_kind: &str,
    registry: &dyn PlaybookRegistry,
) -> bool {
    let mut candidates = vec![selected_kind.to_string()];
    if let Some(playbook_id) = registry.playbook_id_for(selected_kind) {
        candidates.push(playbook_id);
    }
    playbook_load_error_artifact_ids(error)
        .iter()
        .any(|id| candidates.iter().any(|candidate| id == candidate))
}

fn selected_playbook_load_error<'a>(
    session_registry: &'a RequestSessionRegistry,
    selected_kind: &str,
    request_hearth: &Path,
    global_playbooks_hearth: Option<&Path>,
) -> Option<&'a PlaybookLoadError> {
    if selected_kind.is_empty() {
        return None;
    }
    if let Some(error) = session_registry.request_invalid.iter().find(|error| {
        playbook_load_error_matches_selected(
            error,
            selected_kind,
            session_registry.registry.as_ref(),
        )
    }) {
        return Some(error);
    }

    let source = session_registry.registry.source_for(selected_kind);
    let source_tier =
        playbook_source_tier(source.as_ref(), request_hearth, global_playbooks_hearth);
    if source_tier == "request" {
        return None;
    }

    session_registry.global_invalid.iter().find(|error| {
        playbook_load_error_matches_selected(
            error,
            selected_kind,
            session_registry.registry.as_ref(),
        )
    })
}

fn begin_measurement_kind_role(
    result: &anvil_core::domain::begin::BeginResult,
    creation_kind_role: Option<(&str, &str)>,
) -> (String, String) {
    if let Some((kind, role)) = creation_kind_role {
        return (kind.to_string(), role.to_string());
    }
    (
        result.measurement_kind.clone(),
        result.measurement_role.clone(),
    )
}

fn resolve_begin_response_measurement(
    registry: &dyn PlaybookRegistry,
    kind: &str,
    state: &str,
    role: &str,
) -> BeginResponseMeasurement {
    use anvil_core::domain::playbook::interpreter::state_role_measurement;

    let playbook_id = registry
        .source_for(kind)
        .map(|source| source.playbook_id)
        .or_else(|| registry.playbook_id_for(kind))
        .unwrap_or_default();
    let (intent, expected_output) = registry
        .machine_for(kind)
        .and_then(|machine| state_role_measurement(machine, state, role))
        .map(|measurement| {
            (
                measurement.intent.clone(),
                measurement.expected_output.clone(),
            )
        })
        .unwrap_or_default();

    BeginResponseMeasurement {
        intent,
        expected_output,
        playbook_id,
    }
}

/// Whether the measurement-definition DEFINE block is enforced, read from
/// `ANVIL_ENFORCE_MEASUREMENT_DEFINITION` ("1"/"true", case-insensitive).
/// Shared by both the generator's terminal-persist enforcement (candidate ->
/// machine) and the loader-side enforcement below (hearth machine.yaml ->
/// registry) so the two dark-gates flip together from one flag. OFF by
/// default. `anvil-core` itself reads no environment — this is the one place
/// in the engine binary that decides.
fn enforce_measurement_definition() -> bool {
    std::env::var("ANVIL_ENFORCE_MEASUREMENT_DEFINITION")
        .map(|value| {
            let value = value.trim();
            value == "1" || value.eq_ignore_ascii_case("true")
        })
        .unwrap_or(false)
}

/// Whether the evidence-obligation authoring gate is enforced, read from
/// `ANVIL_ENFORCE_EVIDENCE_OBLIGATION` ("1"/"true", case-insensitive). A SEPARATE
/// dark-gate from `ANVIL_ENFORCE_MEASUREMENT_DEFINITION` (T-EEC-1): it flips the
/// obligation leg at ALL THREE seams together — the loader/registry construction
/// (`build_hearth_registry`), the persist WRITE boundary (`persist_playbook`
/// RPC), and candidate intake / generation terminal-persist — so a DRIVEN
/// measured `(state, role)` lacking an obligation, or a FREE machine declaring
/// one, is refused. OFF by default; `anvil-core` reads no environment — this is
/// the one place in the engine binary that decides. Flip on only after the fleet
/// is backfilled with per-step `evidence_obligation`s.
fn enforce_evidence_obligation() -> bool {
    std::env::var("ANVIL_ENFORCE_EVIDENCE_OBLIGATION")
        .map(|value| {
            let value = value.trim();
            value == "1" || value.eq_ignore_ascii_case("true")
        })
        .unwrap_or(false)
}

/// Whether the K5 supervision-bind hardening is active, read from
/// `ANVIL_K5_BIND` ("1"/"true", case-insensitive). Mirrors
/// `enforce_measurement_definition` / `enforce_evidence_obligation`: a
/// DARK-BY-DEFAULT explicit capability control, OFF unless the flag is set —
/// never an implicit fallback (No-Fallbacks law). Unset ⇒ pre-K5 begin/complete/
/// snapshot behaviour EXACTLY; set ⇒ the R10 bind-time bindability precondition
/// and the R9 terminal-resolve no-op guard are active. The flag gates BEHAVIOUR,
/// never masks an error. Enablement in any live/default surface is NICK-GATE-4.
/// `anvil-core` reads no environment — this is the one place in the engine
/// binary that decides.
fn k5_bind_enabled() -> bool {
    std::env::var("ANVIL_K5_BIND")
        .map(|value| {
            let value = value.trim();
            value == "1" || value.eq_ignore_ascii_case("true")
        })
        .unwrap_or(false)
}

/// Construct a `HearthPlaybookRegistry` for `hearth_path`, honoring the
/// loader-side measurement dark-gate. Central call point so every registry
/// construction across `request_session_registry` (both the free function and
/// the `AnvilServer` method below) and the `/ws` bridge shares the same
/// enforcement posture — DARK-GATED: OFF by default (today's behavior,
/// unaffected), and flipped on only after the hearth fleet is backfilled with
/// per-step `success_criteria` + `outcome_predicate`s. When on, a machine
/// that fails the DEFINE block never registers — it surfaces as an invalid
/// artifact (`playbook_measurement_definition_missing`) instead, exactly like
/// any other loader failure.
fn build_hearth_registry(hearth_path: PathBuf) -> HearthPlaybookRegistry {
    // Resolve BOTH dark-gate flags into one LoaderEnforcement so the
    // loader/registry seam obeys the measurement AND evidence-obligation gates in
    // lockstep with the persist and intake seams (T-EEC-1 P4). Both default OFF —
    // when both are off this is byte-identical to the pre-gate `new`.
    HearthPlaybookRegistry::new_with_enforcement(
        hearth_path,
        LoaderEnforcement {
            measurement_definition: enforce_measurement_definition(),
            evidence_obligation: enforce_evidence_obligation(),
        },
    )
}

/// Build the composite playbook registry for one request's resolved hearth,
/// layering the global-playbooks hearth and the compiled-in seeds. Free function
/// so both the RPC handlers (via `AnvilServer::request_session_registry`) and the
/// `/ws` bridge's shared read (`compute_playbook_activity`) construct it
/// identically.
fn request_session_registry(
    request_hearth: &Path,
    global_playbooks_hearth: Option<&Path>,
) -> Box<dyn PlaybookRegistry> {
    let request_registry = build_hearth_registry(request_hearth.to_path_buf());
    match global_playbooks_hearth {
        Some(global) => {
            let global_registry = build_hearth_registry(global.to_path_buf());
            Box::new(CompositePlaybookRegistry::new(
                request_registry,
                CompositePlaybookRegistry::new(global_registry, SeedPlaybookRegistry),
            ))
        }
        None => Box::new(CompositePlaybookRegistry::new(
            request_registry,
            // SeedPlaybookRegistry carries compiled-in lifecycle seeds
            // including free artifacts such as proposal, milestone,
            // decision, and initiative when no hearth playbook overrides them.
            SeedPlaybookRegistry,
        )),
    }
}

impl AnvilServer {
    fn request_session_registry(&self, request_hearth: &Path) -> RequestSessionRegistry {
        let request_invalid = build_hearth_registry(request_hearth.to_path_buf())
            .invalid_artifacts()
            .to_vec();
        let global_invalid = self
            .global_playbooks_hearth
            .as_ref()
            .map(|global| {
                build_hearth_registry(global.clone())
                    .invalid_artifacts()
                    .to_vec()
            })
            .unwrap_or_default();
        let registry =
            request_session_registry(request_hearth, self.global_playbooks_hearth.as_deref());
        RequestSessionRegistry {
            registry,
            request_invalid,
            global_invalid,
        }
    }

    /// Gatekeeper run at the top of every gated RPC handler, BEFORE
    /// `resolve_hearth` and BEFORE the per-hearth lock (D3, J5). An unauth
    /// caller is refused without ever contending the hearth lock.
    ///
    /// - Standalone mode ⇒ `Ok(None)` (proceed; today's behavior unchanged,
    ///   spec Req 3 — the verifier is never consulted).
    /// - Foundry mode ⇒ extract `authorization: Bearer <jwt>` from the gRPC
    ///   metadata; a blank/whitespace bearer under a Foundry-mode engine is a
    ///   refusal (Req 5) — the Req-1 whitespace⇒standalone rule is a *startup*
    ///   property (decided once in `detect_mode_and_verifier`), not a
    ///   per-request escape hatch. A present bearer is verified via the
    ///   cache/seam, then routed through the Phase-1 `SessionMode::decide`
    ///   seam:
    ///   - `Ok(Some(session))` ⇒ `Foundry(session)` ⇒ proceed (R2 binds the
    ///     principal to its `sub`);
    ///   - `Ok(None)` / `Err(_)` ⇒ `Status::unauthenticated("not_authenticated")`
    ///     (spec Req 5; fail-closed on broker-unreachable, spec Req 6).
    async fn authorize<T>(&self, req: &Request<T>) -> Result<Option<VerifiedSession>, Status> {
        if self.mode == EngineMode::Standalone {
            return Ok(None);
        }

        // Foundry mode: every gated RPC MUST carry a valid bearer (spec Req 5).
        // Extract `authorization: Bearer <jwt>` from the gRPC metadata.
        let raw = req
            .metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.strip_prefix("Bearer ").unwrap_or(s).to_string())
            .unwrap_or_default();

        // A blank/whitespace bearer is treated as ABSENT — but absence of a
        // credential under a Foundry-mode engine is a refusal (Req 5), NOT a
        // downgrade to standalone.
        if raw.trim().is_empty() {
            return Err(Status::unauthenticated("not_authenticated"));
        }

        // A present (trimmed-non-empty) bearer: verify via the cache/seam, then
        // route the outcome through the Phase-1 `SessionMode::decide` seam.
        let verify_outcome = self
            .verification_cache
            .lookup(&raw, EXPECTED_AUDIENCE)
            .await;
        match SessionMode::decide(true, verify_outcome) {
            SessionMode::Foundry(session) => Ok(Some(session)),
            // `Ok(None)` from the verifier (no session) under a Foundry-mode
            // engine that received a token is still a refusal — there is no
            // valid session to gate on.
            SessionMode::Standalone => Err(Status::unauthenticated("not_authenticated")),
            SessionMode::Refuse => Err(Status::unauthenticated("not_authenticated")),
        }
    }

    /// Derive the principal actor name to persist in the event log (spec Req 7).
    ///
    /// This override is applied identically by begin/snapshot/complete — every
    /// surface that writes the transition actor.
    ///
    /// - **Foundry mode** (`authorize()` yielded `Some(session)`): the
    ///   caller-supplied `actor_name` is DISCARDED and the principal is derived
    ///   from `session.sub` as `foundry:<sub>`. The `foundry:` prefix is
    ///   collision-safe — a colon never appears in an organically-generated
    ///   `{Word}-{6 digits}` actor name — and greppable in the event log.
    ///   This closes the principal-laundering gap: a caller can never write a
    ///   self-asserted identity into the permanent event log under Foundry.
    /// - **Standalone mode** (`authorize()` yielded `None`): the
    ///   caller-supplied `actor_name` is returned verbatim — today's behavior,
    ///   unchanged (spec Req 3 no-regression). The override fires ONLY on a
    ///   present, validated session.
    fn principal_actor_name(
        session: &Option<VerifiedSession>,
        caller_actor_name: String,
    ) -> String {
        match session {
            Some(s) => format!("foundry:{}", s.sub),
            None => caller_actor_name,
        }
    }

    fn deployment_salt(&self) -> Option<String> {
        telemetry_salt::resolve_salt(
            self.global_playbooks_hearth.as_deref(),
            &self.hearth_policy.permitted_roots,
        )
    }

    fn conversation_hash(&self, conversation_id: &str) -> Option<String> {
        anvil_core::domain::telemetry_salt::actor_hash(
            self.deployment_salt().as_deref(),
            conversation_id,
        )
    }

    /// ONE basename rule, and it is not this function's. The rule lives in
    /// `anvil_core::ports::delivery_log_port::project_label_from_root`, which
    /// the delivery-log projection also calls; this is the `Option` shim the
    /// engine's callers already expect. Two copies of a projection that must
    /// agree is the drift pattern this repository has already paid for, so the
    /// hoist and this delegation land in ONE commit — a commit holding both
    /// rules is the state that invariant forbids.
    fn project_label(project_root: &str) -> Option<String> {
        let label = anvil_core::ports::delivery_log_port::project_label_from_root(project_root);
        if label.is_empty() {
            None
        } else {
            Some(label)
        }
    }

    /// Resolve a conversation's open (begun, non-terminal) playbook CHEAPLY via
    /// the open-marker index, failing open to the full scan on any miss /
    /// uncertainty (open_marker_index Phase 1). The index gives CANDIDATES; the
    /// pure lookup re-confirms each one fresh, so staleness can never produce a
    /// wrong answer. Behavior is identical to a bare
    /// `find_open_playbook_run_for_conversation` (terminal excluded, empty
    /// conversation → `None`, most-recently-begun on multiplicity) — only cost
    /// improves on the warm path.
    ///
    /// Cold path: a conversation the index has never seen → full scan, then the
    /// index is repopulated with the artifact id the scan confirmed so the next
    /// turn takes the cheap path. (A confirmed-empty conversation is recorded as
    /// an empty candidate set so a no_match-while-open lookup on a genuinely
    /// idle conversation pays the scan only once.) Fail-open: a lookup error
    /// degrades to `None`, never an error on the (fire-and-forget) route turn.
    fn open_playbook_run_for_conversation(
        &self,
        hearth: &Path,
        registry: &dyn PlaybookRegistry,
        conversation_id: &str,
    ) -> Option<OpenPlaybookRun> {
        if conversation_id.trim().is_empty() {
            return None;
        }
        let query = FileSystemQueryAdapter::new(hearth.to_path_buf());

        // Warm path: the index knows this conversation's candidates → read only
        // those, re-confirming each fresh (candidates, not verdicts).
        if let Some(candidates) = self.open_marker_index.candidates(hearth, conversation_id) {
            match find_open_playbook_run_indexed(&query, registry, conversation_id, &candidates) {
                Ok(found) => return found,
                Err(e) => {
                    tracing::warn!(
                        command = "route",
                        outcome = "indexed_open_lookup_failed",
                        error = %e,
                        "indexed open-playbook lookup failed — degrading to scan (fail-open)"
                    );
                    // Fall through to the scan below.
                }
            }
        }

        // Cold / degraded path: full scan, then repopulate the index from the
        // confirmed result so the next turn is cheap.
        match find_open_playbook_run_for_conversation(&query, registry, conversation_id) {
            Ok(found) => {
                let candidates = found
                    .as_ref()
                    .map(|w| vec![w.artifact_id.clone()])
                    .unwrap_or_default();
                self.open_marker_index
                    .populate(hearth, conversation_id, candidates);
                found
            }
            Err(e) => {
                tracing::warn!(
                    command = "route",
                    outcome = "open_lookup_failed",
                    error = %e,
                    "open-playbook scan failed — degrading to no-open (fail-open)"
                );
                None
            }
        }
    }

    /// Append ONE redacted universal-activity record for a COMMAND turn that just
    /// resolved (additive; never the read/measurement queries). Mirrors how the
    /// route handler appends the routing-activity record and how complete appends
    /// the step-measurement record: best-effort, warn-on-failure, NEVER fails the
    /// RPC. Only the Part-3 allowlisted fields are written — command, outcome
    /// label, resolved artifact_kind (empty where the command has none), a salted
    /// non-reversible actor_hash (None when no salt / no actor), and the
    /// timestamp. NEVER the message text, raw identities, or paths.
    ///
    /// The deployment salt is resolved exactly as the complete handler resolves
    /// it for the step-measurement sink, so an actor hashes identically across
    /// every sink and every hearth (correct cross-hearth distinct counts).
    #[allow(clippy::too_many_arguments)]
    fn emit_activity(
        &self,
        command: &str,
        outcome: &str,
        artifact_kind: &str,
        from_state: &str,
        to_state: &str,
        actor: &str,
        resolved: &Path,
        at: &str,
        source: &str,
        conversation_hash: Option<String>,
        project_label: Option<String>,
        playbook_run_id: Option<String>,
        call_state: Option<String>,
    ) {
        let salt = self.deployment_salt();
        let actor_hash = anvil_core::domain::telemetry_salt::actor_hash(salt.as_deref(), actor);
        let record = ActivityLogRecord {
            command: command.to_string(),
            outcome: outcome.to_string(),
            artifact_kind: artifact_kind.to_string(),
            from_state: from_state.to_string(),
            to_state: to_state.to_string(),
            actor_hash,
            at: at.to_string(),
            source: source.to_string(),
            conversation_hash,
            project_label,
            playbook_run_id,
            call_state,
        };
        if let Err(e) = FileSystemActivityLogAdapter::new(resolved).append_activity_log(&record) {
            tracing::warn!(
                command = command,
                outcome = "activity_log_append_failed",
                error = %e,
                "activity-log sink append failed (non-fatal)"
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_lean_step_measurement(
        &self,
        resolved: &Path,
        playbook_run_id: &str,
        artifact_kind: &str,
        from_state: &str,
        to_state: &str,
        role: &str,
        actor: &str,
        intent: &str,
        expected_output: &str,
        at: &str,
        conversation_hash: Option<String>,
        project_label: Option<String>,
        evidence: Option<StepEvidenceRecord>,
    ) {
        let actor_hash = anvil_core::domain::telemetry_salt::actor_hash(
            self.deployment_salt().as_deref(),
            actor,
        );
        let record = StepMeasurementRecord {
            kind: STEP_MEASUREMENT_KIND.to_string(),
            from_state: from_state.to_string(),
            to_state: to_state.to_string(),
            role: role.to_string(),
            intent_present: !intent.is_empty(),
            expected_output_present: !expected_output.is_empty(),
            at: at.to_string(),
            artifact_kind: artifact_kind.to_string(),
            actor_hash,
            conversation_hash: Some(
                conversation_hash.unwrap_or_else(|| UNKNOWN_CONVERSATION_HASH.to_string()),
            ),
            project_label: Some(
                project_label.unwrap_or_else(|| "unknown_project_label".to_string()),
            ),
            playbook_run_id: Some(if playbook_run_id.is_empty() {
                "unknown_playbook_run".to_string()
            } else {
                playbook_run_id.to_string()
            }),
            evidence,
        };
        let _ = self
            .step_measurement_dispatcher
            .try_enqueue(resolved, record);
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_transition_measurement(
        &self,
        resolved: &Path,
        registry: &dyn PlaybookRegistry,
        playbook_run_id: &str,
        artifact_kind: &str,
        from_state: &str,
        to_state: &str,
        fallback_role: &str,
        satisfaction_value: &str,
        at: &str,
        conversation_hash: Option<String>,
        project_label: Option<String>,
        // transition_carries_step_evidence_status: the two facts that answer
        // DIFFERENT questions — what the call CLAIMED, and what the artifact of
        // record LOOKED LIKE. Passed in rather than re-derived here: the claim
        // is already in hand on the call, and reading another sink for it would
        // reintroduce the sync-vs-async ordering problem that made an earlier
        // design unimplementable.
        is_begin: bool,
        claims: &[ClaimedEvidence],
        artifact_dir: Option<&Path>,
    ) -> Option<String> {
        let machine = registry.machine_for(artifact_kind);
        let role = machine
            .and_then(|machine| required_role_for_transition(machine, from_state, to_state))
            .map(ToString::to_string)
            .unwrap_or_else(|| fallback_role.to_string());
        let satisfaction = machine
            .and_then(|m| m.states.iter().find(|s| s.name == from_state))
            .and_then(|s| {
                if s.is_review_gate {
                    Some(satisfaction_value.to_string())
                } else {
                    None
                }
            });
        // The state being ASSESSED is the one being LEFT — a begin is entering
        // its state and cannot have authored that state's artifact yet.
        let assessed_state = if is_begin { to_state } else { from_state };
        let classes: Vec<anvil_core::domain::playbook::types::EvidenceClass> =
            claims.iter().map(|c| c.class).collect();
        let artifact_on_disk = artifact_dir.and_then(|dir| {
            anvil_core::domain::begin::read_artifact_on_disk_by_shape(
                dir,
                artifact_kind,
                assessed_state,
            )
        });
        let claim_status = anvil_core::domain::begin::classify_claim(
            is_begin,
            artifact_on_disk.is_some(),
            &classes,
        );
        let artifact_assessment =
            anvil_core::domain::begin::classify_artifact(artifact_on_disk.as_ref());
        // WARN-FIRST. This never changes `success`: the transition happened, and
        // saying otherwise would make a measurement gap look like a failed
        // operation. It fires only on the two-condition gate — an unclaimed
        // completion whose artifact is still the untouched scaffold or gone —
        // which is what separates the 8 genuine abandonments from the 73
        // unclaimed-but-real completions on the live fleet.
        let evidence_warning = if anvil_core::domain::begin::warns(claim_status, artifact_assessment)
        {
            Some(format!(
                "evidence: {artifact_kind} left {assessed_state} with no evidence claim and its \
                 artifact of record is {artifact_assessment} — the step may not have produced \
                 what it was for"
            ))
        } else {
            None
        };
        let record = TransitionMeasurementRecord {
            claimed_evidence_status: Some(claim_status.to_string()),
            artifact_assessment: Some(artifact_assessment.to_string()),
            kind: TRANSITION_MEASUREMENT_KIND.to_string(),
            artifact_kind: artifact_kind.to_string(),
            from_state: from_state.to_string(),
            to_state: to_state.to_string(),
            role,
            satisfaction,
            outcome: "ok".to_string(),
            success: true,
            at: at.to_string(),
            conversation_hash: Some(
                conversation_hash.unwrap_or_else(|| UNKNOWN_CONVERSATION_HASH.to_string()),
            ),
            project_label: Some(
                project_label.unwrap_or_else(|| "unknown_project_label".to_string()),
            ),
            playbook_run_id: Some(if playbook_run_id.is_empty() {
                "unknown_playbook_run".to_string()
            } else {
                playbook_run_id.to_string()
            }),
        };
        if let Err(e) = FileSystemTransitionMeasurementAdapter::new(resolved)
            .append_transition_measurement(&record)
        {
            tracing::warn!(
                outcome = "transition_measurement_append_failed",
                error = %e,
                "transition-measurement sink append failed (non-fatal)"
            );
        }
        evidence_warning
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_playbook_measurement(
        &self,
        resolved: &Path,
        registry: &dyn PlaybookRegistry,
        playbook_run_id: &str,
        artifact_kind: &str,
        terminal_state: &str,
        at: &str,
        conversation_hash: Option<String>,
        project_label: Option<String>,
        artifact_dir: Option<&Path>,
    ) {
        // MERGE RESOLUTION (og/playbook-term-phase-a x origin/main, C-d.1 fix round).
        // BOTH sides carried a change and both are kept:
        //   * origin/main extracted this body into
        //     `append_playbook_measurement_for_machine` AND added the
        //     resolution-gap arm — an unresolvable machine now emits a
        //     `measurement_unknown` record instead of returning silently.
        //   * this branch renamed the parameter and the record field
        //     `artifact_kind` -> `artifact_kind`.
        // Taking either side whole would have dropped the other: main's side
        // would reintroduce the retired `artifact_kind` name, and this branch's
        // side would delete main's coverage-gap record.
        let machine = registry.machine_for(artifact_kind);
        append_playbook_measurement_for_machine(
            resolved,
            machine,
            playbook_run_id,
            artifact_kind,
            terminal_state,
            at,
            conversation_hash,
            project_label,
            artifact_dir,
        );
    }

    /// Capture one structured review verdict when a playbook instance leaves a
    /// review-gate state. Fires only when `from_state` is a machine-declared
    /// review gate; a no-op otherwise. The verdict carries the light structure
    /// from the success-rubric decision: gate/final-gate labels + the honest
    /// satisfaction verdict + join keys. `findings`/`intent_confidence` are the
    /// SHAPE only in this phase — populated by later structured-review work.
    /// `is_final_gate` is true when the gate has an outgoing transition into a
    /// machine-declared terminal state (the final/E2E gate); the holistic
    /// "intent well accomplished" confidence field is present only there.
    /// Fail-open: an append error is logged and swallowed; the transition still
    /// succeeds.
    #[allow(clippy::too_many_arguments)]
    fn emit_review_verdict(
        &self,
        resolved: &Path,
        registry: &dyn PlaybookRegistry,
        playbook_run_id: &str,
        artifact_kind: &str,
        from_state: &str,
        satisfaction: &str,
        at: &str,
        conversation_hash: Option<String>,
        project_label: Option<String>,
    ) {
        let machine = match registry.machine_for(artifact_kind) {
            Some(m) => m,
            None => return,
        };
        let is_review_gate = machine
            .states
            .iter()
            .any(|s| s.name == from_state && s.is_review_gate);
        if !is_review_gate {
            return;
        }
        // Final/E2E gate: some outgoing transition from this gate targets a
        // machine-declared terminal state.
        let terminal_states: std::collections::HashSet<&str> = machine
            .states
            .iter()
            .filter(|s| s.is_terminal)
            .map(|s| s.name.as_str())
            .collect();
        let is_final_gate = machine
            .transitions
            .iter()
            .any(|t| t.from_state == from_state && terminal_states.contains(t.to_state.as_str()));

        // Content-derived playbook version — temper's experiment unit. Computed
        // from the resolved machine at emit; None (fail-open) if unresolvable.
        let playbook_version =
            anvil_core::domain::playbook_version::machine_content_version(machine);

        let record = ReviewVerdictRecord {
            kind: REVIEW_VERDICT_KIND.to_string(),
            artifact_kind: artifact_kind.to_string(),
            playbook_run_id: if playbook_run_id.is_empty() {
                None
            } else {
                Some(playbook_run_id.to_string())
            },
            gate_state: from_state.to_string(),
            satisfaction: satisfaction.to_string(),
            is_final_gate,
            // Structured findings are authored in a later phase; the shape is
            // emitted here so the sink + join keys exist.
            findings: Vec::new(),
            // The holistic confidence field is present (empty) only at the
            // final gate; None at intermediate gates.
            intent_confidence: if is_final_gate {
                Some(String::new())
            } else {
                None
            },
            outcome: "ok".to_string(),
            at: at.to_string(),
            conversation_hash,
            project_label,
            playbook_version,
        };
        if let Err(e) = FileSystemReviewVerdictAdapter::new(resolved).append_review_verdict(&record)
        {
            tracing::warn!(
                outcome = "review_verdict_append_failed",
                error = %e,
                "review-verdict sink append failed (non-fatal)"
            );
        }
    }

    /// Emit one full §0 `StepMeasurementEvent` to the temper-consumed stream at
    /// `<temper_home>/.temper/step-measurements/<kind>/events.jsonl`, GATED on the
    /// `step-measurement-emit-privacy` decision being `decided`. While the gate is
    /// closed (not decided) NO rich prose/identities leave the process — the lean
    /// booleans-only hearth sink (written separately by the caller) is the only
    /// record. Applies the redaction policy (tokens omitted). Emits for EVERY
    /// playbook kind. Fail-open: a gate-read or write error is logged and
    /// swallowed; the transition still succeeds.
    #[allow(clippy::too_many_arguments)]
    fn emit_step0_stream(
        &self,
        resolved: &Path,
        playbook_id: &str,
        kind: &str,
        from_state: &str,
        to_state: &str,
        role: &str,
        actor: &str,
        intent: &str,
        expected_output: &str,
        at: &str,
        model: &str,
        tokens: Option<u64>,
        duration_ms: Option<u64>,
    ) {
        use anvil_core_hearth::fs_step_measurement_stream_adapter::FileSystemStep0StreamAdapter;
        use anvil_core::ports::step_measurement_stream_port::Step0StreamWritePort;

        // Privacy GATE (spec req #6): rich §0 ships ONLY when the decision is
        // decided. Check the request hearth first, then the global playbooks
        // hearth. Closed gate ⇒ no rich emit (the lean sink is the only record).
        let mut candidates: Vec<&Path> = vec![resolved];
        if let Some(global) = self.global_playbooks_hearth.as_deref() {
            if global != resolved {
                candidates.push(global);
            }
        }
        if !privacy_decision_is_decided(&candidates) {
            return;
        }

        let Some(temper_home) = resolve_temper_home() else {
            return; // no temper home resolvable — skip (fail-open).
        };

        let event = build_step0_event(
            playbook_id,
            kind,
            from_state,
            to_state,
            role,
            actor,
            intent,
            expected_output,
            at,
            model,
            tokens,
            duration_ms,
            &next_step0_seq(),
        );
        let adapter = FileSystemStep0StreamAdapter::new(temper_home);
        if let Err(e) = adapter.append_step0_event(&event) {
            tracing::warn!(
                outcome = "step0_stream_append_failed",
                error = %e,
                "§0 temper-stream append failed (non-fatal)"
            );
        }
    }

    /// Resolve (creating on demand) the `(unattributed)` attribution bucket: a
    /// discoverable hearth named `__unattributed__` under the global playbooks
    /// hearth (preferred) or the spawn default. Source-less route turns whose
    /// caller hearth cannot be resolved are charged HERE instead of silently
    /// corrupting the global hearth's own activity record (the dashboard buckets
    /// it separately because it is its own discoverable hearth).
    ///
    /// The scaffold is the minimal hearth predicate — an empty `tracks/` dir and
    /// an empty `tracks.md` registry — so `hearth_discovery::looks_like_hearth`
    /// includes it in `all_hearths` folds. Idempotent. Returns `None` when no
    /// global/default base exists (pure hearth-less, no global): the caller then
    /// SKIPS the attribution write entirely (fail-open) rather than inventing a
    /// location.
    fn unattributed_bucket(&self) -> Option<PathBuf> {
        let base = self
            .global_playbooks_hearth
            .as_deref()
            .or(self.hearth_path.as_deref())?;
        let bucket = base.join("__unattributed__");
        if let Err(e) = std::fs::create_dir_all(bucket.join("tracks")) {
            tracing::warn!(
                outcome = "unattributed_bucket_create_failed",
                error = %e,
                "could not create the (unattributed) attribution bucket (non-fatal)"
            );
            return None;
        }
        let registry = bucket.join("tracks.md");
        // C-d.1 round 8, H-3. `!registry.is_file()` mapped EACCES/EIO/ESTALE
        // onto "there is no registry here" and then TRUNCATED the file with an
        // empty write. `node_kind` cannot express that: an uninspectable
        // registry declines the bucket (warn + None, the fail-safe this function
        // already has for a failed mkdir) instead of emptying it.
        let registry_kind = match anvil_core::domain::playbook::fs_probe::node_kind(&registry) {
            Ok(kind) => kind,
            Err(e) => {
                tracing::warn!(
                    outcome = "unattributed_bucket_registry_uninspectable",
                    error = %e,
                    "the (unattributed) bucket registry could not be inspected; declining the \
                     bucket rather than writing an empty file over it (non-fatal)"
                );
                return None;
            }
        };
        if registry_kind == anvil_core::domain::playbook::fs_probe::NodeKind::Absent {
            if let Err(e) = std::fs::write(&registry, "") {
                tracing::warn!(
                    outcome = "unattributed_bucket_registry_failed",
                    error = %e,
                    "could not seed the (unattributed) bucket registry (non-fatal)"
                );
                return None;
            }
        }
        Some(bucket)
    }
}

impl AnvilServer {
    /// Drive ONE governed K8 lifecycle row from the public Snapshot RPC.
    ///
    /// The caller supplies only the target state, the driver role, and an
    /// optional approver. The engine reads the strict source state itself
    /// (there is no caller-declared `from` to disagree with the store),
    /// prepares against the full organ context, and consumes the resulting
    /// capability exactly once through the compound writer. A rejection at any
    /// step leaves every K8 file byte-identical because nothing is written
    /// before the commit.
    async fn backlog_snapshot(
        &self,
        resolved: &Path,
        adapter: &FileSystemSnapshotAdapter,
        req: &SnapshotRequest,
        actor_name: String,
        at: &str,
    ) -> Result<SnapshotResponse, Status> {
        use anvil_core::domain::backlog_item as k8;
        use anvil_core::ports::snapshot_port::SnapshotPort;

        // A projection-only event is not a lifecycle move and must never be a
        // side door into a backlog_item.
        if req.projection_only {
            return Err(Status::failed_precondition(
                "a backlog_item takes no projection_only snapshot: K8 state moves only \
                 through the governed prepare/commit path",
            ));
        }

        let bi_id = last_segment(&req.artifact_path).to_string();
        let role = k8::parse_driver_role_token(&req.actor_role)
            .map_err(|e| Status::invalid_argument(e.to_string()))?;
        let to = k8::parse_state_token(req.to_state.trim())
            .map_err(|e| Status::invalid_argument(e.to_string()))?;
        let actor = backlog_actor_identity(
            &actor_name,
            &req.actor_type,
            &req.actor_model,
            &req.actor_provider,
            req.actor_context_window,
            &req.actor_sdk_version,
            &req.actor_entrypoint,
        )?;
        let approver = Some(req.approver.trim()).filter(|a| !a.is_empty());

        let source = adapter
            .read_backlog_item(&bi_id)
            .map_err(snapshot_error_to_status)?;
        let from = source.item.state;

        let (_, commit) = anvil_core::domain::snapshot::prepare_k8_transition(
            adapter,
            &bi_id,
            from,
            to,
            role,
            &actor,
            at,
            approver,
            k8::TransitionOrigin::public_snapshot(),
            &format!(
                "snapshot-{bi_id}-{}",
                anvil_core_hearth::fs_transition_event_adapter::short_random_id()
            ),
            &anvil_core_hearth::fs_transition_event_adapter::hi_res_prefix(),
            &anvil_core_hearth::fs_transition_event_adapter::short_random_id(),
        )
        .map_err(snapshot_error_to_status)?;

        anvil_core::domain::snapshot::commit_k8_transition(adapter, commit)
            .map_err(snapshot_error_to_status)?;

        Ok(SnapshotResponse {
            success: true,
            timestamp: at.to_string(),
            status_updated: true,
            registry_updated: true,
            projections_updated: vec!["backlog_items.md".to_string()],
            warnings: Vec::new(),
            actor_name,
            resolved_hearth: resolved.display().to_string(),
        })
    }
}

struct SourceAwarePlaybookHookBodyReader {
    request_hearth: PathBuf,
}

impl PlaybookHookBodyPort for SourceAwarePlaybookHookBodyReader {
    fn read_playbook_hook_body(
        &self,
        source: &PlaybookSource,
        filename: &str,
    ) -> Result<String, QueryError> {
        let hearth = source.hearth.as_ref().unwrap_or(&self.request_hearth);
        FileSystemQueryAdapter::new(hearth.clone())
            .read_playbook_hook_body(&source.playbook_id, filename)
    }
}

#[tonic::async_trait]
impl AnvilService for AnvilServer {
    async fn catalog(
        &self,
        request: Request<CatalogRequest>,
    ) -> Result<Response<CatalogResponse>, Status> {
        // Gatekeeper FIRST (spec Req 5): refuse unauth callers before any
        // hearth resolution or lock. Standalone ⇒ Ok(None), no-op.
        let _session = self.authorize(&request).await?;
        // Offload the synchronous hearth fold + registry rebuild off the async
        // worker (block_in_place, not spawn_blocking, because the body borrows
        // &self and builds a non-Send registry). No `.await` remains after
        // authorize, so this is one synchronous segment. See `route` for the full
        // rationale (keeps `/health` schedulable under concurrent folds).
        tokio::task::block_in_place(move || {
        let req = request.into_inner();
        let resolved = resolve_hearth(
            &req.hearth_path,
            self.hearth_path.as_deref(),
            &self.hearth_policy,
        )?;
        let reader = FileSystemHearthReader::new(resolved.clone());
        let registry = self.request_session_registry(&resolved).registry;

        let result = CatalogQueryHandler::execute(&reader)
            .map_err(|e| Status::internal(format!("{}", e)))?;

        // Query RPC: log hearth + command + outcome only. No actor, no events
        // (Req 3 — catalog carries neither; do not fabricate).
        tracing::info!(
            command = "catalog",
            hearth = %resolved.display(),
            outcome = "ok",
            "catalog query ok"
        );

        // Universal activity log (additive). Catalog carries no actor and no
        // artifact_kind — record the command + ok outcome only.
        self.emit_activity(
            "catalog",
            "ok",
            "",
            "",
            "",
            "",
            &resolved,
            &chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            "",
            None,
            None,
            None,
            None,
        );

        let response = CatalogResponse {
            active_artifacts: result
                .active_artifacts
                .into_iter()
                .map(|a| anvil_engine::proto::ActiveArtifact {
                    id: a.id,
                    artifact_type: a.artifact_type.as_str().to_string(),
                    state: a.state,
                    summary: a.summary,
                    execution_route: a.execution_route,
                })
                .collect(),
            available_types: result
                .available_types
                .into_iter()
                .map(|t| anvil_engine::proto::AvailableArtifactType {
                    name: t.name,
                    description: t.description,
                    requires_parent: t.requires_parent,
                    execution_route: t.execution_route,
                })
                .collect(),
            invalid_artifacts: result
                .invalid_artifacts
                .into_iter()
                .map(|iv| anvil_engine::proto::InvalidArtifact {
                    id: iv.id,
                    code: iv.code,
                    message: iv.message,
                    params: iv.params.into_iter().collect(),
                })
                .collect(),
            resolved_hearth: resolved.display().to_string(),
            available_artifact_kinds: registry
                .all_machines()
                .into_iter()
                .map(|machine| anvil_engine::proto::AvailableArtifactKind {
                    is_described: !machine.description.trim().is_empty(),
                    has_triggers: !machine.route.triggers.is_empty(),
                    kind: machine.kind.clone(),
                    source_tier: playbook_source_tier(
                        registry.source_for(&machine.kind).as_ref(),
                        &resolved,
                        self.global_playbooks_hearth.as_deref(),
                    )
                    .to_string(),
                })
                .collect(),
        };

        Ok(Response::new(response))
        })
    }

    async fn checkin(
        &self,
        request: Request<CheckinRequest>,
    ) -> Result<Response<CheckinResponse>, Status> {
        // Gatekeeper FIRST (spec Req 5). Standalone ⇒ Ok(None), no-op.
        let _session = self.authorize(&request).await?;
        // Offload the synchronous hearth fold + registry rebuild off the async
        // worker (block_in_place — borrows &self + non-Send registry). One
        // synchronous segment (no `.await` after authorize). See `route`.
        tokio::task::block_in_place(move || {
        let req = request.into_inner();
        let resolved = resolve_hearth(
            &req.hearth_path,
            self.hearth_path.as_deref(),
            &self.hearth_policy,
        )?;
        let adapter = FileSystemCheckinQueryAdapter::new(resolved.clone());

        let supplied_actor_name = req.actor_name;
        // T4: only a caller-supplied name can have a prior begin to re-serve; a
        // freshly generated name never does. Capture before the move below.
        let supplied_actor_name_for_reserve = supplied_actor_name.clone();

        let domain_request = CheckinQueryRequest {
            role: req.role,
            actor_type: req.actor_type,
            actor_model: req.actor_model,
            actor_provider: req.actor_provider,
        };

        let role_str = domain_request.role.clone();
        let result = CheckinQueryHandler::execute(&adapter, domain_request)
            .map_err(|e| checkin_error_to_status(e))?;

        // Per spec R1: if the caller supplied a non-empty actor_name,
        // echo it verbatim as the canonical session name (no hearth
        // lookup, no validation). Otherwise use the freshly generated
        // name from the query handler.
        let canonical_actor_name = if supplied_actor_name.is_empty() {
            result.actor_name
        } else {
            supplied_actor_name
        };

        let next_step = checkin_next_step(
            &role_str,
            &result.filtered_artifacts,
            &result.available_types,
        );

        // T4 — re-serve hook content on checkin. For a resumed/compacted session
        // the caller supplies its prior actor_name; for any filtered artifact
        // that actor has an OPEN begin on (in its current state), re-serve that
        // state's hook so the standing context is re-warmed. Reuses the same
        // budget-capped serve path as `begin` (begin::reserve_hook_for_open_begin).
        // Purely additive: a fresh/anonymous actor (empty supplied name) or one
        // with no open begin yields empty `context`, and any per-artifact read
        // error is skipped rather than failing checkin.
        let context = if supplied_actor_name_for_reserve.is_empty() {
            String::new()
        } else {
            let query_adapter = FileSystemQueryAdapter::new(resolved.clone());
            let registry = self.request_session_registry(&resolved).registry;
            let mut sections: Vec<String> = Vec::new();
            for artifact in &result.filtered_artifacts {
                let kind = artifact.artifact_type.as_str();
                if let Ok(body) = anvil_core::domain::begin::reserve_hook_for_open_begin_via_query(
                    &query_adapter,
                    registry.as_ref(),
                    &artifact.id,
                    kind,
                    &artifact.state,
                    &canonical_actor_name,
                ) {
                    if !body.is_empty() {
                        sections.push(body);
                    }
                }
            }
            sections.join("\n\n")
        };

        // M-P3: checkin emits a degenerate step_measurement record (D-3) —
        // orientation has no artifact/kind/state, so only role + actor + at are
        // populated; every other contract key is present-but-empty. No registry
        // needed (checkin has no kind). tokens/duration_ms absent (D-4).
        {
            let at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
            emit_step_measurement(
                "",
                "",
                "",
                "",
                &role_str,
                &canonical_actor_name,
                "",
                "",
                &at,
            );
        }

        // Query RPC: log hearth + command + outcome. checkin carries an actor
        // field, so log it — but only when non-empty (Req 3). No events.
        if canonical_actor_name.is_empty() {
            tracing::info!(
                command = "checkin",
                hearth = %resolved.display(),
                outcome = "ok",
                "checkin query ok"
            );
        } else {
            tracing::info!(
                command = "checkin",
                actor = %canonical_actor_name,
                hearth = %resolved.display(),
                outcome = "ok",
                "checkin query ok"
            );
        }

        // Universal activity log (additive). checkin carries an actor but no
        // artifact_kind (orientation touches no single kind).
        self.emit_activity(
            "checkin",
            "ok",
            "",
            "",
            "",
            &canonical_actor_name,
            &resolved,
            &chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            "",
            None,
            None,
            None,
            None,
        );

        Ok(Response::new(CheckinResponse {
            actor_name: canonical_actor_name,
            filtered_artifacts: result
                .filtered_artifacts
                .into_iter()
                .map(|a| anvil_engine::proto::ActiveArtifact {
                    id: a.id,
                    artifact_type: a.artifact_type.as_str().to_string(),
                    state: a.state,
                    summary: a.summary,
                    execution_route: a.execution_route,
                })
                .collect(),
            available_types: result
                .available_types
                .into_iter()
                .map(|t| anvil_engine::proto::AvailableArtifactType {
                    name: t.name,
                    description: t.description,
                    requires_parent: t.requires_parent,
                    execution_route: t.execution_route,
                })
                .collect(),
            next_step,
            resolved_hearth: resolved.display().to_string(),
            context,
        }))
        })
    }

    async fn describe(
        &self,
        request: Request<DescribeRequest>,
    ) -> Result<Response<DescribeResponse>, Status> {
        // Gatekeeper FIRST (spec Req 5). Standalone ⇒ Ok(None), no-op.
        let _session = self.authorize(&request).await?;
        // Offload the synchronous hearth fold + registry rebuild off the async
        // worker (block_in_place — borrows &self + non-Send registry). One
        // synchronous segment (no `.await` after authorize). See `route`.
        tokio::task::block_in_place(move || {
        let req = request.into_inner();
        let resolved = resolve_hearth(
            &req.hearth_path,
            self.hearth_path.as_deref(),
            &self.hearth_policy,
        )?;
        // Per-request construction IS the always-reload mechanism (spec R7.2, AC7).
        // HearthPlaybookRegistry performs a full disk scan on construction; constructing
        // a fresh instance on every describe RPC call means machine.yaml edits are
        // reflected without engine restart or rebuild — Criterion 1 of
        // orchestrator_driven_track_lifecycle.
        let registry = self.request_session_registry(&resolved).registry;

        // Build the adapter FROM the registry so `read_instance` scans every
        // directory a registered machine declares (`PlaybookMachine::directory`)
        // in addition to the core hearth dirs — resolving `playbook_generation`
        // instances (dir `playbook_generations`) and any future machine kind
        // without another hardcode.
        let adapter =
            FileSystemDescribeAdapter::with_registry(resolved.clone(), registry.as_ref());

        let domain_request = DomainDescribeRequest {
            identifier: req.identifier,
        };

        let result = DescribeQueryHandler::execute(&adapter, registry.as_ref(), domain_request)
            .map_err(|e| describe_error_to_status(e))?;

        // Query RPC: log hearth + command + outcome only. No actor, no events
        // (Req 3 — describe carries neither; do not fabricate).
        tracing::info!(
            command = "describe",
            hearth = %resolved.display(),
            outcome = "ok",
            "describe query ok"
        );

        // Universal activity log (additive). describe carries no actor.
        self.emit_activity(
            "describe",
            "ok",
            "",
            "",
            "",
            "",
            &resolved,
            &chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            "",
            None,
            None,
            None,
            None,
        );

        let response = match result {
            DescribeResult::TypeInfo {
                name,
                description,
                required_fields,
                parent_type,
            } => DescribeResponse {
                info: Some(anvil_engine::proto::describe_response::Info::TypeInfo(
                    anvil_engine::proto::TypeInfo {
                        name,
                        description,
                        required_fields,
                        parent_type,
                    },
                )),
                next_step: describe_next_step_for_type(),
                resolved_hearth: resolved.display().to_string(),
            },
            DescribeResult::InstanceInfo {
                id,
                artifact_type,
                state,
                transition_count,
                last_transition,
                available_actions,
            } => {
                let next_step = describe_next_step_for_instance(&available_actions);
                DescribeResponse {
                    info: Some(anvil_engine::proto::describe_response::Info::InstanceInfo(
                        anvil_engine::proto::InstanceInfo {
                            id,
                            artifact_type,
                            state,
                            transition_count: transition_count as i32,
                            last_transition: last_transition.map(|t| {
                                anvil_engine::proto::TransitionDetail {
                                    to: t.to,
                                    at: t.at,
                                    actor: t.actor,
                                    role: t.role,
                                }
                            }),
                            available_actions: available_actions
                                .into_iter()
                                .map(|a| anvil_engine::proto::ActionInfo {
                                    action: a.action,
                                    required_role: a.required_role,
                                    execution_route: a.execution_route,
                                })
                                .collect(),
                        },
                    )),
                    next_step,
                    resolved_hearth: resolved.display().to_string(),
                }
            }
        };

        Ok(Response::new(response))
        })
    }

    async fn route(
        &self,
        request: Request<RouteRequest>,
    ) -> Result<Response<RouteResponse>, Status> {
        // Gatekeeper FIRST (spec Req 5). Standalone ⇒ Ok(None), no-op.
        let _session = self.authorize(&request).await?;
        let req = request.into_inner();

        // Offload the ENTIRE synchronous route computation off the async worker.
        // route runs EVERY turn and, inline, rebuilds the playbook registry
        // (sync read_dir + YAML parse, 3-4× per call) and folds the caller's
        // hearth for the open-playbook lookup — a hearth-size-growing blocking
        // fold on the shared runtime that Foundry's `/health` probe also lives on.
        // Enough concurrent route/dashboard folds saturate the async workers so
        // the trivial `/health` handler can't be scheduled → 6 watchdog fails →
        // SIGTERM → crash-loop.
        //
        // `block_in_place` (NOT `spawn_blocking`): the body borrows `&self` and
        // builds a non-`Send` `Box<dyn PlaybookRegistry>` that is used throughout,
        // so it cannot cross a `spawn_blocking` `Send + 'static` boundary without a
        // `Box<dyn PlaybookRegistry + Send>` refactor across ~60 sites (the exact
        // higher-risk surgery we are avoiding). `block_in_place` keeps the borrow
        // on THIS thread and instead hands the runtime's scheduler core to another
        // worker for the fold's duration, so `/health` (and every other task) stays
        // schedulable regardless of how heavy/concurrent the folds get. Sound here
        // because there is NO `.await` after `authorize` — the whole body is one
        // synchronous segment (requires the multi-thread runtime, which
        // `#[tokio::main]` always provides).
        tokio::task::block_in_place(move || {

        // The route hook is fire-and-forget (it cannot ask an LLM to retry), so
        // route MUST fail open: it never returns an Err for a missing or
        // unresolvable hearth. Attribution and registry resolution are SEPARATE
        // concerns:
        //
        //   * The ATTRIBUTION hearth (where the routing-activity + activity-log
        //     records are charged) is the caller's REAL hearth — and ONLY a real,
        //     explicitly-supplied, valid+permitted hearth. We deliberately do NOT
        //     fall back to the spawn default for attribution: a source-less turn
        //     defaulting into the global anvil-hearth is exactly the corruption
        //     this change removes.
        //   * The REGISTRY hearth (where playbook machines are resolved FROM) may
        //     still fall back to the default / global playbooks hearth — the
        //     registry is global by design, so routing keeps working without a
        //     request hearth.
        let attribution_hearth: Option<PathBuf> = if req.hearth_path.is_empty() {
            None
        } else {
            // An explicit hearth_path that resolves+validates+permits is the only
            // thing we attribute to. An explicit-but-bad path is NOT a silent
            // default — but route is fail-open, so we drop attribution rather than
            // erroring the turn.
            match resolve_hearth(&req.hearth_path, None, &self.hearth_policy) {
                Ok(resolved) => Some(resolved),
                Err(status) => {
                    tracing::warn!(
                        command = "route",
                        outcome = "attribution_hearth_unresolved",
                        error_code = ?status.code(),
                        "route attribution hearth could not be resolved (fail-open)"
                    );
                    None
                }
            }
        };

        // The registry hearth: the caller's hearth if attributable, else the
        // spawn default, else the global playbooks hearth. A fresh
        // CompositePlaybookRegistry per call IS the always-reload mechanism (spec
        // M-1) and always layers global + seed under the request tier, so even an
        // empty request hearth resolves global/seed kinds.
        let registry_hearth: Option<PathBuf> = attribution_hearth
            .clone()
            .or_else(|| self.hearth_path.clone())
            .or_else(|| self.global_playbooks_hearth.clone());

        // Per-request construction IS the always-reload mechanism (spec M-1),
        // mirroring describe: a fresh CompositePlaybookRegistry per call means
        // machine.yaml + register edits live-reload without engine restart. With
        // no registry hearth at all (pure hearth-less, no global), fall back to
        // the global/seed-only registry keyed at the global hearth (or, absent
        // that, an empty path — SeedPlaybookRegistry still answers).
        let registry_base = registry_hearth.clone().unwrap_or_else(|| PathBuf::from(""));
        let registry = self.request_session_registry(&registry_base).registry;
        let ctx = proto_request_context(
            &req.ctx_org,
            &req.ctx_space,
            &req.ctx_role,
            &req.ctx_clearance,
        );

        let route_turn_id = req.conversation_id.clone();
        let route_input = routing_input(&req.message, &req.signal);
        let route_conversation_hash = self.conversation_hash(&req.conversation_id);
        let route_project_label = Self::project_label(&req.project_root);

        // ---------------------------------------------------------------------
        // resume_aware_routing — resume pre-check (Phase 3). BEFORE normal
        // resolution: if the message is a continuation token AND the
        // conversation has an open (non-terminal) playbook, override the outcome
        // to `resume` and point the agent back at the in-progress step. The
        // guard (req #4) is strict: exact token set + a real open playbook only;
        // any non-token message skips the pre-check entirely and routes normally
        // (selection scoring untouched, req #7). The lookup scans the caller's
        // real hearth (where begins recorded their markers); absent that, no
        // resume fires (degrade gracefully).
        // The route handler is a pure read over an always-constructible registry.
        // BP2b routes through the pure resolver so access is applied before
        // trigger matching. It is computed HERE, above the continuation
        // pre-check, because step 3 of the decision procedure needs to know
        // whether the ORIGINAL message carries new intent — and answering that
        // with anything other than the engine's own matcher would be a second
        // matcher that drifts.
        let mut resolution = resolve_route(registry.as_ref(), &ctx, &route_input);

        // continuation_recognition: resolve the assistant's prior proposal to a
        // kind, if it named exactly one. Ambiguity DECLINES — zero or several
        // matches are treated as no proposal at all, never as a pick.
        //
        // The trigger must also begin at the proposal's FIRST token: that is what
        // separates "record this as a decision" (a proposal) from "the report
        // says start a track" (prose that merely contains a trigger). Single-kind
        // resolution proves routability, not proposal intent.
        let proposal_kind: Option<String> = req.prior_proposal.as_ref().and_then(|p| {
            let text = p.text.trim();
            if text.is_empty() {
                return None;
            }
            let pr = resolve_route(registry.as_ref(), &ctx, &routing_input(text, ""));
            if pr.matching_candidates.len() != 1 {
                return None;
            }
            let kind = pr.matching_candidates[0].clone();
            let trigger = pr.match_signals.get(&kind)?.to_lowercase();
            let lowered = text.to_lowercase();
            if trigger.is_empty() || !lowered.starts_with(&trigger) {
                return None;
            }
            Some(kind)
        });

        let continuation = {
            let lookup_hearth = attribution_hearth
                .clone()
                .or_else(|| self.hearth_path.clone());
            let open = lookup_hearth.as_ref().and_then(|hearth| {
                self.open_playbook_run_for_conversation(
                    hearth,
                    registry.as_ref(),
                    &req.conversation_id,
                )
                .map(|o| (hearth.clone(), o))
            });
            let outcome = anvil_core::domain::route::resolve_continuation(
                &anvil_core::domain::route::ContinuationInputs {
                    message: &req.message,
                    has_open_run: open.is_some(),
                    open_kind: open.as_ref().map(|(_, o)| o.kind.as_str()).unwrap_or(""),
                    has_new_intent: !resolution.matching_candidates.is_empty(),
                    has_recent_context: !req.recent_context.trim().is_empty(),
                    proposal_kind: proposal_kind.as_deref(),
                },
            );
            (outcome, open)
        };

        // Steps 5-7 that RESUME the open run take the same path today's
        // exact-token resume takes — same telemetry, same dedup re-arm, same
        // response builder — differing only in the recorded reason. Keeping one
        // construction is what stops the widened paths drifting from the proven
        // one.
        let resumes_open_run = matches!(
            continuation.0,
            anvil_core::domain::route::ContinuationOutcome::Resume
                | anvil_core::domain::route::ContinuationOutcome::ResumeContextual
                | anvil_core::domain::route::ContinuationOutcome::ResumeWidened
        );
        let continuation_source = continuation.0.resume_source();

        // A REJECTION suppresses substitution only — the message still routes
        // normally below. It is recorded under its own reason so the accepted
        // losses of the deliberately over-broad marker set are COUNTABLE. The
        // spec's earlier claim that they would "land in the existing
        // no_candidate bucket" was false: `abstain("no_candidate")` is a flat
        // string, so a rejected continuation would have been indistinguishable
        // among hundreds of identical rows — the same conflated-cause defect
        // `engine_call_failed` was split into four causes to fix.
        if matches!(
            continuation.0,
            anvil_core::domain::route::ContinuationOutcome::Rejected
        ) {
            tracing::info!(
                command = "route",
                resume_source = "rejected",
                "continuation declined: rejection or deferral marker present"
            );
        }
        if resumes_open_run {
            let lookup_hearth = attribution_hearth
                .clone()
                .or_else(|| self.hearth_path.clone());
            if let Some(hearth) = lookup_hearth {
                // Indexed lookup (open_marker_index Phase 1), fail-open to scan.
                if let Some(open) = self.open_playbook_run_for_conversation(
                    &hearth,
                    registry.as_ref(),
                    &req.conversation_id,
                ) {
                    let registry_hearth_display = registry_hearth
                        .as_deref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    let rd_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
                    tracing::info!(
                        command = "route",
                        outcome = "resume",
                        resolution_outcome = "resume",
                        resume_source = %continuation_source,
                        resume_artifact = %open.artifact_id,
                        resume_kind = %open.kind,
                        resume_state = %open.state,
                        "route resolved to resume (continuation token + open playbook)"
                    );
                    // Record the routing decision telemetry (resume outcome,
                    // no selected kind — resume is not a fresh invocation). The
                    // `confidence`/marker field carries `continuation_token` so
                    // measurement can DISTINGUISH an explicit continuation-token
                    // resume from a MID_PLAYBOOK_RUN matched-turn check-in nudge
                    // (which tags `mid_run_nudge`) — never conflated.
                    emit_routing_decision(
                        "route",
                        &route_turn_id,
                        &route_input,
                        "",
                        "",
                        "resume",
                        continuation_source,
                        &rd_at,
                    );

                    // A continuation token is an explicit "keep going" and is
                    // NEVER deduped (spec req #3); it also RE-ARMS the dedup for
                    // the resumed ARTIFACT so a subsequent matched-turn check-in
                    // nudge is not suppressed and the sticky moved-on counter
                    // resets (the user is actively engaged again — a real change).
                    self.nudge_dedup
                        .rearm(&hearth, &req.conversation_id, &open.artifact_id);

                    self.emit_activity(
                        "route",
                        "resume",
                        &open.kind,
                        "",
                        "",
                        &req.actor_name,
                        &hearth,
                        &rd_at,
                        &req.source,
                        route_conversation_hash.clone(),
                        route_project_label.clone(),
                        Some(open.artifact_id.clone()),
                        Some(CallState::Resume.as_str().to_string()),
                    );

                    // Shared construction (no drift with the matched-turn
                    // check-in nudge path below).
                    let mut response = resume_response_for_open_playbook_run(
                        registry.as_ref(),
                        &registry_base,
                        &registry_hearth_display,
                        route_conversation_hash.as_deref().unwrap_or(""),
                        open,
                    );
                    // The builder defaults to the exact-token reason; the widened
                    // paths overwrite it with their own. Recorded independently of
                    // the outcome so the two questions — what was delivered, and
                    // which branch decided — stay separable.
                    response.resume_source = continuation_source.to_string();
                    return Ok(Response::new(response));
                }
                // Continuation token but no open playbook for this conversation →
                // fall through to normal routing (req #6).
            }
        }

        // (resolution computed ABOVE the continuation pre-check — see there.)

        // SEMANTIC ROUTE RPC (dark-launch, default OFF via ANVIL_SEMANTIC_ROUTE_RPC).
        // When enabled and the semantic plan is eligible, ask the Kiln router to
        // commit to ONE of the granted set or abstain, then fold the verdict into
        // `resolution` before all downstream
        // telemetry / nudge / response logic sees it. Fail-open: a Kiln miss
        // (unreachable / timeout / parse-gap) yields RouterVerdict::Fallback, which
        // apply_semantic_verdict maps back to the UNCHANGED lexical resolution — the
        // RPC is a shared front door and must not degrade to "never route" on a Kiln
        // blip (the deliberate divergence from the hook, which goes silent).
        //
        // Empty matching set normally skips the call. The default-off V1 gate-breadth
        // experiment also admits it when any full-granted signal has content overlap.
        //
        // Async boundary: kiln_router::route builds its OWN current-thread tokio
        // runtime + block_on. This whole handler body runs inside `block_in_place`
        // — a SYNCHRONOUS segment (there is no `.await` here; see the block_in_place
        // rationale above) — and block_in_place KEEPS the runtime context on this
        // thread, so calling kiln_router::route directly would panic "runtime within
        // runtime". Run it on a dedicated std thread (which carries NO tokio runtime
        // context) and join: the sync-context equivalent of spawn_blocking, giving
        // the same no-nested-runtime guarantee without a second (non-Send) registry
        // rebuild. The call is internally time-boxed (~5s), so the join is bounded;
        // a panicked thread degrades to Fallback (fail-open preserved).
        let configured_brief_cap = router_v2_brief_cap();
        let semantic_plan = semantic_route_plan_with_brief_cap(
            &resolution,
            router_v1_gate_breadth_enabled(),
            configured_brief_cap.as_deref(),
        );
        if let Some(invalid_cap) = &semantic_plan.invalid_brief_cap {
            tracing::warn!(
                brief_cap = %invalid_cap,
                "invalid ANVIL_ROUTER_V2_BRIEF_CAP; falling back to all briefs"
            );
        }
        if semantic_route_rpc_enabled() && semantic_plan.eligible {
            // candidate_recall lever: feed the semantic router the FULL granted set,
            // not just the lexical top-3 matching set. The semantic layer selects
            // precisely (and abstains correctly), so a WIDER candidate list raises
            // recall — the right kind can be picked even when the lexical floor ranked
            // it below the top-3. Gated on matching being non-empty so the hard
            // no-task / free-kind gates (which also yield empty matching) still short
            // to no_match without a Kiln call.
            let candidate_count = semantic_plan.brief_kinds.len() as u64;
            let briefs = build_candidate_briefs(&semantic_plan.brief_kinds, registry.as_ref());
            let semantic_message = route_input.clone();
            let verdict = std::thread::spawn(move || {
                kiln_router::route(
                    &semantic_message,
                    "",
                    &InProgressSignal::None,
                    &briefs,
                    "engine",
                )
                .verdict
            })
            .join()
            .unwrap_or(RouterVerdict::Fallback);
            // Content-free semantic-rank telemetry (best-effort, opt-in gated):
            // how many candidates the pass saw + whether it fell open to the
            // lexical floor. No message/candidate text ever rides along.
            let dominant_fallback_used = matches!(verdict, RouterVerdict::Fallback);
            anvil_engine::telemetry::record_semantic_rank(
                None,
                candidate_count,
                if dominant_fallback_used { "yes" } else { "no" },
            );
            resolution =
                apply_semantic_verdict_with_briefs(resolution, verdict, &semantic_plan.brief_kinds);
        }

        let resolution_outcome = route_resolution_outcome_label(&resolution.outcome);
        let legacy_outcome = match &resolution.outcome {
            RouteOutcome::Single | RouteOutcome::Candidates => "candidates",
            RouteOutcome::NoMatch => "no_match",
        };

        let registry_hearth_display = registry_hearth
            .as_deref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        tracing::info!(
            command = "route",
            hearth = %registry_hearth_display,
            outcome = legacy_outcome,
            resolution_outcome = resolution_outcome,
            selected_kind = resolution.selected_kind.as_deref().unwrap_or(""),
            granted_count = resolution.granted_candidates.len(),
            relevant_count = resolution.matching_candidates.len(),
            "route query ok"
        );

        let route_candidate_set = resolution.granted_candidates.join(",");
        let route_selected = match &resolution.outcome {
            RouteOutcome::Single => resolution.selected_kind.as_deref().unwrap_or(""),
            RouteOutcome::Candidates | RouteOutcome::NoMatch => "",
        };
        let rd_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        emit_routing_decision(
            "route",
            &route_turn_id,
            &route_input,
            &route_candidate_set,
            route_selected,
            resolution_outcome,
            "",
            &rd_at,
        );

        // The conversation's open (begun, non-terminal) playbook — looked up ONCE
        // and reused for (a) the Layer-1 call-state classification, (b) the
        // MID_PLAYBOOK_RUN matched-turn check-in nudge (mid_run_checkin_nudge),
        // AND (c) the no_match-while-open reminder (no_match_open_reminder,
        // open_marker_index Phase 2). Now that the lookup is CHEAP (the indexed
        // path reads only candidate artifacts, falling open to a scan on a cold
        // conversation), it runs on EVERY non-empty-conversation turn — including
        // `no_match` — so the dangling-instance catch fires on every turn, not
        // just matched turns. An empty-conversation turn never runs it (it can
        // never be mid-playbook / have a resumable open playbook). The lookup
        // uses the SAME hearth the resume pre-check uses
        // (attribution_hearth.or(self.hearth_path)) and is fail-open: a missing
        // hearth or lookup error degrades to `None` (never a false MID_PLAYBOOK_RUN /
        // false check-in / false reminder without a confirmed open playbook).
        let route_open_playbook_run: Option<OpenPlaybookRun> = if req.conversation_id.trim().is_empty() {
            None
        } else {
            attribution_hearth
                .clone()
                .or_else(|| self.hearth_path.clone())
                .and_then(|hearth| {
                    self.open_playbook_run_for_conversation(
                        &hearth,
                        registry.as_ref(),
                        &req.conversation_id,
                    )
                })
        };

        // Resolve where attribution records are CHARGED, distinct from the
        // registry hearth. The rule (never default-attribute to the global
        // anvil-hearth for a source-less turn):
        //   * caller's real hearth          ⇒ attribute there (source verbatim).
        //   * no caller hearth + spawn default ⇒ attribute to the default
        //       (no-regression: the documented "empty hearth_path uses --hearth
        //       default" standalone single-project contract).
        //   * no caller hearth + no default + global ⇒ attribute to the
        //       `(unattributed)` bucket (the production hearth-less daemon path).
        //   * neither ⇒ skip the attribution write (fail-open).
        // `attribution_source` is the caller's source verbatim, except a
        // source-less turn landing in the bucket is tagged `(unattributed)`.
        let (attribution_target, attribution_source): (Option<PathBuf>, String) =
            match &attribution_hearth {
                Some(h) => (Some(h.clone()), req.source.clone()),
                None => match self.hearth_path.clone() {
                    Some(default) => (Some(default), req.source.clone()),
                    None => {
                        let source = if req.source.is_empty() {
                            "(unattributed)".to_string()
                        } else {
                            req.source.clone()
                        };
                        (self.unattributed_bucket(), source)
                    }
                },
            };

        // Durable, redacted routing-activity append (CQRS write half). The
        // stderr `routing_decision` event above is not queryable; the
        // PlaybookActivity read-side folds THIS sink for per-kind call counts.
        // Only the Part-3 allowlisted fields are written (kind + outcome label +
        // timestamp) — never the message text, identities, or paths. We record a
        // call only when a single kind resolved (an actual invocation); the
        // candidates/no_match halves carry no called kind. Best-effort: an append
        // failure logs a warning by variant name and never fails the read.
        if let Some(target) = attribution_target.as_deref() {
            if !route_selected.is_empty() {
                let record = RoutingActivityRecord {
                    kind: route_selected.to_string(),
                    outcome: resolution_outcome.to_string(),
                    at: rd_at.clone(),
                    conversation_hash: route_conversation_hash.clone(),
                    project_label: route_project_label.clone(),
                };
                if let Err(e) =
                    FileSystemRoutingActivityAdapter::new(target).append_routing_activity(&record)
                {
                    tracing::warn!(
                        command = "route",
                        outcome = "routing_activity_append_failed",
                        error = %e,
                        "routing-activity sink append failed (non-fatal)"
                    );
                }
            }

            // Universal activity log (additive). Unlike the routing-activity sink
            // (single-resolution only), record EVERY route outcome — single,
            // candidates, AND no_match — so routed-vs-abstained is measurable. The
            // artifact_kind is the selected kind (empty for candidates/no_match).
            //
            // Layer-1 call-state (honest-adoption classification): a no_match is
            // NO_PLAYBOOK_RUN; a matched turn is MID_PLAYBOOK_RUN when the conversation
            // already has an open (non-terminal) playbook and START_OPPORTUNITY
            // otherwise. Reuses the single open-playbook lookup hoisted above (no
            // second scan). An empty conversation_id resolves no open playbook, so
            // it falls back to START_OPPORTUNITY by construction.
            // resume-signal context-awareness (P1 telemetry half): an UNRELATED
            // matched turn (the open playbook's kind is NOT in this turn's
            // matching set — the agent moved to different work) is NORMAL routing,
            // NOT mid_playbook_run. classify_call_state applies that relevance gate.
        }

        // Open-playbook check-in nudge / reminder — WITH the resume-signal
        // relevance gate (resume-signal context-awareness, P1). The conversation
        // has an open (begun, non-terminal) playbook. Whether to surface a resume
        // now depends on whether THIS turn RELATES to that open playbook:
        //
        //   * a MATCHED turn whose `matching_candidates` INCLUDE the open kind
        //     (RELATED) → `mid_run_nudge`: resume the open playbook (as
        //     before). A related resume ALWAYS fires and RE-ARMS the sticky
        //     suppression — a real related resume is never quieted.
        //   * a MATCHED turn whose match does NOT include the open kind
        //     (UNRELATED — the agent moved to different work) → SUPPRESS the
        //     resume; fall through to the normal route response (candidates /
        //     matching_candidates / selected_kind preserved), never nagging.
        //   * a `no_match` turn → `no_match_open_reminder` (the dangling-instance
        //     catch), subject to the time-window dedup below.
        //
        // Sticky "moved-on" suppression (P2): each consecutive UNRELATED turn
        // (matched-other or idle-no_match) with NO intervening
        // begin/snapshot/complete/continuation rearm increments a per-artifact
        // counter. Once it exceeds `UNRELATED_QUIET_THRESHOLD` the resume/reminder
        // for that artifact goes QUIET and a PARK HINT is surfaced ONCE (so the
        // agent can abandon the dangling track instead of being nagged). A later
        // RELATED turn or any lifecycle rearm resets the counter. All dedup state
        // is keyed by the open ARTIFACT id, so two open tracks of the same kind in
        // one conversation stay independent.
        //
        // NOT a hard block (spec req #3): the agent retains agency to begin a
        // different playbook. Fail-open: `route_open_playbook_run` is `None` on any
        // lookup error / empty conversation / no open playbook.
        let mut route_park_hint: Option<ParkHint> = None;
        if let Some(ref open) = route_open_playbook_run {
            let is_no_match = matches!(resolution.outcome, RouteOutcome::NoMatch);
            let related =
                !is_no_match && open_playbook_run_is_relevant(&open, &resolution.matching_candidates);
            let dedup_hearth = attribution_hearth
                .clone()
                .or_else(|| self.hearth_path.clone());

            if related {
                // RELATED matched turn → resume. Re-arm resets the sticky
                // unrelated suppression + park-hint state for this artifact — a
                // real related resume is never quieted (Codex P2).
                if let Some(h) = dedup_hearth.as_deref() {
                    self.nudge_dedup
                        .rearm(h, &req.conversation_id, &open.artifact_id);
                }
                tracing::info!(
                    command = "route",
                    outcome = "resume",
                    resolution_outcome = "resume",
                    resume_source = "mid_run_nudge",
                    resume_artifact = %open.artifact_id,
                    resume_kind = %open.kind,
                    resume_state = %open.state,
                    "route surfaced an open-playbook check-in nudge (relevant turn)"
                );
                let nudge_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
                emit_routing_decision(
                    "route",
                    &route_turn_id,
                    &route_input,
                    "",
                    "",
                    "resume",
                    "mid_run_nudge",
                    &nudge_at,
                );
                if let Some(target) = attribution_target.as_deref() {
                    self.emit_activity(
                        "route",
                        "resume",
                        &open.kind,
                        "",
                        "",
                        &req.actor_name,
                        target,
                        &nudge_at,
                        &attribution_source,
                        route_conversation_hash.clone(),
                        route_project_label.clone(),
                        Some(open.artifact_id.clone()),
                        Some(CallState::Resume.as_str().to_string()),
                    );
                }
                // Shared construction (no drift with the continuation-token path).
                let mut response = resume_response_for_open_playbook_run(
                    registry.as_ref(),
                    &registry_base,
                    &registry_hearth_display,
                    route_conversation_hash.as_deref().unwrap_or(""),
                    open.clone(),
                );
                // A mid-run check-in nudge is NOT a continuation-token resume.
                // Same distinction the routing-decision marker already draws.
                // A REJECTION out-ranks the nudge label. The nudge is an affordance
                    // about an open run; `rejected` is the fact about THIS turn's
                    // continuation decision, and it is the one the accepted-loss
                    // count depends on. Empty continuation_source means the
                    // procedure did not decide anything, so the nudge names itself.
                    response.resume_source = if continuation_source.is_empty() {
                        "mid_run_nudge".to_string()
                    } else {
                        continuation_source.to_string()
                    };
                return Ok(Response::new(response));
            }

            // UNRELATED turn (matched-other OR no_match): count it toward the
            // sticky moved-on suppression for THIS open artifact.
            let unrelated_count = dedup_hearth
                .as_deref()
                .map(|h| {
                    self.nudge_dedup
                        .record_unrelated(h, &req.conversation_id, &open.artifact_id)
                })
                .unwrap_or(0);
            let quiet = unrelated_count > route_index::UNRELATED_QUIET_THRESHOLD;

            // The no_match-while-open REMINDER still fires while the artifact has
            // not gone quiet, subject to the time-window dedup. A matched-other
            // turn never resumes (the relevance gate above) — it just falls
            // through to the normal candidates/single response.
            if is_no_match && !quiet {
                let should_nudge = dedup_hearth
                    .as_deref()
                    .map(|h| {
                        self.nudge_dedup
                            .should_nudge(h, &req.conversation_id, &open.artifact_id)
                    })
                    .unwrap_or(true);
                if should_nudge {
                    tracing::info!(
                        command = "route",
                        outcome = "resume",
                        resolution_outcome = "resume",
                        resume_source = "no_match_open_reminder",
                        resume_artifact = %open.artifact_id,
                        resume_kind = %open.kind,
                        resume_state = %open.state,
                        "route surfaced a no_match-while-open reminder"
                    );
                    let nudge_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
                    emit_routing_decision(
                        "route",
                        &route_turn_id,
                        &route_input,
                        "",
                        "",
                        "resume",
                        "no_match_open_reminder",
                        &nudge_at,
                    );
                    if let Some(target) = attribution_target.as_deref() {
                        self.emit_activity(
                            "route",
                            "resume",
                            &open.kind,
                            "",
                            "",
                            &req.actor_name,
                            target,
                            &nudge_at,
                            &attribution_source,
                            route_conversation_hash.clone(),
                            route_project_label.clone(),
                            Some(open.artifact_id.clone()),
                            Some(CallState::Resume.as_str().to_string()),
                        );
                    }
                    let mut response = resume_response_for_open_playbook_run(
                        registry.as_ref(),
                        &registry_base,
                        &registry_hearth_display,
                        route_conversation_hash.as_deref().unwrap_or(""),
                        open.clone(),
                    );
                    // A mid-run check-in nudge is NOT a continuation-token resume.
                    // Same distinction the routing-decision marker already draws.
                    // A REJECTION out-ranks the nudge label. The nudge is an affordance
                    // about an open run; `rejected` is the fact about THIS turn's
                    // continuation decision, and it is the one the accepted-loss
                    // count depends on. Empty continuation_source means the
                    // procedure did not decide anything, so the nudge names itself.
                    response.resume_source = if continuation_source.is_empty() {
                        "mid_run_nudge".to_string()
                    } else {
                        continuation_source.to_string()
                    };
                    return Ok(Response::new(response));
                }
                // Time-window suppressed: emit the `*_suppressed` marker and fall
                // through to the normal (handoff) response.
                let suppressed_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
                tracing::info!(
                    command = "route",
                    outcome = "resume_suppressed",
                    resume_source = "no_match_open_reminder_suppressed",
                    resume_artifact = %open.artifact_id,
                    "route suppressed a repeat no_match-while-open reminder (dedup)"
                );
                emit_routing_decision(
                    "route",
                    &route_turn_id,
                    &route_input,
                    "",
                    "",
                    resolution_outcome,
                    "no_match_open_reminder_suppressed",
                    &suppressed_at,
                );
                // fall through
            } else if quiet {
                // MOVED ON: the agent has run past the quiet threshold of
                // consecutive unrelated turns with no progress on this artifact.
                // Suppress the resume/reminder and surface the PARK HINT ONCE so
                // the dangling track can be abandoned instead of nagged.
                let suppressed_marker = if is_no_match {
                    "no_match_open_reminder_suppressed"
                } else {
                    "mid_run_nudge_suppressed"
                };
                let suppressed_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
                tracing::info!(
                    command = "route",
                    outcome = "resume_suppressed",
                    resume_source = suppressed_marker,
                    resume_artifact = %open.artifact_id,
                    unrelated_count = unrelated_count,
                    "route quieted an open-playbook resume (agent moved on)"
                );
                emit_routing_decision(
                    "route",
                    &route_turn_id,
                    &route_input,
                    "",
                    "",
                    resolution_outcome,
                    suppressed_marker,
                    &suppressed_at,
                );
                let surface = dedup_hearth
                    .as_deref()
                    .map(|h| {
                        self.nudge_dedup.take_park_hint_once(
                            h,
                            &req.conversation_id,
                            &open.artifact_id,
                        )
                    })
                    .unwrap_or(false);
                if surface {
                    let park_action =
                        abandon_action_for(registry.as_ref(), &open.kind, &open.state);
                    tracing::info!(
                        command = "route",
                        outcome = "park_hint",
                        resume_artifact = %open.artifact_id,
                        resume_kind = %open.kind,
                        resume_state = %open.state,
                        park_action = %park_action,
                        "route surfaced a park hint (moved-on, surface-once)"
                    );
                    route_park_hint = Some(ParkHint {
                        artifact_id: open.artifact_id.clone(),
                        kind: open.kind.clone(),
                        state: open.state.clone(),
                        park_action,
                    });
                }
            }
            // Non-quiet matched-other turns fall through to the normal response
            // with no resume and no park hint (P1 relevance-gate suppression).
        }

        if let Some(target) = attribution_target.as_deref() {
            let route_call_state = classify_call_state(
                &resolution.outcome,
                route_open_playbook_run.as_ref(),
                &resolution.matching_candidates,
            );
            self.emit_activity(
                "route",
                resolution_outcome,
                route_selected,
                "",
                "",
                &req.actor_name,
                target,
                &rd_at,
                &attribution_source,
                route_conversation_hash.clone(),
                route_project_label.clone(),
                None,
                Some(route_call_state.as_str().to_string()),
            );
        }

        // route_response_mirrors_begin Phase 2/3 — enrich the response. The legacy
        // `candidates` field stays the GRANTED set (existing contract: see
        // route_rpc_resolution.feature). Per-candidate intent/step_outline/why_fits
        // are populated ONLY for the MATCHING set (M4 — a granted-but-not-matching
        // playbook carries no honest match signal, so it appears in the candidate
        // list but is NOT surfaced as a MATCHED candidate: empty annotations +
        // absence from `matching_candidates` make that observable). The
        // single-outcome `guidance` is the begin-equivalent hook body served via
        // the shared seam.
        //
        // Budget (H2): the matching annotations are passed through `apply_budget`,
        // which accounts for the bytes actually serialized per candidate (kind +
        // description + intent + step_outline + why_fits) and applies the
        // deterministic truncation fallback (drop step_outline first, then collapse
        // to kind + description) so the serialized annotation payload stays within
        // ROUTE_RESPONSE_BUDGET_BYTES. The truncated annotations are what the engine
        // copies onto the matching candidates below.
        //
        // Fail-open (req #6): the guidance enrichment (the only hook-body read on
        // the route path) is wrapped HERE so a slow/missing/oversized read
        // degrades to the thin response (empty guidance) and never turns a
        // successful route into an error. Candidate annotations never read hook
        // bodies, so they cannot fail-open. A genuine routing/engine error already
        // surfaced above (this code only runs after a successful resolution).
        let hook_reader = SourceAwarePlaybookHookBodyReader {
            request_hearth: registry_base.clone(),
        };
        let annotated = apply_budget(annotate_candidates(registry.as_ref(), &resolution));
        let annotation_for = |kind: &str| annotated.iter().find(|c| c.kind == kind);
        let candidates: Vec<CandidateMeta> = resolution
            .granted_candidates
            .iter()
            .filter_map(|kind| {
                registry.machine_for(kind).map(|m| {
                    let ann = annotation_for(kind);
                    let description = m
                        .route
                        .description
                        .as_deref()
                        .filter(|description| !description.trim().is_empty())
                        .unwrap_or(&m.description)
                        .to_string();
                    CandidateMeta {
                        kind: m.kind.clone(),
                        description,
                        required_fields: m.required_fields.iter().map(|f| f.name.clone()).collect(),
                        intent: ann.map(|a| a.intent.clone()).unwrap_or_default(),
                        step_outline: ann.map(|a| a.step_outline.clone()).unwrap_or_default(),
                        why_fits: ann.map(|a| a.why_fits.clone()).unwrap_or_default(),
                        route_triggers: m.route.triggers.clone(),
                    }
                })
            })
            .collect();

        let guidance = match &resolution.outcome {
            RouteOutcome::Single => {
                match single_guidance(&hook_reader, registry.as_ref(), &resolution) {
                    Ok(body) => body,
                    Err(e) => {
                        tracing::warn!(
                            command = "route",
                            outcome = "guidance_enrichment_failed",
                            error = %e,
                            "route guidance enrichment failed — degrading to thin response (fail-open)"
                        );
                        String::new()
                    }
                }
            }
            RouteOutcome::Candidates | RouteOutcome::NoMatch => String::new(),
        };

        let (handoff, intent) = match &resolution.outcome {
            RouteOutcome::NoMatch => (CANDIDATE_PLAYBOOK_INTAKE.to_string(), req.message),
            RouteOutcome::Single | RouteOutcome::Candidates => (String::new(), String::new()),
        };

        let response = RouteResponse {
            outcome: legacy_outcome.to_string(),
            candidates,
            handoff,
            intent,
            resolved_hearth: registry_hearth_display,
            selected_kind: resolution.selected_kind.unwrap_or_default(),
            resolution_outcome: resolution_outcome.to_string(),
            matching_candidates: resolution.matching_candidates,
            guidance,
            resume_artifact_id: String::new(),
            resume_kind: String::new(),
            resume_state: String::new(),
            resume_guidance: String::new(),
            resume_advance_action: String::new(),
            // A rejection routes normally but says WHY the contextual path
            // declined. Set INDEPENDENTLY of the outcome: the two answer
            // different questions, and a rejection that then routes successfully
            // must record both or the reason becomes a partial count.
            resume_source: continuation_source.to_string(),
            // resume-signal context-awareness — populated ONCE for the open
            // artifact when the resume was suppressed because the agent moved on.
            park_hint: route_park_hint,
            // The SAME per-turn value that already feeds the routing-activity
            // row and the activity-log route row. Never a recomputation: a
            // second hasher lands in a disjoint keyspace, which is the defect
            // this whole track exists to close.
            conversation_hash: route_conversation_hash.clone().unwrap_or_default(),
        };

        Ok(Response::new(response))
        })
    }

    async fn begin(
        &self,
        request: Request<BeginRequest>,
    ) -> Result<Response<BeginResponse>, Status> {
        // Gatekeeper FIRST (spec Req 5), BEFORE resolve_hearth and BEFORE the
        // per-hearth lock — an unauth caller never contends the lock.
        // Standalone ⇒ Ok(None), no-op. R2 binds the principal to the
        // session's `sub` (spec Req 7).
        let session = self.authorize(&request).await?;
        // The CQRS command seam. Read the surface BEFORE the hearth is resolved
        // and before the per-hearth lock is taken, for the same reason the
        // gatekeeper runs first: a caller this engine will not serve never
        // contends the lock. A request naming no surface is refused here and
        // changes nothing.
        let surface = command_seam::read_surface(&request)
            .map_err(|refusal| command_seam::refuse("begin", refusal))?;
        let req = request.into_inner();
        let resolved = resolve_hearth(
            &req.hearth_path,
            self.hearth_path.as_deref(),
            &self.hearth_policy,
        )?;
        // N2-a: acquire the per-hearth guard ONCE, before the initial read, and
        // hold it across the whole begin transaction. The per-hearth mutex is
        // non-reentrant, so the lock-bearing event arms (ReviewTransition,
        // TrackCreation, PlaybookCreation) must NOT re-lock — they reuse this
        // single guard. Re-locking would self-deadlock.
        let _begin_guard = self.hearth_locks.lock_for(&resolved).await;
        let query_adapter = FileSystemQueryAdapter::new(resolved.clone());
        // Build the playbook registry exactly as the describe RPC does: a fresh
        // per-call hearth scan (always-reload, spec R7.2) with seed fallback.
        // The begin handler resolves the create-flow (spec, doer) hook
        // declaration through this registry before serving its body.
        let session_registry = self.request_session_registry(&resolved);
        let hook_reader = SourceAwarePlaybookHookBodyReader {
            request_hearth: resolved.clone(),
        };
        // Bind the persisted principal to the verified `sub` under Foundry
        // (spec Req 7); standalone keeps the caller-supplied name verbatim
        // (Req 3). Captured also for the outcome log.
        let actor_name = Self::principal_actor_name(&session, req.actor_name.clone());
        let req_actor_name = actor_name.clone();
        // Open the turn: the `issued` record lands before anything is written,
        // so an interrupted begin reads as asked-for-never-resolved rather than
        // vanishing. The matching `settled` record is emitted on drop, which is
        // why begin's seven separate error exits need no emit of their own.
        let mut seam = CommandSeam::issue("begin", surface, &req_actor_name);
        let ctx = proto_request_context(
            &req.ctx_org,
            &req.ctx_space,
            &req.ctx_role,
            &req.ctx_clearance,
        );
        // [H-3] The begin RPC has no outer `at` — each event arm stamps its own
        // `at` for snapshot/transition writes. Stamp one here, before the event
        // loop, for the step_measurement emit only (M-P3).
        let at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let rd_turn_id = req.rd_turn_id.clone();
        let rd_input = req.rd_input.clone();
        let rd_candidate_set = req.rd_candidate_set.clone();
        let rd_selected = req.rd_selected.clone();
        let rd_confidence = req.rd_confidence.clone();
        let begin_conversation_hash = self.conversation_hash(&req.conversation_id);
        // Raw conversation_id retained for the open-marker index + dedup re-arm
        // (open_marker_index Phases 1/3) — `req.conversation_id` is moved into the
        // domain request below, so capture it here.
        let begin_conversation_id = req.conversation_id.clone();
        let begin_project_label = Self::project_label(&req.project_root);
        let requested_artifact_type = req.artifact_type.clone();
        let selected_kind_for_diagnostics = if rd_selected.is_empty() {
            requested_artifact_type.clone()
        } else {
            rd_selected.clone()
        };

        if let Some(error) = selected_playbook_load_error(
            &session_registry,
            &selected_kind_for_diagnostics,
            &resolved,
            self.global_playbooks_hearth.as_deref(),
        ) {
            return Err(Status::invalid_argument(error.to_string()));
        }
        let registry = session_registry.registry;

        // Captured for the §0 temper emit (req #4) before req fields are moved.
        let begin_actor_model = req.actor_model.clone();
        // P1 retains the typed claims across handler consumption; P2 wires
        // this copy to the post-handler evidence emitter.
        let MappedLifecycleRequest {
            domain_request,
            claimed_evidence: begin_claimed_evidence,
        } = begin_request_to_domain(req, ctx, actor_name).map_err(Status::invalid_argument)?;

        let mut outcome = BeginCommandHandler::execute_with_hook_reader(
            &query_adapter,
            &hook_reader,
            registry.as_ref(),
            domain_request,
        )
        .map_err(|e| begin_error_to_status(e))?;

        // K5 (R10) bind-time bindability precondition — dark-by-default behind
        // `ANVIL_K5_BIND`. A workflow-instance bind (the K5 fire path creates a
        // DRIVEN workflow instance to supervise a fired run) must bind to a
        // K5-bindable machine: one whose single plain-doer `Complete` resolves to
        // terminal `completed` and which declares a `-> abandoned` park edge
        // (docs/contracts/k5-bind-v1.md §5; spec R10 / §4.3a). Driven
        // workflow-instance creation is carried by `Event::ArtifactCreation` with
        // the driven kind (`handle_create` in anvil-core/src/domain/begin.rs) — the
        // `PlaybookCreation` variant is only the playbook-authoring create — so BOTH
        // creation events are inspected and the check is scoped to `is_driven()`
        // machines. Checked AFTER the pure handler resolved the machine and BEFORE
        // any event is routed to disk (the loop below), so a rejection is
        // create-or-nothing: nothing has been scaffolded yet. Non-driven creates (a
        // track, a resume, a review) either carry a non-driven machine (skipped) or
        // no creation event, and are untouched; when the flag is unset this whole
        // block is inert (A7). Symmetric with the R9 terminal-resolve guard, which
        // likewise applies engine-wide when the flag is enabled (engine-hardening
        // framing).
        if k5_bind_enabled() {
            let created_kind = outcome.events.iter().find_map(|event| match event {
                Event::ArtifactCreation { status, .. } | Event::PlaybookCreation { status, .. } => {
                    Some(status.kind.clone())
                }
                _ => None,
            });
            if let Some(created_kind) = created_kind {
                match registry.machine_for(&created_kind) {
                    Some(machine) if machine.is_driven() => {
                        // R2 idempotent bind (A2), dark-by-default. A fire-path
                        // begin correlated to a run that ALREADY has an open
                        // (begun, non-terminal) instance must resolve and RETURN
                        // that existing instance rather than mint a second — the
                        // exactly-one-instance-per-run guarantee. The correlation
                        // is the run token carried on `conversation_id` (§4.1); an
                        // empty correlation opts OUT (no idempotency requested →
                        // bind fresh), which is why the no-correlation K5 scenarios
                        // are unaffected. Resolved by
                        // `k5_open_bound_instance_for_conversation` — the bind
                        // seam's purpose-built open-instance resolver (a driven
                        // instance carrying a `begin` marker for this correlation
                        // whose CURRENT state is non-terminal). It deliberately does
                        // NOT reuse `find_open_workflow_for_conversation`'s
                        // begin-adoption `has_open_begin` gate: that gate treats a
                        // same-actor transition at-or-after the marker as "closed",
                        // which a driven instance's OWN creation transition (same
                        // actor, same second) trips — masking the freshly-bound
                        // instance from a same-second retry. Checked BEFORE the R10
                        // bindability gate and BEFORE any event is routed to disk, so
                        // the second begin is create-or-nothing: it scaffolds
                        // nothing. Fail-OPEN by construction (the resolver swallows a
                        // read error to `None` → fresh bind), mirroring the route
                        // path's open-lookup degradation: a transient read must never
                        // strand the fire path, and a missed dedup is a duplicate
                        // instance — never a masked bind FAILURE, so R7's fail-loud
                        // law is preserved (a real bind error still returns non-OK).
                        if !begin_conversation_id.trim().is_empty() {
                            let query = FileSystemQueryAdapter::new(resolved.clone());
                            if let Some(open) = k5_open_bound_instance_for_conversation(
                                &query,
                                registry.as_ref(),
                                &begin_conversation_id,
                            ) {
                                let directory = registry
                                    .machine_for(&open.kind)
                                    .map(|m| m.directory.clone())
                                    .unwrap_or_default();
                                let existing_track_path = if directory.is_empty() {
                                    open.artifact_id.clone()
                                } else {
                                    format!("{}/{}", directory, open.artifact_id)
                                };
                                let existing_playbook_id =
                                    registry.playbook_id_for(&open.kind).unwrap_or_default();
                                let existing_next_step =
                                    begin_next_step(registry.as_ref(), &open.kind, &open.state);
                                return Ok(Response::new(BeginResponse {
                                    track_path: existing_track_path,
                                    state: open.state,
                                    context_text: String::new(),
                                    artifact_text: String::new(),
                                    review_context_text: String::new(),
                                    review_doc_path: String::new(),
                                    next_step: existing_next_step,
                                    resolved_hearth: resolved.display().to_string(),
                                    intent: String::new(),
                                    expected_output: String::new(),
                                    playbook_id: existing_playbook_id,
                                }));
                            }
                        }
                        if let Err(err) =
                            anvil_core::domain::playbook::bindability::machine_is_k5_bindable(
                                machine,
                            )
                        {
                            return Err(Status::failed_precondition(format!(
                                "machine_not_bindable: {}",
                                err
                            )));
                        }
                    }
                    // A non-driven creation (e.g. a track) is not a run-supervised
                    // workflow bind — the K5 bindability precondition does not apply.
                    Some(_) => {}
                    // Fail CLOSED: the pure handler just emitted a creation event for
                    // `created_kind`, so its machine MUST resolve in this same
                    // registry; if it does not, we cannot verify bindability, so we
                    // reject create-or-nothing rather than scaffold an unverified
                    // (possibly unbindable) bind. Dark-by-default and unreachable in
                    // practice, but the R10 gate must never fail open (fail-closed
                    // law; spec R10 create-or-nothing).
                    None => {
                        return Err(Status::failed_precondition(format!(
                            "machine_not_bindable: could not resolve machine '{}' to verify K5 bindability",
                            created_kind
                        )));
                    }
                }
            }
        }

        // Resolve the actual lifecycle step from the PURE outcome before any
        // event is routed. Creation begins use the same machine-derived doer
        // axis as the P2 evidence emitter; resume/review begins use the domain
        // result. Adoption/projection/marker-only outcomes are not forward
        // lifecycle transitions and remain outside the completeness gate.
        let creation_kind_role = outcome.events.iter().find_map(|event| match event {
            Event::ArtifactCreation { status, .. } | Event::PlaybookCreation { status, .. } => {
                Some((status.kind.clone(), "doer".to_string()))
            }
            _ => None,
        });
        let (response_kind, response_role) = begin_measurement_kind_role(
            &outcome.result,
            creation_kind_role
                .as_ref()
                .map(|(kind, role)| (kind.as_str(), role.as_str())),
        );
        let begin_has_lifecycle_transition = outcome.events.iter().any(|event| {
            matches!(
                event,
                Event::ReviewTransition { .. }
                    | Event::ArtifactCreation { .. }
                    | Event::PlaybookCreation { .. }
            )
        });
        if begin_has_lifecycle_transition {
            enforce_claimed_evidence_preflight(
                claimed_evidence_gate_active(
                    &resolved,
                    self.global_playbooks_hearth.as_deref(),
                ),
                registry.as_ref(),
                &response_kind,
                &outcome.result.state,
                &response_role,
                &begin_claimed_evidence,
            )
            .map_err(claimed_evidence_gate_error_to_status)?;
        }

        // Route events. The per-hearth guard (`_begin_guard`) acquired at the
        // top of this RPC covers ALL event-arm writes — no arm re-locks (N2-a;
        // the per-hearth mutex is non-reentrant). `ReviewDocCreated` never
        // touched the lock and remains untouched.
        let artifact_adapter = FileSystemArtifactAdapter::new(resolved.clone());

        // [spark fold identity] A projection-only spark capture appends to the
        // SHARED `sparks/sparks.md` path, so the begin activity record would
        // otherwise carry the same `playbook_run_id` (`sparks.md`) for
        // EVERY spark — collapsing all captures into one fold identity, so the
        // outcome-predicate fold could never count captures per-spark. Give each
        // capture its own unique fold identity (the same content-addressed id
        // written into the sparks.md entry) so `captured` reach is a per-capture
        // checkable fact.
        let mut spark_fold_id: Option<String> = None;

        for event in &outcome.events {
            match event {
                Event::ReviewTransition {
                    track_path,
                    to_state,
                    actor,
                    role,
                    approver,
                    note,
                } => {
                    // No re-lock here (N2-a): the begin transaction-wide guard
                    // already covers this write.
                    let snapshot_adapter = FileSystemSnapshotAdapter::new(resolved.clone());
                    let actor_write_adapter = FileSystemActorWriteAdapter::new(resolved.clone());
                    let at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
                    let snapshot_request = DomainSnapshotRequest {
                        artifact_path: track_path.clone(),
                        to_state: to_state.clone(),
                        actor_name: actor.name.clone(),
                        actor_role: role.clone(),
                        approver: approver.clone().unwrap_or_default(),
                        note: note.clone().unwrap_or_default(),
                        actor_type: actor.actor_type.clone(),
                        actor_model: actor.model.clone(),
                        actor_provider: actor.provider.clone(),
                        actor_context_window: actor.context_window,
                        actor_sdk_version: actor.sdk_version.clone(),
                        actor_entrypoint: actor.entrypoint.clone(),
                        projection_only: false,
                        event_type: String::new(),
                        allow_reserved_event_type: false,
                        at,
                        claimed_evidence: Vec::new(),
                    };
                    SnapshotCommandHandler::execute(
                        &snapshot_adapter,
                        &actor_write_adapter,
                        snapshot_request,
                    )
                    .map_err(|e| begin_error_to_status(map_snapshot_error_to_begin(e)))?;
                }
                Event::ArtifactAdopted {
                    track_path,
                    to_state,
                    actor,
                    role,
                    note,
                } => {
                    // A governance-adoption reset lands the artifact at its
                    // machine's initial state through the SAME snapshot handler as
                    // an ordinary transition, but tags the persisted transition
                    // with `event_type: "adoption"` so `has_open_begin` skips it
                    // (the reset must NOT close the begin marker emitted alongside
                    // it) and so the begin measurement stream can distinguish the
                    // reset from real doer progress. No re-lock (N2-a).
                    let snapshot_adapter = FileSystemSnapshotAdapter::new(resolved.clone());
                    let actor_write_adapter = FileSystemActorWriteAdapter::new(resolved.clone());
                    let at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
                    let snapshot_request = DomainSnapshotRequest {
                        artifact_path: track_path.clone(),
                        to_state: to_state.clone(),
                        actor_name: actor.name.clone(),
                        actor_role: role.clone(),
                        approver: String::new(),
                        note: note.clone().unwrap_or_default(),
                        actor_type: actor.actor_type.clone(),
                        actor_model: actor.model.clone(),
                        actor_provider: actor.provider.clone(),
                        actor_context_window: actor.context_window,
                        actor_sdk_version: actor.sdk_version.clone(),
                        actor_entrypoint: actor.entrypoint.clone(),
                        projection_only: false,
                        event_type: "adoption".to_string(),
                        // INTERNAL adoption route: the ONLY caller permitted to
                        // stamp the reserved "adoption" event_type. A public
                        // snapshot leaves this false and is rejected.
                        allow_reserved_event_type: true,
                        at,
                        claimed_evidence: Vec::new(),
                    };
                    SnapshotCommandHandler::execute(
                        &snapshot_adapter,
                        &actor_write_adapter,
                        snapshot_request,
                    )
                    .map_err(|e| begin_error_to_status(map_snapshot_error_to_begin(e)))?;
                }
                Event::ReviewDocCreated {
                    track_path,
                    doc_name,
                    header,
                } => {
                    let review_doc_path = artifact_adapter
                        .create_review_doc(track_path, doc_name, header)
                        .map_err(|e| begin_error_to_status(BeginError::from(e)))?;
                    outcome.result.review_doc_path = review_doc_path;
                }
                Event::BeginMarkerWritten {
                    artifact_path,
                    kind,
                    actor,
                    state,
                    at: _,
                    conversation_id,
                } => {
                    // Append-only begin-marker. The engine stamps `at` at
                    // routing time (mirroring the ReviewTransition arm). No
                    // re-lock here (N2-a): the begin transaction-wide guard
                    // already covers this write. NOT a state transition.
                    use anvil_core::domain::shared_types::ActivityEntry;
                    use anvil_core::ports::activity_write_port::ActivityWritePort;
                    let activity_write_adapter =
                        FileSystemActivityWriteAdapter::new(resolved.clone());
                    let at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
                    let entry = ActivityEntry {
                        kind: kind.clone(),
                        actor: actor.clone(),
                        state: state.clone(),
                        at,
                        conversation_id: conversation_id.clone(),
                    };
                    activity_write_adapter
                        .append_activity(artifact_path, &entry)
                        .map_err(|e| {
                            begin_error_to_status(BeginError::IoError {
                                message: e.to_string(),
                            })
                        })?;
                }
                Event::ArtifactCreation {
                    track_name,
                    parent_id,
                    display_name: _,
                    actor,
                    approver,
                    status,
                    directory,
                    registry_file: _,
                    scaffold_files,
                    creation_role,
                    conversation_id,
                } => {
                    // Generalized create routing: scaffold the machine-derived
                    // directory + initial status.yaml + an optional placeholder
                    // doc, then dispatch a standard SnapshotRequest with the
                    // machine's initial state so the existing
                    // SnapshotCommandHandler creates the registry entry, appends
                    // the first transition, and (where applicable) inserts the
                    // execution projection row. ZERO `"track"` literals — kind,
                    // state, directory and roles all come from the event.
                    let initial_status_yaml = build_initial_artifact_status_yaml(
                        &status.kind,
                        &status.state,
                        parent_id,
                        &status.target_owner,
                        &status.fields,
                        &status.origin_turn,
                    );
                    let scaffold_refs = scaffold_files
                        .iter()
                        .map(|(filename, contents)| (filename.as_str(), contents.as_str()))
                        .collect::<Vec<_>>();
                    let artifact_path = artifact_adapter
                        .scaffold_artifact_directory(
                            directory,
                            track_name,
                            &initial_status_yaml,
                            &scaffold_refs,
                        )
                        .map_err(|e| begin_error_to_status(BeginError::from(e)))?;

                    let snapshot_adapter = FileSystemSnapshotAdapter::new(resolved.clone());
                    let actor_write_adapter = FileSystemActorWriteAdapter::new(resolved.clone());
                    let snapshot_request = DomainSnapshotRequest {
                        artifact_path: artifact_path.clone(),
                        to_state: status.state.clone(),
                        actor_name: actor.name.clone(),
                        actor_role: creation_role.clone(),
                        approver: approver.clone(),
                        note: String::new(),
                        actor_type: actor.actor_type.clone(),
                        actor_model: actor.model.clone(),
                        actor_provider: actor.provider.clone(),
                        actor_context_window: actor.context_window,
                        actor_sdk_version: actor.sdk_version.clone(),
                        actor_entrypoint: actor.entrypoint.clone(),
                        projection_only: false,
                        event_type: String::new(),
                        allow_reserved_event_type: false,
                        at: status.transition_at.clone(),
                        claimed_evidence: Vec::new(),
                    };
                    SnapshotCommandHandler::execute(
                        &snapshot_adapter,
                        &actor_write_adapter,
                        snapshot_request,
                    )
                    .map_err(|e| begin_error_to_status(map_snapshot_error_to_begin(e)))?;

                    // Resume-aware routing: record a durable open-begin marker on
                    // the freshly-scaffolded artifact so a later continuation
                    // message can bridge this conversation to its open playbook.
                    // Append-only: when conversation_id is empty the marker still
                    // records an empty conversation_id (back-compat with markers
                    // that predate this field). The marker's `actor`/`state` mirror
                    // the begin-marker the resume/review flows append.
                    {
                        use anvil_core::domain::shared_types::ActivityEntry;
                        use anvil_core::ports::activity_write_port::ActivityWritePort;
                        let activity_write_adapter =
                            FileSystemActivityWriteAdapter::new(resolved.clone());
                        let marker_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
                        let entry = ActivityEntry {
                            kind: "begin".to_string(),
                            actor: actor.name.clone(),
                            state: status.state.clone(),
                            at: marker_at,
                            conversation_id: conversation_id.clone(),
                        };
                        // M3: the resume marker is load-bearing — a begin that
                        // does not write it produces a NON-RESUMABLE artifact (a
                        // later "go" can never bridge back to it). So a marker
                        // write failure is NOT swallowed: it is surfaced
                        // distinctly AND fails the begin (unlike measurement-style
                        // emits, which stay advisory). A successful begin therefore
                        // reliably carries its resumable marker.
                        if let Err(e) =
                            activity_write_adapter.append_activity(&artifact_path, &entry)
                        {
                            tracing::error!(
                                command = "begin",
                                outcome = "create_begin_marker_append_failed",
                                artifact_path = %artifact_path,
                                error = %e,
                                "create begin-marker append failed — begin cannot guarantee a resumable artifact"
                            );
                            return Err(begin_error_to_status(BeginError::IoError {
                                message: format!(
                                    "failed to write the resume begin-marker for '{}': {}",
                                    artifact_path, e
                                ),
                            }));
                        }
                    }

                    outcome.result.track_path = artifact_path;
                }
                Event::BacklogItemCreation {
                    item,
                    created,
                    actor,
                    status_bytes,
                    ..
                } => {
                    // K8 exact-ID publication (plan Task 5). No generic
                    // scaffold, no adoption, and no candidate -> candidate
                    // Snapshot: the strict store publishes
                    // `backlog_items/<bi_id>` from one journaled staging
                    // directory under the begin transaction's hearth lock.
                    use anvil_core::domain::backlog_manifest::{
                        build_genesis_commit, BacklogRegistryRow,
                    };
                    use anvil_core::domain::content_hash::content_hash;
                    use anvil_core_hearth::fs_backlog_item_adapter::FileSystemBacklogItemAdapter;
                    use anvil_core::ports::backlog_item_port::BacklogItemPort;
                    let store = FileSystemBacklogItemAdapter::new(resolved.clone());
                    let existing = store.load_all().map_err(|e| {
                        begin_error_to_status(BeginError::IoError {
                            message: format!("backlog store: {e}"),
                        })
                    })?;
                    let mut rows: Vec<BacklogRegistryRow> = existing
                        .iter()
                        .map(|l| BacklogRegistryRow {
                            id: l.id.clone(),
                            title: l.item.title.clone(),
                            section: l.item.state.as_str().to_string(),
                        })
                        .collect();
                    rows.push(BacklogRegistryRow {
                        id: item.backlog_item_id.clone(),
                        title: item.title.clone(),
                        section: item.state.as_str().to_string(),
                    });
                    let registry_old_hash =
                        match std::fs::read(resolved.join("backlog_items.md")) {
                            Ok(bytes) => Some(content_hash(&bytes)),
                            Err(_) => None,
                        };
                    let commit = build_genesis_commit(
                        &format!("genesis-{}", item.backlog_item_id),
                        item,
                        created,
                        actor,
                        status_bytes,
                        &rows,
                        registry_old_hash,
                        &anvil_core_hearth::fs_transition_event_adapter::hi_res_prefix(),
                        &anvil_core_hearth::fs_transition_event_adapter::short_random_id(),
                    )
                    .map_err(|e| {
                        begin_error_to_status(BeginError::IoError {
                            message: format!("backlog genesis preparation: {e}"),
                        })
                    })?;
                    let path = store.create_genesis(commit).map_err(|e| {
                        begin_error_to_status(BeginError::IoError {
                            message: format!("backlog genesis: {e}"),
                        })
                    })?;
                    outcome.result.track_path = path;
                }
                Event::ProjectionOnlySnapshot {
                    artifact_path,
                    event_type,
                    body,
                    actor,
                } => {
                    let at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
                    // Unique per-capture fold identity (same recipe as the id
                    // written into the sparks.md entry): distinct sparks never
                    // collapse into one instance in the outcome-predicate fold.
                    spark_fold_id = Some(spark_id(body.trim(), actor, &at));
                    append_spark_source_event(&resolved, body, actor, &at)
                        .map_err(begin_error_to_status)?;
                    let snapshot_adapter = FileSystemSnapshotAdapter::new(resolved.clone());
                    let actor_write_adapter = FileSystemActorWriteAdapter::new(resolved.clone());
                    let snapshot_request = DomainSnapshotRequest {
                        artifact_path: artifact_path.clone(),
                        to_state: String::new(),
                        actor_name: actor.name.clone(),
                        actor_role: String::new(),
                        approver: String::new(),
                        note: String::new(),
                        actor_type: actor.actor_type.clone(),
                        actor_model: actor.model.clone(),
                        actor_provider: actor.provider.clone(),
                        actor_context_window: actor.context_window,
                        actor_sdk_version: actor.sdk_version.clone(),
                        actor_entrypoint: actor.entrypoint.clone(),
                        projection_only: true,
                        event_type: event_type.clone(),
                        allow_reserved_event_type: false,
                        at,
                        claimed_evidence: Vec::new(),
                    };
                    SnapshotCommandHandler::execute(
                        &snapshot_adapter,
                        &actor_write_adapter,
                        snapshot_request,
                    )
                    .map_err(|e| begin_error_to_status(map_snapshot_error_to_begin(e)))?;
                    outcome.result.track_path = artifact_path.clone();
                }
                Event::PlaybookCreation {
                    playbook_name,
                    parent_id,
                    actor,
                    approver,
                    status,
                } => {
                    // Scaffold playbook directory + initial status.yaml +
                    // placeholder definition.md, then dispatch a standard
                    // SnapshotRequest with to_state: "draft".
                    let initial_status_yaml =
                        build_initial_playbook_status_yaml(parent_id, &status.origin_turn);
                    let playbook_path = artifact_adapter
                        .scaffold_playbook_directory(playbook_name, parent_id, &initial_status_yaml)
                        .map_err(|e| begin_error_to_status(BeginError::from(e)))?;

                    let snapshot_adapter = FileSystemSnapshotAdapter::new(resolved.clone());
                    let actor_write_adapter = FileSystemActorWriteAdapter::new(resolved.clone());
                    let snapshot_request = DomainSnapshotRequest {
                        artifact_path: playbook_path.clone(),
                        to_state: "draft".to_string(),
                        actor_name: actor.name.clone(),
                        actor_role: "doer".to_string(),
                        approver: approver.clone(),
                        note: String::new(),
                        actor_type: actor.actor_type.clone(),
                        actor_model: actor.model.clone(),
                        actor_provider: actor.provider.clone(),
                        actor_context_window: actor.context_window,
                        actor_sdk_version: actor.sdk_version.clone(),
                        actor_entrypoint: actor.entrypoint.clone(),
                        projection_only: false,
                        event_type: String::new(),
                        allow_reserved_event_type: false,
                        at: status.transition_at.clone(),
                        claimed_evidence: Vec::new(),
                    };
                    SnapshotCommandHandler::execute(
                        &snapshot_adapter,
                        &actor_write_adapter,
                        snapshot_request,
                    )
                    .map_err(|e| begin_error_to_status(map_snapshot_error_to_begin(e)))?;
                    outcome.result.track_path = playbook_path;
                }
            }
        }

        // Crucible publication-log mirror (best-effort; gated; never fatal).
        if let Some(pub_log) = self.publication_log.as_ref() {
            for event in &outcome.events {
                pub_log.mirror_begin(event);
            }
        }

        let response_measurement = resolve_begin_response_measurement(
            registry.as_ref(),
            &response_kind,
            &outcome.result.state,
            &response_role,
        );
        outcome.result.intent = response_measurement.intent;
        outcome.result.expected_output = response_measurement.expected_output;
        outcome.result.playbook_id = response_measurement.playbook_id;
        // M-P3: emit the per-step measurement AFTER routing succeeded (additive).
        // begin = entering `to_state` (no prior state ⇒ from_state=""), uniform
        // across create/review/draft. The kind/role tuple is resolved by the
        // begin domain result or the machine-derived create event, then
        // intent/expected_output come from (to_state, role).
        if !outcome.events.is_empty() {
            if begin_has_lifecycle_transition {
                self.emit_transition_measurement(
                    &resolved,
                    registry.as_ref(),
                    last_segment(&outcome.result.track_path),
                    &response_kind,
                    "",
                    &outcome.result.state,
                    &response_role,
                    "",
                    &at,
                    begin_conversation_hash.clone(),
                    begin_project_label.clone(),
                    true,
                    &begin_claimed_evidence,
                    Some(&resolved.join(&outcome.result.track_path)),
                );
                // A begin records `pending` and never warns, so there is nothing
                // to surface here — asserted by a scenario rather than assumed.
                self.emit_playbook_measurement(
                    &resolved,
                    registry.as_ref(),
                    last_segment(&outcome.result.track_path),
                    &response_kind,
                    &outcome.result.state,
                    &at,
                    begin_conversation_hash.clone(),
                    begin_project_label.clone(),
                    Some(&resolved.join(&outcome.result.track_path)),
                );
            }
            emit_begin_step_measurement(
                registry.as_ref(),
                &response_kind,
                &response_role,
                &outcome.result.state,
                &outcome.result.track_path,
                &req_actor_name,
                &at,
            );

            // Full §0 temper stream for the begin transition (track_id = KIND,
            // playbook_id = the per-RUN instance id — the run/artifact dir id,
            // NOT the playbook definition id; H1). begin enters to_state with no
            // prior state (from_state empty). Gated on the privacy decision;
            // fail-open.
            {
                use anvil_core::domain::playbook::interpreter::state_role_measurement;
                let playbook_id = last_segment(&outcome.result.track_path);
                let (intent, expected_output, evidence) = registry
                    .machine_for(&response_kind)
                    .map(|machine| {
                        let (intent, expected_output) = state_role_measurement(
                            machine,
                            &outcome.result.state,
                            &response_role,
                        )
                        .map(|measurement| {
                            (
                                measurement.intent.clone(),
                                measurement.expected_output.clone(),
                            )
                        })
                        .unwrap_or_default();
                        let evidence = resolve_step_evidence(
                            machine,
                            &outcome.result.state,
                            &response_role,
                            &begin_claimed_evidence,
                        );
                        (intent, expected_output, evidence)
                    })
                    .unwrap_or_default();
                self.emit_lean_step_measurement(
                    &resolved,
                    playbook_id,
                    &response_kind,
                    "",
                    &outcome.result.state,
                    &response_role,
                    &req_actor_name,
                    &intent,
                    &expected_output,
                    &at,
                    begin_conversation_hash.clone(),
                    begin_project_label.clone(),
                    evidence,
                );
                self.emit_step0_stream(
                    &resolved,
                    playbook_id,
                    &response_kind,
                    "",
                    &outcome.result.state,
                    &response_role,
                    &req_actor_name,
                    &intent,
                    &expected_output,
                    &at,
                    &begin_actor_model,
                    None,
                    None,
                );
            }

            if !rd_selected.is_empty() {
                emit_routing_decision(
                    "begin",
                    &rd_turn_id,
                    &rd_input,
                    &rd_candidate_set,
                    &rd_selected,
                    "",
                    &rd_confidence,
                    &at,
                );
            }
        }

        // AC1: per-command outcome record. Log the emitted event VARIANT NAMES
        // only — never the note/header payloads carried inside them (Req 6).
        let begin_events: Vec<&str> = outcome.events.iter().map(begin_event_name).collect();
        seam.ok();
        tracing::info!(
            command = "begin",
            actor = %req_actor_name,
            hearth = %resolved.display(),
            events = %begin_events.join(","),
            // The record this engine already wrote now also names the program
            // that asked. Extending it beats a second record beside it: a reader
            // reconstructing a change should not have to join two log shapes.
            surface = seam.surface(),
            outcome = "ok",
            "begin command ok"
        );

        // Universal activity log (additive). begin carries an actor and a
        // resolved playbook kind (the create/review subject).
        self.emit_activity(
            "begin",
            "ok",
            &response_kind,
            // begin enters a state with no prior state (from_state empty); the
            // entered step is the begin result's state.
            "",
            &outcome.result.state,
            &req_actor_name,
            &resolved,
            &at,
            "",
            begin_conversation_hash.clone(),
            begin_project_label.clone(),
            // Spark captures fold under their own unique id (see spark_fold_id
            // above); every other begin keeps the artifact-dir instance id.
            Some(
                spark_fold_id
                    .clone()
                    .unwrap_or_else(|| last_segment(&outcome.result.track_path).to_string()),
            ),
            None,
        );

        // Open-marker index + dedup (open_marker_index Phases 1/3). A successful
        // begin opens a playbook for this conversation: record it as a candidate
        // (so the next route turn resolves it WITHOUT a scan) and re-arm the
        // dedup (a begin is a state change → the next check-in nudge must fire).
        // Best-effort: the index is an optimization, so a no-op here is harmless
        // (a later route miss rebuilds it via scan). Empty conversation ids are
        // ignored by both calls (they can never be mid-playbook).
        {
            let begun_artifact_id = last_segment(&outcome.result.track_path).to_string();
            self.open_marker_index.record_begin(
                &resolved,
                &begin_conversation_id,
                &begun_artifact_id,
            );
            self.nudge_dedup
                .rearm(&resolved, &begin_conversation_id, &begun_artifact_id);
        }

        let next_step = begin_next_step(registry.as_ref(), &response_kind, &outcome.result.state);
        Ok(Response::new(BeginResponse {
            track_path: outcome.result.track_path,
            state: outcome.result.state,
            context_text: outcome.result.context_text,
            artifact_text: outcome.result.artifact_text,
            review_context_text: outcome.result.review_context_text,
            review_doc_path: outcome.result.review_doc_path,
            next_step,
            resolved_hearth: resolved.display().to_string(),
            intent: outcome.result.intent,
            expected_output: outcome.result.expected_output,
            playbook_id: outcome.result.playbook_id,
        }))
    }

    async fn snapshot(
        &self,
        request: Request<SnapshotRequest>,
    ) -> Result<Response<SnapshotResponse>, Status> {
        // Gatekeeper FIRST (spec Req 5), BEFORE resolve_hearth and BEFORE the
        // per-hearth lock. Standalone ⇒ Ok(None), no-op. R2 binds the
        // persisted transition actor to the verified `sub` (spec Req 7).
        let session = self.authorize(&request).await?;
        // The CQRS command seam, read before the hearth is resolved and before
        // the lock is taken. A snapshot naming no surface changes nothing.
        let surface = command_seam::read_surface(&request)
            .map_err(|refusal| command_seam::refuse("snapshot", refusal))?;
        // Serialize concurrent snapshot writers. The guard's RAII drop
        // releases the lock at the end of this function, covering the
        // full status/registry/projection write sequence.
        let req = request.into_inner();
        let resolved = resolve_hearth(
            &req.hearth_path,
            self.hearth_path.as_deref(),
            &self.hearth_policy,
        )?;
        let _guard = self.hearth_locks.lock_for(&resolved).await;
        let adapter = FileSystemSnapshotAdapter::new(resolved.clone());
        let actor_write_adapter = FileSystemActorWriteAdapter::new(resolved.clone());
        let at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        // Bind the persisted transition actor to the verified `sub` under
        // Foundry (spec Req 7); standalone keeps the caller-supplied name
        // verbatim (Req 3). Captured also for the outcome log.
        let actor_name = Self::principal_actor_name(&session, req.actor_name.clone());
        let req_actor_name = actor_name.clone();
        let mut seam = CommandSeam::issue("snapshot", surface, &req_actor_name);

        // ── K8 governed lifecycle route (plan Task 6) ───────────────────────
        //
        // A backlog_item never reaches the generic snapshot handler: its only
        // public lifecycle route is prepare + consume-once compound commit,
        // under this hearth lock. The generic `append_transition` refuses a
        // backlog_item outright, so an unrouted request would be a loud
        // failure rather than a bypass — but it would also be an UNSERVED
        // surface. Branch here so the public row set is actually drivable.
        {
            use anvil_core::ports::snapshot_port::SnapshotPort;
            if adapter
                .read_artifact_kind(&req.artifact_path)
                .ok()
                .as_deref()
                == Some("backlog_item")
            {
                let response = self
                    .backlog_snapshot(&resolved, &adapter, &req, actor_name.clone(), &at)
                    .await?;
                return Ok(Response::new(response));
            }
        }

        // Capture the destination state before the request is consumed — the
        // activity-log record stamps it as `to_state`. snapshot has no prior
        // state in scope (the SnapshotResult exposes none), so `from_state`
        // stays empty.
        let snapshot_to_state = req.to_state.clone();
        // Resolve the artifact's playbook kind at log time so the activity-log
        // turn is attributable per-kind (spec req #1). Observational only — the
        // transition/registry/projection writes are untouched (req #2), and no
        // extra turn is emitted (req #3). A failed resolution falls back to
        // empty (the prior behavior) rather than failing the snapshot.
        let snapshot_artifact_path = req.artifact_path.clone();
        let snapshot_conversation_id =
            if req.conversation_id.trim().is_empty() && !req.projection_only {
                let query = FileSystemQueryAdapter::new(resolved.clone());
                match open_begin_conversation_id_for_artifact(&query, &snapshot_artifact_path) {
                    Ok(Some(conversation_id)) => conversation_id,
                    Ok(None) => String::new(),
                    Err(e) => {
                        tracing::warn!(
                            command = "snapshot",
                            outcome = "open_begin_conversation_lookup_failed",
                            error = %e,
                            "snapshot could not inherit conversation id from open begin marker"
                        );
                        String::new()
                    }
                }
            } else {
                req.conversation_id.clone()
            };
        let snapshot_conversation_hash = self.conversation_hash(&snapshot_conversation_id);
        let snapshot_project_label = Self::project_label(&req.project_root);
        let (snapshot_kind, snapshot_from_state) = {
            use anvil_core::ports::snapshot_port::SnapshotPort;
            (
                adapter
                    .read_artifact_kind(&snapshot_artifact_path)
                    .unwrap_or_default(),
                // The pre-transition state becomes the §0 from_state. Read it
                // BEFORE the transition executes; empty when unresolvable.
                adapter
                    .read_artifact_state(&snapshot_artifact_path)
                    .unwrap_or_default(),
            )
        };
        // Captured for the §0 temper emit (req #4 — snapshot emits too). The
        // role/model come off the request; intent/expected_output resolve from
        // the (to_state, role) measurement schema via a fresh registry scan.
        // projection_only snapshots (spark/annotation events) are NOT lifecycle
        // step transitions — they carry no real from/to state — so they do not
        // emit a §0 step event.
        let snapshot_role = req.actor_role.clone();
        let snapshot_actor_model = req.actor_model.clone();
        let snapshot_projection_only = req.projection_only;
        let registry = self.request_session_registry(&resolved).registry;
        // P1 retains the typed claims across handler consumption; P2 wires
        // this copy to the post-handler evidence emitter for lifecycle events.
        let MappedLifecycleRequest {
            domain_request,
            claimed_evidence: snapshot_claimed_evidence,
        } = snapshot_request_to_domain(req, actor_name, at).map_err(Status::invalid_argument)?;

        // Preserve established syntactic/identity/reserved-event error
        // precedence. This validation is pure; execute repeats it defensively
        // before performing the handler's reads and writes.
        SnapshotCommandHandler::validate_before_write(&adapter, &domain_request)
            .inspect_err(|error| {
                let code = snapshot_error_to_status(error.clone()).code();
                tracing::warn!(
                    command = "snapshot",
                    hearth = %resolved.display(),
                    outcome = "error",
                    error_code = ?code,
                    "snapshot command failed"
                );
            })
            .map_err(snapshot_error_to_status)?;

        // K5 (R9) terminal-resolve no-op guard — dark-by-default behind
        // ANVIL_K5_BIND. The run-cancel resolve calls Snapshot(to_state:
        // "abandoned") on the bound instance; a Kiln retry would otherwise
        // DOUBLE-WRITE the transitions ledger (Snapshot writes a free-form
        // to_state with no current-state guard). When the instance is already in
        // a terminal state, short-circuit to an idempotent no-op success: record
        // NO second transition and return OK. Real resolve errors (not-found /
        // malformed / missing actor params) already failed validate_before_write
        // above, so reaching here means a well-formed call on an existing
        // instance — this is an explicit contracted success on an already-resolved
        // instance, NOT a silent fallback (a real error still returns non-OK, spec
        // R9 / §4.4; contract docs/contracts/k5-bind-v1.md §6). Inert when unset.
        if k5_bind_enabled()
            && !snapshot_projection_only
            && anvil_core::domain::is_terminal_state(&snapshot_from_state)
        {
            return Ok(Response::new(SnapshotResponse {
                success: true,
                timestamp: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                status_updated: false,
                registry_updated: false,
                projections_updated: Vec::new(),
                warnings: Vec::new(),
                actor_name: req_actor_name.clone(),
                resolved_hearth: resolved.display().to_string(),
            }));
        }

        // Projection-only snapshots do not represent lifecycle progress. Every
        // real snapshot is preflighted against its destination `(state, role)`
        // before SnapshotCommandHandler performs the first status/registry write.
        if !snapshot_projection_only {
            enforce_claimed_evidence_preflight(
                claimed_evidence_gate_active(
                    &resolved,
                    self.global_playbooks_hearth.as_deref(),
                ),
                registry.as_ref(),
                &snapshot_kind,
                &snapshot_to_state,
                &snapshot_role,
                &snapshot_claimed_evidence,
            )
            .map_err(claimed_evidence_gate_error_to_status)?;
            // Snapshot writes a free-form `to_state` with no current-state
            // guard, so it can land `completed` directly. Gating `complete`
            // alone would leave a door beside the gate.
            enforce_completion_merge_check(
                &snapshot_to_state,
                &resolved,
                &snapshot_claimed_evidence,
            )?;
        }

        tracing::debug!(
            command = "snapshot",
            detail = "snapshot_dispatch",
            "snapshot command dispatched"
        );

        // AC2/C1: snapshot is the single error funnel (`begin` has seven error
        // exits). Log the failure here, before converting to Status. Carry the
        // gRPC status-code name as `error_code` — never the message body, which
        // could echo payload (Req 6).
        let result =
            SnapshotCommandHandler::execute(&adapter, &actor_write_adapter, domain_request)
                .inspect_err(|e| {
                    let code = snapshot_error_to_status(e.clone()).code();
                    tracing::warn!(
                        command = "snapshot",
                        hearth = %resolved.display(),
                        outcome = "error",
                        error_code = ?code,
                        "snapshot command failed"
                    );
                })
                .map_err(snapshot_error_to_status)?;

        // AC1: per-command outcome record. Log identifiers + write-result
        // flags only — NEVER req.note / req.artifact_text (Req 6).
        let write_results: Vec<&str> = [
            ("status_updated", result.status_updated),
            ("registry_updated", result.registry_updated),
            (
                "projections_updated",
                !result.projections_updated.is_empty(),
            ),
        ]
        .into_iter()
        .filter(|(_, on)| *on)
        .map(|(name, _)| name)
        .collect();
        tracing::info!(
            command = "snapshot",
            actor = %req_actor_name,
            hearth = %resolved.display(),
            events = %write_results.join(","),
            surface = seam.surface(),
            outcome = "ok",
            "snapshot command ok"
        );
        seam.ok();

        // Universal activity log (additive). snapshot carries an actor; the
        // playbook kind is resolved from the artifact's status.yaml at log time
        // (spec req #1) so the turn is attributable per-kind.
        self.emit_activity(
            "snapshot",
            "ok",
            &snapshot_kind,
            "",
            &snapshot_to_state,
            &req_actor_name,
            &resolved,
            &result.timestamp,
            "",
            snapshot_conversation_hash.clone(),
            snapshot_project_label.clone(),
            Some(last_segment(&snapshot_artifact_path).to_string()),
            None,
        );

        if !snapshot_projection_only {
            self.emit_transition_measurement(
                &resolved,
                registry.as_ref(),
                last_segment(&snapshot_artifact_path),
                &snapshot_kind,
                &snapshot_from_state,
                &snapshot_to_state,
                &snapshot_role,
                "",
                &result.timestamp,
                snapshot_conversation_hash.clone(),
                snapshot_project_label.clone(),
                false,
                &snapshot_claimed_evidence,
                Some(&resolved.join(&snapshot_artifact_path)),
            );
            self.emit_playbook_measurement(
                &resolved,
                registry.as_ref(),
                last_segment(&snapshot_artifact_path),
                &snapshot_kind,
                &snapshot_to_state,
                &result.timestamp,
                snapshot_conversation_hash.clone(),
                snapshot_project_label.clone(),
                Some(&resolved.join(&snapshot_artifact_path)),
            );
        }

        // Full §0 temper stream for the snapshot transition (spec req #4 —
        // snapshot emits too, for EVERY kind). track_id = KIND. Gated on the
        // privacy decision; fail-open. playbook_id + intent/expected_output
        // resolve from a fresh registry scan keyed on (to_state, role). Skip
        // projection_only (spark/annotation) — not a lifecycle step transition.
        if !snapshot_projection_only {
            use anvil_core::domain::playbook::interpreter::state_role_measurement;
            // playbook_id = the per-RUN instance id (the artifact dir id), NOT
            // the playbook definition id (H1). intent/expected_output resolve
            // from the (to_state, role) measurement schema.
            let playbook_id = last_segment(&snapshot_artifact_path);
            let (intent, expected_output, evidence) = registry
                .machine_for(&snapshot_kind)
                .map(|machine| {
                    let (intent, expected_output) =
                        state_role_measurement(machine, &snapshot_to_state, &snapshot_role)
                            .map(|measurement| {
                                (
                                    measurement.intent.clone(),
                                    measurement.expected_output.clone(),
                                )
                            })
                            .unwrap_or_default();
                    let evidence = resolve_step_evidence(
                        machine,
                        &snapshot_to_state,
                        &snapshot_role,
                        &snapshot_claimed_evidence,
                    );
                    (intent, expected_output, evidence)
                })
                .unwrap_or_default();
            self.emit_lean_step_measurement(
                &resolved,
                playbook_id,
                &snapshot_kind,
                &snapshot_from_state,
                &snapshot_to_state,
                &snapshot_role,
                &req_actor_name,
                &intent,
                &expected_output,
                &result.timestamp,
                snapshot_conversation_hash.clone(),
                snapshot_project_label.clone(),
                evidence,
            );
            self.emit_step0_stream(
                &resolved,
                playbook_id,
                &snapshot_kind,
                &snapshot_from_state,
                &snapshot_to_state,
                &snapshot_role,
                &req_actor_name,
                &intent,
                &expected_output,
                &result.timestamp,
                &snapshot_actor_model,
                None,
                None,
            );
        }

        // Open-marker index + dedup (open_marker_index Phases 1/3). A snapshot
        // may have moved the playbook to a terminal state, so DROP the cached
        // candidate set for this conversation (the next route turn rebuilds via
        // scan; re-read confirmation already prevents a stale "open" verdict) and
        // RE-ARM the dedup (a state change → the next check-in nudge must fire).
        // Best-effort; empty conversation ids are ignored by both calls.
        self.open_marker_index
            .invalidate(&resolved, &snapshot_conversation_id);
        self.nudge_dedup.rearm(
            &resolved,
            &snapshot_conversation_id,
            last_segment(&snapshot_artifact_path),
        );

        Ok(Response::new(SnapshotResponse {
            success: result.success,
            timestamp: result.timestamp,
            status_updated: result.status_updated,
            registry_updated: result.registry_updated,
            projections_updated: result.projections_updated,
            warnings: result.warnings,
            actor_name: result.actor_name,
            resolved_hearth: resolved.display().to_string(),
        }))
    }

    async fn complete(
        &self,
        request: Request<CompleteRequest>,
    ) -> Result<Response<CompleteResponse>, Status> {
        // Gatekeeper FIRST (spec Req 5), BEFORE resolve_hearth and BEFORE the
        // per-hearth lock. Standalone ⇒ Ok(None), no-op. R2 binds the
        // persisted transition actor to the verified `sub` (spec Req 7).
        let session = self.authorize(&request).await?;
        // The CQRS command seam, read before the hearth is resolved and before
        // the lock is taken. A complete naming no surface changes nothing.
        let surface = command_seam::read_surface(&request)
            .map_err(|refusal| command_seam::refuse("complete", refusal))?;
        let req = request.into_inner();
        let resolved = resolve_hearth(
            &req.hearth_path,
            self.hearth_path.as_deref(),
            &self.hearth_policy,
        )?;
        // Serialize concurrent complete writes — same criticality as snapshot.
        let _guard = self.hearth_locks.lock_for(&resolved).await;
        let at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        // Bind the persisted transition actor to the verified `sub` under
        // Foundry (spec Req 7); standalone keeps the caller-supplied name
        // verbatim (Req 3). Captured also for the outcome log.
        let actor_name = Self::principal_actor_name(&session, req.actor_name.clone());
        let req_actor_name = actor_name.clone();
        let mut seam = CommandSeam::issue("complete", surface, &req_actor_name);
        // Build the playbook registry exactly as begin does (D-6) so the
        // step_measurement emit can resolve `playbook_id_for(kind)` and the
        // `(from_state, role)` measurement spec. A fresh per-call hearth scan
        // with seed fallback (always-reload, spec R7.2).
        let registry = self.request_session_registry(&resolved).registry;
        // Retain the legacy call-shape role as a fallback for machines whose
        // selected transition cannot be resolved after command execution.
        let complete_role = if req.satisfaction.is_empty() {
            "doer"
        } else {
            "reviewer"
        };
        let complete_actor_type = req.actor_type.clone();
        let complete_actor_model = req.actor_model.clone();
        let complete_actor_provider = req.actor_provider.clone();
        let complete_artifact_path = req.artifact_path.clone();
        let complete_satisfaction = req.satisfaction.clone();
        let complete_conversation_id = if req.conversation_id.trim().is_empty() {
            let query = FileSystemQueryAdapter::new(resolved.clone());
            match open_begin_conversation_id_for_artifact(&query, &complete_artifact_path) {
                Ok(Some(conversation_id)) => conversation_id,
                Ok(None) => String::new(),
                Err(e) => {
                    tracing::warn!(
                        command = "complete",
                        outcome = "open_begin_conversation_lookup_failed",
                        error = %e,
                        "complete could not inherit conversation id from open begin marker"
                    );
                    String::new()
                }
            }
        } else {
            req.conversation_id.clone()
        };
        let complete_conversation_hash = self.conversation_hash(&complete_conversation_id);
        let complete_project_label = Self::project_label(&req.project_root);
        // P1 retains the typed claims across handler consumption; P2 wires
        // this copy to the post-handler evidence emitter.
        let MappedLifecycleRequest {
            domain_request,
            claimed_evidence: complete_claimed_evidence,
        } = complete_request_to_domain(req, actor_name, at).map_err(Status::invalid_argument)?;

        // Phase 2 CQRS: pure handler reads via QueryPort, emits Vec<CompleteEvent>.
        let query_adapter = FileSystemQueryAdapter::new(resolved.clone());

        // K5 (R9) terminal-resolve no-op guard — dark-by-default behind
        // ANVIL_K5_BIND. The run-complete resolve calls Complete (no satisfaction)
        // on the bound instance; a Kiln retry on an already-`completed` instance
        // would otherwise be REJECTED (a terminal state has no forward doer edge,
        // so select_edge errors WrongStateForComplete). When the instance is
        // already terminal AND the call is well-formed (non-empty actor identity —
        // a genuinely missing-actor call falls through to the handler's §4.3b
        // INVALID_ARGUMENT), short-circuit to an idempotent no-op success: record
        // NO second transition and return OK (spec R9 / §4.4; contract §6). A read
        // error (unknown/malformed instance) falls through so the handler surfaces
        // the proper §4.3b non-OK status. Inert when the flag is unset (A7).
        if k5_bind_enabled()
            && !req_actor_name.is_empty()
            && !complete_actor_type.is_empty()
            && !complete_actor_model.is_empty()
            && !complete_actor_provider.is_empty()
        {
            use anvil_core::ports::query_port::QueryPort;
            if let Ok(current_state) = query_adapter.read_artifact_state(&complete_artifact_path) {
                if anvil_core::domain::is_terminal_state(&current_state) {
                    return Ok(Response::new(CompleteResponse {
                        new_state: current_state,
                        transition_at: chrono::Utc::now()
                            .format("%Y-%m-%dT%H:%M:%SZ")
                            .to_string(),
                        artifact_path: complete_artifact_path.clone(),
                        next_step: String::new(),
                        reflection_path: String::new(),
                        resolved_hearth: resolved.display().to_string(),
                        warnings: Vec::new(),
                        carry_forward_path: String::new(),
                    }));
                }
            }
        }

        let mut outcome =
            CompleteCommandHandler::execute(&query_adapter, registry.as_ref(), domain_request)
                .map_err(complete_error_to_status)?;

        use anvil_core::domain::complete_events::CompleteEvent;
        let complete_has_lifecycle_transition = outcome
            .events
            .iter()
            .any(|event| matches!(event, CompleteEvent::TransitionRecorded { .. }));
        // The pure handler has selected the exact edge. Prefer that edge's role
        // when it names a measurement, then refuse (when both dark guards allow)
        // before terminal generation, actor/reflection files, or transition
        // routing can mutate the hearth.
        let complete_step_role = registry
            .machine_for(&outcome.result.kind)
            .map(|machine| {
                completed_step_measurement_role(
                    machine,
                    &outcome.result.from_state,
                    &outcome.result.selected_required_role,
                    complete_role,
                )
            })
            .unwrap_or_else(|| complete_role.to_string());
        if complete_has_lifecycle_transition {
            enforce_claimed_evidence_preflight(
                claimed_evidence_gate_active(
                    &resolved,
                    self.global_playbooks_hearth.as_deref(),
                ),
                registry.as_ref(),
                &outcome.result.kind,
                &outcome.result.from_state,
                &complete_step_role,
                &complete_claimed_evidence,
            )
            .map_err(claimed_evidence_gate_error_to_status)?;
            // The merge check runs on the SAME side of the first write. The
            // pure handler has already selected the edge, so `new_state` is the
            // state this call would land in.
            enforce_completion_merge_check(
                &outcome.result.new_state,
                &resolved,
                &complete_claimed_evidence,
            )?;
        }

        use anvil_core::ports::actor_write_port::ActorWritePort;
        use anvil_core::ports::reflection_write_port::ReflectionWritePort;
        let actor_write_adapter =
            anvil_core_hearth::fs_actor_write_adapter::FileSystemActorWriteAdapter::new(
                resolved.clone(),
            );
        let reflection_write_adapter = FileSystemReflectionWriteAdapter::new(resolved.clone());

        persist_completed_playbook_generation(
            &resolved,
            &outcome.result.artifact_path,
            &outcome.result.kind,
            &outcome.result.new_state,
            &req_actor_name,
            &complete_actor_type,
            &complete_actor_model,
            &complete_actor_provider,
            self.publication_log.as_ref(),
        )?;

        for event in &outcome.events {
            match event {
                CompleteEvent::ActorUpserted {
                    artifact_path,
                    identity,
                } => {
                    actor_write_adapter
                        .upsert_actor_configuration(artifact_path, identity)
                        .map_err(|e| Status::internal(format!("actor upsert failed: {}", e)))?;
                }
                CompleteEvent::ReflectionWritten {
                    artifact_path,
                    source_state,
                    filename,
                    body,
                } => {
                    let path = reflection_write_adapter
                        .write_reflection_file(artifact_path, source_state, filename, body)
                        .map_err(|e| {
                            complete_error_to_status(
                                anvil_core::domain::complete::CompleteError::from(e),
                            )
                        })?;
                    outcome.result.reflection_path = path;
                }
                CompleteEvent::CarryForwardWritten {
                    artifact_path,
                    body,
                } => {
                    // Slice C: write carry-forward.md as a primary artifact,
                    // BEFORE the status/registry/projection transition (R2.4).
                    // Fail-fast on error — no transition is recorded if the
                    // carry-forward write fails.
                    use anvil_core::ports::snapshot_port::SnapshotPort;
                    let snapshot_adapter = FileSystemSnapshotAdapter::new(resolved.clone());
                    let path = snapshot_adapter
                        .write_carry_forward(artifact_path, body)
                        .map_err(|e| {
                            Status::internal(format!("carry-forward.md write failed: {}", e))
                        })?;
                    outcome.result.carry_forward_path = path;
                }
                CompleteEvent::TransitionRecorded {
                    artifact_path,
                    to_state,
                    at,
                    role,
                    approver,
                    note,
                    actor_name,
                    satisfaction,
                } => {
                    use anvil_core::domain::shared_types::TransitionContent;
                    use anvil_core::ports::snapshot_port::SnapshotPort;
                    let snapshot_adapter = FileSystemSnapshotAdapter::new(resolved.clone());
                    // === Criticality 1: status.yaml append (fail-fast) ===
                    let transition = TransitionContent {
                        to: to_state.clone(),
                        at: at.clone(),
                        actor: actor_name.clone(),
                        role: role.clone(),
                        approver: approver.clone(),
                        note: note.clone(),
                        satisfaction: satisfaction.clone(),
                        event_type: None,
                    };
                    snapshot_adapter
                        .append_transition(artifact_path, &transition)
                        .map_err(|e| {
                            Status::internal(format!("status.yaml append failed: {}", e))
                        })?;
                    let artifact_id = artifact_path
                        .rsplit('/')
                        .next()
                        .unwrap_or(artifact_path.as_str());
                    // === Criticality 2: registry (warn-and-continue) ===
                    // DATA-DRIVEN registry placement. Resolve the machine for
                    // this kind and look up the destination state's declared
                    // `registry_section`. Empty machine registry OR empty
                    // registry_section means no registry move.
                    let result_kind = outcome.result.kind.clone();
                    let machine = registry.machine_for(&result_kind);
                    let registry_file = machine
                        .map(|m| m.registry.clone())
                        .unwrap_or_else(|| "tracks.md".to_string());
                    let machine_state =
                        machine.and_then(|m| m.states.iter().find(|s| s.name == *to_state));
                    let registry_section = machine_state
                        .map(|s| s.registry_section.clone())
                        .unwrap_or_default();
                    let projection_section = machine_state
                        .and_then(|s| s.projection_targets.first().cloned())
                        .unwrap_or_default();
                    if !registry_file.is_empty() && !registry_section.is_empty() {
                        match snapshot_adapter.registry_entry_exists(&registry_file, artifact_id) {
                            Ok(true) => {
                                if let Err(e) = snapshot_adapter.move_registry_entry(
                                    &registry_file,
                                    artifact_id,
                                    &registry_section,
                                ) {
                                    outcome
                                        .result
                                        .warnings
                                        .push(format!("registry update failed: {}", e));
                                }
                            }
                            Ok(false) => {
                                match snapshot_adapter.build_registry_entry_text(
                                    &result_kind,
                                    artifact_path,
                                    &registry_section,
                                ) {
                                    Ok(entry_text) => {
                                        if let Err(e) = snapshot_adapter.create_registry_entry(
                                            &registry_file,
                                            artifact_id,
                                            &result_kind,
                                            &registry_section,
                                            &entry_text,
                                        ) {
                                            outcome
                                                .result
                                                .warnings
                                                .push(format!("registry update failed: {}", e));
                                        }
                                    }
                                    Err(e) => outcome
                                        .result
                                        .warnings
                                        .push(format!("registry update failed: {}", e)),
                                }
                            }
                            Err(e) => outcome
                                .result
                                .warnings
                                .push(format!("registry update failed: {}", e)),
                        }
                    }
                    // === Criticality 3: projection (warn-and-continue) ===
                    if !projection_section.is_empty() {
                        if let Err(e) =
                            snapshot_adapter.move_execution_row(artifact_id, &projection_section)
                        {
                            outcome
                                .result
                                .warnings
                                .push(format!("projection update failed (execution.md): {}", e));
                        }
                    }
                }
            }
        }

        // Crucible publication-log mirror (best-effort; gated; never fatal).
        if let Some(pub_log) = self.publication_log.as_ref() {
            for event in &outcome.events {
                pub_log.mirror_complete(event);
            }
        }
        // M-P3: emit the per-step measurement AFTER the transition/routing
        // succeeded (additive — a failed complete never reaches here). The real
        // from_state->to_state comes from `CompleteResult`; intent/expected_output
        // from the selected `(from_state, role)` schema (empty when undeclared).
        // Warn-first: declared OUTSIDE the measurement block so it can reach the
        // response. The transition succeeded; the warning rides alongside it.
        let mut complete_evidence_warning: Option<String> = None;
        {
            use anvil_core::domain::playbook::interpreter::state_role_measurement;
            let kind = &outcome.result.kind;
            // playbook_id = the per-RUN instance id (the run/artifact dir id),
            // NOT the playbook definition id (H1) — it threads one run's steps.
            // track_id = the playbook KIND: the Scorecard B aggregation key
            // across instances. They are DIFFERENT values.
            let playbook_id = last_segment(&outcome.result.artifact_path);
            let track_id = kind.as_str();
            if complete_has_lifecycle_transition {
                let evidence_warning = self.emit_transition_measurement(
                    &resolved,
                    registry.as_ref(),
                    playbook_id,
                    kind,
                    &outcome.result.from_state,
                    &outcome.result.new_state,
                    &complete_step_role,
                    &complete_satisfaction,
                    &outcome.result.transition_at,
                    complete_conversation_hash.clone(),
                    complete_project_label.clone(),
                    false,
                    &complete_claimed_evidence,
                    Some(&resolved.join(&outcome.result.artifact_path)),
                );
                if let Some(w) = evidence_warning.clone() {
                    complete_evidence_warning = Some(w);
                }
                self.emit_playbook_measurement(
                    &resolved,
                    registry.as_ref(),
                    playbook_id,
                    kind,
                    &outcome.result.new_state,
                    &outcome.result.transition_at,
                    complete_conversation_hash.clone(),
                    complete_project_label.clone(),
                    Some(&resolved.join(&outcome.result.artifact_path)),
                );
                // Phase D: capture the structured review verdict when leaving a
                // review-gate state (no-op for non-review-gate from_states).
                self.emit_review_verdict(
                    &resolved,
                    registry.as_ref(),
                    playbook_id,
                    kind,
                    &outcome.result.from_state,
                    &complete_satisfaction,
                    &outcome.result.transition_at,
                    complete_conversation_hash.clone(),
                    complete_project_label.clone(),
                );
            }
            let (intent, expected_output, evidence) = registry
                .machine_for(kind)
                .map(|machine| {
                    let (intent, expected_output) = state_role_measurement(
                        machine,
                        &outcome.result.from_state,
                        &complete_step_role,
                    )
                    .map(|measurement| {
                        (
                            measurement.intent.clone(),
                            measurement.expected_output.clone(),
                        )
                    })
                    .unwrap_or_default();
                    let evidence = resolve_step_evidence(
                        machine,
                        &outcome.result.from_state,
                        &complete_step_role,
                        &complete_claimed_evidence,
                    );
                    (intent, expected_output, evidence)
                })
                .unwrap_or_default();
            emit_step_measurement(
                playbook_id,
                track_id,
                &outcome.result.from_state,
                &outcome.result.new_state,
                &complete_step_role,
                &req_actor_name,
                &intent,
                &expected_output,
                &outcome.result.transition_at,
            );

            // Full §0 temper stream (track_id = KIND, not the run id), gated on
            // the privacy decision. Emit for EVERY kind. Fail-open.
            self.emit_step0_stream(
                &resolved,
                playbook_id,
                kind,
                &outcome.result.from_state,
                &outcome.result.new_state,
                &complete_step_role,
                &req_actor_name,
                &intent,
                &expected_output,
                &outcome.result.transition_at,
                &complete_actor_model,
                None,
                None,
            );

            self.emit_lean_step_measurement(
                &resolved,
                playbook_id,
                kind,
                &outcome.result.from_state,
                &outcome.result.new_state,
                &complete_step_role,
                &req_actor_name,
                &intent,
                &expected_output,
                &outcome.result.transition_at,
                complete_conversation_hash.clone(),
                complete_project_label.clone(),
                evidence,
            );
        }

        // AC1: per-command outcome record. Log event VARIANT NAMES only — never
        // the note or reflection body carried inside them (Req 6).
        let complete_events: Vec<&str> = outcome.events.iter().map(complete_event_name).collect();
        tracing::info!(
            command = "complete",
            actor = %req_actor_name,
            hearth = %resolved.display(),
            events = %complete_events.join(","),
            surface = seam.surface(),
            outcome = "ok",
            "complete command ok"
        );
        seam.ok();

        // Universal activity log (additive). complete carries an actor and the
        // resolved playbook kind.
        self.emit_activity(
            "complete",
            "ok",
            &outcome.result.kind,
            &outcome.result.from_state,
            &outcome.result.new_state,
            &req_actor_name,
            &resolved,
            &outcome.result.transition_at,
            "",
            complete_conversation_hash.clone(),
            complete_project_label.clone(),
            Some(last_segment(&outcome.result.artifact_path).to_string()),
            None,
        );

        let next_step = complete_next_step(
            registry.as_ref(),
            &outcome.result.kind,
            &outcome.result.new_state,
        );

        // Open-marker index + dedup (open_marker_index Phases 1/3). A complete
        // moves the playbook forward (often to terminal), so DROP the cached
        // candidate set for this conversation (next route turn rebuilds via scan;
        // re-read confirmation prevents a stale "open" verdict) and RE-ARM the
        // dedup (a state change → the next check-in nudge must fire). Best-effort;
        // empty conversation ids are ignored by both calls.
        self.open_marker_index
            .invalidate(&resolved, &complete_conversation_id);
        self.nudge_dedup.rearm(
            &resolved,
            &complete_conversation_id,
            last_segment(&complete_artifact_path),
        );

        Ok(Response::new(CompleteResponse {
            new_state: outcome.result.new_state,
            transition_at: outcome.result.transition_at,
            artifact_path: outcome.result.artifact_path,
            next_step,
            reflection_path: outcome.result.reflection_path,
            resolved_hearth: resolved.display().to_string(),
            warnings: {
                let mut w = outcome.result.warnings;
                if let Some(x) = complete_evidence_warning {
                    w.push(x);
                }
                w
            },
            carry_forward_path: outcome.result.carry_forward_path,
        }))
    }

    async fn amend(
        &self,
        request: Request<AmendRequest>,
    ) -> Result<Response<AmendResponse>, Status> {
        // Gatekeeper FIRST (AC-7), BEFORE resolve_hearth and the per-hearth
        // lock. Standalone ⇒ Ok(None), no-op. Mirrors complete.
        let session = self.authorize(&request).await?;
        // The CQRS command seam, read before the hearth is resolved and before
        // the lock is taken. An amend naming no surface changes nothing.
        let surface = command_seam::read_surface(&request)
            .map_err(|refusal| command_seam::refuse("amend", refusal))?;
        let req = request.into_inner();
        let resolved = resolve_hearth(
            &req.hearth_path,
            self.hearth_path.as_deref(),
            &self.hearth_policy,
        )?;
        // Serialize concurrent amend writes — same criticality as complete.
        let _guard = self.hearth_locks.lock_for(&resolved).await;
        let at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        // Bind the persisted actor to the verified `sub` under Foundry; standalone
        // keeps the caller-supplied name verbatim. Captured also for the log.
        let actor_name = Self::principal_actor_name(&session, req.actor_name);
        let req_actor_name = actor_name.clone();
        let mut seam = CommandSeam::issue("amend", surface, &req_actor_name);

        // Build the playbook registry exactly as complete/begin do (HIGH-2) so the
        // handler can resolve the artifact's lifecycle machine for state-driving.
        let registry = self.request_session_registry(&resolved).registry;

        let domain_request = DomainAmendRequest {
            artifact_path: req.artifact_path,
            kind: req.kind,
            target_document: req.target_document,
            target_id: req.target_id,
            op_kind: req.op_kind,
            body: req.body,
            new_kind: req.new_kind,
            anchor: req.anchor,
            actor_name,
            actor_type: req.actor_type,
            actor_model: req.actor_model,
            actor_provider: req.actor_provider,
            actor_context_window: req.actor_context_window,
            actor_sdk_version: req.actor_sdk_version,
            actor_entrypoint: req.actor_entrypoint,
            at,
        };

        // Phase 2 CQRS: pure handler reads via QueryPort + registry, emits events.
        let query_adapter = FileSystemQueryAdapter::new(resolved.clone());
        let outcome =
            AmendCommandHandler::execute(&query_adapter, registry.as_ref(), domain_request)
                .map_err(amend_error_to_status)?;

        use anvil_core::domain::amend_events::AmendEvent;
        use anvil_core::ports::actor_write_port::ActorWritePort;
        use anvil_core::ports::op_log_write_port::OpLogWritePort;
        let op_log_adapter =
            anvil_core_hearth::fs_op_log_adapter::FileSystemOpLogAdapter::new(resolved.clone());
        let actor_write_adapter = FileSystemActorWriteAdapter::new(resolved.clone());

        // Route events inline (mirror complete). Exhaustive match — no catch-all.
        for event in &outcome.events {
            match event {
                AmendEvent::OpRecorded {
                    artifact_path,
                    target_document,
                    entry,
                } => {
                    op_log_adapter
                        .append_op(artifact_path, target_document, entry)
                        .map_err(|e| Status::internal(format!("op log append failed: {}", e)))?;
                }
                AmendEvent::ActorUpserted {
                    artifact_path,
                    identity,
                } => {
                    actor_write_adapter
                        .upsert_actor_configuration(artifact_path, identity)
                        .map_err(|e| Status::internal(format!("actor upsert failed: {}", e)))?;
                }
                AmendEvent::TransitionRecorded {
                    artifact_path,
                    to_state,
                    at,
                    role,
                    actor_name,
                } => {
                    use anvil_core::domain::shared_types::TransitionContent;
                    use anvil_core::ports::snapshot_port::SnapshotPort;
                    let snapshot_adapter = FileSystemSnapshotAdapter::new(resolved.clone());
                    // Interpreter-validated transition into `amend`. status.yaml
                    // append (fail-fast), exactly like complete's direct dispatch.
                    let transition = TransitionContent {
                        to: to_state.clone(),
                        at: at.clone(),
                        actor: actor_name.clone(),
                        role: role.clone(),
                        approver: None,
                        note: None,
                        satisfaction: None,
                        event_type: None,
                    };
                    snapshot_adapter
                        .append_transition(artifact_path, &transition)
                        .map_err(|e| {
                            Status::internal(format!("status.yaml append failed: {}", e))
                        })?;
                }
            }
        }

        // Crucible publication-log mirror (best-effort; gated; never fatal).
        if let Some(pub_log) = self.publication_log.as_ref() {
            for event in &outcome.events {
                pub_log.mirror_amend(event);
            }
        }

        // Per-command outcome record. Log event VARIANT NAMES only (Req 6).
        let amend_events: Vec<&str> = outcome.events.iter().map(amend_event_name).collect();
        tracing::info!(
            command = "amend",
            actor = %req_actor_name,
            hearth = %resolved.display(),
            events = %amend_events.join(","),
            surface = seam.surface(),
            outcome = "ok",
            "amend command ok"
        );
        seam.ok();

        // Universal activity log (additive). amend carries an actor; its
        // amendment kind is not a playbook kind, so artifact_kind stays empty.
        self.emit_activity(
            "amend",
            "ok",
            "",
            "",
            "",
            &req_actor_name,
            &resolved,
            &chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            "",
            None,
            None,
            None,
            None,
        );

        Ok(Response::new(AmendResponse {
            op_id: outcome.result.op_id,
            resolved_hearth: resolved.display().to_string(),
            new_state: outcome.result.new_state.unwrap_or_default(),
        }))
    }

    async fn persist_playbook(
        &self,
        request: Request<PersistPlaybookRequest>,
    ) -> Result<Response<PersistPlaybookResponse>, Status> {
        // Gatekeeper FIRST (mirror amend), BEFORE resolve_hearth and the lock.
        // Standalone ⇒ Ok(None), no-op.
        let session = self.authorize(&request).await?;
        let req = request.into_inner();
        // The engine's OWN hearth — used for authorize/lock scope only, NOT the
        // write target. The write + duplicate-kind check both happen under
        // `owner_home` (the TARGET), which is an explicit absolute path arg.
        let resolved = resolve_hearth(
            &req.hearth_path,
            self.hearth_path.as_deref(),
            &self.hearth_policy,
        )?;
        // Serialize concurrent persist writes — same criticality as amend.
        let _guard = self.hearth_locks.lock_for(&resolved).await;
        // Bind the persisted actor to the verified `sub` under Foundry; standalone
        // keeps the caller-supplied name verbatim. Captured for the log only.
        let actor_name = Self::principal_actor_name(&session, req.actor_name);
        let req_actor_name = actor_name.clone();

        // Build the registry from the TARGET owner-home (NOT the engine hearth)
        // so the handler's duplicate-kind check observes the write target (A5).
        let owner_home = normalize_persist_owner_home(&req.owner_home)?;
        // C-d.1 round 5, M-1: the round-4 `persist_hearth_root_guard(&owner_home)`
        // that stood here is REMOVED. It had no falsifiable behavioral delta —
        // deleting both of its call sites left the whole engine suite
        // byte-identical, because on every state it refused,
        // `HearthPlaybookRegistry::new()` refused the same state through
        // `registration_blocked()` and nothing had moved either way; and on the
        // one state where the rename actually happens (a legacy-only hearth) the
        // guard returned `Ok` and the rename proceeded. It could not be made
        // load-bearing without changing rename behaviour, which is
        // `NG-DATA-CUTOVER`'s decision and not a fix round's. A second guard
        // nothing can detect the absence of is a second contract to keep in
        // sync, so it is gone rather than re-argued. The domain refusal inside
        // `PersistPlaybookCommandHandler::execute_impl` is the one that blocks,
        // and it is pinned at both the domain seam and the RPC.
        let target_registry = HearthPlaybookRegistry::new(PathBuf::from(&owner_home));

        let domain_request = DomainPersistPlaybookRequest {
            owner_home: owner_home.clone(),
            kind: req.kind,
            machine_yaml: req.machine_yaml,
            exemplars: Vec::new(),
            // Map the proto repeated PlaybookHook into (filename, content) pairs.
            // The handler validates the machine against these filenames.
            hooks: req
                .hooks
                .into_iter()
                .map(|h| (h.name, h.content))
                .collect(),
            actor_name,
            actor_type: req.actor_type,
            actor_model: req.actor_model,
            actor_provider: req.actor_provider,
        };

        // Pure CQRS handler at the direct persist WRITE boundary: enforcing
        // loader (measurement DEFINE) + hook-coverage — a predicate-less or
        // hookless machine is refused here regardless of the runtime dark-gate,
        // so new junk can never land silently via this seam. (The generator's
        // terminal persist stays on plain `execute`; its hookless-by-construction
        // machines are a documented, separate gap.) Measurement is ALWAYS on at
        // this WRITE boundary; the evidence-obligation leg rides the
        // ANVIL_ENFORCE_EVIDENCE_OBLIGATION dark-gate (default OFF ⇒ byte-identical
        // to `execute_enforcing`; T-EEC-1 P4).
        let outcome = PersistPlaybookCommandHandler::execute_enforcing_with(
            &target_registry,
            domain_request,
            LoaderEnforcement {
                measurement_definition: true,
                evidence_obligation: enforce_evidence_obligation(),
            },
        )
        .map_err(persist_playbook_error_to_status)?;

        // Route the persist event inline (mirror amend) — its OWN event enum,
        // exhaustively matched (no catch-all). The FileSystemArtifactAdapter is
        // constructed with the owner_home defensively; the op writes under the
        // owner_home ARG regardless (BP1 proved hearth_path is vestigial here).
        use anvil_core::domain::persist_playbook_events::PersistPlaybookEvent;
        let artifact_adapter = FileSystemArtifactAdapter::new(PathBuf::from(&owner_home));
        let mut written_path = String::new();
        for event in &outcome.events {
            match event {
                PersistPlaybookEvent::PlaybookPersisted {
                    owner_home,
                    kind,
                    machine_yaml,
                    hooks,
                    exemplars,
                } => {
                    written_path = artifact_adapter
                        .persist_generated_playbook(
                            owner_home,
                            kind,
                            machine_yaml,
                            Some(hooks.as_slice()),
                            Some(exemplars.as_slice()),
                        )
                        .map_err(persist_playbook_artifact_error_to_status)?;
                }
            }
        }

        // Crucible publication-log mirror (best-effort; gated; never fatal).
        if let Some(pub_log) = self.publication_log.as_ref() {
            for event in &outcome.events {
                pub_log.mirror_persist(event);
            }
        }

        if written_path.is_empty() {
            written_path = Path::new(&owner_home)
                .join("playbooks")
                .join(&outcome.result.kind)
                .join("machine.yaml")
                .display()
                .to_string();
        }

        // Per-command outcome record. Log event VARIANT NAMES only.
        let persist_events: Vec<&str> = outcome.events.iter().map(persist_event_name).collect();
        tracing::info!(
            command = "persist_playbook",
            actor = %req_actor_name,
            hearth = %resolved.display(),
            owner_home = %owner_home,
            events = %persist_events.join(","),
            outcome = "ok",
            "persist_playbook command ok"
        );

        // Universal activity log (additive). persist_playbook carries an actor
        // and the persisted playbook kind.
        self.emit_activity(
            "persist_playbook",
            "ok",
            &outcome.result.kind,
            "",
            "",
            &req_actor_name,
            &resolved,
            &chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            "",
            None,
            None,
            None,
            None,
        );

        Ok(Response::new(PersistPlaybookResponse {
            kind: outcome.result.kind,
            written_path,
            resolved_owner_home: owner_home,
        }))
    }

    async fn intake_candidate_playbook(
        &self,
        request: Request<IntakeCandidatePlaybookRequest>,
    ) -> Result<Response<IntakeCandidatePlaybookResponse>, Status> {
        // Gatekeeper FIRST (mirror begin/amend), BEFORE resolve_hearth and lock.
        let session = self.authorize(&request).await?;
        let req = request.into_inner();
        let resolved = resolve_hearth(
            &req.hearth_path,
            self.hearth_path.as_deref(),
            &self.hearth_policy,
        )?;
        let _begin_guard = self.hearth_locks.lock_for(&resolved).await;

        let candidate = proto_candidate_playbook_to_domain(req.candidate)?
            .ok_or_else(|| Status::invalid_argument("candidate is required"))?;
        if candidate.source.trim().is_empty() {
            return Err(Status::invalid_argument(
                "invalid_candidate: candidate.source must be a non-empty string",
            ));
        }
        if candidate.at.trim().is_empty() {
            return Err(Status::invalid_argument(
                "invalid_candidate: candidate.at must be a non-empty string",
            ));
        }
        // Intake honors the measurement dark-gate, exactly like the terminal-
        // generation persist: enforcing generator when
        // ANVIL_ENFORCE_MEASUREMENT_DEFINITION is on, plain `generate` otherwise.
        // Default OFF, so the ordinary intake path is unchanged. When ON, the
        // enforcing generator refuses a candidate lacking a falsifiable per-step
        // `success_criteria` or an `outcome_predicate`
        // (GenerateError::VacuousSuccessCriteria / MissingOutcomePredicate), so a
        // predicate-less candidate can never open a generation track under
        // enforcement. The wire contract (proto `ProposedState.success_criteria`
        // + `CandidatePlaybook.outcome_predicate`) cannot yet carry these, so
        // every proto-intake candidate is refused while the flag is on — the
        // deliberate dark-gate posture until that follow-on lands.
        let machine = if enforce_measurement_definition() {
            generate_candidate_playbook_enforcing(&candidate)
        } else {
            generate_candidate_playbook(&candidate)
        }
        .map_err(|e| Status::invalid_argument(format!("invalid_candidate: {:?}", e)))?;
        // Evidence-obligation dark-gate at intake (T-EEC-1 P4), the third seam the
        // gate rides in lockstep with loader/registry + persist. When
        // ANVIL_ENFORCE_EVIDENCE_OBLIGATION is on, the generated machine must
        // satisfy the shared loader-side obligation validator. Candidate register
        // and per-state obligations survive proto mapping and generation before
        // this check, so FREE declarations receive the FREE-specific error while
        // compliant DRIVEN candidates can proceed.
        if enforce_evidence_obligation() {
            validate_evidence_obligation(&machine, &machine.kind)
                .map_err(|e| Status::invalid_argument(format!("invalid_candidate: {}", e)))?;
        }
        let playbook_name = machine.kind;
        let instance_name_component = intake_instance_name_component(&candidate);
        let begin_track_name = format!("{}__{}", playbook_name, instance_name_component);
        let approver = if req.approver.trim().is_empty() {
            "lore".to_string()
        } else {
            req.approver
        };
        let target_owner = req.target_owner.clone();
        let parent_id = req.parent_id.clone();
        let intake_begin_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let generation_context = serde_json::json!({
            "kind": "playbook_generation",
            "playbook_name": playbook_name.clone(),
            "instance_name_component": instance_name_component.clone(),
            "target_owner": target_owner.clone(),
            "parent_id": parent_id.clone(),
            "approver": approver.clone(),
            "seed": {
                "candidate": candidate.clone()
            }
        });
        let context_json = serde_json::to_string_pretty(&generation_context)
            .map_err(|e| Status::internal(format!("serialize generation context failed: {}", e)))?;

        let actor_name = Self::principal_actor_name(&session, req.actor_name);
        let req_actor_name = actor_name.clone();
        let query_adapter = FileSystemQueryAdapter::new(resolved.clone());
        let registry = self.request_session_registry(&resolved).registry;

        let domain_request = DomainBeginRequest {
            ctx: proto_request_context("Foundation", "", "read", "internal"),
            artifact_type: "playbook_generation".to_string(),
            parent_id,
            track_name: begin_track_name,
            playbook_name: playbook_name.clone(),
            target_owner,
            fields: std::collections::BTreeMap::new(),
            approver: approver.clone(),
            actor_name,
            actor_type: req.actor_type,
            actor_model: req.actor_model,
            actor_provider: req.actor_provider,
            actor_context_window: req.actor_context_window,
            actor_sdk_version: req.actor_sdk_version,
            actor_entrypoint: req.actor_entrypoint,
            identifier: String::new(),
            session_role: "creator".to_string(),
            adopt: false,
            conversation_id: String::new(),
            rd_turn_id: String::new(),
            rd_input: String::new(),
            rd_candidate_set: String::new(),
            rd_selected: String::new(),
            rd_confidence: String::new(),
            claimed_evidence: Vec::new(),
        };

        let hook_reader = SourceAwarePlaybookHookBodyReader {
            request_hearth: resolved.clone(),
        };
        let mut outcome = BeginCommandHandler::execute_with_hook_reader(
            &query_adapter,
            &hook_reader,
            registry.as_ref(),
            domain_request,
        )
        .map_err(begin_error_to_status)?;

        let artifact_adapter = FileSystemArtifactAdapter::new(resolved.clone());
        let mut creation_kind_role: Option<(String, String)> = None;
        for event in &outcome.events {
            match event {
                Event::ArtifactCreation {
                    track_name,
                    parent_id,
                    display_name: _,
                    actor,
                    approver,
                    status,
                    directory,
                    registry_file: _,
                    scaffold_files,
                    creation_role,
                    conversation_id: _,
                } => {
                    let initial_status_yaml = build_initial_artifact_status_yaml(
                        &status.kind,
                        &status.state,
                        parent_id,
                        &status.target_owner,
                        &status.fields,
                        &status.origin_turn,
                    )
                    .replacen(
                        "actors:\n",
                        &format!("playbook_name: {}\nactors:\n", playbook_name),
                        1,
                    );
                    let mut files = scaffold_files.clone();
                    files.push(("generation-context.json".to_string(), context_json.clone()));
                    let scaffold_refs = files
                        .iter()
                        .map(|(filename, contents)| (filename.as_str(), contents.as_str()))
                        .collect::<Vec<_>>();
                    let artifact_path = artifact_adapter
                        .scaffold_artifact_directory(
                            directory,
                            track_name,
                            &initial_status_yaml,
                            &scaffold_refs,
                        )
                        .map_err(|e| begin_error_to_status(BeginError::from(e)))?;

                    let snapshot_adapter = FileSystemSnapshotAdapter::new(resolved.clone());
                    let actor_write_adapter = FileSystemActorWriteAdapter::new(resolved.clone());
                    let snapshot_request = DomainSnapshotRequest {
                        artifact_path: artifact_path.clone(),
                        to_state: status.state.clone(),
                        actor_name: actor.name.clone(),
                        actor_role: creation_role.clone(),
                        approver: approver.clone(),
                        note: String::new(),
                        actor_type: actor.actor_type.clone(),
                        actor_model: actor.model.clone(),
                        actor_provider: actor.provider.clone(),
                        actor_context_window: actor.context_window,
                        actor_sdk_version: actor.sdk_version.clone(),
                        actor_entrypoint: actor.entrypoint.clone(),
                        projection_only: false,
                        event_type: String::new(),
                        allow_reserved_event_type: false,
                        at: intake_begin_at.clone(),
                        claimed_evidence: Vec::new(),
                    };
                    SnapshotCommandHandler::execute(
                        &snapshot_adapter,
                        &actor_write_adapter,
                        snapshot_request,
                    )
                    .map_err(|e| begin_error_to_status(map_snapshot_error_to_begin(e)))?;
                    outcome.result.track_path = artifact_path;
                    creation_kind_role = Some((status.kind.clone(), "doer".to_string()));
                }
                other => {
                    return Err(Status::internal(format!(
                        "intake emitted unexpected begin event: {}",
                        begin_event_name(other)
                    )));
                }
            }
        }

        // Crucible publication-log mirror (best-effort; gated; never fatal).
        if let Some(pub_log) = self.publication_log.as_ref() {
            for event in &outcome.events {
                pub_log.mirror_begin(event);
            }
        }

        emit_begin_step_measurement(
            registry.as_ref(),
            creation_kind_role
                .as_ref()
                .map(|(kind, _)| kind.as_str())
                .unwrap_or_default(),
            creation_kind_role
                .as_ref()
                .map(|(_, role)| role.as_str())
                .unwrap_or_default(),
            &outcome.result.state,
            &outcome.result.track_path,
            &req_actor_name,
            &intake_begin_at,
        );

        let instance_id = last_segment(&outcome.result.track_path).to_string();
        let begin_events: Vec<&str> = outcome.events.iter().map(begin_event_name).collect();
        tracing::info!(
            command = "candidate_playbook_intake",
            actor = %req_actor_name,
            hearth = %resolved.display(),
            events = %begin_events.join(","),
            outcome = "ok",
            "intake candidate playbook command ok"
        );

        // Universal activity log (additive). Mirror the command label the tracing
        // event uses ("candidate_playbook_intake"); artifact_kind is the generated
        // playbook's kind.
        self.emit_activity(
            "candidate_playbook_intake",
            "ok",
            &playbook_name,
            "",
            "",
            &req_actor_name,
            &resolved,
            &intake_begin_at,
            "",
            None,
            None,
            None,
            None,
        );

        Ok(Response::new(IntakeCandidatePlaybookResponse {
            instance_id,
            kind: "playbook_generation".to_string(),
            resolved_owner_home: generation_context["target_owner"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            playbook_name: generation_context["playbook_name"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        }))
    }

    async fn health_check(
        &self,
        _request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        Ok(Response::new(HealthCheckResponse {
            status: "ok".to_string(),
            wire_proto_version: anvil_engine::WIRE_PROTO_VERSION,
            build_version: env!("CARGO_PKG_VERSION").to_string(),
        }))
    }

    /// Pure-read RPC: returns whether (actor_name, artifact_path, state) has an
    /// open begin-marker. No mutation, no write lock — mirrors the structure of
    /// the read-only catalog/describe RPCs. The predicate is the same
    /// `has_open_begin` function used by the complete/snapshot soft-warn (BP2),
    /// guaranteeing the query and the warning can never disagree (PD-3).
    async fn begin_adoption_status(
        &self,
        request: Request<BeginAdoptionStatusRequest>,
    ) -> Result<Response<BeginAdoptionStatusResponse>, Status> {
        // Gatekeeper FIRST (spec Req 5): refuse unauth callers before any
        // hearth resolution. Standalone ⇒ Ok(None), no-op.
        let _session = self.authorize(&request).await?;
        // Offload the synchronous activity-log + transition fold off the async
        // worker (block_in_place — borrows &self). One synchronous segment (no
        // `.await` after authorize). See `route`.
        tokio::task::block_in_place(move || {
        let req = request.into_inner();
        let resolved = resolve_hearth(
            &req.hearth_path,
            self.hearth_path.as_deref(),
            &self.hearth_policy,
        )?;

        // Pure read: no write lock acquired (read RPCs never contend the hearth
        // lock — only write RPCs do; see catalog/describe for the same pattern).
        let query_adapter = FileSystemQueryAdapter::new(resolved.clone());

        // Read the log WITH its degradation diagnostics so a dropped begin
        // marker is observable (the fs adapter also emits a path-aware boundary
        // warning). This is an informational read RPC — the honest surviving-
        // entry verdict is returned as-is; the hard, conservative enforcement
        // (which fails OPEN on a degraded log to avoid nudging a duplicate
        // begin) lives daemon-free in
        // `gate_check::resolve_begin_status_from_disk`.
        let activity_log = query_adapter
            .read_activity_log(&req.artifact_path)
            .map_err(|e| Status::internal(format!("read_activity_log failed: {}", e)))?;

        // Fold the per-file transition event store (merged with any legacy
        // array) so the open-begin comparison sees event-sourced history.
        let transitions = query_adapter
            .read_transitions(&req.artifact_path)
            .map_err(|e| Status::internal(format!("read_transitions failed: {}", e)))?;
        let result = has_open_begin(
            &activity_log.entries,
            &transitions,
            &req.actor_name,
            &req.state,
        );

        tracing::info!(
            command = "begin_adoption_status",
            actor = %req.actor_name,
            artifact = %req.artifact_path,
            state = %req.state,
            hearth = %resolved.display(),
            has_open_begin = result,
            activity_dropped = activity_log.dropped,
            outcome = "ok",
            "begin_adoption_status query ok"
        );

        Ok(Response::new(BeginAdoptionStatusResponse {
            has_open_begin: result,
            resolved_hearth: resolved.display().to_string(),
        }))
        })
    }

    async fn playbook_activity(
        &self,
        request: Request<PlaybookActivityRequest>,
    ) -> Result<Response<PlaybookActivityResponse>, Status> {
        // Gatekeeper FIRST (spec Req 5): refuse unauth callers before any
        // hearth resolution. Standalone ⇒ Ok(None), no-op.
        let _session = self.authorize(&request).await?;
        let req = request.into_inner();
        // Both the gRPC handler and the HTTP `/ws` bridge fold the SAME core
        // query path — there is one owners-from-status + routing-activity-sink
        // fold (`compute_playbook_activity`), so the two surfaces can never
        // diverge. Offloaded onto tokio's blocking pool via `spawn_blocking` so a
        // heavy/concurrent whole-hearth+activity-log fold NEVER occupies an async
        // worker and starves Foundry's `/health` probe (shared runtime + listener).
        let hearth_path = req.hearth_path;
        let all_hearths = req.all_hearths;
        let default_hearth = self.hearth_path.clone();
        let policy = self.hearth_policy.clone();
        let global = self.global_playbooks_hearth.clone();
        let response = tokio::task::spawn_blocking(move || {
            compute_playbook_activity(
                &hearth_path,
                all_hearths,
                default_hearth.as_deref(),
                &policy,
                global.as_deref(),
            )
        })
        .await
        .map_err(|e| Status::internal(format!("playbook_activity fold task failed: {e}")))??;
        Ok(Response::new(response))
    }

    async fn actor_activity(
        &self,
        request: Request<ActorActivityRequest>,
    ) -> Result<Response<ActorActivityResponse>, Status> {
        // Gatekeeper FIRST (mirror playbook_activity): refuse unauth callers
        // before any hearth resolution. Standalone ⇒ Ok(None), no-op.
        let _session = self.authorize(&request).await?;
        let req = request.into_inner();
        // LOCAL-ONLY query (raw actor names). The /ws bridge folds the SAME core
        // path (`compute_actor_activity`), so loopback dashboard and local gRPC
        // can never diverge. Offloaded via `spawn_blocking` — see playbook_activity
        // for why the fold must never run inline on an async worker.
        let hearth_path = req.hearth_path;
        let all_hearths = req.all_hearths;
        let default_hearth = self.hearth_path.clone();
        let policy = self.hearth_policy.clone();
        let response = tokio::task::spawn_blocking(move || {
            compute_actor_activity(&hearth_path, all_hearths, default_hearth.as_deref(), &policy)
        })
        .await
        .map_err(|e| Status::internal(format!("actor_activity fold task failed: {e}")))??;
        Ok(Response::new(response))
    }

    async fn usage_time_series(
        &self,
        request: Request<UsageTimeSeriesRequest>,
    ) -> Result<Response<UsageTimeSeriesResponse>, Status> {
        // Gatekeeper FIRST (mirror playbook_activity): refuse unauth callers
        // before any hearth resolution. Standalone ⇒ Ok(None), no-op.
        let _session = self.authorize(&request).await?;
        let req = request.into_inner();
        // Offloaded via `spawn_blocking` — see playbook_activity for why the fold
        // must never run inline on an async worker.
        let hearth_path = req.hearth_path;
        let granularity = req.granularity;
        let all_hearths = req.all_hearths;
        let default_hearth = self.hearth_path.clone();
        let policy = self.hearth_policy.clone();
        let global = self.global_playbooks_hearth.clone();
        let response = tokio::task::spawn_blocking(move || {
            compute_usage_timeseries(
                &hearth_path,
                &granularity,
                all_hearths,
                default_hearth.as_deref(),
                &policy,
                global.as_deref(),
            )
        })
        .await
        .map_err(|e| Status::internal(format!("usage_time_series fold task failed: {e}")))??;
        Ok(Response::new(response))
    }

    async fn playbook_step_volume(
        &self,
        request: Request<PlaybookStepVolumeRequest>,
    ) -> Result<Response<PlaybookStepVolumeResponse>, Status> {
        let _session = self.authorize(&request).await?;
        let req = request.into_inner();
        // Offloaded via `spawn_blocking` — see playbook_activity for why the fold
        // must never run inline on an async worker.
        let hearth_path = req.hearth_path;
        let kind = req.kind;
        let all_hearths = req.all_hearths;
        let default_hearth = self.hearth_path.clone();
        let policy = self.hearth_policy.clone();
        let global = self.global_playbooks_hearth.clone();
        let response = tokio::task::spawn_blocking(move || {
            compute_playbook_step_volume(
                &hearth_path,
                &kind,
                all_hearths,
                default_hearth.as_deref(),
                &policy,
                global.as_deref(),
            )
        })
        .await
        .map_err(|e| Status::internal(format!("playbook_step_volume fold task failed: {e}")))??;
        Ok(Response::new(response))
    }

    async fn hook_manifest(
        &self,
        request: Request<HookManifestRequest>,
    ) -> Result<Response<HookManifestResponse>, Status> {
        // Gatekeeper FIRST (mirror playbook_activity): refuse unauth callers
        // before any hearth resolution. Standalone ⇒ Ok(None), no-op.
        let _session = self.authorize(&request).await?;
        let req = request.into_inner();
        // Offloaded via `spawn_blocking` — see playbook_activity for why the fold
        // must never run inline on an async worker.
        let hearth_path = req.hearth_path;
        let default_hearth = self.hearth_path.clone();
        let policy = self.hearth_policy.clone();
        let global = self.global_playbooks_hearth.clone();
        let response = tokio::task::spawn_blocking(move || {
            compute_hook_manifest(&hearth_path, default_hearth.as_deref(), &policy, global.as_deref())
        })
        .await
        .map_err(|e| Status::internal(format!("hook_manifest fold task failed: {e}")))??;
        Ok(Response::new(response))
    }

    async fn activity_summary(
        &self,
        request: Request<ActivitySummaryRequest>,
    ) -> Result<Response<ActivitySummaryResponse>, Status> {
        // Gatekeeper FIRST (mirror playbook_activity): refuse unauth callers
        // before any hearth resolution. Standalone ⇒ Ok(None), no-op. This read
        // query is NOT itself logged (logging the dashboard's own reads would
        // pollute the measurement).
        let _session = self.authorize(&request).await?;
        let req = request.into_inner();
        // Offloaded via `spawn_blocking` — see playbook_activity for why the fold
        // must never run inline on an async worker.
        let hearth_path = req.hearth_path;
        let granularity = req.granularity;
        let all_hearths = req.all_hearths;
        let default_hearth = self.hearth_path.clone();
        let policy = self.hearth_policy.clone();
        let response = tokio::task::spawn_blocking(move || {
            compute_activity_summary(
                &hearth_path,
                &granularity,
                all_hearths,
                default_hearth.as_deref(),
                &policy,
            )
        })
        .await
        .map_err(|e| Status::internal(format!("activity_summary fold task failed: {e}")))??;
        Ok(Response::new(response))
    }

    async fn playbook_fidelity(
        &self,
        request: Request<PlaybookFidelityRequest>,
    ) -> Result<Response<PlaybookFidelityResponse>, Status> {
        // Gatekeeper FIRST (mirror activity_summary): refuse unauth callers before
        // any hearth resolution. Standalone ⇒ Ok(None), no-op. This read query is
        // NOT itself logged (logging the dashboard's own reads would pollute the
        // measurement).
        let _session = self.authorize(&request).await?;
        let req = request.into_inner();
        // Offloaded via `spawn_blocking` — see playbook_activity for why the fold
        // must never run inline on an async worker.
        let hearth_path = req.hearth_path;
        let all_hearths = req.all_hearths;
        let default_hearth = self.hearth_path.clone();
        let policy = self.hearth_policy.clone();
        let global = self.global_playbooks_hearth.clone();
        let response = tokio::task::spawn_blocking(move || {
            compute_playbook_fidelity(
                &hearth_path,
                all_hearths,
                default_hearth.as_deref(),
                &policy,
                global.as_deref(),
            )
        })
        .await
        .map_err(|e| Status::internal(format!("playbook_fidelity fold task failed: {e}")))??;
        Ok(Response::new(response))
    }

    async fn join_coverage(
        &self,
        request: Request<anvil_engine::proto::JoinCoverageRequest>,
    ) -> Result<Response<anvil_engine::proto::JoinCoverageResponse>, Status> {
        // Gatekeeper FIRST (mirror playbook_fidelity): refuse unauth callers
        // before any hearth resolution. This read query is NOT itself logged.
        let _session = self.authorize(&request).await?;
        let req = request.into_inner();
        // Offloaded via `spawn_blocking` — see playbook_activity for why the
        // fold must never run inline on an async worker. This one reads a 46 MB
        // sink line by line, so it is the least optional of them.
        let default_hearth = self.hearth_path.clone();
        let policy = self.hearth_policy.clone();
        let global = self.global_playbooks_hearth.clone();
        let response = tokio::task::spawn_blocking(move || {
            compute_join_coverage(&req, default_hearth.as_deref(), &policy, global.as_deref())
        })
        .await
        .map_err(|e| Status::internal(format!("join_coverage fold task failed: {e}")))??;
        Ok(Response::new(response))
    }

    async fn playbook_atlas(
        &self,
        request: Request<anvil_engine::proto::PlaybookAtlasRequest>,
    ) -> Result<Response<anvil_engine::proto::PlaybookAtlasResponse>, Status> {
        // Gatekeeper FIRST (mirror hook_manifest): refuse unauth callers before
        // any hearth resolution. Standalone ⇒ Ok(None), no-op.
        let _session = self.authorize(&request).await?;
        let req = request.into_inner();
        // Offloaded via `spawn_blocking` — see playbook_activity for why the fold
        // must never run inline on an async worker.
        let hearth_path = req.hearth_path;
        let default_hearth = self.hearth_path.clone();
        let policy = self.hearth_policy.clone();
        let response = tokio::task::spawn_blocking(move || {
            compute_playbook_atlas(&hearth_path, default_hearth.as_deref(), &policy)
        })
        .await
        .map_err(|e| Status::internal(format!("playbook_atlas fold task failed: {e}")))??;
        Ok(Response::new(response))
    }

    async fn playbook_atlas_detail(
        &self,
        request: Request<anvil_engine::proto::PlaybookAtlasDetailRequest>,
    ) -> Result<Response<anvil_engine::proto::PlaybookAtlasDetailResponse>, Status> {
        let _session = self.authorize(&request).await?;
        let req = request.into_inner();
        // Offloaded via `spawn_blocking` — see playbook_activity for why the fold
        // must never run inline on an async worker.
        let hearth_path = req.hearth_path;
        let kind = req.kind;
        let default_hearth = self.hearth_path.clone();
        let policy = self.hearth_policy.clone();
        let response = tokio::task::spawn_blocking(move || {
            compute_playbook_atlas_detail(&hearth_path, &kind, default_hearth.as_deref(), &policy)
        })
        .await
        .map_err(|e| Status::internal(format!("playbook_atlas_detail fold task failed: {e}")))??;
        Ok(Response::new(response))
    }

    async fn live_instances(
        &self,
        request: Request<anvil_engine::proto::LiveInstancesRequest>,
    ) -> Result<Response<anvil_engine::proto::LiveInstancesResponse>, Status> {
        // Gatekeeper FIRST (mirror actor_activity): refuse unauth callers before
        // any hearth resolution. Standalone ⇒ Ok(None), no-op.
        let _session = self.authorize(&request).await?;
        let req = request.into_inner();
        // LOCAL-ONLY query (raw actor names from begin-markers). The /ws bridge
        // folds the SAME core path (`compute_live_instances`), so loopback
        // dashboard and local gRPC can never diverge. Offloaded via `spawn_blocking`
        // — see playbook_activity for why the fold must never run inline.
        let hearth_path = req.hearth_path;
        let all_hearths = req.all_hearths;
        let default_hearth = self.hearth_path.clone();
        let policy = self.hearth_policy.clone();
        let global = self.global_playbooks_hearth.clone();
        let response = tokio::task::spawn_blocking(move || {
            compute_live_instances(
                &hearth_path,
                all_hearths,
                default_hearth.as_deref(),
                &policy,
                global.as_deref(),
            )
        })
        .await
        .map_err(|e| Status::internal(format!("live_instances fold task failed: {e}")))??;
        Ok(Response::new(response))
    }

    async fn list_instance_artifacts(
        &self,
        request: Request<anvil_engine::proto::ListInstanceArtifactsRequest>,
    ) -> Result<Response<anvil_engine::proto::ListInstanceArtifactsResponse>, Status> {
        // Gatekeeper FIRST (mirror live_instances): refuse unauth callers before
        // any path resolution. Standalone ⇒ Ok(None), no-op.
        let _session = self.authorize(&request).await?;
        let req = request.into_inner();
        let instance_dir = req.instance_dir;
        let policy = self.hearth_policy.clone();
        let response = tokio::task::spawn_blocking(move || {
            compute_list_instance_artifacts(&instance_dir, &policy)
        })
        .await
        .map_err(|e| Status::internal(format!("list_instance_artifacts fold task failed: {e}")))??;
        Ok(Response::new(response))
    }

    async fn read_instance_artifact(
        &self,
        request: Request<anvil_engine::proto::ReadInstanceArtifactRequest>,
    ) -> Result<Response<anvil_engine::proto::ReadInstanceArtifactResponse>, Status> {
        // Gatekeeper FIRST (mirror list_instance_artifacts): refuse unauth
        // callers before any path resolution. Standalone ⇒ Ok(None), no-op.
        let _session = self.authorize(&request).await?;
        let req = request.into_inner();
        let instance_dir = req.instance_dir;
        let name = req.name;
        let policy = self.hearth_policy.clone();
        let response = tokio::task::spawn_blocking(move || {
            compute_read_instance_artifact(&instance_dir, &name, &policy)
        })
        .await
        .map_err(|e| Status::internal(format!("read_instance_artifact fold task failed: {e}")))??;
        Ok(Response::new(response))
    }

    async fn run_detail(
        &self,
        request: Request<anvil_engine::proto::RunDetailRequest>,
    ) -> Result<Response<anvil_engine::proto::RunDetailResponse>, Status> {
        // Gatekeeper FIRST (mirror live_instances): refuse unauth callers before
        // any hearth resolution. Standalone ⇒ Ok(None), no-op.
        let _session = self.authorize(&request).await?;
        let req = request.into_inner();
        // The /ws bridge folds the SAME `compute_run_detail`, so the depth panel
        // and this RPC can never disagree about a run. Offloaded via
        // `spawn_blocking` — see playbook_activity for why a whole-hearth fold
        // must never run inline on an async worker.
        let hearth_path = req.hearth_path;
        let instance_id = req.instance_id;
        let default_hearth = self.hearth_path.clone();
        let policy = self.hearth_policy.clone();
        let response = tokio::task::spawn_blocking(move || {
            compute_run_detail(
                &hearth_path,
                &instance_id,
                default_hearth.as_deref(),
                &policy,
            )
        })
        .await
        .map_err(|e| Status::internal(format!("run_detail fold task failed: {e}")))??;
        Ok(Response::new(response))
    }

    // ── K8 backlog_item typed RPCs (plan Task 9) ────────────────────────────
    //
    // Each handler authorizes, resolves the request hearth, holds that hearth's
    // lock across strict recovery + read + write, resolves STRICT hearth-local
    // K8 policy per request (never installed process-wide), constructs real
    // filesystem adapters, invokes the core operation, and maps typed errors to
    // a non-OK status. The engine — never the caller — stamps audit data,
    // history sequence, actor role and every id.

    async fn backlog_mutate(
        &self,
        request: Request<anvil_engine::proto::BacklogMutateRequest>,
    ) -> Result<Response<anvil_engine::proto::BacklogMutateResponse>, Status> {
        let session = self.authorize(&request).await?;
        let req = request.into_inner();
        let resolved = resolve_hearth(
            &req.hearth_path,
            self.hearth_path.as_deref(),
            &self.hearth_policy,
        )?;
        let _guard = self.hearth_locks.lock_for(&resolved).await;
        let actor_name = Self::principal_actor_name(&session, req.actor_name.clone());
        let policy = backlog_policy_for(&resolved)?;
        let role = backlog_role(&req.actor_role)?;
        let mutation = backlog_mutation_from_request(&req)?;
        let actor = backlog_actor_identity(
            &actor_name,
            &req.actor_type,
            &req.actor_model,
            &req.actor_provider,
            req.actor_context_window,
            &req.actor_sdk_version,
            &req.actor_entrypoint,
        )?;
        let at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let store = FileSystemBacklogItemAdapter::new(resolved.clone());
        let outcome = anvil_core::domain::backlog_item::execute_backlog_mutation(
            &store,
            &mutation,
            &policy,
            role,
            &actor,
            &at,
            &format!(
                "{}-{}",
                mutation.operation(),
                anvil_core_hearth::fs_transition_event_adapter::short_random_id()
            ),
        )
        .map_err(|e| Status::failed_precondition(e.to_string()))?;
        Ok(Response::new(anvil_engine::proto::BacklogMutateResponse {
            success: true,
            operation: outcome.operation.to_string(),
            affected_item_ids: outcome.affected,
            proposal_id: outcome.proposal_id.unwrap_or_default(),
            resolved_hearth: resolved.display().to_string(),
            unranked_item_ids: outcome.unranked,
        }))
    }

    async fn backlog_evaluate(
        &self,
        request: Request<anvil_engine::proto::BacklogEvaluateRequest>,
    ) -> Result<Response<anvil_engine::proto::BacklogEvaluateResponse>, Status> {
        let session = self.authorize(&request).await?;
        let req = request.into_inner();
        let resolved = resolve_hearth(
            &req.hearth_path,
            self.hearth_path.as_deref(),
            &self.hearth_policy,
        )?;
        let _guard = self.hearth_locks.lock_for(&resolved).await;
        let actor_name = Self::principal_actor_name(&session, req.actor_name.clone());
        let policy = backlog_policy_for(&resolved)?;
        let actor = backlog_actor_identity(
            &actor_name,
            &req.actor_type,
            &req.actor_model,
            &req.actor_provider,
            req.actor_context_window,
            &req.actor_sdk_version,
            &req.actor_entrypoint,
        )?;
        let at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let organ = req.business_node_id.trim();
        let store = FileSystemBacklogItemAdapter::new(resolved.clone());
        // Evaluate accepts NO caller role: the service constructs the private
        // Evaluation origin and stamps engine_auto itself.
        let outcome = anvil_core::domain::backlog_item::evaluate_backlog(
            &store,
            if organ.is_empty() { None } else { Some(organ) },
            &policy,
            &actor,
            &at,
            &format!(
                "evaluate-{}",
                anvil_core_hearth::fs_transition_event_adapter::short_random_id()
            ),
            &anvil_core_hearth::fs_transition_event_adapter::hi_res_prefix(),
            &anvil_core_hearth::fs_transition_event_adapter::short_random_id(),
        )
        .map_err(|e| Status::failed_precondition(e.to_string()))?;
        Ok(Response::new(anvil_engine::proto::BacklogEvaluateResponse {
            success: true,
            recomputed_item_ids: outcome.recomputed,
            woken_item_ids: outcome.woken,
            aged_out_item_ids: outcome.aged_out,
            vetoed_item_ids: outcome.vetoed,
            resolved_hearth: resolved.display().to_string(),
            unranked_item_ids: outcome.unranked,
        }))
    }

    async fn backlog_queue(
        &self,
        request: Request<anvil_engine::proto::BacklogQueueRequest>,
    ) -> Result<Response<anvil_engine::proto::BacklogQueueResponse>, Status> {
        let _session = self.authorize(&request).await?;
        let req = request.into_inner();
        let resolved = resolve_hearth(
            &req.hearth_path,
            self.hearth_path.as_deref(),
            &self.hearth_policy,
        )?;
        // A queue read still takes the hearth lock: the strict store recovers
        // interrupted K8 transactions before it reads.
        let _guard = self.hearth_locks.lock_for(&resolved).await;
        let store = FileSystemBacklogItemAdapter::new(resolved.clone());
        use anvil_engine::proto::backlog_queue_request::Scope;
        let view = match req.scope {
            Some(Scope::BusinessNodeId(ref organ)) => {
                anvil_core::domain::backlog_item::read_organ_queue(&store, organ)
            }
            Some(Scope::CrossOrgan(_)) => {
                anvil_core::domain::backlog_item::read_cross_organ_view(&store)
            }
            None => {
                return Err(Status::invalid_argument(
                    "BacklogQueue requires exactly one scope: business_node_id or cross_organ",
                ))
            }
        }
        .map_err(|e| Status::failed_precondition(e.to_string()))?;
        Ok(Response::new(anvil_engine::proto::BacklogQueueResponse {
            items: view
                .ranked
                .iter()
                .map(|r| anvil_engine::proto::BacklogQueueItem {
                    backlog_item_id: r.backlog_item_id.clone(),
                    business_node_id: r.business_node_id.clone(),
                    state: r.state.as_str().to_string(),
                    position: r.position,
                    title: r.title.clone(),
                    explanation: r.explanation.clone(),
                    route_to_intake: r.route_to_intake,
                })
                .collect(),
            resolved_hearth: resolved.display().to_string(),
            unranked_items: view
                .unranked_candidates
                .iter()
                .map(|u| anvil_engine::proto::BacklogUnrankedQueueItem {
                    backlog_item_id: u.backlog_item_id.clone(),
                    business_node_id: u.business_node_id.clone(),
                    state: u.state.as_str().to_string(),
                    title: u.title.clone(),
                    route_to_intake: u.route_to_intake,
                    reason: u.reason.clone(),
                })
                .collect(),
        }))
    }
}

// ---------------------------------------------------------------------------
// K8 wire mapping (plan Task 9). Every token is domain-validated here; nothing
// downstream re-parses a caller string.
// ---------------------------------------------------------------------------

/// Resolve STRICT hearth-local K8 policy for ONE request. A present malformed
/// value is non-OK and never a silent default, and one hearth's values never
/// leak into another.
fn backlog_policy_for(
    resolved: &Path,
) -> Result<anvil_core::domain::backlog_item::BacklogPolicy, Status> {
    anvil_engine::engine_flags::resolve_backlog_policy_for_hearth(resolved, |k| {
        std::env::var(k).ok()
    })
        .map_err(|e| Status::invalid_argument(e.to_string()))
}

fn backlog_role(raw: &str) -> Result<anvil_core::domain::backlog_item::DriverRole, Status> {
    use anvil_core::domain::backlog_item::DriverRole;
    Ok(match raw.trim() {
        "nick_shape" => DriverRole::NickShape,
        "organ_loop" => DriverRole::OrganLoop,
        "orchestrator" => DriverRole::Orchestrator,
        "track_driver" => DriverRole::TrackDriver,
        // `engine_auto` and `intake` are ENGINE authorities: no mutation
        // request may select either.
        other => {
            return Err(Status::invalid_argument(format!(
                "`{other}` is not a caller-selectable K8 driver role"
            )))
        }
    })
}

fn backlog_actor_identity(
    name: &str,
    actor_type: &str,
    model: &str,
    provider: &str,
    context_window: i64,
    sdk_version: &str,
    entrypoint: &str,
) -> Result<anvil_core::domain::shared_types::ActorIdentity, Status> {
    for (field, value) in [
        ("actor_name", name),
        ("actor_type", actor_type),
        ("actor_model", model),
        ("actor_provider", provider),
    ] {
        if value.trim().is_empty() {
            return Err(Status::invalid_argument(format!("{field} is required")));
        }
    }
    if context_window < 0 {
        return Err(Status::invalid_argument(
            "actor_context_window is absent at zero and otherwise positive",
        ));
    }
    Ok(anvil_core::domain::shared_types::ActorIdentity {
        name: name.to_string(),
        actor_type: actor_type.to_string(),
        model: model.to_string(),
        provider: provider.to_string(),
        context_window,
        sdk_version: sdk_version.to_string(),
        entrypoint: entrypoint.to_string(),
        registered_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    })
}

fn backlog_closed<T: serde::de::DeserializeOwned>(field: &str, token: &str) -> Result<T, Status> {
    serde_yaml::from_str::<T>(token).map_err(|e| {
        Status::invalid_argument(format!("`{token}` is not a closed {field} token: {e}"))
    })
}

fn backlog_evidence(
    reference: &Option<anvil_engine::proto::BacklogEvidenceRef>,
    field: &str,
) -> Result<anvil_core::domain::backlog_item::EvidenceRef, Status> {
    let r = reference
        .as_ref()
        .ok_or_else(|| Status::invalid_argument(format!("{field} is required")))?;
    Ok(anvil_core::domain::backlog_item::EvidenceRef {
        kind: backlog_closed("evidence kind", &r.kind)?,
        id: r.id.clone(),
    })
}

fn backlog_mutation_from_request(
    req: &anvil_engine::proto::BacklogMutateRequest,
) -> Result<anvil_core::domain::backlog_item::BacklogMutation, Status> {
    use anvil_core::domain::backlog_item as k8;
    use anvil_engine::proto::backlog_mutate_request::Operation;
    let op = req
        .operation
        .as_ref()
        .ok_or_else(|| Status::invalid_argument("exactly one BacklogMutate operation is required"))?;
    Ok(match op {
        Operation::ShapeEdit(edit) => {
            let body = backlog_shape_body(edit)?;
            k8::BacklogMutation::ShapeEdit {
                bi_id: edit.backlog_item_id.clone(),
                body,
            }
        }
        Operation::RecomputeRank(r) => k8::BacklogMutation::RecomputeRank {
            business_node_id: r.business_node_id.clone(),
        },
        Operation::StampExecutionBinding(s) => {
            let eb = s.execution_binding.as_ref().ok_or_else(|| {
                Status::invalid_argument("stamp_execution_binding requires execution_binding")
            })?;
            let ob = s.outcome_binding.as_ref().ok_or_else(|| {
                Status::invalid_argument("stamp_execution_binding requires outcome_binding")
            })?;
            use anvil_engine::proto::backlog_outcome_binding_decl::SuccessMeasure;
            let success_measure_id = match ob.success_measure.as_ref() {
                Some(SuccessMeasure::SuccessMeasureId(id)) => Some(id.clone()),
                Some(SuccessMeasure::NoSuccessMeasureId(_)) => None,
                None => {
                    return Err(Status::invalid_argument(
                        "outcome_binding must select exactly one success-measure branch",
                    ))
                }
            };
            k8::BacklogMutation::StampExecutionBinding {
                bi_id: s.backlog_item_id.clone(),
                execution_binding: k8::ExecutionBinding {
                    track_id: eb.track_id.clone(),
                    playbook_definition_id: eb.playbook_definition_id.clone(),
                    playbook_run_id: eb.playbook_run_id.clone(),
                    run_id: eb.run_id.clone(),
                },
                outcome_binding: k8::OutcomeBindingDecl {
                    success_measure_id,
                    tree_node: ob.tree_node.clone(),
                    reading_status: backlog_closed("reading status", &ob.reading_status)?,
                },
            }
        }
        Operation::RecordOutcomeSignoff(s) => k8::BacklogMutation::RecordOutcomeSignoff {
            bi_id: s.backlog_item_id.clone(),
            approver: s.approver.clone(),
        },
        Operation::ProposeReshuffle(p) => {
            if p.proposed.is_empty() {
                return Err(Status::invalid_argument(
                    "propose_reshuffle requires a nonempty proposal",
                ));
            }
            k8::BacklogMutation::ProposeReshuffle {
                proposed: p
                    .proposed
                    .iter()
                    .map(|e| k8::ProposedPosition {
                        backlog_item_id: e.backlog_item_id.clone(),
                        position: e.position,
                    })
                    .collect(),
            }
        }
        Operation::CommitReshuffle(c) => k8::BacklogMutation::CommitReshuffle {
            proposal_id: c.proposal_id.clone(),
            approver: c.approver.clone(),
        },
        Operation::RejectReshuffle(r) => k8::BacklogMutation::RejectReshuffle {
            proposal_id: r.proposal_id.clone(),
            approver: r.approver.clone(),
        },
        Operation::VetoAgeOut(v) => k8::BacklogMutation::VetoAgeOut {
            bi_id: v.backlog_item_id.clone(),
            approver: v.approver.clone(),
        },
        // The engine strict-loads the unique unresolved veto and writes its
        // sequence itself; no caller supplies `lifts_seq`.
        Operation::LiftAgeOutVeto(l) => k8::BacklogMutation::LiftAgeOutVeto {
            bi_id: l.backlog_item_id.clone(),
            approver: l.approver.clone(),
        },
    })
}

fn backlog_shape_body(
    edit: &anvil_engine::proto::BacklogShapeEdit,
) -> Result<anvil_core::domain::backlog_item::ShapeEditBody, Status> {
    use anvil_core::domain::backlog_item as k8;
    use anvil_engine::proto::backlog_shape_edit as se;
    let mut body = k8::ShapeEditBody::default();
    if let Some(se::ActionClassEdit::SetActionClass(v)) = edit.action_class_edit.as_ref() {
        body.action_class = Some(backlog_closed("action class", v)?);
    }
    match edit.description_edit.as_ref() {
        Some(se::DescriptionEdit::SetDescription(v)) => {
            body.description = Some(k8::FieldEdit::Set(v.clone()))
        }
        Some(se::DescriptionEdit::ClearDescription(_)) => {
            body.description = Some(k8::FieldEdit::Clear)
        }
        None => {}
    }
    match edit.effort_class_edit.as_ref() {
        Some(se::EffortClassEdit::SetEffortClass(v)) => {
            body.effort_class = Some(k8::FieldEdit::Set(backlog_closed("effort class", v)?))
        }
        Some(se::EffortClassEdit::ClearEffortClass(_)) => {
            body.effort_class = Some(k8::FieldEdit::Clear)
        }
        None => {}
    }
    match edit.playbook_binding_edit.as_ref() {
        Some(se::PlaybookBindingEdit::SetPlaybookBinding(pb)) => {
            use anvil_engine::proto::backlog_playbook_binding::Definition;
            let playbook_definition_id = match pb.definition.as_ref() {
                Some(Definition::PlaybookDefinitionId(id)) => Some(id.clone()),
                Some(Definition::NoPlaybookDefinition(_)) => None,
                None => {
                    return Err(Status::invalid_argument(
                        "playbook_binding must select exactly one definition branch",
                    ))
                }
            };
            body.playbook_binding = Some(k8::FieldEdit::Set(k8::PlaybookBinding {
                playbook_definition_id,
                route_to_intake: pb.route_to_intake,
            }));
        }
        Some(se::PlaybookBindingEdit::ClearPlaybookBinding(_)) => {
            body.playbook_binding = Some(k8::FieldEdit::Clear)
        }
        None => {}
    }
    if let Some(inputs) = edit.rank_inputs.as_ref() {
        use anvil_engine::proto::backlog_rank_inputs_edit as ri;
        let mut touched = false;
        match inputs.value_gap_magnitude_edit.as_ref() {
            Some(ri::ValueGapMagnitudeEdit::SetValueGapMagnitude(v)) => {
                body.value_gap_magnitude = Some(k8::FieldEdit::Set(k8::ValueGapMagnitude {
                    r#ref: backlog_evidence(&v.reference, "value_gap_magnitude.ref")?,
                    magnitude: v.magnitude,
                }));
                touched = true;
            }
            Some(ri::ValueGapMagnitudeEdit::ClearValueGapMagnitude(_)) => {
                body.value_gap_magnitude = Some(k8::FieldEdit::Clear);
                touched = true;
            }
            None => {}
        }
        match inputs.nick_weight_edit.as_ref() {
            Some(ri::NickWeightEdit::SetNickWeight(v)) => {
                body.nick_weight = Some(k8::FieldEdit::Set(*v));
                touched = true;
            }
            Some(ri::NickWeightEdit::ClearNickWeight(_)) => {
                body.nick_weight = Some(k8::FieldEdit::Clear);
                touched = true;
            }
            None => {}
        }
        match inputs.dependency_readiness_edit.as_ref() {
            Some(ri::DependencyReadinessEdit::SetDependencyReadiness(v)) => {
                let mut blocker_refs = Vec::new();
                for r in &v.blocker_refs {
                    blocker_refs.push(k8::EvidenceRef {
                        kind: backlog_closed("evidence kind", &r.kind)?,
                        id: r.id.clone(),
                    });
                }
                body.dependency_readiness = Some(k8::FieldEdit::Set(k8::DependencyReadiness {
                    status: backlog_closed("dependency status", &v.status)?,
                    blocker_refs,
                }));
                touched = true;
            }
            Some(ri::DependencyReadinessEdit::ClearDependencyReadiness(_)) => {
                body.dependency_readiness = Some(k8::FieldEdit::Clear);
                touched = true;
            }
            None => {}
        }
        if !touched {
            return Err(Status::invalid_argument(
                "a present rank_inputs must carry at least one branch",
            ));
        }
    }
    match edit.wake_condition_edit.as_ref() {
        Some(se::WakeConditionEdit::SetWakeCondition(w)) => {
            let kind: k8::WakeKind = backlog_closed("wake kind", &w.kind)?;
            let reference = match (&w.reference, kind) {
                (None, k8::WakeKind::Manual) => None,
                (Some(_), k8::WakeKind::Manual) => {
                    return Err(Status::invalid_argument(
                        "wake_condition.ref must be absent for kind=manual",
                    ))
                }
                _ => Some(backlog_evidence(&w.reference, "wake_condition.ref")?),
            };
            body.wake_condition = Some(k8::FieldEdit::Set(k8::WakeCondition {
                kind,
                r#ref: reference,
                predicate: w.predicate.clone(),
            }));
        }
        Some(se::WakeConditionEdit::ClearWakeCondition(_)) => {
            body.wake_condition = Some(k8::FieldEdit::Clear)
        }
        None => {}
    }
    match edit.superseded_by_edit.as_ref() {
        Some(se::SupersededByEdit::SetSupersededBy(v)) => {
            body.superseded_by = Some(k8::FieldEdit::Set(v.clone()))
        }
        Some(se::SupersededByEdit::ClearSupersededBy(_)) => {
            body.superseded_by = Some(k8::FieldEdit::Clear)
        }
        None => {}
    }
    Ok(body)
}


/// The shared playbook-activity read used by BOTH the gRPC `playbook_activity`
/// RPC and the HTTP `/ws` JSON-RPC `playbook_activity` method. Holding the fold
/// in one place guarantees the two surfaces return byte-identical data.
///
/// Pure read: no write lock acquired (read RPCs never contend the hearth lock —
/// only write RPCs do; see catalog/describe for the same pattern). Per-request
/// construction IS the always-reload mechanism (mirror route/describe): a fresh
/// registry per call means machine.yaml edits live-reload without restart.
/// Compute the per-hearth `ArtifactActivityResult` for one resolved hearth:
/// build the per-call registry, resolve owners from `contributed_by`, and fold
/// the durable routing-activity sink into per-kind call counts.
fn playbook_activity_result_for_hearth(
    hearth: &Path,
    global_playbooks_hearth: Option<&Path>,
) -> Result<anvil_core::domain::artifact_activity::ArtifactActivityResult, Status> {
    let registry = request_session_registry(hearth, global_playbooks_hearth);
    // Owner derives from each playbook's status.yaml `contributed_by`
    // (injected at kit-install), resolved via the registry's playbook_id.
    // Absent → the domain query applies the "anvil" default.
    let owners = FileSystemOwnerResolver::new(hearth, registry.as_ref());
    // Call counts fold the UNIVERSAL ACTIVITY LOG (counts per resolved
    // artifact_kind — the SAME fold `activity_summary.by_artifact_kind` uses), so the
    // owner roll-up reconciles with the universal usage view. (Previously this
    // folded the routing-activity sink, which only logs `route` turns and so
    // under-counted every playbook that was begun/advanced without re-routing.) A
    // missing log reads as zero counts (no error).
    let counts = anvil_core::domain::activity_summary::artifact_kind_counts(
        &read_hearth_activity_log(hearth)?,
    );
    Ok(ArtifactActivityQuery::execute_with_counts(
        registry.as_ref(),
        &owners,
        &counts,
    ))
}

/// Merge several per-hearth `ArtifactActivityResult`s into one, summing
/// call_counts per kind and merging owner groups by owner. A playbook's owner +
/// description come from the first hearth that carries the kind (deterministic:
/// hearths are folded in `all_hearths_to_fold` order). The merged result is
/// re-grouped by owner with entries ordered by kind, matching the single-hearth
/// shape.
fn merge_playbook_activity_results(
    results: Vec<anvil_core::domain::artifact_activity::ArtifactActivityResult>,
) -> anvil_core::domain::artifact_activity::ArtifactActivityResult {
    use anvil_core::domain::artifact_activity::{OwnerGroup, ArtifactActivityEntry};
    use std::collections::BTreeMap;

    // kind -> merged entry (owner/description first-seen, call_count summed).
    let mut by_kind: BTreeMap<String, ArtifactActivityEntry> = BTreeMap::new();
    for result in results {
        for group in result.groups {
            for entry in group.entries {
                by_kind
                    .entry(entry.kind.clone())
                    .and_modify(|merged| merged.call_count += entry.call_count)
                    .or_insert(entry);
            }
        }
    }

    let mut by_owner: BTreeMap<String, Vec<ArtifactActivityEntry>> = BTreeMap::new();
    for (_kind, entry) in by_kind {
        by_owner.entry(entry.owner.clone()).or_default().push(entry);
    }

    let groups = by_owner
        .into_iter()
        .map(|(owner, mut entries)| {
            entries.sort_by(|a, b| a.kind.cmp(&b.kind));
            OwnerGroup { owner, entries }
        })
        .collect();

    anvil_core::domain::artifact_activity::ArtifactActivityResult { groups }
}

fn compute_playbook_activity(
    req_hearth_path: &str,
    all_hearths: bool,
    default_hearth: Option<&Path>,
    hearth_policy: &HearthPolicy,
    global_playbooks_hearth: Option<&Path>,
) -> Result<PlaybookActivityResponse, Status> {
    let (result, resolved_hearth, hearths_included) = if all_hearths {
        // Cross-hearth: fold EVERY permitted hearth and merge — sum call counts,
        // merge owner groups by owner. Mirrors usage_time_series all_hearths.
        let hearths = all_hearths_to_fold(hearth_policy);
        let mut results = Vec::with_capacity(hearths.len());
        for hearth in &hearths {
            results.push(playbook_activity_result_for_hearth(
                hearth,
                global_playbooks_hearth,
            )?);
        }
        let merged = merge_playbook_activity_results(results);
        let included: Vec<String> = hearths.iter().map(|h| h.display().to_string()).collect();
        (merged, ALL_HEARTHS_SENTINEL.to_string(), included)
    } else {
        let resolved = match resolve_hearth(req_hearth_path, default_hearth, hearth_policy) {
            Ok(resolved) => resolved,
            Err(status) => {
                // CQRS-log the error outcome by status code name (never the message).
                tracing::warn!(
                    command = "playbook_activity",
                    outcome = "error",
                    error_code = ?status.code(),
                    "playbook_activity query failed"
                );
                return Err(status);
            }
        };
        let result = playbook_activity_result_for_hearth(&resolved, global_playbooks_hearth)?;
        let resolved_str = resolved.display().to_string();
        let included = vec![resolved_str.clone()];
        (result, resolved_str, included)
    };

    tracing::info!(
        command = "playbook_activity",
        hearth = %resolved_hearth,
        all_hearths = all_hearths,
        owner_count = result.groups.len(),
        outcome = "ok",
        "playbook_activity query ok"
    );

    let owner_groups = result
        .groups
        .into_iter()
        .map(|group| PlaybookOwnerGroup {
            owner: group.owner,
            entries: group
                .entries
                .into_iter()
                .map(|entry| anvil_engine::proto::PlaybookActivityEntry {
                    kind: entry.kind,
                    owner: entry.owner,
                    description: entry.description,
                    call_count: entry.call_count,
                })
                .collect(),
        })
        .collect();

    Ok(PlaybookActivityResponse {
        owners: owner_groups,
        resolved_hearth,
        hearths_included,
    })
}

/// The default open-begin-session gate query name. Echoed in the hook manifest
/// when the kit manifest declares no `hooks.gate_query` override. Pinned here as
/// the single source of truth so the contract's `gate_query` is never empty.
const DEFAULT_GATE_QUERY: &str = "begin_adoption_status";

/// The kit manifest filename the engine reads its hook GATE POLICY from. The
/// engine's cwd is `${KIT_ROOT}` (per the kit manifest's `app.engine.cwd`), so a
/// walk-up from cwd reaches the manifest at the kit root. Absent → safe defaults
/// (gate_query = `begin_adoption_status`, hard_enforce = []), which makes the
/// manifest a pure-policy overlay rather than a hard dependency.
const KIT_MANIFEST_FILENAME: &str = "foundry-manifest.json";

/// The harness-agnostic hook GATE POLICY: the gate-query name and the set of
/// playbook kinds whose hooks are a HARD pre-mutation gate (everything else is
/// SOFT-warn). Discovered from the kit manifest's `hooks` block; safe defaults
/// when absent.
struct HookGatePolicy {
    gate_query: String,
    hard_enforce: Vec<String>,
}

impl Default for HookGatePolicy {
    fn default() -> Self {
        HookGatePolicy {
            gate_query: DEFAULT_GATE_QUERY.to_string(),
            hard_enforce: Vec::new(),
        }
    }
}

/// Read the hook gate policy from the kit manifest's `hooks` block. Searches the
/// current working directory and its ancestors for `foundry-manifest.json`
/// (cwd = `${KIT_ROOT}` at runtime). A missing file, unreadable file, malformed
/// JSON, or absent `hooks` block all degrade to [`HookGatePolicy::default`] —
/// the policy is a non-fatal overlay (mirrors the empty-sink-reads-as-zero
/// posture of the other read queries). `gate_query` falls back to the default
/// when the manifest omits it or leaves it empty.
fn read_hook_gate_policy() -> HookGatePolicy {
    let Ok(cwd) = std::env::current_dir() else {
        return HookGatePolicy::default();
    };
    let mut dir: Option<&Path> = Some(cwd.as_path());
    while let Some(current) = dir {
        let candidate = current.join(KIT_MANIFEST_FILENAME);
        if candidate.is_file() {
            return parse_hook_gate_policy(&candidate).unwrap_or_default();
        }
        dir = current.parent();
    }
    HookGatePolicy::default()
}

/// Parse the `hooks` block from a kit manifest file into a [`HookGatePolicy`].
/// Returns `None` when the file can't be read or parsed; an absent `hooks` block
/// yields the default policy (Some(default)).
fn parse_hook_gate_policy(path: &Path) -> Option<HookGatePolicy> {
    let text = std::fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let hooks = match json.get("hooks") {
        Some(h) => h,
        None => return Some(HookGatePolicy::default()),
    };
    let gate_query = hooks
        .get("gate_query")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_GATE_QUERY)
        .to_string();
    let hard_enforce = hooks
        .get("hard_enforce")
        .and_then(serde_json::Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    Some(HookGatePolicy {
        gate_query,
        hard_enforce,
    })
}

/// The shared hook-manifest read used by BOTH the gRPC `HookManifest` RPC and the
/// HTTP `/ws` JSON-RPC `hook_manifest` method. Folds every registered playbook
/// machine's `hooks_by_role` with the kit manifest's `hard_enforce` policy into
/// the installable hook set. Each hook body is read via the SAME hook-body path
/// `begin` uses (`SourceAwarePlaybookHookBodyReader`), so the contract body can
/// never drift from what `begin` serves. Pure read: no write lock (mirror
/// playbook_activity). Per-request registry construction IS the always-reload
/// mechanism (mirror route/describe).
fn compute_hook_manifest(
    req_hearth_path: &str,
    default_hearth: Option<&Path>,
    hearth_policy: &HearthPolicy,
    global_playbooks_hearth: Option<&Path>,
) -> Result<HookManifestResponse, Status> {
    let resolved = match resolve_hearth(req_hearth_path, default_hearth, hearth_policy) {
        Ok(resolved) => resolved,
        Err(status) => {
            tracing::warn!(
                command = "hook_manifest",
                outcome = "error",
                error_code = ?status.code(),
                "hook_manifest query failed"
            );
            return Err(status);
        }
    };

    let registry = request_session_registry(&resolved, global_playbooks_hearth);
    let hook_reader = SourceAwarePlaybookHookBodyReader {
        request_hearth: resolved.clone(),
    };
    let policy = read_hook_gate_policy();

    let hooks = fold_hook_manifest(registry.as_ref(), &hook_reader, &policy.hard_enforce);

    tracing::info!(
        command = "hook_manifest",
        hearth = %resolved.display(),
        hook_count = hooks.len(),
        outcome = "ok",
        "hook_manifest query ok"
    );

    let proto_hooks = hooks
        .into_iter()
        .map(|h| anvil_engine::proto::ResolvedHook {
            artifact_kind: h.artifact_kind,
            state: h.state,
            role: h.role,
            body: h.body,
            gate: h.gate,
        })
        .collect();

    Ok(HookManifestResponse {
        resolved_hearth: resolved.display().to_string(),
        gate_query: policy.gate_query,
        hooks: proto_hooks,
    })
}

/// The shared playbook-atlas read used by BOTH the gRPC `PlaybookAtlas` RPC and
/// the HTTP `/ws` JSON-RPC `playbook_atlas` method. Folds the CONCRETE hearth
/// registry — not the composite `&dyn` registry — because the atlas needs
/// `invalid_artifacts()` (degenerate breakage entries) and the per-dir
/// `status.yaml` (owner_kit / lifecycle state), both unreachable through the
/// trait object (see `fold_playbook_atlas` doc + plan P2-finding-2). Per-request
/// construction IS the always-reload mechanism (mirror route/describe). Pure
/// read: no write lock.
fn compute_playbook_atlas(
    req_hearth_path: &str,
    default_hearth: Option<&Path>,
    hearth_policy: &HearthPolicy,
) -> Result<anvil_engine::proto::PlaybookAtlasResponse, Status> {
    use anvil_core::domain::playbook::atlas::fold_playbook_atlas;

    let resolved = match resolve_hearth(req_hearth_path, default_hearth, hearth_policy) {
        Ok(resolved) => resolved,
        Err(status) => {
            tracing::warn!(
                command = "playbook_atlas",
                outcome = "error",
                error_code = ?status.code(),
                "playbook_atlas query failed"
            );
            return Err(status);
        }
    };

    let registry = build_hearth_registry(resolved.clone());
    let atlas = fold_playbook_atlas(&resolved, &registry);
    let entry_count = atlas.entries.len();

    let entries = atlas
        .entries
        .into_iter()
        .map(atlas_entry_to_proto)
        .collect();

    tracing::info!(
        command = "playbook_atlas",
        hearth = %resolved.display(),
        entry_count = entry_count,
        outcome = "ok",
        "playbook_atlas query ok"
    );

    Ok(anvil_engine::proto::PlaybookAtlasResponse {
        resolved_hearth: resolved.display().to_string(),
        entries,
    })
}

/// The shared playbook-atlas-detail read used by BOTH the gRPC
/// `PlaybookAtlasDetail` RPC and the HTTP `/ws` `playbook_atlas_detail` method.
/// Returns the full states + edges + success_rubric for ONE registry-resolved
/// kind (the list/detail split keeps any single response small). `found == false`
/// when the kind is not a valid loaded machine. Same concrete-registry posture as
/// `compute_playbook_atlas`.
fn compute_playbook_atlas_detail(
    req_hearth_path: &str,
    kind: &str,
    default_hearth: Option<&Path>,
    hearth_policy: &HearthPolicy,
) -> Result<anvil_engine::proto::PlaybookAtlasDetailResponse, Status> {
    use anvil_core::domain::playbook::atlas::fold_playbook_atlas_detail;
    use anvil_core::domain::playbook::scorecard_agg::{
        fold_artifact_quality_into_steps, fold_overall_artifact_quality,
        fold_scorecard_measurements,
    };

    let resolved = match resolve_hearth(req_hearth_path, default_hearth, hearth_policy) {
        Ok(resolved) => resolved,
        Err(status) => {
            tracing::warn!(
                command = "playbook_atlas_detail",
                outcome = "error",
                error_code = ?status.code(),
                "playbook_atlas_detail query failed"
            );
            return Err(status);
        }
    };

    let registry = build_hearth_registry(resolved.clone());
    let detail = fold_playbook_atlas_detail(&registry, kind);
    let resolved_hearth = resolved.display().to_string();

    // Honest per-kind measurement enrichment: read+fold temper's on-disk
    // scorecards (read-only, fail-open — a missing/empty temper home yields an
    // empty summary, never an error). Independent of whether the machine
    // itself loaded, so a kind's real measurement history still surfaces even
    // if its machine.yaml is currently broken.
    const RECENT_MEASUREMENTS_LIMIT: usize = 20;
    let temper_home = resolve_temper_home();
    let mut scorecard_summary = temper_home
        .as_deref()
        .map(scorecard_reader::read_scorecard_instances)
        .map(|instances| {
            fold_scorecard_measurements(&instances, kind, RECENT_MEASUREMENTS_LIMIT)
        })
        .unwrap_or_default();
    // Fold in the REAL, harsh-anchored artifact-quality score — a separate
    // grader's sibling `artifact_quality.json` per instance. Additive: absent
    // temper home or no artifact_quality.json anywhere leaves `steps`
    // untouched (fail-open, same posture as the coherence read above).
    if let Some(home) = temper_home.as_deref() {
        let artifact_instances = scorecard_reader::read_artifact_quality_instances(home);
        scorecard_summary.steps =
            fold_artifact_quality_into_steps(scorecard_summary.steps, &artifact_instances, kind);
    }
    // The REAL headline number: the HARSH 0-10 artifact-quality mean, weighted
    // across every already-deduped step_quality row that carries one. NOT the
    // coherence-based overall_mean_quality below (uniformly low — a generic
    // §0-template grade the Atlas must never show as its primary score).
    let (overall_artifact_quality, artifact_measured_count) =
        fold_overall_artifact_quality(&scorecard_summary.steps);
    // Lookup so each recent (coherence) event can carry the step's CURRENT
    // known artifact-quality aggregate alongside it — NOT a per-event score
    // (artifact-quality is graded per-instance by a separate grader, not
    // emitted alongside every coherence event), but enough for the recent feed
    // to show the real score next to the secondary coherence one instead of
    // only the misleading coherence number.
    let artifact_quality_by_step: std::collections::HashMap<(String, String), (f64, u32)> =
        scorecard_summary
            .steps
            .iter()
            .filter_map(|s| {
                s.artifact_quality
                    .map(|q| ((s.to_state.clone(), s.role.clone()), (q, s.artifact_sample_count)))
            })
            .collect();
    let step_quality: Vec<anvil_engine::proto::AtlasStepQuality> = scorecard_summary
        .steps
        .iter()
        .map(|s| anvil_engine::proto::AtlasStepQuality {
            from_state: s.from_state.clone(),
            to_state: s.to_state.clone(),
            role: s.role.clone(),
            mean_quality: s.mean_quality,
            sample_count: s.sample_count,
            artifact_quality: s.artifact_quality.unwrap_or(0.0),
            artifact_sample_count: s.artifact_sample_count,
        })
        .collect();
    let recent_measurements: Vec<anvil_engine::proto::AtlasRecentMeasurement> = scorecard_summary
        .recent
        .iter()
        .map(|r| {
            let (artifact_quality, artifact_sample_count) = artifact_quality_by_step
                .get(&(r.to_state.clone(), r.role.clone()))
                .copied()
                .unwrap_or((0.0, 0));
            anvil_engine::proto::AtlasRecentMeasurement {
                instance_id: r.instance_id.clone(),
                to_state: r.to_state.clone(),
                role: r.role.clone(),
                actor: r.actor.clone(),
                quality_score: r.quality_score,
                model: r.model.clone(),
                artifact_quality,
                artifact_sample_count,
            }
        })
        .collect();
    let overall_mean_quality = scorecard_summary.overall_mean_quality;
    let measured_instance_count = scorecard_summary.instance_count;

    tracing::info!(
        command = "playbook_atlas_detail",
        hearth = %resolved.display(),
        kind = kind,
        found = detail.is_some(),
        measured_instance_count = measured_instance_count,
        outcome = "ok",
        "playbook_atlas_detail query ok"
    );

    match detail {
        Some(detail) => {
            let states = detail
                .states
                .into_iter()
                .map(|s| anvil_engine::proto::AtlasState {
                    name: s.name,
                    is_review_gate: s.is_review_gate,
                    is_terminal: s.is_terminal,
                    has_measurement: s.has_measurement,
                    has_hook: s.has_hook,
                })
                .collect();
            let edges = detail
                .edges
                .into_iter()
                .map(|e| anvil_engine::proto::AtlasEdge {
                    from: e.from,
                    to: e.to,
                    required_role: e.required_role,
                    required_satisfaction: e.required_satisfaction,
                })
                .collect();
            let success_rubric = detail.success_rubric.map(atlas_rubric_to_proto);
            Ok(anvil_engine::proto::PlaybookAtlasDetailResponse {
                resolved_hearth,
                kind: detail.kind,
                found: true,
                states,
                edges,
                success_rubric,
                step_quality,
                overall_mean_quality,
                measured_instance_count,
                recent_measurements,
                overall_artifact_quality,
                artifact_measured_count,
            })
        }
        None => Ok(anvil_engine::proto::PlaybookAtlasDetailResponse {
            resolved_hearth,
            kind: kind.to_string(),
            found: false,
            states: vec![],
            edges: vec![],
            success_rubric: None,
            step_quality,
            overall_mean_quality,
            measured_instance_count,
            recent_measurements,
            overall_artifact_quality,
            artifact_measured_count,
        }),
    }
}

/// Map the shared `Integrity` fold into its proto shape (usize counts → u32).
fn atlas_integrity_to_proto(
    integrity: &anvil_core::domain::playbook::integrity::Integrity,
) -> anvil_engine::proto::AtlasIntegrity {
    anvil_engine::proto::AtlasIntegrity {
        loads: integrity.loads,
        rubber_stamp_gates: integrity.rubber_stamp_gates.clone(),
        unmeasured_states: integrity.unmeasured_states.clone(),
        anchors_count: integrity.anchors_count as u32,
        grader_declared: integrity.grader_declared,
    }
}

/// The serialized `Register` token.
fn register_str(register: anvil_core::domain::playbook::types::Register) -> &'static str {
    use anvil_core::domain::playbook::types::Register;
    match register {
        Register::Driven => "driven",
        Register::Free => "free",
    }
}

/// Map one domain atlas entry (valid or invalid) into the flat proto `AtlasEntry`.
/// The invalid arm sets `loads == false` and carries the load error while still
/// slotting into its kit column via the per-dir status.yaml read.
fn atlas_entry_to_proto(
    entry: anvil_core::domain::playbook::atlas::AtlasEntry,
) -> anvil_engine::proto::AtlasEntry {
    use anvil_core::domain::playbook::atlas::AtlasEntry;
    match entry {
        AtlasEntry::Valid(v) => anvil_engine::proto::AtlasEntry {
            kind: v.kind,
            artifact_id: v.artifact_id,
            owner_kit: v.owner_kit,
            state: v.state.unwrap_or_default(),
            register: register_str(v.register).to_string(),
            loads: v.integrity.loads,
            error_code: String::new(),
            error_message: String::new(),
            integrity: Some(atlas_integrity_to_proto(&v.integrity)),
            calibration: v.calibration.as_str().to_string(),
            state_count: v.state_count as u32,
            edge_count: v.edge_count as u32,
        },
        AtlasEntry::Invalid(i) => anvil_engine::proto::AtlasEntry {
            kind: i.artifact_id.clone(),
            artifact_id: i.artifact_id,
            owner_kit: i.owner_kit,
            state: i.state.unwrap_or_default(),
            register: String::new(),
            loads: false,
            error_code: i.error_code,
            error_message: i.error_message,
            integrity: Some(anvil_engine::proto::AtlasIntegrity {
                loads: false,
                rubber_stamp_gates: vec![],
                unmeasured_states: vec![],
                anchors_count: 0,
                grader_declared: false,
            }),
            calibration: String::new(),
            state_count: 0,
            edge_count: 0,
        },
    }
}

/// Map the domain atlas rubric summary into its proto shape.
fn atlas_rubric_to_proto(
    rubric: anvil_core::domain::playbook::atlas::AtlasRubric,
) -> anvil_engine::proto::AtlasRubric {
    anvil_engine::proto::AtlasRubric {
        dimensions: rubric
            .dimensions
            .into_iter()
            .map(|d| anvil_engine::proto::AtlasRubricDimension {
                dimension: d.dimension,
                weight: d.weight,
                evidence_class: d.evidence_class,
            })
            .collect(),
        grader_declared: rubric.grader_declared,
        anchors_count: rubric.anchors_count as u32,
        lagging_signals: rubric.lagging_signals,
    }
}

/// The shared usage-time-series read used by BOTH the gRPC `UsageTimeSeries` RPC
/// and the HTTP `/ws` JSON-RPC `usage_timeseries` method. Folds the SAME durable
/// routing-activity sink `compute_playbook_activity` folds, bucketed by time.
/// Pure read: no write lock (mirror playbook_activity). Empty sink → empty
/// buckets (no error).
/// The sentinel `resolved_hearth` value returned when `all_hearths` folds the
/// whole deployment rather than a single resolved hearth.
const ALL_HEARTHS_SENTINEL: &str = "(all hearths)";

/// The set of hearths to fold for an `all_hearths` query.
///
/// A permitted root is frequently a PARENT directory (e.g. `~/Development`) that
/// holds NO sink itself — the real activity lives in sub-hearths beneath it
/// (`foundry-hearth/`, `kiln-hearth/`, …). So rather than folding each permitted
/// root verbatim, we DISCOVER the hearths beneath each root: the root itself if
/// it is a hearth, plus every immediate (1-level) subdirectory that looks like a
/// hearth. The explicit `--hearth` default is always included. The result is
/// deduplicated. This is purely a read-enumeration concern — the permitted-root
/// authorization gate in `resolve_hearth` is unchanged. The per-deployment salt
/// makes the unioned actor sets correct.
fn all_hearths_to_fold(hearth_policy: &HearthPolicy) -> Vec<PathBuf> {
    anvil_core_hearth::hearth_discovery::discover_hearths(&hearth_policy.permitted_roots, None)
}

fn compute_usage_timeseries(
    req_hearth_path: &str,
    granularity: &str,
    all_hearths: bool,
    default_hearth: Option<&Path>,
    hearth_policy: &HearthPolicy,
    _global_playbooks_hearth: Option<&Path>,
) -> Result<UsageTimeSeriesResponse, Status> {
    let granularity_parsed = Granularity::parse(granularity);

    let (result, resolved_hearth, hearths_included) = if all_hearths {
        // Cross-hearth: fold EVERY permitted hearth's ACTIVITY LOG and merge — sum
        // total turns, union the actor_hash sets (correct because the salt is
        // per-deployment). Counts ALL command turns (the headline denominator), so
        // the over-time / per-hearth views reconcile with activity_summary.
        let hearths = all_hearths_to_fold(hearth_policy);
        let mut streams = Vec::with_capacity(hearths.len());
        for hearth in &hearths {
            streams.push(read_hearth_activity_log(hearth)?);
        }
        let result =
            anvil_core::domain::usage_timeseries::fold_usage_timeseries_from_activity_log_across_hearths(
                &streams,
                granularity_parsed,
            );
        let included: Vec<String> = hearths.iter().map(|h| h.display().to_string()).collect();
        (result, ALL_HEARTHS_SENTINEL.to_string(), included)
    } else {
        let resolved = match resolve_hearth(req_hearth_path, default_hearth, hearth_policy) {
            Ok(resolved) => resolved,
            Err(status) => {
                tracing::warn!(
                    command = "usage_timeseries",
                    outcome = "error",
                    error_code = ?status.code(),
                    "usage_timeseries query failed"
                );
                return Err(status);
            }
        };
        let records = read_hearth_activity_log(&resolved)?;
        let result = anvil_core::domain::usage_timeseries::fold_usage_timeseries_from_activity_log(
            &records,
            granularity_parsed,
        );
        let resolved_str = resolved.display().to_string();
        let included = vec![resolved_str.clone()];
        (result, resolved_str, included)
    };

    tracing::info!(
        command = "usage_timeseries",
        hearth = %resolved_hearth,
        all_hearths = all_hearths,
        bucket_count = result.buckets.len(),
        outcome = "ok",
        "usage_timeseries query ok"
    );

    let buckets = result
        .buckets
        .into_iter()
        .map(|bucket| anvil_engine::proto::TimeBucket {
            period_start: bucket.period_start,
            total_calls: bucket.total_calls,
            distinct_actors: bucket.distinct_actors,
            begin_count: bucket.begin_count,
            complete_count: bucket.complete_count,
            per_artifact_kind: bucket
                .per_artifact_kind
                .into_iter()
                .map(|w| anvil_engine::proto::ArtifactKindPeriodCount {
                    kind: w.kind,
                    call_count: w.call_count,
                })
                .collect(),
        })
        .collect();

    Ok(UsageTimeSeriesResponse {
        buckets,
        resolved_hearth,
        hearths_included,
    })
}

/// Read one hearth's universal activity-log sink. A missing sink reads as an
/// empty stream (no error).
fn read_hearth_activity_log(hearth: &Path) -> Result<Vec<ActivityLogRecord>, Status> {
    FileSystemActivityLogAdapter::new(hearth)
        .read_activity_log()
        .map_err(|e| Status::internal(format!("read_activity_log failed: {}", e)))
}

/// The shared activity-summary read used by BOTH the gRPC `ActivitySummary` RPC
/// and the HTTP `/ws` JSON-RPC `activity_summary` method. Folds the durable,
/// redacted universal activity-log sink into the usage summary. Pure read: no
/// write lock. `all_hearths` folds every permitted root and unions, exactly like
/// `usage_time_series`/`actor_activity`. An empty sink yields a zeroed summary.
fn compute_activity_summary(
    req_hearth_path: &str,
    granularity: &str,
    all_hearths: bool,
    default_hearth: Option<&Path>,
    hearth_policy: &HearthPolicy,
) -> Result<ActivitySummaryResponse, Status> {
    use anvil_core::domain::activity_summary::{
        fold_activity_summary, fold_activity_summary_across_hearths,
    };
    let granularity_parsed = Granularity::parse(granularity);

    let (result, resolved_hearth, hearths_included) = if all_hearths {
        let hearths = all_hearths_to_fold(hearth_policy);
        let mut streams = Vec::with_capacity(hearths.len());
        for hearth in &hearths {
            streams.push(read_hearth_activity_log(hearth)?);
        }
        let result = fold_activity_summary_across_hearths(&streams, granularity_parsed);
        let included: Vec<String> = hearths.iter().map(|h| h.display().to_string()).collect();
        (result, ALL_HEARTHS_SENTINEL.to_string(), included)
    } else {
        let resolved = match resolve_hearth(req_hearth_path, default_hearth, hearth_policy) {
            Ok(resolved) => resolved,
            Err(status) => {
                tracing::warn!(
                    command = "activity_summary",
                    outcome = "error",
                    error_code = ?status.code(),
                    "activity_summary query failed"
                );
                return Err(status);
            }
        };
        let records = read_hearth_activity_log(&resolved)?;
        let result = fold_activity_summary(&records, granularity_parsed);
        let resolved_str = resolved.display().to_string();
        let included = vec![resolved_str.clone()];
        (result, resolved_str, included)
    };

    tracing::info!(
        command = "activity_summary",
        hearth = %resolved_hearth,
        all_hearths = all_hearths,
        total_turns = result.total_turns,
        outcome = "ok",
        "activity_summary query ok"
    );

    let to_proto = |list: Vec<anvil_core::domain::activity_summary::LabelCount>| {
        list.into_iter()
            .map(|c| anvil_engine::proto::ActivityLabelCount {
                label: c.label,
                count: c.count,
            })
            .collect::<Vec<_>>()
    };

    let buckets = result
        .buckets
        .into_iter()
        .map(|b| anvil_engine::proto::ActivityTimeBucket {
            period_start: b.period_start,
            total_turns: b.total_turns,
            distinct_actors: b.distinct_actors,
        })
        .collect();

    Ok(ActivitySummaryResponse {
        resolved_hearth,
        hearths_included,
        total_turns: result.total_turns,
        by_command: to_proto(result.by_command),
        by_route_outcome: to_proto(result.by_route_outcome),
        by_source: to_proto(result.by_source),
        by_artifact_kind: to_proto(result.by_artifact_kind),
        buckets,
        routed_conversations: result.routed_conversations,
        converted_conversations: result.converted_conversations,
        by_call_state: to_proto(result.by_call_state),
        playbook_step_turns: result.playbook_step_turns,
    })
}

/// Resolve `JoinCoverageRequest`'s hearth set (R9.2), in five rules that a
/// wrong implementation fails one at a time.
///
/// 1. UNION of the scalar and the list — supplying both is legal. An empty
///    union resolves the engine default exactly as every other read RPC does.
/// 2. An EMPTY entry inside the repeated field names no hearth and is SKIPPED.
///    The empty string is the scalar field's default sentinel only; defaulting
///    it here would fold a hearth nobody asked for into a per-hearth report.
///    `None` is passed as `resolve_hearth`'s default precisely so this skip
///    cannot be re-entered through the back door.
/// 3. FAIL-CLOSED: any member that does not resolve returns THAT member's
///    `Status` and no report at all. A partial report answers a different
///    question than the one asked while looking like an answer to this one.
/// 4. Dedup by CANONICAL path, after resolution — a relative spelling, a
///    `..`-bearing spelling and a symlinked alias are one hearth.
/// 5. Order canonical-path ascending, never request order, so two requests
///    naming the same set in opposite order are the same request.
fn resolve_join_hearths(
    req: &anvil_engine::proto::JoinCoverageRequest,
    default_hearth: Option<&Path>,
    policy: &HearthPolicy,
) -> Result<Vec<PathBuf>, Status> {
    let mut requested: Vec<&str> = Vec::new();
    if !req.hearth_path.is_empty() {
        requested.push(req.hearth_path.as_str());
    }
    requested.extend(
        req.hearth_paths
            .iter()
            .map(|p| p.as_str())
            .filter(|p| !p.is_empty()),
    );

    let mut resolved: Vec<PathBuf> = Vec::new();
    if requested.is_empty() {
        resolved.push(resolve_hearth("", default_hearth, policy)?);
    } else {
        for member in requested {
            resolved.push(resolve_hearth(member, None, policy)?);
        }
    }
    resolved.sort();
    resolved.dedup();
    Ok(resolved)
}

/// `hearth_label` per R9.2a: the ONE basename rule, disambiguated to
/// `<basename>#<ordinal>` for every member of a colliding group.
///
/// The collision matters beyond display. A label is how a consumer groups, and
/// two hearths sharing one would have their groups silently merged. The ordinal
/// is used rather than a path fingerprint because this report exposes no path
/// and a fingerprint OF a path is derived from one; the price, stated rather
/// than buried, is that a disambiguated label is unique WITHIN a report and is
/// not a stable cross-report identifier.
fn join_hearth_labels(resolved: &[PathBuf]) -> Vec<String> {
    let bases: Vec<String> = resolved
        .iter()
        .map(|p| {
            anvil_core::ports::delivery_log_port::project_label_from_root(&p.to_string_lossy())
        })
        .collect();
    let mut seen: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for base in &bases {
        *seen.entry(base.as_str()).or_insert(0) += 1;
    }
    let mut ordinal: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    bases
        .iter()
        .map(|base| {
            if seen.get(base.as_str()).copied().unwrap_or(0) < 2 {
                return base.clone();
            }
            let n = ordinal.entry(base.as_str()).or_insert(0);
            *n += 1;
            format!("{}#{}", base, n)
        })
        .collect()
}

/// The `JoinCoverage` read. Resolves the hearth set, reads both sinks through
/// their read ports (the activity side reduced at ingest by `is_join_relevant`),
/// resolves the key epoch ONCE for the whole request from the deployment salt,
/// hands the vectors to `fold_join_episodes`, and returns the per-hearth report.
///
/// Two properties this function is responsible for and the fold cannot be:
/// the ENGINE touches the filesystem so `anvil-core` never sees a salt, and the
/// salt is read with the NON-generating `peek_salt` — `resolve_salt` persists a
/// generated one, and a read RPC that writes a file is a write path whatever
/// its name says.
fn compute_join_coverage(
    req: &anvil_engine::proto::JoinCoverageRequest,
    default_hearth: Option<&Path>,
    hearth_policy: &HearthPolicy,
    global_playbooks_hearth: Option<&Path>,
) -> Result<anvil_engine::proto::JoinCoverageResponse, Status> {
    use anvil_core::domain::join_episode::{
        fold_join_coverage, fold_join_episodes, is_join_relevant, HearthJoinInput, JoinOptions,
    };
    use anvil_core::domain::telemetry_salt::key_epoch;
    use anvil_core::ports::delivery_log_port::DeliveryLogReadPort;
    use anvil_core_hearth::fs_delivery_log_adapter::FileSystemDeliveryLogAdapter;

    let resolved = match resolve_join_hearths(req, default_hearth, hearth_policy) {
        Ok(resolved) => resolved,
        Err(status) => {
            tracing::warn!(
                command = "join_coverage",
                outcome = "error",
                error_code = ?status.code(),
                "join_coverage query failed"
            );
            return Err(status);
        }
    };
    let labels = join_hearth_labels(&resolved);

    // ONE epoch per request: the salt is per-DEPLOYMENT, so every per-hearth
    // entry of one response carries the same value. Reading it per hearth would
    // make two hearths under one deployment look like two keyspaces.
    let deployment_epoch = key_epoch(
        telemetry_salt::peek_salt(global_playbooks_hearth, &hearth_policy.permitted_roots)
            .as_deref(),
    );

    // Read every hearth first; the fold borrows these slices and is pure.
    let mut delivery_scans = Vec::with_capacity(resolved.len());
    let mut activity_scans = Vec::with_capacity(resolved.len());
    let mut local_epochs = Vec::with_capacity(resolved.len());
    for hearth in &resolved {
        delivery_scans.push(
            FileSystemDeliveryLogAdapter::new(hearth)
                .read_delivery_log()
                .map_err(|e| Status::internal(format!("read_delivery_log failed: {}", e)))?,
        );
        activity_scans.push(
            FileSystemActivityLogAdapter::new(hearth)
                .read_activity_log_where(&is_join_relevant)
                .map_err(|e| Status::internal(format!("read_activity_log failed: {}", e)))?,
        );
        // This hearth's OWN salt file, fingerprinted the same way. Never the
        // salt: the report carries epochs so two of them can be compared
        // without either carrying the thing they fingerprint.
        local_epochs.push(
            std::fs::read_to_string(hearth.join(telemetry_salt::SALT_FILENAME))
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .map(|s| key_epoch(Some(&s))),
        );
    }

    let inputs: Vec<HearthJoinInput> = (0..resolved.len())
        .map(|i| HearthJoinInput {
            hearth_label: labels[i].clone(),
            delivery: &delivery_scans[i].records,
            activity: &activity_scans[i].records,
            hearth_salt_file_epoch: local_epochs[i].clone(),
            read_defects: delivery_scans[i].read_defects,
            activity_rows_scanned: activity_scans[i].rows_scanned,
        })
        .collect();

    let registry = request_session_registry(
        resolved.first().map(|h| h.as_path()).unwrap_or(Path::new(".")),
        global_playbooks_hearth,
    );
    let options = JoinOptions {
        window_start: req.window_start.clone(),
        window_end: req.window_end.clone(),
        project_label: Some(req.project_label.clone()).filter(|p| !p.is_empty()),
        key_epoch: deployment_epoch,
    };
    let set = fold_join_episodes(&inputs, registry.as_ref(), &options);
    let report = fold_join_coverage(&set);

    tracing::info!(
        command = "join_coverage",
        hearths = resolved.len(),
        episodes = set.episodes.len(),
        outcome = "ok",
        "join_coverage query ok"
    );

    Ok(anvil_engine::proto::JoinCoverageResponse {
        per_hearth: report
            .per_hearth
            .into_iter()
            .map(|h| anvil_engine::proto::HearthJoinCoverageReport {
                hearth_label: h.hearth_label,
                window_start: h.window_start,
                window_end: h.window_end,
                delivery_rows_read: h.delivery_rows_read,
                read_defects: h.read_defects,
                activity_rows_scanned: h.activity_rows_scanned,
                activity_rows_retained: h.activity_rows_retained,
                begin_rows_read: h.begin_rows_read,
                episode_denominator: h.episode_denominator,
                begin_denominator: h.begin_denominator,
                menu_delivered: h.menu_delivered,
                nothing_delivered: h.nothing_delivered,
                no_engine_answer: h.no_engine_answer,
                joined: h.joined,
                unjoin: Some(anvil_engine::proto::JoinUnjoinCounts {
                    no_conversation_key: h.unjoin.no_conversation_key,
                    pre_migration_row: h.unjoin.pre_migration_row,
                    conversation_absent_from_begin_side: h
                        .unjoin
                        .conversation_absent_from_begin_side,
                    no_begin_of_kind_in_conversation: h.unjoin.no_begin_of_kind_in_conversation,
                    superseded_by_later_delivery_of_kind: h
                        .unjoin
                        .superseded_by_later_delivery_of_kind,
                }),
                begin_unjoin: Some(anvil_engine::proto::JoinBeginUnjoinCounts {
                    no_conversation_key: h.begin_unjoin.no_conversation_key,
                    conversation_absent_from_delivery_side: h
                        .begin_unjoin
                        .conversation_absent_from_delivery_side,
                    no_prior_delivery_of_kind: h.begin_unjoin.no_prior_delivery_of_kind,
                    no_unconsumed_prior_delivery_of_kind: h
                        .begin_unjoin
                        .no_unconsumed_prior_delivery_of_kind,
                }),
                terminal: Some(anvil_engine::proto::JoinTerminalCounts {
                    not_joined: h.terminal.not_joined,
                    not_yet_terminal: h.terminal.not_yet_terminal,
                    reached_terminal: h.terminal.reached_terminal,
                    unknown_run_state: h.terminal.unknown_run_state,
                }),
                key_epoch: h.key_epoch,
                hearth_salt_file_epoch: h.hearth_salt_file_epoch.unwrap_or_default(),
                key_epoch_reconciliation: format!("{:?}", h.key_epoch_reconciliation),
            })
            .collect(),
        filter_version: report.filter_version,
    })
}

/// The shared playbook-fidelity read backing the gRPC `PlaybookFidelity` RPC.
/// Folds the SAME durable, redacted universal activity-log sink
/// `compute_activity_summary` folds, into the Layer-3 fidelity measure. Pure
/// read: no write lock. The per-call registry (mirror route/describe) supplies
/// the per-kind terminal predicate; a fresh registry per call means machine.yaml
/// edits live-reload without restart. `all_hearths` folds every permitted root
/// and merges. An empty sink yields a zeroed fidelity result.
fn compute_playbook_fidelity(
    req_hearth_path: &str,
    all_hearths: bool,
    default_hearth: Option<&Path>,
    hearth_policy: &HearthPolicy,
    global_playbooks_hearth: Option<&Path>,
) -> Result<PlaybookFidelityResponse, Status> {
    use anvil_core::domain::playbook_run_fidelity::{
        fold_playbook_run_fidelity, fold_playbook_run_fidelity_across_hearths,
    };

    let (result, resolved_hearth, hearths_included) = if all_hearths {
        let hearths = all_hearths_to_fold(hearth_policy);
        let mut streams = Vec::with_capacity(hearths.len());
        for hearth in &hearths {
            streams.push(read_hearth_activity_log(hearth)?);
        }
        // The lifecycle kinds resolve identically across hearths (seed/global
        // fallback); build the registry from the first folded hearth (or the
        // default when none), since the terminal predicate is kind-stable.
        let registry_hearth = hearths
            .first()
            .map(|h| h.as_path())
            .or(default_hearth)
            .unwrap_or_else(|| Path::new("."));
        let registry = request_session_registry(registry_hearth, global_playbooks_hearth);
        let result = fold_playbook_run_fidelity_across_hearths(&streams, registry.as_ref());
        let included: Vec<String> = hearths.iter().map(|h| h.display().to_string()).collect();
        (result, ALL_HEARTHS_SENTINEL.to_string(), included)
    } else {
        let resolved = match resolve_hearth(req_hearth_path, default_hearth, hearth_policy) {
            Ok(resolved) => resolved,
            Err(status) => {
                tracing::warn!(
                    command = "playbook_fidelity",
                    outcome = "error",
                    error_code = ?status.code(),
                    "playbook_fidelity query failed"
                );
                return Err(status);
            }
        };
        let records = read_hearth_activity_log(&resolved)?;
        let registry = request_session_registry(&resolved, global_playbooks_hearth);
        let result = fold_playbook_run_fidelity(&records, registry.as_ref());
        let resolved_str = resolved.display().to_string();
        let included = vec![resolved_str.clone()];
        (result, resolved_str, included)
    };

    tracing::info!(
        command = "playbook_fidelity",
        hearth = %resolved_hearth,
        all_hearths = all_hearths,
        dangling_instances = result.dangling_instances,
        outcome = "ok",
        "playbook_fidelity query ok"
    );

    let to_proto = |list: Vec<anvil_core::domain::activity_summary::LabelCount>| {
        list.into_iter()
            .map(|c| anvil_engine::proto::ActivityLabelCount {
                label: c.label,
                count: c.count,
            })
            .collect::<Vec<_>>()
    };

    let completion = result
        .completion
        .into_iter()
        .map(|c| anvil_engine::proto::KindCompletion {
            kind: c.kind,
            begun: c.begun,
            terminal: c.terminal,
            completion_rate: c.completion_rate,
        })
        .collect();

    let instances = result
        .instances
        .into_iter()
        .map(|i| anvil_engine::proto::PlaybookRunFidelity {
            instance_id: i.instance_id,
            kind: i.kind,
            folded_state: i.folded_state,
            begun: i.begun,
            transition_count: i.transition_count,
            reached_terminal: i.reached_terminal,
            dangling: i.dangling,
            revision_cycles: i.revision_cycles,
        })
        .collect();

    Ok(PlaybookFidelityResponse {
        resolved_hearth,
        hearths_included,
        completion,
        instances,
        dangling_instances: result.dangling_instances,
        dangling_by_kind: to_proto(result.dangling_by_kind),
        revision_cycles: to_proto(result.revision_cycles),
        revision_cycles_total: result.revision_cycles_total,
        review_exits: result.review.review_exits,
        delegated_exits: result.review.delegated_exits,
        self_review_exits: result.review.self_review_exits,
        review_elapsed_seconds: result.review_elapsed_seconds,
    })
}

/// The shared per-step-volume read used by BOTH the gRPC `PlaybookStepVolume`
/// RPC and the HTTP `/ws` JSON-RPC `playbook_step_volume` method. Folds the
/// durable step-measurement sink filtered to `kind`. Pure read: no write lock.
/// Unknown/zero-record kind → empty steps (no error).
fn compute_playbook_step_volume(
    req_hearth_path: &str,
    kind: &str,
    all_hearths: bool,
    default_hearth: Option<&Path>,
    hearth_policy: &HearthPolicy,
    _global_playbooks_hearth: Option<&Path>,
) -> Result<PlaybookStepVolumeResponse, Status> {
    let (result, resolved_hearth, hearths_included) = if all_hearths {
        let hearths = all_hearths_to_fold(hearth_policy);
        let mut streams = Vec::with_capacity(hearths.len());
        for hearth in &hearths {
            let records = FileSystemStepMeasurementAdapter::new(hearth)
                .read_step_measurements()
                .map_err(|e| Status::internal(format!("read_step_measurements failed: {}", e)))?;
            streams.push(records);
        }
        let result = anvil_core::domain::usage_timeseries::fold_playbook_step_volume_across_hearths(
            &streams, kind,
        );
        let included: Vec<String> = hearths.iter().map(|h| h.display().to_string()).collect();
        (result, ALL_HEARTHS_SENTINEL.to_string(), included)
    } else {
        let resolved = match resolve_hearth(req_hearth_path, default_hearth, hearth_policy) {
            Ok(resolved) => resolved,
            Err(status) => {
                tracing::warn!(
                    command = "playbook_step_volume",
                    outcome = "error",
                    error_code = ?status.code(),
                    "playbook_step_volume query failed"
                );
                return Err(status);
            }
        };
        let records = FileSystemStepMeasurementAdapter::new(&resolved)
            .read_step_measurements()
            .map_err(|e| Status::internal(format!("read_step_measurements failed: {}", e)))?;
        let result = fold_playbook_step_volume(&records, kind);
        let resolved_str = resolved.display().to_string();
        let included = vec![resolved_str.clone()];
        (result, resolved_str, included)
    };

    tracing::info!(
        command = "playbook_step_volume",
        hearth = %resolved_hearth,
        all_hearths = all_hearths,
        step_count = result.steps.len(),
        outcome = "ok",
        "playbook_step_volume query ok"
    );

    let steps = result
        .steps
        .into_iter()
        .map(|step| anvil_engine::proto::StepCount {
            from_state: step.from_state,
            to_state: step.to_state,
            role: step.role,
            call_count: step.call_count,
        })
        .collect();

    Ok(PlaybookStepVolumeResponse {
        resolved_hearth,
        kind: result.kind,
        steps,
        hearths_included,
    })
}

/// Read one hearth's per-artifact `(artifact_kind, begin-markers)` streams.
///
/// Walks every immediate subdirectory of the hearth (the per-kind directories,
/// e.g. `tracks/`, `milestones/`) and each of their child artifact directories
/// for a `status.yaml`, parsing the artifact's `kind` and its `activity:`
/// begin-markers. A directory with no `status.yaml`, an unparseable
/// `status.yaml`, or no `activity:` section contributes nothing (best-effort,
/// mirroring the empty-sink-reads-as-zero posture of the other read queries).
///
/// LOCAL-ONLY: the returned [`ActivityEntry`]s carry RAW actor names. This data
/// feeds ONLY the local-dashboard `actor_activity` query — never telemetry.
fn read_hearth_actor_streams(hearth: &Path) -> Vec<(String, Vec<ActivityEntry>)> {
    let mut streams: Vec<(String, Vec<ActivityEntry>)> = Vec::new();
    let Ok(type_dirs) = std::fs::read_dir(hearth) else {
        return streams;
    };
    for type_entry in type_dirs.flatten() {
        let type_dir = type_entry.path();
        if !type_dir.is_dir() {
            continue;
        }
        let Ok(artifact_dirs) = std::fs::read_dir(&type_dir) else {
            continue;
        };
        for artifact_entry in artifact_dirs.flatten() {
            let artifact_dir = artifact_entry.path();
            if !artifact_dir.is_dir() {
                continue;
            }
            let status_path = artifact_dir.join("status.yaml");
            let Ok(content) = std::fs::read_to_string(&status_path) else {
                continue;
            };
            let Ok(status) = serde_yaml::from_str::<FullStatusYaml>(&content) else {
                continue;
            };
            // Actor-activity display fold: show the surviving markers. A
            // dropped (unparseable) marker cannot be displayed anyway; the
            // adapter boundary already warned about the degradation.
            let activity = status.activity_entries();
            if activity.is_empty() {
                continue;
            }
            let kind = status.kind.unwrap_or_default();
            streams.push((kind, activity));
        }
    }
    streams
}

/// The shared LOCAL-ONLY actor-activity read used by BOTH the gRPC
/// `ActorActivity` RPC and the HTTP `/ws` JSON-RPC `actor_activity` method.
///
/// Folds the `activity:` begin-markers across every artifact in the resolved
/// hearth (or every permitted hearth when `all_hearths`) into per-actor
/// activity, returning the RAW actor NAMES.
///
/// CRITICAL — LOCAL-ONLY, NEVER TELEMETRY. This is the local-dashboard
/// counterpart to the salted `distinct_actors` count UsageTimeSeries reports.
/// It returns raw actor names served ONLY over the loopback `/ws` bridge and the
/// on-machine gRPC surface. The raw names MUST NOT be emitted to the Part-3
/// telemetry path — the salted `actor_hash` in the step-measurement sink remains
/// the only actor signal that may leave the machine. This read touches ONLY the
/// begin-markers; it never reads or writes the telemetry emitter or the
/// step-measurement sink. Pure read: no write lock (mirror playbook_activity).
fn compute_actor_activity(
    req_hearth_path: &str,
    all_hearths: bool,
    default_hearth: Option<&Path>,
    hearth_policy: &HearthPolicy,
) -> Result<ActorActivityResponse, Status> {
    let (result, resolved_hearth, hearths_included) = if all_hearths {
        let hearths = all_hearths_to_fold(hearth_policy);
        let mut per_hearth = Vec::with_capacity(hearths.len());
        for hearth in &hearths {
            per_hearth.push(read_hearth_actor_streams(hearth));
        }
        let result = fold_actor_activity_across_hearths(&per_hearth);
        let included: Vec<String> = hearths.iter().map(|h| h.display().to_string()).collect();
        (result, ALL_HEARTHS_SENTINEL.to_string(), included)
    } else {
        let resolved = match resolve_hearth(req_hearth_path, default_hearth, hearth_policy) {
            Ok(resolved) => resolved,
            Err(status) => {
                tracing::warn!(
                    command = "actor_activity",
                    outcome = "error",
                    error_code = ?status.code(),
                    "actor_activity query failed"
                );
                return Err(status);
            }
        };
        let streams = read_hearth_actor_streams(&resolved);
        let result = fold_actor_activity(&streams);
        let resolved_str = resolved.display().to_string();
        let included = vec![resolved_str.clone()];
        (result, resolved_str, included)
    };

    // CQRS log: hearth + command + outcome + actor COUNT only. Never log the
    // raw actor names (they are local-only and have no place in the log stream).
    tracing::info!(
        command = "actor_activity",
        hearth = %resolved_hearth,
        all_hearths = all_hearths,
        actor_count = result.entries.len(),
        outcome = "ok",
        "actor_activity query ok"
    );

    let actors = result
        .entries
        .into_iter()
        .map(|entry| anvil_engine::proto::ActorActivityEntry {
            actor: entry.actor,
            begin_count: entry.begin_count,
            last_active: entry.last_active,
            artifact_kinds: entry.artifact_kinds,
        })
        .collect();

    Ok(ActorActivityResponse {
        resolved_hearth,
        hearths_included,
        actors,
    })
}

/// Read one hearth's OPEN-instance inputs for the live view.
///
/// The open set is the catalog active-artifacts (already non-terminal via the
/// hardcoded `is_terminal_state` filter, with each state resolved via the SAME
/// `resolve_state_with_events` fold). For each open artifact we reach its
/// status.yaml to read the SPECIFIC `kind` (`status.yaml.kind`, falling back to
/// the coarse artifact type) and the LOCAL-ONLY `activity:` begin-markers — the
/// SAME begin-marker source `read_hearth_actor_streams` reads, NEVER the redacted
/// activity-log `actor_hash`. A registry-aware `state_is_terminal` belt-and-
/// suspenders drop catches machine-declared terminal states outside the hardcoded
/// list. A missing/unreadable hearth yields no inputs (no error — a fresh hearth
/// simply has no open instances). Returns inputs in catalog order; the pure
/// `fold_live_instances` imposes the deterministic instance_id ordering.
fn read_hearth_live_inputs(
    hearth: &Path,
    global_playbooks_hearth: Option<&Path>,
) -> Vec<anvil_core::domain::live_instances::LiveInstanceInput> {
    use anvil_core::domain::live_instances::LiveInstanceInput;
    use anvil_core::domain::route::state_is_terminal;
    use std::collections::HashMap;

    let reader = FileSystemHearthReader::new(hearth.to_path_buf());
    let catalog = match CatalogQueryHandler::execute(&reader) {
        Ok(catalog) => catalog,
        Err(_) => return Vec::new(),
    };
    let registry = request_session_registry(hearth, global_playbooks_hearth);

    // §0-activity is partitioned per-kind on disk; cache one index per kind
    // encountered so a hearth with many instances of the same kind reads its
    // events.jsonl once, not once per instance.
    let temper_home = resolve_temper_home();
    let mut step0_cache: HashMap<String, step0_activity_index::Step0ActivityIndex> =
        HashMap::new();

    let mut inputs = Vec::new();
    for artifact in catalog.active_artifacts {
        // Reach the instance's status.yaml for its specific kind + the LOCAL-ONLY
        // begin-markers. `directory_name()` maps Playbook → "playbooks".
        let dir = hearth
            .join(artifact.artifact_type.directory_name())
            .join(&artifact.id);
        let (kind, activity) = match std::fs::read_to_string(dir.join("status.yaml")) {
            Ok(content) => match serde_yaml::from_str::<FullStatusYaml>(&content) {
                Ok(status) => {
                    // Live-instance display fold: the surviving markers (a
                    // dropped one is unparseable and cannot be shown anyway).
                    // Read entries BEFORE moving `status.kind` below.
                    let activity = status.activity_entries();
                    (
                        status
                            .kind
                            .filter(|k| !k.trim().is_empty())
                            .unwrap_or_else(|| artifact.artifact_type.as_str().to_string()),
                        activity,
                    )
                }
                Err(_) => (artifact.artifact_type.as_str().to_string(), Vec::new()),
            },
            Err(_) => (artifact.artifact_type.as_str().to_string(), Vec::new()),
        };

        // Belt-and-suspenders: drop a machine-declared terminal state even when
        // its name is outside the hardcoded TERMINAL_STATES the catalog filters on.
        if state_is_terminal(registry.as_ref(), &kind, &artifact.state) {
            continue;
        }

        let step0_index = step0_cache
            .entry(kind.clone())
            .or_insert_with(|| {
                step0_activity_index::read_step0_activity_index(temper_home.as_deref(), &kind)
            });
        let (action_count, last_step0_at) = step0_index
            .get(&artifact.id)
            .cloned()
            .unwrap_or((0, String::new()));

        inputs.push(LiveInstanceInput {
            instance_id: artifact.id,
            kind,
            state: artifact.state,
            activity,
            action_count,
            last_step0_at,
            artifact_dir: dir.display().to_string(),
        });
    }
    inputs
}

/// The shared LOCAL-ONLY live-instances read used by BOTH the gRPC
/// `LiveInstances` RPC and the HTTP `/ws` JSON-RPC `live_instances` method.
///
/// Folds the OPEN (non-terminal) instances across the resolved hearth (or every
/// permitted hearth when `all_hearths`) into per-instance rows carrying the RAW
/// actor NAME + timestamp from each instance's `activity:` begin-markers.
///
/// CRITICAL — LOCAL-ONLY, NEVER TELEMETRY. The raw actor names this returns are
/// served ONLY over the loopback `/ws` bridge and the on-machine gRPC surface.
/// This read touches begin-markers only; it NEVER reads the salted `actor_hash`
/// in the redacted activity-log (the dishonesty the Atlas exists to expose) or
/// the step-measurement sink. Pure read: no write lock (mirror actor_activity).
fn compute_live_instances(
    req_hearth_path: &str,
    all_hearths: bool,
    default_hearth: Option<&Path>,
    hearth_policy: &HearthPolicy,
    global_playbooks_hearth: Option<&Path>,
) -> Result<anvil_engine::proto::LiveInstancesResponse, Status> {
    use anvil_core::domain::live_instances::fold_live_instances;

    // Injected once so the (single) fold call below classifies live-vs-dormant
    // against ONE consistent instant, even when all_hearths concatenates
    // inputs from multiple hearths before folding.
    let now = chrono::Utc::now();

    let (result, resolved_hearth, hearths_included) = if all_hearths {
        let hearths = all_hearths_to_fold(hearth_policy);
        let mut all_inputs = Vec::new();
        for hearth in &hearths {
            all_inputs.extend(read_hearth_live_inputs(hearth, global_playbooks_hearth));
        }
        let result = fold_live_instances(&all_inputs, now);
        let included: Vec<String> = hearths.iter().map(|h| h.display().to_string()).collect();
        (result, ALL_HEARTHS_SENTINEL.to_string(), included)
    } else {
        let resolved = match resolve_hearth(req_hearth_path, default_hearth, hearth_policy) {
            Ok(resolved) => resolved,
            Err(status) => {
                tracing::warn!(
                    command = "live_instances",
                    outcome = "error",
                    error_code = ?status.code(),
                    "live_instances query failed"
                );
                return Err(status);
            }
        };
        let inputs = read_hearth_live_inputs(&resolved, global_playbooks_hearth);
        let result = fold_live_instances(&inputs, now);
        let resolved_str = resolved.display().to_string();
        let included = vec![resolved_str.clone()];
        (result, resolved_str, included)
    };

    // CQRS log: hearth + command + outcome + instance COUNT only. Never log the
    // raw actor names (they are local-only and have no place in the log stream).
    tracing::info!(
        command = "live_instances",
        hearth = %resolved_hearth,
        all_hearths = all_hearths,
        instance_count = result.instances.len(),
        idle_count = result.idle_count,
        outcome = "ok",
        "live_instances query ok"
    );

    let idle_count = result.idle_count;
    let instances = result
        .instances
        .into_iter()
        .map(|inst| anvil_engine::proto::LiveInstance {
            instance_id: inst.instance_id,
            kind: inst.kind,
            state: inst.state,
            actor: inst.actor,
            at: inst.at,
            current_step: inst.current_step,
            action_count: inst.action_count,
            artifact_dir: inst.artifact_dir,
        })
        .collect();

    Ok(anvil_engine::proto::LiveInstancesResponse {
        resolved_hearth,
        hearths_included,
        instances,
        idle_count,
    })
}

/// The shared read used by BOTH the gRPC `ListInstanceArtifacts` RPC and the
/// HTTP `/ws` JSON-RPC `list_instance_artifacts` method: lets the Atlas live
/// panel explore a live instance's on-disk artifacts read-only from its track
/// directory (the SAME `LiveInstance.artifact_dir` value).
///
/// FAIL-CLOSED on path escape: `instance_dir` must canonicalize under one of
/// `policy`'s permitted roots (the SAME `HearthPolicy` every other query
/// enforces) — a client cannot use this to read arbitrary filesystem paths.
/// FAIL-OPEN on ordinary read trouble: a permitted-but-unreadable directory
/// yields an empty `artifacts` list and a non-empty `error_message`, never a
/// hard error. Non-recursive (track/proposal/milestone/decision directories
/// are flat) — one `read_dir` is enough.
fn compute_list_instance_artifacts(
    instance_dir: &str,
    policy: &HearthPolicy,
) -> Result<anvil_engine::proto::ListInstanceArtifactsResponse, Status> {
    let candidate = Path::new(instance_dir);
    let canonical = std::fs::canonicalize(candidate).map_err(|_| {
        Status::invalid_argument(format!(
            "instance_dir does not resolve to an existing path: '{}'",
            candidate.display()
        ))
    })?;

    if !canonical.is_dir() {
        return Err(Status::invalid_argument(format!(
            "instance_dir is not a directory: '{}'",
            canonical.display()
        )));
    }

    if !policy.permits(&canonical) {
        return Err(Status::permission_denied(format!(
            "instance_dir_not_permitted: '{}' is outside permitted roots",
            canonical.display()
        )));
    }

    let mut artifacts = Vec::new();
    let mut error_message = String::new();
    match std::fs::read_dir(&canonical) {
        Ok(entries) => {
            let mut files: Vec<_> = entries
                .flatten()
                .filter(|entry| entry.path().is_file())
                .collect();
            files.sort_by_key(|entry| entry.file_name());
            for entry in files {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                let metadata = entry.metadata().ok();
                let size_bytes = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
                let modified_at = metadata
                    .and_then(|m| m.modified().ok())
                    .map(|t| {
                        let datetime: chrono::DateTime<chrono::Utc> = t.into();
                        datetime.to_rfc3339()
                    })
                    .unwrap_or_default();
                // A file that can't be read as UTF-8 text (binary, permission
                // trouble) gets an empty preview — fail-open, never an error
                // for a single file among many.
                let preview = std::fs::read_to_string(&path)
                    .map(|content| content.chars().take(500).collect::<String>())
                    .unwrap_or_default();
                artifacts.push(anvil_engine::proto::InstanceArtifact {
                    name,
                    size_bytes,
                    preview,
                    modified_at,
                });
            }
        }
        Err(e) => {
            error_message = format!("could not read instance directory: {e}");
        }
    }

    Ok(anvil_engine::proto::ListInstanceArtifactsResponse {
        instance_dir: canonical.display().to_string(),
        artifacts,
        error_message,
    })
}

/// Cap on `ReadInstanceArtifact` content: a generous 2 MiB. Anything larger is
/// truncated to exactly this many bytes and `truncated=true` is set —
/// `size_bytes` still reports the file's real on-disk size.
const MAX_ARTIFACT_READ_BYTES: u64 = 2 * 1024 * 1024;

/// The shared read used by BOTH the gRPC `ReadInstanceArtifact` RPC and the
/// HTTP `/ws` JSON-RPC `read_instance_artifact` method: companion to
/// `compute_list_instance_artifacts` — that fold lists a live instance's
/// files with a short preview each; this fold reads ONE named file's FULL
/// content (capped at `MAX_ARTIFACT_READ_BYTES`) so the Atlas live-agent
/// artifact explorer can render (and poll) the real in-progress work.
///
/// FAIL-CLOSED on path escape: `instance_dir` is validated with the SAME
/// `HearthPolicy` guard `compute_list_instance_artifacts` uses. `name` must
/// additionally be a single flat path component (no `..`, no path
/// separators, no absolute path) naming a file directly inside the
/// canonicalized `instance_dir` — track/proposal/milestone/decision
/// directories are flat, so a legitimate name never needs to nest. Either
/// violation is refused with `PermissionDenied`.
///
/// FAIL-OPEN on ordinary read trouble: a missing, unreadable, or non-file
/// target under an otherwise-permitted directory yields an empty `content`
/// and a non-empty `error_message`, never a hard error — mirrors the
/// per-file fail-open behavior in `compute_list_instance_artifacts`.
fn compute_read_instance_artifact(
    instance_dir: &str,
    name: &str,
    policy: &HearthPolicy,
) -> Result<anvil_engine::proto::ReadInstanceArtifactResponse, Status> {
    let candidate = Path::new(instance_dir);
    let canonical = std::fs::canonicalize(candidate).map_err(|_| {
        Status::invalid_argument(format!(
            "instance_dir does not resolve to an existing path: '{}'",
            candidate.display()
        ))
    })?;

    if !canonical.is_dir() {
        return Err(Status::invalid_argument(format!(
            "instance_dir is not a directory: '{}'",
            canonical.display()
        )));
    }

    if !policy.permits(&canonical) {
        return Err(Status::permission_denied(format!(
            "instance_dir_not_permitted: '{}' is outside permitted roots",
            canonical.display()
        )));
    }

    // FAIL-CLOSED: `name` must resolve to exactly one flat, normal path
    // component — rejects `..`, embedded separators (`sub/dir/file`), and
    // absolute paths in one check, so this RPC can never be used to read a
    // file outside `instance_dir`.
    let name_path = Path::new(name);
    let is_single_normal_component = {
        let mut components = name_path.components();
        matches!(components.next(), Some(std::path::Component::Normal(_))) && components.next().is_none()
    };
    if name.is_empty() || !is_single_normal_component {
        return Err(Status::permission_denied(format!(
            "artifact name is not permitted: '{}'",
            name
        )));
    }

    let target = canonical.join(name);

    let mut content = String::new();
    let mut size_bytes: u64 = 0;
    let mut modified_at = String::new();
    let mut truncated = false;
    let mut error_message = String::new();

    match std::fs::metadata(&target) {
        Ok(metadata) if metadata.is_file() => {
            size_bytes = metadata.len();
            modified_at = metadata
                .modified()
                .ok()
                .map(|t| {
                    let datetime: chrono::DateTime<chrono::Utc> = t.into();
                    datetime.to_rfc3339()
                })
                .unwrap_or_default();

            match std::fs::read(&target) {
                Ok(bytes) => {
                    if (bytes.len() as u64) > MAX_ARTIFACT_READ_BYTES {
                        truncated = true;
                    }
                    let cap = MAX_ARTIFACT_READ_BYTES as usize;
                    let capped = if bytes.len() > cap { &bytes[..cap] } else { &bytes[..] };
                    content = String::from_utf8_lossy(capped).into_owned();
                }
                Err(e) => {
                    error_message = format!("could not read artifact '{}': {e}", name);
                }
            }
        }
        Ok(_) => {
            error_message = format!("'{}' is not a file", name);
        }
        Err(e) => {
            error_message = format!("could not stat artifact '{}': {e}", name);
        }
    }

    Ok(anvil_engine::proto::ReadInstanceArtifactResponse {
        name: name.to_string(),
        content,
        size_bytes,
        modified_at,
        truncated,
        error_message,
    })
}

/// Read ONE artifact directory into the pure fold's input: its `kind`,
/// `parent_id`, `actors:` table, FOLDED current state, and FOLDED transition
/// history.
///
/// The history is resolved through the one transition-log seam
/// (`read_event_files` + `fold_transitions` / `fold_state`), which merges the
/// authoritative per-file event store under `<dir>/transitions/` with the legacy
/// `status.yaml` array. This function must never fold that itself — a second
/// fold is a second opinion about what happened.
///
/// `None` on any read/parse trouble, so the caller can SKIP a damaged child
/// rather than fail the whole record (mirrors the fail-open posture of the
/// other Atlas reads).
fn read_run_detail_input(
    dir: &Path,
    instance_id: &str,
) -> Option<anvil_core::domain::run_detail::RunDetailInput> {
    use anvil_core::domain::transition_log::{fold_state, fold_transitions, read_event_files};

    let content = std::fs::read_to_string(dir.join("status.yaml")).ok()?;
    let status = serde_yaml::from_str::<FullStatusYaml>(&content).ok()?;
    // ONE read of the event directory, folded twice. `resolve_transitions_with_events`
    // + `resolve_state_with_events` would each re-list `transitions/`, which is a
    // second syscall walk per artifact on a query that already walks every
    // artifact in the hearth.
    let events = read_event_files(dir).ok()?;
    Some(anvil_core::domain::run_detail::RunDetailInput {
        instance_id: instance_id.to_string(),
        kind: status.kind.clone().unwrap_or_default(),
        state: fold_state(&status, &events).unwrap_or_default(),
        artifact_dir: dir.display().to_string(),
        parent_id: status.parent_id.clone().unwrap_or_default(),
        transitions: fold_transitions(
            &anvil_core::domain::transition_log::resolve_transitions(&status),
            &events,
        ),
        actors: status.actors.clone(),
    })
}

/// The case a playbook makes for a rung, for one playbook KIND in one hearth.
///
/// Served over `/ws` so a screen can ASK for it. A fold with no caller answers
/// nothing, and the point of this substrate is that the four facts the autonomy
/// ladder promises become askable — three of them; the fourth is a cost and this
/// engine refuses to state one (see `proto/anvil.proto`).
///
/// THE SHAPING IS `anvil_core::domain::autonomy_evidence::fold_autonomy_evidence`
/// and nothing here duplicates it. This function performs the I/O: it locates
/// every artifact of the requested kind, folds each one's history through the one
/// transition-log seam, asks the playbook MACHINE whether the run's state is
/// terminal, and counts its corrections with the one shared counter. `clean` is
/// decided in the fold, from those two inputs, and is never decided here.
///
/// `None` for a kind nobody has run. NOT an empty record: a playbook with no runs
/// has no case, and `0 clean of 0 attempted` reads as a measured perfect failure
/// rather than as the absence of evidence — which is the exact confusion the
/// ladder's first beat ("the case is on the screen BEFORE the act") exists to
/// prevent.
///
/// NO COST FIGURE. The record carries none and
/// `anvil-core/features/autonomy_evidence.feature` fails on any cost-shaped key
/// anywhere in it, so this response cannot acquire one quietly.
fn compute_autonomy_evidence(
    req_hearth_path: &str,
    kind: &str,
    default_hearth: Option<&Path>,
    hearth_policy: &HearthPolicy,
    global_playbooks_hearth: Option<&Path>,
) -> Result<(String, Option<anvil_core::domain::autonomy_evidence::AutonomyEvidence>), Status> {
    use anvil_core::domain::autonomy_evidence::{
        count_revision_cycles,
        fold_autonomy_evidence,
        EvidenceRunInput,
    };
    use anvil_core::domain::route::state_is_terminal;

    if kind.trim().is_empty() {
        return Err(Status::invalid_argument(
            "kind_required: name the playbook whose case you are asking for",
        ));
    }

    let resolved = resolve_hearth(req_hearth_path, default_hearth, hearth_policy)?;
    let resolved_hearth = resolved.display().to_string();
    let registry = request_session_registry(&resolved, global_playbooks_hearth);

    let type_dirs = match std::fs::read_dir(&resolved) {
        Ok(type_dirs) => type_dirs,
        // FAIL-OPEN on ordinary read trouble, exactly as `run_detail` does: an
        // unreadable hearth yields no case, never a fabricated one.
        Err(_) => return Ok((resolved_hearth, None)),
    };

    let mut runs: Vec<EvidenceRunInput> = Vec::new();
    for type_entry in type_dirs.flatten() {
        let type_dir = type_entry.path();
        if !type_dir.is_dir() {
            continue;
        }
        let Ok(artifact_dirs) = std::fs::read_dir(&type_dir) else {
            continue;
        };
        for artifact_entry in artifact_dirs.flatten() {
            let dir = artifact_entry.path();
            if !dir.is_dir() {
                continue;
            }
            let id = artifact_entry.file_name().to_string_lossy().to_string();
            let Some(input) = read_run_detail_input(&dir, &id) else {
                continue;
            };
            if input.kind != kind {
                continue;
            }
            runs.push(EvidenceRunInput {
                instance_id: input.instance_id,
                // The MACHINE decides what terminal means. This function asks it
                // rather than pattern-matching a state name, which would be a
                // second opinion about a question the machine owns.
                reached_terminal: state_is_terminal(registry.as_ref(), kind, &input.state),
                revision_cycles: count_revision_cycles(&input.transitions),
                transitions: input.transitions,
                actors: input.actors,
            });
        }
    }

    let evidence = fold_autonomy_evidence(&runs);
    tracing::info!(
        command = "autonomy_evidence",
        hearth = %resolved_hearth,
        kind = kind,
        runs = runs.len(),
        found = evidence.is_some(),
        outcome = "ok",
        "autonomy_evidence query ok"
    );
    Ok((resolved_hearth, evidence))
}

/// The shared read used by BOTH the gRPC `RunDetail` RPC and the HTTP `/ws`
/// JSON-RPC `run_detail` method: one run's own folded record plus the runs that
/// were started from inside it.
///
/// The Playbooks depth panel needs a run's real steps — each with when it
/// happened, who took it, the part they were playing, and (where one was
/// required) who approved it. It must not re-derive them: two surfaces each
/// folding a run's history is how two surfaces come to disagree about the same
/// artifact. Both surfaces call THIS function, and the shaping below the I/O is
/// the pure `anvil_core::domain::run_detail::fold_run_detail`.
///
/// NO COST FIGURE EXISTS IN ANVIL, so the response carries none. Nothing here
/// records what a step cost — not `status.yaml`, not a transition event, not the
/// measurement sink — and a zero would be a fabricated number that reads as a
/// step that was free.
///
/// FAIL-CLOSED on path escape: `resolve_hearth` applies the permitted-root
/// policy, the instance is located by SCANNING the hearth's per-kind
/// subdirectories for a directory whose NAME is `instance_id` (the caller never
/// supplies a path — same two-level walk shape as `read_hearth_actor_streams`),
/// and the located directory is re-checked against the SAME `HearthPolicy` so a
/// symlinked artifact directory pointing out of the hearth is refused.
/// FAIL-OPEN on ordinary read trouble: an unlistable hearth answers
/// `found=false` with an honest `error_message`; an unreadable or unparseable
/// CHILD artifact is skipped with a warning rather than sinking the read. An id
/// that matches no directory is `found=false` with NO error — that is an answer,
/// not a failure.
fn compute_run_detail(
    req_hearth_path: &str,
    instance_id: &str,
    default_hearth: Option<&Path>,
    hearth_policy: &HearthPolicy,
) -> Result<anvil_engine::proto::RunDetailResponse, Status> {
    use anvil_core::domain::run_detail::{fold_run_detail, RunDetailInput};

    let resolved = resolve_hearth(req_hearth_path, default_hearth, hearth_policy)?;
    let resolved_hearth = resolved.display().to_string();
    let empty = |error_message: String| anvil_engine::proto::RunDetailResponse {
        resolved_hearth: resolved_hearth.clone(),
        found: false,
        nodes: Vec::new(),
        error_message,
    };

    let type_dirs = match std::fs::read_dir(&resolved) {
        Ok(type_dirs) => type_dirs,
        Err(e) => return Ok(empty(format!("could not read hearth: {e}"))),
    };

    let mut root: Option<RunDetailInput> = None;
    let mut children: Vec<RunDetailInput> = Vec::new();
    for type_entry in type_dirs.flatten() {
        let type_dir = type_entry.path();
        if !type_dir.is_dir() {
            continue;
        }
        let Ok(artifact_dirs) = std::fs::read_dir(&type_dir) else {
            continue;
        };
        for artifact_entry in artifact_dirs.flatten() {
            let dir = artifact_entry.path();
            if !dir.is_dir() {
                continue;
            }
            let id = artifact_entry.file_name().to_string_lossy().to_string();
            let is_root = id == instance_id;
            // Only parse what can end up in the record: the run itself, or an
            // artifact naming it as parent.
            let Some(input) = read_run_detail_input(&dir, &id) else {
                if is_root {
                    tracing::warn!(
                        command = "run_detail",
                        instance_id = %id,
                        "run_detail skipped an unreadable artifact"
                    );
                }
                continue;
            };
            if is_root {
                // The ONLY path check the caller can influence: refuse a located
                // directory that canonicalizes outside the permitted roots (a
                // symlink planted inside the hearth).
                let canonical = std::fs::canonicalize(&dir)
                    .map_err(|e| Status::internal(format!("could not resolve '{}': {e}", dir.display())))?;
                if !hearth_policy.permits(&canonical) {
                    return Err(Status::permission_denied(format!(
                        "instance_not_permitted: '{}' is outside permitted roots",
                        canonical.display()
                    )));
                }
                root = Some(input);
            } else if input.parent_id == instance_id {
                children.push(input);
            }
        }
    }

    let Some(root) = root else {
        // A clean answer, not an error: nothing in this hearth carries that id.
        tracing::info!(
            command = "run_detail",
            hearth = %resolved_hearth,
            found = false,
            outcome = "ok",
            "run_detail query ok"
        );
        return Ok(empty(String::new()));
    };

    let nodes: Vec<anvil_engine::proto::RunNode> = fold_run_detail(&root, &children)
        .into_iter()
        .map(|node| anvil_engine::proto::RunNode {
            instance_id: node.instance_id,
            kind: node.kind,
            state: node.state,
            artifact_dir: node.artifact_dir,
            parent_instance_id: node.parent_instance_id,
            depth: node.depth,
            steps: node
                .steps
                .into_iter()
                .map(|step| anvil_engine::proto::RunStep {
                    to_state: step.to_state,
                    at: step.at,
                    actor: step.actor,
                    role: step.role,
                    approver: step.approver,
                    note: step.note,
                    verdict: step.verdict,
                })
                .collect(),
            actors: node
                .actors
                .into_iter()
                .map(|actor| anvil_engine::proto::RunActor {
                    name: actor.name,
                    actor_type: actor.actor_type,
                    model: actor.model,
                    provider: actor.provider,
                })
                .collect(),
        })
        .collect();

    tracing::info!(
        command = "run_detail",
        hearth = %resolved_hearth,
        found = true,
        node_count = nodes.len(),
        outcome = "ok",
        "run_detail query ok"
    );

    Ok(anvil_engine::proto::RunDetailResponse {
        resolved_hearth,
        found: true,
        nodes,
        error_message: String::new(),
    })
}

/// Process entry point. CRITICAL ORDERING INVARIANT (soundness): the DURABLE
/// hearth-local engine flags are merged into the process env HERE — in `main`,
/// BEFORE the Tokio runtime is built or the tracing subscriber is initialized. On
/// Unix `std::env::set_var` is only sound while the process is single-threaded (a
/// concurrent env read from another thread is a data race); the Tokio multi-thread
/// runtime and tracing's background workers both spawn threads. So this is a PLAIN
/// `fn main` (NOT `#[tokio::main]`): we resolve the startup path args with `std`
/// only, `set_var` the overrides while still single-threaded, and ONLY THEN build
/// the runtime and enter the async engine body. Do not move the flags merge below
/// the runtime build, and do not reintroduce `#[tokio::main]` here.
///
/// ARG-VALIDATION PRECEDENCE (parity with the pre-branch `#[tokio::main]` main):
/// `--hearth` is validated FIRST, then `--global-playbooks-hearth` — so when BOTH
/// are invalid the `--hearth` error is the one that surfaces, exactly as before the
/// plain-main restructure. The flags merge only needs to precede RUNTIME
/// construction, so this arg validation soundly runs ahead of it in the original
/// order.
///
/// RUNTIME-BUILD FAILURE (parity with `#[tokio::main]`): the macro `.expect(...)`s
/// the runtime build, so a build failure PANICS rather than returning an `Err`. The
/// `.expect("Failed building the Runtime")` below preserves that exact behavior +
/// message — do not turn it back into a `?`.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    // Resolve the startup path args with `std` only — no runtime, no tracing, no
    // threads yet — so the merge below happens in the single-threaded window.
    // `--hearth` FIRST, then `--global-playbooks-hearth`, to preserve the pre-branch
    // arg-error precedence (hearth error wins when both are invalid).
    let hearth_path = parse_arg(&args, "--hearth")
        .map(PathBuf::from)
        .map(|path| canonicalize_startup_path("--hearth", &path))
        .transpose()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let global_playbooks_hearth = configured_global_playbooks_hearth(&args)
        .map(|path| canonicalize_startup_hearth("--global-playbooks-hearth", &path))
        .transpose()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

    // SINGLE-THREADED WINDOW — merge DURABLE hearth-local engine flags into the
    // process env NOW, before any thread exists. Overrides from
    // `<global_playbooks_hearth>/engine-flags.env` are `set_var`'d (real env wins,
    // file beats defaults) so every in-process reader
    // (`enforce_measurement_definition`, `semantic_route`) sees the merged value.
    // This keeps Nick's LOCAL router opt-ins surviving every kit update — the
    // manifest engine.env is wiped on `update_kit`, but the hearth file is not.
    // Fail-open. The returned outcome is logged after tracing init inside `run`.
    let installed_flags =
        anvil_engine::engine_flags::install_hearth_local_flags(global_playbooks_hearth.as_deref());

    // Now it is safe to spawn threads: build the multi-thread runtime and run the
    // engine body. `enable_all()` AND the panic-on-build-failure `.expect(...)` below
    // match the previous `#[tokio::main]` expansion exactly (the macro `.expect`s the
    // build, so a failure PANICS — it never returned an `Err` here).
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed building the Runtime");
    runtime.block_on(run(
        args,
        hearth_path,
        global_playbooks_hearth,
        installed_flags,
    ))
}

/// The async engine body, entered from `main` AFTER the durable flags merge has
/// already `set_var`'d its overrides in the single-threaded window. `args` and the
/// already-resolved `hearth_path` + `global_playbooks_hearth` are threaded in so
/// they are not re-parsed (and so their arg-validation precedence stays in `main`);
/// `installed_flags` is logged once the tracing subscriber is up.
async fn run(
    args: Vec<String>,
    hearth_path: Option<PathBuf>,
    global_playbooks_hearth: Option<PathBuf>,
    installed_flags: anvil_engine::engine_flags::ResolvedFlags,
) -> Result<(), Box<dyn std::error::Error>> {
    let permitted_roots = configured_permitted_roots(&args);
    let hearth_policy = HearthPolicy::new(
        hearth_path.as_deref(),
        global_playbooks_hearth.as_deref(),
        permitted_roots,
    )
    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

    let port: u16 = parse_arg(&args, "--port")
        .and_then(|s| s.parse().ok())
        .unwrap_or(50051);

    // Initialize the structured JSON tracing subscriber BEFORE serving (Req 1).
    // - JSON output to stderr for stable, machine-parseable, field-matchable logs.
    // - `.flatten_event(true)` puts event fields at the top level so the test
    //   matcher reads top-level keys (matcher key-path decision, C3/T1.2).
    // - Level controlled by the single `ANVIL_LOG` env var with a quiet default
    //   (`info` for the engine, `warn` for everything else). One init per process
    //   (the engine is always a subprocess), never per-RPC.
    let env_filter = tracing_subscriber::EnvFilter::try_from_env("ANVIL_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn,anvil_engine=info"));
    tracing_subscriber::fmt()
        .json()
        .flatten_event(true)
        .with_writer(std::io::stderr)
        .with_env_filter(env_filter)
        .init();

    // The DURABLE hearth-local engine flags were already merged into the process env
    // in `main`, BEFORE this runtime/subscriber existed (soundness: `set_var` must
    // run single-threaded). Now that tracing is up, emit the one INFO/WARN line
    // describing what the merge applied/ignored.
    anvil_engine::engine_flags::log_installed_flags(&installed_flags);

    start_parent_exit_watcher_from_env();

    let addr: std::net::SocketAddr = format!("127.0.0.1:{}", port).parse()?;
    // Decide the session-enforcement mode + verifier ONCE at startup from the
    // engine's own env (D1). Independent of HearthLocks.
    let (mode, verifier) = detect_mode_and_verifier();

    // The HTTP bridge (/ws + /health) shares the engine's hearth-resolution
    // config. Build its read context BEFORE moving the owned fields into the
    // gRPC server (clones — the bridge is a pure read surface).
    let ws_state = ws_bridge::WsBridgeState {
        default_hearth: hearth_path.clone(),
        global_playbooks_hearth: global_playbooks_hearth.clone(),
        hearth_policy: hearth_policy.clone(),
    };

    let server = AnvilServer {
        hearth_path,
        global_playbooks_hearth,
        hearth_policy,
        hearth_locks: HearthLocks::new(),
        mode,
        verification_cache: VerificationCache::new(verifier),
        step_measurement_dispatcher: StepMeasurementDispatcher::filesystem(),
        publication_log: PublicationLog::from_env(),
        open_marker_index: OpenMarkerIndex::new(),
        nudge_dedup: NudgeDedup::new(),
    };

    // Multiplex gRPC (HTTP/2 h2c, prior-knowledge) and the HTTP/1.1 bridge
    // (/ws + /health) on ONE TcpListener. `tonic::service::Routes` is an
    // `axum::Router` underneath, so the gRPC service merges directly into the
    // bridge router; axum's `serve` drives connections through hyper-util's auto
    // builder, which sniffs HTTP/1.1 vs HTTP/2-prior-knowledge per connection.
    // Preserves the prior `serve_with_shutdown` graceful-shutdown behavior.
    let grpc_router =
        tonic::service::Routes::new(AnvilServiceServer::new(server)).into_axum_router();
    let app = ws_bridge::router(ws_state).merge(grpc_router);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    let _hook_install_task = anvil_engine::startup_hooks::spawn_self_install_hooks_from_env(None);

    // Foundry rendezvous publication. The listener is up and the port is known,
    // so advertise the engine's real address at `~/.anvil/engine.json` for
    // Foundry + the kit's MCP/CLI to discover. The `/health` path served by the
    // HTTP bridge (ws_bridge::router, multiplexed onto this same listener) makes
    // the standard http-url + /health probe work even though this is a gRPC
    // engine. Warn-on-error, never crash the engine over rendezvous I/O.
    let rendezvous_disabled = rendezvous_disabled();
    let rendezvous_dir = if rendezvous_disabled {
        None
    } else {
        anvil_rendezvous_dir()
    };
    if !rendezvous_disabled {
        if let Some(dir) = rendezvous_dir.as_deref() {
            let started_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let record = foundry_engine_addressing::RendezvousRecord::new(
                format!("http://127.0.0.1:{}", port),
                env!("CARGO_PKG_VERSION").to_string(),
                started_at,
            );
            if let Err(e) = foundry_engine_addressing::publish(dir, &record) {
                tracing::warn!(error = %e, dir = %dir.display(), "failed to publish Foundry rendezvous record");
            }
        } else {
            tracing::warn!("could not resolve home dir; skipping Foundry rendezvous publication");
        }
    }

    // Readiness marker: the listener is bound and we are about to enter the accept
    // loop, so `/health` and gRPC are now serviceable. Emit this BEFORE blocking on
    // `axum::serve` so the last startup log line is "serving on <port>" — not the
    // fire-and-forget self-install task (`spawn_self_install_hooks_from_env`), which
    // previously left an idle-looking engine whose final log implied it had stalled.
    tracing::info!(port, "anvil-engine serving");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    // Graceful shutdown drained: remove the rendezvous record so stale addresses
    // never linger. Best-effort (cleanup itself swallows errors).
    if !rendezvous_disabled {
        if let Some(dir) = rendezvous_dir.as_deref() {
            foundry_engine_addressing::cleanup(dir);
        }
    }

    Ok(())
}

/// Resolve `~/.anvil` — the Foundry rendezvous data dir for this kit. Returns
/// `None` only when the home directory can't be resolved from `$HOME`.
fn anvil_rendezvous_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".anvil"))
}

fn rendezvous_disabled() -> bool {
    std::env::var("ANVIL_RENDEZVOUS_DISABLE")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Decide the engine's session-enforcement mode + verifier ONCE at startup from
/// the engine's own environment (D1).
///
/// `FOUNDRY_SESSION_TOKEN` selects Foundry; absent or whitespace-only selects
/// Standalone. The trim lives here so a whitespace-only token never causes a
/// Foundry-mode refusal (spec Req 1, J5).
///
/// Verifier (J4 — concrete `EngineVerifier`, no generics on `AnvilServer`):
/// - Standalone ⇒ `EngineVerifier::Standalone` (verify => Ok(None));
/// - Foundry + a test stub requested via `ANVIL_TEST_SESSION_VERIFIER`
///   (debug builds only) ⇒ the hermetic `EngineVerifier::Stub`;
/// - Foundry on Unix + `foundry-session` ⇒ the production
///   `EngineVerifier::Broker` (delegates to the foundry helper, honoring the
///   forwarded bearer token);
/// - Foundry without a broker compiled in (feature-off / non-unix) ⇒ falls
///   back to `Standalone` (nothing to verify against).
fn detect_mode_and_verifier() -> (EngineMode, EngineVerifier) {
    let token_present = std::env::var("FOUNDRY_SESSION_TOKEN")
        .map(|t| !t.trim().is_empty())
        .unwrap_or(false);

    if !token_present {
        return (EngineMode::Standalone, EngineVerifier::Standalone);
    }

    // Foundry mode. Select the verifier.
    #[cfg(debug_assertions)]
    {
        if let Ok(kind) = std::env::var("ANVIL_TEST_SESSION_VERIFIER") {
            use anvil_engine::session::StubSessionVerifier;
            let stub = match kind.as_str() {
                "stub_reject" => Some(StubSessionVerifier::Reject),
                "stub_unreachable" => Some(StubSessionVerifier::Unreachable),
                "stub_accept" => Some(StubSessionVerifier::Accept {
                    // The accept-path stub binds to the sub supplied via a
                    // companion env var (D4); defaults to a fixed test sub.
                    sub: std::env::var("ANVIL_TEST_SESSION_SUB")
                        .unwrap_or_else(|_| "user-stub".to_string()),
                }),
                _ => None,
            };
            if let Some(stub) = stub {
                return (EngineMode::Foundry, EngineVerifier::Stub(stub));
            }
        }
    }

    #[cfg(all(unix, feature = "foundry-session"))]
    {
        use anvil_engine::session::BrokerSessionVerifier;
        use std::sync::Arc;
        // anvil's kit id; the broker derives the audience as
        // `foundry-mcp:anvil-kit` (matches EXPECTED_AUDIENCE).
        return (
            EngineMode::Foundry,
            EngineVerifier::Broker(Arc::new(BrokerSessionVerifier::new("anvil-kit"))),
        );
    }

    // Foundry token present but no broker verifier compiled in (feature-off /
    // non-unix). Nothing to verify against, so operate standalone.
    #[cfg(not(all(unix, feature = "foundry-session")))]
    {
        (EngineMode::Standalone, EngineVerifier::Standalone)
    }
}

#[cfg(unix)]
fn start_parent_exit_watcher_from_env() {
    let Ok(raw_pid) = std::env::var(ENGINE_PARENT_PID_ENV) else {
        return;
    };
    let Ok(parent_pid) = raw_pid.parse::<libc::pid_t>() else {
        eprintln!(
            "anvil-engine: ignoring invalid {}={:?}",
            ENGINE_PARENT_PID_ENV, raw_pid
        );
        return;
    };
    if parent_pid <= 1 {
        return;
    }

    std::thread::spawn(move || {
        wait_for_parent_exit(parent_pid);
        std::process::exit(0);
    });
}

#[cfg(not(unix))]
fn start_parent_exit_watcher_from_env() {}

#[cfg(target_os = "macos")]
fn wait_for_parent_exit(parent_pid: libc::pid_t) {
    unsafe {
        let kq = libc::kqueue();
        if kq == -1 {
            poll_for_parent_exit(parent_pid);
            return;
        }

        let mut change: libc::kevent = std::mem::zeroed();
        change.ident = parent_pid as libc::uintptr_t;
        change.filter = libc::EVFILT_PROC;
        change.flags = libc::EV_ADD | libc::EV_ENABLE;
        change.fflags = libc::NOTE_EXIT;

        if libc::kevent(kq, &change, 1, std::ptr::null_mut(), 0, std::ptr::null()) == -1 {
            let _ = libc::close(kq);
            if parent_is_gone(parent_pid) {
                return;
            }
            poll_for_parent_exit(parent_pid);
            return;
        }

        let mut event: libc::kevent = std::mem::zeroed();
        let _ = libc::kevent(kq, std::ptr::null(), 0, &mut event, 1, std::ptr::null());
        let _ = libc::close(kq);
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn wait_for_parent_exit(parent_pid: libc::pid_t) {
    poll_for_parent_exit(parent_pid);
}

#[cfg(unix)]
fn poll_for_parent_exit(parent_pid: libc::pid_t) {
    loop {
        if parent_is_gone(parent_pid) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

#[cfg(unix)]
fn parent_is_gone(parent_pid: libc::pid_t) -> bool {
    let missing = unsafe { libc::kill(parent_pid, 0) != 0 };
    missing && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

/// Resolve when the engine should shut down gracefully: on Ctrl-C (SIGINT) or
/// SIGTERM. Foundry stops/restarts the supervised engine with these signals;
/// awaiting either lets `serve_with_shutdown` drain in-flight RPCs, release the
/// listening port, and return so `main` exits cleanly (code 0). Without this
/// the engine blocks forever on `.serve(addr)` and the default signal
/// disposition kills it (non-zero, signal-terminated) — not supervisable.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}

fn checkin_error_to_status(error: CheckinError) -> Status {
    match error {
        CheckinError::UnsupportedType { .. } | CheckinError::UnsupportedRole { .. } => {
            Status::invalid_argument(format!("{}", error))
        }
        CheckinError::ParentNotFound { .. } => Status::not_found(format!("{}", error)),
        CheckinError::ParentNotActive { .. } => Status::failed_precondition(format!("{}", error)),
        CheckinError::IoError { .. } | CheckinError::MalformedStatus { .. } => {
            Status::internal(format!("{}", error))
        }
        CheckinError::ParentKindInvalid { .. } => Status::failed_precondition(format!("{}", error)),
        // A missing machine-required input on an otherwise-valid create is a
        // precondition failure (matches the ParentNotActive/ParentKindInvalid
        // create-time gate precedent). (Anvil-lane 1b.)
        CheckinError::MissingRequiredField { .. } => {
            Status::failed_precondition(format!("{}", error))
        }
    }
}

fn describe_error_to_status(error: DescribeError) -> Status {
    match error {
        DescribeError::UnknownIdentifier { .. } => Status::not_found(format!("{}", error)),
        DescribeError::IoError { .. } => Status::internal(format!("{}", error)),
    }
}

fn append_spark_source_event(
    hearth: &Path,
    body: &str,
    actor: &ActorIdentity,
    at: &str,
) -> Result<(), BeginError> {
    let sparks_dir = hearth.join("sparks");
    std::fs::create_dir_all(&sparks_dir).map_err(|e| BeginError::IoError {
        message: format!("Failed to create sparks directory: {}", e),
    })?;
    let path = sparks_dir.join("sparks.md");
    // ── C-d.1 round 8, H-3 ──
    //
    // This was `read_to_string(&path).unwrap_or_default()` feeding an
    // `atomic_write` of the WHOLE file. Measured on unmutated 63df2ff:
    //
    //   sparks.md 0644 : read ok      atomic_write Ok  -> 3 sparks survive (control)
    //   sparks.md 0200 : read FAILED  atomic_write Ok  -> 1 spark  survives
    //   sparks.md 0000 : read FAILED  atomic_write Ok  -> 1 spark  survives
    //
    // The entire accumulated spark log replaced by the single new entry, from an
    // ordinary `begin` carrying a spark body.
    //
    // **`atomic_write` AMPLIFIES this class and that is worth stating on its
    // own.** Temp-write-then-rename needs only the DIRECTORY to be writable, so
    // a read-then-atomic-write pair loses data at modes where a plain
    // `fs::write` would have refused — including 0000. Every such pair in this
    // tree inherits that, which is why the read has to refuse rather than
    // relying on the write to fail.
    let existing = match std::fs::read_to_string(&path) {
        Ok(body) => body,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            return Err(BeginError::IoError {
                message: format!(
                    "spark_source_uninspectable: {} exists and could not be read: {e}. Refusing \
                     to append. Reading it as empty and atomic-writing the result replaces every \
                     accumulated spark with this one.",
                    path.display()
                ),
            })
        }
    };
    let spark_body = body.trim();
    let heading = spark_body
        .lines()
        .next()
        .unwrap_or("untitled spark")
        .trim()
        .replace(['\r', '\n'], " ");
    let id = spark_id(spark_body, actor, at);
    let entry = format!(
        "## spark: {}\nid: {}\nat: {}\nactor: {}\n\n{}\n",
        heading, id, at, actor.name, spark_body
    );
    let combined = if existing.trim().is_empty() {
        entry
    } else if existing.ends_with("\n\n") {
        format!("{}{}", existing, entry)
    } else if existing.ends_with('\n') {
        format!("{}\n{}", existing, entry)
    } else {
        format!("{}\n\n{}", existing, entry)
    };
    anvil_core_hearth::atomic_write::atomic_write(&path, combined.as_bytes()).map_err(|e| {
        BeginError::IoError {
            message: format!("Failed to append spark event: {}", e),
        }
    })
}

fn spark_id(body: &str, actor: &ActorIdentity, at: &str) -> String {
    let mut hasher = DefaultHasher::new();
    body.hash(&mut hasher);
    actor.name.hash(&mut hasher);
    at.hash(&mut hasher);
    let now = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
    now.hash(&mut hasher);
    format!("spark-{:016x}", hasher.finish())
}

/// Map a begin `Event` to its variant name for logging (Req 2, Req 6 — names
/// only, never the `note`/`header`/payload contents).
fn begin_event_name(event: &Event) -> &'static str {
    match event {
        Event::ReviewTransition { .. } => "ReviewTransition",
        Event::ArtifactAdopted { .. } => "ArtifactAdopted",
        Event::ReviewDocCreated { .. } => "ReviewDocCreated",
        Event::BeginMarkerWritten { .. } => "BeginMarkerWritten",
        // The create event is now the generalized `ArtifactCreation`. Preserve
        // the historical logged name for the track encoding (the
        // cqrs_logging_begin oracle pins "TrackCreation") and emit a
        // kind-appropriate name for other machines. Names are interned to
        // `&'static str` for the small set of kinds the engine drives today.
        Event::BacklogItemCreation { .. } => "BacklogItemCreation",
        Event::ArtifactCreation { status, .. } => match status.kind.as_str() {
            "track" => "TrackCreation",
            "playbook" => "PlaybookCreation",
            "knowledge_lifecycle" => "ArtifactCreation",
            _ => "ArtifactCreation",
        },
        Event::ProjectionOnlySnapshot { .. } => "ProjectionOnlySnapshot",
        Event::PlaybookCreation { .. } => "PlaybookCreation",
    }
}

/// Map a `CompleteEvent` to its variant name for logging (Req 2, Req 6 — names
/// only, never the `note`/reflection `body` contents).
fn complete_event_name(event: &anvil_core::domain::complete_events::CompleteEvent) -> &'static str {
    use anvil_core::domain::complete_events::CompleteEvent;
    match event {
        CompleteEvent::TransitionRecorded { .. } => "TransitionRecorded",
        CompleteEvent::ActorUpserted { .. } => "ActorUpserted",
        CompleteEvent::ReflectionWritten { .. } => "ReflectionWritten",
        CompleteEvent::CarryForwardWritten { .. } => "CarryForwardWritten",
    }
}

fn emit_begin_step_measurement(
    registry: &dyn anvil_core::domain::playbook::registry::PlaybookRegistry,
    kind: &str,
    role: &str,
    to_state: &str,
    track_path: &str,
    actor: &str,
    at: &str,
) {
    use anvil_core::domain::playbook::interpreter::state_role_measurement;

    // playbook_id = the per-RUN instance id (the run/artifact dir id), NOT the
    // playbook definition id (H1) — it threads one run's steps. track_id = the
    // playbook KIND: the Scorecard B aggregation key across instances. They are
    // DIFFERENT values; the registry's `playbook_id_for(kind)` (the definition
    // id) is intentionally NOT used for playbook_id.
    let playbook_id = last_segment(track_path);
    let track_id = kind;
    let (intent, expected_output) = registry
        .machine_for(kind)
        .and_then(|machine| state_role_measurement(machine, to_state, role))
        .map(|m| (m.intent.clone(), m.expected_output.clone()))
        .unwrap_or_default();
    emit_step_measurement(
        playbook_id,
        track_id,
        "",
        to_state,
        role,
        actor,
        &intent,
        &expected_output,
        at,
    );
}

/// Emit one flat `event_kind="step_measurement"` record via the existing
/// tracing sink (M-P3, D-2). Every contract key is present even when its value
/// is empty (AC-6); `tokens`/`duration_ms` are intentionally NOT emitted
/// (absent keys, D-4). All three lifecycle verbs (begin/checkin/complete) route
/// through this single helper so the flat field set stays identical across
/// emit sites. Domain handlers stay pure — this lives at the engine RPC layer.
#[allow(clippy::too_many_arguments)]
fn emit_step_measurement(
    playbook_id: &str,
    track_id: &str,
    from_state: &str,
    to_state: &str,
    role: &str,
    actor: &str,
    intent: &str,
    expected_output: &str,
    at: &str,
) {
    tracing::info!(
        event_kind = "step_measurement",
        playbook_id = %playbook_id,
        track_id = %track_id,
        from_state = %from_state,
        to_state = %to_state,
        role = %role,
        actor = %actor,
        intent = %intent,
        expected_output = %expected_output,
        at = %at,
        "step measurement"
    );
}

/// The id of the decision artifact that gates rich §0 emit. Rich step prose +
/// identities ship ONLY when this decision is `decided` in a checked hearth.
const STEP_MEASUREMENT_EMIT_PRIVACY_DECISION: &str = "step-measurement-emit-privacy";

/// Resolve the temper storage home. The §0 stream lives at
/// `<temper_home>/.temper/step-measurements/<kind>/events.jsonl`. Prefer the
/// `ANVIL_TEMPER_HOME` override (tests redirect it into the temp tree); fall back
/// to `$HOME`. `None` only when neither is resolvable — the caller then SKIPS
/// the temper write (fail-open).
fn resolve_temper_home() -> Option<PathBuf> {
    std::env::var_os("ANVIL_TEMPER_HOME")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// Whether the `step-measurement-emit-privacy` decision is `decided` in any of
/// the candidate hearths (the request hearth first, then the global playbooks
/// hearth). The decision's directory id is matched by suffix so the
/// date-prefixed instance (`<ts>_step_measurement_emit_privacy`) resolves. A
/// resolution failure reads as NOT decided (fail-closed on the gate — no rich
/// data leaves until the decision is provably decided).
fn privacy_decision_is_decided(candidate_hearths: &[&Path]) -> bool {
    use anvil_core::ports::snapshot_port::SnapshotPort;
    for hearth in candidate_hearths {
        let decisions_dir = hearth.join("decisions");
        let Ok(entries) = std::fs::read_dir(&decisions_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            // Match the bare decision id or a `<timestamp>_<id-with-underscores>`
            // instance directory (the date-prefixed scaffold form).
            let matches = name == STEP_MEASUREMENT_EMIT_PRIVACY_DECISION
                || name.ends_with(&format!(
                    "_{}",
                    STEP_MEASUREMENT_EMIT_PRIVACY_DECISION.replace('-', "_")
                ))
                || name.ends_with(&format!("_{}", STEP_MEASUREMENT_EMIT_PRIVACY_DECISION));
            if !matches {
                continue;
            }
            let adapter = FileSystemSnapshotAdapter::new((*hearth).to_path_buf());
            let rel = format!("decisions/{}", name);
            if let Ok(state) = adapter.read_artifact_state(&rel) {
                if state == "decided" {
                    return true;
                }
            }
        }
    }
    false
}

/// A process-monotonic counter feeding the §0 `event_seq` uniqueness component.
/// Every distinct transition the engine emits gets a strictly-larger value, so
/// two genuinely-distinct transitions that collapse on (playbook_id, from→to,
/// at, role) — e.g. same whole-second, same role, like two back-to-back
/// same-state snapshots the transition-log fold intentionally preserves as
/// SEPARATE transitions — still receive DISTINCT `event_id`s and are NOT
/// false-deduped (H3).
///
/// ## Idempotency model (M5 — Option B: engine-emits-once)
///
/// The §0 stream's idempotency does NOT rely on write-side replay-dedupe,
/// because the engine emits each LOGICAL transition EXACTLY ONCE — there is no
/// engine path that re-emits the same persisted transition:
///   * begin short-circuits in `origin_turn_hit` (begin.rs) — a re-begin of the
///     same routed turn returns the existing artifact with EMPTY events and
///     never reaches the emit;
///   * complete is gated by `select_edge` (complete.rs) — re-completing a
///     terminal artifact errors out before any event is created;
///   * each snapshot APPENDS a new, distinct transition event the fold keeps
///     separate (two same-second same-state snapshots are two legitimate
///     transitions, NOT one re-emitted — deduping them would lose causal
///     history per `transition_log::fold_transitions`).
/// True at-least-once / crash-redelivery idempotency is TEMPER's job: its
/// store-watcher reads new lines past a per-file cursor (idempotent re-reads —
/// `MEASUREMENT_step_contract.md`). The adapter's `event_id` dedupe therefore
/// stands only as cheap DEFENSE-IN-DEPTH against a literal re-append of an
/// already-built event object; it is not the load-bearing guarantee.
static STEP0_EMIT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Mint the next uniqueness component for a freshly-built §0 event. Combines a
/// nanosecond wall-clock reading with a process-monotonic counter so the value
/// is unique even for two transitions minted inside the same nanosecond.
fn next_step0_seq() -> String {
    let n = STEP0_EMIT_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}-{}", nanos, n)
}

/// Build the full §0 event from the resolved transition fields, applying the
/// redaction policy. Per the `step-measurement-emit-privacy` decision, `tokens`
/// is REDACTED — always omitted from the temper stream regardless of whether the
/// runtime surfaced usage. `duration_ms` is permitted (carried when surfaced).
/// `seq` is the per-emit uniqueness component (see `next_step0_seq`): it makes
/// two genuinely-distinct same-second/same-role transitions carry DISTINCT
/// `event_id`s (no false-dedupe), while a literal replay of the SAME built event
/// re-uses the same `seq` and so dedupes at the sink (H3).
#[allow(clippy::too_many_arguments)]
fn build_step0_event(
    playbook_id: &str,
    kind: &str,
    from_state: &str,
    to_state: &str,
    role: &str,
    actor: &str,
    intent: &str,
    expected_output: &str,
    at: &str,
    model: &str,
    tokens: Option<u64>,
    duration_ms: Option<u64>,
    seq: &str,
) -> anvil_core::ports::step_measurement_stream_port::Step0Event {
    use anvil_core::ports::step_measurement_stream_port::Step0Event;
    // === Redaction policy (step-measurement-emit-privacy) ===
    // `tokens` is redacted: the field is OMITTED from the stream regardless of
    // availability. `duration_ms` is permitted.
    let _ = tokens; // policy-redacted; never written.
    Step0Event {
        // playbook_id = the per-RUN instance id (threads one run's steps); the
        // KIND lives in track_id (the Scorecard B aggregation key). They are
        // DIFFERENT values (H1).
        playbook_id: playbook_id.to_string(),
        track_id: kind.to_string(),
        from_state: from_state.to_string(),
        to_state: to_state.to_string(),
        role: role.to_string(),
        actor: actor.to_string(),
        intent: intent.to_string(),
        expected_output: expected_output.to_string(),
        at: at.to_string(),
        model: model.to_string(),
        tokens: None,
        duration_ms,
        // Real uniqueness component (H3): a nanosecond + monotonic-counter value
        // minted once per distinct emit. Distinct transitions ⇒ distinct seq ⇒
        // distinct event_id (no false-dedupe). A replay re-appends the SAME built
        // event (same seq) ⇒ same event_id ⇒ deduped at the sink.
        event_seq: seq.to_string(),
    }
}

fn routing_input(message: &str, signal: &str) -> String {
    if signal.is_empty() {
        message.to_string()
    } else {
        format!("{} {}", message, signal)
    }
}

/// Build the ≤3 [`CandidateBrief`]s the semantic Route RPC feeds Kiln from the
/// lexical resolver's MATCHING set (fidelity constraint: the SAME floor-filtered
/// candidates the hook passes, never the granted flood). Mirrors the hook's brief
/// construction — kind + the routing-grade description (`route.description` when
/// present, else the machine description) + required field names + triggers. The
/// annotation fields (intent / step_outline / why_fits) are left empty: the router
/// prompt reads only kind + description.
fn build_candidate_briefs(matching: &[String], registry: &dyn PlaybookRegistry) -> Vec<CandidateBrief> {
    matching
        .iter()
        .filter_map(|kind| {
            registry.machine_for(kind).map(|m| {
                let description = m
                    .route
                    .description
                    .as_deref()
                    .filter(|d| !d.trim().is_empty())
                    .unwrap_or(&m.description)
                    .to_string();
                CandidateBrief {
                    kind: m.kind.clone(),
                    description,
                    route_triggers: m.route.triggers.clone(),
                    required_fields: m.required_fields.iter().map(|f| f.name.clone()).collect(),
                    intent: String::new(),
                    step_outline: Vec::new(),
                    why_fits: String::new(),
                }
            })
        })
        .collect()
}

fn route_resolution_outcome_label(outcome: &RouteOutcome) -> &'static str {
    match outcome {
        RouteOutcome::Single => "single",
        RouteOutcome::Candidates => "candidates",
        RouteOutcome::NoMatch => "no_match",
    }
}

/// Build the `outcome: "resume"` [`RouteResponse`] for an open (begun,
/// non-terminal) playbook. This is the SINGLE construction shared by BOTH the
/// continuation-token resume pre-check AND the MID_PLAYBOOK_RUN matched-turn
/// check-in nudge (mid_run_checkin_nudge) so the two paths cannot drift:
/// each surfaces the open artifact (id/kind/state), the current `(state, doer)`
/// next-step guidance (served via the SAME hook_serve seam begin/route reuse,
/// fail-open), and the supported advance action. The hook's
/// `RouteTurnOutcome::Resume` renders this identically for both paths.
fn resume_response_for_open_playbook_run(
    registry: &dyn PlaybookRegistry,
    registry_base: &Path,
    registry_hearth_display: &str,
    // A PARAMETER rather than three assignments after the call: every resume
    // return is a route return, the hook persists this field to the delivery
    // log, and a resume path that answered with an empty hash would be an
    // unjoinable row with no visible cause. Passing it in makes a fourth resume
    // site a compile error instead of a silent gap.
    conversation_hash: &str,
    open: OpenPlaybookRun,
) -> RouteResponse {
    let hook_reader = SourceAwarePlaybookHookBodyReader {
        request_hearth: registry_base.to_path_buf(),
    };
    let resume_guidance = serve_hook_body(&hook_reader, registry, &open.kind, &open.state, "doer")
        .unwrap_or_default();
    let advance_action = advance_action_for(registry, &open.kind, &open.state);
    RouteResponse {
        outcome: "resume".to_string(),
        candidates: Vec::new(),
        handoff: String::new(),
        intent: String::new(),
        resolved_hearth: registry_hearth_display.to_string(),
        selected_kind: String::new(),
        resolution_outcome: "resume".to_string(),
        matching_candidates: Vec::new(),
        guidance: String::new(),
        resume_artifact_id: open.artifact_id,
        resume_kind: open.kind,
        resume_state: open.state,
        resume_guidance,
        resume_advance_action: advance_action,
        // The builder does NOT name the reason: it is shared by three callers
        // (the continuation procedure and two mid-run check-in nudges) and only
        // the CALLER knows which path it is on. Phase 1 hard-coded
        // "continuation_token" here, which silently labelled both nudge paths as
        // token resumes — mislabelling the very measurement this track exists to
        // produce. Each caller sets it immediately after.
        resume_source: String::new(),
        // A resume response never carries a park hint — park is surfaced only on
        // the SUPPRESSED (moved-on) turn's normal response.
        park_hint: None,
        conversation_hash: conversation_hash.to_string(),
    }
}

fn emit_routing_decision(
    phase: &str,
    turn_id: &str,
    input: &str,
    candidate_set: &str,
    selected: &str,
    resolution_outcome: &str,
    confidence: &str,
    at: &str,
) {
    let configured_brief_cap = router_v2_brief_cap();
    let route_variant = match (
        router_v1_gate_breadth_enabled(),
        configured_brief_cap.is_some(),
    ) {
        (false, false) => "control",
        (true, false) => "v1_gate_breadth",
        (false, true) => "v2_brief_breadth",
        (true, true) => "v1_gate_breadth+v2_brief_breadth",
    };
    let brief_cap = configured_brief_cap
        .as_deref()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|cap| *cap > 0)
        .map(|cap| cap.to_string())
        .unwrap_or_else(|| "all".to_string());
    if route_variant == "control" {
        tracing::info!(
            event_kind = "routing_decision",
            phase = %phase,
            turn_id = %turn_id,
            input = %input,
            candidate_set = %candidate_set,
            selected = %selected,
            resolution_outcome = %resolution_outcome,
            confidence = %confidence,
            at = %at,
            "routing decision"
        );
    } else {
        tracing::info!(
            event_kind = "routing_decision",
            phase = %phase,
            turn_id = %turn_id,
            input = %input,
            candidate_set = %candidate_set,
            selected = %selected,
            resolution_outcome = %resolution_outcome,
            confidence = %confidence,
            route_variant = route_variant,
            brief_cap = %brief_cap,
            at = %at,
            "routing decision"
        );
    }
}

/// Last `/`-segment of an artifact path (the artifact id / track_id). Mirrors
/// the inline `rsplit('/')` the complete RPC uses for the registry id.
fn last_segment(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn persist_completed_playbook_generation(
    hearth_path: &Path,
    artifact_path: &str,
    kind: &str,
    new_state: &str,
    actor_name: &str,
    actor_type: &str,
    actor_model: &str,
    actor_provider: &str,
    publication_log: Option<&PublicationLog>,
) -> Result<(), Status> {
    // The playbook-authoring kind, under its one canonical name (see
    // anvil_core registry::PLAYBOOK_GENERATION_KIND).
    if kind != "playbook_generation" || new_state != "completed" {
        return Ok(());
    }

    let context_path = playbook_generation_context_path(hearth_path, artifact_path).map_err(|e| {
        Status::internal(format!(
            "playbook_generation_terminal_persist: the intake seed for '{}' could not be located \
             under {}: {}. Skipping the persist would report success over a playbook that never \
             reached the registry.",
            artifact_path,
            hearth_path.display(),
            e
        ))
    })?;
    let context_text = match std::fs::read_to_string(&context_path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::info!(
                command = "playbook_generation_terminal_persist",
                artifact_path = %artifact_path,
                context_path = %context_path.display(),
                outcome = "skipped",
                "playbook_generation terminal persist skipped without intake seed"
            );
            return Ok(());
        }
        Err(e) => {
            return Err(Status::internal(format!(
                "playbook_generation_terminal_persist: failed to read {}: {}",
                context_path.display(),
                e
            )));
        }
    };
    let context: serde_json::Value = serde_json::from_str(&context_text).map_err(|e| {
        Status::internal(format!(
            "playbook_generation_terminal_persist: failed to parse {}: {}",
            context_path.display(),
            e
        ))
    })?;
    let owner_home = normalize_persist_owner_home(
        context
            .get("target_owner")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                Status::internal(format!(
                    "playbook_generation_terminal_persist: {} is missing target_owner",
                    context_path.display()
                ))
            })?,
    )?;
    let candidate_value = context
        .get("seed")
        .and_then(|seed| seed.get("candidate"))
        .cloned()
        .ok_or_else(|| {
            Status::internal(format!(
                "playbook_generation_terminal_persist: {} is missing seed.candidate",
                context_path.display()
            ))
        })?;
    let candidate: DomainCandidatePlaybook =
        serde_json::from_value(candidate_value).map_err(|e| {
            Status::internal(format!(
                "playbook_generation_terminal_persist: invalid seed.candidate in {}: {}",
                context_path.display(),
                e
            ))
        })?;
    // playbook_generation terminal-persist path: the candidate here is the
    // JSON `context.seed.candidate`, which — unlike the proto-intake wire
    // format — CAN carry per-step `success_criteria` (it round-trips through
    // serde with the `ProposedState.success_criteria` field). This is the one
    // site where the measurement DEFINE gate is meaningfully enforceable
    // today, so it is dark-gated: OFF by default (plain `generate`, threading
    // only), ON when `ANVIL_ENFORCE_MEASUREMENT_DEFINITION` is "1"/"true"
    // (mirrors the loader-block decision's default-off dark-gate). Before the
    // flag can be flipped on broadly, the proto-extension follow-on
    // (proto/anvil.proto `ProposedState.success_criteria` + anvil-mcp intake
    // schema + Lore candidate emission) must land so the proto-intake path
    // can carry criteria too.
    let enforce_measurement = enforce_measurement_definition();
    let machine = if enforce_measurement {
        generate_candidate_playbook_enforcing(&candidate)
    } else {
        generate_candidate_playbook(&candidate)
    }
    .map_err(|e| Status::invalid_argument(format!("invalid_candidate: {:?}", e)))?;
    // Evidence-obligation dark-gate at the generation terminal-persist seam
    // (T-EEC-1 P4), mirroring the intake seam so the obligation leg is present and
    // consistent everywhere the measurement leg is. The same candidate register
    // and obligation propagation used at intake is regenerated here before the
    // shared validator runs. Default OFF ⇒ no behavior change.
    if enforce_evidence_obligation() {
        validate_evidence_obligation(&machine, &machine.kind)
            .map_err(|e| Status::invalid_argument(format!("invalid_candidate: {}", e)))?;
    }
    let generated_kind = machine.kind.clone();
    let machine_yaml = serde_yaml::to_string(&machine).map_err(|e| {
        Status::internal(format!(
            "playbook_generation_terminal_persist: serialize generated machine failed: {}",
            e
        ))
    })?;

    // C-d.1 round 5, M-1: the round-4 pre-guard is removed here for the same
    // reason as at the `PersistPlaybook` seam — see that site.
    let target_registry = HearthPlaybookRegistry::new(PathBuf::from(&owner_home));
    let domain_request = DomainPersistPlaybookRequest {
        owner_home: owner_home.clone(),
        kind: generated_kind.clone(),
        machine_yaml,
        exemplars: generated_exemplar_files(&candidate).map_err(|e| {
            Status::internal(format!(
                "playbook_generation_terminal_persist: serialize exemplar failed: {}",
                e
            ))
        })?,
        // Candidate-generation terminal persist carries no hooks (machine.yaml +
        // exemplars only); the primitive supports them but this path emits none.
        hooks: Vec::new(),
        actor_name: actor_name.to_string(),
        actor_type: actor_type.to_string(),
        actor_model: actor_model.to_string(),
        actor_provider: actor_provider.to_string(),
    };
    let outcome = PersistPlaybookCommandHandler::execute(&target_registry, domain_request)
        .map_err(persist_playbook_error_to_status)?;

    use anvil_core::domain::persist_playbook_events::PersistPlaybookEvent;
    let artifact_adapter = FileSystemArtifactAdapter::new(PathBuf::from(&owner_home));
    let mut written_path = String::new();
    for event in &outcome.events {
        match event {
            PersistPlaybookEvent::PlaybookPersisted {
                owner_home,
                kind,
                machine_yaml,
                hooks,
                exemplars,
            } => {
                written_path = artifact_adapter
                    .persist_generated_playbook(
                        owner_home,
                        kind,
                        machine_yaml,
                        Some(hooks.as_slice()),
                        Some(exemplars.as_slice()),
                    )
                    .map_err(persist_playbook_artifact_error_to_status)?;
            }
        }
    }

    // Crucible publication-log mirror (best-effort; gated; never fatal).
    if let Some(pub_log) = publication_log {
        for event in &outcome.events {
            pub_log.mirror_persist(event);
        }
    }

    if written_path.is_empty() {
        written_path = Path::new(&owner_home)
            .join("playbooks")
            .join(&generated_kind)
            .join("machine.yaml")
            .display()
            .to_string();
    }

    let persist_events: Vec<&str> = outcome.events.iter().map(persist_event_name).collect();
    tracing::info!(
        command = "playbook_generation_terminal_persist",
        actor = %actor_name,
        owner_home = %owner_home,
        generated_kind = %generated_kind,
        written_path = %written_path,
        events = %persist_events.join(","),
        outcome = "ok",
        "playbook_generation terminal persist ok"
    );

    Ok(())
}

/// Locate the intake seed for a playbook-generation artifact.
///
/// # C-d.1 round 8, H-3 — the INVERTED POLARITY of this class
///
/// This used to answer a bare `PathBuf`, with `literal.exists()`,
/// `read_dir(..).ok()` and `entries.flatten()` — rounds 3, 4 and 5 in twelve
/// lines. Its consequence runs the other way from the rest of the class: the
/// caller treats `NotFound` on the returned path as *"no intake seed, nothing to
/// persist"* and returns `Ok(())`. So an unreadable hearth root did not corrupt
/// anything — it made `persist_generated_playbook` **skip entirely and report
/// success**, and a generated playbook silently never reached the registry.
///
/// A refusal is now representable, so "I could not look for the seed" cannot be
/// delivered to the caller as "there is no seed".
fn playbook_generation_context_path(
    hearth_path: &Path,
    artifact_path: &str,
) -> std::io::Result<PathBuf> {
    use anvil_core::domain::playbook::fs_probe::{list_dir, node_kind, NodeKind};
    let literal = hearth_path
        .join(artifact_path)
        .join("generation-context.json");
    if node_kind(&literal)? != NodeKind::Absent {
        return Ok(literal);
    }

    let artifact_id = last_segment(artifact_path);
    let entries = match list_dir(hearth_path) {
        Ok(entries) => entries,
        // An absent hearth root is a real answer; anything else is a refusal.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(literal),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let candidate = entry.path.join(artifact_id).join("generation-context.json");
        if node_kind(&candidate)? != NodeKind::Absent {
            return Ok(candidate);
        }
    }

    Ok(literal)
}

fn normalize_persist_owner_home(owner_home: &str) -> Result<String, Status> {
    let trimmed = owner_home.trim();
    let looks_like_descriptor = trimmed.contains(':');
    if trimmed != owner_home || !Path::new(trimmed).is_absolute() || looks_like_descriptor {
        return Err(Status::invalid_argument(format!(
            "unresolved_target_owner: target_owner must be a resolved absolute owner-home path, got '{}'. Foundry resolves owner descriptors before calling the engine; the engine requires the resolved path.",
            owner_home
        )));
    }
    Ok(trimmed.to_string())
}

// C-d.1 round 5, M-1: `persist_hearth_root_guard` USED TO LIVE HERE and is
// deleted. Round 4 added it at both persist seams and self-reported that it had
// "no falsifiable behavioral delta"; the round-4 review then measured that
// claim — deleting BOTH call sites left the engine suite byte-identical, same
// four names — and the audit of the structural claim it was kept for came back
// TRUE AND EMPTY: on every state the guard refused, `HearthPlaybookRegistry::new()`
// refused the same state and nothing would have moved, and on the ONE state
// where the rename actually happens (a legacy-only hearth) the guard returned
// `Ok` and the rename proceeded anyway.
//
// It could only have been given a delta by refusing `MovePending` — i.e. by
// changing rename behaviour — which is the `NG-DATA-CUTOVER` decision, not a
// fix round's call. New untested production code is what this round set out to
// stop producing, so the guard is removed rather than defended. The refusal that
// blocks a write is the domain one inside
// `PersistPlaybookCommandHandler::execute_impl`, mapped to FAILED_PRECONDITION
// at exactly ONE site (`persist_playbook_error_to_status`) and pinned by
// `anvil-engine/features/persist_playbook_throwaway_target.feature`.

fn proto_candidate_playbook_to_domain(
    candidate: Option<anvil_engine::proto::CandidatePlaybook>,
) -> Result<Option<DomainCandidatePlaybook>, Status> {
    let Some(candidate) = candidate else {
        return Ok(None);
    };
    let register = proto_candidate_register(&candidate.register)?;
    let proposed_states = candidate
        .proposed_states
        .into_iter()
        .map(|state| {
            let evidence_obligation = state
                .evidence_obligation
                .into_iter()
                .map(|value| proto_candidate_evidence_class(&value))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(DomainProposedState {
                state: state.state,
                role: state.role,
                intent: state.intent,
                expected_output: state.expected_output,
                // The wire does not yet carry per-step success criteria.
                success_criteria: None,
                evidence_obligation,
            })
        })
        .collect::<Result<Vec<_>, Status>>()?;

    Ok(Some(DomainCandidatePlaybook {
        source: candidate.source,
        evidence: candidate.evidence,
        intent: candidate.intent,
        route_description: candidate.route_description,
        route_triggers: candidate.route_triggers,
        projection_targets: candidate.projection_targets,
        register,
        proposed_states,
        success_rubric: candidate.success_rubric.map(proto_success_rubric_to_domain),
        // The wire (proto `CandidatePlaybook`) contract does not yet carry an
        // outcome_predicate field — threading it from Lore's candidate intake
        // over gRPC is a follow-on to this track, mirroring the
        // success_criteria gap above. Defaults to None until the proto
        // contract is extended; `generate_enforcing()` rejects driven
        // candidates that reach it without one, so callers must populate this
        // once the wire format grows the field.
        outcome_predicate: None,
        anchors: candidate
            .anchors
            .into_iter()
            .map(proto_anchor_to_domain)
            .collect(),
        exemplars: candidate
            .exemplars
            .into_iter()
            .filter_map(proto_exemplar_to_domain)
            .collect(),
        ledger_classification: candidate.ledger_classification.map(|classification| {
            anvil_core::domain::playbook::candidate::LedgerClassification {
                corpus: classification.corpus,
                ledger: classification.ledger,
                classification: classification.classification,
            }
        }),
        none_yet_justification: candidate.none_yet_justification.map(|justification| {
            anvil_core::domain::playbook::candidate::NoneYetJustification {
                corpus_searched: justification.corpus_searched,
                ledger_searched: justification.ledger_searched,
                why_no_exemplar: justification.why_no_exemplar,
                production_routing_allowed: justification.production_routing_allowed,
                followup_condition: justification.followup_condition,
            }
        }),
        at: candidate.at,
    }))
}

fn proto_candidate_register(
    value: &str,
) -> Result<anvil_core::domain::playbook::types::Register, Status> {
    match value {
        "" | "driven" => Ok(anvil_core::domain::playbook::types::Register::Driven),
        "free" => Ok(anvil_core::domain::playbook::types::Register::Free),
        unknown => Err(Status::invalid_argument(format!(
            "invalid_candidate: unknown candidate register '{}'",
            unknown
        ))),
    }
}

fn proto_candidate_evidence_class(
    value: &str,
) -> Result<anvil_core::domain::playbook::types::EvidenceClass, Status> {
    match value {
        "artifact_of_consequence" => {
            Ok(anvil_core::domain::playbook::types::EvidenceClass::ArtifactOfConsequence)
        }
        "verifiable_citation" => {
            Ok(anvil_core::domain::playbook::types::EvidenceClass::VerifiableCitation)
        }
        "self_description" => {
            Ok(anvil_core::domain::playbook::types::EvidenceClass::SelfDescription)
        }
        unknown => Err(Status::invalid_argument(format!(
            "invalid_candidate: unknown candidate evidence class '{}'",
            unknown
        ))),
    }
}

fn proto_success_rubric_to_domain(
    rubric: anvil_engine::proto::candidate_playbook::SuccessRubric,
) -> anvil_core::domain::playbook::types::SuccessRubric {
    anvil_core::domain::playbook::types::SuccessRubric {
        dimensions: rubric
            .dimensions
            .into_iter()
            .map(
                |dimension| anvil_core::domain::playbook::types::RubricDimension {
                    dimension: dimension.dimension,
                    weight: dimension.weight,
                    evidence_class: proto_evidence_class(&dimension.evidence_class),
                },
            )
            .collect(),
        grader: if rubric.grader.trim().is_empty() {
            None
        } else {
            Some(rubric.grader)
        },
        lagging_signals: rubric.lagging_signals,
        anchors: rubric
            .anchors
            .into_iter()
            .map(proto_anchor_to_domain)
            .collect(),
    }
}

fn proto_anchor_to_domain(
    anchor: anvil_engine::proto::candidate_playbook::AnchorRef,
) -> anvil_core::domain::playbook::types::AnchorRef {
    anvil_core::domain::playbook::types::AnchorRef {
        instance: anchor.instance,
        band: anchor.band,
    }
}

fn proto_exemplar_to_domain(
    exemplar: anvil_engine::proto::candidate_playbook::Exemplar,
) -> Option<anvil_core::domain::playbook::candidate::CandidateExemplar> {
    let frontmatter = exemplar.frontmatter?;
    Some(anvil_core::domain::playbook::candidate::CandidateExemplar {
        frontmatter: anvil_core::domain::playbook::exemplar::ExemplarFrontmatter {
            id: frontmatter.id,
            band: frontmatter.band,
            dimensions: frontmatter.dimensions,
            evidence_class: proto_evidence_class(&frontmatter.evidence_class),
            outcome_link: frontmatter.outcome_link.map(|link| {
                anvil_core::domain::playbook::exemplar::OutcomeLink {
                    authority: link.authority,
                    opaque_ref: link.opaque_ref,
                    verified_at: if link.verified_at.trim().is_empty() {
                        None
                    } else {
                        Some(link.verified_at)
                    },
                }
            }),
            provenance: frontmatter
                .provenance
                .map(
                    |provenance| anvil_core::domain::playbook::exemplar::ExemplarProvenance {
                        source: provenance.source,
                        corpus: provenance.corpus,
                    },
                )
                .unwrap_or(anvil_core::domain::playbook::exemplar::ExemplarProvenance {
                    source: String::new(),
                    corpus: String::new(),
                }),
            playbook_version: frontmatter.playbook_version,
            refreshed_at: frontmatter.refreshed_at,
        },
        body: exemplar.body,
    })
}

fn proto_evidence_class(value: &str) -> anvil_core::domain::playbook::types::EvidenceClass {
    match value {
        "artifact_of_consequence" => {
            anvil_core::domain::playbook::types::EvidenceClass::ArtifactOfConsequence
        }
        "verifiable_citation" => {
            anvil_core::domain::playbook::types::EvidenceClass::VerifiableCitation
        }
        _ => anvil_core::domain::playbook::types::EvidenceClass::SelfDescription,
    }
}

fn generated_exemplar_files(
    candidate: &DomainCandidatePlaybook,
) -> Result<Vec<anvil_core::domain::playbook::candidate::GeneratedExemplarFile>, serde_yaml::Error>
{
    candidate
        .exemplars
        .iter()
        .map(anvil_core::domain::playbook::candidate::GeneratedExemplarFile::try_from)
        .collect()
}

fn intake_instance_name_component(candidate: &DomainCandidatePlaybook) -> String {
    // The generated playbook_name stays slug(intent). The begin instance name
    // gets the normalized candidate `at` timestamp so same-intent submissions in
    // the same scaffold minute land in distinct playbook_generations directories.
    let normalized_at = normalize_instance_component(&candidate.at);
    if !normalized_at.is_empty() {
        return normalized_at;
    }

    format!("candidate_{:08x}", short_candidate_hash(candidate))
}

fn normalize_instance_component(input: &str) -> String {
    let mut out = String::new();
    let mut previous_was_underscore = false;

    for ch in input.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            previous_was_underscore = false;
        } else if !previous_was_underscore {
            out.push('_');
            previous_was_underscore = true;
        }
    }

    out.trim_matches('_').to_string()
}

fn short_candidate_hash(candidate: &DomainCandidatePlaybook) -> u32 {
    let mut hasher = DefaultHasher::new();
    candidate.source.hash(&mut hasher);
    candidate.intent.hash(&mut hasher);
    candidate.at.hash(&mut hasher);
    candidate.evidence.hash(&mut hasher);
    for proposed in &candidate.proposed_states {
        proposed.state.hash(&mut hasher);
        proposed.role.hash(&mut hasher);
        proposed.intent.hash(&mut hasher);
        proposed.expected_output.hash(&mut hasher);
    }
    (hasher.finish() & 0xffff_ffff) as u32
}

fn snapshot_error_to_status(error: SnapshotError) -> Status {
    let msg = format!("{}", error);
    match error {
        SnapshotError::InvalidArgument { .. } => Status::invalid_argument(msg),
        SnapshotError::NotFound { .. } => Status::not_found(msg),
        SnapshotError::MalformedStatus { .. } => Status::internal(msg),
        SnapshotError::IoError { .. } => Status::internal(msg),
        SnapshotError::BacklogStore { .. } => Status::failed_precondition(msg),
        SnapshotError::ActorNameRequired => Status::invalid_argument(msg),
        SnapshotError::ActorParamsRequired { .. } => Status::invalid_argument(msg),
    }
}

/// Maps each `CompleteError` variant to the appropriate gRPC status code.
fn complete_error_to_status(error: CompleteError) -> Status {
    let msg = format!("{}", error);
    match error {
        CompleteError::ArtifactPathRequired => Status::invalid_argument(msg),
        CompleteError::ActorNameRequired => Status::invalid_argument(msg),
        CompleteError::ActorParamsRequired { .. } => Status::invalid_argument(msg),
        CompleteError::WrongStateForComplete { .. } => Status::failed_precondition(msg),
        CompleteError::SatisfactionOutOfScope { .. } => Status::invalid_argument(msg),
        CompleteError::SatisfactionUnknown { .. } => Status::invalid_argument(msg),
        CompleteError::FindingsRequiredForAddressInNextStep => Status::invalid_argument(msg),
        CompleteError::NotFound { .. } => Status::not_found(msg),
        CompleteError::MalformedStatus { .. } => Status::internal(msg),
        CompleteError::IoError { .. } => Status::internal(msg),
        CompleteError::ReflectionWriteFailed { .. } => Status::internal(msg),
    }
}

/// Maps each `AmendError` variant to the appropriate gRPC status code.
/// `Amendment(e)` (a B5a schema/application error) → INVALID_ARGUMENT carrying
/// the wrapped code + Display, mirroring AC-1.
fn amend_error_to_status(error: AmendError) -> Status {
    let msg = format!("{}", error);
    match error {
        AmendError::ArtifactPathRequired => Status::invalid_argument(msg),
        AmendError::TargetDocumentRequired => Status::invalid_argument(msg),
        AmendError::ActorNameRequired => Status::invalid_argument(msg),
        AmendError::ActorParamsRequired { .. } => Status::invalid_argument(msg),
        AmendError::UnknownAmendmentKind { .. } => Status::invalid_argument(msg),
        AmendError::UnknownOpKind { .. } => Status::invalid_argument(msg),
        AmendError::UnknownAnchor { .. } => Status::invalid_argument(msg),
        AmendError::NotFound { .. } => Status::not_found(msg),
        AmendError::MalformedStatus { .. } => Status::internal(msg),
        AmendError::IoError { .. } => Status::internal(msg),
        AmendError::Amendment(_) => Status::invalid_argument(msg),
    }
}

/// Maps each `PersistPlaybookError` variant to the appropriate gRPC status code.
///
/// L-2: a duplicate kind at the owner-home maps to ALREADY_EXISTS — the
/// resource (a machine of that kind) already exists at the target home, which
/// is semantically precise (NOT a reflexive INVALID_ARGUMENT). Required-field
/// guards and a loader-invalid machine map to INVALID_ARGUMENT.
fn persist_playbook_error_to_status(error: PersistPlaybookError) -> Status {
    let msg = format!("{}", error);
    match error {
        PersistPlaybookError::OwnerHomeRequired => Status::invalid_argument(msg),
        PersistPlaybookError::KindRequired => Status::invalid_argument(msg),
        PersistPlaybookError::MachineYamlRequired => Status::invalid_argument(msg),
        PersistPlaybookError::ActorNameRequired => Status::invalid_argument(msg),
        PersistPlaybookError::ActorParamsRequired { .. } => Status::invalid_argument(msg),
        PersistPlaybookError::LoaderInvalid(_) => Status::invalid_argument(msg),
        PersistPlaybookError::KindMismatch { .. } => Status::invalid_argument(msg),
        PersistPlaybookError::DuplicateKind { .. } => Status::already_exists(msg),
        PersistPlaybookError::ExistingMachineInvalid { .. } => Status::already_exists(msg),
        PersistPlaybookError::NonTerminalStateHookless { .. } => Status::invalid_argument(msg),
        // FAILED_PRECONDITION, not INVALID_ARGUMENT: the request is fine and the
        // TARGET HEARTH is not in a state that may be registered into. The caller
        // fixes the hearth (complete the move, or make the merge decision), not
        // the request.
        PersistPlaybookError::HearthRegistrationBlocked { .. } => Status::failed_precondition(msg),
    }
}

/// Maps write-boundary failures from the filesystem persist to gRPC status.
///
/// The domain handler's duplicate preflight is only a fast path. The
/// authoritative collision decision is the adapter's exclusive create; if that
/// boundary refuses a different or invalid existing target, the command returns
/// ALREADY_EXISTS and no post-write success log/projection is emitted.
fn persist_playbook_artifact_error_to_status(error: ArtifactError) -> Status {
    let msg = format!("{}", error);
    match error {
        ArtifactError::PlaybookDuplicateKind { .. }
        | ArtifactError::PlaybookExistingMachineInvalid { .. } => Status::already_exists(msg),
        ArtifactError::IoError { .. } => {
            Status::internal(format!("persist_generated_playbook failed: {}", msg))
        }
    }
}

/// Stable variant name for the persist outcome log (names only).
fn persist_event_name(
    event: &anvil_core::domain::persist_playbook_events::PersistPlaybookEvent,
) -> &'static str {
    use anvil_core::domain::persist_playbook_events::PersistPlaybookEvent;
    match event {
        PersistPlaybookEvent::PlaybookPersisted { .. } => "PlaybookPersisted",
    }
}

/// Stable variant name for the amend outcome log (Req 6 — names only).
fn amend_event_name(event: &anvil_core::domain::amend_events::AmendEvent) -> &'static str {
    use anvil_core::domain::amend_events::AmendEvent;
    match event {
        AmendEvent::OpRecorded { .. } => "OpRecorded",
        AmendEvent::ActorUpserted { .. } => "ActorUpserted",
        AmendEvent::TransitionRecorded { .. } => "TransitionRecorded",
    }
}

/// Maps each `BeginError` variant to the appropriate gRPC status code,
/// preserving the message so the MCP shim can surface fallback skill
/// names to the agent end-to-end.
fn begin_error_to_status(error: BeginError) -> Status {
    let msg = format!("{}", error);
    match error {
        BeginError::SessionRequired => Status::unauthenticated(msg),
        BeginError::ModeNotImplemented { .. } => Status::unimplemented(msg),
        BeginError::RoleStateMismatch { .. } => Status::invalid_argument(msg),
        BeginError::StateNotReviewable { .. } => Status::failed_precondition(msg),
        BeginError::SpecNotReadyForReview { .. } => Status::failed_precondition(msg),
        BeginError::AlreadyGoverned { .. } => Status::failed_precondition(msg),
        BeginError::AdoptionEvidenceUnreadable { .. } => Status::failed_precondition(msg),
        BeginError::InvalidArgument { .. } => Status::invalid_argument(msg),
        BeginError::NotFound { .. } => Status::not_found(msg),
        BeginError::IoError { .. } => Status::internal(msg),
        BeginError::MalformedStatus { .. } => Status::internal(msg),
        BeginError::Checkin(inner) => checkin_error_to_status(inner),
        BeginError::RoutedSelectionMismatch { .. } => Status::failed_precondition(msg),
        BeginError::NotDrivenCandidate { .. } => Status::failed_precondition(msg),
        BeginError::AccessDenied { .. } => Status::permission_denied(msg),
        BeginError::ActorNameRequired => Status::invalid_argument(msg),
        BeginError::ActorParamsRequired { .. } => Status::invalid_argument(msg),
    }
}

/// K5 idempotent-bind open-instance resolver (spec R2 / A2), used only behind
/// `ANVIL_K5_BIND` from the begin bind seam. Returns the existing OPEN driven
/// instance bound to `conversation_id` — a driven-kind instance carrying a
/// `begin` activity marker for that run correlation whose CURRENT state is
/// non-terminal — most-recently-begun on multiplicity, else `None`.
///
/// It deliberately does NOT apply the begin-adoption `has_open_begin` gate that
/// [`find_open_workflow_for_conversation`] uses. That gate treats any same-actor
/// non-adoption transition at-or-after the marker as "closed", which a driven
/// instance's OWN creation transition trips (the create-time `Snapshot` to the
/// initial state carries the begin actor and shares the marker's second-precision
/// timestamp), masking a freshly-bound instance from a same-second Kiln retry.
/// For the create-time dedup the openness criterion is simply the non-terminal
/// current state: a driven K5 instance is closed only by reaching a terminal
/// state (`completed`/`abandoned`), so a retry of the SAME run resolves to the
/// one open instance while a NEW run reusing a correlation whose prior instance
/// already resolved (terminal) correctly binds fresh.
///
/// Fail-open by construction: any read error resolves to `None` (a fresh bind),
/// never an error that would strand the fire path — R7's fail-loud law is
/// preserved because a missed dedup is a duplicate instance, not a masked bind
/// failure.
fn k5_open_bound_instance_for_conversation(
    query: &dyn anvil_core::ports::query_port::QueryPort,
    registry: &dyn PlaybookRegistry,
    conversation_id: &str,
) -> Option<anvil_core::domain::route::OpenPlaybookRun> {
    use anvil_core::domain::route::{state_is_terminal, OpenPlaybookRun};
    if conversation_id.trim().is_empty() {
        return None;
    }
    let artifacts = query.list_artifacts().ok()?;
    // (begun_at, OpenPlaybookRun) of the best match so far — most-recently-begun
    // wins, ties break by artifact id desc (mirrors `most_recently_begun`).
    let mut best: Option<(String, OpenPlaybookRun)> = None;
    for (artifact_id, kind) in artifacts {
        let is_driven = registry
            .machine_for(&kind)
            .map(|machine| machine.is_driven())
            .unwrap_or(false);
        if !is_driven {
            continue;
        }
        let Ok(activity) = query.read_activity_entries(&artifact_id) else {
            continue;
        };
        let mut newest_begin_at: Option<String> = None;
        for entry in activity.iter() {
            if entry.kind != "begin" || entry.conversation_id != conversation_id {
                continue;
            }
            if newest_begin_at
                .as_deref()
                .map(|cur| entry.at.as_str() > cur)
                .unwrap_or(true)
            {
                newest_begin_at = Some(entry.at.clone());
            }
        }
        let Some(begun_at) = newest_begin_at else {
            continue;
        };
        let Ok(state) = query.read_artifact_state(&artifact_id) else {
            continue;
        };
        if state_is_terminal(registry, &kind, &state) {
            continue;
        }
        let candidate = OpenPlaybookRun {
            artifact_id: artifact_id.clone(),
            kind,
            state,
        };
        let wins = best
            .as_ref()
            .map(|(cur_at, cur)| {
                begun_at > *cur_at
                    || (begun_at == *cur_at && candidate.artifact_id > cur.artifact_id)
            })
            .unwrap_or(true);
        if wins {
            best = Some((begun_at, candidate));
        }
    }
    best.map(|(_, workflow)| workflow)
}

/// Generates the `next_step` text for a checkin response. Varies by role
/// and by response content; always references the per-entry
/// `execution_route` discriminator rather than enumerating
/// engine-supported combinations in prose (per spec §15).
fn checkin_next_step(
    role: &str,
    filtered: &[anvil_core::domain::ArtifactSummary],
    available_types: &[anvil_core::domain::AvailableArtifactType],
) -> String {
    match role {
        "creator" => {
            if available_types.is_empty() {
                "No artifact types available for creation.".to_string()
            } else {
                let target_count = available_types.len();
                let parent_count = filtered.len();
                format!(
                    "You can create a new artifact. For each of the {} type(s) in `available_types`, \
                     check its `execution_route`: if `engine`, call `describe(<type>)` to learn \
                     required fields then `begin(artifact_type: <type>, ...)`; if `fallback:forge:<skill>`, \
                     invoke that forge skill instead. {} parent artifact(s) are available in `filtered_artifacts` \
                     as valid parents for child-track creation.",
                    target_count, parent_count
                )
            }
        }
        "reviewer" => {
            if filtered.is_empty() {
                "No artifacts are awaiting review.".to_string()
            } else {
                format!(
                    "{} artifact(s) await review. For each entry in `filtered_artifacts`, \
                     check its `execution_route`: if `engine`, call `begin(identifier: <id>)` to record \
                     the review transition; if `fallback:forge:<skill>`, invoke that forge skill instead.",
                    filtered.len()
                )
            }
        }
        "resumer" => {
            if filtered.is_empty() {
                "No artifacts awaiting resumption.".to_string()
            } else {
                format!(
                    "{} artifact(s) can be resumed. For each entry in `filtered_artifacts`, \
                     check its `execution_route`: if `engine`, call `begin(identifier: <id>)`; \
                     if `fallback:forge:<skill>`, invoke that forge skill instead.",
                    filtered.len()
                )
            }
        }
        _ => String::new(),
    }
}

/// Next-step text for a begin response, derived from the MACHINE for any
/// playbook (not a hardcoded per-track string table). Looks up the machine for
/// `kind` in the registry and synthesizes guidance from the OUTGOING
/// transitions of `state` via `next_step_for`. Produces a NON-EMPTY, skill-free
/// next_step for every non-terminal state of every registered playbook; falls
/// back to empty only when the machine for `kind` cannot be resolved.
fn begin_next_step(registry: &dyn PlaybookRegistry, kind: &str, state: &str) -> String {
    use anvil_core::domain::playbook::next_step::next_step_for;
    registry
        .machine_for(kind)
        .map(|machine| next_step_for(machine, state))
        .unwrap_or_default()
}

/// Next-step text for a complete response, derived from the MACHINE for any
/// playbook. Mirrors `begin_next_step`: synthesizes guidance from the outgoing
/// transitions of the new state the artifact moved into, for any registered
/// playbook kind.
fn complete_next_step(registry: &dyn PlaybookRegistry, kind: &str, new_state: &str) -> String {
    use anvil_core::domain::playbook::next_step::next_step_for;
    registry
        .machine_for(kind)
        .map(|machine| next_step_for(machine, new_state))
        .unwrap_or_default()
}

/// Next-step text for a type-level describe response.
fn describe_next_step_for_type() -> String {
    "Ask the human for the required fields listed above, then call `begin` with them.".to_string()
}

/// Next-step text for an instance-level describe response.
/// Varies by response content: wording changes when there are no available
/// actions vs. when there are, and references the per-action
/// `execution_route` discriminator for the agent to route.
fn describe_next_step_for_instance(
    actions: &[anvil_core::domain::describe::AvailableAction],
) -> String {
    if actions.is_empty() {
        "No further actions are available from this state.".to_string()
    } else {
        format!(
            "{} action(s) available. For each entry in `available_actions`, check its \
             `execution_route`: if `engine`, call `begin(identifier: <id>)` to execute it; \
             if `fallback:forge:<skill>`, invoke that forge skill instead.",
            actions.len()
        )
    }
}

/// Maps `SnapshotError` → `BeginError` for the engine's event-routing
/// shim (plan.md Phase 2 mapping table).
fn map_snapshot_error_to_begin(error: SnapshotError) -> BeginError {
    match error {
        SnapshotError::NotFound { artifact_path } => BeginError::NotFound {
            identifier: artifact_path,
        },
        SnapshotError::MalformedStatus {
            artifact_path,
            message,
        } => BeginError::MalformedStatus {
            artifact_id: artifact_path,
            message,
        },
        SnapshotError::IoError { message } => BeginError::IoError { message },
        SnapshotError::BacklogStore { message } => BeginError::IoError {
            message: message.clone(),
        },
        SnapshotError::ActorNameRequired => BeginError::IoError {
            message: "actor_name_required".to_string(),
        },
        SnapshotError::ActorParamsRequired { field } => BeginError::IoError {
            message: format!("actor_params_required: {}", field),
        },
        SnapshotError::InvalidArgument { reason } => BeginError::InvalidArgument { reason },
    }
}

/// Maps `SnapshotError` to `CompleteError` for the engine event-routing shim.
fn map_snapshot_error_to_complete(error: SnapshotError) -> CompleteError {
    match error {
        SnapshotError::NotFound { artifact_path } => CompleteError::NotFound { artifact_path },
        SnapshotError::MalformedStatus {
            artifact_path,
            message,
        } => CompleteError::MalformedStatus {
            artifact_path,
            message,
        },
        SnapshotError::IoError { message } => CompleteError::IoError { message },
        SnapshotError::BacklogStore { message } => CompleteError::IoError {
            message: message.clone(),
        },
        SnapshotError::ActorNameRequired => CompleteError::IoError {
            message: "actor_name_required".to_string(),
        },
        SnapshotError::ActorParamsRequired { field } => CompleteError::IoError {
            message: format!("actor_params_required: {}", field),
        },
        SnapshotError::InvalidArgument { reason } => CompleteError::IoError {
            message: format!("invalid_argument: {}", reason),
        },
    }
}

/// Build the initial status.yaml string written by
/// `ArtifactPort::scaffold_playbook_directory`. Minimal shape — `state: draft`,
/// `kind: playbook`, empty transitions and actors — because the subsequent
/// `SnapshotCommandHandler::execute` call appends the first transition and seeds the actor.
fn build_initial_playbook_status_yaml(parent_id: &str, origin_turn: &str) -> String {
    let origin_line = if origin_turn.is_empty() {
        String::new()
    } else {
        format!("origin_turn: {}\n", origin_turn)
    };
    format!(
        "version: 1\nkind: playbook\nstate: draft\n{}parent_id: {}\nactors:\ntransitions:\n",
        origin_line, parent_id
    )
}

/// Build the initial status.yaml string for a machine-derived artifact create.
/// Minimal shape — `kind`/`state` from the resolved machine, empty transitions
/// and actors (the subsequent `SnapshotCommandHandler::execute` appends the
/// first transition and seeds the actor). When the artifact has no parent
/// (`parent_id` empty — domain machines with `parent_kind: ~`) NO parent line is
/// written. When it has a parent, the unified `parent_id:` key carries it
/// (any parent kind; see StatusContent docs).
/// When `target_owner` is non-empty, a `target_owner:` line is appended
/// (skip-if-empty, mirroring the parent-line conditional). (Anvil-lane 1b.)
fn build_initial_artifact_status_yaml(
    kind: &str,
    state: &str,
    parent_id: &str,
    target_owner: &str,
    fields: &std::collections::BTreeMap<String, String>,
    origin_turn: &str,
) -> String {
    let mut optional_lines = String::new();
    if !origin_turn.is_empty() {
        optional_lines.push_str(&format!("origin_turn: {}\n", origin_turn));
    }
    if !parent_id.is_empty() {
        optional_lines.push_str(&format!("parent_id: {}\n", parent_id));
    }
    if !target_owner.is_empty() {
        optional_lines.push_str(&format!("target_owner: {}\n", target_owner));
    }
    // Generic machine-declared required fields, deterministic key order
    // (BTreeMap), skip-if-empty exactly like target_owner.
    if !fields.is_empty() {
        optional_lines.push_str("fields:\n");
        for (key, value) in fields {
            optional_lines.push_str(&format!("  {}: {}\n", key, value));
        }
    }
    format!(
        "version: 1\nkind: {}\nstate: {}\n{}actors:\ntransitions:\n",
        kind, state, optional_lines
    )
}

fn parse_arg(args: &[String], flag: &str) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        if args[i] == flag {
            i += 1;
            if i < args.len() {
                return Some(args[i].clone());
            }
        }
        i += 1;
    }
    None
}

fn parse_repeated_arg(args: &[String], flag: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == flag {
            i += 1;
            if i < args.len() {
                values.push(args[i].clone());
            }
        }
        i += 1;
    }
    values
}

fn configured_permitted_roots(args: &[String]) -> Vec<PathBuf> {
    let cli_roots = parse_repeated_arg(args, "--permitted-root");
    if !cli_roots.is_empty() {
        return cli_roots.into_iter().map(PathBuf::from).collect();
    }

    std::env::var("ANVIL_PERMITTED_ROOTS")
        .ok()
        .map(|raw| {
            raw.split(':')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
}

fn configured_global_playbooks_hearth(args: &[String]) -> Option<PathBuf> {
    parse_arg(args, "--global-playbooks-hearth")
        .or_else(|| std::env::var("ANVIL_GLOBAL_PLAYBOOKS_HEARTH").ok())
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
        .map(PathBuf::from)
}
