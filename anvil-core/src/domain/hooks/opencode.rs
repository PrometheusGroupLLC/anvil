//! The opencode adapter — FORCED via a drop-in JS plugin (best-effort).
//!
//! opencode reads `~/.config/opencode/opencode.jsonc` and auto-loads JS/TS
//! plugins from `~/.config/opencode/plugin/`. A plugin's `chat.message` hook
//! fires per user message — the per-turn route point. So this adapter ships the
//! route hook as a drop-in PLUGIN artifact (`plugin/anvil-route-turn.js`) that
//! spawns `anvil-hooks route-turn --source opencode` on each turn, fail-open.
//!
//! CRITICAL — the config must NOT be marked. opencode does STRICT config
//! validation and REJECTS any unknown top-level key: an earlier version stamped
//! `_anvil_managed` into `opencode.jsonc`, which made opencode refuse to start
//! ("Configuration is invalid … Unrecognized key: _anvil_managed"). So this
//! adapter NEVER writes a config marker; the plugin file is the entire delivery.
//! `install`/`uninstall` only REMOVE a legacy `_anvil_managed` key (repairing a
//! broken config), and otherwise return the config byte-for-byte so JSONC
//! comments and every user key are preserved.
//!
//! Best-effort note: `chat.message` is verified to load (the plugin module is
//! evaluated and its function runs) but may only dispatch in interactive/serve
//! mode, not headless `opencode run`. Routing is therefore best-effort, surfaced
//! (not claimed as a hard guarantee) by the coverage scorer.

use super::{GateCapability, Harness, HookAdapter, HookArtifact, InstallSpec};
use serde_json::{Map, Value};
use std::path::PathBuf;

pub struct OpenCodeAdapter;

/// The plugin file name (under `~/.config/opencode/plugin/`) the route hook ships
/// as. Stable so install is idempotent and uninstall can remove it.
pub const PLUGIN_FILE: &str = "anvil-route-turn.js";

/// The legacy top-level key earlier versions stamped into `opencode.jsonc`. It
/// breaks opencode's strict config validation, so install/uninstall STRIP it.
pub const MARKER_KEY: &str = "_anvil_managed";

/// Parse opencode config JSON, treating empty/whitespace as an empty object.
fn parse_config(existing: &str) -> Result<Map<String, Value>, String> {
    if existing.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(existing) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err("opencode config is not a JSON object".to_string()),
        Err(e) => Err(format!("Failed to parse opencode config: {}", e)),
    }
}

/// Remove the legacy `_anvil_managed` key if present, returning the repaired
/// config. If the key is ABSENT, return the original text byte-for-byte (so
/// JSONC comments and formatting are preserved — we never reformat a clean file).
fn strip_legacy_marker(existing: &str) -> Result<String, String> {
    // Cheap raw check first: only parse (which would drop comments) when we must.
    if !existing.contains(MARKER_KEY) {
        return Ok(existing.to_string());
    }
    let mut config = parse_config(existing)?;
    config.remove(MARKER_KEY);
    serde_json::to_string_pretty(&Value::Object(config))
        .map(|s| {
            let mut s = s;
            s.push('\n');
            s
        })
        .map_err(|e| format!("Failed to serialize opencode config: {}", e))
}

/// The opencode plugin source (`plugin/anvil-route-turn.js`): a `chat.message`
/// hook that pipes the user prompt as JSON stdin to the source-tagged route
/// command, swallowing all errors so a missing/failing `anvil-hooks` never
/// blocks the user's turn (fail-open).
fn plugin_source(spec: &InstallSpec) -> String {
    // Split the base turn command (default "anvil-hooks route-turn") into a JS
    // argv array, then append the source tag for opencode.
    let mut argv: Vec<String> = spec
        .turn_command
        .split_whitespace()
        .map(|s| format!("{:?}", s)) // JS-safe double-quoted strings
        .collect();
    argv.push(format!("{:?}", "--source"));
    argv.push(format!("{:?}", Harness::OpenCode.id()));
    let bin = argv.remove(0); // first token is the binary
    let args = argv.join(", ");
    format!(
        r#"// anvil-managed (do not edit) — routes every opencode turn through Anvil.
// Fires anvil-hooks route-turn --source opencode on each user message, fail-open.
import {{ spawn }} from "node:child_process";
export const AnvilRouteTurn = async ({{ directory }}) => ({{
  "chat.message": async (_input, output) => {{
    const prompt = (output.parts || [])
      .filter((p) => p && p.type === "text")
      .map((p) => p.text)
      .join("\n");
    if (!prompt.trim()) return;
    await new Promise((resolve) => {{
      try {{
        const c = spawn({bin}, [{args}], {{ cwd: directory, stdio: ["pipe", "ignore", "ignore"] }});
        c.on("error", () => resolve());
        c.on("close", () => resolve());
        c.stdin.write(JSON.stringify({{ prompt }}));
        c.stdin.end();
      }} catch {{ resolve(); }}
    }});
  }},
}});
export default AnvilRouteTurn;
"#,
        bin = bin,
        args = args,
    )
}

impl HookAdapter for OpenCodeAdapter {
    fn harness(&self) -> Harness {
        Harness::OpenCode
    }

    fn gate_capability(&self) -> GateCapability {
        GateCapability::Cooperative
    }

    fn install(&self, existing: &str, _spec: &InstallSpec) -> Result<String, String> {
        // Never add a marker (it breaks opencode). Only repair a legacy one.
        strip_legacy_marker(existing)
    }

    fn uninstall(&self, existing: &str) -> Result<String, String> {
        strip_legacy_marker(existing)
    }

    fn artifacts(&self, spec: &InstallSpec) -> Vec<HookArtifact> {
        vec![HookArtifact {
            rel_path: PathBuf::from("plugin").join(PLUGIN_FILE),
            content: plugin_source(spec),
            executable: false,
        }]
    }

    fn artifact_paths(&self) -> Vec<PathBuf> {
        vec![PathBuf::from("plugin").join(PLUGIN_FILE)]
    }
}

/// Whether the opencode config still carries the LEGACY `_anvil_managed` marker
/// (true only before migration; the current adapter never writes it).
pub fn has_managed_marker(existing: &str) -> bool {
    parse_config(existing)
        .map(|c| c.contains_key(MARKER_KEY))
        .unwrap_or(false)
}

/// The count of legacy markers (0 or 1) — 0 after the current install/uninstall.
pub fn managed_marker_count(existing: &str) -> usize {
    usize::from(has_managed_marker(existing))
}
