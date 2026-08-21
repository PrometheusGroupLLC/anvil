//! The Claude Code adapter — the HARD-gate-capable target.
//!
//! Claude Code reads `~/.claude/settings.json`. Its `hooks.PreToolUse` array
//! carries matcher entries; a PreToolUse hook can BLOCK a mutation (exit code 2
//! / decision JSON), which is why this is the hard-gate target. The real entry
//! shape is:
//!
//! ```json
//! { "matcher": "Edit|Write|...",
//!   "hooks": [ { "type": "command", "command": "anvil-hooks gate-check", "timeout": 5000 } ] }
//! ```
//!
//! We tag our entry with `"_anvil_managed": true` so install is idempotent
//! (re-install replaces our entry, never duplicates) and uninstall removes ONLY
//! our entry — every other hook and every unrelated key round-trips untouched
//! because we parse/serialize the whole JSON document.

use super::{
    GateCapability, Harness, HookAdapter, InstallSpec, DEFAULT_MATCHER, MANAGED_KEY,
    SUBAGENT_MATCHER,
};
use serde_json::{json, Map, Value};

pub struct ClaudeCodeAdapter;

/// Whether a PreToolUse entry is anvil-managed (`"_anvil_managed": true`).
fn is_managed_entry(entry: &Value) -> bool {
    entry
        .get(MANAGED_KEY)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// The command prefix of anvil's own hook binary. Any hook whose command runs
/// `anvil-hooks …` is one of ours regardless of whether it carries the managed tag.
const ANVIL_COMMAND_PREFIX: &str = "anvil-hooks ";

/// Whether an entry is anvil's — EITHER tagged `_anvil_managed`, OR (an older /
/// foreign copy) one of its hook commands invokes the `anvil-hooks` binary.
///
/// Folding the command match into the install/uninstall filter makes install
/// idempotent against an UNTAGGED copy a prior `anvil-hooks` version (or a second
/// writer) left behind: without it, an untagged copy is treated as a foreign user
/// hook and KEPT, then a fresh tagged copy is added on top — duplicating the hook
/// so it fires twice per turn. Matching by the owned command prefix absorbs the
/// stale copy instead. A genuine user hook (any non-`anvil-hooks` command) never
/// matches and round-trips untouched.
fn is_anvil_entry(entry: &Value) -> bool {
    if is_managed_entry(entry) {
        return true;
    }
    entry
        .get("hooks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|h| h.get("command").and_then(Value::as_str))
        .any(|c| c.starts_with(ANVIL_COMMAND_PREFIX))
}

/// Parse settings JSON, treating empty/whitespace as an empty object.
fn parse_settings(existing: &str) -> Result<Map<String, Value>, String> {
    if existing.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(existing) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err("Claude Code settings.json is not a JSON object".to_string()),
        Err(e) => Err(format!("Failed to parse settings.json: {}", e)),
    }
}

/// The PreToolUse array within `hooks`, with ALL anvil entries removed (tagged or
/// untagged). Returns the surviving (genuinely user) entries.
fn pretool_without_managed(settings: &Map<String, Value>) -> Vec<Value> {
    event_without_managed(settings, "PreToolUse")
}

/// The entries of `hooks.<event>` with ALL anvil entries removed (tagged via
/// `_anvil_managed` OR an untagged copy identified by its `anvil-hooks` command),
/// returning the surviving (user) entries. Generic over PreToolUse /
/// UserPromptSubmit. Filtering by [`is_anvil_entry`] (not just the tag) is what
/// makes re-install absorb a stale untagged copy instead of duplicating it.
fn event_without_managed(settings: &Map<String, Value>, event: &str) -> Vec<Value> {
    settings
        .get("hooks")
        .and_then(|h| h.get(event))
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter(|e| !is_anvil_entry(e)).cloned().collect())
        .unwrap_or_default()
}

/// Write a PreToolUse array back into `settings.hooks.PreToolUse`, creating the
/// `hooks` object when absent and dropping the `PreToolUse` key (and an empty
/// `hooks` object) when the array is empty.
fn set_pretool(settings: &mut Map<String, Value>, entries: Vec<Value>) {
    set_event(settings, "PreToolUse", entries)
}

/// Write an entries array back into `settings.hooks.<event>`, creating the
/// `hooks` object when absent and dropping the `<event>` key (and an empty
/// `hooks` object) when the array is empty. Generic over PreToolUse /
/// UserPromptSubmit.
fn set_event(settings: &mut Map<String, Value>, event: &str, entries: Vec<Value>) {
    let hooks = settings
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(hooks_map) = hooks.as_object_mut() else {
        // `hooks` was a non-object; replace it with a fresh object.
        *hooks = Value::Object(Map::new());
        return set_event(settings, event, entries);
    };
    if entries.is_empty() {
        hooks_map.remove(event);
    } else {
        hooks_map.insert(event.to_string(), Value::Array(entries));
    }
    // Drop an empty `hooks` object so uninstall round-trips to the original.
    if settings
        .get("hooks")
        .and_then(Value::as_object)
        .is_some_and(Map::is_empty)
    {
        settings.remove("hooks");
    }
}

impl HookAdapter for ClaudeCodeAdapter {
    fn harness(&self) -> Harness {
        Harness::ClaudeCode
    }

    fn gate_capability(&self) -> GateCapability {
        GateCapability::Hard
    }

    fn install(&self, existing: &str, spec: &InstallSpec) -> Result<String, String> {
        let mut settings = parse_settings(existing)?;
        // Idempotent: drop any prior anvil entries, then re-add ours.
        let mut entries = pretool_without_managed(&settings);
        // (1) The hard gate on mutating tools.
        entries.push(json!({
            MANAGED_KEY: true,
            "matcher": DEFAULT_MATCHER,
            "hooks": [ {
                "type": "command",
                "command": spec.command,
                "timeout": spec.timeout_ms,
            } ],
        }));
        // (2) The SUBAGENT route hook: fires when a subagent is spawned (Task
        // tool), routing its mission through Anvil so subagent/automated turns
        // reach the router for a decision — not only top-level user prompts.
        entries.push(json!({
            MANAGED_KEY: true,
            "matcher": SUBAGENT_MATCHER,
            "hooks": [ {
                "type": "command",
                "command": self.harness().subagent_command_with_source(spec),
                "timeout": spec.turn_timeout_ms,
            } ],
        }));
        set_pretool(&mut settings, entries);

        // The per-turn routing hook fires on UserPromptSubmit; its stdout is
        // injected back to the agent as added context. No matcher (every turn).
        let mut turn_entries = event_without_managed(&settings, "UserPromptSubmit");
        turn_entries.push(json!({
            MANAGED_KEY: true,
            "hooks": [ {
                "type": "command",
                "command": self.harness().turn_command_with_source(spec),
                "timeout": spec.turn_timeout_ms,
            } ],
        }));
        set_event(&mut settings, "UserPromptSubmit", turn_entries);

        serde_json::to_string_pretty(&Value::Object(settings))
            .map_err(|e| format!("Failed to serialize settings.json: {}", e))
    }

    fn uninstall(&self, existing: &str) -> Result<String, String> {
        let mut settings = parse_settings(existing)?;
        let entries = pretool_without_managed(&settings);
        set_pretool(&mut settings, entries);
        let turn_entries = event_without_managed(&settings, "UserPromptSubmit");
        set_event(&mut settings, "UserPromptSubmit", turn_entries);
        serde_json::to_string_pretty(&Value::Object(settings))
            .map_err(|e| format!("Failed to serialize settings.json: {}", e))
    }
}

/// The count of UNTAGGED anvil entries (an `anvil-hooks` command WITHOUT the
/// `_anvil_managed` tag) across `PreToolUse` + `UserPromptSubmit` — i.e. stale
/// copies a prior version or a second writer left behind. The brine assertions use
/// this to prove install absorbs them (0 after install). It must be 0 in steady
/// state; any positive value is a duplicate waiting to happen.
pub fn untagged_anvil_count(existing: &str) -> usize {
    let settings = match parse_settings(existing) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let hooks = match settings.get("hooks").and_then(Value::as_object) {
        Some(h) => h,
        None => return 0,
    };
    ["PreToolUse", "UserPromptSubmit"]
        .iter()
        .filter_map(|ev| hooks.get(*ev).and_then(Value::as_array))
        .flatten()
        .filter(|e| !is_managed_entry(e) && is_anvil_entry(e))
        .count()
}

/// The count of anvil-managed PreToolUse entries in a settings document — used by
/// the brine assertions to prove idempotency (exactly 1 after any number of
/// installs).
pub fn managed_pretool_count(existing: &str) -> usize {
    let settings = match parse_settings(existing) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    settings
        .get("hooks")
        .and_then(|h| h.get("PreToolUse"))
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter(|e| is_managed_entry(e)).count())
        .unwrap_or(0)
}

/// The `(matcher, command, timeout)` of the single anvil-managed PreToolUse hook,
/// or `None`. Reads the first `hooks[]` command entry of the managed matcher
/// entry. Used by the brine assertions.
pub fn managed_pretool_detail(existing: &str) -> Option<(String, String, u64)> {
    let settings = parse_settings(existing).ok()?;
    let entry = settings
        .get("hooks")?
        .get("PreToolUse")?
        .as_array()?
        .iter()
        .find(|e| is_managed_entry(e))?;
    let matcher = entry.get("matcher")?.as_str()?.to_string();
    let cmd_entry = entry.get("hooks")?.as_array()?.first()?;
    let command = cmd_entry.get("command")?.as_str()?.to_string();
    let timeout = cmd_entry.get("timeout")?.as_u64()?;
    Some((matcher, command, timeout))
}

/// The `(matcher, command, timeout)` of the anvil-managed PreToolUse entry whose
/// matcher is the SUBAGENT matcher (`Task`) — the subagent route hook — or `None`.
/// Used by the brine assertions to prove subagent dispatches route.
pub fn managed_subagent_detail(existing: &str) -> Option<(String, String, u64)> {
    let settings = parse_settings(existing).ok()?;
    let entry = settings
        .get("hooks")?
        .get("PreToolUse")?
        .as_array()?
        .iter()
        .find(|e| {
            is_managed_entry(e)
                && e.get("matcher").and_then(Value::as_str) == Some(SUBAGENT_MATCHER)
        })?;
    let matcher = entry.get("matcher")?.as_str()?.to_string();
    let cmd_entry = entry.get("hooks")?.as_array()?.first()?;
    let command = cmd_entry.get("command")?.as_str()?.to_string();
    let timeout = cmd_entry.get("timeout")?.as_u64()?;
    Some((matcher, command, timeout))
}

/// The count of anvil-managed UserPromptSubmit (turn) entries — proves the turn
/// hook is idempotent (exactly 1 after any number of installs, 0 after uninstall).
pub fn managed_turn_count(existing: &str) -> usize {
    let settings = match parse_settings(existing) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    settings
        .get("hooks")
        .and_then(|h| h.get("UserPromptSubmit"))
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter(|e| is_managed_entry(e)).count())
        .unwrap_or(0)
}

/// The `timeout` of the single anvil-managed UserPromptSubmit (turn) hook, or
/// `None`. Separate from [`managed_pretool_detail`], which reports the GATE
/// entry's timeout — the two are deliberately different numbers and an assertion
/// that could only see one of them would not notice them being re-coupled.
pub fn managed_turn_timeout(existing: &str) -> Option<u64> {
    let settings = parse_settings(existing).ok()?;
    let entry = settings
        .get("hooks")?
        .get("UserPromptSubmit")?
        .as_array()?
        .iter()
        .find(|e| is_managed_entry(e))?;
    let cmd_entry = entry.get("hooks")?.as_array()?.first()?;
    cmd_entry.get("timeout")?.as_u64()
}

/// The `command` of the single anvil-managed UserPromptSubmit (turn) hook, or
/// `None`. Used by the brine assertions.
pub fn managed_turn_command(existing: &str) -> Option<String> {
    let settings = parse_settings(existing).ok()?;
    let entry = settings
        .get("hooks")?
        .get("UserPromptSubmit")?
        .as_array()?
        .iter()
        .find(|e| is_managed_entry(e))?;
    let cmd_entry = entry.get("hooks")?.as_array()?.first()?;
    Some(cmd_entry.get("command")?.as_str()?.to_string())
}
