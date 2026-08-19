//! The Codex adapter — a native route plus HARD pre-mutation gate.
//!
//! FEASIBILITY (codex-cli >= 0.144 — the verified minimum on which the `hooks`
//! feature is stable + default-on; verified against the shipped binary +
//! <https://learn.chatgpt.com/docs/hooks>): Codex has a Claude-Code-parity hooks
//! engine. User-global hooks live in `~/.codex/hooks.json` (also
//! `~/.codex/config.toml` `[hooks]`, and the `<repo>/.codex/` project variants),
//! using the EXACT Claude Code hook schema — events under a top-level `hooks`
//! object, each an array of
//! `{ "hooks": [ { "type": "command", "command": ..., "timeout": ... } ] }`.
//!
//! TIMEOUT UNIT: codex 0.144 reads the hook `timeout` in SECONDS (not the
//! milliseconds `InstallSpec::timeout_ms` carries). The adapter converts on the
//! way out — a 5000 ms spec becomes `timeout: 5`. Emitting the raw ms value would
//! give codex a ~83-minute timeout.
//!
//! ABSORPTION is SURGICAL, not group-wide. On install we reclaim (a) any entry we
//! tagged `_anvil_managed` and (b) the exact canonical LEGACY route command an
//! older anvil version wrote untagged (`anvil-hooks route-turn …`) — and ONLY
//! that. An operator's own `anvil-hooks audit` hook, or an operator handler
//! sharing a group with a stale route handler, is preserved: we remove only our
//! handler from a mixed group and keep the siblings. On uninstall we remove ONLY
//! entries carrying our tag; every untagged entry round-trips untouched.
//!
//! FAIL OPEN: if the existing `hooks` structure is malformed (present but not the
//! expected object / array shape), we refuse to transform it and return an error
//! so the installer leaves the file untouched rather than overwriting operator
//! content we can't safely parse.
//!
//! A `UserPromptSubmit` command hook fires on every user turn. It receives the
//! turn (the prompt) on stdin and can INJECT guidance back: its stdout — or a
//! `{"hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":...}}`
//! payload — "is added as extra developer context." That is a genuine per-turn
//! route channel with both halves the router needs: (a) the user's message and
//! (b) a way to surface guidance. So this adapter installs the SAME per-turn route
//! hook Claude Code gets: `anvil-hooks route-turn --source codex`.
//!
//! This SUPERSEDES the earlier advisory approach. Codex's `notify` program is NOT
//! a route channel: it fires on turn-COMPLETE / approval events (too late to steer
//! the turn) and its output is not injected back — it exists for desktop
//! notifications only. The old adapter wrote a commented-out `notify` block that
//! did nothing; hooks.json is the real thing.
//!
//! Codex 0.144's `PreToolUse` command hook blocks only on exit 2 with a stderr
//! reason; outer hook errors and timeouts fail open. The gate command therefore
//! carries an internal deadline plus `--source codex`, selecting the Codex-only
//! fail-closed runtime policy without changing other harnesses.
//! TRUST: a non-managed command hook must
//! be reviewed + trusted once (`/hooks`) before it runs — the same "forced once
//! trusted" posture as Grok.
//!
//! Like Claude Code, we tag our entry `"_anvil_managed": true` so install is
//! idempotent (re-install replaces our entry, never duplicates) and uninstall
//! removes ONLY our entry — every other hook and top-level key (e.g.
//! `description`) round-trips untouched because we parse/serialize the whole JSON
//! document.

use super::{GateCapability, Harness, HookAdapter, InstallSpec, MANAGED_KEY};
use serde_json::{json, Map, Value};

pub struct CodexAdapter;

/// The exact canonical LEGACY route command an older anvil version wrote UNTAGGED.
/// Absorption matches ONLY a handler whose command is exactly this or begins with
/// this plus an argument (`anvil-hooks route-turn --source codex`). It deliberately
/// does NOT match other `anvil-hooks` subcommands (e.g. an operator's own
/// `anvil-hooks audit`), which round-trip untouched.
const LEGACY_ROUTE_COMMAND: &str = "anvil-hooks route-turn";
pub const CODEX_MUTATION_MATCHER: &str = "apply_patch|Bash";
pub const CODEX_HARD_ENFORCE: &str = "track";
const PLUGIN_HOOKS_POSIX: &str = r#""${PLUGIN_ROOT}/codex-hooks/anvil-hooks""#;
const PLUGIN_HOOKS_WINDOWS: &str = r#""%PLUGIN_ROOT%\codex-hooks\anvil-hooks.cmd""#;

fn internal_deadline_ms(spec: &InstallSpec) -> u64 {
    spec.timeout_ms
        .saturating_sub((spec.timeout_ms / 5).max(1))
}

/// Whether an entry is anvil-managed (`"_anvil_managed": true`).
fn is_managed_entry(entry: &Value) -> bool {
    entry
        .get(MANAGED_KEY)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Whether a single hook HANDLER (`{ type, command, timeout }`) is a stale anvil
/// route handler — its command is exactly the canonical legacy route command, or
/// that command followed by its arguments. This is the ONLY untagged shape install
/// absorbs; every other command (including other `anvil-hooks` subcommands) is an
/// operator's and is preserved.
fn is_legacy_route_handler(handler: &Value) -> bool {
    handler
        .get("command")
        .and_then(Value::as_str)
        .is_some_and(|c| {
            c == LEGACY_ROUTE_COMMAND || c.starts_with(&format!("{} ", LEGACY_ROUTE_COMMAND))
        })
}

/// Parse hooks.json, treating empty/whitespace as an empty object.
fn parse_config(existing: &str) -> Result<Map<String, Value>, String> {
    if existing.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(existing) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err("Codex hooks.json is not a JSON object".to_string()),
        Err(e) => Err(format!("Failed to parse hooks.json: {}", e)),
    }
}

/// Validate the hooks-container SHAPE, refusing to mutate anything malformed. Returns
/// `Err` (so install/uninstall leave the file untouched) when `hooks` is present
/// but is not an object, or `hooks.UserPromptSubmit` is present but is not an
/// array. An ABSENT container is fine — install creates it.
fn validate_shape(settings: &Map<String, Value>) -> Result<(), String> {
    let Some(hooks) = settings.get("hooks") else {
        return Ok(());
    };
    let Some(hooks_map) = hooks.as_object() else {
        return Err(
            "Codex hooks.json `hooks` is not an object; refusing to overwrite \
             (fix or remove it by hand)"
                .to_string(),
        );
    };
    for event in ["UserPromptSubmit", "PreToolUse"] {
        if hooks_map.get(event).is_some_and(|value| !value.is_array()) {
            return Err(format!(
                "Codex hooks.json `hooks.{event}` is not an array; refusing to \
                     overwrite (fix or remove it by hand)"
            ));
        }
    }
    Ok(())
}

/// The `hooks.UserPromptSubmit` entries with anvil's route hook SURGICALLY removed
/// for INSTALL: a wholly-managed entry is dropped, and an untagged entry has only
/// its legacy route handler(s) pruned (operator siblings kept). Absorbing the
/// untagged legacy shape here is what makes re-install idempotent instead of
/// stacking a second route hook.
fn turn_for_install(settings: &Map<String, Value>) -> Vec<Value> {
    turn_entries(settings, prune_for_install)
}

/// The `hooks.UserPromptSubmit` entries for UNINSTALL: drop ONLY the entries WE
/// tagged `_anvil_managed`. Every untagged entry — operator hooks and any legacy
/// untagged copy alike — round-trips untouched (we only reclaim what we own).
fn turn_for_uninstall(settings: &Map<String, Value>) -> Vec<Value> {
    turn_entries(settings, prune_for_uninstall)
}

fn retained_non_managed_entries(settings: &Map<String, Value>, event: &str) -> Vec<Value> {
    settings
        .get("hooks")
        .and_then(|hooks| hooks.get(event))
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter(|entry| !is_managed_entry(entry))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// Apply an entry pruner across `hooks.UserPromptSubmit`, keeping the entries it
/// returns `Some` for (in order). Absent / empty container → no entries.
fn turn_entries(settings: &Map<String, Value>, prune: fn(&Value) -> Option<Value>) -> Vec<Value> {
    settings
        .get("hooks")
        .and_then(|h| h.get("UserPromptSubmit"))
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(prune).collect())
        .unwrap_or_default()
}

/// Install-time pruner: drop a wholly-managed entry; from an untagged entry remove
/// only its legacy route handler(s), keeping operator siblings. `None` means the
/// entry is entirely ours (drop it).
fn prune_for_install(entry: &Value) -> Option<Value> {
    if is_managed_entry(entry) {
        return None; // a tagged entry is wholly ours
    }
    let Some(handlers) = entry.get("hooks").and_then(Value::as_array) else {
        return Some(entry.clone()); // no handler array → operator entry, keep as-is
    };
    let kept: Vec<Value> = handlers
        .iter()
        .filter(|h| !is_legacy_route_handler(h))
        .cloned()
        .collect();
    if kept.len() == handlers.len() {
        return Some(entry.clone()); // nothing of ours → untouched
    }
    if kept.is_empty() {
        return None; // the entry held only our legacy route handler(s)
    }
    // Mixed group: rebuild the entry preserving its other keys, with our handler
    // pruned and the operator siblings kept.
    let mut obj = entry.as_object().cloned().unwrap_or_default();
    obj.insert("hooks".to_string(), Value::Array(kept));
    Some(Value::Object(obj))
}

/// Uninstall-time pruner: drop a tagged entry (entirely ours), keep everything else.
fn prune_for_uninstall(entry: &Value) -> Option<Value> {
    if is_managed_entry(entry) {
        None
    } else {
        Some(entry.clone())
    }
}

/// Write a UserPromptSubmit array back into `settings.hooks.UserPromptSubmit`,
/// creating the `hooks` object when absent and dropping the key (and an empty
/// `hooks` object) when the array is empty — so uninstall round-trips to the
/// original document.
fn set_turn(settings: &mut Map<String, Value>, entries: Vec<Value>) {
    set_event(settings, "UserPromptSubmit", entries);
}

fn set_event(settings: &mut Map<String, Value>, event: &str, entries: Vec<Value>) {
    let hooks = settings
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(hooks_map) = hooks.as_object_mut() else {
        *hooks = Value::Object(Map::new());
        return set_event(settings, event, entries);
    };
    if entries.is_empty() {
        hooks_map.remove(event);
    } else {
        hooks_map.insert(event.to_string(), Value::Array(entries));
    }
    if settings
        .get("hooks")
        .and_then(Value::as_object)
        .is_some_and(Map::is_empty)
    {
        settings.remove("hooks");
    }
}

fn managed_route_entry(spec: &InstallSpec) -> Value {
    json!({
        MANAGED_KEY: true,
        "hooks": [{
            "type": "command",
            "command": Harness::Codex.turn_command_with_source(spec),
            "timeout": timeout_secs(spec),
        }],
    })
}

fn managed_gate_entry(spec: &InstallSpec) -> Value {
    json!({
        MANAGED_KEY: true,
        "matcher": CODEX_MUTATION_MATCHER,
        "hooks": [{
            "type": "command",
            "command": format!(
                "{} --source codex --hard-enforce {} --internal-deadline-ms {}",
                spec.command,
                CODEX_HARD_ENFORCE,
                internal_deadline_ms(spec)
            ),
            "timeout": timeout_secs(spec),
        }],
    })
}

/// Render the hook document packaged in the Codex plugin. Standalone global
/// installation and package assembly intentionally share this constructor.
pub fn render_plugin_hooks(spec: &InstallSpec) -> Result<String, String> {
    let plugin_spec = InstallSpec {
        command: format!("{PLUGIN_HOOKS_POSIX} gate-check"),
        turn_command: format!("{PLUGIN_HOOKS_POSIX} route-turn"),
        ..spec.clone()
    };
    let mut route = managed_route_entry(&plugin_spec);
    let mut gate = managed_gate_entry(&plugin_spec);
    route["hooks"][0]["commandWindows"] = Value::String(format!(
        "{PLUGIN_HOOKS_WINDOWS} route-turn --source codex"
    ));
    gate["hooks"][0]["commandWindows"] = Value::String(format!(
        "{PLUGIN_HOOKS_WINDOWS} gate-check --source codex --hard-enforce {} --internal-deadline-ms {}",
        CODEX_HARD_ENFORCE,
        internal_deadline_ms(spec)
    ));
    serde_json::to_string_pretty(&json!({
        "hooks": {
            "UserPromptSubmit": [route],
            "PreToolUse": [gate],
        }
    }))
    .map_err(|e| format!("Failed to serialize Codex plugin hooks: {e}"))
}

/// Foundry plugin migration: remove only Anvil-owned global entries and the exact
/// canonical untagged legacy route. It never installs a replacement global hook.
pub fn absorb_legacy_global_route(existing: &str) -> Result<String, String> {
    let mut settings = parse_config(existing)?;
    validate_shape(&settings)?;
    let turns = turn_for_install(&settings);
    let gates = retained_non_managed_entries(&settings, "PreToolUse");
    set_turn(&mut settings, turns);
    set_event(&mut settings, "PreToolUse", gates);
    serde_json::to_string_pretty(&Value::Object(settings))
        .map_err(|e| format!("Failed to serialize hooks.json: {e}"))
}

impl HookAdapter for CodexAdapter {
    fn harness(&self) -> Harness {
        Harness::Codex
    }

    fn gate_capability(&self) -> GateCapability {
        GateCapability::Hard
    }

    fn install(&self, existing: &str, spec: &InstallSpec) -> Result<String, String> {
        let mut settings = parse_config(existing)?;
        // Refuse malformed operator content without writing it.
        validate_shape(&settings)?;
        // Idempotent: surgically reclaim any prior Anvil route hook, then add the
        // single route and gate produced by the shared Codex constructors.
        let mut turn_entries = turn_for_install(&settings);
        turn_entries.push(managed_route_entry(spec));
        set_turn(&mut settings, turn_entries);
        let mut gate_entries = retained_non_managed_entries(&settings, "PreToolUse");
        gate_entries.push(managed_gate_entry(spec));
        set_event(&mut settings, "PreToolUse", gate_entries);

        serde_json::to_string_pretty(&Value::Object(settings))
            .map_err(|e| format!("Failed to serialize hooks.json: {}", e))
    }

    fn uninstall(&self, existing: &str) -> Result<String, String> {
        let mut settings = parse_config(existing)?;
        validate_shape(&settings)?;
        let entries = turn_for_uninstall(&settings);
        set_turn(&mut settings, entries);
        let entries = retained_non_managed_entries(&settings, "PreToolUse");
        set_event(&mut settings, "PreToolUse", entries);
        serde_json::to_string_pretty(&Value::Object(settings))
            .map_err(|e| format!("Failed to serialize hooks.json: {}", e))
    }
}

/// Convert the spec's millisecond timeout to the SECONDS codex 0.144 expects,
/// rounding up so a sub-second timeout still yields at least 1 second.
fn timeout_secs(spec: &InstallSpec) -> u64 {
    spec.timeout_ms.div_ceil(1000).max(1)
}

/// The count of anvil-managed UserPromptSubmit (route) entries — proves the route
/// hook is idempotent (exactly 1 after any number of installs, 0 after uninstall).
pub fn managed_turn_count(existing: &str) -> usize {
    let settings = match parse_config(existing) {
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

pub fn managed_gate_count(existing: &str) -> usize {
    let settings = match parse_config(existing) {
        Ok(settings) => settings,
        Err(_) => return 0,
    };
    settings
        .get("hooks")
        .and_then(|hooks| hooks.get("PreToolUse"))
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter(|entry| is_managed_entry(entry))
                .count()
        })
        .unwrap_or(0)
}

/// The `command` of the single anvil-managed UserPromptSubmit (route) hook, or
/// `None`.
pub fn managed_turn_command(existing: &str) -> Option<String> {
    let settings = parse_config(existing).ok()?;
    let entry = settings
        .get("hooks")?
        .get("UserPromptSubmit")?
        .as_array()?
        .iter()
        .find(|e| is_managed_entry(e))?;
    let cmd_entry = entry.get("hooks")?.as_array()?.first()?;
    Some(cmd_entry.get("command")?.as_str()?.to_string())
}
