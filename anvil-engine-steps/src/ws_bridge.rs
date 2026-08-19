//! Step module for `engine_ws_bridge.feature` (engine seam).
//!
//! Exercises the engine's HTTP bridge multiplexed onto the gRPC port: an
//! HTTP/1.1 `GET /health` probe and a JSON-RPC 2.0 WebSocket on `/ws`. These
//! steps drive the bridge from the perspective of the anvil-kit frontend (and
//! Foundry's health probe) — the bridge's actual users. They reuse the
//! `playbook_activity_rpc` seeding step and the shared `engine` lifecycle/route
//! steps; this module adds ONLY the HTTP GET, the WS request, and the
//! response/JSON-RPC-envelope assertions.

use anvil_test_support::engine::EngineProcess;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{async_step_def, check_def, step_def, StepDef};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

const HEALTH_STATUS_KEY: &str = "ws_bridge_health_status";
/// The response key every /ws step in this crate stores its frame under, so the
/// envelope assertions below bite over any module's /ws call and no second
/// spelling of "the reply frame" comes into existence.
pub const WS_RESPONSE_KEY: &str = "ws_bridge_ws_response";
const HEALTH_MAX_MS_KEY: &str = "ws_bridge_health_max_ms";
const HEALTH_OK_KEY: &str = "ws_bridge_health_all_ok";
const HEALTH_PROBES_KEY: &str = "ws_bridge_health_probes";

/// Raw HTTP/1.1 GET against the engine's loopback port; returns the status code.

async fn http_get_status(port: u16, path: &str) -> Result<u16, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .map_err(|e| format!("connect to engine port {} failed: {}", port, e))?;
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
        path
    );
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|e| format!("write HTTP request failed: {}", e))?;
    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .await
        .map_err(|e| format!("read HTTP response failed: {}", e))?;
    let text = String::from_utf8_lossy(&buf);
    // Status line: "HTTP/1.1 200 OK"
    let status_line = text.lines().next().ok_or("empty HTTP response")?;
    let code = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| format!("malformed status line: {:?}", status_line))?;
    code.parse::<u16>()
        .map_err(|e| format!("unparseable status code {:?}: {}", code, e))
}

/// Open a WebSocket to `/ws`, send one JSON-RPC frame, await one reply frame.
/// Shared across the crate's /ws step modules: one transport, so a second
/// module cannot drive the bridge in a subtly different way and grade it.
pub async fn ws_roundtrip(port: u16, request: &Value) -> Result<Value, String> {
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
            Message::Binary(bytes) => {
                return serde_json::from_slice(&bytes)
                    .map_err(|e| format!("ws binary reply not JSON: {}", e));
            }
            Message::Close(_) => return Err("ws closed before a reply frame".to_string()),
            _ => continue,
        }
    }
    Err("ws stream ended before a reply frame".to_string())
}

/// Pull the JSON-RPC `result` value from the stored response, erroring if the
/// frame is an error envelope.
fn ws_result(ctx: &Context) -> Result<Value, String> {
    let response = ctx
        .get::<Value>(WS_RESPONSE_KEY)
        .ok_or("No ws_bridge_ws_response in context")?;
    if let Some(error) = response.get("error") {
        return Err(format!("Expected JSON-RPC result, got error: {}", error));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| format!("JSON-RPC response has no result: {}", response))
}

/// Flatten every entry across owner groups in a JSON-RPC playbook_activity result.
fn result_entries(result: &Value) -> Result<Vec<Value>, String> {
    let owners = result
        .get("owners")
        .and_then(Value::as_array)
        .ok_or("result.owners is not an array")?;
    Ok(owners
        .iter()
        .filter_map(|g| g.get("entries").and_then(Value::as_array))
        .flatten()
        .cloned()
        .collect())
}

pub fn steps() -> Vec<StepDef> {
    vec![
        async_step_def(
            "an HTTP GET /health is sent to the engine port",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                (HEALTH_STATUS_KEY, "u16"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let status = http_get_status(engine.port, "/health").await?;
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set::<u16>(HEALTH_STATUS_KEY, status);
                Ok(out)
            },
        ),
        check_def(
            "the /health response status is {int}",
            &[(HEALTH_STATUS_KEY, "u16")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected status code")? as u16;
                let actual = *ctx
                    .get::<u16>(HEALTH_STATUS_KEY)
                    .ok_or("No ws_bridge_health_status")?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected /health status {}, got {}",
                        expected, actual
                    ))
                }
            },
        ),
        async_step_def(
            "a playbook_activity JSON-RPC request is sent over /ws with hearth_path {string}",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                (WS_RESPONSE_KEY, "Value"),
            ],
            |mut ctx, params| async move {
                let hearth_path = params.get_string(0).unwrap_or_default().to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let request = json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "playbook_activity",
                    "params": { "surface": "test-harness", "hearth_path": hearth_path }
                });
                let response = ws_roundtrip(engine.port, &request).await?;
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set::<Value>(WS_RESPONSE_KEY, response);
                Ok(out)
            },
        ),
        async_step_def(
            "a JSON-RPC request for method {string} is sent over /ws",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                (WS_RESPONSE_KEY, "Value"),
            ],
            |mut ctx, params| async move {
                let method = params.get_string(0).ok_or("Expected method")?.to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let request = json!({
                    "jsonrpc": "2.0",
                    "id": 7,
                    "method": method,
                    // Named, so this scenario still grades method resolution
                    // rather than tripping the surface gate ahead of it.
                    "params": { "surface": "test-harness" }
                });
                let response = ws_roundtrip(engine.port, &request).await?;
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set::<Value>(WS_RESPONSE_KEY, response);
                Ok(out)
            },
        ),
        check_def(
            "the /ws JSON-RPC result groups owner {string} with kinds {string}",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let owner = params.get_string(0).ok_or("Expected owner")?.to_string();
                let expected: Vec<String> = params
                    .get_string(1)
                    .ok_or("Expected kinds")?
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect();
                let result = ws_result(&ctx)?;
                let owners = result
                    .get("owners")
                    .and_then(Value::as_array)
                    .ok_or("result.owners is not an array")?;
                let group = owners
                    .iter()
                    .find(|g| g.get("owner").and_then(Value::as_str) == Some(owner.as_str()))
                    .ok_or_else(|| format!("No group for owner '{}'", owner))?;
                let actual: Vec<String> = group
                    .get("entries")
                    .and_then(Value::as_array)
                    .map(|entries| {
                        entries
                            .iter()
                            .filter_map(|e| {
                                e.get("kind").and_then(Value::as_str).map(str::to_string)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let missing: Vec<&String> =
                    expected.iter().filter(|k| !actual.contains(k)).collect();
                if missing.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "Owner '{}': missing kinds {:?} (group has {:?})",
                        owner, missing, actual
                    ))
                }
            },
        ),
        check_def(
            "the /ws JSON-RPC result entry for kind {string} has call count {int}",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let expected = params.get_int(1).ok_or("Expected count")?;
                let result = ws_result(&ctx)?;
                let entries = result_entries(&result)?;
                let entry = entries
                    .iter()
                    .find(|e| e.get("kind").and_then(Value::as_str) == Some(kind.as_str()))
                    .ok_or_else(|| format!("No entry for kind '{}'", kind))?;
                let actual = entry
                    .get("call_count")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| format!("kind '{}' call_count not an integer", kind))?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Kind '{}': expected call count {}, got {}",
                        kind, expected, actual
                    ))
                }
            },
        ),
        check_def(
            "the /ws JSON-RPC result call_count for kind {string} is a JSON number",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let result = ws_result(&ctx)?;
                let entries = result_entries(&result)?;
                let entry = entries
                    .iter()
                    .find(|e| e.get("kind").and_then(Value::as_str) == Some(kind.as_str()))
                    .ok_or_else(|| format!("No entry for kind '{}'", kind))?;
                let call_count = entry
                    .get("call_count")
                    .ok_or_else(|| format!("kind '{}' has no call_count", kind))?;
                if call_count.is_number() {
                    Ok(())
                } else {
                    Err(format!(
                        "kind '{}' call_count must be a JSON number, got {}",
                        kind, call_count
                    ))
                }
            },
        ),
        check_def(
            "the /ws JSON-RPC response is an error envelope",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, _params| {
                let response = ctx
                    .get::<Value>(WS_RESPONSE_KEY)
                    .ok_or("No ws_bridge_ws_response")?;
                if response.get("error").is_some() {
                    Ok(())
                } else {
                    Err(format!("Expected an error envelope, got: {}", response))
                }
            },
        ),
        check_def(
            "the /ws JSON-RPC error.data.code is a non-empty string",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, _params| {
                let response = ctx
                    .get::<Value>(WS_RESPONSE_KEY)
                    .ok_or("No ws_bridge_ws_response")?;
                let code = response
                    .get("error")
                    .and_then(|e| e.get("data"))
                    .and_then(|d| d.get("code"))
                    .ok_or("error.data.code missing")?;
                match code.as_str() {
                    Some(s) if !s.is_empty() => Ok(()),
                    Some(_) => Err("error.data.code is an empty string".to_string()),
                    None => Err(format!("error.data.code is not a string: {}", code)),
                }
            },
        ),
        // -----------------------------------------------------------------
        // The read seam: every request over this bridge names the surface
        // making it, and the engine records that name. A request that names no
        // surface is REFUSED — never served under a default, because a default
        // is how an unattributable read comes to look attributed.
        // -----------------------------------------------------------------
        async_step_def(
            "a {string} request is sent over /ws naming the surface {string}",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                (WS_RESPONSE_KEY, "Value"),
            ],
            |mut ctx, params| async move {
                let method = params.get_string(0).ok_or("Expected method")?.to_string();
                let surface = params.get_string(1).ok_or("Expected surface")?.to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let request = json!({
                    "jsonrpc": "2.0",
                    "id": 11,
                    "method": method,
                    "params": { "surface": surface, "hearth_path": "" }
                });
                let response = ws_roundtrip(engine.port, &request).await?;
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set::<Value>(WS_RESPONSE_KEY, response);
                Ok(out)
            },
        ),
        async_step_def(
            "a {string} request is sent over /ws naming no surface",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                (WS_RESPONSE_KEY, "Value"),
            ],
            |mut ctx, params| async move {
                let method = params.get_string(0).ok_or("Expected method")?.to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let request = json!({
                    "jsonrpc": "2.0",
                    "id": 12,
                    "method": method,
                    "params": { "hearth_path": "" }
                });
                let response = ws_roundtrip(engine.port, &request).await?;
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set::<Value>(WS_RESPONSE_KEY, response);
                Ok(out)
            },
        ),
        async_step_def(
            "the case for playbook {string} is asked for over /ws naming the surface {string}",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                (WS_RESPONSE_KEY, "Value"),
            ],
            |mut ctx, params| async move {
                let kind = params.get_string(0).ok_or("Expected a playbook kind")?.to_string();
                let surface = params.get_string(1).ok_or("Expected surface")?.to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let request = json!({
                    "jsonrpc": "2.0",
                    "id": 21,
                    "method": "autonomy_evidence",
                    "params": { "surface": surface, "hearth_path": "", "kind": kind }
                });
                let response = ws_roundtrip(engine.port, &request).await?;
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set::<Value>(WS_RESPONSE_KEY, response);
                Ok(out)
            },
        ),
        check_def(
            "the /ws case reports {int} clean of {int} attempted",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let clean = params.get_int(0).ok_or("Expected a clean count")? as u64;
                let attempted = params.get_int(1).ok_or("Expected an attempted count")? as u64;
                let response = ctx.get::<Value>(WS_RESPONSE_KEY).ok_or("No ws response")?;
                let result = response
                    .get("result")
                    .ok_or_else(|| format!("Response carries no result: {}", response))?;
                if result.get("found").and_then(Value::as_bool) != Some(true) {
                    return Err(format!("the bridge reports no case at all: {}", result));
                }
                let evidence = result
                    .get("evidence")
                    .ok_or_else(|| format!("result carries no evidence: {}", result))?;
                let got_clean = evidence.get("runs_clean").and_then(Value::as_u64);
                let got_attempted = evidence.get("runs_attempted").and_then(Value::as_u64);
                // BOTH POPULATIONS, BOTH REQUIRED NON-ZERO — the same two-sided
                // rule the core fold's own scenario keeps, restated at the wire
                // because a zero on either side is what a vacuous pass looks
                // like here too.
                if got_clean == Some(0) || got_attempted == Some(0) {
                    return Err(format!(
                        "the bridge reports {:?} clean of {:?} attempted — a zero population proves nothing",
                        got_clean, got_attempted
                    ));
                }
                if got_clean == Some(clean) && got_attempted == Some(attempted) {
                    Ok(())
                } else {
                    Err(format!(
                        "the bridge reports {:?} clean of {:?} attempted, expected {} of {}",
                        got_clean, got_attempted, clean, attempted
                    ))
                }
            },
        ),
        check_def(
            "the /ws case names a wrong action in run {string} caught by {string}",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let run_id = params.get_string(0).ok_or("Expected a run id")?;
                let catcher = params.get_string(1).ok_or("Expected a catcher")?;
                let response = ctx.get::<Value>(WS_RESPONSE_KEY).ok_or("No ws response")?;
                let corrections = response
                    .get("result")
                    .and_then(|r| r.get("evidence"))
                    .and_then(|e| e.get("corrections"))
                    .and_then(Value::as_array)
                    .ok_or_else(|| format!("the response carries no corrections list: {}", response))?;
                let hit = corrections.iter().any(|c| {
                    c.get("run_id").and_then(Value::as_str) == Some(run_id)
                        && c.get("caught_by").and_then(Value::as_str) == Some(catcher)
                });
                if hit {
                    Ok(())
                } else {
                    Err(format!(
                        "no wrong action in run '{}' caught by '{}'; the bridge names: {:?}",
                        run_id, catcher, corrections
                    ))
                }
            },
        ),
        check_def(
            "the /ws response says the playbook has no case",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, _params| {
                let response = ctx.get::<Value>(WS_RESPONSE_KEY).ok_or("No ws response")?;
                let result = response
                    .get("result")
                    .ok_or_else(|| format!("Response carries no result: {}", response))?;
                // ABSENT, not an empty case. `0 clean of 0 attempted` would read
                // as a measured perfect failure rather than as no evidence.
                if result.get("found").and_then(Value::as_bool) != Some(false) {
                    return Err(format!("the bridge reports a case: {}", result));
                }
                match result.get("evidence") {
                    Some(Value::Null) => Ok(()),
                    other => Err(format!(
                        "the bridge carries evidence {:?} alongside found=false",
                        other
                    )),
                }
            },
        ),
        check_def(
            "no part of the /ws case carries a cost figure, and the case is not empty",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, _params| {
                let response = ctx.get::<Value>(WS_RESPONSE_KEY).ok_or("No ws response")?;
                let evidence = response
                    .get("result")
                    .and_then(|r| r.get("evidence"))
                    .ok_or_else(|| format!("the response carries no evidence: {}", response))?;
                // THE NON-EMPTY ARM. A response that serialized nothing carries
                // no cost-shaped key either.
                let verdicts = evidence
                    .get("run_verdicts")
                    .and_then(Value::as_array)
                    .map(|v| v.len())
                    .unwrap_or(0);
                if verdicts == 0 {
                    return Err(format!(
                        "the bridge's case carries no run verdicts — nothing was examined: {}",
                        evidence
                    ));
                }
                let mut keys: Vec<String> = Vec::new();
                fn walk(value: &Value, out: &mut Vec<String>) {
                    match value {
                        Value::Object(map) => {
                            for (key, child) in map {
                                out.push(key.clone());
                                walk(child, out);
                            }
                        }
                        Value::Array(items) => items.iter().for_each(|c| walk(c, out)),
                        _ => {}
                    }
                }
                walk(evidence, &mut keys);
                const COST_TOKENS: &[&str] = &[
                    "cost", "spend", "price", "usd", "dollar", "token", "budget", "charge", "bill",
                ];
                let offenders: Vec<&String> = keys
                    .iter()
                    .filter(|k| {
                        let lower = k.to_lowercase();
                        COST_TOKENS.iter().any(|t| lower.contains(t))
                    })
                    .collect();
                if offenders.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "the bridge's case carries cost-shaped key(s) {:?} across {} keys examined",
                        offenders,
                        keys.len()
                    ))
                }
            },
        ),
        check_def(
            "the /ws JSON-RPC response carries a result",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, _params| {
                let response = ctx
                    .get::<Value>(WS_RESPONSE_KEY)
                    .ok_or("No ws_bridge_ws_response")?;
                if let Some(error) = response.get("error") {
                    return Err(format!("Expected a result, got an error: {}", error));
                }
                response
                    .get("result")
                    .map(|_| ())
                    .ok_or_else(|| format!("Response carries no result: {}", response))
            },
        ),
        check_def(
            "the /ws JSON-RPC response carries no result",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, _params| {
                let response = ctx
                    .get::<Value>(WS_RESPONSE_KEY)
                    .ok_or("No ws_bridge_ws_response")?;
                match response.get("result") {
                    None => Ok(()),
                    Some(result) => Err(format!(
                        "A refused request was still served a result: {}",
                        result
                    )),
                }
            },
        ),
        check_def(
            "the ws read seam record for {string} carries no {string} field",
            &[("engine_process", "EngineProcess")],
            |ctx, params| {
                let command = params.get_string(0).ok_or("Expected a command")?.to_string();
                let field = params.get_string(1).ok_or("Expected a field")?.to_string();
                let engine = ctx
                    .get::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;

                // Select the SEAM record, not merely a record that shares the
                // command name. The bridge's methods each emit their own query
                // log under the same `command`, and a check that keys only on
                // the command reads whichever of the two it meets first —
                // which makes the answer depend on log ordering rather than on
                // the seam record's contents.
                let mut seen = 0usize;
                let mut offenders: Vec<String> = Vec::new();
                for line in engine.stderr_lines() {
                    let value: Value = match serde_json::from_str(&line) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let obj = match value.as_object() {
                        Some(o) => o,
                        None => continue,
                    };
                    if obj.get("seam").and_then(Value::as_str) != Some("ws_bridge") {
                        continue;
                    }
                    if obj.get("command").and_then(Value::as_str) != Some(command.as_str()) {
                        continue;
                    }
                    seen += 1;
                    if obj.contains_key(&field) {
                        offenders.push(line.clone());
                    }
                }
                // A prohibition over an empty population prohibits nothing.
                if seen == 0 {
                    return Err(format!(
                        "No ws read seam record for command {:?} was emitted at all — \
                         'carries no {}' would pass over an empty population.",
                        command, field
                    ));
                }
                if offenders.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "{} of {} ws read seam record(s) for {:?} carry {:?}:\n{}",
                        offenders.len(),
                        seen,
                        command,
                        field,
                        offenders.join("\n")
                    ))
                }
            },
        ),
        check_def(
            "the /ws JSON-RPC error.data.code is {string}",
            &[(WS_RESPONSE_KEY, "Value")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected a code")?.to_string();
                let response = ctx
                    .get::<Value>(WS_RESPONSE_KEY)
                    .ok_or("No ws_bridge_ws_response")?;
                let code = response
                    .get("error")
                    .and_then(|e| e.get("data"))
                    .and_then(|d| d.get("code"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!("error.data.code missing from: {}", response))?;
                if code == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected error.data.code {:?}, got {:?}",
                        expected, code
                    ))
                }
            },
        ),
        // -----------------------------------------------------------------
        // Liveness-under-load: prove a heavy, concurrent whole-hearth fold
        // cannot starve the /health accept loop (the production crash mode).
        // -----------------------------------------------------------------
        // Make each whole-hearth fold genuinely heavy by pre-populating the
        // universal activity-log sink (`<hearth>/activity-log.jsonl`) with many
        // records. `playbook_activity` parses and folds the ENTIRE log per call, so
        // a fat log turns every /ws fold into a multi-line synchronous scan — the
        // same shape that grew unbounded as foundry-hearth accumulated turns. Writes
        // the file in one shot (fast to seed, slow to fold).
        step_def(
            "the hearth activity log has {int} route entries for kind {string}",
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let count = params.get_int(0).ok_or("Expected entry count")?;
                let kind = params.get_string(1).ok_or("Expected kind")?.to_string();
                let hearth = ctx
                    .get::<std::path::PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let mut body = String::with_capacity((count as usize).max(1) * 160);
                for i in 0..count {
                    body.push_str(&format!(
                        "{{\"command\":\"route\",\"outcome\":\"single\",\"artifact_kind\":\"{}\",\"from_state\":\"\",\"to_state\":\"\",\"actor_hash\":null,\"at\":\"2026-06-18T12:00:{:02}Z\",\"source\":\"claude-code\"}}\n",
                        kind,
                        i % 60
                    ));
                }
                std::fs::write(hearth.join("activity-log.jsonl"), body)
                    .map_err(|e| format!("write activity-log.jsonl failed: {}", e))?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                if let Some(h) = ctx.get::<anvil_test_support::RetainedTempDir>("hearth_path_handle") {
                    out.set::<anvil_test_support::RetainedTempDir>(
                        "hearth_path_handle",
                        std::sync::Arc::clone(h),
                    );
                }
                Ok(out)
            },
        ),
        // Sustained-load probe: 12 concurrent clients loop `playbook_activity` /ws
        // folds for a fixed window while a separate loop probes /health and records
        // the worst-case latency. On the single-worker engine, an inline fold would
        // pin the lone async worker and stall /health for the whole window; with the
        // fold offloaded to the blocking pool the worker stays free and /health
        // answers immediately. Each probe is wrapped in a 5s timeout so a fully
        // starved endpoint records a failure rather than hanging the test.
        async_step_def(
            "the engine is hammered with concurrent whole-hearth folds while /health is probed",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                (HEALTH_MAX_MS_KEY, "u64"),
                (HEALTH_OK_KEY, "bool"),
                (HEALTH_PROBES_KEY, "u64"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let port = engine.port;

                let deadline =
                    std::time::Instant::now() + std::time::Duration::from_millis(2000);

                // Fire the concurrent fold loaders.
                let mut loaders = Vec::new();
                for _ in 0..12u32 {
                    loaders.push(tokio::spawn(async move {
                        let req = json!({
                            "jsonrpc": "2.0",
                            "id": 1,
                            "method": "playbook_activity",
                            "params": { "surface": "test-harness", "hearth_path": "" }
                        });
                        while std::time::Instant::now() < deadline {
                            let _ = ws_roundtrip(port, &req).await;
                        }
                    }));
                }

                // Let the loaders saturate the runtime before we start probing.
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;

                let mut max_ms: u64 = 0;
                let mut all_ok = true;
                let mut probes: u64 = 0;
                while std::time::Instant::now() < deadline {
                    let start = std::time::Instant::now();
                    let result = tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        http_get_status(port, "/health"),
                    )
                    .await;
                    let elapsed = start.elapsed().as_millis() as u64;
                    probes += 1;
                    match result {
                        Ok(Ok(200)) => {
                            if elapsed > max_ms {
                                max_ms = elapsed;
                            }
                        }
                        _ => {
                            all_ok = false;
                            if elapsed > max_ms {
                                max_ms = elapsed;
                            }
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }

                for loader in loaders {
                    let _ = loader.await;
                }

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set::<u64>(HEALTH_MAX_MS_KEY, max_ms);
                out.set::<bool>(HEALTH_OK_KEY, all_ok);
                out.set::<u64>(HEALTH_PROBES_KEY, probes);
                Ok(out)
            },
        ),
        // Sustained-load probe over the gRPC SURFACE (not /ws). 12 concurrent
        // clients loop gRPC `playbook_activity` (folds the whole 120k-entry
        // activity log) and gRPC `route` (rebuilds the playbook registry every
        // call) against the single-worker engine while a separate loop probes
        // /health. If EITHER gRPC fold runs inline on the lone async worker, that
        // worker is pinned for the fold's duration and /health starves — the exact
        // production crash, reached through the gRPC RPCs instead of /ws. With the
        // folds offloaded (spawn_blocking for the reads, block_in_place for route)
        // the async worker stays free and /health answers immediately. Each probe
        // carries a 5s timeout so a fully starved endpoint records a failure rather
        // than hanging the test.
        async_step_def(
            "the engine is hammered with concurrent gRPC route and playbook_activity calls while /health is probed",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                (HEALTH_MAX_MS_KEY, "u64"),
                (HEALTH_OK_KEY, "bool"),
                (HEALTH_PROBES_KEY, "u64"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let port = engine.port;
                let addr = format!("http://127.0.0.1:{}", port);

                let deadline =
                    std::time::Instant::now() + std::time::Duration::from_millis(2000);

                // Fire the concurrent gRPC loaders: half hammer the heavy
                // whole-log fold (`playbook_activity`), half hammer `route` (the
                // per-turn registry rebuild + open-playbook lookup).
                let mut loaders = Vec::new();
                for i in 0..12u32 {
                    let addr = addr.clone();
                    let hammer_playbook_activity = i % 2 == 0;
                    loaders.push(tokio::spawn(async move {
                        // One connection per loader, reused for every call.
                        let mut client = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                            Ok(c) => c,
                            Err(_) => return,
                        };
                        while std::time::Instant::now() < deadline {
                            if hammer_playbook_activity {
                                let request = anvil_test_support::surfaced(
                                    anvil_engine::proto::PlaybookActivityRequest {
                                        hearth_path: String::new(),
                                        all_hearths: false,
                                        ..Default::default()
                                    },
                                );
                                let _ = client.playbook_activity(request).await;
                            } else {
                                let request = anvil_test_support::surfaced(
                                    anvil_engine::proto::RouteRequest {
                                        hearth_path: String::new(),
                                        message: "answer a question about my notes".to_string(),
                                        ctx_org: "Foundation".to_string(),
                                        ctx_role: "read".to_string(),
                                        ctx_clearance: "internal".to_string(),
                                        ..Default::default()
                                    },
                                );
                                let _ = client.route(request).await;
                            }
                        }
                    }));
                }

                // Let the loaders saturate the runtime before we start probing.
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;

                let mut max_ms: u64 = 0;
                let mut all_ok = true;
                let mut probes: u64 = 0;
                while std::time::Instant::now() < deadline {
                    let start = std::time::Instant::now();
                    let result = tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        http_get_status(port, "/health"),
                    )
                    .await;
                    let elapsed = start.elapsed().as_millis() as u64;
                    probes += 1;
                    match result {
                        Ok(Ok(200)) => {
                            if elapsed > max_ms {
                                max_ms = elapsed;
                            }
                        }
                        _ => {
                            all_ok = false;
                            if elapsed > max_ms {
                                max_ms = elapsed;
                            }
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }

                for loader in loaders {
                    let _ = loader.await;
                }

                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set::<u64>(HEALTH_MAX_MS_KEY, max_ms);
                out.set::<bool>(HEALTH_OK_KEY, all_ok);
                out.set::<u64>(HEALTH_PROBES_KEY, probes);
                Ok(out)
            },
        ),
        check_def(
            "every /health probe returned 200 within the watchdog window",
            &[(HEALTH_OK_KEY, "bool"), (HEALTH_PROBES_KEY, "u64")],
            |ctx, _params| {
                let probes = *ctx.get::<u64>(HEALTH_PROBES_KEY).ok_or("No probe count")?;
                if probes == 0 {
                    return Err("no /health probes were taken under load".to_string());
                }
                let all_ok = *ctx.get::<bool>(HEALTH_OK_KEY).ok_or("No health-ok flag")?;
                if all_ok {
                    Ok(())
                } else {
                    Err(format!(
                        "at least one of {} /health probes failed or timed out under concurrent fold load",
                        probes
                    ))
                }
            },
        ),
        check_def(
            "the max /health latency under load is under {int} ms",
            &[(HEALTH_MAX_MS_KEY, "u64")],
            |ctx, params| {
                let budget = params.get_int(0).ok_or("Expected budget ms")? as u64;
                let max_ms = *ctx
                    .get::<u64>(HEALTH_MAX_MS_KEY)
                    .ok_or("No max latency recorded")?;
                if max_ms < budget {
                    Ok(())
                } else {
                    Err(format!(
                        "worst-case /health latency {}ms exceeded the {}ms budget under concurrent fold load (a heavy fold is starving the /health accept loop)",
                        max_ms, budget
                    ))
                }
            },
        ),
    ]
}
