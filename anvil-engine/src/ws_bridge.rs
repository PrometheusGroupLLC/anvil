//! HTTP bridge multiplexed onto the engine's single fixed gRPC port.
//!
//! The anvil-kit React frontend talks to the engine over a loopback WebSocket
//! using JSON-RPC 2.0, and Foundry health-probes the engine over plain HTTP.
//! Both are HTTP/1.1; the existing tonic gRPC service is HTTP/2 (h2c,
//! prior-knowledge). One `TcpListener` serves all three: an `axum::Router`
//! carries `/ws` and `/health`, and the tonic service is merged in as the
//! fallback. axum's `serve` drives connections through hyper-util's auto
//! connection builder, which sniffs each accepted connection and dispatches
//! HTTP/1.1 vs HTTP/2-prior-knowledge accordingly — so a gRPC client and a
//! browser WebSocket coexist on the same socket with no second listener.
//!
//! The `/ws` `playbook_activity` method folds the SAME core query path as the
//! gRPC `playbook_activity` RPC (`crate::compute_playbook_activity`), so the two
//! surfaces can never return divergent data.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use futures_util::StreamExt;
use http::StatusCode;
use serde_json::{json, Value};

use super::HearthPolicy;

/// The read-only context the HTTP bridge needs to resolve a hearth and run the
/// shared playbook-activity fold. Mirrors how the gRPC read RPCs treat the
/// principal: catalog/playbook_activity are reads, and in standalone mode the
/// gRPC gatekeeper is a no-op — so the bridge applies no extra auth. (Under
/// Foundry the engine's frontend reaches it over loopback within the same
/// supervised session; read queries stay read queries.)
#[derive(Clone)]
pub(crate) struct WsBridgeState {
    pub(crate) default_hearth: Option<PathBuf>,
    pub(crate) global_playbooks_hearth: Option<PathBuf>,
    pub(crate) hearth_policy: HearthPolicy,
}

/// Build the axum router carrying `/health` and `/ws`, sharing the read context.
pub(crate) fn router(state: WsBridgeState) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        // The Playbooks column Foundry PULLS at kit start. Read-only and
        // unauthenticated for the same reason `/health` is: the host probes it
        // from the same machine before any session exists, and a column that
        // needed a token would be a column only a person could fill — which is
        // the defect this route closes.
        .route("/panel/playbooks", get(panel_playbooks_handler))
        .route("/ws", get(ws_upgrade_handler))
        .with_state(Arc::new(state))
}

/// `GET /panel/playbooks` — the Playbooks column, projected for the host to PULL.
///
/// FOLDS THE SAME THREE READS the frontend's `registry.ts` folds, through the
/// SAME `compute_*` functions the gRPC RPCs and the `/ws` methods use, so this
/// surface cannot drift from the pane beside it: the atlas (does the definition
/// load), the live instances (is a run going right now) and the playbook
/// activity (has it ever been called).
///
/// A READ FAILURE ANSWERS 500, NOT AN EMPTY DOCUMENT. The host refuses a body it
/// cannot read and leaves the previous document alone; an empty list would be
/// written and the column would look answered.
async fn panel_playbooks_handler(State(state): State<Arc<WsBridgeState>>) -> Response {
    use anvil_engine::panel::{build_document, PlaybookFacts};

    // The kit's own hearth: "" resolves through the same policy every other
    // read uses. Never a path from the caller — this route takes no parameters.
    let atlas = match super::compute_playbook_atlas(
        "",
        state.default_hearth.as_deref(),
        &state.hearth_policy,
    ) {
        Ok(a) => a,
        Err(status) => return panel_error(status),
    };
    let live = match super::compute_live_instances(
        "",
        false,
        state.default_hearth.as_deref(),
        &state.hearth_policy,
        state.global_playbooks_hearth.as_deref(),
    ) {
        Ok(l) => l,
        Err(status) => return panel_error(status),
    };
    // ALL HEARTHS — `true`, and NOT the `false` its neighbour above uses.
    // `PlaybookRegistryView.tsx` reads live instances scoped to one hearth and
    // activity across EVERY hearth, and the asymmetry is load-bearing: a call
    // recorded against another hearth still means this playbook HAS run, and
    // scoping it away turns "ran, outcome unknown" into "never run". Measured:
    // with `false` here, 5 of anvil's 32 rows derived `hollow` that the shipped
    // frontend derives as having run.
    let activity = match super::compute_playbook_activity(
        "",
        true,
        state.default_hearth.as_deref(),
        &state.hearth_policy,
        state.global_playbooks_hearth.as_deref(),
    ) {
        Ok(a) => a,
        Err(status) => return panel_error(status),
    };

    let mut live_by_kind: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for i in &live.instances {
        *live_by_kind.entry(i.kind.clone()).or_insert(0) += 1;
    }
    let mut calls_by_kind: std::collections::HashMap<String, u64> =
        std::collections::HashMap::new();
    for owner in &activity.owners {
        for e in &owner.entries {
            *calls_by_kind.entry(e.kind.clone()).or_insert(0) += e.call_count as u64;
        }
    }

    let facts: Vec<PlaybookFacts> = atlas
        .entries
        .iter()
        .map(|e| PlaybookFacts {
            // `registry.ts` line 155: the artifact id when there is one, else the kind.
            id: if e.artifact_id.is_empty() {
                e.kind.clone()
            } else {
                e.artifact_id.clone()
            },
            kind: e.kind.clone(),
            owner_kit: e.owner_kit.clone(),
            loads: e.loads,
            live_runs: live_by_kind.get(&e.kind).copied().unwrap_or(0),
            calls: calls_by_kind.get(&e.kind).copied().unwrap_or(0),
        })
        .collect();

    (StatusCode::OK, axum::Json(build_document(&facts))).into_response()
}

fn panel_error(status: tonic::Status) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        axum::Json(json!({
            "error": format!("the playbooks could not be read: {}", status.message()),
        })),
    )
        .into_response()
}

/// `GET /health` — the endpoint Foundry's kit manifest probes
/// (`http://127.0.0.1:<port>/health`). A 200 with a tiny JSON body is enough.
async fn health_handler() -> Response {
    (StatusCode::OK, axum::Json(json!({ "status": "ok" }))).into_response()
}


/// `GET /ws` — upgrade the HTTP/1.1 connection to a WebSocket carrying JSON-RPC
/// 2.0 frames.
async fn ws_upgrade_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<WsBridgeState>>,
) -> Response {
    ws.on_upgrade(move |socket| ws_session(socket, state))
}

/// Drive one upgraded WebSocket: read JSON-RPC request frames, dispatch each,
/// and write the matching response frame. The loop ends when the peer closes
/// the socket or a transport error occurs.
async fn ws_session(mut socket: WebSocket, state: Arc<WsBridgeState>) {
    // Content-free session telemetry: bound the session with a wall clock and a
    // closed categorical close reason. Best-effort + opt-in gated (no `~/.anvil`
    // write unless telemetry is enabled), so it never touches the hot path.
    let started = Instant::now();
    let mut close_reason = "server_close";
    while let Some(incoming) = socket.next().await {
        let text = match incoming {
            Ok(Message::Text(text)) => text,
            Ok(Message::Binary(bytes)) => match String::from_utf8(bytes) {
                Ok(text) => text,
                Err(_) => continue,
            },
            Ok(Message::Close(_)) => {
                close_reason = "peer_close";
                break;
            }
            // Ping/Pong are handled by axum; ignore anything else.
            Ok(_) => continue,
            Err(_) => {
                close_reason = "transport_err";
                break;
            }
        };

        // Dispatch on tokio's BLOCKING pool via `spawn_blocking` — NEVER inline on
        // the async worker. Every JSON-RPC method here folds the whole hearth off
        // disk synchronously (registry rebuild + activity-log scan + owner
        // resolution), and this bridge is multiplexed onto the SAME listener/runtime
        // as Foundry's `/health` probe. Run inline, one heavy `all_hearths` fold
        // occupies a worker thread for its full (hearth-size-growing) duration; enough
        // concurrent dashboard folds saturate every worker so the `/health` accept
        // loop can't be scheduled → Foundry's watchdog sees 6 consecutive probe
        // failures → SIGTERM → crash-loop. Offloading keeps the async workers free for
        // the accept loop and the trivial `/health` handler regardless of how heavy or
        // concurrent the folds get.
        //
        // The `catch_unwind` stays INSIDE the blocking closure: a panicking query
        // handler must NOT drop the socket. A dropped socket makes the frontend's
        // ResilientTransport fall back to MockTransport, silently collapsing the
        // ENTIRE dashboard to sample data (a single bad query → "everything is fake").
        // Catching the panic returns an error frame for ONLY the offending request, so
        // the socket stays live and the rest of the dashboard keeps rendering real
        // data. (This is defense-in-depth; handlers should not panic.)
        let text_for_dispatch = text;
        let state_for_dispatch = Arc::clone(&state);
        let reply = match tokio::task::spawn_blocking(move || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                handle_jsonrpc(&text_for_dispatch, &state_for_dispatch)
            }))
            .unwrap_or_else(|_| {
                let id = serde_json::from_str::<Value>(&text_for_dispatch)
                    .ok()
                    .and_then(|v| v.get("id").cloned())
                    .unwrap_or(Value::Null);
                error_frame(id, -32603, "internal error handling request", "internal")
            })
        })
        .await
        {
            Ok(reply) => reply,
            // JoinError (the blocking task itself was cancelled or aborted): keep the
            // socket alive with the same internal-error envelope rather than dropping.
            Err(_) => error_frame(Value::Null, -32603, "internal error handling request", "internal"),
        };
        let payload = serde_json::to_string(&reply).unwrap_or_else(|_| {
            r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"serialize_failed","data":{"code":"internal"}}}"#.to_string()
        });
        if socket.send(Message::Text(payload)).await.is_err() {
            close_reason = "transport_err";
            break;
        }
    }
    anvil_engine::telemetry::record_ws_session(started.elapsed().as_millis() as f64, close_reason);
}

/// One structured seam record per bridge turn — the CQRS **read** seam.
///
/// The command line, the screens and the tool surface are peers of this engine.
/// The other two are already accountable in its log stream: every command turn
/// lands in the universal activity sink, and the query RPCs each emit their own
/// `command`/`outcome` record. A screen could ask this bridge for anything and
/// leave no trace of having asked, so anything it then rendered was state with
/// no explanation behind it — which is a defect in the seam, not a mystery
/// about the reader.
///
/// This record extends the idiom already in use (`tracing` event, flattened
/// fields, closed labels only) rather than inventing a second shape. It stays
/// OUT of the universal activity sink deliberately: that sink is one record per
/// COMMAND TURN and is folded into the usage numbers, and a dashboard that
/// re-polls every fifteen seconds would bury the measured signal under its own
/// reads.
///
/// **There is no acting person on this channel.** It is loopback, read-only,
/// and carries no principal — so `actor` is the closed label `unknown` rather
/// than an omitted field or the machine's own name. An omitted actor reads as
/// "nobody asked"; the honest statement is that this channel has no way to
/// know. Every field is a closed label or a method name: no paths, no message
/// text, no identity.
fn record_read_seam(command: &str, surface: &str, outcome: &str) {
    tracing::info!(
        seam = "ws_bridge",
        command = command,
        surface = surface,
        actor = "unknown",
        outcome = outcome,
        "ws bridge read seam"
    );
}

/// Parse one JSON-RPC 2.0 request frame and produce its response frame.
///
/// Wire contract the frontend already expects:
///   request:  { "jsonrpc": "2.0", "id": <n>, "method": <str>,
///               "params": { "surface": <str>, ... } }
///   success:  { "jsonrpc": "2.0", "id": <n>, "result": <any> }
///   error:    { "jsonrpc": "2.0", "id": <n>,
///               "error": { "code": <num>, "message": <str>,
///                          "data": { "code": <STRING> } } }
/// The UI keys on the STRING `error.data.code`.
///
/// `params.surface` is REQUIRED on every method and there is no default. A
/// default surface is exactly how an unattributable read comes to look
/// attributed, and the seam record is worth nothing if the name in it was
/// supplied by the thing writing the record.
fn handle_jsonrpc(raw: &str, state: &WsBridgeState) -> Value {
    let request: Value = match serde_json::from_str(raw) {
        Ok(value) => value,
        Err(_) => {
            return error_frame(Value::Null, -32700, "parse_error", "parse_error");
        }
    };

    // `id` echoes back on both success and error; default to null when absent.
    let id = request.get("id").cloned().unwrap_or(Value::Null);

    let method = match request.get("method").and_then(Value::as_str) {
        Some(method) => method,
        None => {
            record_read_seam("(none)", "(unnamed)", "refused_missing_method");
            return error_frame(id, -32600, "missing method", "invalid_request");
        }
    };

    // The surface is the whole point of the record. Refuse loudly rather than
    // serve a read nobody can be held to: a served-but-unattributed read is
    // indistinguishable, afterwards, from one that never happened.
    let surface = request
        .get("params")
        .and_then(|p| p.get("surface"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if surface.is_empty() {
        record_read_seam(method, "(unnamed)", "refused_surface_required");
        return error_frame(
            id,
            -32602,
            "every request over this bridge must name the surface making it",
            "surface_required",
        );
    }

    let reply = match method {
        "playbook_activity" => {
            let params = request.get("params");
            let hearth_path = params
                .and_then(|p| p.get("hearth_path"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let all_hearths = params
                .and_then(|p| p.get("all_hearths"))
                .and_then(Value::as_bool)
                .unwrap_or(false);

            match super::compute_playbook_activity(
                hearth_path,
                all_hearths,
                state.default_hearth.as_deref(),
                &state.hearth_policy,
                state.global_playbooks_hearth.as_deref(),
            ) {
                Ok(response) => success_frame(id, playbook_activity_to_json(&response)),
                Err(status) => {
                    // Map the gRPC Status to the JSON-RPC error envelope, keeping a
                    // stable STRING `data.code` the UI can key on.
                    let data_code = status_data_code(&status);
                    error_frame(id, -32000, status.message(), data_code)
                }
            }
        }
        "actor_activity" => {
            // LOCAL-ONLY query (raw actor names) — served over loopback /ws only.
            let params = request.get("params");
            let hearth_path = params
                .and_then(|p| p.get("hearth_path"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let all_hearths = params
                .and_then(|p| p.get("all_hearths"))
                .and_then(Value::as_bool)
                .unwrap_or(false);

            match super::compute_actor_activity(
                hearth_path,
                all_hearths,
                state.default_hearth.as_deref(),
                &state.hearth_policy,
            ) {
                Ok(response) => success_frame(id, actor_activity_to_json(&response)),
                Err(status) => {
                    let data_code = status_data_code(&status);
                    error_frame(id, -32000, status.message(), data_code)
                }
            }
        }
        "usage_timeseries" => {
            let params = request.get("params");
            let hearth_path = params
                .and_then(|p| p.get("hearth_path"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let granularity = params
                .and_then(|p| p.get("granularity"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let all_hearths = params
                .and_then(|p| p.get("all_hearths"))
                .and_then(Value::as_bool)
                .unwrap_or(false);

            match super::compute_usage_timeseries(
                hearth_path,
                granularity,
                all_hearths,
                state.default_hearth.as_deref(),
                &state.hearth_policy,
                state.global_playbooks_hearth.as_deref(),
            ) {
                Ok(response) => success_frame(id, usage_timeseries_to_json(&response)),
                Err(status) => {
                    let data_code = status_data_code(&status);
                    error_frame(id, -32000, status.message(), data_code)
                }
            }
        }
        "playbook_step_volume" => {
            let params = request.get("params");
            let hearth_path = params
                .and_then(|p| p.get("hearth_path"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let kind = params
                .and_then(|p| p.get("kind"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let all_hearths = params
                .and_then(|p| p.get("all_hearths"))
                .and_then(Value::as_bool)
                .unwrap_or(false);

            match super::compute_playbook_step_volume(
                hearth_path,
                kind,
                all_hearths,
                state.default_hearth.as_deref(),
                &state.hearth_policy,
                state.global_playbooks_hearth.as_deref(),
            ) {
                Ok(response) => success_frame(id, playbook_step_volume_to_json(&response)),
                Err(status) => {
                    let data_code = status_data_code(&status);
                    error_frame(id, -32000, status.message(), data_code)
                }
            }
        }
        "activity_summary" => {
            let params = request.get("params");
            let hearth_path = params
                .and_then(|p| p.get("hearth_path"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let granularity = params
                .and_then(|p| p.get("granularity"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let all_hearths = params
                .and_then(|p| p.get("all_hearths"))
                .and_then(Value::as_bool)
                .unwrap_or(false);

            match super::compute_activity_summary(
                hearth_path,
                granularity,
                all_hearths,
                state.default_hearth.as_deref(),
                &state.hearth_policy,
            ) {
                Ok(response) => success_frame(id, activity_summary_to_json(&response)),
                Err(status) => {
                    let data_code = status_data_code(&status);
                    error_frame(id, -32000, status.message(), data_code)
                }
            }
        }
        "playbook_fidelity" => {
            // Layer-3 FIDELITY read: per-kind completion, dangling instances,
            // revision cycles, review authenticity. Folds the SAME path the gRPC
            // PlaybookFidelity RPC uses (`compute_playbook_fidelity`), so the WS
            // panel and the RPC can never diverge. Mirrors activity_summary:
            // hearth_path + all_hearths, no granularity.
            let params = request.get("params");
            let hearth_path = params
                .and_then(|p| p.get("hearth_path"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let all_hearths = params
                .and_then(|p| p.get("all_hearths"))
                .and_then(Value::as_bool)
                .unwrap_or(false);

            match super::compute_playbook_fidelity(
                hearth_path,
                all_hearths,
                state.default_hearth.as_deref(),
                &state.hearth_policy,
                state.global_playbooks_hearth.as_deref(),
            ) {
                Ok(response) => success_frame(id, playbook_fidelity_to_json(&response)),
                Err(status) => {
                    let data_code = status_data_code(&status);
                    error_frame(id, -32000, status.message(), data_code)
                }
            }
        }
        "hook_manifest" => {
            let hearth_path = request
                .get("params")
                .and_then(|p| p.get("hearth_path"))
                .and_then(Value::as_str)
                .unwrap_or("");

            match super::compute_hook_manifest(
                hearth_path,
                state.default_hearth.as_deref(),
                &state.hearth_policy,
                state.global_playbooks_hearth.as_deref(),
            ) {
                Ok(response) => success_frame(id, hook_manifest_to_json(&response)),
                Err(status) => {
                    let data_code = status_data_code(&status);
                    error_frame(id, -32000, status.message(), data_code)
                }
            }
        }
        "playbook_atlas" => {
            // Atlas measurement surface (list). Folds the SAME concrete-registry
            // path the gRPC PlaybookAtlas RPC uses (`compute_playbook_atlas`), so
            // the in-app Atlas and the RPC can never diverge.
            let hearth_path = request
                .get("params")
                .and_then(|p| p.get("hearth_path"))
                .and_then(Value::as_str)
                .unwrap_or("");

            match super::compute_playbook_atlas(
                hearth_path,
                state.default_hearth.as_deref(),
                &state.hearth_policy,
            ) {
                Ok(response) => success_frame(id, playbook_atlas_to_json(&response)),
                Err(status) => {
                    let data_code = status_data_code(&status);
                    error_frame(id, -32000, status.message(), data_code)
                }
            }
        }
        "playbook_atlas_detail" => {
            // Atlas per-kind detail (states + edges + rubric). Same fold path as
            // the gRPC PlaybookAtlasDetail RPC (`compute_playbook_atlas_detail`).
            let params = request.get("params");
            let hearth_path = params
                .and_then(|p| p.get("hearth_path"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let kind = params
                .and_then(|p| p.get("kind"))
                .and_then(Value::as_str)
                .unwrap_or("");

            match super::compute_playbook_atlas_detail(
                hearth_path,
                kind,
                state.default_hearth.as_deref(),
                &state.hearth_policy,
            ) {
                Ok(response) => success_frame(id, playbook_atlas_detail_to_json(&response)),
                Err(status) => {
                    let data_code = status_data_code(&status);
                    error_frame(id, -32000, status.message(), data_code)
                }
            }
        }
        "live_instances" => {
            // LOCAL-ONLY query (raw actor names from begin-markers) — served over
            // loopback /ws only. Folds the SAME core path the gRPC LiveInstances
            // RPC uses (`compute_live_instances`), so the Atlas live overlay and
            // the RPC can never diverge.
            let params = request.get("params");
            let hearth_path = params
                .and_then(|p| p.get("hearth_path"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let all_hearths = params
                .and_then(|p| p.get("all_hearths"))
                .and_then(Value::as_bool)
                .unwrap_or(false);

            match super::compute_live_instances(
                hearth_path,
                all_hearths,
                state.default_hearth.as_deref(),
                &state.hearth_policy,
                state.global_playbooks_hearth.as_deref(),
            ) {
                Ok(response) => success_frame(id, live_instances_to_json(&response)),
                Err(status) => {
                    let data_code = status_data_code(&status);
                    error_frame(id, -32000, status.message(), data_code)
                }
            }
        }
        "list_instance_artifacts" => {
            // Lets the Atlas live panel explore a live instance's on-disk
            // artifacts. Folds the SAME core path the gRPC ListInstanceArtifacts
            // RPC uses (`compute_list_instance_artifacts`), so the two surfaces
            // can never diverge. FAIL-CLOSED on path escape (instance_dir must
            // resolve under a permitted root) — see that function's doc.
            let instance_dir = request
                .get("params")
                .and_then(|p| p.get("instance_dir"))
                .and_then(Value::as_str)
                .unwrap_or("");

            match super::compute_list_instance_artifacts(instance_dir, &state.hearth_policy) {
                Ok(response) => success_frame(id, list_instance_artifacts_to_json(&response)),
                Err(status) => {
                    let data_code = status_data_code(&status);
                    error_frame(id, -32000, status.message(), data_code)
                }
            }
        }
        "read_instance_artifact" => {
            // Companion to list_instance_artifacts: reads ONE named file's
            // FULL content so the Atlas live-agent artifact explorer can
            // render (and poll) the real in-progress work, not just a ~500
            // char preview. Folds the SAME core path the gRPC
            // ReadInstanceArtifact RPC uses (`compute_read_instance_artifact`),
            // so the two surfaces can never diverge. FAIL-CLOSED on path
            // escape for BOTH instance_dir and name — see that function's doc.
            let instance_dir = request
                .get("params")
                .and_then(|p| p.get("instance_dir"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let name = request
                .get("params")
                .and_then(|p| p.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("");

            match super::compute_read_instance_artifact(instance_dir, name, &state.hearth_policy) {
                Ok(response) => success_frame(id, read_instance_artifact_to_json(&response)),
                Err(status) => {
                    let data_code = status_data_code(&status);
                    error_frame(id, -32000, status.message(), data_code)
                }
            }
        }
        "run_detail" => {
            // One run's own folded record + the runs started from inside it, for
            // the Playbooks depth panel. Folds the SAME core path the gRPC
            // RunDetail RPC uses (`compute_run_detail`), so the panel and the RPC
            // can never disagree about what happened in a run. The caller names
            // an INSTANCE ID, never a path — see that function's doc for the
            // permitted-root guard.
            let params = request.get("params");
            let hearth_path = params
                .and_then(|p| p.get("hearth_path"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let instance_id = params
                .and_then(|p| p.get("instance_id"))
                .and_then(Value::as_str)
                .unwrap_or("");

            match super::compute_run_detail(
                hearth_path,
                instance_id,
                state.default_hearth.as_deref(),
                &state.hearth_policy,
            ) {
                Ok(response) => success_frame(id, run_detail_to_json(&response)),
                Err(status) => {
                    let data_code = status_data_code(&status);
                    error_frame(id, -32000, status.message(), data_code)
                }
            }
        }
        "autonomy_evidence" => {
            // The case a playbook makes for a rung: how many of its runs were
            // clean out of how many were attempted, the human-touch delta, and
            // what it got wrong — which run, which week, who caught it, and what
            // they caught.
            //
            // Folds the SAME `compute_autonomy_evidence` a gRPC twin would, and
            // serializes the CORE record directly rather than hand-mapping it, so
            // the wire shape is the record's shape by construction. That is also
            // what makes the no-cost guard cover this surface: the scenario that
            // walks the serialized record for a cost-shaped key is walking the
            // exact bytes this method returns.
            //
            // `found: false` when the playbook has no runs at all. That is an
            // ANSWER, not a failure — a playbook nobody has run has no case, and
            // reporting `0 clean of 0 attempted` would read as a measured perfect
            // failure rather than as the absence of evidence.
            let params = request.get("params");
            let hearth_path = params
                .and_then(|p| p.get("hearth_path"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let kind = params
                .and_then(|p| p.get("kind"))
                .and_then(Value::as_str)
                .unwrap_or("");

            match super::compute_autonomy_evidence(
                hearth_path,
                kind,
                state.default_hearth.as_deref(),
                &state.hearth_policy,
                state.global_playbooks_hearth.as_deref(),
            ) {
                Ok((resolved_hearth, evidence)) => {
                    let payload = match &evidence {
                        Some(record) => serde_json::to_value(record).unwrap_or(Value::Null),
                        None => Value::Null,
                    };
                    success_frame(
                        id,
                        serde_json::json!({
                            "resolved_hearth": resolved_hearth,
                            "found": evidence.is_some(),
                            "evidence": payload,
                        }),
                    )
                }
                Err(status) => {
                    let data_code = status_data_code(&status);
                    error_frame(id, -32000, status.message(), data_code)
                }
            }
        }
        "join_coverage" => {
            // The conversation-scoped join, at the surface a person reaches. A
            // number computable only by a gRPC client is computable in a sense
            // that does not reach whoever has to read it.
            //
            // Folds the SAME path the gRPC JoinCoverage RPC uses
            // (`compute_join_coverage`), so the WS panel and the RPC can never
            // diverge — and that includes the multi-hearth resolution, which is
            // INHERITED whole rather than restated here: the union of
            // `hearth_path` and `hearth_paths`, empty entries skipped, every
            // member re-gated with the request failing CLOSED, dedup after
            // canonicalization, canonical-path ascending order. This arm's only
            // job is to turn JSON params into the request that fold already
            // takes. Re-reading any of those rules here would be a second
            // implementation of them, which is the divergence a scenario
            // asserts against by comparing the two surfaces byte for byte.
            let params = request.get("params");
            let text = |name: &str| {
                params
                    .and_then(|p| p.get(name))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string()
            };
            let hearth_paths: Vec<String> = params
                .and_then(|p| p.get("hearth_paths"))
                .and_then(Value::as_array)
                .map(|entries| {
                    entries
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            let join_request = anvil_engine::proto::JoinCoverageRequest {
                hearth_path: text("hearth_path"),
                hearth_paths,
                window_start: text("window_start"),
                window_end: text("window_end"),
                project_label: text("project_label"),
            };

            match super::compute_join_coverage(
                &join_request,
                state.default_hearth.as_deref(),
                &state.hearth_policy,
                state.global_playbooks_hearth.as_deref(),
            ) {
                Ok(response) => success_frame(id, join_coverage_to_json(&response)),
                Err(status) => {
                    let data_code = status_data_code(&status);
                    error_frame(id, -32000, status.message(), data_code)
                }
            }
        }
        other => error_frame(
            id,
            -32601,
            &format!("method not found: {}", other),
            "method_not_found",
        ),
    };

    // One record per served turn, after the outcome is known. The outcome is a
    // closed label read off the envelope this bridge is about to return, not a
    // second opinion computed beside it.
    let outcome = if reply.get("error").is_some() { "error" } else { "ok" };
    record_read_seam(method, surface, outcome);
    reply
}

/// Serialize the proto `PlaybookActivityResponse` into the exact JSON shape the
/// frontend's `PlaybookActivityResponse` type consumes (snake_case fields). The
/// proto messages do not derive serde, so the mapping is explicit here — this is
/// also where the wire contract is pinned: `call_count` (proto u64) is emitted
/// as a JSON NUMBER, never a string.
fn playbook_activity_to_json(response: &anvil_engine::proto::PlaybookActivityResponse) -> Value {
    let owners: Vec<Value> = response
        .owners
        .iter()
        .map(|group| {
            let entries: Vec<Value> = group
                .entries
                .iter()
                .map(|entry| {
                    json!({
                        "kind": entry.kind,
                        "owner": entry.owner,
                        "description": entry.description,
                        "call_count": entry.call_count,
                    })
                })
                .collect();
            json!({ "owner": group.owner, "entries": entries })
        })
        .collect();

    json!({
        "owners": owners,
        "resolved_hearth": response.resolved_hearth,
        "hearths_included": response.hearths_included,
    })
}

/// Serialize the proto `ActorActivityResponse` into the exact JSON shape the
/// LOCAL dashboard binds to (snake_case fields). Counts (`begin_count`) are
/// emitted as JSON NUMBERS. The proto messages do not derive serde, so the
/// mapping is explicit here. LOCAL-ONLY: this carries RAW actor names served
/// over the loopback /ws bridge only — never telemetry.
fn actor_activity_to_json(response: &anvil_engine::proto::ActorActivityResponse) -> Value {
    let actors: Vec<Value> = response
        .actors
        .iter()
        .map(|entry| {
            json!({
                "actor": entry.actor,
                "begin_count": entry.begin_count,
                "last_active": entry.last_active,
                "artifact_kinds": entry.artifact_kinds,
            })
        })
        .collect();

    json!({
        "resolved_hearth": response.resolved_hearth,
        "hearths_included": response.hearths_included,
        "actors": actors,
    })
}

/// Serialize the proto `UsageTimeSeriesResponse` into the JSON shape the frontend
/// consumes (snake_case fields). Counts (`total_calls`, `begin_count`,
/// `complete_count`, `call_count`) are emitted as JSON NUMBERS, never strings.
/// Identical data to the gRPC `UsageTimeSeries` RPC (same
/// `compute_usage_timeseries` fold).
fn usage_timeseries_to_json(response: &anvil_engine::proto::UsageTimeSeriesResponse) -> Value {
    let buckets: Vec<Value> = response
        .buckets
        .iter()
        .map(|bucket| {
            let per_artifact_kind: Vec<Value> = bucket
                .per_artifact_kind
                .iter()
                .map(|w| {
                    json!({
                        "kind": w.kind,
                        "call_count": w.call_count,
                    })
                })
                .collect();
            json!({
                "period_start": bucket.period_start,
                "total_calls": bucket.total_calls,
                "distinct_actors": bucket.distinct_actors,
                "begin_count": bucket.begin_count,
                "complete_count": bucket.complete_count,
                "per_artifact_kind": per_artifact_kind,
            })
        })
        .collect();

    json!({
        "buckets": buckets,
        "resolved_hearth": response.resolved_hearth,
        "hearths_included": response.hearths_included,
    })
}

/// Serialize the proto `PlaybookStepVolumeResponse` into the JSON shape the
/// frontend consumes (snake_case fields). `call_count` is a JSON NUMBER.
/// Identical data to the gRPC `PlaybookStepVolume` RPC.
fn playbook_step_volume_to_json(
    response: &anvil_engine::proto::PlaybookStepVolumeResponse,
) -> Value {
    let steps: Vec<Value> = response
        .steps
        .iter()
        .map(|step| {
            json!({
                "from_state": step.from_state,
                "to_state": step.to_state,
                "role": step.role,
                "call_count": step.call_count,
            })
        })
        .collect();

    json!({
        "resolved_hearth": response.resolved_hearth,
        "kind": response.kind,
        "steps": steps,
        "hearths_included": response.hearths_included,
    })
}

/// Serialize the proto `HookManifestResponse` into the JSON shape the installer
/// (and Foundry) consumes (snake_case fields). `gate` is a string enum
/// ("hard" | "soft"). Identical data to the gRPC `HookManifest` RPC (same
/// `compute_hook_manifest` fold), so the two surfaces can never diverge.
fn hook_manifest_to_json(response: &anvil_engine::proto::HookManifestResponse) -> Value {
    let hooks: Vec<Value> = response
        .hooks
        .iter()
        .map(|hook| {
            json!({
                "artifact_kind": hook.artifact_kind,
                "state": hook.state,
                "role": hook.role,
                "body": hook.body,
                "gate": hook.gate,
            })
        })
        .collect();

    json!({
        "resolved_hearth": response.resolved_hearth,
        "gate_query": response.gate_query,
        "hooks": hooks,
    })
}

/// Serialize the proto `AtlasIntegrity` into the JSON shape the Atlas consumes.
/// All counts (`anchors_count`) are JSON NUMBERS; the two `*_gates`/`*_states`
/// lists are JSON arrays. An absent integrity (should never happen — every entry
/// carries one) degrades to a degenerate `loads:false` object.
fn atlas_integrity_to_json(integrity: Option<&anvil_engine::proto::AtlasIntegrity>) -> Value {
    match integrity {
        Some(i) => json!({
            "loads": i.loads,
            "rubber_stamp_gates": i.rubber_stamp_gates,
            "unmeasured_states": i.unmeasured_states,
            "anchors_count": i.anchors_count,
            "grader_declared": i.grader_declared,
        }),
        None => json!({
            "loads": false,
            "rubber_stamp_gates": [],
            "unmeasured_states": [],
            "anchors_count": 0,
            "grader_declared": false,
        }),
    }
}

/// Serialize the proto `PlaybookAtlasResponse` (list) into the JSON shape the
/// AtlasView consumes (snake_case fields). Counts (`state_count`, `edge_count`,
/// `anchors_count`) are JSON NUMBERS. Identical data to the gRPC `PlaybookAtlas`
/// RPC (same `compute_playbook_atlas` fold), so the two surfaces can never
/// diverge. Invalid entries carry `loads:false` + `error_code`/`error_message`.
fn playbook_atlas_to_json(response: &anvil_engine::proto::PlaybookAtlasResponse) -> Value {
    let entries: Vec<Value> = response
        .entries
        .iter()
        .map(|e| {
            json!({
                "kind": e.kind,
                "artifact_id": e.artifact_id,
                "owner_kit": e.owner_kit,
                "state": e.state,
                "register": e.register,
                "loads": e.loads,
                "error_code": e.error_code,
                "error_message": e.error_message,
                "integrity": atlas_integrity_to_json(e.integrity.as_ref()),
                "calibration": e.calibration,
                "state_count": e.state_count,
                "edge_count": e.edge_count,
            })
        })
        .collect();

    json!({
        "resolved_hearth": response.resolved_hearth,
        "entries": entries,
    })
}

/// Serialize the proto `PlaybookAtlasDetailResponse` (per-kind detail) into the
/// JSON shape the AtlasView flow/rubric panels consume. `weight`/`anchors_count`
/// are JSON NUMBERS; `required_satisfaction`/`lagging_signals` are JSON arrays;
/// `success_rubric` is `null` when the kind declares none. Identical data to the
/// gRPC `PlaybookAtlasDetail` RPC (same `compute_playbook_atlas_detail` fold).
fn playbook_atlas_detail_to_json(
    response: &anvil_engine::proto::PlaybookAtlasDetailResponse,
) -> Value {
    let states: Vec<Value> = response
        .states
        .iter()
        .map(|s| {
            json!({
                "name": s.name,
                "is_review_gate": s.is_review_gate,
                "is_terminal": s.is_terminal,
                "has_measurement": s.has_measurement,
                "has_hook": s.has_hook,
            })
        })
        .collect();
    let edges: Vec<Value> = response
        .edges
        .iter()
        .map(|e| {
            json!({
                "from": e.from,
                "to": e.to,
                "required_role": e.required_role,
                "required_satisfaction": e.required_satisfaction,
            })
        })
        .collect();
    let success_rubric = response.success_rubric.as_ref().map(|r| {
        let dimensions: Vec<Value> = r
            .dimensions
            .iter()
            .map(|d| {
                json!({
                    "dimension": d.dimension,
                    "weight": d.weight,
                    "evidence_class": d.evidence_class,
                })
            })
            .collect();
        json!({
            "dimensions": dimensions,
            "grader_declared": r.grader_declared,
            "anchors_count": r.anchors_count,
            "lagging_signals": r.lagging_signals,
        })
    });
    let step_quality: Vec<Value> = response
        .step_quality
        .iter()
        .map(|s| {
            json!({
                "from_state": s.from_state,
                "to_state": s.to_state,
                "role": s.role,
                "mean_quality": s.mean_quality,
                "sample_count": s.sample_count,
                // REGRESSION GUARD: these two were missing from the /ws wire
                // shape even though the gRPC RPC and the frontend's
                // AtlasStepQuality type both carry them — the frontend (which
                // talks to the engine over /ws, NOT gRPC, in every real
                // deployment) silently always fell back to the coherence
                // score, believing no artifact-quality measurement ever
                // existed. See playbook_atlas_rpc.feature.
                "artifact_quality": s.artifact_quality,
                "artifact_sample_count": s.artifact_sample_count,
            })
        })
        .collect();
    let recent_measurements: Vec<Value> = response
        .recent_measurements
        .iter()
        .map(|r| {
            json!({
                "instance_id": r.instance_id,
                "to_state": r.to_state,
                "role": r.role,
                "actor": r.actor,
                "quality_score": r.quality_score,
                "model": r.model,
                "artifact_quality": r.artifact_quality,
                "artifact_sample_count": r.artifact_sample_count,
            })
        })
        .collect();

    json!({
        "resolved_hearth": response.resolved_hearth,
        "kind": response.kind,
        "found": response.found,
        "states": states,
        "edges": edges,
        "success_rubric": success_rubric,
        "step_quality": step_quality,
        "overall_mean_quality": response.overall_mean_quality,
        "measured_instance_count": response.measured_instance_count,
        "recent_measurements": recent_measurements,
        "overall_artifact_quality": response.overall_artifact_quality,
        "artifact_measured_count": response.artifact_measured_count,
    })
}

/// Serialize the proto `LiveInstancesResponse` into the JSON shape the Atlas live
/// overlay consumes (snake_case fields). Identical data to the gRPC
/// `LiveInstances` RPC (same `compute_live_instances` fold), so the two surfaces
/// can never diverge. `actor`/`at` are the RAW begin-marker values (LOCAL-ONLY) —
/// never a salted hash — empty when the instance carries no begin-marker.
fn live_instances_to_json(response: &anvil_engine::proto::LiveInstancesResponse) -> Value {
    let instances: Vec<Value> = response
        .instances
        .iter()
        .map(|inst| {
            json!({
                "instance_id": inst.instance_id,
                "kind": inst.kind,
                "state": inst.state,
                "actor": inst.actor,
                "at": inst.at,
                "current_step": inst.current_step,
                "action_count": inst.action_count,
                "artifact_dir": inst.artifact_dir,
            })
        })
        .collect();

    json!({
        "resolved_hearth": response.resolved_hearth,
        "hearths_included": response.hearths_included,
        "instances": instances,
        "idle_count": response.idle_count,
    })
}

/// Serialize the proto `ActivitySummaryResponse` into the JSON shape the
/// frontend consumes (snake_case fields). All counts (`total_turns`, `count`,
/// `total_turns`/`distinct_actors` per bucket) are JSON NUMBERS, never strings.
/// Identical data to the gRPC `ActivitySummary` RPC (same `compute_activity_summary`
/// fold), so the two surfaces can never diverge.
fn activity_summary_to_json(response: &anvil_engine::proto::ActivitySummaryResponse) -> Value {
    let label_counts = |list: &[anvil_engine::proto::ActivityLabelCount]| -> Vec<Value> {
        list.iter()
            .map(|c| json!({ "label": c.label, "count": c.count }))
            .collect()
    };
    let buckets: Vec<Value> = response
        .buckets
        .iter()
        .map(|b| {
            json!({
                "period_start": b.period_start,
                "total_turns": b.total_turns,
                "distinct_actors": b.distinct_actors,
            })
        })
        .collect();

    json!({
        "resolved_hearth": response.resolved_hearth,
        "hearths_included": response.hearths_included,
        "total_turns": response.total_turns,
        "by_command": label_counts(&response.by_command),
        "by_route_outcome": label_counts(&response.by_route_outcome),
        "by_source": label_counts(&response.by_source),
        "by_artifact_kind": label_counts(&response.by_artifact_kind),
        "buckets": buckets,
    })
}

/// Serialize the proto `PlaybookFidelityResponse` into the JSON shape the
/// dashboard's fidelity panel consumes (snake_case fields). All counts
/// (`begun`/`terminal`, `dangling_instances`, `count`, `*_exits`,
/// `review_elapsed_seconds`) are JSON NUMBERS, never strings; `completion_rate`
/// is a JSON number (proto double). Identical data to the gRPC `PlaybookFidelity`
/// RPC (same `compute_playbook_fidelity` fold), so the two surfaces can never
/// diverge.
fn playbook_fidelity_to_json(response: &anvil_engine::proto::PlaybookFidelityResponse) -> Value {
    let label_counts = |list: &[anvil_engine::proto::ActivityLabelCount]| -> Vec<Value> {
        list.iter()
            .map(|c| json!({ "label": c.label, "count": c.count }))
            .collect()
    };
    let completion: Vec<Value> = response
        .completion
        .iter()
        .map(|c| {
            json!({
                "kind": c.kind,
                "begun": c.begun,
                "terminal": c.terminal,
                "completion_rate": c.completion_rate,
            })
        })
        .collect();
    let instances: Vec<Value> = response
        .instances
        .iter()
        .map(|i| {
            json!({
                "instance_id": i.instance_id,
                "kind": i.kind,
                "folded_state": i.folded_state,
                "begun": i.begun,
                "transition_count": i.transition_count,
                "reached_terminal": i.reached_terminal,
                "dangling": i.dangling,
                "revision_cycles": i.revision_cycles,
            })
        })
        .collect();

    json!({
        "resolved_hearth": response.resolved_hearth,
        "hearths_included": response.hearths_included,
        "completion": completion,
        "instances": instances,
        "dangling_instances": response.dangling_instances,
        "dangling_by_kind": label_counts(&response.dangling_by_kind),
        "revision_cycles": label_counts(&response.revision_cycles),
        "revision_cycles_total": response.revision_cycles_total,
        "review_exits": response.review_exits,
        "delegated_exits": response.delegated_exits,
        "self_review_exits": response.self_review_exits,
        "review_elapsed_seconds": response.review_elapsed_seconds,
    })
}

/// Serialize the proto `JoinCoverageResponse` into the JSON shape a join panel
/// consumes (snake_case fields; every count a JSON NUMBER). Identical data to
/// the gRPC `JoinCoverage` RPC — same `compute_join_coverage` fold, same keys —
/// and a scenario compares the two payloads directly rather than trusting this
/// sentence.
///
/// A REPORT PER HEARTH AND NO POOLED TOTAL. There is no fleet-total key here
/// and none may be added: the two measured hearths sit at 32.9% and 64.4%
/// begin-side coverage under different salts, so a pooled number would be
/// dominated by whichever one is better instrumented and would still be
/// reported to two decimal places. The payload also carries no path, no salt
/// and no conversation id — the epochs are fingerprints, and a scenario asserts
/// every key here is on a declared allowlist.
fn join_coverage_to_json(response: &anvil_engine::proto::JoinCoverageResponse) -> Value {
    let per_hearth: Vec<Value> = response
        .per_hearth
        .iter()
        .map(|h| {
            let unjoin = h
                .unjoin
                .as_ref()
                .map(|c| {
                    json!({
                        "no_conversation_key": c.no_conversation_key,
                        "pre_migration_row": c.pre_migration_row,
                        "conversation_absent_from_begin_side": c.conversation_absent_from_begin_side,
                        "no_begin_of_kind_in_conversation": c.no_begin_of_kind_in_conversation,
                        "superseded_by_later_delivery_of_kind": c.superseded_by_later_delivery_of_kind,
                    })
                })
                .unwrap_or(Value::Null);
            let begin_unjoin = h
                .begin_unjoin
                .as_ref()
                .map(|c| {
                    json!({
                        "no_conversation_key": c.no_conversation_key,
                        "conversation_absent_from_delivery_side": c.conversation_absent_from_delivery_side,
                        "no_prior_delivery_of_kind": c.no_prior_delivery_of_kind,
                        "no_unconsumed_prior_delivery_of_kind": c.no_unconsumed_prior_delivery_of_kind,
                    })
                })
                .unwrap_or(Value::Null);
            let terminal = h
                .terminal
                .as_ref()
                .map(|c| {
                    json!({
                        "not_joined": c.not_joined,
                        "not_yet_terminal": c.not_yet_terminal,
                        "reached_terminal": c.reached_terminal,
                        "unknown_run_state": c.unknown_run_state,
                    })
                })
                .unwrap_or(Value::Null);
            json!({
                "hearth_label": h.hearth_label,
                "window_start": h.window_start,
                "window_end": h.window_end,
                "delivery_rows_read": h.delivery_rows_read,
                "read_defects": h.read_defects,
                "activity_rows_scanned": h.activity_rows_scanned,
                "activity_rows_retained": h.activity_rows_retained,
                "begin_rows_read": h.begin_rows_read,
                "episode_denominator": h.episode_denominator,
                "begin_denominator": h.begin_denominator,
                "menu_delivered": h.menu_delivered,
                "nothing_delivered": h.nothing_delivered,
                "no_engine_answer": h.no_engine_answer,
                "joined": h.joined,
                "unjoin": unjoin,
                "begin_unjoin": begin_unjoin,
                "terminal": terminal,
                "key_epoch": h.key_epoch,
                "hearth_salt_file_epoch": h.hearth_salt_file_epoch,
                "key_epoch_reconciliation": h.key_epoch_reconciliation,
            })
        })
        .collect();

    json!({
        "per_hearth": per_hearth,
        "filter_version": response.filter_version,
    })
}

/// Serialize the proto `ListInstanceArtifactsResponse` into the JSON shape the
/// Atlas live-instance artifact explorer consumes (snake_case fields).
/// `size_bytes` is a JSON NUMBER. Identical data to the gRPC
/// `ListInstanceArtifacts` RPC (same `compute_list_instance_artifacts` fold).
fn list_instance_artifacts_to_json(
    response: &anvil_engine::proto::ListInstanceArtifactsResponse,
) -> Value {
    let artifacts: Vec<Value> = response
        .artifacts
        .iter()
        .map(|a| {
            json!({
                "name": a.name,
                "size_bytes": a.size_bytes,
                "preview": a.preview,
                "modified_at": a.modified_at,
            })
        })
        .collect();

    json!({
        "instance_dir": response.instance_dir,
        "artifacts": artifacts,
        "error_message": response.error_message,
    })
}

/// Serialize the proto `ReadInstanceArtifactResponse` into the JSON shape the
/// Atlas live-agent artifact explorer consumes (snake_case fields). Every
/// field is carried explicitly — a prior bug in this file's sibling
/// serializers was a field silently missing from the /ws shape while the gRPC
/// response carried it; this function is the guarded seam, and
/// `read_instance_artifact_rpc.feature` pins the full field set over /ws.
/// `size_bytes` is a JSON NUMBER. Identical data to the gRPC
/// `ReadInstanceArtifact` RPC (same `compute_read_instance_artifact` fold).
fn read_instance_artifact_to_json(
    response: &anvil_engine::proto::ReadInstanceArtifactResponse,
) -> Value {
    json!({
        "name": response.name,
        "content": response.content,
        "size_bytes": response.size_bytes,
        "modified_at": response.modified_at,
        "truncated": response.truncated,
        "error_message": response.error_message,
    })
}

/// Serialize the proto `RunDetailResponse` into the JSON shape the Playbooks
/// depth panel consumes (snake_case, `depth` a JSON NUMBER).
///
/// NOTE WHAT IS NOT HERE: there is no `cost` key, on the node or on the step.
/// Anvil records no cost anywhere, so a key here could only carry a fabricated
/// zero — which a panel would render as a step that was free. Absent fields on a
/// step (an approver nobody needed, a note nobody wrote) serialize as EMPTY
/// STRINGS and never as a placeholder value.
fn run_detail_to_json(response: &anvil_engine::proto::RunDetailResponse) -> Value {
    let nodes: Vec<Value> = response
        .nodes
        .iter()
        .map(|node| {
            let steps: Vec<Value> = node
                .steps
                .iter()
                .map(|step| {
                    json!({
                        "to_state": step.to_state,
                        "at": step.at,
                        "actor": step.actor,
                        "role": step.role,
                        "approver": step.approver,
                        "note": step.note,
                        "verdict": step.verdict,
                    })
                })
                .collect();
            let actors: Vec<Value> = node
                .actors
                .iter()
                .map(|actor| {
                    json!({
                        "name": actor.name,
                        "actor_type": actor.actor_type,
                        "model": actor.model,
                        "provider": actor.provider,
                    })
                })
                .collect();
            json!({
                "instance_id": node.instance_id,
                "kind": node.kind,
                "state": node.state,
                "artifact_dir": node.artifact_dir,
                "parent_instance_id": node.parent_instance_id,
                "depth": node.depth,
                "steps": steps,
                "actors": actors,
            })
        })
        .collect();

    json!({
        "resolved_hearth": response.resolved_hearth,
        "found": response.found,
        "nodes": nodes,
        "error_message": response.error_message,
    })
}

/// Stable string code the UI keys on, derived from the gRPC status code.
fn status_data_code(status: &tonic::Status) -> &'static str {
    use tonic::Code;
    match status.code() {
        Code::InvalidArgument => "invalid_argument",
        Code::PermissionDenied => "permission_denied",
        Code::Unauthenticated => "not_authenticated",
        Code::NotFound => "not_found",
        _ => "internal",
    }
}

fn success_frame(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_frame(id: Value, code: i64, message: &str, data_code: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message,
            "data": { "code": data_code }
        }
    })
}
