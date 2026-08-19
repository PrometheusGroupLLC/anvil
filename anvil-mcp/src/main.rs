#![recursion_limit = "256"]

use anvil_core::domain::playbook::composite_registry::CompositePlaybookRegistry;
use anvil_core::domain::playbook::hearth_registry::HearthPlaybookRegistry;
use anvil_core::domain::playbook::registry::{PlaybookRegistry, SeedPlaybookRegistry};
use anvil_core_hearth::containment;
use anvil_engine::proto::anvil_service_client::AnvilServiceClient;
use anvil_engine::proto::{
    AmendRequest, BeginAdoptionStatusRequest, BeginRequest, CatalogRequest, CheckinRequest,
    ClaimedEvidence, CompleteRequest, DescribeRequest, HealthCheckRequest,
    IntakeCandidatePlaybookRequest, PersistPlaybookRequest, PlaybookHook, RouteRequest,
    SnapshotRequest,
    backlog_mutate_request, backlog_outcome_binding_decl, backlog_playbook_binding,
    backlog_queue_request, backlog_rank_inputs_edit, backlog_shape_edit, BacklogCommitReshuffle,
    BacklogDependencyReadiness, BacklogEvaluateRequest, BacklogEvidenceRef,
    BacklogExecutionBinding, BacklogLiftAgeOutVeto, BacklogMutateRequest,
    BacklogOutcomeBindingDecl, BacklogPlaybookBinding, BacklogPresence, BacklogProposeReshuffle,
    BacklogProposedPosition, BacklogQueueRequest, BacklogRankInputsEdit, BacklogRecomputeRank,
    BacklogRecordOutcomeSignoff, BacklogRejectReshuffle, BacklogShapeEdit,
    BacklogStampExecutionBinding, BacklogValueGapMagnitude, BacklogVetoAgeOut,
    BacklogWakeCondition,
};
use serde_json::Value;
use std::future::Future;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;

/// Session state stored in the MCP shim after a successful checkin.
#[derive(Debug, Clone)]
struct SessionState {
    role: String,
}

/// Env var the Foundry desktop shell sets when it spawns an opted-in
/// kit's MCP child (T6/T7 of `unified_foundry_auth` — see
/// `foundry/docs/foundry-auth-standard.md`). Presence + non-empty
/// value means the supervisor injected a session JWT. Absence is the
/// normal local-dev case and stays silent.
const FOUNDRY_SESSION_TOKEN_ENV: &str = "FOUNDRY_SESSION_TOKEN";
const DEFAULT_ENGINE_PORT: u16 = 50051;
const CLIENT_ENGINE_PROTO_VERSION_MISMATCH: &str = "client_engine_proto_version_mismatch";

/// One small backoff before a single transport retry (Fix #2). The harness
/// captures the shim's stderr into its MCP log, so the retry leaves a
/// breadcrumb. Engine restarts are expected to come back within this window.
const TRANSPORT_RETRY_BACKOFF: std::time::Duration = std::time::Duration::from_millis(250);

/// Emit a concise diagnostic line on stderr. The MCP harness folds shim
/// stderr into its log, so these are the only crash/restart breadcrumbs an
/// operator gets. Keep them to command names + error kinds — never payloads
/// or secrets.
fn log_diag(args: std::fmt::Arguments) {
    eprintln!("anvil-mcp: {}", args);
}

macro_rules! diag {
    ($($arg:tt)*) => { $crate::log_diag(format_args!($($arg)*)) };
}

/// Serialize a tool-result JSON value into the MCP `text` string without
/// ever panicking (Fix #3). A failure here would have been a process-killing
/// `.unwrap()`; instead we return `Err` so the caller surfaces a tool error
/// and the shim survives to serve the next request.
fn serialize_tool_text(value: &Value) -> Result<String, String> {
    // Test seam: force the serialization path to fail so the BDD harness can
    // prove the handler returns a tool error (and the shim stays alive)
    // instead of panicking the whole process.
    if std::env::var("ANVIL_TEST_FORCE_RESULT_SERIALIZE_FAIL").is_ok() {
        return Err("forced serialization failure (test seam)".to_string());
    }
    serde_json::to_string(value).map_err(|e| format!("Failed to serialize tool result: {}", e))
}

/// Build the standard MCP tool-success envelope, hardening the result
/// serialization (Fix #3). On serialization failure returns a tool error
/// rather than panicking.
fn tool_text_response(id: &Value, result_json: &Value) -> Value {
    match serialize_tool_text(result_json) {
        Ok(text) => serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "content": [{
                    "type": "text",
                    "text": text
                }]
            }
        }),
        Err(e) => {
            diag!("tool result serialization failed: {}", e);
            return_tool_error(id, &e)
        }
    }
}

/// Internal error type for gRPC calls that preserves the tonic::Status
/// on failure so callers can pattern-match on the gRPC code (e.g.,
/// Unauthenticated for Req 4 auth-error surfacing).
enum RpcError {
    /// The engine returned a gRPC status (could be UNAUTHENTICATED, etc.).
    Status(tonic::Status),
    /// A transport / connect error (before any gRPC exchange).
    Transport(String),
}

type EngineClient = AnvilServiceClient<tonic::transport::Channel>;
type EngineRpcFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<tonic::Response<T>, tonic::Status>> + Send + 'a>>;

/// The outcome of the kit-boundary session check (spec Req 1 + Req 4).
///
/// This is a shim-level bi-state:
/// - `Standalone`     — token absent or whitespace-only; proceed as today with
///                      no authorization metadata on outbound gRPC calls.
/// - `Foundry(token)` — token present; the shim attaches `authorization: Bearer
///                      <token>` on every outbound gRPC call (except health_check)
///                      so the Foundry-mode engine can verify it per-call.
///
/// The shim does not itself validate or refuse tokens — it has no broker socket.
/// It forwards the token to the engine on the first RPC and the engine's R1
/// gatekeeper decides.  When the engine returns `UNAUTHENTICATED` the shim
/// maps that gRPC status to a JSON-RPC error (see `map_grpc_auth_error`).
///
/// For the e2e harness the `FOUNDRY_SESSION_TOKEN` env var is set by the test
/// step to a known-invalid or known-valid token; the engine's stub verifier
/// accepts or rejects deterministically.
#[derive(Debug, Clone)]
enum FoundryMode {
    /// No token or whitespace-only: standalone, no metadata.
    Standalone,
    /// Token present: attach as Bearer on outbound gRPC calls.
    Foundry(String),
}

#[derive(Debug, Clone)]
enum CallHearth {
    Resolved(PathBuf),
    Ambiguous(String),
}

#[derive(Debug, Clone)]
struct RefuseError {
    code: &'static str,
    reason: String,
}

trait HearthDisambiguator {
    fn disambiguate(&self, reason: &str) -> Result<PathBuf, RefuseError>;
}

struct RefuseDisambiguator;

impl HearthDisambiguator for RefuseDisambiguator {
    fn disambiguate(&self, reason: &str) -> Result<PathBuf, RefuseError> {
        Err(RefuseError {
            code: "ambiguous_hearth",
            reason: reason.to_string(),
        })
    }
}

fn tool_uses_hearth(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "catalog"
            | "checkin"
            | "describe"
            | "begin"
            | "anvil_orchestrate"
            | "snapshot"
            | "complete"
            | "amend"
            | "persist_playbook"
            | "candidate_playbook_intake"
            | "begin_adoption_status"
    ) || BACKLOG_TOOLS.contains(&tool_name)
}

fn return_unknown_tool(id: &Value, tool_name: &str) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "isError": true,
            "content": [{
                "type": "text",
                "text": format!("Unknown tool: {}", tool_name)
            }]
        }
    })
}

/// Read `FOUNDRY_SESSION_TOKEN` from the environment and determine the
/// kit-boundary operating mode (spec Req 1 + Req 4).
///
/// Whitespace-only tokens are treated as absent (Standalone) per spec Req 1.
/// A present token is held for forwarding; the engine is the verifier.
fn observe_foundry_session() -> FoundryMode {
    let raw = match std::env::var(FOUNDRY_SESSION_TOKEN_ENV) {
        Ok(v) => v,
        Err(_) => return FoundryMode::Standalone,
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return FoundryMode::Standalone;
    }
    // Token is present and non-blank. Emit a breadcrumb and hold it for
    // forwarding as `authorization: Bearer` metadata on gRPC calls.
    eprintln!(
        "anvil-mcp: foundry session inherited (token len={}, audience=foundry-mcp:anvil-kit)",
        trimmed.len()
    );
    FoundryMode::Foundry(trimmed.to_string())
}

/// Attach `authorization: Bearer <token>` to a tonic Request's metadata.
///
/// Used for every gated RPC (catalog, begin, describe, checkin, snapshot,
/// complete) when the shim is in Foundry mode. Never called for health_check.
///
/// The implementation lives in `anvil_engine::kit_bearer` so the shim and the
/// hooks CLI present a byte-identical header to the engine's gatekeeper; a
/// second local copy was free to drift from the shape the engine parses.
use anvil_engine::kit_bearer::attach_bearer;

/// Ensure the engine is reachable, connect a fresh tonic client, attach Foundry
/// authorization metadata when present, invoke one RPC, and map engine failures
/// to the MCP response shape used by tool handlers.
fn call_engine<TRequest, TResponse, RpcCall, StatusMessage>(
    id: &Value,
    engine_port: &mut u16,
    rt: &tokio::runtime::Runtime,
    foundry_mode: &FoundryMode,
    rpc_request: TRequest,
    rpc_call: RpcCall,
    status_message: StatusMessage,
) -> Result<TResponse, Value>
where
    TRequest: Clone + Send + 'static,
    TResponse: Send + 'static,
    RpcCall: for<'a> Fn(
        &'a mut EngineClient,
        tonic::Request<TRequest>,
    ) -> EngineRpcFuture<'a, TResponse>,
    StatusMessage: FnOnce(&tonic::Status) -> String,
{
    if let Err(e) = ensure_engine_reachable(engine_port, rt) {
        diag!("ensure_engine_reachable failed: {}", e);
        return Err(return_tool_error(id, &e));
    }

    let port = *engine_port;
    let foundry_token = match foundry_mode {
        FoundryMode::Foundry(t) => Some(t.clone()),
        FoundryMode::Standalone => None,
    };

    // Fix #2: an engine restart (Foundry churn) makes the first connect/call
    // fail with a transport error or gRPC Unavailable. Rather than surface a
    // "drop", re-establish the client and retry the SAME call ONCE after a
    // small backoff. Non-transport gRPC errors (FAILED_PRECONDITION, etc.) are
    // NOT retried — they are deterministic and returned as tool errors.
    let result = rt.block_on(async {
        // A single attempt: fresh connect, attach bearer, invoke. Each attempt
        // consumes a clone of the request so the call can be retried.
        let attempt = |req: TRequest| {
            let token = foundry_token.clone();
            let call = &rpc_call;
            async move {
                let addr = format!("http://127.0.0.1:{}", port);
                let mut client = EngineClient::connect(addr).await.map_err(|e| {
                    RpcError::Transport(format!("Failed to connect to engine: {}", e))
                })?;

                let mut request = tonic::Request::new(req);
                // Name the surface on every call this shim makes. One line here
                // covers all fifteen call sites because they all route through
                // `call_engine`, and it sits INSIDE `attempt` so the retry path
                // re-stamps it rather than sending an unattributable second try.
                // The engine refuses a state-changing call that names none, so
                // this is the shim's half of the CQRS command seam and not a
                // decoration.
                request.metadata_mut().insert(
                    anvil_engine::command_seam::SURFACE_METADATA_KEY,
                    tonic::metadata::MetadataValue::from_static("mcp"),
                );
                if let Some(token) = &token {
                    request = attach_bearer(request, token).map_err(RpcError::Transport)?;
                }

                (call)(&mut client, request)
                    .await
                    .map(|r| r.into_inner())
                    .map_err(RpcError::Status)
            }
        };

        // Test seam: force the first attempt to fail as if the engine had just
        // been restarted, so the BDD harness can prove the retry path recovers.
        let first: Result<TResponse, RpcError> = if take_force_transport_fail_once() {
            Err(RpcError::Transport(
                "forced transport failure (test seam)".to_string(),
            ))
        } else {
            attempt(rpc_request.clone()).await
        };

        match first {
            Ok(resp) => Ok(resp),
            // Retry ONCE on a transport error or gRPC Unavailable (engine
            // momentarily unreachable / restarting).
            Err(err) if is_retryable_transport_error(&err) => {
                diag!(
                    "engine transport error ({}); reconnecting and retrying once after {}ms",
                    transport_error_label(&err),
                    TRANSPORT_RETRY_BACKOFF.as_millis()
                );
                tokio::time::sleep(TRANSPORT_RETRY_BACKOFF).await;
                let retried = attempt(rpc_request).await;
                if let Err(ref e2) = retried {
                    diag!("engine retry also failed: {}", transport_error_label(e2));
                }
                retried
            }
            Err(err) => Err(err),
        }
    });

    match result {
        Ok(response) => Ok(response),
        Err(RpcError::Status(s)) if s.code() == tonic::Code::Unauthenticated => {
            Err(grpc_unauthenticated_to_jsonrpc_error(id, &s))
        }
        Err(RpcError::Status(s)) => {
            diag!("engine RPC returned {}", grpc_code_label(s.code()));
            Err(return_tool_error(id, &status_message(&s)))
        }
        Err(RpcError::Transport(e)) => {
            diag!("engine RPC failed (transport): {}", e);
            Err(return_tool_error(id, &e))
        }
    }
}

/// A transport error is retryable: a pre-RPC connect/transport failure, or a
/// gRPC status of Unavailable (engine restarting / broken pipe). Deterministic
/// gRPC errors (FAILED_PRECONDITION, INVALID_ARGUMENT, etc.) are NOT retried.
fn is_retryable_transport_error(err: &RpcError) -> bool {
    match err {
        RpcError::Transport(_) => true,
        RpcError::Status(s) => s.code() == tonic::Code::Unavailable,
    }
}

fn transport_error_label(err: &RpcError) -> String {
    match err {
        RpcError::Transport(e) => format!("transport: {}", e),
        RpcError::Status(s) => format!("grpc {}", grpc_code_label(s.code())),
    }
}

/// Test seam (Fix #2 BDD): when `ANVIL_TEST_FORCE_TRANSPORT_FAIL_ONCE` names a
/// marker file, the FIRST engine call in the process fails with a transport
/// error, then the marker is removed so the retry (and every later call)
/// proceeds normally. This lets a feature exercise the reconnect-and-retry
/// path against a real running engine without killing it.
fn take_force_transport_fail_once() -> bool {
    let marker = match std::env::var("ANVIL_TEST_FORCE_TRANSPORT_FAIL_ONCE") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => return false,
    };
    // The marker file's presence is the one-shot flag. Remove it so only the
    // first call is forced to fail.
    if std::path::Path::new(&marker).exists() {
        let _ = std::fs::remove_file(&marker);
        return true;
    }
    // No marker file yet means the harness wants the env present without a
    // pre-created file — fall back to a process-local one-shot.
    static FORCED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    !FORCED.swap(true, std::sync::atomic::Ordering::SeqCst)
}

/// Map a gRPC UNAUTHENTICATED status returned by the engine to a JSON-RPC
/// protocol-level error (spec Req 4 — surface to the client, not silently fail).
fn grpc_unauthenticated_to_jsonrpc_error(
    id: &serde_json::Value,
    status: &tonic::Status,
) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": -32001,
            "message": "not_authenticated",
            "data": {
                "code": "not_authenticated",
                "detail": status.message()
            }
        }
    })
}

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    // Determine operating mode from the Foundry session token env var.
    // Returns Standalone (no token) or Foundry(jwt) (token present).
    // The token is forwarded per-call as authorization:Bearer metadata.
    let foundry_mode = observe_foundry_session();

    // Fix #1: startup breadcrumb so the harness MCP log records the shim
    // version, mode, and engine endpoint at boot. Without this, a crash before
    // any response is fully invisible.
    let startup_engine_port = std::env::var("ANVIL_ENGINE_PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(DEFAULT_ENGINE_PORT);
    diag!(
        "startup v{} (mode={}, engine_addr=127.0.0.1:{})",
        env!("CARGO_PKG_VERSION"),
        match foundry_mode {
            FoundryMode::Foundry(_) => "foundry",
            FoundryMode::Standalone => "standalone",
        },
        startup_engine_port
    );

    // Launch-default hearth resolved after initialize (explicit --hearth, then MCP roots,
    // then cwd fallback). Tool handlers still resolve their effective hearth per call.
    let explicit_hearth_path = cli_hearth_path();
    let mut launch_default_hearth: Option<Result<PathBuf, String>> = None;

    // Engine connection state
    let mut engine_port: u16 = 0;

    // Session state from checkin
    let mut session: Option<SessionState> = None;

    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        if line.trim().is_empty() {
            continue;
        }

        let mut request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Skip notifications (no id field per JSON-RPC 2.0)
        if request.get("id").is_none() {
            continue;
        }

        // Own `method` and `id` so the tools/call arm can rewrite `request`
        // (forwarding the normalized artifact_path) without holding a borrow of
        // it across the dispatch match.
        let method = request["method"].as_str().unwrap_or("").to_string();
        let id_owned = request["id"].clone();
        let id = &id_owned;

        let response = match method.as_str() {
            "initialize" => {
                // Extract workspace root from MCP roots parameter
                let root_dir = extract_root_dir(&request);
                launch_default_hearth = Some(resolve_hearth_config(
                    explicit_hearth_path.as_deref(),
                    root_dir.as_deref(),
                ));

                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": request["params"]["protocolVersion"]
                            .as_str()
                            .unwrap_or("2024-11-05"),
                        "capabilities": {
                            "tools": {}
                        },
                        "serverInfo": {
                            "name": "anvil-mcp",
                            "version": env!("CARGO_PKG_VERSION")
                        }
                    }
                })
            }
            "tools/list" => {
                let mut listing = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "tools": [
                            {
                                "name": "catalog",
                                "description": "List active artifacts and available artifact types in the hearth",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "hearth": { "type": "string", "description": "Optional: absolute path to the target hearth for this call." },
                                        "project": { "type": "string", "description": "Optional: absolute path to a project directory containing .hearth." }
                                    },
                                    "required": []
                                }
                            },
                            {
                                "name": "checkin",
                                "description": "Check in to start work on an artifact. Returns a role-filtered view of active artifacts and the canonical actor_name for this conversation. actor_name is optional; if supplied, the engine echoes the caller-supplied name in its response (use it to carry identity across conversation boundaries); if absent, a fresh {Word}-{6 digits} name is generated.",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "role": { "type": "string", "description": "Intent: 'creator', 'resumer', or 'reviewer'" },
                                        "hearth": { "type": "string", "description": "Optional: absolute path to the target hearth for this call." },
                                        "project": { "type": "string", "description": "Optional: absolute path to a project directory containing .hearth." },
                                        "actor_name": { "type": "string", "description": "Optional: caller-supplied actor identifier ({Word}-{6 digits}). If non-empty, echoed verbatim; if omitted, the engine generates one." },
                                        "actor_type": { "type": "string", "description": "Actor type (e.g., 'agent')" },
                                        "actor_model": { "type": "string", "description": "Model identifier (e.g., 'claude-opus-4-6')" },
                                        "actor_provider": { "type": "string", "description": "Provider name (e.g., 'anthropic')" },
                                        "actor_context_window": { "type": "integer", "description": "Context window size" },
                                        "actor_sdk_version": { "type": "string", "description": "SDK version" },
                                        "actor_entrypoint": { "type": "string", "description": "Entrypoint identifier" }
                                    },
                                    "required": ["role", "actor_type", "actor_model", "actor_provider"]
                                }
                            },
                            {
                                "name": "describe",
                                "description": "Describe an artifact type or instance. Returns creation schema for types, current state and available actions for instances.",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "hearth": { "type": "string", "description": "Optional: absolute path to the target hearth for this call." },
                                        "project": { "type": "string", "description": "Optional: absolute path to a project directory containing .hearth." },
                                        "identifier": { "type": "string", "description": "Artifact type name (e.g., 'track') or instance id" }
                                    },
                                    "required": ["identifier"]
                                }
                            },
                            {
                                "name": "begin",
                                "description": "Begin work on an artifact. Creation mode: set artifact_type + parent_id + track_name + approver to create a new artifact. Resume/review mode: set identifier to act on an existing artifact — the prior `checkin` call's role determines whether this is review or resume. actor_name and the runtime actor_* fields are required arguments that the caller must supply on every call.",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "hearth": { "type": "string", "description": "Optional: absolute path to the target hearth for this call." },
                                        "project": { "type": "string", "description": "Optional: absolute path to a project directory containing .hearth." },
                                        "artifact_type": { "type": "string", "description": "Creation mode: the type of artifact to create (currently only 'track'). Mutually exclusive with identifier." },
                                        "parent_id": { "type": "string", "description": "Creation mode: the parent proposal directory name" },
                                        "track_name": { "type": "string", "description": "Creation mode: descriptive name for the new track" },
                                        "playbook_name": { "type": "string", "description": "Creation mode: canonical name for the new playbook." },
                                        "approver": { "type": "string", "description": "Creation mode: human who authorized this action" },
                                        "identifier": { "type": "string", "description": "Resume/review mode: the existing artifact id. Mutually exclusive with artifact_type." },
                                        "adopt": { "type": "boolean", "description": "Resume/review mode: set true to ADOPT an artifact authored outside the engine. The engine resets it to its machine's initial state (recording an adoption event) and drives it through every phase and review gate; its existing files are preserved as raw material. Default false leaves a plain begin unchanged." },
                                        "actor_name": { "type": "string", "description": "Required: caller-supplied actor identifier ({Word}-{6 digits})." },
                                        "actor_type": { "type": "string", "description": "Required: 'agent' or 'human'." },
                                        "actor_model": { "type": "string", "description": "Required: model identifier." },
                                        "actor_provider": { "type": "string", "description": "Required: provider name." },
                                        "actor_context_window": { "type": "integer", "description": "Optional: context window size." },
                                        "actor_sdk_version": { "type": "string", "description": "Optional: SDK version." },
                                        "actor_entrypoint": { "type": "string", "description": "Optional: entrypoint identifier." },
                                        "conversation_id": { "type": "string", "description": "Optional: originating conversation/session id for resumable begin correlation." },
                                        "surface": { "type": "string", "description": "Optional: originating surface label. BeginRequest has no source field today; route records use this as source." },
                                        "target_owner": { "type": "string", "description": "Optional: owner-descriptor (e.g. 'kit:<id>' or 'user:<space>') the generated playbook is destined for. Required for artifact kinds whose machine declares it." },
                                        "fields": { "type": "object", "additionalProperties": { "type": "string" }, "description": "Creation mode: a map of any machine-declared required field (outside the builtin set) to its value, e.g. { \"question\": \"...\", \"requester\": \"...\" }. Required fields a kind's machine declares that are not builtins (name/parent_id/approver/target_owner) are supplied here." }
                                    },
                                    "required": ["actor_name", "actor_type", "actor_model", "actor_provider"],
                                    "oneOf": begin_fields_one_of()
                                }
                            },
                            {
                                "name": "anvil_orchestrate",
                                "description": "The universal surface→Anvil handoff: ship a user's message into Anvil's orchestration. Two-phase (LLM-selects, engine-executes): call with no `selection` to ROUTE — Anvil returns the active driven candidate artifact kinds (kind, description, required_fields) plus a next_call; select one and re-call with `selection` set to that kind to BEGIN a playbook run for it (returns the first per-(state,role) hook context_text + the step's intent/expected_output from machine.yaml + a next_call to continue). An unroutable message yields a typed no_match → candidate_playbook_intake outcome (not an error). The surface's agent is the doer; Anvil routes + guides. Surface-agnostic (the surface field is recorded, never branches).",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "hearth": { "type": "string", "description": "Optional: absolute path to the target hearth for this call." },
                                        "project": { "type": "string", "description": "Optional: absolute path to a project directory containing .hearth." },
                                        "message": { "type": "string", "description": "The user's message to ship into Anvil. Required in route-mode; optional in begin-mode when the selection and required creation fields are supplied." },
                                        "selection": { "type": "string", "description": "Optional: the chosen candidate artifact kind to begin a playbook run for. Blank → route-mode (returns candidates); set → begin-mode (begins a run of the selected driven kind)." },
                                        "track_name": { "type": "string", "description": "Creation mode: explicit track name for playbooks whose machine requires name." },
                                        "name": { "type": "string", "description": "Creation mode alias for track_name." },
                                        "parent_id": { "type": "string", "description": "Creation mode: parent artifact id. scope.parent_id is still accepted for compatibility." },
                                        "playbook_name": { "type": "string", "description": "Creation mode — BUILTIN field, pass TOP-LEVEL (NOT inside `fields`). The new playbook's name." },
                                        "target_owner": { "type": "string", "description": "Creation mode — BUILTIN field, pass TOP-LEVEL (NOT inside `fields`). The owning kit/home for driven kinds that declare it (e.g. playbook_generation, the playbook-authoring kind)." },
                                        "approver": { "type": "string", "description": "Creation mode — BUILTIN field, pass TOP-LEVEL. The gatekeeper authorizing the begin, for kinds whose machine requires it." },
                                        "fields": { "type": "object", "description": "Creation mode — map of NON-builtin machine-declared required fields ONLY (e.g. lore_query's question/requester). BUILTIN fields (name/track_name, parent_id, approver, playbook_name, target_owner) do NOT go here — pass them top-level. Compare a kind's required_fields (from route-mode) against the builtin set to decide placement." },
                                        "confidence": { "type": "string", "description": "Optional: surface confidence for the selected routing decision." },
                                        "routing_hint": { "type": "string", "description": "Optional: a coarse routing hint (a ranking input, never the decider)." },
                                        "candidate_set": { "type": "string", "description": "Optional: candidate artifact kinds from route-mode next_call, echoed into begin-mode routing_decision measurement." },
                                        "surface": { "type": "string", "description": "Originating surface: claude-desktop | kiln | claude-code | codex (recorded, never branches)." },
                                        "scope": { "type": "object", "description": "Routing scope, e.g. { parent_id } for the parent proposal." },
                                        "ctx": { "type": "object", "description": "Request access context { org, space, role, clearance }. Standalone default is Foundation/read/internal." },
                                        "ctx_org": { "type": "string", "description": "Optional flat access context org; overridden by ctx.org when supplied." },
                                        "ctx_space": { "type": "string", "description": "Optional flat access context space; overridden by ctx.space when supplied." },
                                        "ctx_role": { "type": "string", "description": "Optional flat access context role; overridden by ctx.role when supplied." },
                                        "ctx_clearance": { "type": "string", "description": "Optional flat access context clearance; overridden by ctx.clearance when supplied." },
                                        "conversation_id": { "type": "string", "description": "Conversation/session id." },
                                        "actor_name": { "type": "string", "description": "Required: caller-supplied actor identifier." },
                                        "actor_type": { "type": "string", "description": "Required: 'agent' or 'human'." },
                                        "actor_model": { "type": "string", "description": "Required: model identifier." },
                                        "actor_provider": { "type": "string", "description": "Required: provider name." },
                                        "approver": { "type": "string", "description": "Optional: human who authorized creation." },
                                        "fields": { "type": "object", "additionalProperties": { "type": "string" }, "description": "Begin-mode: a map of any machine-declared required field (outside the builtin set) to its value, e.g. { \"question\": \"...\", \"requester\": \"...\" }. A route/begin that is missing required generic fields returns a missing_required_fields outcome whose next_call.arguments.fields is pre-keyed with exactly the field names to fill." }
                                    },
                                    "required": ["actor_name", "actor_type", "actor_model", "actor_provider"]
                                }
                            },
                            {
                                "name": "candidate_playbook_intake",
                                "description": "Begin a seeded playbook_generation builder instance from a Lore CandidatePlaybook.",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "source": { "type": "string", "description": "Candidate source, e.g. lore." },
                                        "intent": { "type": "string", "description": "User-facing playbook intent to transform into playbook_name." },
                                        "at": { "type": "string", "description": "Candidate emission timestamp." },
                                        "evidence": {
                                            "type": "array",
                                            "items": { "type": "string" },
                                            "description": "Observation or evidence identifiers supporting the candidate."
                                        },
                                        "route_description": {
                                            "type": "string",
                                            "description": "Required route.description for the generated playbook: action-verb-led and explicit about NOT-this contrasts."
                                        },
                                        "route_triggers": {
                                            "type": "array",
                                            "items": { "type": "string" },
                                            "description": "Required concrete example trigger phrases for the generated playbook."
                                        },
                                        "projection_targets": {
                                            "type": "array",
                                            "items": { "type": "string" },
                                            "description": "Required projection targets to place on generated playbook states."
                                        },
                                        "proposed_states": {
                                            "type": "array",
                                            "items": {
                                                "type": "object",
                                                "properties": {
                                                    "state": { "type": "string" },
                                                    "role": { "type": "string" },
                                                    "intent": { "type": "string" },
                                                    "expected_output": { "type": "string" }
                                                },
                                                "required": ["state", "role", "intent", "expected_output"]
                                            },
                                            "description": "Candidate state proposals carried into the seeded builder context."
                                        },
                                        "target_owner": { "type": "string", "description": "Required: caller/Foundry-resolved owner-home or owner descriptor for the generated playbook." },
                                        "parent_id": { "type": "string", "description": "Required: active parent track id under which the playbook_generation is recorded." },
                                        "approver": { "type": "string", "description": "Optional: authorizer; defaults to lore." },
                                        "actor_name": { "type": "string", "description": "Required: caller-supplied actor identifier." },
                                        "actor_type": { "type": "string", "description": "Required: 'agent' or 'human'." },
                                        "actor_model": { "type": "string", "description": "Required: model identifier." },
                                        "actor_provider": { "type": "string", "description": "Required: provider name." },
                                        "actor_context_window": { "type": "integer", "description": "Optional: context window size." },
                                        "actor_sdk_version": { "type": "string", "description": "Optional: SDK version." },
                                        "actor_entrypoint": { "type": "string", "description": "Optional: entrypoint identifier." }
                                    },
                                    "required": ["source", "intent", "at", "route_description", "route_triggers", "projection_targets", "proposed_states", "target_owner", "parent_id", "actor_name", "actor_type", "actor_model", "actor_provider"]
                                }
                            },
                            {
                                "name": "snapshot",
                                "description": "Record a state transition on a forge artifact. Writes the transition to status.yaml, moves the registry entry to the consolidated target section, and updates the relevant projection. actor_name and the runtime actor_* fields are required arguments that the caller must supply on every call.",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "hearth": { "type": "string", "description": "Optional: absolute path to the target hearth for this call." },
                                        "project": { "type": "string", "description": "Optional: absolute path to a project directory containing .hearth." },
                                        "artifact_path": { "type": "string", "description": "Relative path under the hearth (e.g., 'tracks/20260416T0155_...')." },
                                        "to_state": { "type": "string", "description": "Target state (e.g., 'spec_review', 'plan', 'implementing')." },
                                        "actor_role": { "type": "string", "description": "What role the actor played (e.g., 'spec', 'review', 'implement')." },
                                        "approver": { "type": "string", "description": "Optional: who authorized the transition." },
                                        "note": { "type": "string", "description": "Optional: free-text note recorded on the transition." },
                                        "projection_only": { "type": "boolean", "description": "Optional: set true for spark/annotation events — skips status.yaml and registry writes." },
                                        "event_type": { "type": "string", "description": "Required in projection-only mode: 'spark' or 'annotation'." },
                                        "actor_name": { "type": "string", "description": "Required: caller-supplied actor identifier ({Word}-{6 digits}). The engine echoes this value verbatim and writes it to the transition's actor field." },
                                        "actor_type": { "type": "string", "description": "Required: 'agent' or 'human'." },
                                        "actor_model": { "type": "string", "description": "Required: model identifier." },
                                        "actor_provider": { "type": "string", "description": "Required: provider name." },
                                        "actor_context_window": { "type": "integer", "description": "Optional: context window size." },
                                        "actor_sdk_version": { "type": "string", "description": "Optional: SDK version." },
                                        "actor_entrypoint": { "type": "string", "description": "Optional: entrypoint identifier." }
                                    },
                                    "required": ["artifact_path", "to_state", "actor_role", "actor_name", "actor_type", "actor_model", "actor_provider"]
                                }
                            },
                            {
                                "name": "complete",
                                "description": "Declare the current pass finished on a forge artifact. Doer path (no satisfaction): advances a spec track from spec → spec_review. Reviewer path (satisfaction: 'satisfied'): advances spec_review → plan. Reviewer carry-forward path (satisfaction: 'address_in_next_step'): advances spec_review → plan AND writes carry-forward.md with the verbatim findings for the plan-phase doer to address — the findings field is required for this path, and the response carries carry_forward_path. No prior checkin required — supply held actor_name and re-detected runtime actor_* fields directly. actor_name and the runtime actor_* fields are required on every call. Any complete call may carry an optional reflection_notes string; when non-empty, the engine persists it to a per-phase reflection file under <source_state>_reflection/. Omitting the field leaves behavior unchanged.",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "hearth": { "type": "string", "description": "Optional: absolute path to the target hearth for this call." },
                                        "project": { "type": "string", "description": "Optional: absolute path to a project directory containing .hearth." },
                                        "artifact_path": { "type": "string", "description": "Required: relative path under the hearth (e.g., 'tracks/20260416T0155_...')." },
                                        "actor_name": { "type": "string", "description": "Required: caller-supplied actor identifier ({Word}-{6 digits})." },
                                        "actor_type": { "type": "string", "description": "Required: 'agent' or 'human'." },
                                        "actor_model": { "type": "string", "description": "Required: model identifier." },
                                        "actor_provider": { "type": "string", "description": "Required: provider name." },
                                        "actor_context_window": { "type": "integer", "description": "Optional: context window size." },
                                        "actor_sdk_version": { "type": "string", "description": "Optional: SDK version." },
                                        "actor_entrypoint": { "type": "string", "description": "Optional: entrypoint identifier." },
                                        "satisfaction": { "type": "string", "description": "Optional: reviewer judgment. Omit for doer path. 'satisfied' advances spec_review → plan; 'full_revision' advances spec_review → spec_revision; 'address_in_next_step' advances spec_review → plan and carries findings forward." },
                                        "findings": { "type": "string", "description": "Required when satisfaction is 'address_in_next_step': verbatim reviewer findings that the plan-phase doer must address. Written to carry-forward.md and delivered on the next begin." },
                                        "approver": { "type": "string", "description": "Optional: who authorized this transition." },
                                        "note": { "type": "string", "description": "Optional: free-text note." },
                                        "reflection_notes": { "type": "string", "description": "Optional: free-form narrative this actor wants to persist as a per-phase reflection. When non-empty, the engine writes it to <source_state>_reflection/<timestamp>-<actor>.md and returns reflection_path in the response." },
                                        "claimed_evidence": { "type": "array", "items": { "type": "object", "properties": { "class": { "type": "string" }, "reference": { "type": "string" } } }, "description": "Optional: ordered evidence claims for this completion — {class, reference} pairs assessed against the step's obligation. class one of artifact_of_consequence | verifiable_citation | self_description; reference is an identifier (e.g. commit:<sha> or <file>:<line>) copied verbatim into the measurement record. On a completion that lands the artifact in `completed` it is also MERGE-CHECKED: a cited commit must be an ancestor of that repository's origin/main and a cited path must exist, or the completion is refused before any write. Name the repository for cross-repo claims — commit:<repo>@<sha>, <repo>@<path>. Omitted or empty is byte-identical to the pre-affordance behavior." }
                                    },
                                    "required": ["artifact_path", "actor_name", "actor_type", "actor_model", "actor_provider"]
                                }
                            },
                            {
                                "name": "amend",
                                "description": "Record a structured amendment op (add/revise/retire/reorder) against a frozen artifact document. Validates the op against the amendment-kind's content schema and the document's existing op log, then appends it atomically. For a track artifact in `completed` state, also drives the `completed → amend` transition. actor_name and the runtime actor_* fields are required on every call.",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "hearth": { "type": "string", "description": "Optional: absolute path to the target hearth for this call." },
                                        "project": { "type": "string", "description": "Optional: absolute path to a project directory containing .hearth." },
                                        "artifact_path": { "type": "string", "description": "Required: relative path under the hearth (e.g., 'tracks/20260416T0155_...')." },
                                        "kind": { "type": "string", "description": "Required: amendment kind — one of spec, plan, proposal, track, milestone, decision, learning, playbook." },
                                        "target_document": { "type": "string", "description": "Required: the frozen document to amend (e.g., 'spec', 'plan')." },
                                        "target_id": { "type": "string", "description": "Required: element id within the document's op log." },
                                        "op_kind": { "type": "string", "description": "Required: operation kind — add, revise, retire, or reorder." },
                                        "body": { "type": "string", "description": "Optional: op body payload (required for add/revise, omitted for retire)." },
                                        "new_kind": { "type": "string", "description": "Optional: new element kind for add ops." },
                                        "anchor": { "type": "string", "description": "Optional: positioning anchor for add/reorder ops." },
                                        "actor_name": { "type": "string", "description": "Required: caller-supplied actor identifier ({Word}-{6 digits})." },
                                        "actor_type": { "type": "string", "description": "Required: 'agent' or 'human'." },
                                        "actor_model": { "type": "string", "description": "Required: model identifier." },
                                        "actor_provider": { "type": "string", "description": "Required: provider name." },
                                        "actor_context_window": { "type": "integer", "description": "Optional: context window size." },
                                        "actor_sdk_version": { "type": "string", "description": "Optional: SDK version." },
                                        "actor_entrypoint": { "type": "string", "description": "Optional: entrypoint identifier." }
                                    },
                                    "required": ["artifact_path", "kind", "target_document", "target_id", "op_kind", "actor_name", "actor_type", "actor_model", "actor_provider"]
                                }
                            },
                            {
                                "name": "persist_playbook",
                                "description": "Surface PersistPlaybook: write a generated playbook machine.yaml to <owner_home>/playbooks/<kind>/, registry-resolvable. owner_home is a pre-resolved absolute path (Foundry resolves the owner descriptor upstream).",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "owner_home": { "type": "string", "description": "Required: pre-resolved absolute owner-home path. The engine writes under <owner_home>/playbooks/<kind>/." },
                                        "kind": { "type": "string", "description": "Required: generated playbook kind and directory name under playbooks/." },
                                        "machine_yaml": { "type": "string", "description": "Required: generated playbook machine.yaml text." },
                                        "actor_name": { "type": "string", "description": "Required: caller-supplied actor identifier ({Word}-{6 digits})." },
                                        "actor_type": { "type": "string", "description": "Required: 'agent' or 'human'." },
                                        "actor_model": { "type": "string", "description": "Required: model identifier." },
                                        "actor_provider": { "type": "string", "description": "Required: provider name." },
                                        "actor_context_window": { "type": "integer", "description": "Optional: context window size." },
                                        "actor_sdk_version": { "type": "string", "description": "Optional: SDK version." },
                                        "actor_entrypoint": { "type": "string", "description": "Optional: entrypoint identifier." },
                                        "hooks": {
                                            "type": "array",
                                            "description": "Optional: role hook files written beside machine.yaml under hooks/. Each state/transition `hook:` in the playbook's machine.yaml must reference a filename carried here, else the persist is rejected (playbook_unknown_hook_reference). Absent/empty = machine.yaml-only.",
                                            "items": {
                                                "type": "object",
                                                "properties": {
                                                    "name": { "type": "string", "description": "Hook filename under hooks/ (e.g. 'intent.md'), matching a machine.yaml `hook:` reference." },
                                                    "content": { "type": "string", "description": "The hook body." }
                                                },
                                                "required": ["name", "content"]
                                            }
                                        }
                                    },
                                    "required": ["owner_home", "kind", "machine_yaml", "actor_name", "actor_type", "actor_model", "actor_provider"]
                                }
                            },
                            {
                                "name": "begin_adoption_status",
                                "description": "Pure-read query: returns whether (actor_name, artifact_path, state) has an open begin-marker. True when the actor called begin() for this artifact in this state and has not yet completed/snapshotted. False when never begun or after the closing transition. The harness hard-gate consults this before allowing edit-tool access.",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "actor_name": { "type": "string", "description": "Required: the actor identifier to query." },
                                        "artifact_path": { "type": "string", "description": "Required: relative path under the hearth (e.g., 'tracks/20260416T0155_...')." },
                                        "state": { "type": "string", "description": "Required: the state to check (e.g., 'spec_review'). Matching is state-scoped." }
                                    },
                                    "required": ["actor_name", "artifact_path", "state"]
                                }
                            }
                        ]
                    }
                });
                if let Some(tools) = listing["result"]["tools"].as_array_mut() {
                    tools.extend(backlog_tool_definitions());
                }
                listing
            }
            "tools/call" => {
                // Own the tool name so `request` can be mutated below (to forward
                // the normalized artifact_path) without an outstanding borrow.
                let tool_name = request["params"]["name"].as_str().map(str::to_string);
                match tool_name {
                    None => {
                        serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": {
                                "code": -32602,
                                "message": "Invalid params: missing required field 'name'",
                                "data": { "code": "invalid_params" }
                            }
                        })
                    }
                    Some(tool_name) => {
                        if !tool_uses_hearth(&tool_name) {
                            return_unknown_tool(id, &tool_name)
                        } else {
                            let launch_default = launch_default_hearth.get_or_insert_with(|| {
                                resolve_hearth_config(explicit_hearth_path.as_deref(), None)
                            });
                            let launch_default = launch_default.as_ref().ok().map(PathBuf::as_path);
                            let disambiguator = RefuseDisambiguator;
                            match call_hearth_or_refuse(
                                id,
                                &request["params"]["arguments"],
                                launch_default,
                                &disambiguator,
                            ) {
                                Ok((hp, normalized_artifact_path)) => {
                                    // (Item 1) Forward the NORMALIZED
                                    // hearth-relative path the shim just
                                    // validated — validated form == used form —
                                    // so the engine re-derives the identical,
                                    // symlink-resolved location.
                                    if let Some(normalized) = normalized_artifact_path {
                                        request["params"]["arguments"]["artifact_path"] =
                                            serde_json::Value::String(normalized);
                                    }
                                    match tool_name.as_str() {
                                        "catalog" => handle_catalog_call(
                                            id,
                                            &hp,
                                            &mut engine_port,
                                            &rt,
                                            &foundry_mode,
                                        ),
                                        "checkin" => handle_checkin_call(
                                            id,
                                            &request,
                                            &hp,
                                            &mut engine_port,
                                            &rt,
                                            &mut session,
                                            &foundry_mode,
                                        ),
                                        "describe" => handle_describe_call(
                                            id,
                                            &request,
                                            &hp,
                                            &mut engine_port,
                                            &rt,
                                            &foundry_mode,
                                        ),
                                        "begin" => handle_begin_call(
                                            id,
                                            &request,
                                            &hp,
                                            &mut engine_port,
                                            &rt,
                                            &session,
                                            &foundry_mode,
                                        ),
                                        "anvil_orchestrate" => handle_anvil_orchestrate_call(
                                            id,
                                            &request,
                                            &hp,
                                            &mut engine_port,
                                            &rt,
                                            &foundry_mode,
                                        ),
                                        "snapshot" => handle_snapshot_call(
                                            id,
                                            &request,
                                            &hp,
                                            &mut engine_port,
                                            &rt,
                                            &mut session,
                                            &foundry_mode,
                                        ),
                                        "complete" => handle_complete_call(
                                            id,
                                            &request,
                                            &hp,
                                            &mut engine_port,
                                            &rt,
                                            &foundry_mode,
                                        ),
                                        "amend" => handle_amend_call(
                                            id,
                                            &request,
                                            &hp,
                                            &mut engine_port,
                                            &rt,
                                            &foundry_mode,
                                        ),
                                        "persist_playbook" => handle_persist_playbook_call(
                                            id,
                                            &request,
                                            &hp,
                                            &mut engine_port,
                                            &rt,
                                            &foundry_mode,
                                        ),
                                        "candidate_playbook_intake" => {
                                            handle_intake_candidate_playbook_call(
                                                id,
                                                &request,
                                                &hp,
                                                &mut engine_port,
                                                &rt,
                                                &foundry_mode,
                                            )
                                        }
                                        "begin_adoption_status" => {
                                            handle_begin_adoption_status_call(
                                                id,
                                                &request,
                                                &hp,
                                                &mut engine_port,
                                                &rt,
                                                &foundry_mode,
                                            )
                                        }
                                        "backlog_evaluate" => handle_backlog_evaluate_call(
                                            id,
                                            &request,
                                            &hp,
                                            &mut engine_port,
                                            &rt,
                                            &foundry_mode,
                                        ),
                                        "backlog_organ_queue" | "backlog_cross_organ_view" => {
                                            handle_backlog_queue_call(
                                                id,
                                                &tool_name,
                                                &request,
                                                &hp,
                                                &mut engine_port,
                                                &rt,
                                                &foundry_mode,
                                            )
                                        }
                                        other if BACKLOG_TOOLS.contains(&other) => {
                                            handle_backlog_mutate_call(
                                                id,
                                                &tool_name,
                                                &request,
                                                &hp,
                                                &mut engine_port,
                                                &rt,
                                                &foundry_mode,
                                            )
                                        }
                                        _ => {
                                            serde_json::json!({
                                                "jsonrpc": "2.0",
                                                "id": id,
                                                "result": {
                                                    "isError": true,
                                                    "content": [{
                                                        "type": "text",
                                                        "text": format!("Unknown tool: {}", tool_name)
                                                    }]
                                                }
                                            })
                                        }
                                    }
                                }
                                Err(error_response) => error_response,
                            }
                        }
                    } // close Some(tool_name)
                }
            }
            _ => {
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32601,
                        "message": format!("Method not found: {}", method),
                        "data": { "code": "method_not_found" }
                    }
                })
            }
        };

        // Fix #3: a single request must never panic the process. The response
        // envelope is built from our own json! values so serialization should
        // not fail, but if it ever did we degrade to a JSON-RPC error line
        // rather than aborting the whole shim.
        let response_str = match serde_json::to_string(&response) {
            Ok(s) => s,
            Err(e) => {
                diag!("response serialization failed: {}", e);
                format!(
                    "{{\"jsonrpc\":\"2.0\",\"id\":{},\"error\":{{\"code\":-32603,\"message\":\"internal serialization error\"}}}}",
                    id
                )
            }
        };
        // A write/flush failure means stdout (the client pipe) is gone; there
        // is nothing left to serve, so exit the loop cleanly instead of
        // panicking.
        if writeln!(stdout, "{}", response_str).is_err() || stdout.flush().is_err() {
            diag!("stdout closed; shutting down");
            break;
        }
    }

    // The shim owns no engine lifecycle. Shutdown drops stdio and gRPC clients only.
}

/// Extract the workspace root directory from MCP initialize roots.
///
/// MCP clients send `params.roots` in the initialize request, each with
/// a `uri` field (e.g., `file:///path/to/project`). The first root is
/// used as the base directory for `.hearth` discovery.
fn extract_root_dir(request: &Value) -> Option<PathBuf> {
    let roots = request["params"]["roots"].as_array()?;
    let first_root = roots.first()?;
    let uri = first_root["uri"].as_str()?;

    // Strip file:// prefix
    let path = uri.strip_prefix("file://").unwrap_or(uri);
    Some(PathBuf::from(path))
}

/// Read an explicit `--hearth <path>` CLI override.
///
/// The shell wrapper passes this argument after resolving ANVIL_HEARTH_PATH,
/// project `.hearth`, or its fallback. The compiled MCP binary must honor it;
/// otherwise the wrapper's resolution is silently discarded and the shim may
/// connect to the wrong hearth.
fn cli_hearth_path() -> Option<PathBuf> {
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--hearth" {
            return args.next().map(PathBuf::from);
        }
    }
    None
}

fn resolve_hearth_config(
    explicit_hearth_path: Option<&Path>,
    root_dir: Option<&Path>,
) -> Result<PathBuf, String> {
    if let Some(path) = explicit_hearth_path {
        if path.is_dir() {
            return Ok(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()));
        }
        return Err(format!(
            "Explicit --hearth path does not resolve to a directory: {}",
            path.display()
        ));
    }

    read_hearth_config(root_dir)
}

/// Read the .hearth file to resolve the hearth path.
///
/// Searches for `.hearth` in: the MCP root directory (if provided),
/// then the current working directory as fallback.
fn read_hearth_config(root_dir: Option<&Path>) -> Result<PathBuf, String> {
    let search_dir = match root_dir {
        Some(dir) if dir.join(".hearth").exists() => dir.to_path_buf(),
        _ => std::env::current_dir()
            .map_err(|e| format!("Cannot determine working directory: {}", e))?,
    };

    read_hearth_config_file(&search_dir.join(".hearth"))
}

fn read_hearth_config_file(hearth_file: &Path) -> Result<PathBuf, String> {
    if !hearth_file.exists() {
        let search_dir = hearth_file.parent().unwrap_or_else(|| Path::new("."));
        return Err(format!(
            "No .hearth file found in {}. Create a .hearth file with 'path: <hearth-directory>'.",
            search_dir.display()
        ));
    }

    let content = std::fs::read_to_string(&hearth_file)
        .map_err(|e| format!("Failed to read .hearth: {}", e))?;

    #[derive(serde::Deserialize)]
    struct HearthConfig {
        path: String,
    }

    let config: HearthConfig =
        serde_yaml::from_str(&content).map_err(|e| format!("Invalid .hearth format: {}", e))?;

    // Resolve relative paths against the .hearth file's parent directory
    let base_dir = hearth_file
        .parent()
        .unwrap_or(&PathBuf::new())
        .to_path_buf();
    let resolved = base_dir.join(&config.path);
    let resolved = resolved.canonicalize().unwrap_or(resolved);

    Ok(resolved)
}

fn canonicalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn resolve_explicit_hearth_signal(explicit: &str) -> CallHearth {
    let trimmed = explicit.trim();
    if trimmed.is_empty() {
        return CallHearth::Ambiguous("empty explicit hearth/project argument".to_string());
    }
    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return CallHearth::Ambiguous(format!(
            "explicit hearth/project must be an absolute path: {}",
            trimmed
        ));
    }
    if path.join(".hearth").exists() {
        return match read_hearth_config_file(&path.join(".hearth")) {
            Ok(hearth) => CallHearth::Resolved(hearth),
            Err(msg) => CallHearth::Ambiguous(msg),
        };
    }
    if path.is_dir() {
        return CallHearth::Resolved(canonicalize_path(&path));
    }
    CallHearth::Ambiguous(format!(
        "explicit hearth/project path does not resolve to a directory or .hearth-bearing project: {}",
        path.display()
    ))
}

fn derive_hearth_from_artifact_path(artifact_path: &str) -> Result<Option<PathBuf>, String> {
    let trimmed = artifact_path.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let raw_path = PathBuf::from(trimmed);
    // Derivation is a FALLBACK, used only when no explicit hearth/project arg
    // was supplied (see `resolve_call_hearth`'s precedence). An ABSOLUTE
    // artifact_path is walked up directly; a RELATIVE one is resolved against
    // the shim's cwd first, then walked up to the nearest `.hearth`. Restoring
    // the relative cwd walk-up preserves the legitimate nested-cwd flow (shim
    // cwd inside a project, no explicit arg, no launch default — the walk-up is
    // the only thing that can resolve the hearth) WITHOUT re-introducing the
    // conflict-block regression: because derivation never runs when an explicit
    // arg is present, a relative artifact_path can no longer compete with or
    // block the caller's explicit target the way it did on the wedged write
    // path. (This is why the earlier read-only tools honored the explicit arg
    // while the write tools returned `ambiguous_hearth` — note `begin_adoption_
    // status` is itself a read tool that DOES carry a relative artifact_path,
    // and now follows the exact same precedence as the write tools.)
    let search_path = if raw_path.is_absolute() {
        raw_path
    } else {
        std::env::current_dir()
            .map_err(|e| format!("Cannot determine working directory: {}", e))?
            .join(raw_path)
    };
    let mut current = if search_path.is_dir() {
        Some(search_path.as_path())
    } else {
        search_path.parent()
    };

    while let Some(dir) = current {
        let hearth_file = dir.join(".hearth");
        if hearth_file.exists() {
            return read_hearth_config_file(&hearth_file).map(Some);
        }
        current = dir.parent();
    }

    Ok(None)
}

fn same_hearth(left: &Path, right: &Path) -> bool {
    canonicalize_path(left) == canonicalize_path(right)
}

fn resolve_call_hearth(
    explicit: Option<&str>,
    artifact_path: Option<&str>,
    launch_default: Option<&Path>,
) -> CallHearth {
    // Precedence:
    //   explicit arg
    //     > absolute-artifact_path .hearth derivation
    //     > relative-artifact_path cwd walk-up derivation
    //     > launch default
    //     > ambiguous (refuse).
    //
    // An explicit hearth/project arg wins OUTRIGHT — it is the caller's
    // deliberate target. We deliberately do NOT also derive a hearth from the
    // call's artifact_path and let it conflict-block the explicit arg: a
    // relative artifact_path is, by the tool contract, relative to the resolved
    // hearth, so deriving a competing hearth from it (via the shim's cwd) and
    // then refusing on the mismatch is exactly what wedged the live write path.
    // Derivation is therefore a fallback that only runs when the arg is absent.
    if let Some(value) = explicit {
        return resolve_explicit_hearth_signal(value);
    }

    if let Some(value) = artifact_path {
        match derive_hearth_from_artifact_path(value) {
            Ok(Some(path)) => return CallHearth::Resolved(path),
            Ok(None) => {}
            Err(msg) => return CallHearth::Ambiguous(msg),
        }
    }

    if let Some(path) = launch_default {
        return CallHearth::Resolved(canonicalize_path(path));
    }

    CallHearth::Ambiguous(
        "no explicit hearth/project, no artifact_path .hearth, and no launch default hearth"
            .to_string(),
    )
}

/// The single containment gate every resolved-or-disambiguated hearth flows
/// through (Item 3: full routing). Validates `artifact_path` against the
/// resolved `hearth` via the shared core guard and, on success, returns the
/// NORMALIZED, symlink-resolved, hearth-relative form the shim forwards to the
/// engine — so the value the shim VALIDATED is the value the engine USES
/// (Item 1: validate/use split). Any escape is refused with the distinct
/// `invalid_artifact_path` taxonomy, never `ambiguous_hearth`. Returns
/// `Ok((hearth, None))` when there is no artifact_path to check.
fn enforce_and_return(
    id: &Value,
    hearth: PathBuf,
    artifact_path: Option<&str>,
) -> Result<(PathBuf, Option<String>), Value> {
    let Some(artifact_path) = artifact_path else {
        return Ok((hearth, None));
    };
    match containment::contained_relative_path(&hearth, artifact_path) {
        Ok(relative) => {
            let normalized = relative.to_string_lossy().to_string();
            Ok((hearth, Some(normalized)))
        }
        Err(reason) => Err(refuse_error_response(
            id,
            &RefuseError {
                code: "invalid_artifact_path",
                reason,
            },
        )),
    }
}

fn explicit_call_arg(args: &Value) -> Result<Option<String>, String> {
    let hearth = args["hearth"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let project = args["project"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    match (hearth, project) {
        (Some(hearth), Some(project)) => {
            let resolved_hearth = match resolve_explicit_hearth_signal(hearth) {
                CallHearth::Resolved(path) => path,
                CallHearth::Ambiguous(reason) => return Err(reason),
            };
            let resolved_project = match resolve_explicit_hearth_signal(project) {
                CallHearth::Resolved(path) => path,
                CallHearth::Ambiguous(reason) => return Err(reason),
            };
            if same_hearth(&resolved_hearth, &resolved_project) {
                Ok(Some(hearth.to_string()))
            } else {
                Err(format!(
                    "explicit hearth {} conflicts with explicit project {}",
                    resolved_hearth.display(),
                    resolved_project.display()
                ))
            }
        }
        (Some(hearth), None) => Ok(Some(hearth.to_string())),
        (None, Some(project)) => Ok(Some(project.to_string())),
        (None, None) => Ok(None),
    }
}

fn project_root_for_request(args: &Value) -> String {
    args["project"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|path| path.display().to_string())
        })
        .unwrap_or_default()
}

/// Resolve the target hearth for a hearth-bearing tool call and, when the call
/// carries an `artifact_path`, validate it and return its NORMALIZED
/// hearth-relative form for forwarding. `Ok((hearth, Some(normalized)))` when a
/// contained artifact_path was validated, `Ok((hearth, None))` otherwise.
fn call_hearth_or_refuse(
    id: &Value,
    args: &Value,
    launch_default: Option<&Path>,
    disambiguator: &dyn HearthDisambiguator,
) -> Result<(PathBuf, Option<String>), Value> {
    let artifact_path = args["artifact_path"]
        .as_str()
        .filter(|s| !s.trim().is_empty());

    // (Item 3: ordering) Path-INDEPENDENT syntax validation runs BEFORE any
    // hearth resolution. A `..`/drive-relative/rooted artifact_path is refused
    // as `invalid_artifact_path` even when no hearth can be resolved — so a
    // `../x` with no resolvable hearth never masquerades as `ambiguous_hearth`.
    if let Some(artifact_path) = artifact_path {
        if containment::is_syntactically_invalid(artifact_path) {
            return Err(refuse_error_response(
                id,
                &RefuseError {
                    code: "invalid_artifact_path",
                    reason: format!(
                        "artifact_path must be a hearth-relative path with no '..', drive-relative, or rooted forms: {}",
                        artifact_path.trim()
                    ),
                },
            ));
        }
    }

    let explicit = match explicit_call_arg(args) {
        Ok(explicit) => explicit,
        Err(reason) => {
            // (Item 3: full routing) The disambiguated explicit-arg-error hearth
            // flows through the SAME containment gate as every other path —
            // it is no longer returned unchecked.
            let hearth = match disambiguator.disambiguate(&reason) {
                Ok(path) => path,
                Err(err) => return Err(refuse_error_response(id, &err)),
            };
            return enforce_and_return(id, hearth, artifact_path);
        }
    };

    let hearth = match resolve_call_hearth(explicit.as_deref(), artifact_path, launch_default) {
        CallHearth::Resolved(path) => path,
        CallHearth::Ambiguous(reason) => match disambiguator.disambiguate(&reason) {
            Ok(path) => path,
            Err(err) => return Err(refuse_error_response(id, &err)),
        },
    };

    // Containment is enforced AFTER resolution, against whichever hearth the
    // call resolved to (explicit, derived, or launch default), returning the
    // normalized forwarded form (validated form == used form).
    enforce_and_return(id, hearth, artifact_path)
}

fn refuse_error_response(id: &Value, err: &RefuseError) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": -32002,
            "message": err.code,
            "data": {
                "code": err.code,
                "reason": err.reason
            }
        }
    })
}

/// Handle a tools/call for "catalog" — ensure engine is running, call gRPC, return MCP response.
fn handle_catalog_call(
    id: &Value,
    hearth_path: &PathBuf,
    engine_port: &mut u16,
    rt: &tokio::runtime::Runtime,
    foundry_mode: &FoundryMode,
) -> Value {
    let catalog = match call_engine(
        id,
        engine_port,
        rt,
        foundry_mode,
        CatalogRequest {
            hearth_path: hearth_path.display().to_string(),
        },
        |client, request| Box::pin(client.catalog(request)),
        |s| format!("Catalog RPC failed: {}", s.message()),
    ) {
        Ok(catalog) => catalog,
        Err(error_response) => return error_response,
    };

    // Convert proto response to MCP tool result
    let catalog_json = serde_json::json!({
        "active_artifacts": catalog.active_artifacts.iter().map(|a| {
            serde_json::json!({
                "id": a.id,
                "type": a.artifact_type,
                "state": a.state,
                "summary": a.summary
            })
        }).collect::<Vec<_>>(),
        "available_types": catalog.available_types.iter().map(|t| {
            serde_json::json!({
                "name": t.name,
                "description": t.description,
                "requires_parent": t.requires_parent
            })
        }).collect::<Vec<_>>(),
        // Playbook kinds resolvable for this session (engine registry union:
        // request hearth + global-playbooks hearth + seeds). This is what lets
        // a foreign project SEE knowledge_lifecycle/daily_recap/etc. without a
        // per-project --hearth pin (topology A2 added the engine field; the
        // shim must surface it to the MCP client).
        "available_artifact_kinds": catalog.available_artifact_kinds.iter().map(|k| {
            serde_json::json!({
                "kind": k.kind,
                "source_tier": k.source_tier,
                "is_described": k.is_described,
                "has_triggers": k.has_triggers
            })
        }).collect::<Vec<_>>()
    });

    tool_text_response(id, &catalog_json)
}

/// Handle a tools/call for "begin" — check session, ensure engine, call gRPC with session identity.
fn handle_begin_call(
    id: &Value,
    request: &Value,
    hearth_path: &PathBuf,
    engine_port: &mut u16,
    rt: &tokio::runtime::Runtime,
    session: &Option<SessionState>,
    foundry_mode: &FoundryMode,
) -> Value {
    let session = match session {
        Some(s) => s,
        None => {
            return return_tool_error(
                id,
                "No active session. Call checkin first to register your identity and intent.",
            );
        }
    };

    let args = &request["params"]["arguments"];

    // Per spec R5 of the checkin_backfill_spec_context track: the shim
    // forwards caller-supplied actor_* fields verbatim. Session state
    // does not inject identity into outbound requests. `session_role`
    // stays session-sourced because it carries the conversation's
    // intent (creator/reviewer/resumer), not identity.
    let begin_request = BeginRequest {
        hearth_path: hearth_path.display().to_string(),
        artifact_type: args["artifact_type"].as_str().unwrap_or("").to_string(),
        parent_id: args["parent_id"].as_str().unwrap_or("").to_string(),
        track_name: args["track_name"].as_str().unwrap_or("").to_string(),
        playbook_name: playbook_name_arg(args),
        target_owner: args["target_owner"].as_str().unwrap_or("").to_string(),
        // The K8 branch is re-checked HERE, not only in the advertised schema,
        // so a client that bypasses schema validation cannot broaden either
        // path. Non-K8 begins keep the byte-identical string-map behavior.
        create_fields: match begin_create_fields(args) {
            Ok(fields) => fields,
            Err(reason) => return backlog_error(id, &reason),
        },
        approver: args["approver"].as_str().unwrap_or("").to_string(),
        actor_name: args["actor_name"].as_str().unwrap_or("").to_string(),
        actor_type: args["actor_type"].as_str().unwrap_or("").to_string(),
        actor_model: args["actor_model"].as_str().unwrap_or("").to_string(),
        actor_provider: args["actor_provider"].as_str().unwrap_or("").to_string(),
        actor_context_window: args["actor_context_window"].as_i64().unwrap_or(0),
        actor_sdk_version: args["actor_sdk_version"].as_str().unwrap_or("").to_string(),
        actor_entrypoint: args["actor_entrypoint"].as_str().unwrap_or("").to_string(),
        identifier: args["identifier"].as_str().unwrap_or("").to_string(),
        session_role: session.role.clone(),
        // resume-aware routing: record the originating conversation id on the
        // durable open-begin marker (distinct from rd_turn_id below). Defaults to
        // the Claude Code session id (CLAUDE_CODE_SESSION_ID) when the call omits
        // it, so the begin's activity-log record hashes + joins the route leg.
        conversation_id: effective_conversation_id(args),
        // INTERIM (default-deny-safe): the request-time access ctx is hard-coded to the
        // Foundation baseline. It is NOT sourced from agent-supplied tool arguments by
        // design — per decision playbook-ownership-and-access-scoping, access context must
        // come from the authenticated session principal (Foundry stamps effective access;
        // an agent self-declaring org/role/clearance would be the request-time analogue of
        // self-escalation). Sourcing the real ctx from the authenticated principal is
        // Foundry's layer (out of this repo, parallel to owner->path resolution). Until
        // then, default-safe is the correct safe value. FOLLOW-ON: wire principal->ctx.
        ctx_org: "Foundation".to_string(),
        ctx_space: String::new(),
        ctx_role: "read".to_string(),
        ctx_clearance: "internal".to_string(),
        rd_turn_id: String::new(),
        rd_input: String::new(),
        rd_candidate_set: String::new(),
        rd_selected: String::new(),
        rd_confidence: String::new(),
        project_root: project_root_for_request(args),
        // Opt-in adoption of an out-of-engine artifact (identifier mode). The
        // shim forwards the caller-supplied flag verbatim; default false.
        adopt: args["adopt"].as_bool().unwrap_or(false),
        claimed_evidence: Vec::new(),
    };

    let begin = match call_engine(
        id,
        engine_port,
        rt,
        foundry_mode,
        begin_request,
        |client, request| Box::pin(client.begin(request)),
        |s| format!("Begin RPC failed: {}", s.message()),
    ) {
        Ok(begin) => begin,
        Err(error_response) => return error_response,
    };

    let mut result_json = serde_json::json!({
        "track_path": begin.track_path,
        "state": begin.state,
        "context_text": begin.context_text,
        "review_context_text": begin.review_context_text,
        "review_doc_path": begin.review_doc_path,
        "intent": begin.intent,
        "expected_output": begin.expected_output,
        "playbook_id": begin.playbook_id,
    });
    // R2.4: the `artifact_text` key is omitted entirely when empty (field
    // absent, not present-and-empty). The doer-on-`spec_revision` begin leaves
    // it empty so the key is dropped; the reviewer-on-`spec_review` begin always
    // populates it so the key is present — behaviorally equivalent to the
    // previous unconditional inclusion for that path.
    if !begin.artifact_text.is_empty() {
        result_json["artifact_text"] = serde_json::Value::String(begin.artifact_text.clone());
    }

    tool_text_response(id, &result_json)
}

/// Derive a short track name from the surface message (first chars).
fn derive_track_name(message: &str) -> String {
    let slug: String = message.chars().take(60).collect();
    let slug = slug.trim();
    if slug.is_empty() {
        "surface_turn".to_string()
    } else {
        slug.to_string()
    }
}

fn routing_input(message: &str, signal: &str) -> String {
    if signal.is_empty() {
        message.to_string()
    } else {
        format!("{} {}", message, signal)
    }
}

#[derive(Debug, Clone)]
struct OrchestrateRequestContext {
    org: String,
    space: String,
    role: String,
    clearance: String,
}

impl OrchestrateRequestContext {
    fn from_args(args: &Value) -> Self {
        let ctx = &args["ctx"];
        Self {
            org: ctx["org"]
                .as_str()
                .or_else(|| args["ctx_org"].as_str())
                .unwrap_or("Foundation")
                .to_string(),
            space: ctx["space"]
                .as_str()
                .or_else(|| args["ctx_space"].as_str())
                .unwrap_or("")
                .to_string(),
            role: ctx["role"]
                .as_str()
                .or_else(|| args["ctx_role"].as_str())
                .unwrap_or("read")
                .to_string(),
            clearance: ctx["clearance"]
                .as_str()
                .or_else(|| args["ctx_clearance"].as_str())
                .unwrap_or("internal")
                .to_string(),
        }
    }

    fn as_json(&self) -> Value {
        serde_json::json!({
            "org": self.org,
            "space": self.space,
            "role": self.role,
            "clearance": self.clearance,
        })
    }
}

fn tool_text_json_response(id: &Value, result_json: Value) -> Value {
    tool_text_response(id, &result_json)
}

fn machine_required_fields(hearth_path: &PathBuf, kind: &str) -> Vec<String> {
    let registry = CompositePlaybookRegistry::new(
        HearthPlaybookRegistry::new(hearth_path.clone()),
        SeedPlaybookRegistry,
    );
    registry
        .machine_for(kind)
        .map(|machine| {
            machine
                .required_fields
                .iter()
                .map(|field| field.name.clone())
                .collect()
        })
        .unwrap_or_default()
}

fn orchestrate_parent_id(args: &Value) -> String {
    args["parent_id"]
        .as_str()
        .or_else(|| args["scope"]["parent_id"].as_str())
        .unwrap_or("")
        .to_string()
}

fn orchestrate_track_name(args: &Value) -> String {
    args["track_name"]
        .as_str()
        .or_else(|| args["name"].as_str())
        .unwrap_or("")
        .to_string()
}

fn playbook_name_arg(args: &Value) -> String {
    args["playbook_name"].as_str().unwrap_or("").to_string()
}

/// Builtin required-field names that have dedicated tool arguments (and so are
/// NOT carried in the generic `fields` bag). Mirrors the core's BUILTIN list.
fn is_builtin_required_field(field: &str) -> bool {
    matches!(
        field,
        "name"
            | "track_name"
            | "playbook_name"
            // Read-side recognition for protected hearth machines that still
            // declare `workflow_name`. It resolves the SAME dedicated
            // `playbook_name` argument (see `required_field_value`); the public
            // tool schema stays canonical-only, so there is no second client
            // input. Without this arm the shim pre-keys `fields.workflow_name`,
            // reads the value from somewhere else, and loops forever.
            | "workflow_name"
            | "parent_id"
            | "approver"
            | "target_owner"
    )
}

fn required_field_value(args: &Value, field: &str) -> String {
    match field {
        "name" | "track_name" => orchestrate_track_name(args),
        "parent_id" => orchestrate_parent_id(args),
        "approver" => args["approver"].as_str().unwrap_or("").to_string(),
        // Both spellings resolve the SAME argument, and neither is a client
        // input alias: `field` here is a name a MACHINE.YAML declares, and the
        // live builder machine in anvil-hearth still declares `workflow_name`
        // (a persisted definition C-p.1 owns, not something this repo writes).
        // Dropping the arm makes that machine's required field unsatisfiable,
        // so anvil_orchestrate's begin-mode can never complete for it. The
        // client-facing argument name is `playbook_name` and only that.
        "playbook_name" | "workflow_name" => playbook_name_arg(args),
        "target_owner" => args["target_owner"].as_str().unwrap_or("").to_string(),
        // Any non-builtin required field is read from the generic `fields` bag.
        other => args["fields"][other].as_str().unwrap_or("").to_string(),
    }
}

/// Extract the generic `fields` object from tool arguments into the proto
/// `create_fields` map. Non-string values and a missing/non-object `fields`
/// argument yield an empty map (the engine treats absent generic fields as
/// machine-driven missing).
fn extract_create_fields(args: &Value) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    if let Some(obj) = args["fields"].as_object() {
        for (k, v) in obj {
            if let Some(s) = v.as_str() {
                map.insert(k.clone(), s.to_string());
            }
        }
    }
    map
}

/// The advertised conditional `begin.fields` schema: the `backlog_item` branch
/// requires `fields` to be exactly `{item: <closed object>}`, and the mutually
/// exclusive non-K8 branch retains the string-map. `begin_create_fields`
/// repeats this check by hand so schema bypass cannot broaden either path.
fn begin_fields_one_of() -> Value {
    let evidence_ref = serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "properties": { "kind": { "type": "string" }, "id": { "type": "string" } },
        "required": ["kind", "id"]
    });
    serde_json::json!([
        {
            "properties": {
                "artifact_type": { "const": "backlog_item" },
                "fields": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "item": {
                            "type": "object",
                            "additionalProperties": false,
                            "properties": {
                                "business_node_id": { "type": "string" },
                                "title": { "type": "string" },
                                "description": { "type": ["string", "null"] },
                                "action_class": { "type": "string" },
                                "effort_class": { "type": ["string", "null"] },
                                "intake": {
                                    "type": "object",
                                    "additionalProperties": false,
                                    "properties": {
                                        "edge": { "type": "string" },
                                        "evidence_refs": {
                                            "type": "array",
                                            "minItems": 1,
                                            "items": evidence_ref
                                        }
                                    },
                                    "required": ["edge", "evidence_refs"]
                                },
                                "playbook_binding": {
                                    "type": ["object", "null"],
                                    "additionalProperties": false,
                                    "properties": {
                                        "playbook_definition_id": { "type": ["string", "null"] },
                                        "route_to_intake": { "type": "boolean" }
                                    },
                                    "required": ["route_to_intake"]
                                },
                                "origin_binding": {
                                    "type": "object",
                                    "additionalProperties": false,
                                    "properties": {
                                        "value_gap_served": evidence_ref,
                                        "minting_council_id": { "type": ["string", "null"] },
                                        "experiment_id": { "type": ["string", "null"] },
                                        "predicted_value": { "type": ["number", "null"] }
                                    },
                                    "required": ["value_gap_served"],
                                    "oneOf": [
                                        {
                                            "properties": {
                                                "minting_council_id": { "type": "null" },
                                                "experiment_id": { "type": "null" },
                                                "predicted_value": { "type": "null" }
                                            }
                                        },
                                        {
                                            "properties": {
                                                "minting_council_id": { "type": "string" },
                                                "experiment_id": { "type": "null" },
                                                "predicted_value": { "type": "number" }
                                            },
                                            "required": ["minting_council_id", "predicted_value"]
                                        },
                                        {
                                            "properties": {
                                                "minting_council_id": { "type": "null" },
                                                "experiment_id": { "type": "string" },
                                                "predicted_value": { "type": "number" }
                                            },
                                            "required": ["experiment_id", "predicted_value"]
                                        }
                                    ]
                                }
                            },
                            "required": [
                                "business_node_id",
                                "title",
                                "action_class",
                                "intake",
                                "origin_binding"
                            ]
                        }
                    },
                    "required": ["item"]
                }
            },
            "required": ["artifact_type", "fields"]
        },
        {
            "properties": {
                "artifact_type": { "not": { "const": "backlog_item" } },
                "fields": { "type": "object", "additionalProperties": { "type": "string" } }
            }
        }
    ])
}

/// The genesis keys a K8 `begin` may carry. Everything else — including every
/// engine-owned key — is refused before a byte reaches the engine.
const BACKLOG_ITEM_REQUIRED_KEYS: [&str; 5] = [
    "business_node_id",
    "title",
    "action_class",
    "intake",
    "origin_binding",
];
const BACKLOG_ITEM_OPTIONAL_KEYS: [&str; 3] = ["description", "effort_class", "playbook_binding"];

/// Collect `begin.fields` for the wire.
///
/// `artifact_type: "backlog_item"` takes the K8 branch: `fields` must be
/// exactly `{item: <closed object>}`, `item` is an OBJECT (never a
/// pre-stringified string), its keys are closed, and the origin binding selects
/// exactly one of the three admissible predictor/value shapes. The shim
/// serializes it ONCE into `create_fields["item"]`. Every other artifact_type
/// keeps today's string-map behavior byte-for-byte.
fn begin_create_fields(args: &Value) -> Result<std::collections::HashMap<String, String>, String> {
    if args["artifact_type"].as_str().unwrap_or("") != "backlog_item" {
        return Ok(extract_create_fields(args));
    }
    let fields = args["fields"].as_object().ok_or(
        "begin(artifact_type: \"backlog_item\") requires `fields` to be an object containing \
         exactly one key: `item`",
    )?;
    let stray: Vec<&String> = fields.keys().filter(|k| k.as_str() != "item").collect();
    if !stray.is_empty() {
        return Err(format!(
            "begin(artifact_type: \"backlog_item\") admits exactly `fields.item`; \
             unexpected sibling keys {stray:?}"
        ));
    }
    let item = fields
        .get("item")
        .ok_or("begin(artifact_type: \"backlog_item\") requires `fields.item`")?;
    let object = item.as_object().ok_or(
        "`fields.item` must be a JSON object, never a pre-stringified string — the shim \
         serializes it exactly once",
    )?;
    for key in object.keys() {
        if !BACKLOG_ITEM_REQUIRED_KEYS.contains(&key.as_str())
            && !BACKLOG_ITEM_OPTIONAL_KEYS.contains(&key.as_str())
        {
            return Err(format!(
                "`item.{key}` is not one of the closed K8 genesis keys; the engine owns \
                 backlog_item_id, state, rank, execution_binding, outcome_binding, exit and \
                 history"
            ));
        }
    }
    for key in BACKLOG_ITEM_REQUIRED_KEYS {
        if !object.contains_key(key) {
            return Err(format!("`item.{key}` is required for a K8 genesis"));
        }
    }
    validate_backlog_origin_binding(&object["origin_binding"])?;
    let serialized = serde_json::to_string(item)
        .map_err(|e| format!("`fields.item` could not be serialized: {e}"))?;
    let mut map = std::collections::HashMap::new();
    map.insert("item".to_string(), serialized);
    Ok(map)
}

/// The origin binding admits exactly three shapes: neither predictor with no
/// prediction, one council id with a finite prediction, or one experiment id
/// with a finite prediction.
fn validate_backlog_origin_binding(origin: &Value) -> Result<(), String> {
    let object = origin
        .as_object()
        .ok_or("`item.origin_binding` must be an object")?;
    if !object.contains_key("value_gap_served") {
        return Err("`item.origin_binding.value_gap_served` is required".to_string());
    }
    let present = |key: &str| {
        object
            .get(key)
            .map(|v| !v.is_null())
            .unwrap_or(false)
    };
    let council = present("minting_council_id");
    let experiment = present("experiment_id");
    let value = object.get("predicted_value").filter(|v| !v.is_null());
    if council && experiment {
        return Err(
            "`item.origin_binding` may name a minting_council_id OR an experiment_id, never both"
                .to_string(),
        );
    }
    match (council || experiment, value) {
        (false, None) => Ok(()),
        (false, Some(_)) => Err(
            "`item.origin_binding.predicted_value` requires exactly one predictor id".to_string(),
        ),
        (true, None) => Err(
            "`item.origin_binding` names a predictor id, so predicted_value is required"
                .to_string(),
        ),
        (true, Some(v)) => match v.as_f64() {
            Some(f) if f.is_finite() => Ok(()),
            _ => Err("`item.origin_binding.predicted_value` must be a finite number".to_string()),
        },
    }
}

fn copy_hearth_routing_args(args: &Value, next_obj: &mut serde_json::Map<String, Value>) {
    for key in ["hearth", "project"] {
        if let Some(value) = args[key].as_str().filter(|s| !s.trim().is_empty()) {
            next_obj.insert(key.to_string(), serde_json::json!(value));
        }
    }
}

fn missing_required_fields(args: &Value, required_fields: &[String]) -> Vec<String> {
    required_fields
        .iter()
        .filter(|field| required_field_value(args, field).is_empty())
        .cloned()
        .collect()
}

/// Pre-key a `next_call` arguments object with placeholders for every declared
/// required field (builtin: track_name/parent_id/approver; generic: a `fields`
/// object) so the begin call the model issues is actionable. Supplied values are
/// echoed; missing ones get a `<placeholder>` (builtins) or empty string
/// (generics). Shared by the advisory single route (H1) and the
/// missing_required_fields response so the two never drift.
fn inject_required_field_placeholders(
    next_obj: &mut serde_json::Map<String, Value>,
    args: &Value,
    required_fields: &[String],
    parent_id: &str,
) {
    if required_fields
        .iter()
        .any(|field| field == "name" || field == "track_name")
    {
        let track_name = orchestrate_track_name(args);
        next_obj.insert(
            "track_name".to_string(),
            serde_json::json!(if track_name.is_empty() {
                "<track_name>"
            } else {
                track_name.as_str()
            }),
        );
    }
    // A machine may declare the playbook-name field under either the canonical
    // or the pre-migration spelling; both resolve the ONE canonical argument,
    // so the placeholder names that argument and only that. Without this the
    // caller is told a field is missing and given no slot to put it in.
    if required_fields
        .iter()
        .any(|field| field == "playbook_name" || field == "workflow_name")
    {
        let playbook_name = playbook_name_arg(args);
        next_obj.insert(
            "playbook_name".to_string(),
            serde_json::json!(if playbook_name.is_empty() {
                "<playbook_name>"
            } else {
                playbook_name.as_str()
            }),
        );
    }
    if required_fields.iter().any(|field| field == "parent_id") {
        next_obj.insert(
            "parent_id".to_string(),
            serde_json::json!(if parent_id.is_empty() {
                "<parent_id>"
            } else {
                parent_id
            }),
        );
    }
    if required_fields.iter().any(|field| field == "approver") {
        let approver = args["approver"].as_str().unwrap_or("");
        next_obj.insert(
            "approver".to_string(),
            serde_json::json!(if approver.is_empty() {
                "<approver>"
            } else {
                approver
            }),
        );
    }

    // Generic (non-builtin) required fields ride in a `fields` object. Pre-key
    // it with EVERY declared generic field so the calling agent sees exactly
    // what to fill: supplied values are echoed, missing ones get empty strings.
    let generic_fields: Vec<&String> = required_fields
        .iter()
        .filter(|f| !is_builtin_required_field(f))
        .collect();
    if !generic_fields.is_empty() {
        let mut fields_obj = serde_json::Map::new();
        for field in generic_fields {
            let supplied = args["fields"][field].as_str().unwrap_or("");
            fields_obj.insert(field.clone(), serde_json::json!(supplied));
        }
        next_obj.insert("fields".to_string(), serde_json::Value::Object(fields_obj));
    }
}

fn missing_required_fields_for_selection(
    args: &Value,
    selection: &str,
    required_fields: &[String],
) -> Vec<String> {
    if selection != "track" {
        return missing_required_fields(args, required_fields);
    }

    let derived_track_name = derive_track_name(args["message"].as_str().unwrap_or(""));
    required_fields
        .iter()
        .filter(|field| {
            let value = required_field_value(args, field);
            if !value.is_empty() {
                return false;
            }
            if (*field == "name" || *field == "track_name") && !derived_track_name.is_empty() {
                return false;
            }
            true
        })
        .cloned()
        .collect()
}

fn missing_required_fields_response(
    args: &Value,
    selection: &str,
    required_fields: &[String],
    missing_fields: Vec<String>,
    parent_id: &str,
    candidate_set: &str,
    conversation_id: &str,
    confidence: &str,
    request_ctx: &OrchestrateRequestContext,
) -> Value {
    let message = args["message"].as_str().unwrap_or("");
    let signal = args["routing_hint"].as_str().unwrap_or("");
    let mut next_args = serde_json::json!({
        "message": message,
        "selection": selection,
        "routing_hint": signal,
        "candidate_set": candidate_set,
        "conversation_id": conversation_id,
        "confidence": confidence,
        "surface": args["surface"].as_str().unwrap_or(""),
        "ctx": request_ctx.as_json(),
        "actor_name": args["actor_name"].as_str().unwrap_or(""),
        "actor_type": args["actor_type"].as_str().unwrap_or(""),
        "actor_model": args["actor_model"].as_str().unwrap_or(""),
        "actor_provider": args["actor_provider"].as_str().unwrap_or(""),
    });
    // `next_args` is always a json object literal, so this is infallible; the
    // fallback keeps a single bad call from ever panicking the shim (Fix #3).
    let mut empty = serde_json::Map::new();
    let next_obj = next_args.as_object_mut().unwrap_or(&mut empty);
    copy_hearth_routing_args(args, next_obj);
    inject_required_field_placeholders(next_obj, args, required_fields, parent_id);

    serde_json::json!({
        "outcome": "missing_required_fields",
        "selected_kind": selection,
        "required_fields": required_fields,
        "missing_fields": missing_fields,
        "guidance": "Supply the missing required fields and re-call anvil_orchestrate in begin-mode.",
        "next_call": { "tool": "anvil_orchestrate", "arguments": next_args },
    })
}

fn begin_selected(
    id: &Value,
    args: &Value,
    hearth_path: &PathBuf,
    engine_port: &mut u16,
    rt: &tokio::runtime::Runtime,
    foundry_mode: &FoundryMode,
    selection: &str,
    parent_id: &str,
    candidate_set: &str,
    conversation_id: &str,
    confidence: &str,
    rd_input: &str,
    request_ctx: &OrchestrateRequestContext,
) -> Result<Value, Value> {
    // The selection IS the kind for domain machines: pass it straight through as
    // the artifact_type. BeginResponse supplies playbook_id/measurement from
    // the engine-selected playbook source.
    let artifact_type = selection.to_string();
    let supplied_track_name = orchestrate_track_name(args);
    let track_name = if supplied_track_name.is_empty() {
        derive_track_name(args["message"].as_str().unwrap_or(""))
    } else {
        supplied_track_name
    };

    // The surface ships identity directly (no prior checkin); session_role is
    // "creator" because the handoff opens/instantiates the playbook.
    let begin_request = BeginRequest {
        hearth_path: hearth_path.display().to_string(),
        artifact_type,
        parent_id: parent_id.to_string(),
        track_name,
        // Thread the builtin playbook_name like the plain `begin` tool + the
        // neighboring target_owner do; hardcoding it empty made
        // anvil_orchestrate(selection:"playbook_generation") unbeginnable (the
        // engine rejected it with missing_required_field: playbook_name).
        playbook_name: playbook_name_arg(args),
        target_owner: args["target_owner"].as_str().unwrap_or("").to_string(),
        create_fields: extract_create_fields(args),
        approver: args["approver"].as_str().unwrap_or("").to_string(),
        actor_name: args["actor_name"].as_str().unwrap_or("").to_string(),
        actor_type: args["actor_type"].as_str().unwrap_or("").to_string(),
        actor_model: args["actor_model"].as_str().unwrap_or("").to_string(),
        actor_provider: args["actor_provider"].as_str().unwrap_or("").to_string(),
        actor_context_window: args["actor_context_window"].as_i64().unwrap_or(0),
        actor_sdk_version: args["actor_sdk_version"].as_str().unwrap_or("").to_string(),
        actor_entrypoint: args["actor_entrypoint"].as_str().unwrap_or("").to_string(),
        identifier: String::new(),
        session_role: "creator".to_string(),
        // resume-aware routing: record the originating conversation id on the
        // durable open-begin marker. DISTINCT from the rd_turn_id mapping below
        // (the routing-decision turn id); both are populated from the same
        // conversation_id here, but they are separate fields with separate
        // consumers (rd_turn_id → routing-decision telemetry; conversation_id →
        // the resume open-playbook lookup).
        conversation_id: conversation_id.to_string(),
        ctx_org: request_ctx.org.clone(),
        ctx_space: request_ctx.space.clone(),
        ctx_role: request_ctx.role.clone(),
        ctx_clearance: request_ctx.clearance.clone(),
        rd_turn_id: conversation_id.to_string(),
        rd_input: rd_input.to_string(),
        rd_candidate_set: candidate_set.to_string(),
        rd_selected: selection.to_string(),
        rd_confidence: confidence.to_string(),
        project_root: project_root_for_request(args),
        // Routed create-begin: adoption is an identifier-mode concern only.
        adopt: false,
        claimed_evidence: Vec::new(),
    };

    let begin = match call_engine(
        id,
        engine_port,
        rt,
        foundry_mode,
        begin_request,
        |client, request| Box::pin(client.begin(request)),
        |s| format!("anvil_orchestrate begin failed: {}", s.message()),
    ) {
        Ok(begin) => begin,
        Err(error_response) => return Err(error_response),
    };

    let role = "doer";
    let instance_id = begin
        .track_path
        .rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or(&begin.track_path)
        .to_string();
    let mut complete_args = serde_json::json!({
        "artifact_path": begin.track_path,
        "actor_name": args["actor_name"].as_str().unwrap_or(""),
        "actor_type": args["actor_type"].as_str().unwrap_or(""),
        "actor_model": args["actor_model"].as_str().unwrap_or(""),
        "actor_provider": args["actor_provider"].as_str().unwrap_or("")
    });
    if let Some(obj) = complete_args.as_object_mut() {
        copy_hearth_routing_args(args, obj);
    }
    Ok(serde_json::json!({
        "instance_id": instance_id,
        "playbook_id": begin.playbook_id,
        "state": begin.state,
        "role": role,
        "context_text": begin.context_text,
        "intent": begin.intent,
        "expected_output": begin.expected_output,
        "guidance": format!(
            "You are the doer for state '{}'. Read context_text, perform the step, then issue next_call to advance.",
            begin.state
        ),
        "surface": args["surface"].as_str().unwrap_or(""),
        "next_call": {
            "tool": "complete",
            "arguments": complete_args
        }
    }))
}

/// Handle a tools/call for "anvil_orchestrate" — the universal surface→Anvil
/// handoff. Routes the surface's message (v0: by routing_hint) to a playbook,
/// creates the instance via the engine Begin RPC, reads the entry step's
/// intent/expected_output from the routed machine.yaml, and returns the guided
/// first step. No prior checkin required: the surface ships identity directly.
fn handle_anvil_orchestrate_call(
    id: &Value,
    request: &Value,
    hearth_path: &PathBuf,
    engine_port: &mut u16,
    rt: &tokio::runtime::Runtime,
    foundry_mode: &FoundryMode,
) -> Value {
    let args = &request["params"]["arguments"];
    let message = args["message"].as_str().unwrap_or("");

    // Two-phase handoff (playbook_routing_layer R-2). When `selection` is blank,
    // we are in ROUTE-MODE: call engine route(...) to expose the candidate set
    // to the surface LLM (the LLM selects). When `selection` is set, we are in
    // BEGIN-MODE: call begin(...) only, carrying the routing decision fields in
    // the request body. `routing_hint`, if still supplied, is carried as the
    // coarse `signal` (a ranking input, never the decider).
    let selection = args["selection"].as_str().unwrap_or("").trim().to_string();
    let signal = args["routing_hint"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    let conversation_id = effective_conversation_id(args);
    let confidence = args["confidence"].as_str().unwrap_or("").to_string();
    let rd_input = routing_input(message, &signal);
    let parent_id = orchestrate_parent_id(args);
    let request_ctx = OrchestrateRequestContext::from_args(args);

    if selection.is_empty() {
        let route_request = RouteRequest {
            recent_context: String::new(),
            prior_proposal: None,
            hearth_path: hearth_path.display().to_string(),
            message: message.to_string(),
            signal: signal.clone(),
            surface: args["surface"].as_str().unwrap_or("").to_string(),
            parent_id: parent_id.clone(),
            conversation_id: conversation_id.clone(),
            actor_name: args["actor_name"].as_str().unwrap_or("").to_string(),
            actor_type: args["actor_type"].as_str().unwrap_or("").to_string(),
            actor_model: args["actor_model"].as_str().unwrap_or("").to_string(),
            actor_provider: args["actor_provider"].as_str().unwrap_or("").to_string(),
            ctx_org: request_ctx.org.clone(),
            ctx_space: request_ctx.space.clone(),
            ctx_role: request_ctx.role.clone(),
            ctx_clearance: request_ctx.clearance.clone(),
            source: args["surface"].as_str().unwrap_or("").to_string(),
            project_root: project_root_for_request(args),
        };
        let route = match call_engine(
            id,
            engine_port,
            rt,
            foundry_mode,
            route_request,
            |client, request| Box::pin(client.route(request)),
            |s| format!("anvil_orchestrate route failed: {}", s.message()),
        ) {
            Ok(route) => route,
            Err(error_response) => return error_response,
        };
        // ROUTE-MODE. Return either the candidate set (LLM selects) or the typed
        // no_match → candidate_playbook_intake outcome (NOT an error — this
        // replaces both former routing_unavailable error sites).
        if route.outcome == "no_match" {
            let result_json = serde_json::json!({
                "outcome": "no_match",
                "handoff": route.handoff,
                "intent": route.intent,
                "guidance": "No active driven playbook matched. Hand off to candidate_playbook_intake (the generation half) with this intent.",
                "surface": args["surface"].as_str().unwrap_or(""),
            });
            return tool_text_json_response(id, result_json);
        }
        // resume_aware_routing C1 — RESUME outcome. The engine's resume pre-check
        // fired (continuation token + an open, non-terminal playbook for this
        // conversation), so route resolved to `resume` rather than a fresh
        // single/candidates set. Surface the open playbook back to the caller —
        // its id/kind/state, the current-step guidance, and the supported advance
        // action — with a clear "continue via <advance action>" instruction, so
        // Claude continues the in-progress work instead of seeing empty candidates.
        if route.resolution_outcome == "resume" {
            let advance = route.resume_advance_action.trim();
            let continue_instruction = if advance.is_empty() {
                format!(
                    "Playbook run {} ({}) is already in progress for this conversation. Continue it rather than starting a new run.",
                    route.resume_artifact_id, route.resume_kind
                )
            } else {
                format!(
                    "Playbook run {} ({}) is already in progress for this conversation. Continue via {} — do NOT start a new run.",
                    route.resume_artifact_id, route.resume_kind, advance
                )
            };
            let result_json = serde_json::json!({
                "outcome": "resume",
                "resolution_outcome": "resume",
                "resume_artifact_id": route.resume_artifact_id,
                "resume_kind": route.resume_kind,
                "resume_state": route.resume_state,
                "resume_guidance": route.resume_guidance,
                "resume_advance_action": route.resume_advance_action,
                "guidance": continue_instruction,
                "surface": args["surface"].as_str().unwrap_or(""),
            });
            return tool_text_json_response(id, result_json);
        }
        let candidates: Vec<Value> = route
            .candidates
            .iter()
            .map(|c| {
                serde_json::json!({
                    "kind": c.kind,
                    "description": c.description,
                    "required_fields": c.required_fields,
                    // route_response_mirrors_begin Phase 2/3 — per-candidate
                    // annotations so the model chooses deliberately.
                    "intent": c.intent,
                    "step_outline": c.step_outline,
                    "why_fits": c.why_fits,
                })
            })
            .collect();
        let candidate_set = route
            .candidates
            .iter()
            .map(|c| c.kind.as_str())
            .collect::<Vec<_>>()
            .join(",");
        if route.resolution_outcome == "single" && !route.selected_kind.is_empty() {
            let required_fields = route
                .candidates
                .iter()
                .find(|candidate| candidate.kind.as_str() == route.selected_kind.as_str())
                .map(|candidate| candidate.required_fields.clone())
                .unwrap_or_else(|| machine_required_fields(hearth_path, &route.selected_kind));
            // route_response_mirrors_begin H1 (advisory contract): a single
            // resolution ALWAYS returns the advisory shape — the begin-equivalent
            // `guidance`, the begin call (next_call), and `required_fields` —
            // WHETHER OR NOT required fields are present. Missing fields no longer
            // short-circuit to a guidance-less missing_required_fields response;
            // they ride along as `missing_fields` so the model sees the same
            // advisory plus exactly what it must supply at begin. NO begin / NO
            // state transition occurs at route ("LLM selects, engine executes").
            let missing_fields = missing_required_fields(args, &required_fields);
            let mut begin_args = serde_json::json!({
                "message": message,
                "selection": route.selected_kind,
                "routing_hint": signal,
                "candidate_set": candidate_set,
                "conversation_id": conversation_id,
                "surface": args["surface"].as_str().unwrap_or(""),
                "scope": { "parent_id": parent_id },
                "ctx": request_ctx.as_json(),
                "actor_name": args["actor_name"].as_str().unwrap_or(""),
                "actor_type": args["actor_type"].as_str().unwrap_or(""),
                "actor_model": args["actor_model"].as_str().unwrap_or(""),
                "actor_provider": args["actor_provider"].as_str().unwrap_or(""),
            });
            if let Some(begin_obj) = begin_args.as_object_mut() {
                copy_hearth_routing_args(args, begin_obj);
                // Pre-key the begin call with placeholders for every required
                // field (builtin + generic) so the begin call the model issues is
                // actionable — it sees exactly what to supply. Supplied values are
                // echoed; missing ones get `<placeholder>` (H1).
                inject_required_field_placeholders(begin_obj, args, &required_fields, &parent_id);
            }
            let result_json = serde_json::json!({
                "outcome": "single",
                "mode": "route_advisory",
                "resolution_outcome": route.resolution_outcome,
                "selected_kind": route.selected_kind,
                "matching_candidates": route.matching_candidates,
                "required_fields": required_fields,
                // The required fields the model has NOT yet supplied. Advisory only
                // — a single route never gates on these; the model supplies them at
                // begin (H1). Empty when all required fields are already present.
                "missing_fields": missing_fields,
                // The begin-equivalent first-step guidance the engine served for
                // the selected kind's initial (state, doer). Empty when the engine
                // degraded to a thin route (fail-open).
                "guidance": route.guidance,
                "surface": args["surface"].as_str().unwrap_or(""),
                "next_call": { "tool": "anvil_orchestrate", "arguments": begin_args },
            });
            return tool_text_json_response(id, result_json);
        }
        let mut next_args = serde_json::json!({
            "message": message,
            "selection": "<chosen kind from candidates>",
            "routing_hint": signal,
            "candidate_set": candidate_set,
            "conversation_id": conversation_id,
            "surface": args["surface"].as_str().unwrap_or(""),
            "scope": { "parent_id": parent_id },
            "ctx": request_ctx.as_json(),
            "actor_name": args["actor_name"].as_str().unwrap_or(""),
            "actor_type": args["actor_type"].as_str().unwrap_or(""),
            "actor_model": args["actor_model"].as_str().unwrap_or(""),
            "actor_provider": args["actor_provider"].as_str().unwrap_or(""),
        });
        if let Some(next_obj) = next_args.as_object_mut() {
            copy_hearth_routing_args(args, next_obj);
        }
        let result_json = serde_json::json!({
            "outcome": "candidates",
            "candidates": candidates,
            "guidance": "Select one candidate kind, then re-call anvil_orchestrate with `selection` set to that kind to begin it.",
            "surface": args["surface"].as_str().unwrap_or(""),
            "next_call": { "tool": "anvil_orchestrate", "arguments": next_args },
        });
        return tool_text_json_response(id, result_json);
    }

    let candidate_set = args["candidate_set"].as_str().unwrap_or("").to_string();
    let required_fields = machine_required_fields(hearth_path, &selection);
    let missing_fields = missing_required_fields_for_selection(args, &selection, &required_fields);
    if !missing_fields.is_empty() {
        let result_json = missing_required_fields_response(
            args,
            &selection,
            &required_fields,
            missing_fields,
            &parent_id,
            &candidate_set,
            &conversation_id,
            &confidence,
            &request_ctx,
        );
        return tool_text_json_response(id, result_json);
    }

    match begin_selected(
        id,
        args,
        hearth_path,
        engine_port,
        rt,
        foundry_mode,
        &selection,
        &parent_id,
        &candidate_set,
        &conversation_id,
        &confidence,
        &rd_input,
        &request_ctx,
    ) {
        Ok(result_json) => tool_text_json_response(id, result_json),
        Err(error_response) => error_response,
    }
}

/// Handle a tools/call for "describe" — ensure engine is running, call gRPC, return MCP response.
fn handle_describe_call(
    id: &Value,
    request: &Value,
    hearth_path: &PathBuf,
    engine_port: &mut u16,
    rt: &tokio::runtime::Runtime,
    foundry_mode: &FoundryMode,
) -> Value {
    let args = &request["params"]["arguments"];
    let identifier = args["identifier"].as_str().unwrap_or("").to_string();

    let describe_request = DescribeRequest {
        hearth_path: hearth_path.display().to_string(),
        identifier,
    };

    let describe = match call_engine(
        id,
        engine_port,
        rt,
        foundry_mode,
        describe_request,
        |client, request| Box::pin(client.describe(request)),
        |s| format!("Describe RPC failed: {}", s.message()),
    ) {
        Ok(describe) => describe,
        Err(error_response) => return error_response,
    };

    let next_step = describe.next_step.clone();
    let result_json = match describe.info {
        Some(anvil_engine::proto::describe_response::Info::TypeInfo(t)) => {
            serde_json::json!({
                "kind": "type_info",
                "name": t.name,
                "description": t.description,
                "required_fields": t.required_fields,
                "parent_type": t.parent_type,
                "next_step": next_step,
            })
        }
        Some(anvil_engine::proto::describe_response::Info::InstanceInfo(i)) => {
            let mut json = serde_json::json!({
                "kind": "instance_info",
                "id": i.id,
                "artifact_type": i.artifact_type,
                "state": i.state,
                "transition_count": i.transition_count,
                "available_actions": i.available_actions.iter().map(|a| {
                    serde_json::json!({
                        "action": a.action,
                        "required_role": a.required_role,
                        "execution_route": a.execution_route
                    })
                }).collect::<Vec<_>>(),
                "next_step": next_step,
            });
            if let Some(t) = i.last_transition {
                json["last_transition"] = serde_json::json!({
                    "to": t.to, "at": t.at, "actor": t.actor, "role": t.role
                });
            }
            json
        }
        None => {
            return return_tool_error(id, "Empty describe response from engine");
        }
    };

    tool_text_response(id, &result_json)
}

/// Create an MCP tool error response.
fn return_tool_error(id: &Value, message: &str) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "isError": true,
            "content": [{
                "type": "text",
                "text": message
            }]
        }
    })
}

fn grpc_code_label(code: tonic::Code) -> &'static str {
    match code {
        tonic::Code::Ok => "OK",
        tonic::Code::Cancelled => "CANCELLED",
        tonic::Code::Unknown => "UNKNOWN",
        tonic::Code::InvalidArgument => "INVALID_ARGUMENT",
        tonic::Code::DeadlineExceeded => "DEADLINE_EXCEEDED",
        tonic::Code::NotFound => "NOT_FOUND",
        tonic::Code::AlreadyExists => "ALREADY_EXISTS",
        tonic::Code::PermissionDenied => "PERMISSION_DENIED",
        tonic::Code::ResourceExhausted => "RESOURCE_EXHAUSTED",
        tonic::Code::FailedPrecondition => "FAILED_PRECONDITION",
        tonic::Code::Aborted => "ABORTED",
        tonic::Code::OutOfRange => "OUT_OF_RANGE",
        tonic::Code::Unimplemented => "UNIMPLEMENTED",
        tonic::Code::Internal => "INTERNAL",
        tonic::Code::Unavailable => "UNAVAILABLE",
        tonic::Code::DataLoss => "DATA_LOSS",
        tonic::Code::Unauthenticated => "UNAUTHENTICATED",
    }
}

/// Handle a tools/call for "checkin" — ensure engine is reachable, call gRPC,
/// store session state, return filtered result.
fn handle_checkin_call(
    id: &Value,
    request: &Value,
    hearth_path: &PathBuf,
    engine_port: &mut u16,
    rt: &tokio::runtime::Runtime,
    session: &mut Option<SessionState>,
    foundry_mode: &FoundryMode,
) -> Value {
    let args = &request["params"]["arguments"];
    let role = args["role"].as_str().unwrap_or("").to_string();
    let actor_type = args["actor_type"].as_str().unwrap_or("").to_string();
    let actor_model = args["actor_model"].as_str().unwrap_or("").to_string();
    let actor_provider = args["actor_provider"].as_str().unwrap_or("").to_string();
    // Caller may supply actor_name to carry identity across conversation
    // boundaries (spec R1). The shim forwards verbatim; the engine echoes
    // the supplied name if non-empty, otherwise generates one.
    let tool_actor_name = args["actor_name"].as_str().unwrap_or("").to_string();

    let checkin_request = CheckinRequest {
        hearth_path: hearth_path.display().to_string(),
        role: role.clone(),
        actor_type: actor_type.clone(),
        actor_model: actor_model.clone(),
        actor_provider: actor_provider.clone(),
        actor_name: tool_actor_name,
    };

    let checkin = match call_engine(
        id,
        engine_port,
        rt,
        foundry_mode,
        checkin_request,
        |client, request| Box::pin(client.checkin(request)),
        |s| format!("Checkin RPC failed: {}", s.message()),
    ) {
        Ok(checkin) => checkin,
        Err(error_response) => return error_response,
    };

    // Store session state
    *session = Some(SessionState { role });

    let mut result_json = serde_json::json!({
        "actor_name": checkin.actor_name,
        "filtered_artifacts": checkin.filtered_artifacts.iter().map(|a| {
            serde_json::json!({
                "id": a.id,
                "type": a.artifact_type,
                "state": a.state,
                "summary": a.summary,
                "execution_route": a.execution_route
            })
        }).collect::<Vec<_>>(),
        "available_types": checkin.available_types.iter().map(|t| {
            serde_json::json!({
                "name": t.name,
                "description": t.description,
                "requires_parent": t.requires_parent,
                "execution_route": t.execution_route
            })
        }).collect::<Vec<_>>(),
        "next_step": checkin.next_step
    });

    // T4: surface re-served hook content only when present, so a checkin with no
    // open begin to re-warm keeps its lean discovery-only shape.
    if !checkin.context.is_empty() {
        result_json["context"] = serde_json::Value::String(checkin.context.clone());
    }

    tool_text_response(id, &result_json)
}

/// Handle a tools/call for `snapshot`. Caller-supplied identity is forwarded
/// verbatim; session state is retained for begin role routing only.
fn handle_snapshot_call(
    id: &Value,
    request: &Value,
    hearth_path: &PathBuf,
    engine_port: &mut u16,
    rt: &tokio::runtime::Runtime,
    session: &mut Option<SessionState>,
    foundry_mode: &FoundryMode,
) -> Value {
    let args = &request["params"]["arguments"];

    // Per spec R5 of the checkin_backfill_spec_context track: the shim
    // forwards caller-supplied actor_* fields verbatim. Session state
    // does not inject identity into outbound requests. The session
    // cache is kept for potential non-identity uses (see spec Out of
    // Scope) but never mutates the outbound SnapshotRequest.
    let snapshot_request = SnapshotRequest {
        hearth_path: hearth_path.display().to_string(),
        artifact_path: args["artifact_path"].as_str().unwrap_or("").to_string(),
        to_state: args["to_state"].as_str().unwrap_or("").to_string(),
        actor_name: args["actor_name"].as_str().unwrap_or("").to_string(),
        actor_role: args["actor_role"].as_str().unwrap_or("").to_string(),
        approver: args["approver"].as_str().unwrap_or("").to_string(),
        note: args["note"].as_str().unwrap_or("").to_string(),
        actor_type: args["actor_type"].as_str().unwrap_or("").to_string(),
        actor_model: args["actor_model"].as_str().unwrap_or("").to_string(),
        actor_provider: args["actor_provider"].as_str().unwrap_or("").to_string(),
        actor_context_window: args["actor_context_window"].as_i64().unwrap_or(0),
        actor_sdk_version: args["actor_sdk_version"].as_str().unwrap_or("").to_string(),
        actor_entrypoint: args["actor_entrypoint"].as_str().unwrap_or("").to_string(),
        projection_only: args["projection_only"].as_bool().unwrap_or(false),
        event_type: args["event_type"].as_str().unwrap_or("").to_string(),
        conversation_id: String::new(),
        project_root: project_root_for_request(args),
        claimed_evidence: Vec::new(),
    };

    let _ = session; // session cache no longer mutates outbound identity
    let resp = match call_engine(
        id,
        engine_port,
        rt,
        foundry_mode,
        snapshot_request,
        |client, request| Box::pin(client.snapshot(request)),
        |s| format!("Snapshot RPC failed: {}", s.message()),
    ) {
        Ok(resp) => resp,
        Err(error_response) => return error_response,
    };

    let result_json = serde_json::json!({
        "success": resp.success,
        "timestamp": resp.timestamp,
        "actor_name": resp.actor_name,
        "status_updated": resp.status_updated,
        "registry_updated": resp.registry_updated,
        "projections_updated": resp.projections_updated,
        "warnings": resp.warnings,
    });

    tool_text_response(id, &result_json)
}

/// Handle a tools/call for "complete" — forwards caller-supplied identity
/// verbatim per spec R1.5 (no session injection). No prior checkin required.
/// The effective conversation/session id for an RPC: the caller-supplied
/// `conversation_id` when non-empty, else the Claude Code session id from the
/// `CLAUDE_CODE_SESSION_ID` env — the SAME id the route hook hashes for the route
/// leg. Defaulting here is what lets begin/complete activity-log records carry a
/// `conversation_hash` that JOINS the route leg (without it the adoption metric's
/// takes∩routed∩begun join collapses). Empty only when neither source is present.
fn effective_conversation_id(args: &Value) -> String {
    let from_args = args["conversation_id"].as_str().unwrap_or("").trim();
    if !from_args.is_empty() {
        return from_args.to_string();
    }
    std::env::var("CLAUDE_CODE_SESSION_ID")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_default()
}

fn handle_complete_call(
    id: &Value,
    request: &Value,
    hearth_path: &PathBuf,
    engine_port: &mut u16,
    rt: &tokio::runtime::Runtime,
    foundry_mode: &FoundryMode,
) -> Value {
    let args = &request["params"]["arguments"];

    // Per spec R1.5: the shim forwards caller-supplied actor_* fields
    // verbatim. No session state is injected into complete calls.
    let complete_request = CompleteRequest {
        artifact_path: args["artifact_path"].as_str().unwrap_or("").to_string(),
        actor_name: args["actor_name"].as_str().unwrap_or("").to_string(),
        actor_type: args["actor_type"].as_str().unwrap_or("").to_string(),
        actor_model: args["actor_model"].as_str().unwrap_or("").to_string(),
        actor_provider: args["actor_provider"].as_str().unwrap_or("").to_string(),
        actor_context_window: args["actor_context_window"].as_i64().unwrap_or(0),
        actor_sdk_version: args["actor_sdk_version"].as_str().unwrap_or("").to_string(),
        actor_entrypoint: args["actor_entrypoint"].as_str().unwrap_or("").to_string(),
        satisfaction: args["satisfaction"].as_str().unwrap_or("").to_string(),
        approver: args["approver"].as_str().unwrap_or("").to_string(),
        note: args["note"].as_str().unwrap_or("").to_string(),
        reflection_notes: args["reflection_notes"].as_str().unwrap_or("").to_string(),
        // Slice C: forward findings verbatim (required by the engine when
        // satisfaction == "address_in_next_step"; ignored otherwise).
        findings: args["findings"].as_str().unwrap_or("").to_string(),
        // Carry the resolved hearth on the wire so the engine resolves
        // per-request, matching the other RPCs that already send hearth_path.
        hearth_path: hearth_path.display().to_string(),
        // Default to the Claude Code session id when the caller omits it, so the
        // complete's activity-log record hashes + joins the route/begin legs.
        conversation_id: effective_conversation_id(args),
        project_root: project_root_for_request(args),
        claimed_evidence: claimed_evidence_arg(&args["claimed_evidence"]),
    };

    let resp = match call_engine(
        id,
        engine_port,
        rt,
        foundry_mode,
        complete_request,
        |client, request| Box::pin(client.complete(request)),
        |s| format!("Complete RPC failed: {}", s.message()),
    ) {
        Ok(resp) => resp,
        Err(error_response) => return error_response,
    };

    // Build response JSON. Per spec R3.2: omit reflection_path key entirely
    // when the value is empty (the field is absent, not present with empty value).
    // This matches Slice C's carry_forward_path convention.
    // BP4: surface CompleteResponse.warnings (field 7) so the MCP caller
    // receives begin-adoption warnings (AC-8, standalone + Foundry modes).
    let mut result_map = serde_json::Map::new();
    result_map.insert(
        "new_state".to_string(),
        serde_json::Value::String(resp.new_state),
    );
    result_map.insert(
        "transition_at".to_string(),
        serde_json::Value::String(resp.transition_at),
    );
    result_map.insert(
        "artifact_path".to_string(),
        serde_json::Value::String(resp.artifact_path),
    );
    if !resp.reflection_path.is_empty() {
        result_map.insert(
            "reflection_path".to_string(),
            serde_json::Value::String(resp.reflection_path),
        );
    }
    // Slice C: surface carry_forward_path on the carry-forward success path;
    // drop the key when empty (mirrors reflection_path).
    if !resp.carry_forward_path.is_empty() {
        result_map.insert(
            "carry_forward_path".to_string(),
            serde_json::Value::String(resp.carry_forward_path),
        );
    }
    result_map.insert(
        "warnings".to_string(),
        serde_json::Value::Array(
            resp.warnings
                .into_iter()
                .map(serde_json::Value::String)
                .collect(),
        ),
    );
    let result_json = serde_json::Value::Object(result_map);
    tool_text_response(id, &result_json)
}

/// Handle a tools/call for "amend" — forwards caller-supplied identity
/// verbatim per spec R1.5 (no session injection). No prior checkin required.
/// Mirrors handle_complete_call exactly.
fn handle_amend_call(
    id: &Value,
    request: &Value,
    hearth_path: &PathBuf,
    engine_port: &mut u16,
    rt: &tokio::runtime::Runtime,
    foundry_mode: &FoundryMode,
) -> Value {
    let args = &request["params"]["arguments"];

    // Per spec R1.5: the shim forwards caller-supplied actor_* fields
    // verbatim. No session state is injected into amend calls.
    let amend_request = AmendRequest {
        hearth_path: hearth_path.display().to_string(),
        artifact_path: args["artifact_path"].as_str().unwrap_or("").to_string(),
        kind: args["kind"].as_str().unwrap_or("").to_string(),
        target_document: args["target_document"].as_str().unwrap_or("").to_string(),
        target_id: args["target_id"].as_str().unwrap_or("").to_string(),
        op_kind: args["op_kind"].as_str().unwrap_or("").to_string(),
        body: args["body"].as_str().unwrap_or("").to_string(),
        new_kind: args["new_kind"].as_str().unwrap_or("").to_string(),
        anchor: args["anchor"].as_str().unwrap_or("").to_string(),
        actor_name: args["actor_name"].as_str().unwrap_or("").to_string(),
        actor_type: args["actor_type"].as_str().unwrap_or("").to_string(),
        actor_model: args["actor_model"].as_str().unwrap_or("").to_string(),
        actor_provider: args["actor_provider"].as_str().unwrap_or("").to_string(),
        actor_context_window: args["actor_context_window"].as_i64().unwrap_or(0),
        actor_sdk_version: args["actor_sdk_version"].as_str().unwrap_or("").to_string(),
        actor_entrypoint: args["actor_entrypoint"].as_str().unwrap_or("").to_string(),
    };

    let resp = match call_engine(
        id,
        engine_port,
        rt,
        foundry_mode,
        amend_request,
        |client, request| Box::pin(client.amend(request)),
        |s| format!("Amend RPC failed: {}", s.message()),
    ) {
        Ok(resp) => resp,
        Err(error_response) => return error_response,
    };

    // Build response JSON. Per spec: omit new_state key entirely when
    // the value is empty (no transition was driven — record-only path).
    // Mirrors how complete drops reflection_path when empty.
    let mut result_map = serde_json::Map::new();
    result_map.insert("op_id".to_string(), serde_json::Value::String(resp.op_id));
    result_map.insert(
        "resolved_hearth".to_string(),
        serde_json::Value::String(resp.resolved_hearth),
    );
    if !resp.new_state.is_empty() {
        result_map.insert(
            "new_state".to_string(),
            serde_json::Value::String(resp.new_state),
        );
    }
    let result_json = serde_json::Value::Object(result_map);
    tool_text_response(id, &result_json)
}

/// Handle a tools/call for "persist_playbook" — forwards the generated
/// machine.yaml to the engine's existing PersistPlaybook RPC. The engine hearth
/// path is used only for lock/authorization scope; owner_home is the write
/// target.
fn handle_persist_playbook_call(
    id: &Value,
    request: &Value,
    hearth_path: &PathBuf,
    engine_port: &mut u16,
    rt: &tokio::runtime::Runtime,
    foundry_mode: &FoundryMode,
) -> Value {
    let args = &request["params"]["arguments"];

    let persist_request = PersistPlaybookRequest {
        hearth_path: hearth_path.display().to_string(),
        owner_home: args["owner_home"].as_str().unwrap_or("").to_string(),
        kind: args["kind"].as_str().unwrap_or("").to_string(),
        machine_yaml: args["machine_yaml"].as_str().unwrap_or("").to_string(),
        actor_name: args["actor_name"].as_str().unwrap_or("").to_string(),
        actor_type: args["actor_type"].as_str().unwrap_or("").to_string(),
        actor_model: args["actor_model"].as_str().unwrap_or("").to_string(),
        actor_provider: args["actor_provider"].as_str().unwrap_or("").to_string(),
        actor_context_window: args["actor_context_window"].as_i64().unwrap_or(0),
        actor_sdk_version: args["actor_sdk_version"].as_str().unwrap_or("").to_string(),
        actor_entrypoint: args["actor_entrypoint"].as_str().unwrap_or("").to_string(),
        // Optional role-hook passthrough. Each {name, content} entry is forwarded
        // as a proto PlaybookHook; the engine writes them under hooks/ and the
        // domain loader validates every machine.yaml `hook:` reference against the
        // carried filenames. Absent/omitted → empty (machine.yaml-only), preserving
        // byte-identical behavior for callers that never send `hooks`.
        hooks: playbook_hooks_arg(&args["hooks"]),
    };

    let resp = match call_engine(
        id,
        engine_port,
        rt,
        foundry_mode,
        persist_request,
        |client, request| Box::pin(client.persist_playbook(request)),
        |s| {
            format!(
                "PersistPlaybook RPC failed: {}: {}",
                grpc_code_label(s.code()),
                s.message()
            )
        },
    ) {
        Ok(resp) => resp,
        Err(error_response) => return error_response,
    };

    let result_json = serde_json::json!({
        "kind": resp.kind,
        "written_path": resp.written_path,
        "resolved_owner_home": resp.resolved_owner_home
    });
    tool_text_response(id, &result_json)
}

/// Parse the optional `hooks` argument of `persist_playbook` into proto
/// `PlaybookHook`s. Accepts an array of `{name, content}` objects; a missing or
/// non-array value yields an empty vec (machine.yaml-only). Entries missing
/// either field default that field to an empty string so the domain's own
/// validation (unknown-hook-reference) remains the single source of truth rather
/// than the wire silently dropping malformed hooks.
fn playbook_hooks_arg(value: &Value) -> Vec<PlaybookHook> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| PlaybookHook {
                    name: item["name"].as_str().unwrap_or("").to_string(),
                    content: item["content"].as_str().unwrap_or("").to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Parse the optional `claimed_evidence` argument of `complete` into proto
/// `ClaimedEvidence`s. Accepts an array of `{class, reference}` objects; a
/// missing or non-array value yields an empty vec — byte-identical to the
/// pre-affordance hardcode. Entries missing either field default that field to
/// an empty string so the engine's already-shipped `claimed_evidence_to_domain`
/// stays the single validator (unknown-class → INVALID_ARGUMENT) rather than the
/// wire silently dropping a malformed claim. Order is preserved. The reference is
/// copied verbatim — the shim never parses, normalizes, or resolves it.
fn claimed_evidence_arg(value: &Value) -> Vec<ClaimedEvidence> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| ClaimedEvidence {
                    class: item["class"].as_str().unwrap_or("").to_string(),
                    reference: item["reference"].as_str().unwrap_or("").to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn string_array_arg(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToString::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn proposed_states_arg(
    value: &Value,
) -> Result<Vec<anvil_engine::proto::candidate_playbook::ProposedState>, String> {
    let states = value
        .as_array()
        .ok_or_else(|| "proposed_states must be an array".to_string())?;
    states
        .iter()
        .enumerate()
        .map(|(idx, state)| {
            if !state.is_object() {
                return Err(format!("proposed_states[{}] must be an object", idx));
            }
            Ok(anvil_engine::proto::candidate_playbook::ProposedState {
                state: required_non_empty_string(state, idx, "state")?,
                role: required_non_empty_string(state, idx, "role")?,
                intent: required_non_empty_string(state, idx, "intent")?,
                expected_output: required_non_empty_string(state, idx, "expected_output")?,
                evidence_obligation: Vec::new(),
            })
        })
        .collect()
}

fn required_non_empty_string(value: &Value, idx: usize, field: &str) -> Result<String, String> {
    match value.get(field) {
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(s.to_string()),
        Some(Value::String(_)) => Err(format!(
            "proposed_states[{}].{} must be a non-empty string",
            idx, field
        )),
        Some(_) => Err(format!(
            "proposed_states[{}].{} must be a non-empty string",
            idx, field
        )),
        None => Err(format!(
            "proposed_states[{}].{} must be a non-empty string",
            idx, field
        )),
    }
}

fn required_candidate_string(value: &Value, field: &str) -> Result<String, String> {
    match value.get(field) {
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(s.to_string()),
        Some(_) | None => Err(format!("candidate.{} must be a non-empty string", field)),
    }
}

fn required_candidate_string_array(value: &Value, field: &str) -> Result<Vec<String>, String> {
    let values = string_array_arg(&value[field]);
    if values.is_empty() {
        Err(format!("candidate.{} must be a non-empty array", field))
    } else {
        Ok(values)
    }
}

fn candidate_playbook_arg(args: &Value) -> Result<anvil_engine::proto::CandidatePlaybook, String> {
    let candidate = if args["candidate"].is_object() {
        &args["candidate"]
    } else {
        args
    };
    Ok(anvil_engine::proto::CandidatePlaybook {
        source: required_candidate_string(candidate, "source")?,
        intent: candidate["intent"].as_str().unwrap_or("").to_string(),
        at: required_candidate_string(candidate, "at")?,
        evidence: string_array_arg(&candidate["evidence"]),
        proposed_states: proposed_states_arg(&candidate["proposed_states"])?,
        route_description: required_candidate_string(candidate, "route_description")?,
        route_triggers: required_candidate_string_array(candidate, "route_triggers")?,
        projection_targets: required_candidate_string_array(candidate, "projection_targets")?,
        success_rubric: success_rubric_arg(candidate.get("success_rubric"))?,
        anchors: anchors_arg(candidate.get("anchors"))?,
        exemplars: exemplars_arg(candidate.get("exemplars"))?,
        ledger_classification: ledger_classification_arg(candidate.get("ledger_classification"))?,
        none_yet_justification: none_yet_justification_arg(
            candidate.get("none_yet_justification"),
        )?,
        register: String::new(),
    })
}

fn success_rubric_arg(
    value: Option<&Value>,
) -> Result<Option<anvil_engine::proto::candidate_playbook::SuccessRubric>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let object = value
        .as_object()
        .ok_or_else(|| "candidate.success_rubric must be an object".to_string())?;
    let dimensions = object
        .get("dimensions")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .enumerate()
                .map(|(idx, item)| {
                    let item = item.as_object().ok_or_else(|| {
                        format!("candidate.success_rubric.dimensions[{}] must be an object", idx)
                    })?;
                    Ok::<_, String>(anvil_engine::proto::candidate_playbook::RubricDimension {
                        dimension: json_string(item.get("dimension")).ok_or_else(|| {
                            format!(
                                "candidate.success_rubric.dimensions[{}].dimension must be a string",
                                idx
                            )
                        })?,
                        weight: item
                            .get("weight")
                            .and_then(Value::as_u64)
                            .ok_or_else(|| {
                                format!(
                                    "candidate.success_rubric.dimensions[{}].weight must be an integer",
                                    idx
                                )
                            })? as u32,
                        evidence_class: json_string(item.get("evidence_class"))
                            .unwrap_or_else(|| "self_description".to_string()),
                    })
                })
                .collect()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(Some(
        anvil_engine::proto::candidate_playbook::SuccessRubric {
            dimensions,
            grader: json_string(object.get("grader")).unwrap_or_default(),
            lagging_signals: object
                .get("lagging_signals")
                .map(string_array_arg)
                .unwrap_or_default(),
            anchors: anchors_arg(object.get("anchors"))?,
        },
    ))
}

fn anchors_arg(
    value: Option<&Value>,
) -> Result<Vec<anvil_engine::proto::candidate_playbook::AnchorRef>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    if value.is_null() {
        return Ok(Vec::new());
    }
    let anchors = value
        .as_array()
        .ok_or_else(|| "candidate.anchors must be an array".to_string())?;
    anchors
        .iter()
        .enumerate()
        .map(|(idx, anchor)| {
            let anchor = anchor
                .as_object()
                .ok_or_else(|| format!("candidate.anchors[{}] must be an object", idx))?;
            Ok(anvil_engine::proto::candidate_playbook::AnchorRef {
                instance: json_string(anchor.get("instance")).ok_or_else(|| {
                    format!("candidate.anchors[{}].instance must be a string", idx)
                })?,
                band: json_string(anchor.get("band"))
                    .ok_or_else(|| format!("candidate.anchors[{}].band must be a string", idx))?,
            })
        })
        .collect()
}

fn exemplars_arg(
    value: Option<&Value>,
) -> Result<Vec<anvil_engine::proto::candidate_playbook::Exemplar>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    if value.is_null() {
        return Ok(Vec::new());
    }
    let exemplars = value
        .as_array()
        .ok_or_else(|| "candidate.exemplars must be an array".to_string())?;
    exemplars
        .iter()
        .enumerate()
        .map(|(idx, exemplar)| {
            let exemplar = exemplar
                .as_object()
                .ok_or_else(|| format!("candidate.exemplars[{}] must be an object", idx))?;
            let frontmatter = exemplar
                .get("frontmatter")
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    format!("candidate.exemplars[{}].frontmatter must be an object", idx)
                })?;
            Ok(anvil_engine::proto::candidate_playbook::Exemplar {
                frontmatter: Some(
                    anvil_engine::proto::candidate_playbook::ExemplarFrontmatter {
                        id: json_string(frontmatter.get("id")).ok_or_else(|| {
                            format!(
                                "candidate.exemplars[{}].frontmatter.id must be a string",
                                idx
                            )
                        })?,
                        band: json_string(frontmatter.get("band")).ok_or_else(|| {
                            format!(
                                "candidate.exemplars[{}].frontmatter.band must be a string",
                                idx
                            )
                        })?,
                        dimensions: frontmatter
                            .get("dimensions")
                            .map(string_array_arg)
                            .unwrap_or_default(),
                        evidence_class: json_string(frontmatter.get("evidence_class"))
                            .unwrap_or_else(|| "self_description".to_string()),
                        outcome_link: outcome_link_arg(frontmatter.get("outcome_link"))?,
                        provenance: provenance_arg(frontmatter.get("provenance"))?,
                        playbook_version: json_string(frontmatter.get("playbook_version"))
                            .unwrap_or_default(),
                        refreshed_at: json_string(frontmatter.get("refreshed_at"))
                            .unwrap_or_default(),
                    },
                ),
                body: json_string(exemplar.get("body")).unwrap_or_default(),
            })
        })
        .collect()
}

fn outcome_link_arg(
    value: Option<&Value>,
) -> Result<Option<anvil_engine::proto::candidate_playbook::OutcomeLink>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let object = value.as_object().ok_or_else(|| {
        "candidate.exemplars[].frontmatter.outcome_link must be an object".to_string()
    })?;
    Ok(Some(anvil_engine::proto::candidate_playbook::OutcomeLink {
        authority: json_string(object.get("authority")).unwrap_or_default(),
        opaque_ref: json_string(object.get("opaque_ref")).unwrap_or_default(),
        verified_at: json_string(object.get("verified_at")).unwrap_or_default(),
    }))
}

fn provenance_arg(
    value: Option<&Value>,
) -> Result<Option<anvil_engine::proto::candidate_playbook::ExemplarProvenance>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let object = value.as_object().ok_or_else(|| {
        "candidate.exemplars[].frontmatter.provenance must be an object".to_string()
    })?;
    Ok(Some(
        anvil_engine::proto::candidate_playbook::ExemplarProvenance {
            source: json_string(object.get("source")).unwrap_or_default(),
            corpus: json_string(object.get("corpus")).unwrap_or_default(),
        },
    ))
}

fn ledger_classification_arg(
    value: Option<&Value>,
) -> Result<Option<anvil_engine::proto::candidate_playbook::LedgerClassification>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let object = value
        .as_object()
        .ok_or_else(|| "candidate.ledger_classification must be an object".to_string())?;
    Ok(Some(
        anvil_engine::proto::candidate_playbook::LedgerClassification {
            corpus: json_string(object.get("corpus")).unwrap_or_default(),
            ledger: json_string(object.get("ledger")).unwrap_or_default(),
            classification: json_string(object.get("classification")).unwrap_or_default(),
        },
    ))
}

fn none_yet_justification_arg(
    value: Option<&Value>,
) -> Result<Option<anvil_engine::proto::candidate_playbook::NoneYetJustification>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let object = value
        .as_object()
        .ok_or_else(|| "candidate.none_yet_justification must be an object".to_string())?;
    Ok(Some(
        anvil_engine::proto::candidate_playbook::NoneYetJustification {
            corpus_searched: json_string(object.get("corpus_searched")).unwrap_or_default(),
            ledger_searched: json_string(object.get("ledger_searched")).unwrap_or_default(),
            why_no_exemplar: json_string(object.get("why_no_exemplar")).unwrap_or_default(),
            production_routing_allowed: object
                .get("production_routing_allowed")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            followup_condition: json_string(object.get("followup_condition")).unwrap_or_default(),
        },
    ))
}

fn json_string(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(ToString::to_string)
}

fn handle_intake_candidate_playbook_call(
    id: &Value,
    request: &Value,
    hearth_path: &PathBuf,
    engine_port: &mut u16,
    rt: &tokio::runtime::Runtime,
    foundry_mode: &FoundryMode,
) -> Value {
    let args = &request["params"]["arguments"];
    let candidate = match candidate_playbook_arg(args) {
        Ok(candidate) => candidate,
        Err(msg) => return return_tool_error(id, &format!("INVALID_ARGUMENT: {}", msg)),
    };

    let intake_request = IntakeCandidatePlaybookRequest {
        hearth_path: hearth_path.display().to_string(),
        candidate: Some(candidate),
        target_owner: args["target_owner"].as_str().unwrap_or("").to_string(),
        parent_id: args["parent_id"].as_str().unwrap_or("").to_string(),
        approver: args["approver"].as_str().unwrap_or("lore").to_string(),
        actor_name: args["actor_name"].as_str().unwrap_or("").to_string(),
        actor_type: args["actor_type"].as_str().unwrap_or("").to_string(),
        actor_model: args["actor_model"].as_str().unwrap_or("").to_string(),
        actor_provider: args["actor_provider"].as_str().unwrap_or("").to_string(),
        actor_context_window: args["actor_context_window"].as_i64().unwrap_or(0),
        actor_sdk_version: args["actor_sdk_version"].as_str().unwrap_or("").to_string(),
        actor_entrypoint: args["actor_entrypoint"].as_str().unwrap_or("").to_string(),
    };

    let resp = match call_engine(
        id,
        engine_port,
        rt,
        foundry_mode,
        intake_request,
        |client, request| Box::pin(client.intake_candidate_playbook(request)),
        |s| {
            format!(
                "IntakeCandidatePlaybook RPC failed: {}: {}",
                grpc_code_label(s.code()),
                s.message()
            )
        },
    ) {
        Ok(resp) => resp,
        Err(error_response) => return error_response,
    };

    let result_json = serde_json::json!({
        "instance_id": resp.instance_id,
        "kind": resp.kind,
        "resolved_owner_home": resp.resolved_owner_home,
        "playbook_name": resp.playbook_name
    });
    tool_text_response(id, &result_json)
}

/// Handle a tools/call for "begin_adoption_status" — pure-read passthrough to
/// the BeginAdoptionStatus RPC. No session injection; actor_name, artifact_path,
/// and state are forwarded from the caller's arguments verbatim.
fn handle_begin_adoption_status_call(
    id: &Value,
    request: &Value,
    hearth_path: &PathBuf,
    engine_port: &mut u16,
    rt: &tokio::runtime::Runtime,
    foundry_mode: &FoundryMode,
) -> Value {
    let args = &request["params"]["arguments"];

    let rpc_request = BeginAdoptionStatusRequest {
        hearth_path: hearth_path.display().to_string(),
        actor_name: args["actor_name"].as_str().unwrap_or("").to_string(),
        artifact_path: args["artifact_path"].as_str().unwrap_or("").to_string(),
        state: args["state"].as_str().unwrap_or("").to_string(),
    };

    let resp = match call_engine(
        id,
        engine_port,
        rt,
        foundry_mode,
        rpc_request,
        |client, request| Box::pin(client.begin_adoption_status(request)),
        |s| format!("BeginAdoptionStatus RPC failed: {}", s.message()),
    ) {
        Ok(resp) => resp,
        Err(error_response) => return error_response,
    };

    let result_json = serde_json::json!({
        "has_open_begin": resp.has_open_begin,
    });
    tool_text_response(id, &result_json)
}

/// Ensure the supervisor-owned engine is reachable. The shim discovers and
/// connects only; it never starts or owns an engine process.
fn ensure_engine_reachable(
    engine_port: &mut u16,
    rt: &tokio::runtime::Runtime,
) -> Result<(), String> {
    let port = resolve_engine_port();
    let expected_wire_version = expected_wire_proto_version();

    // Discovery is RETRIED, briefly. A single probe cannot distinguish "no
    // engine" from "the engine did not answer THIS instant", and the moment that
    // distinction matters most is `update_kit`: the supervisor restarts the
    // engine, and a one-shot probe turns a sub-second restart into a hard
    // `daemon_unreachable` for whoever happened to send a turn.
    //
    // Measured: the anvil-mcp suite failed ~1 scenario per full run on exactly
    // this, and the SAME feature passed 6/6 in isolation — the signature of a
    // probe losing a race under load, not of an absent engine.
    //
    // Bounded on purpose. This must not paper over a genuinely missing engine,
    // which is a real condition users need told about; ~1s total is long enough
    // to ride out a restart and far too short to hide an outage.
    const DISCOVERY_ATTEMPTS: usize = 4;
    const DISCOVERY_BACKOFF: std::time::Duration = std::time::Duration::from_millis(250);
    let mut health = None;
    for attempt in 0..DISCOVERY_ATTEMPTS {
        health = anvil_health_check(rt, port);
        if health.is_some() {
            if attempt > 0 {
                diag!(
                    "engine discovery succeeded on attempt {} of {}",
                    attempt + 1,
                    DISCOVERY_ATTEMPTS
                );
            }
            break;
        }
        if attempt + 1 < DISCOVERY_ATTEMPTS {
            std::thread::sleep(DISCOVERY_BACKOFF);
        }
    }

    if let Some(response) = health {
        let engine_wire_version = response.wire_proto_version;
        if engine_wire_version == expected_wire_version {
            *engine_port = port;
            return Ok(());
        }

        return Err(engine_proto_version_mismatch_error(
            port,
            engine_wire_version,
            response.build_version,
            expected_wire_version,
        ));
    }

    // Foreign-holder behavior: if the discovered endpoint accepts TCP but fails
    // the Anvil HealthCheck, report a discovery failure and never compete.
    let addr: std::net::SocketAddr = ([127, 0, 0, 1], port).into();
    if std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(250)).is_ok() {
        return Err(format!(
            "daemon_unreachable: engine endpoint 127.0.0.1:{} is occupied but did not answer Anvil HealthCheck; discovery failed",
            port
        ));
    }

    Err(format!(
        "daemon_unreachable: anvil engine not reachable on 127.0.0.1:{}; start Foundry (or launch anvil-engine manually for local dev).",
        port
    ))
}

fn resolve_engine_port() -> u16 {
    engine_port_from_env()
        .or_else(engine_port_from_rendezvous)
        .unwrap_or(DEFAULT_ENGINE_PORT)
}

fn engine_port_from_env() -> Option<u16> {
    std::env::var("ANVIL_ENGINE_PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
}

fn engine_port_from_rendezvous() -> Option<u16> {
    let path = std::env::var_os("HOME")
        .map(PathBuf::from)?
        .join(".anvil")
        .join("engine.json");
    let bytes = std::fs::read(path).ok()?;
    let record: Value = serde_json::from_slice(&bytes).ok()?;
    let url = record.get("url")?.as_str()?;
    port_from_http_url(url)
}

fn port_from_http_url(url: &str) -> Option<u16> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))?;
    let authority = rest.split('/').next().unwrap_or(rest);
    let host_port = authority.rsplit_once(':')?;
    host_port.1.parse::<u16>().ok()
}

/// Probe the Anvil HealthCheck on a port. Returns the response only when the
/// endpoint answers Anvil's gRPC HealthCheck (not merely accepts TCP).
fn anvil_health_check(
    rt: &tokio::runtime::Runtime,
    port: u16,
) -> Option<anvil_engine::proto::HealthCheckResponse> {
    rt.block_on(async {
        let addr = format!("http://127.0.0.1:{}", port);
        if let Ok(mut client) = AnvilServiceClient::connect(addr).await {
            let request = tonic::Request::new(HealthCheckRequest {});
            client
                .health_check(request)
                .await
                .map(|r| r.into_inner())
                .ok()
        } else {
            None
        }
    })
}

fn expected_wire_proto_version() -> u32 {
    std::env::var("ANVIL_SHIM_EXPECT_WIRE_VERSION")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(anvil_engine::WIRE_PROTO_VERSION)
}

fn engine_proto_version_mismatch_error(
    port: u16,
    engine_wire_version: u32,
    build_version: String,
    expected_wire_version: u32,
) -> String {
    format!(
        "{}: engine on :{} reports wire v{} (build {}) but this shim expects wire v{}. The engine on the shared endpoint is stale/incompatible - restart it from a current build (or let Foundry reap+restart the active-version engine).",
        CLIENT_ENGINE_PROTO_VERSION_MISMATCH,
        port,
        engine_wire_version,
        build_version,
        expected_wire_version
    )
}

// ---------------------------------------------------------------------------
// K8 backlog_item MCP tools (plan Task 9).
//
// Every tool and nested object is `type: object` with
// `additionalProperties: false`, so an unknown key is refused BEFORE any gRPC
// call. Roles are closed per tool, no tool exposes a caller-controlled history
// sequence, audit stamp, state, or rank position (except a DECIDE proposal),
// and every engine failure propagates loudly.
// ---------------------------------------------------------------------------

/// The twelve K8 tool names, in advertised order.
pub(crate) const BACKLOG_TOOLS: [&str; 12] = [
    "backlog_shape_edit",
    "backlog_recompute_rank",
    "backlog_stamp_execution_binding",
    "backlog_record_outcome_signoff",
    "backlog_propose_reshuffle",
    "backlog_commit_reshuffle",
    "backlog_reject_reshuffle",
    "backlog_veto_age_out",
    "backlog_lift_age_out_veto",
    "backlog_evaluate",
    "backlog_organ_queue",
    "backlog_cross_organ_view",
];

fn backlog_object(properties: Value, required: Vec<&str>) -> Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "properties": properties,
        "required": required
    })
}

fn backlog_evidence_ref_schema() -> Value {
    backlog_object(
        serde_json::json!({
            "kind": { "type": "string" },
            "id": { "type": "string" }
        }),
        vec!["kind", "id"],
    )
}

fn backlog_identity_properties(roles: &[&str]) -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::new();
    map.insert("actor_name".into(), serde_json::json!({ "type": "string" }));
    map.insert(
        "actor_role".into(),
        serde_json::json!({ "type": "string", "enum": roles }),
    );
    map.insert("actor_type".into(), serde_json::json!({ "type": "string" }));
    map.insert("actor_model".into(), serde_json::json!({ "type": "string" }));
    map.insert(
        "actor_provider".into(),
        serde_json::json!({ "type": "string" }),
    );
    map.insert(
        "actor_context_window".into(),
        serde_json::json!({ "type": "integer", "minimum": 1 }),
    );
    map.insert(
        "actor_sdk_version".into(),
        serde_json::json!({ "type": "string" }),
    );
    map.insert(
        "actor_entrypoint".into(),
        serde_json::json!({ "type": "string" }),
    );
    map.insert("hearth".into(), serde_json::json!({ "type": "string" }));
    map.insert("project".into(), serde_json::json!({ "type": "string" }));
    map
}

const BACKLOG_IDENTITY_REQUIRED: [&str; 5] = [
    "actor_name",
    "actor_role",
    "actor_type",
    "actor_model",
    "actor_provider",
];

fn backlog_mutation_tool(
    name: &str,
    description: &str,
    roles: &[&str],
    extra: Vec<(&str, Value)>,
    extra_required: Vec<&str>,
) -> Value {
    let mut properties = backlog_identity_properties(roles);
    for (key, schema) in extra {
        properties.insert(key.to_string(), schema);
    }
    let mut required: Vec<String> = BACKLOG_IDENTITY_REQUIRED
        .iter()
        .map(|s| s.to_string())
        .collect();
    required.extend(extra_required.into_iter().map(|s| s.to_string()));
    serde_json::json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "additionalProperties": false,
            "properties": Value::Object(properties),
            "required": required
        }
    })
}

/// The advertised K8 tool definitions.
pub(crate) fn backlog_tool_definitions() -> Vec<Value> {
    let value_gap = backlog_object(
        serde_json::json!({
            "ref": backlog_evidence_ref_schema(),
            "magnitude": { "type": "number" }
        }),
        vec!["ref", "magnitude"],
    );
    let dependency = backlog_object(
        serde_json::json!({
            "status": { "type": "string", "enum": ["ready", "blocked"] },
            "blocker_refs": { "type": "array", "items": backlog_evidence_ref_schema() }
        }),
        vec!["status", "blocker_refs"],
    );
    let wake = backlog_object(
        serde_json::json!({
            "kind": {
                "type": "string",
                "enum": ["item_state", "dependency_ready", "manual", "measure_threshold", "external_event"]
            },
            "ref": backlog_evidence_ref_schema(),
            "predicate": { "type": "string" }
        }),
        vec!["kind", "predicate"],
    );
    let playbook = backlog_object(
        serde_json::json!({
            "playbook_definition_id": { "type": ["string", "null"] },
            "route_to_intake": { "type": "boolean" }
        }),
        vec!["playbook_definition_id", "route_to_intake"],
    );
    let execution = backlog_object(
        serde_json::json!({
            "track_id": { "type": "string" },
            "playbook_definition_id": { "type": "string" },
            "playbook_run_id": { "type": "string" },
            "run_id": { "type": "string" }
        }),
        vec![
            "track_id",
            "playbook_definition_id",
            "playbook_run_id",
            "run_id",
        ],
    );
    let outcome = backlog_object(
        serde_json::json!({
            "success_measure_id": { "type": ["string", "null"] },
            "tree_node": { "type": "string" },
            "reading_status": { "type": "string", "enum": ["registered", "unmeasurable_signed"] }
        }),
        vec!["success_measure_id", "tree_node", "reading_status"],
    );
    let rank_inputs = serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "minProperties": 1,
        "properties": {
            "value_gap_magnitude": { "oneOf": [value_gap.clone(), { "type": "null" }] },
            "nick_weight": { "type": ["number", "null"] },
            "dependency_readiness": { "oneOf": [dependency.clone(), { "type": "null" }] }
        }
    });
    let changes = serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "minProperties": 1,
        "properties": {
            "action_class": { "type": "string" },
            "description": { "type": ["string", "null"] },
            "effort_class": { "type": ["string", "null"] },
            "playbook_binding": { "oneOf": [playbook, { "type": "null" }] },
            "rank_inputs": rank_inputs,
            "wake_condition": { "oneOf": [wake, { "type": "null" }] },
            "superseded_by": { "type": ["string", "null"] }
        }
    });

    let item_id = || serde_json::json!({ "type": "string" });
    let approver = || serde_json::json!({ "type": "string" });

    let mut tools = vec![
        backlog_mutation_tool(
            "backlog_shape_edit",
            "Apply one closed SHAPE edit to a backlog item. Never changes lifecycle state, rank position, rank age, the effort mirror, or the explanation.",
            &["nick_shape", "organ_loop", "orchestrator", "track_driver"],
            vec![("backlog_item_id", item_id()), ("changes", changes)],
            vec!["backlog_item_id", "changes"],
        ),
        backlog_mutation_tool(
            "backlog_recompute_rank",
            "Re-rank one organ: consume pending rank inputs in sequence, mirror effort, advance cycle age, and materialize the complete per-organ positions.",
            &["organ_loop"],
            vec![("business_node_id", serde_json::json!({ "type": "string" }))],
            vec!["business_node_id"],
        ),
        backlog_mutation_tool(
            "backlog_stamp_execution_binding",
            "Stamp the #5 pickup precondition on a ready item: execution binding plus the declared outcome binding.",
            &["track_driver"],
            vec![
                ("backlog_item_id", item_id()),
                ("execution_binding", execution),
                ("outcome_binding", outcome),
            ],
            vec!["backlog_item_id", "execution_binding", "outcome_binding"],
        ),
        backlog_mutation_tool(
            "backlog_record_outcome_signoff",
            "Nick's unmeasurable-outcome sign-off on an in-flight item (NICK-GATE).",
            &["nick_shape"],
            vec![("backlog_item_id", item_id()), ("approver", approver())],
            vec!["backlog_item_id", "approver"],
        ),
        backlog_mutation_tool(
            "backlog_propose_reshuffle",
            "Orchestrator DECIDE proposal over one organ's complete ranked set. Moves no position.",
            &["orchestrator"],
            vec![(
                "proposed",
                serde_json::json!({
                    "type": "array",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "backlog_item_id": { "type": "string" },
                            "position": { "type": "integer", "minimum": 1, "maximum": 4294967295u32 }
                        },
                        "required": ["backlog_item_id", "position"]
                    }
                }),
            )],
            vec!["proposed"],
        ),
        backlog_mutation_tool(
            "backlog_commit_reshuffle",
            "Nick commits one unresolved, non-stale DECIDE proposal. Every authorization entry is appended before any approved position byte.",
            &["nick_shape"],
            vec![
                ("proposal_id", serde_json::json!({ "type": "string" })),
                ("approver", approver()),
            ],
            vec!["proposal_id", "approver"],
        ),
        backlog_mutation_tool(
            "backlog_reject_reshuffle",
            "Nick rejects one unresolved DECIDE proposal. Changes no position.",
            &["nick_shape"],
            vec![
                ("proposal_id", serde_json::json!({ "type": "string" })),
                ("approver", approver()),
            ],
            vec!["proposal_id", "approver"],
        ),
        backlog_mutation_tool(
            "backlog_veto_age_out",
            "Set Nick's standing age-out exemption on a candidate/ready/parked item (NICK-GATE).",
            &["nick_shape"],
            vec![("backlog_item_id", item_id()), ("approver", approver())],
            vec!["backlog_item_id", "approver"],
        ),
        backlog_mutation_tool(
            "backlog_lift_age_out_veto",
            "Lift the unique unresolved age-out veto. The engine resolves and records its sequence; no caller supplies it.",
            &["nick_shape"],
            vec![("backlog_item_id", item_id()), ("approver", approver())],
            vec!["backlog_item_id", "approver"],
        ),
    ];

    // Evaluate accepts NO actor_role: the engine stamps engine_auto itself.
    let mut evaluate_props = backlog_identity_properties(&[]);
    evaluate_props.remove("actor_role");
    evaluate_props.insert(
        "business_node_id".into(),
        serde_json::json!({ "type": "string" }),
    );
    tools.push(serde_json::json!({
        "name": "backlog_evaluate",
        "description": "Run one engine-auto evaluation batch. Empty business_node_id means every organ. The engine constructs the private evaluation origin and stamps engine_auto; no caller can select it.",
        "inputSchema": {
            "type": "object",
            "additionalProperties": false,
            "properties": Value::Object(evaluate_props),
            "required": ["actor_name", "actor_type", "actor_model", "actor_provider"]
        }
    }));

    tools.push(serde_json::json!({
        "name": "backlog_organ_queue",
        "description": "Read one organ's ranked queue plus its explicit unranked pre-triage partition. Never writes.",
        "inputSchema": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "business_node_id": { "type": "string" },
                "hearth": { "type": "string" },
                "project": { "type": "string" }
            },
            "required": ["business_node_id"]
        }
    }));
    tools.push(serde_json::json!({
        "name": "backlog_cross_organ_view",
        "description": "Read the aggregated cross-organ ranked and unranked partitions. Never writes.",
        "inputSchema": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "hearth": { "type": "string" },
                "project": { "type": "string" }
            },
            "required": []
        }
    }));
    tools
}

/// Repeat the closed schema check MANUALLY so a schema bypass cannot broaden
/// any path. Returns the refusal reason when the payload is not admissible.
fn backlog_reject_unknown_keys(arguments: &Value, allowed: &[&str]) -> Option<String> {
    let map = arguments.as_object()?;
    for key in map.keys() {
        if !allowed.contains(&key.as_str()) {
            return Some(format!("unknown key `{key}`"));
        }
    }
    None
}

fn backlog_required_str(arguments: &Value, key: &str) -> Result<String, String> {
    arguments[key]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("`{key}` is required and must be a nonempty string"))
}

fn backlog_evidence_from_json(value: &Value, field: &str) -> Result<BacklogEvidenceRef, String> {
    let kind = value["kind"]
        .as_str()
        .ok_or_else(|| format!("{field}.kind is required"))?;
    let id = value["id"]
        .as_str()
        .ok_or_else(|| format!("{field}.id is required"))?;
    Ok(BacklogEvidenceRef {
        kind: kind.to_string(),
        id: id.to_string(),
    })
}

fn backlog_error(id: &Value, reason: &str) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": -32602,
            "message": format!("Invalid params: {reason}"),
            "data": { "code": "invalid_params" }
        }
    })
}

/// `backlog_shape_edit` .. `backlog_lift_age_out_veto` -> one BacklogMutate.
fn handle_backlog_mutate_call(
    id: &Value,
    tool: &str,
    request: &Value,
    hearth_path: &PathBuf,
    engine_port: &mut u16,
    rt: &tokio::runtime::Runtime,
    foundry_mode: &FoundryMode,
) -> Value {
    let arguments = &request["params"]["arguments"];
    let operation = match backlog_operation_from_json(tool, arguments) {
        Ok(op) => op,
        Err(reason) => return backlog_error(id, &reason),
    };
    let role = match backlog_fixed_role(tool, arguments) {
        Ok(role) => role,
        Err(reason) => return backlog_error(id, &reason),
    };
    let mutate = BacklogMutateRequest {
        hearth_path: hearth_path.display().to_string(),
        actor_name: arguments["actor_name"].as_str().unwrap_or_default().to_string(),
        actor_role: role,
        actor_type: arguments["actor_type"].as_str().unwrap_or_default().to_string(),
        actor_model: arguments["actor_model"].as_str().unwrap_or_default().to_string(),
        actor_provider: arguments["actor_provider"].as_str().unwrap_or_default().to_string(),
        actor_context_window: arguments["actor_context_window"].as_i64().unwrap_or(0),
        actor_sdk_version: arguments["actor_sdk_version"].as_str().unwrap_or_default().to_string(),
        actor_entrypoint: arguments["actor_entrypoint"].as_str().unwrap_or_default().to_string(),
        operation: Some(operation),
    };
    let response = match call_engine(
        id,
        engine_port,
        rt,
        foundry_mode,
        mutate,
        |client, request| Box::pin(client.backlog_mutate(request)),
        |s| format!("BacklogMutate RPC failed: {}", s.message()),
    ) {
        Ok(response) => response,
        Err(error_response) => return error_response,
    };
    let mut body = serde_json::json!({
        "success": response.success,
        "operation": response.operation,
        "affected_item_ids": response.affected_item_ids,
        "unranked_item_ids": response.unranked_item_ids,
        "resolved_hearth": response.resolved_hearth,
    });
    if !response.proposal_id.is_empty() {
        body["proposal_id"] = Value::String(response.proposal_id);
    }
    tool_text_response(id, &body)
}

fn handle_backlog_evaluate_call(
    id: &Value,
    request: &Value,
    hearth_path: &PathBuf,
    engine_port: &mut u16,
    rt: &tokio::runtime::Runtime,
    foundry_mode: &FoundryMode,
) -> Value {
    let arguments = &request["params"]["arguments"];
    if let Some(reason) = backlog_reject_unknown_keys(
        arguments,
        &[
            "actor_name",
            "actor_type",
            "actor_model",
            "actor_provider",
            "actor_context_window",
            "actor_sdk_version",
            "actor_entrypoint",
            "business_node_id",
            "hearth",
            "project",
        ],
    ) {
        return backlog_error(id, &reason);
    }
    // `backlog_evaluate` carries NO actor_role: the engine stamps engine_auto.
    if arguments.get("actor_role").is_some() {
        return backlog_error(id, "backlog_evaluate does not accept actor_role");
    }
    for key in ["actor_name", "actor_type", "actor_model", "actor_provider"] {
        if let Err(reason) = backlog_required_str(arguments, key) {
            return backlog_error(id, &reason);
        }
    }
    let evaluate = BacklogEvaluateRequest {
        hearth_path: hearth_path.display().to_string(),
        business_node_id: arguments["business_node_id"].as_str().unwrap_or_default().to_string(),
        actor_name: arguments["actor_name"].as_str().unwrap_or_default().to_string(),
        actor_type: arguments["actor_type"].as_str().unwrap_or_default().to_string(),
        actor_model: arguments["actor_model"].as_str().unwrap_or_default().to_string(),
        actor_provider: arguments["actor_provider"].as_str().unwrap_or_default().to_string(),
        actor_context_window: arguments["actor_context_window"].as_i64().unwrap_or(0),
        actor_sdk_version: arguments["actor_sdk_version"].as_str().unwrap_or_default().to_string(),
        actor_entrypoint: arguments["actor_entrypoint"].as_str().unwrap_or_default().to_string(),
    };
    let response = match call_engine(
        id,
        engine_port,
        rt,
        foundry_mode,
        evaluate,
        |client, request| Box::pin(client.backlog_evaluate(request)),
        |s| format!("BacklogEvaluate RPC failed: {}", s.message()),
    ) {
        Ok(response) => response,
        Err(error_response) => return error_response,
    };
    tool_text_response(
        id,
        &serde_json::json!({
            "success": response.success,
            "recomputed_item_ids": response.recomputed_item_ids,
            "woken_item_ids": response.woken_item_ids,
            "aged_out_item_ids": response.aged_out_item_ids,
            "vetoed_item_ids": response.vetoed_item_ids,
            "unranked_item_ids": response.unranked_item_ids,
            "resolved_hearth": response.resolved_hearth,
        }),
    )
}

fn handle_backlog_queue_call(
    id: &Value,
    tool: &str,
    request: &Value,
    hearth_path: &PathBuf,
    engine_port: &mut u16,
    rt: &tokio::runtime::Runtime,
    foundry_mode: &FoundryMode,
) -> Value {
    let arguments = &request["params"]["arguments"];
    let allowed: &[&str] = if tool == "backlog_organ_queue" {
        &["business_node_id", "hearth", "project"]
    } else {
        &["hearth", "project"]
    };
    if let Some(reason) = backlog_reject_unknown_keys(arguments, allowed) {
        return backlog_error(id, &reason);
    }
    let scope = if tool == "backlog_organ_queue" {
        match backlog_required_str(arguments, "business_node_id") {
            Ok(organ) => backlog_queue_request::Scope::BusinessNodeId(organ),
            Err(reason) => return backlog_error(id, &reason),
        }
    } else {
        backlog_queue_request::Scope::CrossOrgan(BacklogPresence {})
    };
    let response = match call_engine(
        id,
        engine_port,
        rt,
        foundry_mode,
        BacklogQueueRequest {
            hearth_path: hearth_path.display().to_string(),
            scope: Some(scope),
        },
        |client, request| Box::pin(client.backlog_queue(request)),
        |s| format!("BacklogQueue RPC failed: {}", s.message()),
    ) {
        Ok(response) => response,
        Err(error_response) => return error_response,
    };
    tool_text_response(
        id,
        &serde_json::json!({
            "items": response.items.iter().map(|i| serde_json::json!({
                "backlog_item_id": i.backlog_item_id,
                "business_node_id": i.business_node_id,
                "state": i.state,
                "position": i.position,
                "title": i.title,
                "explanation": i.explanation,
                "route_to_intake": i.route_to_intake,
            })).collect::<Vec<_>>(),
            "unranked_items": response.unranked_items.iter().map(|u| serde_json::json!({
                "backlog_item_id": u.backlog_item_id,
                "business_node_id": u.business_node_id,
                "state": u.state,
                "title": u.title,
                "route_to_intake": u.route_to_intake,
                "reason": u.reason,
            })).collect::<Vec<_>>(),
            "resolved_hearth": response.resolved_hearth,
        }),
    )
}

/// The frozen fixed role per tool, re-checked manually so a schema bypass
/// cannot select a wider authority.
fn backlog_fixed_role(tool: &str, arguments: &Value) -> Result<String, String> {
    let supplied = backlog_required_str(arguments, "actor_role")?;
    let allowed: &[&str] = match tool {
        "backlog_shape_edit" => &["nick_shape", "organ_loop", "orchestrator", "track_driver"],
        "backlog_recompute_rank" => &["organ_loop"],
        "backlog_stamp_execution_binding" => &["track_driver"],
        "backlog_propose_reshuffle" => &["orchestrator"],
        _ => &["nick_shape"],
    };
    if !allowed.contains(&supplied.as_str()) {
        return Err(format!(
            "`{supplied}` is not an admissible actor_role for `{tool}`"
        ));
    }
    Ok(supplied)
}

fn backlog_operation_from_json(
    tool: &str,
    arguments: &Value,
) -> Result<backlog_mutate_request::Operation, String> {
    let identity: Vec<&str> = vec![
        "actor_name",
        "actor_role",
        "actor_type",
        "actor_model",
        "actor_provider",
        "actor_context_window",
        "actor_sdk_version",
        "actor_entrypoint",
        "hearth",
        "project",
    ];
    let mut allowed = identity.clone();
    match tool {
        "backlog_shape_edit" => allowed.extend(["backlog_item_id", "changes"]),
        "backlog_recompute_rank" => allowed.push("business_node_id"),
        "backlog_stamp_execution_binding" => {
            allowed.extend(["backlog_item_id", "execution_binding", "outcome_binding"])
        }
        "backlog_record_outcome_signoff" | "backlog_veto_age_out" | "backlog_lift_age_out_veto" => {
            allowed.extend(["backlog_item_id", "approver"])
        }
        "backlog_propose_reshuffle" => allowed.push("proposed"),
        "backlog_commit_reshuffle" | "backlog_reject_reshuffle" => {
            allowed.extend(["proposal_id", "approver"])
        }
        other => return Err(format!("`{other}` is not a K8 mutation tool")),
    }
    if let Some(reason) = backlog_reject_unknown_keys(arguments, &allowed) {
        return Err(reason);
    }
    for key in BACKLOG_IDENTITY_REQUIRED {
        backlog_required_str(arguments, key)?;
    }
    Ok(match tool {
        "backlog_shape_edit" => backlog_mutate_request::Operation::ShapeEdit(BacklogShapeEdit {
            backlog_item_id: backlog_required_str(arguments, "backlog_item_id")?,
            ..backlog_shape_edit_from_json(&arguments["changes"])?
        }),
        "backlog_recompute_rank" => {
            backlog_mutate_request::Operation::RecomputeRank(BacklogRecomputeRank {
                business_node_id: backlog_required_str(arguments, "business_node_id")?,
            })
        }
        "backlog_stamp_execution_binding" => {
            let eb = &arguments["execution_binding"];
            let ob = &arguments["outcome_binding"];
            backlog_mutate_request::Operation::StampExecutionBinding(BacklogStampExecutionBinding {
                backlog_item_id: backlog_required_str(arguments, "backlog_item_id")?,
                execution_binding: Some(BacklogExecutionBinding {
                    track_id: backlog_required_str(eb, "track_id")?,
                    playbook_definition_id: backlog_required_str(eb, "playbook_definition_id")?,
                    playbook_run_id: backlog_required_str(eb, "playbook_run_id")?,
                    run_id: backlog_required_str(eb, "run_id")?,
                }),
                outcome_binding: Some(BacklogOutcomeBindingDecl {
                    success_measure: Some(match ob["success_measure_id"].as_str() {
                        Some(sm) => backlog_outcome_binding_decl::SuccessMeasure::SuccessMeasureId(
                            sm.to_string(),
                        ),
                        None => backlog_outcome_binding_decl::SuccessMeasure::NoSuccessMeasureId(
                            BacklogPresence {},
                        ),
                    }),
                    tree_node: backlog_required_str(ob, "tree_node")?,
                    reading_status: backlog_required_str(ob, "reading_status")?,
                }),
            })
        }
        "backlog_record_outcome_signoff" => {
            backlog_mutate_request::Operation::RecordOutcomeSignoff(BacklogRecordOutcomeSignoff {
                backlog_item_id: backlog_required_str(arguments, "backlog_item_id")?,
                approver: backlog_required_str(arguments, "approver")?,
            })
        }
        "backlog_propose_reshuffle" => {
            let rows = arguments["proposed"]
                .as_array()
                .filter(|a| !a.is_empty())
                .ok_or_else(|| "`proposed` must be a nonempty array".to_string())?;
            let mut proposed = Vec::new();
            for row in rows {
                let position = row["position"]
                    .as_u64()
                    .filter(|p| *p >= 1 && *p <= u32::MAX as u64)
                    .ok_or_else(|| "each proposed position must be 1..=u32::MAX".to_string())?;
                proposed.push(BacklogProposedPosition {
                    backlog_item_id: backlog_required_str(row, "backlog_item_id")?,
                    position: position as u32,
                });
            }
            backlog_mutate_request::Operation::ProposeReshuffle(BacklogProposeReshuffle { proposed })
        }
        "backlog_commit_reshuffle" => {
            backlog_mutate_request::Operation::CommitReshuffle(BacklogCommitReshuffle {
                proposal_id: backlog_required_str(arguments, "proposal_id")?,
                approver: backlog_required_str(arguments, "approver")?,
            })
        }
        "backlog_reject_reshuffle" => {
            backlog_mutate_request::Operation::RejectReshuffle(BacklogRejectReshuffle {
                proposal_id: backlog_required_str(arguments, "proposal_id")?,
                approver: backlog_required_str(arguments, "approver")?,
            })
        }
        "backlog_veto_age_out" => {
            backlog_mutate_request::Operation::VetoAgeOut(BacklogVetoAgeOut {
                backlog_item_id: backlog_required_str(arguments, "backlog_item_id")?,
                approver: backlog_required_str(arguments, "approver")?,
            })
        }
        _ => backlog_mutate_request::Operation::LiftAgeOutVeto(BacklogLiftAgeOutVeto {
            backlog_item_id: backlog_required_str(arguments, "backlog_item_id")?,
            approver: backlog_required_str(arguments, "approver")?,
        }),
    })
}

/// Decode the closed `changes` object. Omission means untouched; an explicit
/// null selects clear; an empty or unknown change rejects before gRPC.
fn backlog_shape_edit_from_json(changes: &Value) -> Result<BacklogShapeEdit, String> {
    let map = changes
        .as_object()
        .filter(|m| !m.is_empty())
        .ok_or_else(|| "`changes` must be a nonempty object".to_string())?;
    if let Some(reason) = backlog_reject_unknown_keys(
        changes,
        &[
            "action_class",
            "description",
            "effort_class",
            "playbook_binding",
            "rank_inputs",
            "wake_condition",
            "superseded_by",
        ],
    ) {
        return Err(reason);
    }
    let mut edit = BacklogShapeEdit::default();
    if let Some(v) = map.get("action_class") {
        let token = v
            .as_str()
            .ok_or_else(|| "`action_class` must be a string".to_string())?;
        edit.action_class_edit = Some(backlog_shape_edit::ActionClassEdit::SetActionClass(
            token.to_string(),
        ));
    }
    if let Some(v) = map.get("description") {
        edit.description_edit = Some(match v.as_str() {
            Some(s) => backlog_shape_edit::DescriptionEdit::SetDescription(s.to_string()),
            None if v.is_null() => {
                backlog_shape_edit::DescriptionEdit::ClearDescription(BacklogPresence {})
            }
            None => return Err("`description` must be a string or null".to_string()),
        });
    }
    if let Some(v) = map.get("effort_class") {
        edit.effort_class_edit = Some(match v.as_str() {
            Some(s) => backlog_shape_edit::EffortClassEdit::SetEffortClass(s.to_string()),
            None if v.is_null() => {
                backlog_shape_edit::EffortClassEdit::ClearEffortClass(BacklogPresence {})
            }
            None => return Err("`effort_class` must be a string or null".to_string()),
        });
    }
    if let Some(v) = map.get("playbook_binding") {
        edit.playbook_binding_edit = Some(if v.is_null() {
            backlog_shape_edit::PlaybookBindingEdit::ClearPlaybookBinding(BacklogPresence {})
        } else {
            let definition = match v["playbook_definition_id"].as_str() {
                Some(id) => backlog_playbook_binding::Definition::PlaybookDefinitionId(id.to_string()),
                None => backlog_playbook_binding::Definition::NoPlaybookDefinition(BacklogPresence {}),
            };
            backlog_shape_edit::PlaybookBindingEdit::SetPlaybookBinding(BacklogPlaybookBinding {
                definition: Some(definition),
                route_to_intake: v["route_to_intake"]
                    .as_bool()
                    .ok_or_else(|| "`playbook_binding.route_to_intake` is required".to_string())?,
            })
        });
    }
    if let Some(v) = map.get("rank_inputs") {
        let inputs = v
            .as_object()
            .filter(|m| !m.is_empty())
            .ok_or_else(|| "`rank_inputs` must be a nonempty object".to_string())?;
        if let Some(reason) = backlog_reject_unknown_keys(
            v,
            &["value_gap_magnitude", "nick_weight", "dependency_readiness"],
        ) {
            return Err(reason);
        }
        let mut rank = BacklogRankInputsEdit::default();
        if let Some(g) = inputs.get("value_gap_magnitude") {
            rank.value_gap_magnitude_edit = Some(if g.is_null() {
                backlog_rank_inputs_edit::ValueGapMagnitudeEdit::ClearValueGapMagnitude(
                    BacklogPresence {},
                )
            } else {
                backlog_rank_inputs_edit::ValueGapMagnitudeEdit::SetValueGapMagnitude(
                    BacklogValueGapMagnitude {
                        reference: Some(backlog_evidence_from_json(
                            &g["ref"],
                            "value_gap_magnitude.ref",
                        )?),
                        magnitude: g["magnitude"]
                            .as_f64()
                            .ok_or_else(|| "`value_gap_magnitude.magnitude` is required".to_string())?,
                    },
                )
            });
        }
        if let Some(w) = inputs.get("nick_weight") {
            rank.nick_weight_edit = Some(match w.as_f64() {
                Some(v) => backlog_rank_inputs_edit::NickWeightEdit::SetNickWeight(v),
                None if w.is_null() => {
                    backlog_rank_inputs_edit::NickWeightEdit::ClearNickWeight(BacklogPresence {})
                }
                None => return Err("`nick_weight` must be a number or null".to_string()),
            });
        }
        if let Some(d) = inputs.get("dependency_readiness") {
            rank.dependency_readiness_edit = Some(if d.is_null() {
                backlog_rank_inputs_edit::DependencyReadinessEdit::ClearDependencyReadiness(
                    BacklogPresence {},
                )
            } else {
                let mut blocker_refs = Vec::new();
                if let Some(list) = d["blocker_refs"].as_array() {
                    for r in list {
                        blocker_refs.push(backlog_evidence_from_json(r, "blocker_ref")?);
                    }
                }
                backlog_rank_inputs_edit::DependencyReadinessEdit::SetDependencyReadiness(
                    BacklogDependencyReadiness {
                        status: backlog_required_str(d, "status")?,
                        blocker_refs,
                    },
                )
            });
        }
        edit.rank_inputs = Some(rank);
    }
    if let Some(v) = map.get("wake_condition") {
        edit.wake_condition_edit = Some(if v.is_null() {
            backlog_shape_edit::WakeConditionEdit::ClearWakeCondition(BacklogPresence {})
        } else {
            let kind = backlog_required_str(v, "kind")?;
            let reference = match v.get("ref") {
                Some(r) if !r.is_null() => Some(backlog_evidence_from_json(r, "wake_condition.ref")?),
                _ => None,
            };
            backlog_shape_edit::WakeConditionEdit::SetWakeCondition(BacklogWakeCondition {
                kind,
                reference,
                predicate: backlog_required_str(v, "predicate")?,
            })
        });
    }
    if let Some(v) = map.get("superseded_by") {
        edit.superseded_by_edit = Some(match v.as_str() {
            Some(s) => backlog_shape_edit::SupersededByEdit::SetSupersededBy(s.to_string()),
            None if v.is_null() => {
                backlog_shape_edit::SupersededByEdit::ClearSupersededBy(BacklogPresence {})
            }
            None => return Err("`superseded_by` must be a string or null".to_string()),
        });
    }
    Ok(edit)
}
