//! Step module for the engine-seam universal-activity-log features
//! (`activity_log_instrumentation.feature` + `activity_summary_rpc.feature`).
//!
//! Two responsibilities:
//!   1. Assert the durable `activity-log.jsonl` sink contents AFTER a real
//!      command (route/begin/catalog/...) is driven against a started engine via
//!      the shared `engine` module's RPC steps. The sink is read directly from
//!      `hearth_path` (the engine writes it under the resolved hearth).
//!   2. Seed the sink directly + drive the `activity_summary` gRPC RPC and the
//!      `/ws` JSON-RPC method, asserting identical folded data. The WS roundtrip
//!      reuses the same helper pattern as `usage_timeseries_rpc`.

use anvil_test_support::engine::EngineProcess;
use anvil_core_hearth::fs_activity_log_adapter::FileSystemActivityLogAdapter;
use anvil_core::ports::activity_log_port::{
    ActivityLogReadPort, ActivityLogRecord, ActivityLogWritePort,
};
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{async_step_def, check_def, step_def, StepDef};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::path::PathBuf;
use tokio_tungstenite::tungstenite::Message;

use anvil_test_support::{retained_temp_dir, RetainedTempDir};

const WS_RESPONSE_KEY: &str = "as_rpc_ws_response";
const SUMMARY_KEY: &str = "as_rpc_summary";
const FIDELITY_KEY: &str = "as_rpc_fidelity";

fn column_index(table: &DataTable, name: &str) -> Result<usize, String> {
    table
        .headers
        .iter()
        .position(|h| h == name)
        .ok_or_else(|| format!("Missing '{}' column in data table", name))
}

fn read_sink(hearth: &std::path::Path) -> Result<Vec<ActivityLogRecord>, String> {
    FileSystemActivityLogAdapter::new(hearth)
        .read_activity_log()
        .map_err(|e| format!("read activity-log: {}", e))
}

fn new_engine_hearth() -> Result<(RetainedTempDir, PathBuf), String> {
    let (handle, tmp) = retained_temp_dir("anvil-activity-log-")?;
    std::fs::create_dir_all(tmp.join("tracks")).map_err(|e| format!("create tracks: {}", e))?;
    std::fs::write(tmp.join("tracks.md"), "# Tracks\n")
        .map_err(|e| format!("write tracks.md: {}", e))?;
    Ok((handle, tmp))
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
        .ok_or("No as_rpc_ws_response in context")?;
    if let Some(error) = response.get("error") {
        return Err(format!("Expected JSON-RPC result, got error: {}", error));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| format!("JSON-RPC response has no result: {}", response))
}

fn summary_to_json(resp: &anvil_engine::proto::ActivitySummaryResponse) -> Value {
    let label_counts = |list: &[anvil_engine::proto::ActivityLabelCount]| -> Vec<Value> {
        list.iter()
            .map(|c| json!({ "label": c.label, "count": c.count }))
            .collect()
    };
    json!({
        "resolved_hearth": resp.resolved_hearth,
        "hearths_included": resp.hearths_included,
        "total_turns": resp.total_turns,
        "by_command": label_counts(&resp.by_command),
        "by_route_outcome": label_counts(&resp.by_route_outcome),
        "by_source": label_counts(&resp.by_source),
        "by_artifact_kind": label_counts(&resp.by_artifact_kind),
        "buckets": resp.buckets.iter().map(|b| json!({
            "period_start": b.period_start,
            "total_turns": b.total_turns,
            "distinct_actors": b.distinct_actors,
        })).collect::<Vec<_>>(),
    })
}

// ---- assertion helpers over the common summary JSON shape ----

fn find_label_count(result: &Value, list_key: &str, label: &str) -> Result<u64, String> {
    let list = result
        .get(list_key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{} is not an array: {}", list_key, result))?;
    let entry = list
        .iter()
        .find(|e| e.get("label").and_then(Value::as_str) == Some(label))
        .ok_or_else(|| format!("no {} entry for label '{}'", list_key, label))?;
    entry
        .get("count")
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("{} '{}' count not a u64", list_key, label))
}

/// Serialize the gRPC PlaybookFidelityResponse into the SAME JSON shape the
/// `/ws` `playbook_fidelity` method emits, so the gRPC and WS scenarios assert
/// over an identical structure.
fn fidelity_to_json(resp: &anvil_engine::proto::PlaybookFidelityResponse) -> Value {
    let completion: Vec<Value> = resp
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
    let label_counts = |list: &[anvil_engine::proto::ActivityLabelCount]| -> Vec<Value> {
        list.iter()
            .map(|c| json!({ "label": c.label, "count": c.count }))
            .collect()
    };
    let instances: Vec<Value> = resp
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
        "resolved_hearth": resp.resolved_hearth,
        "hearths_included": resp.hearths_included,
        "completion": completion,
        "dangling_instances": resp.dangling_instances,
        "dangling_by_kind": label_counts(&resp.dangling_by_kind),
        "revision_cycles": label_counts(&resp.revision_cycles),
        "revision_cycles_total": resp.revision_cycles_total,
        "review_exits": resp.review_exits,
        "delegated_exits": resp.delegated_exits,
        "self_review_exits": resp.self_review_exits,
        "review_elapsed_seconds": resp.review_elapsed_seconds,
        "instances": instances,
    })
}

/// Look up the completion row for `kind` and return (begun, terminal).
fn fidelity_completion(result: &Value, kind: &str) -> Result<(u64, u64), String> {
    let completion = result
        .get("completion")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("completion is not an array: {}", result))?;
    let row = completion
        .iter()
        .find(|c| c.get("kind").and_then(Value::as_str) == Some(kind))
        .ok_or_else(|| format!("no completion row for kind '{}'", kind))?;
    let begun = row
        .get("begun")
        .and_then(Value::as_u64)
        .ok_or("begun not a u64")?;
    let terminal = row
        .get("terminal")
        .and_then(Value::as_u64)
        .ok_or("terminal not a u64")?;
    Ok((begun, terminal))
}

/// Read a top-level u64 scalar from the fidelity result.
fn fidelity_scalar(result: &Value, key: &str) -> Result<u64, String> {
    result
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("fidelity {} not a u64: {:?}", key, result.get(key)))
}

fn parse_bool(value: &str) -> Result<bool, String> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("expected true or false, got '{}'", value)),
    }
}

fn fidelity_instance<'a>(result: &'a Value, instance_id: &str) -> Result<&'a Value, String> {
    result
        .get("instances")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("instances is not an array: {}", result))?
        .iter()
        .find(|row| row.get("instance_id").and_then(Value::as_str) == Some(instance_id))
        .ok_or_else(|| format!("no instance row for '{}'", instance_id))
}

fn assert_fidelity_instance(
    result: &Value,
    instance_id: &str,
    expected_state: &str,
    expected_terminal: bool,
    expected_dangling: bool,
) -> Result<(), String> {
    let row = fidelity_instance(result, instance_id)?;
    let state = row
        .get("folded_state")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{} folded_state not a string", instance_id))?;
    if state != expected_state {
        return Err(format!(
            "{} folded_state: expected '{}', got '{}'",
            instance_id, expected_state, state
        ));
    }
    let reached_terminal = row
        .get("reached_terminal")
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("{} reached_terminal not a bool", instance_id))?;
    if reached_terminal != expected_terminal {
        return Err(format!(
            "{} reached_terminal: expected {}, got {}",
            instance_id, expected_terminal, reached_terminal
        ));
    }
    let dangling = row
        .get("dangling")
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("{} dangling not a bool", instance_id))?;
    if dangling != expected_dangling {
        return Err(format!(
            "{} dangling: expected {}, got {}",
            instance_id, expected_dangling, dangling
        ));
    }
    Ok(())
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ===== sink assertions after a real command =====
        check_def(
            "the activity log sink has {int} records",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = read_sink(hearth)?;
                if records.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} activity-log records, got {}: {:?}",
                        expected,
                        records.len(),
                        records
                    ))
                }
            },
        ),
        check_def(
            "the activity log sink has exactly {int} records with command {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let command = params.get_string(1).ok_or("Expected command")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = read_sink(hearth)?;
                let count = records.iter().filter(|r| r.command == command).count();
                if count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected exactly {} activity-log records with command '{}', got {}: {:?}",
                        expected, command, count, records
                    ))
                }
            },
        ),
        check_def(
            "the activity log sink has a record command {string} outcome {string} artifact_kind {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let command = params.get_string(0).ok_or("Expected command")?.to_string();
                let outcome = params.get_string(1).ok_or("Expected outcome")?.to_string();
                // artifact_kind may be empty — the step writer passes "" to mean
                // "no kind". The brine {string} param captures it verbatim.
                let artifact_kind = params.get_string(2).unwrap_or_default().to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = read_sink(hearth)?;
                let found = records.iter().any(|r| {
                    r.command == command
                        && r.outcome == outcome
                        && r.artifact_kind == artifact_kind
                });
                if found {
                    Ok(())
                } else {
                    Err(format!(
                        "No activity-log record (command={}, outcome={}, artifact_kind={}); got {:?}",
                        command, outcome, artifact_kind, records
                    ))
                }
            },
        ),
        check_def(
            "the activity log sink has a record command {string} from_state {string} to_state {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let command = params.get_string(0).ok_or("Expected command")?.to_string();
                // from_state/to_state may be empty — the step writer passes ""
                // for commands with no transition (catalog/describe/route) and
                // for begin's from_state (no prior state).
                let from_state = params.get_string(1).unwrap_or_default().to_string();
                let to_state = params.get_string(2).unwrap_or_default().to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = read_sink(hearth)?;
                let found = records.iter().any(|r| {
                    r.command == command
                        && r.from_state == from_state
                        && r.to_state == to_state
                });
                if found {
                    Ok(())
                } else {
                    Err(format!(
                        "No activity-log record (command={}, from_state={}, to_state={}); got {:?}",
                        command, from_state, to_state, records
                    ))
                }
            },
        ),
        check_def(
            "the activity log sink has no record command {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let command = params.get_string(0).ok_or("Expected command")?.to_string();
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = read_sink(hearth)?;
                if records.iter().any(|r| r.command == command) {
                    Err(format!(
                        "Expected NO activity-log record for command '{}', but found one: {:?}",
                        command, records
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the activity log resume record has outcome {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected outcome")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = read_sink(hearth)?;
                if records
                    .iter()
                    .any(|record| record.command == "route" && record.outcome == expected)
                {
                    Ok(())
                } else {
                    Err(format!(
                        "No resume activity record with outcome '{}'; got {:?}",
                        expected, records
                    ))
                }
            },
        ),
        check_def(
            "the activity log resume record has call state {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected call state")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = read_sink(hearth)?;
                if records.iter().any(|record| {
                    record.command == "route"
                        && record.outcome == "resume"
                        && record.call_state.as_deref() == Some(expected)
                }) {
                    Ok(())
                } else {
                    Err(format!(
                        "No resume activity record with call state '{}'; got {:?}",
                        expected, records
                    ))
                }
            },
        ),
        check_def(
            "the activity log resume record identifies artifact {string} kind {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let artifact_id = params.get_string(0).ok_or("Expected artifact id")?;
                let kind = params.get_string(1).ok_or("Expected workflow kind")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = read_sink(hearth)?;
                if records.iter().any(|record| {
                    record.command == "route"
                        && record.outcome == "resume"
                        && record.playbook_run_id.as_deref() == Some(artifact_id)
                        && record.artifact_kind == kind
                }) {
                    Ok(())
                } else {
                    Err(format!(
                        "No resume activity record for artifact '{}' kind '{}'; got {:?}",
                        artifact_id, kind, records
                    ))
                }
            },
        ),
        check_def(
            "the activity log sink has no route record with outcome {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let outcome = params.get_string(0).ok_or("Expected excluded outcome")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = read_sink(hearth)?;
                if records
                    .iter()
                    .any(|record| record.command == "route" && record.outcome == outcome)
                {
                    Err(format!(
                        "Found excluded route outcome '{}': {:?}",
                        outcome, records
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the activity log route record has call state {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected call state")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let records = read_sink(hearth)?;
                if records.iter().any(|record| {
                    record.command == "route"
                        && record.call_state.as_deref() == Some(expected)
                }) {
                    Ok(())
                } else {
                    Err(format!(
                        "No route activity record with call state '{}'; got {:?}",
                        expected, records
                    ))
                }
            },
        ),
        // ===== direct sink seeding (for the activity_summary fold seam) =====
        step_def(
            "an activity log engine hearth seeded with turns:",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let command_col = column_index(table, "command")?;
                let outcome_col = column_index(table, "outcome")?;
                let wfk_col = table.headers.iter().position(|h| h == "artifact_kind");
                let from_col = table.headers.iter().position(|h| h == "from_state");
                let to_col = table.headers.iter().position(|h| h == "to_state");
                let hash_col = table.headers.iter().position(|h| h == "actor_hash");
                let source_col = table.headers.iter().position(|h| h == "source");
                let instance_col = table
                    .headers
                    .iter()
                    .position(|h| h == "playbook_run_id");
                let at_col = column_index(table, "at")?;
                let (handle, tmp) = new_engine_hearth()?;
                let adapter = FileSystemActivityLogAdapter::new(&tmp);
                for row in &table.rows {
                    let actor_hash = hash_col.and_then(|i| {
                        let v = row[i].trim();
                        if v.is_empty() || v == "-" {
                            None
                        } else {
                            Some(v.to_string())
                        }
                    });
                    adapter
                        .append_activity_log(&ActivityLogRecord {
                            command: row[command_col].trim().to_string(),
                            outcome: row[outcome_col].trim().to_string(),
                            artifact_kind: wfk_col
                                .map(|i| row[i].trim().to_string())
                                .unwrap_or_default(),
                            from_state: from_col
                                .map(|i| row[i].trim().to_string())
                                .unwrap_or_default(),
                            to_state: to_col
                                .map(|i| row[i].trim().to_string())
                                .unwrap_or_default(),
                            actor_hash,
                            at: row[at_col].trim().to_string(),
                            source: source_col
                                .map(|i| row[i].trim().to_string())
                                .unwrap_or_default(),
                            conversation_hash: None,
                            project_label: None,
                            playbook_run_id: instance_col.and_then(|i| {
                                let v = row[i].trim();
                                if v.is_empty() {
                                    None
                                } else {
                                    Some(v.to_string())
                                }
                            }),
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
        // ===== gRPC: ActivitySummary =====
        async_step_def(
            "the ActivitySummary RPC is called with granularity {string}",
            &[("engine_process", "EngineProcess")],
            &[(SUMMARY_KEY, "Value"), ("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            |mut ctx, params| async move {
                let granularity = params.get_string(0).unwrap_or_default().to_string();
                let hearth_path = ctx.get::<PathBuf>("hearth_path").cloned();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let mut client =
                    anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr)
                        .await
                        .map_err(|e| format!("connect failed: {}", e))?;
                let request = anvil_test_support::surfaced(anvil_engine::proto::ActivitySummaryRequest {
                    hearth_path: String::new(),
                    granularity,
                    all_hearths: false,
                });
                let resp = client
                    .activity_summary(request)
                    .await
                    .map_err(|s| {
                        format!("ActivitySummary RPC error {:?}: {}", s.code(), s.message())
                    })?
                    .into_inner();
                let value = summary_to_json(&resp);
                let mut out = Context::new();
                out.set::<Value>(SUMMARY_KEY, value);
                out.set("engine_process", engine);
                if let Some(p) = hearth_path {
                    out.set("hearth_path", p);
                }
                Ok(out)
            },
        ),
        // ===== /ws: activity_summary =====
        async_step_def(
            "an activity_summary JSON-RPC request is sent over /ws with granularity {string}",
            &[("engine_process", "EngineProcess")],
            &[(WS_RESPONSE_KEY, "Value"), ("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            |mut ctx, params| async move {
                let granularity = params.get_string(0).unwrap_or_default().to_string();
                let hearth_path = ctx.get::<PathBuf>("hearth_path").cloned();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let request = json!({
                    "jsonrpc": "2.0",
                    "id": 21,
                    "method": "activity_summary",
                    "params": { "surface": "test-harness", "hearth_path": "", "granularity": granularity }
                });
                let response = ws_roundtrip(engine.port, &request).await?;
                let mut out = Context::new();
                out.set::<Value>(WS_RESPONSE_KEY, response);
                out.set("engine_process", engine);
                if let Some(p) = hearth_path {
                    out.set("hearth_path", p);
                }
                Ok(out)
            },
        ),
        // ===== gRPC summary assertions =====
        check_def(
            "the activity summary RPC has total turns {int}",
            &[(SUMMARY_KEY, "Value")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as u64;
                let result = ctx.get::<Value>(SUMMARY_KEY).ok_or("No summary")?;
                let actual = result
                    .get("total_turns")
                    .and_then(Value::as_u64)
                    .ok_or("total_turns not a u64")?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("expected total turns {}, got {}", expected, actual))
                }
            },
        ),
        check_def(
            "the activity summary RPC by_command {string} has count {int}",
            &[(SUMMARY_KEY, "Value")],
            |ctx, params| {
                let label = params.get_string(0).ok_or("label")?;
                let expected = params.get_int(1).ok_or("count")? as u64;
                let result = ctx.get::<Value>(SUMMARY_KEY).ok_or("No summary")?;
                let actual = find_label_count(result, "by_command", label)?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("by_command '{}': expected {}, got {}", label, expected, actual))
                }
            },
        ),
        check_def(
            "the activity summary RPC by_route_outcome {string} has count {int}",
            &[(SUMMARY_KEY, "Value")],
            |ctx, params| {
                let label = params.get_string(0).ok_or("label")?;
                let expected = params.get_int(1).ok_or("count")? as u64;
                let result = ctx.get::<Value>(SUMMARY_KEY).ok_or("No summary")?;
                let actual = find_label_count(result, "by_route_outcome", label)?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("by_route_outcome '{}': expected {}, got {}", label, expected, actual))
                }
            },
        ),
        check_def(
            "the activity summary RPC by_source {string} has count {int}",
            &[(SUMMARY_KEY, "Value")],
            |ctx, params| {
                let label = params.get_string(0).ok_or("label")?;
                let expected = params.get_int(1).ok_or("count")? as u64;
                let result = ctx.get::<Value>(SUMMARY_KEY).ok_or("No summary")?;
                let actual = find_label_count(result, "by_source", label)?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("by_source '{}': expected {}, got {}", label, expected, actual))
                }
            },
        ),
        check_def(
            "the activity summary RPC by_artifact_kind {string} has count {int}",
            &[(SUMMARY_KEY, "Value")],
            |ctx, params| {
                let label = params.get_string(0).ok_or("label")?;
                let expected = params.get_int(1).ok_or("count")? as u64;
                let result = ctx.get::<Value>(SUMMARY_KEY).ok_or("No summary")?;
                let actual = find_label_count(result, "by_artifact_kind", label)?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("by_artifact_kind '{}': expected {}, got {}", label, expected, actual))
                }
            },
        ),
        check_def(
            "the activity summary RPC bucket {string} has distinct actors {int}",
            &[(SUMMARY_KEY, "Value")],
            |ctx, params| {
                let period = params.get_string(0).ok_or("period")?.to_string();
                let expected = params.get_int(1).ok_or("count")? as u64;
                let result = ctx.get::<Value>(SUMMARY_KEY).ok_or("No summary")?;
                let buckets = result
                    .get("buckets")
                    .and_then(Value::as_array)
                    .ok_or("buckets not an array")?;
                let bucket = buckets
                    .iter()
                    .find(|b| b.get("period_start").and_then(Value::as_str) == Some(&period))
                    .ok_or_else(|| format!("no bucket for period '{}'", period))?;
                let actual = bucket
                    .get("distinct_actors")
                    .and_then(Value::as_u64)
                    .ok_or("distinct_actors not a u64")?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("bucket '{}': expected distinct actors {}, got {}", period, expected, actual))
                }
            },
        ),
        // ===== /ws summary assertions (identical fold) =====
        check_def(
            "the /ws activity summary has total turns {int}",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("count")? as u64;
                let result = ws_result(&ctx)?;
                let actual = result
                    .get("total_turns")
                    .and_then(Value::as_u64)
                    .ok_or("total_turns not a u64")?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("expected total turns {}, got {}", expected, actual))
                }
            },
        ),
        check_def(
            "the /ws activity summary total_turns is a JSON number",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, _params| {
                let result = ws_result(&ctx)?;
                if result.get("total_turns").map(Value::is_number) == Some(true) {
                    Ok(())
                } else {
                    Err(format!(
                        "total_turns is not a JSON number: {:?}",
                        result.get("total_turns")
                    ))
                }
            },
        ),
        check_def(
            "the /ws activity summary by_command {string} has count {int}",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let label = params.get_string(0).ok_or("label")?;
                let expected = params.get_int(1).ok_or("count")? as u64;
                let result = ws_result(&ctx)?;
                let actual = find_label_count(&result, "by_command", label)?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("by_command '{}': expected {}, got {}", label, expected, actual))
                }
            },
        ),
        check_def(
            "the /ws activity summary by_route_outcome {string} has count {int}",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let label = params.get_string(0).ok_or("label")?;
                let expected = params.get_int(1).ok_or("count")? as u64;
                let result = ws_result(&ctx)?;
                let actual = find_label_count(&result, "by_route_outcome", label)?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("by_route_outcome '{}': expected {}, got {}", label, expected, actual))
                }
            },
        ),
        check_def(
            "the /ws activity summary by_source {string} has count {int}",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let label = params.get_string(0).ok_or("label")?;
                let expected = params.get_int(1).ok_or("count")? as u64;
                let result = ws_result(&ctx)?;
                let actual = find_label_count(&result, "by_source", label)?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("by_source '{}': expected {}, got {}", label, expected, actual))
                }
            },
        ),
        // ===== gRPC: PlaybookFidelity =====
        async_step_def(
            "the PlaybookFidelity RPC is called",
            &[("engine_process", "EngineProcess")],
            &[
                (FIDELITY_KEY, "Value"),
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
            ],
            |mut ctx, _params| async move {
                let hearth_path = ctx.get::<PathBuf>("hearth_path").cloned();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let mut client =
                    anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr)
                        .await
                        .map_err(|e| format!("connect failed: {}", e))?;
                let request = anvil_test_support::surfaced(anvil_engine::proto::PlaybookFidelityRequest {
                    hearth_path: String::new(),
                    all_hearths: false,
                });
                let resp = client
                    .playbook_fidelity(request)
                    .await
                    .map_err(|s| {
                        format!("PlaybookFidelity RPC error {:?}: {}", s.code(), s.message())
                    })?
                    .into_inner();
                let value = fidelity_to_json(&resp);
                let mut out = Context::new();
                out.set::<Value>(FIDELITY_KEY, value);
                out.set("engine_process", engine);
                if let Some(p) = hearth_path {
                    out.set("hearth_path", p);
                }
                Ok(out)
            },
        ),
        check_def(
            "the fidelity RPC completion for {string} has begun {int}",
            &[(FIDELITY_KEY, "Value")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("kind")?;
                let expected = params.get_int(1).ok_or("begun")? as u64;
                let result = ctx.get::<Value>(FIDELITY_KEY).ok_or("No fidelity")?;
                let (begun, _) = fidelity_completion(result, kind)?;
                if begun == expected {
                    Ok(())
                } else {
                    Err(format!("'{}' begun: expected {}, got {}", kind, expected, begun))
                }
            },
        ),
        check_def(
            "the fidelity RPC completion for {string} has terminal {int}",
            &[(FIDELITY_KEY, "Value")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("kind")?;
                let expected = params.get_int(1).ok_or("terminal")? as u64;
                let result = ctx.get::<Value>(FIDELITY_KEY).ok_or("No fidelity")?;
                let (_, terminal) = fidelity_completion(result, kind)?;
                if terminal == expected {
                    Ok(())
                } else {
                    Err(format!("'{}' terminal: expected {}, got {}", kind, expected, terminal))
                }
            },
        ),
        check_def(
            "the fidelity RPC has {int} dangling instances",
            &[(FIDELITY_KEY, "Value")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("count")? as u64;
                let result = ctx.get::<Value>(FIDELITY_KEY).ok_or("No fidelity")?;
                let actual = fidelity_scalar(result, "dangling_instances")?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("expected {} dangling instances, got {}", expected, actual))
                }
            },
        ),
        check_def(
            "the fidelity RPC instance {string} has folded state {string} reached terminal {string} and dangling {string}",
            &[(FIDELITY_KEY, "Value")],
            |ctx, params| {
                let instance_id = params.get_string(0).ok_or("instance id")?;
                let expected_state = params.get_string(1).ok_or("folded state")?;
                let expected_terminal =
                    parse_bool(params.get_string(2).ok_or("reached terminal")?)?;
                let expected_dangling = parse_bool(params.get_string(3).ok_or("dangling")?)?;
                let result = ctx.get::<Value>(FIDELITY_KEY).ok_or("No fidelity")?;
                assert_fidelity_instance(
                    result,
                    instance_id,
                    expected_state,
                    expected_terminal,
                    expected_dangling,
                )
            },
        ),
        // ===== /ws: playbook_fidelity (identical fold) =====
        async_step_def(
            "a playbook_fidelity JSON-RPC request is sent over /ws",
            &[("engine_process", "EngineProcess")],
            &[
                (WS_RESPONSE_KEY, "Value"),
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
            ],
            |mut ctx, _params| async move {
                let hearth_path = ctx.get::<PathBuf>("hearth_path").cloned();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let request = json!({
                    "jsonrpc": "2.0",
                    "id": 33,
                    "method": "playbook_fidelity",
                    "params": { "surface": "test-harness", "hearth_path": "" }
                });
                let response = ws_roundtrip(engine.port, &request).await?;
                let mut out = Context::new();
                out.set::<Value>(WS_RESPONSE_KEY, response);
                out.set("engine_process", engine);
                if let Some(p) = hearth_path {
                    out.set("hearth_path", p);
                }
                Ok(out)
            },
        ),
        check_def(
            "the /ws fidelity completion for {string} has begun {int}",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("kind")?;
                let expected = params.get_int(1).ok_or("begun")? as u64;
                let result = ws_result(&ctx)?;
                let (begun, _) = fidelity_completion(&result, kind)?;
                if begun == expected {
                    Ok(())
                } else {
                    Err(format!("'{}' begun: expected {}, got {}", kind, expected, begun))
                }
            },
        ),
        check_def(
            "the /ws fidelity completion for {string} has terminal {int}",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("kind")?;
                let expected = params.get_int(1).ok_or("terminal")? as u64;
                let result = ws_result(&ctx)?;
                let (_, terminal) = fidelity_completion(&result, kind)?;
                if terminal == expected {
                    Ok(())
                } else {
                    Err(format!("'{}' terminal: expected {}, got {}", kind, expected, terminal))
                }
            },
        ),
        check_def(
            "the /ws fidelity dangling_instances is {int}",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("count")? as u64;
                let result = ws_result(&ctx)?;
                let actual = fidelity_scalar(&result, "dangling_instances")?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("expected dangling_instances {}, got {}", expected, actual))
                }
            },
        ),
        check_def(
            "the /ws fidelity dangling_instances is a JSON number",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, _params| {
                let result = ws_result(&ctx)?;
                if result.get("dangling_instances").map(Value::is_number) == Some(true) {
                    Ok(())
                } else {
                    Err(format!(
                        "dangling_instances is not a JSON number: {:?}",
                        result.get("dangling_instances")
                    ))
                }
            },
        ),
        check_def(
            "the /ws fidelity review_exits is {int}",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("count")? as u64;
                let result = ws_result(&ctx)?;
                let actual = fidelity_scalar(&result, "review_exits")?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("expected review_exits {}, got {}", expected, actual))
                }
            },
        ),
        check_def(
            "the /ws fidelity delegated_exits is {int}",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("count")? as u64;
                let result = ws_result(&ctx)?;
                let actual = fidelity_scalar(&result, "delegated_exits")?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("expected delegated_exits {}, got {}", expected, actual))
                }
            },
        ),
        check_def(
            "the /ws fidelity instance {string} has folded state {string} reached terminal {string} and dangling {string}",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let instance_id = params.get_string(0).ok_or("instance id")?;
                let expected_state = params.get_string(1).ok_or("folded state")?;
                let expected_terminal =
                    parse_bool(params.get_string(2).ok_or("reached terminal")?)?;
                let expected_dangling = parse_bool(params.get_string(3).ok_or("dangling")?)?;
                let result = ws_result(&ctx)?;
                assert_fidelity_instance(
                    &result,
                    instance_id,
                    expected_state,
                    expected_terminal,
                    expected_dangling,
                )
            },
        ),
        check_def(
            "the /ws fidelity completion has {int} entries",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("count")? as usize;
                let result = ws_result(&ctx)?;
                let actual = result
                    .get("completion")
                    .and_then(Value::as_array)
                    .map(|a| a.len())
                    .unwrap_or(0);
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("expected {} completion entries, got {}", expected, actual))
                }
            },
        ),
        check_def(
            "the /ws fidelity instances has {int} entries",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("count")? as usize;
                let result = ws_result(&ctx)?;
                let actual = result
                    .get("instances")
                    .and_then(Value::as_array)
                    .map(|a| a.len())
                    .unwrap_or(0);
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("expected {} instance entries, got {}", expected, actual))
                }
            },
        ),
    ]
}
