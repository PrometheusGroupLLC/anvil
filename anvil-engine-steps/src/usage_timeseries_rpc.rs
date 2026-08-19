//! Step module for `usage_timeseries_rpc.feature` (engine seam).
//!
//! Seeds a real engine hearth's durable sinks directly (routing-activity.jsonl /
//! step-measurement.jsonl) so the `at` timestamps are deterministic, reuses the
//! shared `engine` module's "the engine is started with that hearth" step, then
//! drives BOTH the gRPC `UsageTimeSeries` / `PlaybookStepVolume` RPCs and the
//! `/ws` JSON-RPC methods, asserting identical folded data. The /ws assertions
//! reuse the WS roundtrip helper pattern from the `ws_bridge` module.

use anvil_test_support::engine::EngineProcess;
use anvil_core_hearth::fs_activity_log_adapter::FileSystemActivityLogAdapter;
use anvil_core_hearth::fs_step_measurement_adapter::FileSystemStepMeasurementAdapter;
use anvil_core::ports::activity_log_port::{ActivityLogRecord, ActivityLogWritePort};
use anvil_core::ports::step_measurement_port::{
    StepMeasurementRecord, StepMeasurementWritePort, STEP_MEASUREMENT_KIND,
};
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{async_step_def, check_def, step_def, StepDef};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

use anvil_test_support::{retained_temp_dir, RetainedTempDir};

const TS_RESULT_KEY: &str = "uts_rpc_ts_result";
const SV_RESULT_KEY: &str = "uts_rpc_sv_result";
const WS_RESPONSE_KEY: &str = "uts_rpc_ws_response";
// Cross-hearth: a second permitted hearth seeded alongside the default.
const HEARTH_B_KEY: &str = "uts_rpc_hearth_b";
const HEARTH_B_HANDLE_KEY: &str = "uts_rpc_hearth_b_handle";

fn column_index(table: &DataTable, name: &str) -> Result<usize, String> {
    table
        .headers
        .iter()
        .position(|h| h == name)
        .ok_or_else(|| format!("Missing '{}' column in data table", name))
}

fn new_engine_hearth() -> Result<(RetainedTempDir, std::path::PathBuf), String> {
    let (handle, tmp) = retained_temp_dir("anvil-usage-rpc-")?;
    // Engine hearth predicate needs tracks/ + tracks.md.
    std::fs::create_dir_all(tmp.join("tracks")).map_err(|e| format!("create tracks: {}", e))?;
    std::fs::write(tmp.join("tracks.md"), "# Tracks\n")
        .map_err(|e| format!("write tracks.md: {}", e))?;
    Ok((handle, tmp))
}

/// Seed one hearth's UNIVERSAL ACTIVITY LOG from a `kind | outcome | at |
/// actor_hash` table. Each row is one `route` turn carrying `artifact_kind = kind`
/// and the `actor_hash` — driving the activity-log fold's total_calls (all turns),
/// per_artifact_kind (by kind), and distinct_actors (by actor_hash). usage_timeseries
/// now folds the activity log (not the routing/step sinks).
fn seed_call_table(hearth: &std::path::Path, table: &DataTable) -> Result<(), String> {
    let kind_col = column_index(table, "kind")?;
    let outcome_col = column_index(table, "outcome")?;
    let at_col = column_index(table, "at")?;
    let hash_col = table.headers.iter().position(|h| h == "actor_hash");
    let log = FileSystemActivityLogAdapter::new(hearth);
    // Also seed a step measurement per row so PlaybookStepVolume (which still
    // folds the step-measurement sink) has data on a "with calls"-seeded hearth.
    let steps = FileSystemStepMeasurementAdapter::new(hearth);
    for row in &table.rows {
        let kind = row[kind_col].trim().to_string();
        let at = row[at_col].trim().to_string();
        let actor_hash = hash_col.and_then(|i| {
            let v = row[i].trim();
            if v.is_empty() || v == "-" {
                None
            } else {
                Some(v.to_string())
            }
        });
        log.append_activity_log(&ActivityLogRecord {
            command: "route".to_string(),
            outcome: row[outcome_col].trim().to_string(),
            artifact_kind: kind.clone(),
            from_state: String::new(),
            to_state: String::new(),
            actor_hash: actor_hash.clone(),
            at: at.clone(),
            source: String::new(),
            conversation_hash: None,
            project_label: None,
            playbook_run_id: None,
            call_state: None,
        })
        .map_err(|e| format!("seed activity log: {}", e))?;
        steps
            .append_step_measurement(&StepMeasurementRecord {
                kind: STEP_MEASUREMENT_KIND.to_string(),
                from_state: String::new(),
                to_state: "in_progress".to_string(),
                role: "doer".to_string(),
                intent_present: false,
                expected_output_present: false,
                at,
                artifact_kind: kind,
                actor_hash,
                conversation_hash: None,
                project_label: None,
                playbook_run_id: None,
                evidence: None,
            })
            .map_err(|e| format!("seed step: {}", e))?;
    }
    Ok(())
}

async fn ws_roundtrip(port: u16, request: &Value) -> Result<Value, String> {
    let url = format!("ws://127.0.0.1:{}/ws", port);
    let (mut socket, _resp) = tokio_tungstenite::connect_async(&url)
        .await
        .map_err(|e| format!("ws connect to {} failed: {}", url, e))?;
    socket
        .send(Message::Text(request.to_string()))
        .await
        .map_err(|e| format!("ws send failed: {}", e))?;
    while let Some(frame) = socket.next().await {
        match frame.map_err(|e| format!("ws recv failed: {}", e))? {
            Message::Text(text) => {
                return serde_json::from_str(&text)
                    .map_err(|e| format!("ws reply not JSON: {} (raw: {})", e, text));
            }
            Message::Close(_) => return Err("ws closed before a reply frame".to_string()),
            _ => continue,
        }
    }
    Err("ws stream ended before a reply frame".to_string())
}

fn ws_result(ctx: &Context) -> Result<Value, String> {
    let response = ctx
        .get::<Value>(WS_RESPONSE_KEY)
        .ok_or("No uts_rpc_ws_response in context")?;
    if let Some(error) = response.get("error") {
        return Err(format!("Expected JSON-RPC result, got error: {}", error));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| format!("JSON-RPC response has no result: {}", response))
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ===== Hearth seeding =====
        step_def(
            "a usage query engine hearth seeded with routing activity:",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let kind_col = column_index(table, "kind")?;
                let outcome_col = column_index(table, "outcome")?;
                let at_col = column_index(table, "at")?;
                let (handle, tmp) = new_engine_hearth()?;
                // usage_timeseries now folds the universal activity log: seed each
                // row as a `route` turn carrying its artifact_kind.
                let log = FileSystemActivityLogAdapter::new(&tmp);
                for row in &table.rows {
                    log.append_activity_log(&ActivityLogRecord {
                        command: "route".to_string(),
                        outcome: row[outcome_col].trim().to_string(),
                        artifact_kind: row[kind_col].trim().to_string(),
                        from_state: String::new(),
                        to_state: String::new(),
                        actor_hash: None,
                        at: row[at_col].trim().to_string(),
                        source: String::new(),
                        conversation_hash: None,
                        project_label: None,
                        playbook_run_id: None,
                        call_state: None,
                    })
                    .map_err(|e| format!("seed activity log: {}", e))?;
                }
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set::<RetainedTempDir>("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a usage query engine hearth seeded with activity log turns:",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                // Seed arbitrary command turns (begin/complete/catalog/...) with an
                // optional artifact_kind — proves usage_timeseries counts ALL turns
                // (total_calls) while per_artifact_kind folds only kinded turns.
                let table = params.data_table().ok_or("Expected data table")?;
                let command_col = column_index(table, "command")?;
                let kind_col = column_index(table, "artifact_kind")?;
                let at_col = column_index(table, "at")?;
                let (handle, tmp) = new_engine_hearth()?;
                let log = FileSystemActivityLogAdapter::new(&tmp);
                for row in &table.rows {
                    log.append_activity_log(&ActivityLogRecord {
                        command: row[command_col].trim().to_string(),
                        outcome: "ok".to_string(),
                        artifact_kind: row[kind_col].trim().to_string(),
                        from_state: String::new(),
                        to_state: String::new(),
                        actor_hash: None,
                        at: row[at_col].trim().to_string(),
                        source: String::new(),
                        conversation_hash: None,
                        project_label: None,
                        playbook_run_id: None,
                        call_state: None,
                    })
                    .map_err(|e| format!("seed activity log: {}", e))?;
                }
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set::<RetainedTempDir>("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a usage query engine hearth seeded with step measurements:",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let kind_col = column_index(table, "kind")?;
                let from_col = column_index(table, "from_state")?;
                let to_col = column_index(table, "to_state")?;
                let role_col = column_index(table, "role")?;
                let at_col = column_index(table, "at")?;
                let hash_col = table.headers.iter().position(|h| h == "actor_hash");
                let (handle, tmp) = new_engine_hearth()?;
                let adapter = FileSystemStepMeasurementAdapter::new(&tmp);
                for row in &table.rows {
                    // Seed with the PLAYBOOK kind in `artifact_kind` (the query
                    // filters on it post-fix); `kind` carries the redaction
                    // constant, the real-world write shape.
                    let actor_hash = hash_col.and_then(|i| {
                        let v = row[i].trim();
                        if v.is_empty() || v == "-" {
                            None
                        } else {
                            Some(v.to_string())
                        }
                    });
                    adapter
                        .append_step_measurement(&StepMeasurementRecord {
                            kind: STEP_MEASUREMENT_KIND.to_string(),
                            from_state: row[from_col].trim().to_string(),
                            to_state: row[to_col].trim().to_string(),
                            role: row[role_col].trim().to_string(),
                            intent_present: false,
                            expected_output_present: false,
                            at: row[at_col].trim().to_string(),
                            artifact_kind: row[kind_col].trim().to_string(),
                            actor_hash,
                            conversation_hash: None,
                            project_label: None,
                            playbook_run_id: None,
                            evidence: None,
                        })
                        .map_err(|e| format!("seed step measurement: {}", e))?;
                }
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set::<RetainedTempDir>("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        // ===== Cross-hearth seeding (two permitted hearths) =====
        // Each row of a `kind | outcome | at | actor_hash` table is BOTH a
        // routing-activity record and a step-measurement record carrying the
        // actor_hash at the same `at` — modeling one actor's call. The primary
        // hearth lands at `hearth_path` (the engine's --hearth default); the
        // secondary at `uts_rpc_hearth_b` (a --permitted-root).
        step_def(
            "a usage query primary hearth seeded with calls:",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let (handle, tmp) = new_engine_hearth()?;
                seed_call_table(&tmp, table)?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set::<RetainedTempDir>("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a usage query secondary hearth seeded with calls:",
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                (HEARTH_B_KEY, "PathBuf"),
                (HEARTH_B_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let (handle, tmp) = new_engine_hearth()?;
                seed_call_table(&tmp, table)?;
                let mut out = Context::new();
                // Carry the primary hearth forward (brine retains only provides).
                if let Some(p) = ctx.get::<std::path::PathBuf>("hearth_path") {
                    out.set("hearth_path", p.clone());
                }
                if let Some(h) = ctx.get::<RetainedTempDir>("hearth_path_handle") {
                    out.set::<RetainedTempDir>("hearth_path_handle", std::sync::Arc::clone(h));
                }
                out.set(HEARTH_B_KEY, tmp);
                out.set::<RetainedTempDir>(HEARTH_B_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "a usage query engine hearth with no sinks",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, _params| {
                let (handle, tmp) = new_engine_hearth()?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set::<RetainedTempDir>("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        // ===== gRPC: UsageTimeSeries =====
        async_step_def(
            "the UsageTimeSeries RPC is called with granularity {string}",
            &[("engine_process", "EngineProcess")],
            &[
                (TS_RESULT_KEY, "Value"),
                ("engine_process", "EngineProcess"),
            ],
            |mut ctx, params| async move {
                let granularity = params.get_string(0).unwrap_or_default().to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let mut client =
                    anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr)
                        .await
                        .map_err(|e| format!("connect failed: {}", e))?;
                let request =
                    anvil_test_support::surfaced(anvil_engine::proto::UsageTimeSeriesRequest {
                        hearth_path: String::new(),
                        granularity,
                        all_hearths: false,
                    });
                let resp = client
                    .usage_time_series(request)
                    .await
                    .map_err(|s| format!("UsageTimeSeries RPC error {:?}: {}", s.code(), s.message()))?
                    .into_inner();
                // Reduce to a plain JSON shape for assertions, mirroring /ws.
                let value = usage_ts_to_json(&resp);
                let mut out = Context::new();
                out.set::<Value>(TS_RESULT_KEY, value);
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        // ===== gRPC: PlaybookStepVolume =====
        async_step_def(
            "the PlaybookStepVolume RPC is called for kind {string}",
            &[("engine_process", "EngineProcess")],
            &[
                (SV_RESULT_KEY, "Value"),
                ("engine_process", "EngineProcess"),
            ],
            |mut ctx, params| async move {
                let kind = params.get_string(0).unwrap_or_default().to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let mut client =
                    anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr)
                        .await
                        .map_err(|e| format!("connect failed: {}", e))?;
                let request =
                    anvil_test_support::surfaced(anvil_engine::proto::PlaybookStepVolumeRequest {
                        hearth_path: String::new(),
                        kind,
                        all_hearths: false,
                    });
                let resp = client
                    .playbook_step_volume(request)
                    .await
                    .map_err(|s| {
                        format!("PlaybookStepVolume RPC error {:?}: {}", s.code(), s.message())
                    })?
                    .into_inner();
                let value = step_volume_to_json(&resp);
                let mut out = Context::new();
                out.set::<Value>(SV_RESULT_KEY, value);
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        // ===== /ws drivers =====
        async_step_def(
            "a usage_timeseries JSON-RPC request is sent over /ws with granularity {string}",
            &[("engine_process", "EngineProcess")],
            &[
                (WS_RESPONSE_KEY, "Value"),
                ("engine_process", "EngineProcess"),
            ],
            |mut ctx, params| async move {
                let granularity = params.get_string(0).unwrap_or_default().to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let request = json!({
                    "jsonrpc": "2.0",
                    "id": 11,
                    "method": "usage_timeseries",
                    "params": { "surface": "test-harness", "hearth_path": "", "granularity": granularity }
                });
                let response = ws_roundtrip(engine.port, &request).await?;
                let mut out = Context::new();
                out.set::<Value>(WS_RESPONSE_KEY, response);
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        async_step_def(
            "a playbook_step_volume JSON-RPC request is sent over /ws for kind {string}",
            &[("engine_process", "EngineProcess")],
            &[
                (WS_RESPONSE_KEY, "Value"),
                ("engine_process", "EngineProcess"),
            ],
            |mut ctx, params| async move {
                let kind = params.get_string(0).unwrap_or_default().to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let request = json!({
                    "jsonrpc": "2.0",
                    "id": 12,
                    "method": "playbook_step_volume",
                    "params": { "surface": "test-harness", "hearth_path": "", "kind": kind }
                });
                let response = ws_roundtrip(engine.port, &request).await?;
                let mut out = Context::new();
                out.set::<Value>(WS_RESPONSE_KEY, response);
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        // ===== all_hearths drivers (gRPC + /ws) =====
        async_step_def(
            "the UsageTimeSeries RPC is called with granularity {string} across all hearths",
            &[("engine_process", "EngineProcess")],
            &[(TS_RESULT_KEY, "Value"), ("engine_process", "EngineProcess")],
            |mut ctx, params| async move {
                let granularity = params.get_string(0).unwrap_or_default().to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let mut client =
                    anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr)
                        .await
                        .map_err(|e| format!("connect failed: {}", e))?;
                let request = anvil_test_support::surfaced(anvil_engine::proto::UsageTimeSeriesRequest {
                    hearth_path: String::new(),
                    granularity,
                    all_hearths: true,
                });
                let resp = client
                    .usage_time_series(request)
                    .await
                    .map_err(|s| {
                        format!("UsageTimeSeries RPC error {:?}: {}", s.code(), s.message())
                    })?
                    .into_inner();
                let value = usage_ts_to_json(&resp);
                let mut out = Context::new();
                out.set::<Value>(TS_RESULT_KEY, value);
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        async_step_def(
            "the PlaybookStepVolume RPC is called for kind {string} across all hearths",
            &[("engine_process", "EngineProcess")],
            &[(SV_RESULT_KEY, "Value"), ("engine_process", "EngineProcess")],
            |mut ctx, params| async move {
                let kind = params.get_string(0).unwrap_or_default().to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let mut client =
                    anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr)
                        .await
                        .map_err(|e| format!("connect failed: {}", e))?;
                let request = anvil_test_support::surfaced(anvil_engine::proto::PlaybookStepVolumeRequest {
                    hearth_path: String::new(),
                    kind,
                    all_hearths: true,
                });
                let resp = client
                    .playbook_step_volume(request)
                    .await
                    .map_err(|s| {
                        format!("PlaybookStepVolume RPC error {:?}: {}", s.code(), s.message())
                    })?
                    .into_inner();
                let value = step_volume_to_json(&resp);
                let mut out = Context::new();
                out.set::<Value>(SV_RESULT_KEY, value);
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        async_step_def(
            "a usage_timeseries JSON-RPC request is sent over /ws with granularity {string} across all hearths",
            &[("engine_process", "EngineProcess")],
            &[(WS_RESPONSE_KEY, "Value"), ("engine_process", "EngineProcess")],
            |mut ctx, params| async move {
                let granularity = params.get_string(0).unwrap_or_default().to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let request = json!({
                    "jsonrpc": "2.0",
                    "id": 13,
                    "method": "usage_timeseries",
                    "params": { "surface": "test-harness", "hearth_path": "", "granularity": granularity, "all_hearths": true }
                });
                let response = ws_roundtrip(engine.port, &request).await?;
                let mut out = Context::new();
                out.set::<Value>(WS_RESPONSE_KEY, response);
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        // ===== gRPC assertions (over the JSON projection) =====
        check_def(
            "the usage timeseries RPC has {int} buckets",
            &[(TS_RESULT_KEY, "Value")],
            |ctx, params| {
                let result = ctx.get::<Value>(TS_RESULT_KEY).ok_or("No ts result")?;
                assert_bucket_count(result, params.get_int(0).ok_or("Expected count")? as usize)
            },
        ),
        check_def(
            "the usage timeseries RPC bucket {string} has total calls {int}",
            &[(TS_RESULT_KEY, "Value")],
            |ctx, params| {
                let result = ctx.get::<Value>(TS_RESULT_KEY).ok_or("No ts result")?;
                assert_bucket_total(
                    result,
                    params.get_string(0).ok_or("Expected period")?,
                    params.get_int(1).ok_or("Expected count")? as u64,
                )
            },
        ),
        check_def(
            "the usage timeseries RPC bucket {string} has begin count {int}",
            &[(TS_RESULT_KEY, "Value")],
            |ctx, params| {
                let result = ctx.get::<Value>(TS_RESULT_KEY).ok_or("No ts result")?;
                assert_bucket_counter(
                    result,
                    params.get_string(0).ok_or("Expected period")?,
                    "begin_count",
                    params.get_int(1).ok_or("Expected count")? as u64,
                )
            },
        ),
        check_def(
            "the usage timeseries RPC bucket {string} has complete count {int}",
            &[(TS_RESULT_KEY, "Value")],
            |ctx, params| {
                let result = ctx.get::<Value>(TS_RESULT_KEY).ok_or("No ts result")?;
                assert_bucket_counter(
                    result,
                    params.get_string(0).ok_or("Expected period")?,
                    "complete_count",
                    params.get_int(1).ok_or("Expected count")? as u64,
                )
            },
        ),
        check_def(
            "the usage timeseries RPC bucket {string} per-playbook kind {string} has call count {int}",
            &[(TS_RESULT_KEY, "Value")],
            |ctx, params| {
                let result = ctx.get::<Value>(TS_RESULT_KEY).ok_or("No ts result")?;
                assert_bucket_per_artifact_kind(
                    result,
                    params.get_string(0).ok_or("Expected period")?,
                    params.get_string(1).ok_or("Expected kind")?,
                    params.get_int(2).ok_or("Expected count")? as u64,
                )
            },
        ),
        check_def(
            "the playbook step volume RPC has {int} steps",
            &[(SV_RESULT_KEY, "Value")],
            |ctx, params| {
                let result = ctx.get::<Value>(SV_RESULT_KEY).ok_or("No sv result")?;
                assert_step_count(result, params.get_int(0).ok_or("Expected count")? as usize)
            },
        ),
        check_def(
            "the playbook step volume RPC step from {string} to {string} role {string} has call count {int}",
            &[(SV_RESULT_KEY, "Value")],
            |ctx, params| {
                let result = ctx.get::<Value>(SV_RESULT_KEY).ok_or("No sv result")?;
                assert_step(
                    result,
                    params.get_string(0).ok_or("from")?,
                    params.get_string(1).ok_or("to")?,
                    params.get_string(2).ok_or("role")?,
                    params.get_int(3).ok_or("count")? as u64,
                )
            },
        ),
        // ===== /ws assertions =====
        check_def(
            "the /ws usage timeseries result has {int} buckets",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let result = ws_result(&ctx)?;
                assert_bucket_count(&result, params.get_int(0).ok_or("Expected count")? as usize)
            },
        ),
        check_def(
            "the /ws usage timeseries bucket {string} has total calls {int}",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let result = ws_result(&ctx)?;
                assert_bucket_total(
                    &result,
                    params.get_string(0).ok_or("period")?,
                    params.get_int(1).ok_or("count")? as u64,
                )
            },
        ),
        check_def(
            "the /ws usage timeseries bucket {string} total_calls is a JSON number",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let result = ws_result(&ctx)?;
                let period = params.get_string(0).ok_or("period")?;
                let bucket = find_bucket(&result, period)?;
                if bucket.get("total_calls").map(Value::is_number) == Some(true) {
                    Ok(())
                } else {
                    Err(format!(
                        "bucket '{}' total_calls is not a JSON number: {:?}",
                        period,
                        bucket.get("total_calls")
                    ))
                }
            },
        ),
        check_def(
            "the /ws playbook step volume result has {int} steps",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let result = ws_result(&ctx)?;
                assert_step_count(&result, params.get_int(0).ok_or("count")? as usize)
            },
        ),
        check_def(
            "the /ws playbook step volume step from {string} to {string} role {string} has call count {int}",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let result = ws_result(&ctx)?;
                assert_step(
                    &result,
                    params.get_string(0).ok_or("from")?,
                    params.get_string(1).ok_or("to")?,
                    params.get_string(2).ok_or("role")?,
                    params.get_int(3).ok_or("count")? as u64,
                )
            },
        ),
        // ===== distinct_actors + hearths_included assertions =====
        check_def(
            "the usage timeseries RPC bucket {string} has distinct actors {int}",
            &[(TS_RESULT_KEY, "Value")],
            |ctx, params| {
                let result = ctx.get::<Value>(TS_RESULT_KEY).ok_or("No ts result")?;
                assert_bucket_distinct(
                    result,
                    params.get_string(0).ok_or("period")?,
                    params.get_int(1).ok_or("count")? as u64,
                )
            },
        ),
        check_def(
            "the /ws usage timeseries bucket {string} has distinct actors {int}",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let result = ws_result(&ctx)?;
                assert_bucket_distinct(
                    &result,
                    params.get_string(0).ok_or("period")?,
                    params.get_int(1).ok_or("count")? as u64,
                )
            },
        ),
        check_def(
            "the /ws usage timeseries bucket {string} distinct_actors is a JSON number",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let result = ws_result(&ctx)?;
                let period = params.get_string(0).ok_or("period")?;
                let bucket = find_bucket(&result, period)?;
                if bucket.get("distinct_actors").map(Value::is_number) == Some(true) {
                    Ok(())
                } else {
                    Err(format!(
                        "bucket '{}' distinct_actors is not a JSON number: {:?}",
                        period,
                        bucket.get("distinct_actors")
                    ))
                }
            },
        ),
        check_def(
            "the usage timeseries RPC included {int} hearths",
            &[(TS_RESULT_KEY, "Value")],
            |ctx, params| {
                let result = ctx.get::<Value>(TS_RESULT_KEY).ok_or("No ts result")?;
                assert_hearths_included(result, params.get_int(0).ok_or("count")? as usize)
            },
        ),
        check_def(
            "the /ws usage timeseries included {int} hearths",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let result = ws_result(&ctx)?;
                assert_hearths_included(&result, params.get_int(0).ok_or("count")? as usize)
            },
        ),
        check_def(
            "the usage timeseries RPC resolved hearth is the all-hearths sentinel",
            &[(TS_RESULT_KEY, "Value")],
            |ctx, _params| {
                let result = ctx.get::<Value>(TS_RESULT_KEY).ok_or("No ts result")?;
                assert_sentinel(result)
            },
        ),
        check_def(
            "the playbook step volume RPC included {int} hearths",
            &[(SV_RESULT_KEY, "Value")],
            |ctx, params| {
                let result = ctx.get::<Value>(SV_RESULT_KEY).ok_or("No sv result")?;
                assert_hearths_included(result, params.get_int(0).ok_or("count")? as usize)
            },
        ),
    ]
}

// ---- JSON projections of the gRPC responses (parallel to ws_bridge serializers) ----

fn usage_ts_to_json(resp: &anvil_engine::proto::UsageTimeSeriesResponse) -> Value {
    let buckets: Vec<Value> = resp
        .buckets
        .iter()
        .map(|b| {
            json!({
                "period_start": b.period_start,
                "total_calls": b.total_calls,
                "distinct_actors": b.distinct_actors,
                "begin_count": b.begin_count,
                "complete_count": b.complete_count,
                "per_artifact_kind": b.per_artifact_kind.iter().map(|w| json!({
                    "kind": w.kind, "call_count": w.call_count
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({
        "buckets": buckets,
        "resolved_hearth": resp.resolved_hearth,
        "hearths_included": resp.hearths_included,
    })
}

fn step_volume_to_json(resp: &anvil_engine::proto::PlaybookStepVolumeResponse) -> Value {
    let steps: Vec<Value> = resp
        .steps
        .iter()
        .map(|s| {
            json!({
                "from_state": s.from_state,
                "to_state": s.to_state,
                "role": s.role,
                "call_count": s.call_count,
            })
        })
        .collect();
    json!({
        "kind": resp.kind,
        "resolved_hearth": resp.resolved_hearth,
        "steps": steps,
        "hearths_included": resp.hearths_included,
    })
}

// ---- Shared assertion helpers over the common JSON shape ----

fn assert_bucket_distinct(result: &Value, period: &str, expected: u64) -> Result<(), String> {
    let bucket = find_bucket(result, period)?;
    let actual = bucket
        .get("distinct_actors")
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("bucket '{}' distinct_actors not a u64", period))?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "bucket '{}': expected distinct actors {}, got {}",
            period, expected, actual
        ))
    }
}

fn assert_hearths_included(result: &Value, expected: usize) -> Result<(), String> {
    let actual = result
        .get("hearths_included")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("hearths_included is not an array: {}", result))?
        .len();
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "expected {} hearths_included, got {}",
            expected, actual
        ))
    }
}

fn assert_sentinel(result: &Value) -> Result<(), String> {
    let rh = result
        .get("resolved_hearth")
        .and_then(Value::as_str)
        .unwrap_or("");
    if rh == "(all hearths)" {
        Ok(())
    } else {
        Err(format!("expected all-hearths sentinel, got '{}'", rh))
    }
}

fn buckets(result: &Value) -> Result<&Vec<Value>, String> {
    result
        .get("buckets")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("result.buckets is not an array: {}", result))
}

fn find_bucket<'a>(result: &'a Value, period: &str) -> Result<&'a Value, String> {
    buckets(result)?
        .iter()
        .find(|b| b.get("period_start").and_then(Value::as_str) == Some(period))
        .ok_or_else(|| format!("No bucket for period '{}'", period))
}

fn assert_bucket_count(result: &Value, expected: usize) -> Result<(), String> {
    let actual = buckets(result)?.len();
    if actual == expected {
        Ok(())
    } else {
        Err(format!("Expected {} buckets, got {}", expected, actual))
    }
}

fn assert_bucket_total(result: &Value, period: &str, expected: u64) -> Result<(), String> {
    let bucket = find_bucket(result, period)?;
    let actual = bucket
        .get("total_calls")
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("bucket '{}' total_calls not a u64", period))?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "bucket '{}': expected total {}, got {}",
            period, expected, actual
        ))
    }
}

fn assert_bucket_counter(
    result: &Value,
    period: &str,
    field: &str,
    expected: u64,
) -> Result<(), String> {
    let bucket = find_bucket(result, period)?;
    let actual = bucket
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("bucket '{}' {} not a u64", period, field))?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "bucket '{}': expected {} {}, got {}",
            period, field, expected, actual
        ))
    }
}

fn assert_bucket_per_artifact_kind(
    result: &Value,
    period: &str,
    kind: &str,
    expected: u64,
) -> Result<(), String> {
    let bucket = find_bucket(result, period)?;
    let per = bucket
        .get("per_artifact_kind")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("bucket '{}' per_artifact_kind not an array", period))?;
    let entry = per
        .iter()
        .find(|w| w.get("kind").and_then(Value::as_str) == Some(kind))
        .ok_or_else(|| format!("bucket '{}' has no per-playbook kind '{}'", period, kind))?;
    let actual = entry
        .get("call_count")
        .and_then(Value::as_u64)
        .ok_or("call_count not a u64")?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "bucket '{}' kind '{}': expected {}, got {}",
            period, kind, expected, actual
        ))
    }
}

fn steps_of(result: &Value) -> Result<&Vec<Value>, String> {
    result
        .get("steps")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("result.steps is not an array: {}", result))
}

fn assert_step_count(result: &Value, expected: usize) -> Result<(), String> {
    let actual = steps_of(result)?.len();
    if actual == expected {
        Ok(())
    } else {
        Err(format!("Expected {} steps, got {}", expected, actual))
    }
}

fn assert_step(
    result: &Value,
    from: &str,
    to: &str,
    role: &str,
    expected: u64,
) -> Result<(), String> {
    let step = steps_of(result)?
        .iter()
        .find(|s| {
            s.get("from_state").and_then(Value::as_str) == Some(from)
                && s.get("to_state").and_then(Value::as_str) == Some(to)
                && s.get("role").and_then(Value::as_str) == Some(role)
        })
        .ok_or_else(|| format!("No step ({} -> {}, {})", from, to, role))?;
    let actual = step
        .get("call_count")
        .and_then(Value::as_u64)
        .ok_or("call_count not a u64")?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "step ({} -> {}, {}): expected {}, got {}",
            from, to, role, expected, actual
        ))
    }
}
