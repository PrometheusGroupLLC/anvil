//! Per-harness MCP-server REGISTRATION — the installable side of wiring the anvil
//! MCP server into each agent harness with a VERSION-STABLE command.
//!
//! The anvil MCP server is a KIT component. Today it is hand-wired per project to
//! direct/stale/foreign binary paths; this module lets the installer register it
//! into each harness's native MCP config exactly like the hooks: an anvil-managed
//! `anvil` server entry, idempotent (re-install refreshes the command, never
//! duplicates) and reversible (uninstall removes ONLY the anvil entry, preserving
//! every other MCP server + config content/comments).
//!
//! The transports differ by harness:
//!   - Claude Code: JSON `mcpServers.anvil` in the USER config (`~/.claude.json`).
//!     We parse/serialize the whole JSON document (round-trip-safe) and edit ONLY
//!     the `anvil` key, leaving every other server + key intact.
//!   - Codex: TOML `[mcp_servers.anvil-mcp]` in `config.toml`. The user maintains
//!     this file by hand (comments, ordering), so we NEVER round-trip it: we
//!     surgically replace/remove ONLY the `[mcp_servers.anvil-mcp]` table block.
//!   - Kiln: JSON ARRAY of `McpServerSpec` in `~/.kiln/mcp.json` (`{ name,
//!     command, args, env?, cwd? }`). We round-trip the whole array and edit ONLY
//!     the element named `anvil`, splitting the command into Kiln's `command` +
//!     `args` shape (`kiln-tools` reads this file at session-process startup).
//!   - Hermes: no discoverable MCP-server config → no writer (skipped).
//!
//! The registration is just command + stdio transport. The foundry_session auth
//! is enforced engine-side via the session token in env; there is no auth field
//! in the registration itself.

use super::{Harness, MANAGED_KEY};
use serde_json::{json, Map, Value};

/// The server id anvil registers under in JSON-config harnesses (Claude Code /
/// Desktop / opencode).
pub const ANVIL_SERVER_ID: &str = "anvil";
/// The TOML table id anvil registers under in Codex (`[mcp_servers.anvil-mcp]`).
pub const CODEX_SERVER_ID: &str = "anvil-mcp";
/// The TOML table id anvil registers under in Grok (`[mcp_servers.anvil]`).
pub const GROK_SERVER_ID: &str = "anvil";

/// A per-harness MCP-server registration writer: pure config-content transforms.
/// `register` injects/refreshes the anvil server entry with `command`; `unregister`
/// removes ONLY the anvil entry. The binary owns reading/writing the file.
pub trait McpWriter {
    /// Return `existing` config content with the anvil MCP server registered (or
    /// its command refreshed) using `command` and stdio transport. Any prior anvil
    /// entry is replaced (idempotent). `existing` empty means a fresh config file.
    fn register(&self, existing: &str, command: &str) -> Result<String, String>;

    /// Return `existing` config content with the anvil MCP server entry removed and
    /// every other server + unrelated content preserved.
    fn unregister(&self, existing: &str) -> Result<String, String>;
}

impl Harness {
    /// This harness's MCP-server registration writer, or `None` when the harness
    /// has no discoverable MCP-server config (only Hermes is skipped).
    pub fn mcp_writer(self) -> Option<Box<dyn McpWriter>> {
        match self {
            Harness::ClaudeCode => Some(Box::new(JsonMcpWriter)),
            Harness::Codex => Some(Box::new(CodexMcpWriter)),
            // Grok shares the TOML `[mcp_servers.<id>]` shape (table id `anvil`);
            // opencode uses a JSON `mcp.anvil` local-transport entry.
            Harness::Grok => Some(Box::new(GrokMcpWriter)),
            Harness::OpenCode => Some(Box::new(OpenCodeMcpWriter)),
            // Kiln reads `~/.kiln/mcp.json` — a JSON ARRAY of McpServerSpec.
            Harness::Kiln => Some(Box::new(KilnMcpWriter)),
            Harness::Hermes => None,
        }
    }

    /// The config FILE name (relative to the config dir) this harness's MCP writer
    /// reads and writes. `None` when the harness has no MCP writer.
    ///
    /// Claude Code's MCP servers live in the USER config `.claude.json` (one entry
    /// covers all projects) — distinct from the hook `settings.json`. Codex's MCP
    /// servers stay in `config.toml`, while its hooks now live in a SEPARATE
    /// `hooks.json` (verified against codex >= 0.144) — they no longer share a file.
    pub fn mcp_config_filename(self) -> Option<&'static str> {
        match self {
            Harness::ClaudeCode => Some(".claude.json"),
            Harness::Codex => Some("config.toml"),
            // Grok + opencode register MCP into the SAME config file their hook
            // adapter writes (TOML / JSONC respectively).
            Harness::Grok => Some("config.toml"),
            Harness::OpenCode => Some("opencode.jsonc"),
            // Kiln's MCP servers live in a SEPARATE file from its hooks.json.
            Harness::Kiln => Some("mcp.json"),
            Harness::Hermes => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Claude Code / Desktop — JSON `mcpServers.anvil` (round-trip-safe).
// ---------------------------------------------------------------------------

/// The JSON MCP writer used by Claude Code (and Claude Desktop): edits ONLY the
/// `mcpServers.anvil` key of the whole JSON document, preserving every other
/// server + top-level key.
pub struct JsonMcpWriter;

/// Parse a JSON MCP config, treating empty/whitespace as an empty object.
fn parse_json(existing: &str) -> Result<Map<String, Value>, String> {
    if existing.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(existing) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err("MCP config is not a JSON object".to_string()),
        Err(e) => Err(format!("Failed to parse MCP config: {}", e)),
    }
}

impl McpWriter for JsonMcpWriter {
    fn register(&self, existing: &str, command: &str) -> Result<String, String> {
        let mut config = parse_json(existing)?;
        let servers = config
            .entry("mcpServers")
            .or_insert_with(|| Value::Object(Map::new()));
        let servers_map = match servers.as_object_mut() {
            Some(m) => m,
            None => {
                *servers = Value::Object(Map::new());
                servers.as_object_mut().unwrap()
            }
        };
        // Replace any existing anvil entry (correcting stale/foreign paths).
        servers_map.insert(
            ANVIL_SERVER_ID.to_string(),
            json!({
                MANAGED_KEY: true,
                "command": command,
                "type": "stdio",
            }),
        );
        serde_json::to_string_pretty(&Value::Object(config))
            .map_err(|e| format!("Failed to serialize MCP config: {}", e))
    }

    fn unregister(&self, existing: &str) -> Result<String, String> {
        let mut config = parse_json(existing)?;
        if let Some(servers) = config.get_mut("mcpServers").and_then(Value::as_object_mut) {
            servers.remove(ANVIL_SERVER_ID);
            // Drop an emptied mcpServers object so uninstall round-trips cleanly.
            if servers.is_empty() {
                config.remove("mcpServers");
            }
        }
        serde_json::to_string_pretty(&Value::Object(config))
            .map_err(|e| format!("Failed to serialize MCP config: {}", e))
    }
}

/// The `command` of the anvil MCP server in a JSON MCP config, or `None`.
pub fn claude_anvil_command(existing: &str) -> Option<String> {
    let config = parse_json(existing).ok()?;
    config
        .get("mcpServers")?
        .get(ANVIL_SERVER_ID)?
        .get("command")?
        .as_str()
        .map(str::to_string)
}

/// The `type`/transport of the anvil MCP server in a JSON MCP config, or `None`.
pub fn claude_anvil_transport(existing: &str) -> Option<String> {
    let config = parse_json(existing).ok()?;
    let entry = config.get("mcpServers")?.get(ANVIL_SERVER_ID)?;
    entry
        .get("type")
        .or_else(|| entry.get("transport"))?
        .as_str()
        .map(str::to_string)
}

/// The count of anvil MCP server entries (0 or 1) in a JSON MCP config.
pub fn claude_anvil_count(existing: &str) -> usize {
    parse_json(existing)
        .ok()
        .and_then(|c| {
            c.get("mcpServers")
                .and_then(Value::as_object)
                .map(|m| usize::from(m.contains_key(ANVIL_SERVER_ID)))
        })
        .unwrap_or(0)
}

/// Whether a JSON MCP config carries a server named `name`.
pub fn claude_has_server(existing: &str, name: &str) -> bool {
    parse_json(existing)
        .ok()
        .and_then(|c| {
            c.get("mcpServers")
                .and_then(Value::as_object)
                .map(|m| m.contains_key(name))
        })
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Codex / Grok — TOML `[mcp_servers.<id>]` (comment-preserving surgical edit).
// ---------------------------------------------------------------------------

const CODEX_TABLE_HEADER: &str = "[mcp_servers.anvil-mcp]";
const GROK_TABLE_HEADER: &str = "[mcp_servers.anvil]";

/// A TOML MCP writer parameterized by its `[mcp_servers.<id>]` table header. It
/// surgically replaces/removes ONLY that table block in `config.toml`, preserving
/// every other line, table, and comment. Shared by Codex and Grok, which differ
/// only in the server id.
pub struct TomlTableMcpWriter {
    header: &'static str,
}

impl McpWriter for TomlTableMcpWriter {
    fn register(&self, existing: &str, command: &str) -> Result<String, String> {
        // Strip any prior anvil table first (idempotent), then append a fresh
        // one. Appending keeps the surgical edit simple and order-stable for the
        // user's existing tables.
        let base = strip_toml_table(existing, self.header);
        let mut out = base.trim_end_matches('\n').to_string();
        if !out.is_empty() {
            out.push('\n');
            out.push('\n');
        }
        out.push_str(self.header);
        out.push('\n');
        out.push_str(&format!("command = \"{}\"\n", command));
        Ok(out)
    }

    fn unregister(&self, existing: &str) -> Result<String, String> {
        let stripped = strip_toml_table(existing, self.header);
        // Preserve a trailing newline iff there is remaining content.
        let trimmed = stripped.trim_end_matches('\n');
        if trimmed.is_empty() {
            Ok(String::new())
        } else {
            Ok(format!("{}\n", trimmed))
        }
    }
}

/// The Codex MCP writer: `[mcp_servers.anvil-mcp]` in `config.toml`.
pub struct CodexMcpWriter;

impl McpWriter for CodexMcpWriter {
    fn register(&self, existing: &str, command: &str) -> Result<String, String> {
        TomlTableMcpWriter {
            header: CODEX_TABLE_HEADER,
        }
        .register(existing, command)
    }
    fn unregister(&self, existing: &str) -> Result<String, String> {
        TomlTableMcpWriter {
            header: CODEX_TABLE_HEADER,
        }
        .unregister(existing)
    }
}

/// The Grok MCP writer: `[mcp_servers.anvil]` in `config.toml`.
pub struct GrokMcpWriter;

impl McpWriter for GrokMcpWriter {
    fn register(&self, existing: &str, command: &str) -> Result<String, String> {
        TomlTableMcpWriter {
            header: GROK_TABLE_HEADER,
        }
        .register(existing, command)
    }
    fn unregister(&self, existing: &str) -> Result<String, String> {
        TomlTableMcpWriter {
            header: GROK_TABLE_HEADER,
        }
        .unregister(existing)
    }
}

/// Drop a `[mcp_servers.<id>]` table block (its `header` through the line before
/// the next table header / EOF), plus a single trailing blank separator, from
/// line-oriented TOML. Every other line/comment/table is preserved.
fn strip_toml_table(existing: &str, header: &str) -> String {
    let lines: Vec<&str> = existing.lines().collect();
    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim() == header {
            // Drop a single blank line we may have inserted before the header.
            if matches!(out.last(), Some(l) if l.trim().is_empty()) {
                out.pop();
            }
            // Skip the header and its body up to the next table header / EOF.
            i += 1;
            while i < lines.len() && !is_table_header(lines[i]) {
                i += 1;
            }
            continue;
        }
        out.push(lines[i]);
        i += 1;
    }
    out.join("\n")
}

/// Whether `line` opens any TOML table / array-of-tables header.
fn is_table_header(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('[') && t.ends_with(']')
}

/// The `command` of the `[mcp_servers.<id>]` table named by `header`, or `None`.
fn toml_table_command(existing: &str, header: &str) -> Option<String> {
    let lines: Vec<&str> = existing.lines().collect();
    let pos = lines.iter().position(|l| l.trim() == header)?;
    for l in lines.iter().skip(pos + 1) {
        if is_table_header(l) {
            break;
        }
        let t = l.trim();
        if let Some(rest) = t.strip_prefix("command") {
            let rest = rest.trim_start().strip_prefix('=')?.trim();
            return Some(unquote_toml(rest));
        }
    }
    None
}

/// The count (0 or 1) of `[mcp_servers.<id>]` tables named by `header`.
fn toml_table_count(existing: &str, header: &str) -> usize {
    existing.lines().filter(|l| l.trim() == header).count()
}

/// The `command` of the `[mcp_servers.anvil-mcp]` table in a Codex config, or
/// `None`.
pub fn codex_anvil_command(existing: &str) -> Option<String> {
    toml_table_command(existing, CODEX_TABLE_HEADER)
}

/// The count of `[mcp_servers.anvil-mcp]` tables (0 or 1) in a Codex config.
pub fn codex_anvil_count(existing: &str) -> usize {
    toml_table_count(existing, CODEX_TABLE_HEADER)
}

/// The `command` of the `[mcp_servers.anvil]` table in a Grok config, or `None`.
pub fn grok_anvil_command(existing: &str) -> Option<String> {
    toml_table_command(existing, GROK_TABLE_HEADER)
}

/// The count of `[mcp_servers.anvil]` tables (0 or 1) in a Grok config.
pub fn grok_anvil_count(existing: &str) -> usize {
    toml_table_count(existing, GROK_TABLE_HEADER)
}

/// Strip surrounding double quotes from a TOML string scalar, if present.
fn unquote_toml(s: &str) -> String {
    let s = s.trim();
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(s)
        .to_string()
}

// ---------------------------------------------------------------------------
// opencode — JSON `mcp.anvil` local transport (round-trip-safe).
// ---------------------------------------------------------------------------

/// The opencode MCP writer: edits ONLY the `mcp.anvil` key of the whole JSON
/// document, preserving every other server + top-level key. opencode's local MCP
/// entry shape is `{ "type": "local", "command": ["<binary>", ...], "enabled": true }`
/// — note `command` is an ARRAY (distinct from Claude Code's string command).
pub struct OpenCodeMcpWriter;

impl McpWriter for OpenCodeMcpWriter {
    fn register(&self, existing: &str, command: &str) -> Result<String, String> {
        let mut config = parse_json(existing)?;
        let servers = config
            .entry("mcp")
            .or_insert_with(|| Value::Object(Map::new()));
        let servers_map = match servers.as_object_mut() {
            Some(m) => m,
            None => {
                *servers = Value::Object(Map::new());
                servers.as_object_mut().unwrap()
            }
        };
        // Replace any existing anvil entry (correcting stale/foreign paths). Keyed
        // on the server NAME for idempotent replace + clean removal.
        servers_map.insert(
            ANVIL_SERVER_ID.to_string(),
            json!({
                "type": "local",
                "command": [command],
                "enabled": true,
            }),
        );
        serde_json::to_string_pretty(&Value::Object(config))
            .map_err(|e| format!("Failed to serialize opencode config: {}", e))
    }

    fn unregister(&self, existing: &str) -> Result<String, String> {
        let mut config = parse_json(existing)?;
        if let Some(servers) = config.get_mut("mcp").and_then(Value::as_object_mut) {
            servers.remove(ANVIL_SERVER_ID);
            // Drop an emptied mcp object so uninstall round-trips cleanly.
            if servers.is_empty() {
                config.remove("mcp");
            }
        }
        serde_json::to_string_pretty(&Value::Object(config))
            .map_err(|e| format!("Failed to serialize opencode config: {}", e))
    }
}

/// The first element of the `mcp.anvil.command` array in an opencode config, or
/// `None`.
pub fn opencode_anvil_command(existing: &str) -> Option<String> {
    let config = parse_json(existing).ok()?;
    config
        .get("mcp")?
        .get(ANVIL_SERVER_ID)?
        .get("command")?
        .as_array()?
        .first()?
        .as_str()
        .map(str::to_string)
}

/// The count of `mcp.anvil` entries (0 or 1) in an opencode config.
pub fn opencode_anvil_count(existing: &str) -> usize {
    parse_json(existing)
        .ok()
        .and_then(|c| {
            c.get("mcp")
                .and_then(Value::as_object)
                .map(|m| usize::from(m.contains_key(ANVIL_SERVER_ID)))
        })
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Kiln — JSON ARRAY `[{ name, command, args, env, cwd }]` (round-trip-safe).
// ---------------------------------------------------------------------------

/// The Kiln MCP writer: edits ONLY the `anvil`-named element of `~/.kiln/mcp.json`,
/// a JSON ARRAY of `McpServerSpec` (`{ name, command, args, env?, cwd? }`). The
/// single `command` string is split into Kiln's `command` + `args` shape; every
/// other server element is preserved. Keyed on the server NAME for idempotent
/// replace + clean removal.
pub struct KilnMcpWriter;

/// Parse a Kiln mcp.json, treating empty/whitespace as an empty array.
fn parse_json_array(existing: &str) -> Result<Vec<Value>, String> {
    if existing.trim().is_empty() {
        return Ok(Vec::new());
    }
    match serde_json::from_str::<Value>(existing) {
        Ok(Value::Array(arr)) => Ok(arr),
        Ok(_) => Err("Kiln mcp.json is not a JSON array".to_string()),
        Err(e) => Err(format!("Failed to parse Kiln mcp.json: {}", e)),
    }
}

/// Split a single command string (`"/abs/anvil-mcp --hearth /h"`) into Kiln's
/// `command` + `args` shape (`"/abs/anvil-mcp"`, `["--hearth","/h"]`).
fn split_mcp_command(command: &str) -> (String, Vec<String>) {
    let mut parts = command.split_whitespace();
    let head = parts.next().unwrap_or("").to_string();
    let args: Vec<String> = parts.map(str::to_string).collect();
    (head, args)
}

impl McpWriter for KilnMcpWriter {
    fn register(&self, existing: &str, command: &str) -> Result<String, String> {
        let mut servers = parse_json_array(existing)?;
        // Drop any prior anvil element (correcting stale/foreign paths).
        servers.retain(|s| s.get("name").and_then(Value::as_str) != Some(ANVIL_SERVER_ID));
        let (cmd, args) = split_mcp_command(command);
        servers.push(json!({
            "name": ANVIL_SERVER_ID,
            "command": cmd,
            "args": args,
            "env": {},
            "cwd": Value::Null,
        }));
        serde_json::to_string_pretty(&Value::Array(servers))
            .map_err(|e| format!("Failed to serialize Kiln mcp.json: {}", e))
    }

    fn unregister(&self, existing: &str) -> Result<String, String> {
        let mut servers = parse_json_array(existing)?;
        servers.retain(|s| s.get("name").and_then(Value::as_str) != Some(ANVIL_SERVER_ID));
        serde_json::to_string_pretty(&Value::Array(servers))
            .map_err(|e| format!("Failed to serialize Kiln mcp.json: {}", e))
    }
}

/// The full command (`command` + `args` rejoined) of the `anvil` element in a
/// Kiln mcp.json, or `None`.
pub fn kiln_anvil_command(existing: &str) -> Option<String> {
    let servers = parse_json_array(existing).ok()?;
    let entry = servers
        .iter()
        .find(|s| s.get("name").and_then(Value::as_str) == Some(ANVIL_SERVER_ID))?;
    let head = entry.get("command")?.as_str()?.to_string();
    let args: Vec<String> = entry
        .get("args")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if args.is_empty() {
        Some(head)
    } else {
        Some(format!("{} {}", head, args.join(" ")))
    }
}

/// The count of `anvil` elements (0 or 1) in a Kiln mcp.json.
pub fn kiln_anvil_count(existing: &str) -> usize {
    parse_json_array(existing)
        .map(|servers| {
            servers
                .iter()
                .filter(|s| s.get("name").and_then(Value::as_str) == Some(ANVIL_SERVER_ID))
                .count()
        })
        .unwrap_or(0)
}
