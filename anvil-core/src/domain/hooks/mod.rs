//! Per-harness hook adapters — the installable side of the harness-agnostic
//! kit-hook DELIVERY contract.
//!
//! Anvil owns the hook contract AND the per-harness adapters (Foundry merely
//! triggers). This module hosts the [`HookAdapter`] trait and its six
//! implementations (Claude Code, Codex, Kiln, Hermes, Grok, opencode). Each
//! adapter transforms a
//! harness's NATIVE config-file content: `install` injects an anvil-managed,
//! clearly DELIMITED block; `uninstall` removes ONLY that block. Both are pure
//! string→string transforms so they are unit/brine-testable without touching the
//! filesystem — the binary handles the read/write plumbing.
//!
//! Idempotency + reversibility are structural: install always STRIPS any prior
//! anvil block before re-emitting, so a second install is a no-op, and uninstall
//! strips the block while round-tripping every unrelated setting untouched.
//!
//! Per the HYBRID hook-mechanism decision (force+validate → inject+block → MCP
//! cooperative), a harness with a true pre-tool BLOCK reports [`GateCapability::Hard`];
//! one without degrades to [`GateCapability::Cooperative`] and never claims a gate
//! it cannot enforce.

pub mod claude_code;
pub mod codex;
pub mod gate_check;
pub mod grok;
pub mod hermes;
pub mod installer;
pub mod kiln;
pub mod mcp_registration;
pub mod opencode;
pub mod route_turn;
pub mod router_degradation;

use std::path::PathBuf;

/// The marker fence opening an anvil-managed block in a comment-bearing config
/// (TOML / YAML). Lines between the open and close fence are anvil-owned.
pub const FENCE_OPEN: &str = "# >>> anvil-hooks managed (do not edit) >>>";
/// The marker fence closing an anvil-managed block.
pub const FENCE_CLOSE: &str = "# <<< anvil-hooks managed <<<";

/// The marker key stamped into a structured (JSON) anvil-managed entry so it can
/// be found and removed without disturbing the user's own entries.
pub const MANAGED_KEY: &str = "_anvil_managed";

/// The default pre-tool matcher set the Claude Code gate hook fires on — the
/// mutating tools. PreToolUse matchers are `|`-joined tool-name regexes.
pub const DEFAULT_MATCHER: &str = "Edit|Write|MultiEdit|NotebookEdit";

/// The pre-tool matcher for the SUBAGENT-dispatch tool. A PreToolUse hook on this
/// fires when the agent spawns a subagent, letting us route the subagent's mission
/// through Anvil so subagent/automated turns reach the router for a decision — not
/// just top-level user prompts. This is load-bearing for coverage: ~95% of sessions
/// are subagents, which never emit `UserPromptSubmit`, so the spawn-time PreToolUse
/// hook is their ONLY routing entry point.
///
/// Claude Code renamed the subagent-spawn tool `Task` → `Agent` in v2.1.63. We match
/// BOTH (PreToolUse matchers are `|`-joined tool-name regexes) so the hook fires on
/// every Claude Code version — a bare `Task` matcher silently stops firing on ≥2.1.63
/// and makes all subagent work invisible to the router.
pub const SUBAGENT_MATCHER: &str = "Task|Agent";

/// The default runtime gate command native hooks invoke.
pub const DEFAULT_GATE_COMMAND: &str = "anvil-hooks gate-check";

/// The default per-turn routing command the user-prompt hooks invoke. Each
/// supported harness ships this ALONGSIDE the gate command so every user turn is
/// routed through anvil (recording the decision) and the routing guidance is
/// injected back to the agent.
pub const DEFAULT_TURN_COMMAND: &str = "anvil-hooks route-turn";

/// The default ROUTE-TURN hook timeout in milliseconds, installed into every
/// harness that renders a route-turn entry.
///
/// This must stay STRICTLY GREATER than `anvil-hooks`' own `ROUTE_TURN_TIMEOUT_MS`
/// (90000) so the binary's deadline fires first and LOGS the timeout, instead of the
/// harness killing the process and logging nothing. See
/// [`InstallSpec::turn_timeout_ms`] for why an inverted pair is worse than a short
/// one.
pub const DEFAULT_TURN_TIMEOUT_MS: u64 = 100_000;

/// A plugin/artifact FILE an adapter installs beside (or instead of) its config
/// transform — for harnesses whose per-turn hook is a separate artifact, not a
/// config-content edit (grok's `~/.grok/plugins/<p>/` plugin, opencode's
/// `~/.config/opencode/plugin/<p>.js`). `rel_path` is relative to the harness
/// CONFIG DIR; the content is pure (so it is unit/brine-testable) and the
/// installer does the filesystem write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookArtifact {
    /// Path relative to the harness config dir (e.g. `plugins/anvil-route-turn/hooks/hooks.json`).
    pub rel_path: PathBuf,
    /// File content.
    pub content: String,
    /// Whether to mark the file executable.
    pub executable: bool,
}

/// Whether a harness can enforce a true pre-mutation BLOCK or only cooperate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateCapability {
    /// The harness's pre-tool hook can refuse the mutation (e.g. Claude Code
    /// PreToolUse exit-2 / Kiln / Hermes native pre-tool hooks).
    Hard,
    /// The harness has no true pre-tool block wired; the adapter delivers a
    /// per-turn route hook that injects guidance but cannot refuse a mutation
    /// (e.g. a harness with only a `UserPromptSubmit` route hook).
    Cooperative,
}

impl GateCapability {
    /// The lowercase wire string ("hard" | "cooperative").
    pub fn as_str(self) -> &'static str {
        match self {
            GateCapability::Hard => "hard",
            GateCapability::Cooperative => "cooperative",
        }
    }
}

/// The six supported agent harnesses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Harness {
    ClaudeCode,
    Codex,
    Kiln,
    Hermes,
    /// xAI's Grok CLI (`~/.grok`, `config.toml`). FORCED via a drop-in plugin:
    /// the adapter writes `~/.grok/plugins/anvil-route-turn/` with a
    /// `UserPromptSubmit` hook that routes every turn. Trust-gated — grok only
    /// runs hooks in a project the user has trusted (`/hooks-trust`); there is no
    /// global trust-all, so it is "forced-once-trusted".
    Grok,
    /// opencode (`~/.config/opencode`, `opencode.jsonc`). FORCED via a drop-in JS
    /// plugin: the adapter writes `plugin/anvil-route-turn.js` whose `chat.message`
    /// hook routes each turn. Best-effort (the config is NEVER marked — opencode
    /// rejects unknown keys). The GATE axis stays cooperative.
    OpenCode,
}

impl Harness {
    /// Parse the `--harness` CLI value. `None` for an unknown value.
    pub fn parse(s: &str) -> Option<Harness> {
        match s {
            "claude-code" | "claude" => Some(Harness::ClaudeCode),
            "codex" => Some(Harness::Codex),
            "kiln" => Some(Harness::Kiln),
            "hermes" => Some(Harness::Hermes),
            "grok" => Some(Harness::Grok),
            "opencode" => Some(Harness::OpenCode),
            _ => None,
        }
    }

    /// The canonical lowercase id ("claude-code" | "codex" | "kiln" | "hermes" |
    /// "grok" | "opencode").
    pub fn id(self) -> &'static str {
        match self {
            Harness::ClaudeCode => "claude-code",
            Harness::Codex => "codex",
            Harness::Kiln => "kiln",
            Harness::Hermes => "hermes",
            Harness::Grok => "grok",
            Harness::OpenCode => "opencode",
        }
    }

    /// Every harness, in install order — used by `--harness auto` to install into
    /// every harness whose config dir is detected.
    pub fn all() -> [Harness; 6] {
        [
            Harness::ClaudeCode,
            Harness::Codex,
            Harness::Kiln,
            Harness::Hermes,
            Harness::Grok,
            Harness::OpenCode,
        ]
    }

    /// The conventional config DIRECTORY under `$HOME` for this harness.
    /// `auto` detects a harness by probing for this directory (the
    /// `--config-dir` flag overrides it).
    pub fn default_config_dir(self, home: &std::path::Path) -> PathBuf {
        match self {
            Harness::ClaudeCode => home.join(".claude"),
            Harness::Codex => home.join(".codex"),
            Harness::Kiln => home.join(".kiln"),
            Harness::Hermes => home.join(".hermes"),
            Harness::Grok => home.join(".grok"),
            // opencode follows the XDG convention: `~/.config/opencode`.
            Harness::OpenCode => home.join(".config").join("opencode"),
        }
    }

    /// The config FILE name (relative to the config dir) this harness's adapter
    /// reads and writes.
    pub fn config_filename(self) -> &'static str {
        match self {
            Harness::ClaudeCode => "settings.json",
            // Codex reads user-global hooks from `~/.codex/hooks.json` using
            // Claude Code's hook schema. Its MCP config stays in `config.toml`
            // (see `mcp_config_filename`) — hooks + MCP are separate files, the
            // same split Claude Code has (settings.json hooks + .claude.json MCP).
            Harness::Codex => "hooks.json",
            Harness::Kiln => "hooks.json",
            Harness::Hermes => "config.yaml",
            Harness::Grok => "config.toml",
            Harness::OpenCode => "opencode.jsonc",
        }
    }

    /// The per-turn routing command this harness installs, tagged with its
    /// `--source <id>` so every routed turn is self-describing (the engine
    /// records which harness it came from). Built from the spec's base
    /// `turn_command` (default `anvil-hooks route-turn`) plus this harness's
    /// canonical id, e.g. `anvil-hooks route-turn --source claude-code`.
    pub fn turn_command_with_source(self, spec: &InstallSpec) -> String {
        format!("{} --source {}", spec.turn_command, self.id())
    }

    /// The SUBAGENT-spawn routing command: identical to the per-turn command but
    /// tagged with a `-subagent` source suffix (e.g. `--source claude-code-subagent`).
    /// ~95% of sessions are subagents, and their spawn-time routes attribute to the
    /// PARENT conversation_hash — so without a distinct source tag they are
    /// indistinguishable from interactive user-turn routes in the activity log, and
    /// subagent coverage (the bulk of the all-traces denominator) can't be measured.
    pub fn subagent_command_with_source(self, spec: &InstallSpec) -> String {
        format!("{} --source {}-subagent", spec.turn_command, self.id())
    }

    /// This harness's adapter.
    pub fn adapter(self) -> Box<dyn HookAdapter> {
        match self {
            Harness::ClaudeCode => Box::new(claude_code::ClaudeCodeAdapter),
            Harness::Codex => Box::new(codex::CodexAdapter),
            // Kiln ships a Claude-Code-parity hook system (as-built): its real
            // adapter writes `~/.kiln/hooks.json`. Hermes gets its real
            // `hooks.pre_tool_call` YAML adapter. Both are our own harnesses with a
            // hard pre-tool gate.
            Harness::Kiln => Box::new(kiln::KilnAdapter),
            Harness::Hermes => Box::new(hermes::HermesAdapter),
            // Grok + opencode have no config-declarable pre-tool BLOCK, but each
            // ships a drop-in PLUGIN artifact carrying a real per-turn route hook
            // (grok UserPromptSubmit / opencode chat.message). See each adapter.
            Harness::Grok => Box::new(grok::GrokAdapter),
            Harness::OpenCode => Box::new(opencode::OpenCodeAdapter),
        }
    }
}

/// The install parameters threaded to every adapter.
#[derive(Debug, Clone)]
pub struct InstallSpec {
    /// The runtime gate command the native hook invokes (e.g. `anvil-hooks gate-check`).
    pub command: String,
    /// The GATE hook's timeout in milliseconds. `gate-check` is a fast local
    /// resolution with no model call, so this stays small: a hung gate blocks a
    /// file edit, and the user should find out quickly.
    pub timeout_ms: u64,
    /// The ROUTE-TURN hook's timeout in milliseconds — deliberately separate from
    /// [`Self::timeout_ms`], because the two hooks have nothing in common but their
    /// installer.
    ///
    /// A route turn makes model calls and was measured end-to-end at 8.5-9.5s. The
    /// gate does not. Sharing one number forced a choice between a gate that hangs
    /// for a minute and a route turn that is killed mid-flight; the route turn lost,
    /// silently, for weeks.
    ///
    /// # This value MUST exceed the binary's own route cap
    ///
    /// `anvil-hooks`' `ROUTE_TURN_TIMEOUT_MS` (90000) is the deadline the hook
    /// process applies to its OWN engine call, and it is the deadline that writes an
    /// `engine_timeout` delivery-log row on expiry. If the harness's cap (this value)
    /// is the lower of the two, the harness kills the process first and NO row is
    /// written — the failure becomes invisible, which is exactly how a total delivery
    /// outage stayed hidden. Keep this strictly greater than 90000.
    pub turn_timeout_ms: u64,
    /// The per-turn routing command the user-prompt hook invokes (e.g.
    /// `anvil-hooks route-turn`). Harnesses that support a user-prompt event
    /// install this ALONGSIDE the gate command.
    pub turn_command: String,
    /// The VERSION-STABLE command the anvil MCP server is registered with in each
    /// harness's MCP config. Foundry passes `${KIT_ROOT}/mcp/anvil-mcp`; standalone
    /// the binary derives the running `anvil-hooks`' sibling `anvil-mcp`.
    pub mcp_command: String,
}

impl Default for InstallSpec {
    fn default() -> Self {
        InstallSpec {
            command: DEFAULT_GATE_COMMAND.to_string(),
            timeout_ms: 5000,
            turn_timeout_ms: DEFAULT_TURN_TIMEOUT_MS,
            turn_command: DEFAULT_TURN_COMMAND.to_string(),
            mcp_command: String::new(),
        }
    }
}

/// A per-harness hook adapter: pure config-content transforms plus the harness's
/// gate capability. The binary owns reading/writing the file at the resolved
/// path; the adapter only transforms its content.
pub trait HookAdapter {
    /// The harness this adapter targets.
    fn harness(&self) -> Harness;

    /// Whether the harness can enforce a HARD pre-mutation block.
    fn gate_capability(&self) -> GateCapability;

    /// Return `existing` config content with the anvil-managed block installed
    /// (idempotently — any prior anvil block is replaced, never duplicated).
    /// `existing` empty means a fresh config file.
    fn install(&self, existing: &str, spec: &InstallSpec) -> Result<String, String>;

    /// Return `existing` config content with the anvil-managed block removed and
    /// every unrelated setting preserved. A config with no anvil block is
    /// returned effectively unchanged.
    fn uninstall(&self, existing: &str) -> Result<String, String>;

    /// Plugin/artifact files this adapter installs on top of the config
    /// transform, each path relative to the harness config dir. Pure (content
    /// only) so it is unit/brine-testable; the installer performs the filesystem
    /// write (creating parent dirs, setting the executable bit). Default: none —
    /// config-only adapters (Claude Code / Codex / Kiln / Hermes) write nothing
    /// here. Grok + opencode return their per-turn plugin files.
    fn artifacts(&self, _spec: &InstallSpec) -> Vec<HookArtifact> {
        Vec::new()
    }

    /// Artifact paths (relative to the harness config dir) to remove on
    /// uninstall — a file or a directory (removed recursively). Default: none.
    fn artifact_paths(&self) -> Vec<PathBuf> {
        Vec::new()
    }
}

/// Strip an anvil-managed fenced block (and the blank line that precedes it, if
/// any) from line-oriented config text. Idempotent and order-preserving for all
/// non-anvil lines. Used by the TOML/YAML adapters.
pub fn strip_fenced_block(existing: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut in_block = false;
    for line in existing.lines() {
        if line.trim() == FENCE_OPEN {
            in_block = true;
            // Drop a single trailing blank separator we may have inserted.
            if matches!(out.last(), Some(l) if l.trim().is_empty()) {
                out.pop();
            }
            continue;
        }
        if line.trim() == FENCE_CLOSE {
            in_block = false;
            continue;
        }
        if !in_block {
            out.push(line);
        }
    }
    let mut joined = out.join("\n");
    // Preserve a single trailing newline when the original had content.
    while joined.ends_with('\n') {
        joined.pop();
    }
    joined
}

/// Append an anvil-managed fenced block of `body` lines to line-oriented config
/// text, after stripping any prior block (idempotent). `body` is the inner
/// content (no fences); the fences are added here.
pub fn append_fenced_block(existing: &str, body: &str) -> String {
    let base = strip_fenced_block(existing);
    let mut out = String::new();
    if !base.trim().is_empty() {
        out.push_str(&base);
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str(FENCE_OPEN);
    out.push('\n');
    out.push_str(body.trim_end_matches('\n'));
    out.push('\n');
    out.push_str(FENCE_CLOSE);
    out.push('\n');
    out
}

/// Whether line-oriented config text carries an anvil-managed fenced block.
pub fn has_fenced_block(existing: &str) -> bool {
    existing.lines().any(|l| l.trim() == FENCE_OPEN)
}
