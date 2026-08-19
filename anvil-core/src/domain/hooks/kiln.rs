//! The Kiln adapter — HARD gate, Claude-Code-parity contract (as-built).
//!
//! Kiln (`~/.kiln`) ships a real, Claude-Code-mirroring hook system (per the Kiln
//! author's as-built answers, 2026-06-19): it reads `~/.kiln/hooks.json` (env
//! `KILN_HOOKS_CONFIG`), a JSON document whose TOP LEVEL is the Claude-Code `hooks`
//! object — event name → array of `{ matcher?, hooks: [ { type:"command", command,
//! args?, timeout_ms?, _managed_by? } ] }`. Kiln emits the Claude-Code stdin
//! envelope (`hook_event_name:"PreToolUse"`) and accepts our `gate-check` decision
//! shapes unchanged, so this is a HARD pre-mutation gate; the adapter only writes
//! the config.
//!
//! Differences from the Claude Code adapter (`claude_code.rs`):
//!   - the events live at the TOP LEVEL of `hooks.json` (no `hooks:` wrapper);
//!   - the command is SPLIT into `command` + `args` (Kiln's schema), not a single
//!     string;
//!   - the matcher is Kiln's mutating built-ins `write|edit|bash`;
//!   - idempotency keys on `_managed_by:"anvil-hooks"` (Kiln carries it through),
//!     not `_anvil_managed`.
//!
//! Install is idempotent (any prior `_managed_by:"anvil-hooks"` entry is stripped
//! before re-emitting) and reversible (uninstall removes ONLY our entries,
//! preserving operator-authored hooks). The whole JSON document round-trips.

use super::{GateCapability, Harness, HookAdapter, InstallSpec};
use serde_json::{json, Map, Value};

pub struct KilnAdapter;

/// Kiln's file-mutating built-in tools — the pre-tool gate matcher.
pub const KILN_MATCHER: &str = "write|edit|bash";
/// The marker field Kiln carries through on each managed hook-command entry.
pub const KILN_MANAGED_FIELD: &str = "_managed_by";
/// The marker value identifying anvil-owned entries (for idempotent replace +
/// clean uninstall).
pub const KILN_MANAGED_VALUE: &str = "anvil-hooks";

const PRE_TOOL_USE: &str = "PreToolUse";
const USER_PROMPT_SUBMIT: &str = "UserPromptSubmit";

/// Parse `hooks.json`, treating empty/whitespace as a fresh document.
fn parse_hooks(existing: &str) -> Result<Map<String, Value>, String> {
    if existing.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(existing) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err("Kiln hooks.json is not a JSON object".to_string()),
        Err(e) => Err(format!("Failed to parse Kiln hooks.json: {}", e)),
    }
}

/// Whether an event-array ENTRY (`{ matcher?, hooks:[...] }`) is anvil-managed —
/// any of its hook-command entries carries `_managed_by:"anvil-hooks"`.
fn is_managed_entry(entry: &Value) -> bool {
    entry
        .get("hooks")
        .and_then(Value::as_array)
        .map(|hooks| {
            hooks.iter().any(|h| {
                h.get(KILN_MANAGED_FIELD).and_then(Value::as_str) == Some(KILN_MANAGED_VALUE)
            })
        })
        .unwrap_or(false)
}

/// The entries of top-level `<event>` with anvil-managed entries removed.
fn event_without_managed(doc: &Map<String, Value>, event: &str) -> Vec<Value> {
    doc.get(event)
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter(|e| !is_managed_entry(e))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// Write an entries array back to the top-level `<event>` key, dropping the key
/// when empty so uninstall round-trips to the original document.
fn set_event(doc: &mut Map<String, Value>, event: &str, entries: Vec<Value>) {
    if entries.is_empty() {
        doc.remove(event);
    } else {
        doc.insert(event.to_string(), Value::Array(entries));
    }
}

/// Split a single command string (`"anvil-hooks gate-check"`) into Kiln's
/// `command` + `args` shape (`"anvil-hooks"`, `["gate-check"]`).
fn split_command(command: &str) -> (String, Vec<String>) {
    let mut parts = command.split_whitespace();
    let head = parts.next().unwrap_or("").to_string();
    let args: Vec<String> = parts.map(str::to_string).collect();
    (head, args)
}

/// Build an anvil-managed hook-command entry from a command string.
fn managed_command(command: &str, timeout_ms: u64) -> Value {
    let (cmd, args) = split_command(command);
    json!({
        "type": "command",
        "command": cmd,
        "args": args,
        "timeout_ms": timeout_ms,
        KILN_MANAGED_FIELD: KILN_MANAGED_VALUE,
    })
}

impl HookAdapter for KilnAdapter {
    fn harness(&self) -> Harness {
        Harness::Kiln
    }

    fn gate_capability(&self) -> GateCapability {
        // Kiln's PreToolUse hook refuses the call (block-and-continue).
        GateCapability::Hard
    }

    fn install(&self, existing: &str, spec: &InstallSpec) -> Result<String, String> {
        let mut doc = parse_hooks(existing)?;

        // PreToolUse gate (matcher on Kiln's mutating built-ins). Idempotent:
        // drop any prior anvil entry, then add exactly one.
        let mut pre = event_without_managed(&doc, PRE_TOOL_USE);
        pre.push(json!({
            "matcher": KILN_MATCHER,
            "hooks": [ managed_command(&spec.command, spec.timeout_ms) ],
        }));
        set_event(&mut doc, PRE_TOOL_USE, pre);

        // UserPromptSubmit per-turn router (no matcher — every turn). Tagged with
        // `--source kiln` so routed turns are self-describing.
        let turn_command = Harness::Kiln.turn_command_with_source(spec);
        let mut turn = event_without_managed(&doc, USER_PROMPT_SUBMIT);
        turn.push(json!({
            "hooks": [ managed_command(&turn_command, spec.timeout_ms) ],
        }));
        set_event(&mut doc, USER_PROMPT_SUBMIT, turn);

        serde_json::to_string_pretty(&Value::Object(doc))
            .map_err(|e| format!("Failed to serialize Kiln hooks.json: {}", e))
    }

    fn uninstall(&self, existing: &str) -> Result<String, String> {
        let mut doc = parse_hooks(existing)?;
        let pre = event_without_managed(&doc, PRE_TOOL_USE);
        set_event(&mut doc, PRE_TOOL_USE, pre);
        let turn = event_without_managed(&doc, USER_PROMPT_SUBMIT);
        set_event(&mut doc, USER_PROMPT_SUBMIT, turn);
        serde_json::to_string_pretty(&Value::Object(doc))
            .map_err(|e| format!("Failed to serialize Kiln hooks.json: {}", e))
    }
}

// ---------------------------------------------------------------------------
// Inspection helpers (used by the brine seam + reporting).
// ---------------------------------------------------------------------------

/// The count of anvil-managed PreToolUse entries (0 or 1) — proves idempotency.
pub fn managed_pretool_count(existing: &str) -> usize {
    managed_count(existing, PRE_TOOL_USE)
}

/// The count of anvil-managed UserPromptSubmit (turn) entries (0 or 1).
pub fn managed_turn_count(existing: &str) -> usize {
    managed_count(existing, USER_PROMPT_SUBMIT)
}

fn managed_count(existing: &str, event: &str) -> usize {
    parse_hooks(existing)
        .ok()
        .and_then(|doc| {
            doc.get(event)
                .and_then(Value::as_array)
                .map(|arr| arr.iter().filter(|e| is_managed_entry(e)).count())
        })
        .unwrap_or(0)
}

/// Whether the config carries an anvil-managed pre-tool hook (parity with the
/// shared brine step that asserts a hard pre-tool gate is present).
pub fn has_managed_pretool(existing: &str) -> bool {
    managed_pretool_count(existing) > 0
}

/// The `(matcher, command, timeout_ms)` of the single anvil-managed PreToolUse
/// hook, or `None`. `command` rejoins `command` + `args` into a single string
/// (`"anvil-hooks gate-check"`) for assertion parity with the other adapters.
pub fn managed_pretool_detail(existing: &str) -> Option<(String, String, u64)> {
    let doc = parse_hooks(existing).ok()?;
    let entry = doc
        .get(PRE_TOOL_USE)?
        .as_array()?
        .iter()
        .find(|e| is_managed_entry(e))?;
    let matcher = entry.get("matcher")?.as_str()?.to_string();
    let cmd_entry = entry.get("hooks")?.as_array()?.first()?;
    let command = rejoin_command(cmd_entry)?;
    let timeout = cmd_entry.get("timeout_ms")?.as_u64()?;
    Some((matcher, command, timeout))
}

/// The full command (`command` + `args` rejoined) of the single anvil-managed
/// UserPromptSubmit (turn) hook, or `None`.
pub fn managed_turn_command(existing: &str) -> Option<String> {
    let doc = parse_hooks(existing).ok()?;
    let entry = doc
        .get(USER_PROMPT_SUBMIT)?
        .as_array()?
        .iter()
        .find(|e| is_managed_entry(e))?;
    let cmd_entry = entry.get("hooks")?.as_array()?.first()?;
    rejoin_command(cmd_entry)
}

/// Rejoin a hook-command entry's `command` + `args` into a single string.
fn rejoin_command(cmd_entry: &Value) -> Option<String> {
    let head = cmd_entry.get("command")?.as_str()?.to_string();
    let args: Vec<String> = cmd_entry
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
