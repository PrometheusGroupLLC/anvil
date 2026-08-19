//! Step module for `hooks_mcp_registration.feature` (core seam).
//!
//! Drives the pure per-harness MCP-registration string→string transforms over an
//! in-context config-content string. No filesystem: the writer only transforms
//! content, so the steps hold the "config file" as a `String` in context and
//! assert on it directly (mirroring `hooks_adapter`).

use anvil_core::domain::hooks::mcp_registration::{self, McpWriter};
use anvil_core::domain::hooks::Harness;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use serde_json::Value;

const CONFIG_KEY: &str = "mcp_reg_config";

// ---------------------------------------------------------------------------
// Test-only Kiln mcp.json inspectors. These live in the STEP crate (not the
// production `mcp_registration` module): they exist purely to let the feature
// assert on the file the writer produced, so they belong on the test side of
// the seam. Kiln mcp.json is a JSON ARRAY of McpServerSpec { name, command,
// args, env?, cwd? }.
// ---------------------------------------------------------------------------

/// Parse a Kiln mcp.json array (empty/whitespace/malformed → empty array).
fn kiln_servers(existing: &str) -> Vec<Value> {
    if existing.trim().is_empty() {
        return Vec::new();
    }
    match serde_json::from_str::<Value>(existing) {
        Ok(Value::Array(arr)) => arr,
        _ => Vec::new(),
    }
}

/// The whole JSON object of the server element named `name`, or `None`.
fn kiln_server(existing: &str, name: &str) -> Option<Value> {
    kiln_servers(existing)
        .into_iter()
        .find(|s| s.get("name").and_then(Value::as_str) == Some(name))
}

/// Whether a Kiln mcp.json array carries a server element named `name`.
/// (Moved off the production crate — this is a test-only inspector.)
fn kiln_has_server(existing: &str, name: &str) -> bool {
    kiln_server(existing, name).is_some()
}

fn writer(harness: Harness) -> Box<dyn McpWriter> {
    harness
        .mcp_writer()
        .expect("harness has no MCP writer in this test")
}

fn register(ctx: &mut Context, harness: Harness, command: &str) -> Result<(), String> {
    let existing = ctx.take::<String>(CONFIG_KEY).unwrap_or_default();
    let next = writer(harness).register(&existing, command)?;
    ctx.set(CONFIG_KEY, next);
    Ok(())
}

fn unregister(ctx: &mut Context, harness: Harness) -> Result<(), String> {
    let existing = ctx.take::<String>(CONFIG_KEY).unwrap_or_default();
    let next = writer(harness).unregister(&existing)?;
    ctx.set(CONFIG_KEY, next);
    Ok(())
}

fn carry(ctx: &mut Context, out: &mut Context) {
    if let Some(c) = ctx.take::<String>(CONFIG_KEY) {
        out.set(CONFIG_KEY, c);
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ---- Given ----
        step_def(
            "an empty Claude Code user MCP config",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, _p| {
                let mut out = Context::new();
                out.set(CONFIG_KEY, String::new());
                Ok(out)
            },
        ),
        step_def(
            "a Claude Code user MCP config with a stale anvil entry and an unrelated server {string}",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, params| {
                let other = params.get_string(0).ok_or("Expected server name")?;
                let json = serde_json::json!({
                    "mcpServers": {
                        "anvil": { "command": "/Users/someoneelse/target/debug/anvil-mcp", "type": "stdio" },
                        other: { "command": "/usr/bin/other-mcp", "type": "stdio" }
                    }
                });
                let mut out = Context::new();
                out.set(CONFIG_KEY, serde_json::to_string_pretty(&json).unwrap());
                Ok(out)
            },
        ),
        step_def(
            "an empty Codex MCP config file",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, _p| {
                let mut out = Context::new();
                out.set(CONFIG_KEY, String::new());
                Ok(out)
            },
        ),
        step_def(
            "a realistic Codex config with a comment, an unrelated key, and a stale mcp_servers anvil-mcp entry",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, _p| {
                let cfg = concat!(
                    "# Codex hand-maintained config (keep my comments!)\n",
                    "approval_policy = \"on-request\"\n",
                    "\n",
                    "[mcp_servers.anvil-mcp]\n",
                    "command = \"/Users/someoneelse/target/debug/anvil-mcp\"\n",
                    "\n",
                    "[mcp_servers.other-mcp]\n",
                    "command = \"/usr/bin/other-mcp\"\n",
                );
                let mut out = Context::new();
                out.set(CONFIG_KEY, cfg.to_string());
                Ok(out)
            },
        ),
        step_def(
            "an empty Kiln MCP config file",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, _p| {
                let mut out = Context::new();
                out.set(CONFIG_KEY, String::new());
                Ok(out)
            },
        ),
        step_def(
            "a Kiln MCP config with a stale anvil entry and an unrelated server {string}",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, params| {
                let other = params.get_string(0).ok_or("Expected server name")?;
                // Kiln mcp.json is a JSON ARRAY of McpServerSpec { name, command, args, env?, cwd? }.
                let json = serde_json::json!([
                    { "name": "anvil", "command": "/Users/someoneelse/target/debug/anvil-mcp", "args": [] },
                    { "name": other, "command": "/usr/bin/other-mcp", "args": [] }
                ]);
                let mut out = Context::new();
                out.set(CONFIG_KEY, serde_json::to_string_pretty(&json).unwrap());
                Ok(out)
            },
        ),
        // ---- When ----
        register_step(
            "the Claude Code MCP writer registers anvil with command {string}",
            Harness::ClaudeCode,
        ),
        register_step(
            "the Codex MCP writer registers anvil with command {string}",
            Harness::Codex,
        ),
        register_step(
            "the Kiln MCP writer registers anvil with command {string}",
            Harness::Kiln,
        ),
        unregister_step("the Claude Code MCP writer unregisters anvil", Harness::ClaudeCode),
        unregister_step("the Codex MCP writer unregisters anvil", Harness::Codex),
        unregister_step("the Kiln MCP writer unregisters anvil", Harness::Kiln),
        // ---- Then: Claude Code ----
        check_def(
            "the Claude Code MCP config has an anvil server with command {string}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected command")?;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let got = mcp_registration::claude_anvil_command(cfg)
                    .ok_or("No anvil MCP server")?;
                if got == want {
                    Ok(())
                } else {
                    Err(format!("command: expected {}, got {}", want, got))
                }
            },
        ),
        check_def(
            "the Claude Code MCP config anvil server transport is {string}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected transport")?;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let got = mcp_registration::claude_anvil_transport(cfg)
                    .ok_or("No anvil MCP transport")?;
                if got == want {
                    Ok(())
                } else {
                    Err(format!("transport: expected {}, got {}", want, got))
                }
            },
        ),
        check_def(
            "the Claude Code MCP config has exactly {int} anvil server",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_int(0).ok_or("Expected count")? as usize;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let got = mcp_registration::claude_anvil_count(cfg);
                if got == want {
                    Ok(())
                } else {
                    Err(format!("anvil count: expected {}, got {}", want, got))
                }
            },
        ),
        check_def(
            "the Claude Code MCP config still has server {string}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected server")?;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                if mcp_registration::claude_has_server(cfg, name) {
                    Ok(())
                } else {
                    Err(format!("server {} was lost in:\n{}", name, cfg))
                }
            },
        ),
        // ---- Then: Codex ----
        check_def(
            "the Codex config has an mcp_servers anvil-mcp entry with command {string}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected command")?;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let got = mcp_registration::codex_anvil_command(cfg)
                    .ok_or("No anvil-mcp entry")?;
                if got == want {
                    Ok(())
                } else {
                    Err(format!("command: expected {}, got {}", want, got))
                }
            },
        ),
        check_def(
            "the Codex config has exactly {int} mcp_servers anvil-mcp entry",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_int(0).ok_or("Expected count")? as usize;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let got = mcp_registration::codex_anvil_count(cfg);
                if got == want {
                    Ok(())
                } else {
                    Err(format!("anvil-mcp count: expected {}, got {}", want, got))
                }
            },
        ),
        check_def(
            "the Codex config still has the leading comment",
            &[(CONFIG_KEY, "String")],
            |ctx, _p| {
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                if cfg.contains("# Codex hand-maintained config (keep my comments!)") {
                    Ok(())
                } else {
                    Err(format!("leading comment was lost in:\n{}", cfg))
                }
            },
        ),
        check_def(
            "the Codex config still has key {string} value {string}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected key")?;
                let val = params.get_string(1).ok_or("Expected value")?;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let needle = format!("{} = \"{}\"", key, val);
                if cfg.contains(&needle) {
                    Ok(())
                } else {
                    Err(format!("missing line '{}' in:\n{}", needle, cfg))
                }
            },
        ),
        // ---- Then: Kiln ----
        check_def(
            "the Kiln MCP config has an anvil server with command {string}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected command")?;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let got = mcp_registration::kiln_anvil_command(cfg)
                    .ok_or("No anvil MCP server")?;
                if got == want {
                    Ok(())
                } else {
                    Err(format!("command: expected {}, got {}", want, got))
                }
            },
        ),
        check_def(
            "the Kiln MCP config has exactly {int} anvil server",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_int(0).ok_or("Expected count")? as usize;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let got = mcp_registration::kiln_anvil_count(cfg);
                if got == want {
                    Ok(())
                } else {
                    Err(format!("anvil count: expected {}, got {}", want, got))
                }
            },
        ),
        check_def(
            "the Kiln MCP config still has server {string}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected server")?;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                if kiln_has_server(cfg, name) {
                    Ok(())
                } else {
                    Err(format!("server {} was lost in:\n{}", name, cfg))
                }
            },
        ),
        // The RAW `command` field of the anvil element, read straight from the
        // mcp.json (NOT the recombined `kiln_anvil_command` — that would let a
        // wrong split like combined-command + empty-args pass).
        check_def(
            "the Kiln MCP config anvil server command field is {string}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected command")?;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let entry = kiln_server(cfg, "anvil").ok_or("No anvil MCP server")?;
                let got = entry
                    .get("command")
                    .and_then(Value::as_str)
                    .ok_or("anvil element has no string command field")?;
                if got == want {
                    Ok(())
                } else {
                    Err(format!("raw command field: expected {}, got {}", want, got))
                }
            },
        ),
        // The EXACT `args` array of the anvil element (order + count + values),
        // asserted separately from the command. `{string}` is a comma-joined
        // list; empty string means an empty array.
        check_def(
            "the Kiln MCP config anvil server args are {string}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want_csv = params.get_string(0).ok_or("Expected args")?;
                let want: Vec<&str> = if want_csv.is_empty() {
                    Vec::new()
                } else {
                    want_csv.split(',').collect()
                };
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let entry = kiln_server(cfg, "anvil").ok_or("No anvil MCP server")?;
                let got: Vec<&str> = entry
                    .get("args")
                    .and_then(Value::as_array)
                    .ok_or("anvil element has no args array")?
                    .iter()
                    .map(|v| v.as_str().unwrap_or("<non-string>"))
                    .collect();
                if got == want {
                    Ok(())
                } else {
                    Err(format!("args array: expected {:?}, got {:?}", want, got))
                }
            },
        ),
        // Byte-identity idempotency: registering from empty once vs twice must
        // produce the SAME file bytes (not merely the same anvil count).
        check_def(
            "registering Kiln anvil twice with command {string} is byte-identical to once",
            &[],
            |_ctx, params| {
                let command = params.get_string(0).ok_or("Expected command")?;
                let w = writer(Harness::Kiln);
                let once = w.register("", command)?;
                let twice = w.register(&once, command)?;
                if once == twice {
                    Ok(())
                } else {
                    Err(format!(
                        "register once != twice:\n--- once ---\n{}\n--- twice ---\n{}",
                        once, twice
                    ))
                }
            },
        ),
        // Deep-equal preservation: the foreign server's WHOLE JSON object must
        // survive an anvil register byte-for-byte (not just its name).
        check_def(
            "the Kiln MCP config still has server {string} unchanged",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected server")?;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let got = kiln_server(cfg, name)
                    .ok_or_else(|| format!("server {} was lost in:\n{}", name, cfg))?;
                // The verbatim object seeded by the "stale anvil entry and an
                // unrelated server" Given.
                let want = serde_json::json!({
                    "name": name,
                    "command": "/usr/bin/other-mcp",
                    "args": [],
                });
                if got == want {
                    Ok(())
                } else {
                    Err(format!(
                        "server {} changed: expected {}, got {}",
                        name, want, got
                    ))
                }
            },
        ),
    ]
}

fn register_step(pattern: &'static str, harness: Harness) -> StepDef {
    step_def(
        pattern,
        &[],
        &[(CONFIG_KEY, "String")],
        move |mut ctx, params| {
            let command = params.get_string(0).ok_or("Expected command")?.to_string();
            register(&mut ctx, harness, &command)?;
            let mut out = Context::new();
            carry(&mut ctx, &mut out);
            Ok(out)
        },
    )
}

fn unregister_step(pattern: &'static str, harness: Harness) -> StepDef {
    step_def(
        pattern,
        &[(CONFIG_KEY, "String")],
        &[(CONFIG_KEY, "String")],
        move |mut ctx, _params| {
            unregister(&mut ctx, harness)?;
            let mut out = Context::new();
            carry(&mut ctx, &mut out);
            Ok(out)
        },
    )
}
