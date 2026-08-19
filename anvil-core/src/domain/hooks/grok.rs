//! The Grok adapter — FORCED via a drop-in plugin (trust-gated per project).
//!
//! Grok (xAI's CLI) reads `~/.grok/config.toml` and discovers user-scoped
//! plugins under `~/.grok/plugins/<name>/`. Grok adopts the Claude Code plugin
//! format, so a plugin that ships `hooks/hooks.json` with a `UserPromptSubmit`
//! entry fires that command on EVERY user turn — a true per-turn route hook
//! (verified live: `grok inspect` lists it `(user, enabled) hooks`).
//!
//! So this adapter delivers TWO things:
//!   1. An anvil-managed cooperative block in `config.toml` — a DETECTION marker
//!      + documentation (TOML comments are inert), carrying the source-tagged
//!      turn command for humans and the coverage scorer.
//!   2. The real per-turn hook as a drop-in PLUGIN artifact under
//!      `plugins/anvil-route-turn/` (`.claude-plugin/plugin.json` +
//!      `hooks/hooks.json`). This is the forced mechanism.
//!
//! IMPORTANT — trust gate: grok only EXECUTES hooks in a project the user has
//! explicitly trusted (`/hooks-trust`); there is no global trust-all (a
//! deliberate grok security boundary). So the route hook is "forced once the
//! project is trusted", not unconditionally global. The plugin is delivered
//! regardless; trust is a one-time per-project user consent.
//!
//! Because Grok config is TOML and we do not vendor a TOML round-trip parser, we
//! write the marker into a clearly DELIMITED, anvil-managed fenced comment block
//! (see [`super::append_fenced_block`]). Install strips any prior block first
//! (idempotent); uninstall removes the block, preserving every other line.

use super::{
    append_fenced_block, has_fenced_block, strip_fenced_block, GateCapability, Harness,
    HookAdapter, HookArtifact, InstallSpec,
};
use std::path::PathBuf;

pub struct GrokAdapter;

/// The plugin directory name (under `~/.grok/plugins/`) the forced route hook
/// ships as. Stable so install is idempotent and uninstall can remove it.
pub const PLUGIN_NAME: &str = "anvil-route-turn";

/// The plugin manifest (`.claude-plugin/plugin.json`) — name + version + a
/// description naming the route command. Grok requires name + version.
fn plugin_manifest() -> String {
    format!(
        "{{\n  \"name\": \"{name}\",\n  \"version\": \"0.1.0\",\n  \
         \"description\": \"Routes every Grok turn through the Anvil router \
         (anvil-hooks route-turn --source grok), so the router records a \
         decision for each turn.\"\n}}\n",
        name = PLUGIN_NAME,
    )
}

/// The plugin hooks file (`hooks/hooks.json`) — a `UserPromptSubmit` command
/// hook firing the source-tagged route command on every user turn.
fn plugin_hooks(spec: &InstallSpec) -> String {
    format!(
        "{{\n  \"hooks\": {{\n    \"UserPromptSubmit\": [\n      \
         {{ \"hooks\": [ {{ \"type\": \"command\", \"command\": \"{cmd}\", \
         \"timeout\": 10 }} ] }}\n    ]\n  }}\n}}\n",
        cmd = Harness::Grok.turn_command_with_source(spec),
    )
}

/// The fenced body the Grok adapter installs into `config.toml`: a DETECTION +
/// documentation marker. The real per-turn hook is the drop-in plugin (see
/// [`GrokAdapter::artifacts`]); this comment block records it for humans and the
/// coverage scorer (TOML treats `#` lines as inert).
fn grok_body(spec: &InstallSpec) -> String {
    format!(
        "# Anvil route hook for Grok. The FORCED per-turn hook ships as a drop-in\n\
         # plugin at ~/.grok/plugins/{name}/ (UserPromptSubmit -> the turn command\n\
         # below). NOTE: grok only EXECUTES hooks in a TRUSTED project — run\n\
         # /hooks-trust once per project to enable routing. The anvil-mcp server\n\
         # (registered under [mcp_servers.anvil]) carries the cooperative protocol.\n\
         # anvil-gate = \"{cmd}\"   # timeout {timeout}ms\n\
         # anvil-turn = \"{turn}\"",
        name = PLUGIN_NAME,
        cmd = spec.command,
        timeout = spec.timeout_ms,
        turn = Harness::Grok.turn_command_with_source(spec),
    )
}

impl HookAdapter for GrokAdapter {
    fn harness(&self) -> Harness {
        Harness::Grok
    }

    fn gate_capability(&self) -> GateCapability {
        // The plugin routes (per-turn) but does not hard-BLOCK tools, so the
        // GATE axis stays cooperative.
        GateCapability::Cooperative
    }

    fn install(&self, existing: &str, spec: &InstallSpec) -> Result<String, String> {
        Ok(append_fenced_block(existing, &grok_body(spec)))
    }

    fn uninstall(&self, existing: &str) -> Result<String, String> {
        Ok(strip_fenced_block(existing))
    }

    fn artifacts(&self, spec: &InstallSpec) -> Vec<HookArtifact> {
        let base = PathBuf::from("plugins").join(PLUGIN_NAME);
        vec![
            HookArtifact {
                rel_path: base.join(".claude-plugin").join("plugin.json"),
                content: plugin_manifest(),
                executable: false,
            },
            HookArtifact {
                rel_path: base.join("hooks").join("hooks.json"),
                content: plugin_hooks(spec),
                executable: false,
            },
        ]
    }

    fn artifact_paths(&self) -> Vec<PathBuf> {
        vec![PathBuf::from("plugins").join(PLUGIN_NAME)]
    }
}

/// Whether the Grok config carries an anvil-managed cooperative block.
pub fn has_managed_block(existing: &str) -> bool {
    has_fenced_block(existing)
}
