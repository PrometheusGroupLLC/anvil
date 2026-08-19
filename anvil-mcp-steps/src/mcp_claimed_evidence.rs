//! Real-shim steps for `mcp_complete_claimed_evidence.feature` (T-ACT-2,
//! phases P1/P2/P4 — MCP leg).
//!
//! Drives the REAL `anvil-mcp` shim over JSON-RPC stdio (via the existing
//! `mcp` module's hearth/shim/session Given steps, plus the
//! `mcp::claimed_evidence_tools_call` seam for sending this module's own
//! `complete` tools/call shape) against a REAL running engine over a
//! `track_lifecycle` hearth that carries NO `playbooks/` directory, so the
//! engine resolves the machine via `SeedPlaybookRegistry` fallback and
//! assesses against T-ACT-1's live `spec`/doer → `[artifact_of_consequence]`
//! and `plan`/doer → `[verifiable_citation, artifact_of_consequence]`
//! obligations — exactly the same real obligation the CLI-leg module
//! (`anvil_hooks_claimed_evidence.rs`) exercises. The MCP `complete` tool's
//! `claimed_evidence` argument is the only lever the caller pulls; the
//! emitted row on `<hearth>/step-measurement.jsonl` is read back with
//! deadline polling (the durable write is dispatched asynchronously).

use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, Params, StepDef};
use serde_json::Value;
use std::path::{Path, PathBuf};

const ACTOR_DOER: &str = "Doer-CE-MCP-100000";
const ACTOR_REVIEWER: &str = "Reviewer-CE-MCP-200000";

#[derive(Clone)]
struct CeMcpOutcome {
    rows: Vec<Value>,
}

/// Deadline-poll `<hearth>/step-measurement.jsonl` for at least one parsed row
/// (mirrors `anvil_hooks_claimed_evidence.rs::poll_sink` exactly — the
/// durable evidence write is dispatched asynchronously by the engine).
fn poll_sink(hearth: &Path) -> Vec<Value> {
    let path = hearth.join("step-measurement.jsonl");
    for _ in 0..150 {
        if let Ok(bytes) = std::fs::read(&path) {
            if !bytes.is_empty() {
                if let Ok(text) = std::str::from_utf8(&bytes) {
                    let rows: Vec<Value> = text
                        .lines()
                        .filter(|l| !l.trim().is_empty())
                        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                        .collect();
                    if !rows.is_empty() {
                        return rows;
                    }
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Vec::new()
}

fn parse_pairs(params: &Params) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    if let Some(table) = params.data_table() {
        if table.headers.len() >= 2 && table.headers[0].trim() != "class" {
            pairs.push((
                table.headers[0].trim().to_string(),
                table.headers[1].trim().to_string(),
            ));
        }
        for row in &table.rows {
            if row.len() >= 2 {
                pairs.push((row[0].trim().to_string(), row[1].trim().to_string()));
            }
        }
    }
    pairs
}

fn csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn render(rows: &[Value]) -> String {
    rows.iter().map(Value::to_string).collect::<Vec<_>>().join("\n")
}

/// Build the fixed actor identity fields shared by every claimed-evidence
/// `complete` tools/call in this module (doer or reviewer).
fn base_arguments(artifact_path: &str, actor: &str) -> serde_json::Map<String, Value> {
    let mut arguments = serde_json::Map::new();
    arguments.insert("artifact_path".to_string(), Value::String(artifact_path.to_string()));
    arguments.insert("actor_name".to_string(), Value::String(actor.to_string()));
    arguments.insert("actor_type".to_string(), Value::String("agent".to_string()));
    arguments.insert("actor_model".to_string(), Value::String("claude-sonnet-4-6".to_string()));
    arguments.insert("actor_provider".to_string(), Value::String("anthropic".to_string()));
    arguments.insert(
        "actor_context_window".to_string(),
        Value::Number(200000.into()),
    );
    arguments.insert("actor_entrypoint".to_string(), Value::String("claude-code".to_string()));
    arguments
}

fn claimed_evidence_array(pairs: &[(String, String)]) -> Value {
    Value::Array(
        pairs
            .iter()
            .map(|(class, reference)| {
                serde_json::json!({ "class": class, "reference": reference })
            })
            .collect(),
    )
}

/// Send a `complete` tools/call carrying the built arguments through the
/// `mcp` module's real-process seam, then deadline-poll the durable evidence
/// sink for the resulting row(s).
fn send_and_collect(
    ctx: Context,
    arguments: serde_json::Map<String, Value>,
) -> Result<Context, String> {
    let hearth = ctx.get::<PathBuf>("hearth_path").cloned().ok_or("No hearth_path")?;
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 188,
        "method": "tools/call",
        "params": {
            "name": "complete",
            "arguments": Value::Object(arguments)
        }
    });
    let (mut out, response) = crate::mcp::claimed_evidence_tools_call(ctx, &request)?;
    let rows = poll_sink(&hearth);

    out.set("hearth_path", hearth);
    out.set("complete_response", response);
    out.set("ce_mcp_outcome", CeMcpOutcome { rows });
    Ok(out)
}

fn outcome(ctx: &Context) -> Result<&CeMcpOutcome, String> {
    ctx.get::<CeMcpOutcome>("ce_mcp_outcome")
        .ok_or_else(|| "No ce_mcp_outcome".to_string())
}

fn evidence_rows(o: &CeMcpOutcome) -> Vec<&Value> {
    o.rows.iter().filter(|r| r.get("evidence_status").is_some()).collect()
}

fn one_evidence_row(o: &CeMcpOutcome) -> Result<&Value, String> {
    let rows = evidence_rows(o);
    if rows.len() == 1 {
        Ok(rows[0])
    } else {
        Err(format!(
            "expected exactly one evidence row, got {} (all rows: {})",
            rows.len(),
            render(&o.rows)
        ))
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        check_def(
            "the {string} tool does not require field {string}",
            &[("response", "JsonValue")],
            |ctx, params| {
                let tool_name = params.get_string(0).ok_or("Expected tool name")?;
                let field = params.get_string(1).ok_or("Expected field")?;
                let response = ctx
                    .get::<serde_json::Value>("response")
                    .ok_or("No response")?;
                let tools = response["result"]["tools"]
                    .as_array()
                    .ok_or("Missing result.tools array")?;
                let tool = tools
                    .iter()
                    .find(|t| t["name"].as_str() == Some(tool_name))
                    .ok_or_else(|| format!("No tool named '{}'", tool_name))?;
                let required = tool["inputSchema"]["required"].as_array();
                let is_required = required
                    .map(|r| r.iter().any(|v| v.as_str() == Some(field)))
                    .unwrap_or(false);
                if is_required {
                    Err(format!(
                        "Tool '{}' unexpectedly requires '{}'. Required: {:?}",
                        tool_name, field, required
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        // ---- When: doer completes via MCP presenting claims from a class|reference table ----
        step_def(
            "a complete tools/call is sent for {string} as doer presenting claims:",
            &[("mcp_process", "McpProcess"), ("hearth_path", "PathBuf")],
            &[
                ("mcp_process", "McpProcess"),
                ("hearth_path", "PathBuf"),
                ("complete_response", "JsonValue"),
                ("ce_mcp_outcome", "CeMcpOutcome"),
            ],
            |ctx, params| {
                let artifact_path = params.get_string(0).ok_or("Expected artifact_path")?;
                let mut arguments = base_arguments(&artifact_path, ACTOR_DOER);
                let pairs = parse_pairs(&params);
                arguments.insert("claimed_evidence".to_string(), claimed_evidence_array(&pairs));
                send_and_collect(ctx, arguments)
            },
        ),
        step_def(
            "a complete tools/call is sent for {string} as doer presenting no claims",
            &[("mcp_process", "McpProcess"), ("hearth_path", "PathBuf")],
            &[
                ("mcp_process", "McpProcess"),
                ("hearth_path", "PathBuf"),
                ("complete_response", "JsonValue"),
                ("ce_mcp_outcome", "CeMcpOutcome"),
            ],
            |ctx, params| {
                let artifact_path = params.get_string(0).ok_or("Expected artifact_path")?;
                let arguments = base_arguments(&artifact_path, ACTOR_DOER);
                send_and_collect(ctx, arguments)
            },
        ),
        step_def(
            "a complete tools/call is sent for {string} as doer with note {string} presenting claims:",
            &[("mcp_process", "McpProcess"), ("hearth_path", "PathBuf")],
            &[
                ("mcp_process", "McpProcess"),
                ("hearth_path", "PathBuf"),
                ("complete_response", "JsonValue"),
                ("ce_mcp_outcome", "CeMcpOutcome"),
            ],
            |ctx, params| {
                let artifact_path = params.get_string(0).ok_or("Expected artifact_path")?;
                let note = params.get_string(1).ok_or("Expected note")?;
                let mut arguments = base_arguments(&artifact_path, ACTOR_DOER);
                arguments.insert("note".to_string(), Value::String(note.to_string()));
                let pairs = parse_pairs(&params);
                arguments.insert("claimed_evidence".to_string(), claimed_evidence_array(&pairs));
                send_and_collect(ctx, arguments)
            },
        ),
        step_def(
            "a complete tools/call is sent for {string} as reviewer satisfied presenting no claims",
            &[("mcp_process", "McpProcess"), ("hearth_path", "PathBuf")],
            &[
                ("mcp_process", "McpProcess"),
                ("hearth_path", "PathBuf"),
                ("complete_response", "JsonValue"),
                ("ce_mcp_outcome", "CeMcpOutcome"),
            ],
            |ctx, params| {
                let artifact_path = params.get_string(0).ok_or("Expected artifact_path")?;
                let mut arguments = base_arguments(&artifact_path, ACTOR_REVIEWER);
                arguments.insert("satisfaction".to_string(), Value::String("satisfied".to_string()));
                send_and_collect(ctx, arguments)
            },
        ),
        // ---- Then: evidence-row assertions (mirror the CLI-leg checks exactly) ----
        check_def(
            "the emitted evidence row records status {string}",
            &[("ce_mcp_outcome", "CeMcpOutcome")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected status")?;
                let o = outcome(&ctx)?;
                let row = one_evidence_row(o)?;
                let actual = row.get("evidence_status").and_then(Value::as_str).unwrap_or("");
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("expected status '{}', got '{}' in {}", expected, actual, row))
                }
            },
        ),
        check_def(
            "the emitted evidence row lists claims in order:",
            &[("ce_mcp_outcome", "CeMcpOutcome")],
            |ctx, params| {
                let expected = parse_pairs(&params);
                let o = outcome(&ctx)?;
                let row = one_evidence_row(o)?;
                let claims = row
                    .get("claimed_evidence")
                    .and_then(Value::as_array)
                    .ok_or_else(|| format!("no claimed_evidence array in {}", row))?;
                if claims.len() != expected.len() {
                    return Err(format!(
                        "expected {} claims, got {} in {}",
                        expected.len(),
                        claims.len(),
                        row
                    ));
                }
                for (i, (class, reference)) in expected.iter().enumerate() {
                    let ac = claims[i].get("class").and_then(Value::as_str).unwrap_or("");
                    let ar = claims[i].get("reference").and_then(Value::as_str).unwrap_or("");
                    if ac != class || ar != reference {
                        return Err(format!(
                            "claim[{}] expected {}:{}, got {}:{}",
                            i, class, reference, ac, ar
                        ));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "the emitted evidence row names missing classes {string}",
            &[("ce_mcp_outcome", "CeMcpOutcome")],
            |ctx, params| {
                let expected = csv(&params.get_string(0).ok_or("Expected missing classes")?);
                let o = outcome(&ctx)?;
                let row = one_evidence_row(o)?;
                let actual: Vec<String> = row
                    .get("missing_evidence_classes")
                    .and_then(Value::as_array)
                    .ok_or_else(|| format!("no missing_evidence_classes in {}", row))?
                    .iter()
                    .filter_map(|v| v.as_str().map(ToString::to_string))
                    .collect();
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("expected missing {:?}, got {:?} in {}", expected, actual, row))
                }
            },
        ),
        check_def(
            "the emitted evidence row carries the track_lifecycle seed content version",
            &[("ce_mcp_outcome", "CeMcpOutcome")],
            |ctx, _params| {
                let expected = anvil_core::domain::playbook_version::machine_content_version(
                    anvil_core::domain::playbook::seeds::track_seed(),
                )
                .ok_or("track seed had no content version")?;
                let o = outcome(&ctx)?;
                let row = one_evidence_row(o)?;
                let actual = row.get("playbook_version").and_then(Value::as_str).unwrap_or("");
                if actual.is_empty() {
                    return Err(format!("empty playbook_version in {}", row));
                }
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "expected seed version '{}', got '{}' in {}",
                        expected, actual, row
                    ))
                }
            },
        ),
        check_def(
            "no emitted step measurement row carries any evidence key",
            &[("ce_mcp_outcome", "CeMcpOutcome")],
            |ctx, _params| {
                let o = outcome(&ctx)?;
                let keys = [
                    "evidence_status",
                    "missing_evidence_classes",
                    "claimed_evidence",
                    "playbook_version",
                ];
                let offending: Vec<&Value> = o
                    .rows
                    .iter()
                    .filter(|r| keys.iter().any(|k| r.get(*k).is_some()))
                    .collect();
                if offending.is_empty() {
                    Ok(())
                } else {
                    Err(format!("rows carry evidence keys: {}", render(&o.rows)))
                }
            },
        ),
        check_def(
            "the emitted evidence row reference contains {string}",
            &[("ce_mcp_outcome", "CeMcpOutcome")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let o = outcome(&ctx)?;
                let row = one_evidence_row(o)?;
                let found = row
                    .get("claimed_evidence")
                    .and_then(Value::as_array)
                    .map(|claims| {
                        claims.iter().any(|c| {
                            c.get("reference").and_then(Value::as_str) == Some(needle.as_ref())
                        })
                    })
                    .unwrap_or(false);
                if found {
                    Ok(())
                } else {
                    Err(format!("no claim reference equal to '{}' in {}", needle, row))
                }
            },
        ),
        check_def(
            "no emitted step measurement row contains the text {string}",
            &[("ce_mcp_outcome", "CeMcpOutcome")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let o = outcome(&ctx)?;
                let raw = render(&o.rows);
                if raw.contains(needle.as_ref() as &str) {
                    Err(format!("raw text '{}' leaked into a measurement row: {}", needle, raw))
                } else {
                    Ok(())
                }
            },
        ),
    ]
}
