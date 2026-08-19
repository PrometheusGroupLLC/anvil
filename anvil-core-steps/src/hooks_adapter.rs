//! Step module for `hooks_adapter.feature` (core seam).
//!
//! Drives the pure per-harness [`HookAdapter`] string→string transforms over an
//! in-context config-content string. No filesystem: the adapter only transforms
//! content, so the steps hold the "config file" as a `String` in context and
//! assert on it directly (mirroring the pure posture of `hook_manifest_fold`).

#[allow(unused_imports)]
use anvil_core::domain::hooks::HookAdapter;
use anvil_core::domain::hooks::{
    claude_code, codex, grok, hermes, kiln, opencode, GateCapability, Harness, InstallSpec,
};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;
use std::process::Command;

const CONFIG_KEY: &str = "hooks_adapter_config";
const CAP_KEY: &str = "hooks_adapter_capability";
// Captured plugin/artifact files from the last install (paths newline-joined,
// contents concatenated) — the pure artifact content the installer would write.
const ART_PATHS_KEY: &str = "hooks_adapter_artifact_paths";
const ART_CONTENT_KEY: &str = "hooks_adapter_artifact_content";
// The captured error string of an install attempt that may fail (empty on success)
// — used to assert an adapter fails open on malformed input.
const ERR_KEY: &str = "hooks_adapter_error";
const CLI_HOME_KEY: &str = "hooks_adapter_cli_home";
const CLI_HOME_HANDLE_KEY: &str = "hooks_adapter_cli_home_handle";

fn carry(ctx: &mut Context, out: &mut Context) {
    if let Some(c) = ctx.take::<String>(CONFIG_KEY) {
        out.set(CONFIG_KEY, c);
    }
    if let Some(c) = ctx.take::<String>(CAP_KEY) {
        out.set(CAP_KEY, c);
    }
    if let Some(c) = ctx.take::<String>(ART_PATHS_KEY) {
        out.set(ART_PATHS_KEY, c);
    }
    if let Some(c) = ctx.take::<String>(ART_CONTENT_KEY) {
        out.set(ART_CONTENT_KEY, c);
    }
    if let Some(c) = ctx.take::<std::path::PathBuf>(CLI_HOME_KEY) {
        out.set(CLI_HOME_KEY, c);
    }
    if let Some(c) = ctx.take::<anvil_test_support::RetainedTempDir>(CLI_HOME_HANDLE_KEY) {
        out.set(CLI_HOME_HANDLE_KEY, c);
    }
}

fn install(ctx: &mut Context, harness: Harness, command: &str, timeout: u64) -> Result<(), String> {
    let existing = ctx.take::<String>(CONFIG_KEY).unwrap_or_default();
    let adapter = harness.adapter();
    let spec = InstallSpec {
        command: command.to_string(),
        timeout_ms: timeout,
        ..InstallSpec::default()
    };
    let next = adapter.install(&existing, &spec)?;
    // Capture the adapter's plugin/artifact files (pure content; the installer
    // does the filesystem write) so scenarios can assert on them.
    let arts = adapter.artifacts(&spec);
    let paths = arts
        .iter()
        .map(|a| a.rel_path.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let contents = arts
        .iter()
        .map(|a| a.content.clone())
        .collect::<Vec<_>>()
        .join("\n\u{1e}\n");
    ctx.set(CONFIG_KEY, next);
    ctx.set(CAP_KEY, adapter.gate_capability().as_str().to_string());
    ctx.set(ART_PATHS_KEY, paths);
    ctx.set(ART_CONTENT_KEY, contents);
    Ok(())
}

fn uninstall(ctx: &mut Context, harness: Harness) -> Result<(), String> {
    let existing = ctx.take::<String>(CONFIG_KEY).unwrap_or_default();
    let next = harness.adapter().uninstall(&existing)?;
    ctx.set(CONFIG_KEY, next);
    Ok(())
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ---- Given: seed config content ----
        step_def(
            "an empty Claude Code settings file",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, _p| {
                let mut out = Context::new();
                out.set(CONFIG_KEY, String::new());
                Ok(out)
            },
        ),
        step_def(
            "detected Claude Code and Codex config directories with legacy Anvil hooks",
            &[],
            &[
                (CLI_HOME_KEY, "PathBuf"),
                (CLI_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, _| {
                let (handle, home) = anvil_test_support::retained_temp_dir("anvil-auto-plugin-managed-")?;
                std::fs::create_dir_all(home.join(".claude")).map_err(|e| e.to_string())?;
                std::fs::create_dir_all(home.join(".codex")).map_err(|e| e.to_string())?;
                std::fs::write(
                    home.join(".claude/settings.json"),
                    serde_json::json!({
                        "hooks": {"UserPromptSubmit":[{"hooks":[{"type":"command","command":"anvil-hooks route-turn --source claude-code"}]}]}
                    })
                    .to_string(),
                )
                .map_err(|e| e.to_string())?;
                std::fs::write(
                    home.join(".codex/hooks.json"),
                    serde_json::json!({
                        "hooks": {"UserPromptSubmit":[{"hooks":[{"type":"command","command":"anvil-hooks route-turn --source codex"}]}]}
                    })
                    .to_string(),
                )
                .map_err(|e| e.to_string())?;
                let mut out = Context::new();
                out.set(CLI_HOME_KEY, home);
                out.set(CLI_HOME_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "a Claude Code settings file with an unrelated user hook and key {string} value {string}",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, params| {
                let key = params.get_string(0).ok_or("Expected key")?;
                let val = params.get_string(1).ok_or("Expected value")?;
                let json = serde_json::json!({
                    key: val,
                    "hooks": {
                        "PreToolUse": [
                            { "matcher": "Bash", "hooks": [ { "type": "command", "command": "user-own-hook" } ] }
                        ]
                    }
                });
                let mut out = Context::new();
                out.set(CONFIG_KEY, serde_json::to_string_pretty(&json).unwrap());
                Ok(out)
            },
        ),
        step_def(
            "a Claude Code settings file with untagged anvil gate and turn hooks",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, _p| {
                // Stale copies a PRIOR anvil-hooks version (or a second writer) left
                // behind: real anvil-hooks commands but WITHOUT the `_anvil_managed`
                // tag. Re-install must absorb these, not preserve them + add a
                // tagged copy on top (which fired the hook twice per turn).
                let json = serde_json::json!({
                    "hooks": {
                        "PreToolUse": [
                            { "matcher": "Edit|Write|MultiEdit|NotebookEdit",
                              "hooks": [ { "type": "command", "command": "anvil-hooks gate-check", "timeout": 5000 } ] },
                            { "matcher": "Task",
                              "hooks": [ { "type": "command", "command": "anvil-hooks route-turn --source claude-code", "timeout": 5000 } ] }
                        ],
                        "UserPromptSubmit": [
                            { "hooks": [ { "type": "command", "command": "anvil-hooks route-turn --source claude-code", "timeout": 5000 } ] }
                        ]
                    }
                });
                let mut out = Context::new();
                out.set(CONFIG_KEY, serde_json::to_string_pretty(&json).unwrap());
                Ok(out)
            },
        ),
        step_def(
            "an empty Codex hooks file",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, _p| {
                let mut out = Context::new();
                out.set(CONFIG_KEY, String::new());
                Ok(out)
            },
        ),
        step_def(
            "a Codex hooks.json with an operator-authored UserPromptSubmit hook",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, _p| {
                // An operator-authored ~/.codex/hooks.json (Claude-Code hook schema:
                // events under a top-level `hooks` object) that must survive anvil
                // install + uninstall untouched.
                let json = serde_json::json!({
                    "description": "operator hooks",
                    "hooks": {
                        "UserPromptSubmit": [
                            { "hooks": [ { "type": "command", "command": "operator-own-hook" } ] }
                        ]
                    }
                });
                let mut out = Context::new();
                out.set(CONFIG_KEY, serde_json::to_string_pretty(&json).unwrap());
                Ok(out)
            },
        ),
        step_def(
            "a Codex hooks.json with an untagged anvil route hook",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, _p| {
                // A stale copy a PRIOR anvil-hooks version left behind: a real
                // `anvil-hooks route-turn` command WITHOUT the `_anvil_managed` tag.
                // Re-install must absorb it, not add a tagged copy on top (which
                // would fire the route hook twice per turn).
                let json = serde_json::json!({
                    "hooks": {
                        "UserPromptSubmit": [
                            { "hooks": [ { "type": "command", "command": "anvil-hooks route-turn --source codex", "timeout": 5000 } ] }
                        ]
                    }
                });
                let mut out = Context::new();
                out.set(CONFIG_KEY, serde_json::to_string_pretty(&json).unwrap());
                Ok(out)
            },
        ),
        step_def(
            "a Codex hooks.json with an operator-authored anvil-hooks audit hook",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, _p| {
                // An operator's OWN anvil-hooks subcommand (NOT the route hook).
                // Absorption must match only the canonical `anvil-hooks route-turn`
                // shape, so this hook survives install AND uninstall untouched.
                let json = serde_json::json!({
                    "hooks": {
                        "UserPromptSubmit": [
                            { "hooks": [ { "type": "command", "command": "anvil-hooks audit", "timeout": 5 } ] }
                        ]
                    }
                });
                let mut out = Context::new();
                out.set(CONFIG_KEY, serde_json::to_string_pretty(&json).unwrap());
                Ok(out)
            },
        ),
        step_def(
            "a Codex hooks.json with a mixed group of a stale anvil route handler and an operator handler",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, _p| {
                // A single UserPromptSubmit entry (group) holding BOTH a stale
                // untagged anvil route handler and an operator handler. Install must
                // surgically drop only our handler and keep the operator sibling.
                let json = serde_json::json!({
                    "hooks": {
                        "UserPromptSubmit": [
                            { "hooks": [
                                { "type": "command", "command": "anvil-hooks route-turn --source codex", "timeout": 5000 },
                                { "type": "command", "command": "operator-own-hook" }
                            ] }
                        ]
                    }
                });
                let mut out = Context::new();
                out.set(CONFIG_KEY, serde_json::to_string_pretty(&json).unwrap());
                Ok(out)
            },
        ),
        step_def(
            "a Codex hooks.json with a malformed UserPromptSubmit container",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, _p| {
                // Valid JSON, but `hooks.UserPromptSubmit` is a STRING, not an array.
                // The adapter must fail open (leave it untouched), not silently
                // replace the operator's malformed-but-present content.
                let json = serde_json::json!({
                    "description": "operator",
                    "hooks": { "UserPromptSubmit": "totally-not-an-array" }
                });
                let mut out = Context::new();
                out.set(CONFIG_KEY, serde_json::to_string_pretty(&json).unwrap());
                Ok(out)
            },
        ),
        step_def(
            "a Codex hooks.json with a malformed PreToolUse container",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, _p| {
                let mut out = Context::new();
                out.set(
                    CONFIG_KEY,
                    serde_json::json!({
                        "description": "operator",
                        "hooks": { "PreToolUse": "totally-not-an-array" }
                    })
                    .to_string(),
                );
                Ok(out)
            },
        ),
        step_def(
            "a Codex hooks.json with an exact managed global route and an unrelated operator route",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, _p| {
                let mut out = Context::new();
                out.set(
                    CONFIG_KEY,
                    serde_json::json!({
                        "hooks": {
                            "UserPromptSubmit": [
                                {"_anvil_managed": true, "hooks": [{"type":"command","command":"anvil-hooks route-turn --source codex"}]},
                                {"hooks": [{"type":"command","command":"operator-own-hook"}]}
                            ],
                            "PreToolUse": [
                                {"_anvil_managed": true, "matcher":"apply_patch|Bash", "hooks":[{"type":"command","command":"anvil-hooks gate-check --source codex --hard-enforce track --internal-deadline-ms 4000"}]}
                            ]
                        }
                    })
                    .to_string(),
                );
                Ok(out)
            },
        ),
        step_def(
            "an empty Kiln config file",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, _p| {
                let mut out = Context::new();
                out.set(CONFIG_KEY, String::new());
                Ok(out)
            },
        ),
        step_def(
            "a Kiln hooks.json with an operator-authored PreToolUse hook",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, _p| {
                // An operator-authored hooks.json entry (no _managed_by marker)
                // that must survive anvil install + uninstall untouched.
                let json = serde_json::json!({
                    "PreToolUse": [
                        { "matcher": "bash",
                          "hooks": [ { "type": "command", "command": "operator-own-hook" } ] }
                    ]
                });
                let mut out = Context::new();
                out.set(CONFIG_KEY, serde_json::to_string_pretty(&json).unwrap());
                Ok(out)
            },
        ),
        step_def(
            "an empty Hermes config file",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, _p| {
                let mut out = Context::new();
                out.set(CONFIG_KEY, String::new());
                Ok(out)
            },
        ),
        step_def(
            "a Hermes config file with key {string} value {string}",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, params| {
                let key = params.get_string(0).ok_or("Expected key")?;
                let val = params.get_string(1).ok_or("Expected value")?;
                let mut out = Context::new();
                out.set(CONFIG_KEY, format!("{}: {}\n", key, val));
                Ok(out)
            },
        ),
        step_def(
            "a realistic Hermes config with comments, an unrelated key, and a user pre_tool_call hook",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, _p| {
                // A comment-bearing fixture with an unrelated key AND a
                // pre-existing user hook under hooks.pre_tool_call that must
                // survive install/uninstall byte-for-byte.
                let cfg = concat!(
                    "# Hermes hand-maintained config (do not clobber my comments!)\n",
                    "approvals_mode: smart\n",
                    "hooks:\n",
                    "  pre_tool_call:\n",
                    "    - matcher: \"terminal\"  # user's own guard\n",
                    "      command: \"~/.hermes/agent-hooks/block-rm-rf.sh\"\n",
                    "      timeout: 10\n",
                    "hooks_auto_accept: false\n",
                );
                let mut out = Context::new();
                out.set(CONFIG_KEY, cfg.to_string());
                Ok(out)
            },
        ),
        step_def(
            "a Hermes config whose anvil entries lost their markers to a serde round-trip",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, _p| {
                // Byte-faithful to what Foundry's Hermes MCP writer leaves behind: comments gone
                // (serde carries none), scalars unquoted, and every sequence emitted with its `-`
                // at the PARENT key's indent rather than nested under it. `approvals_mode` stands
                // in for the user keys that survive the round-trip, so the scenario can also prove
                // the repair does not eat them.
                let cfg = concat!(
                    "approvals_mode: smart\n",
                    "hooks:\n",
                    "  pre_llm_call:\n",
                    "  - command: anvil-hooks route-turn --source hermes\n",
                    "    timeout: 5\n",
                    "  pre_tool_call:\n",
                    "  - matcher: write_file|patch\n",
                    "    command: anvil-hooks gate-check\n",
                    "    timeout: 5\n",
                    "mcp_servers:\n",
                    "  anvil-mcp:\n",
                    "    command: /Users/x/.foundry/bin/foundry-mcp-dev-proxy\n",
                );
                let mut out = Context::new();
                out.set(CONFIG_KEY, cfg.to_string());
                Ok(out)
            },
        ),
        step_def(
            "an empty Grok config file",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, _p| {
                let mut out = Context::new();
                out.set(CONFIG_KEY, String::new());
                Ok(out)
            },
        ),
        step_def(
            "a Grok config file with key {string} value {string}",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, params| {
                let key = params.get_string(0).ok_or("Expected key")?;
                let val = params.get_string(1).ok_or("Expected value")?;
                let mut out = Context::new();
                out.set(CONFIG_KEY, format!("{} = {}\n", key, val));
                Ok(out)
            },
        ),
        step_def(
            "an empty opencode config file",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, _p| {
                let mut out = Context::new();
                out.set(CONFIG_KEY, String::new());
                Ok(out)
            },
        ),
        step_def(
            "an opencode config with an existing mcp server {string}",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, params| {
                let name = params.get_string(0).ok_or("Expected server name")?;
                let json = serde_json::json!({
                    "mcp": {
                        name: { "type": "local", "command": ["user-own-server"] }
                    }
                });
                let mut out = Context::new();
                out.set(CONFIG_KEY, serde_json::to_string_pretty(&json).unwrap());
                Ok(out)
            },
        ),
        step_def(
            "an opencode config broken by a legacy anvil marker",
            &[],
            &[(CONFIG_KEY, "String")],
            |_ctx, _p| {
                // A config an OLD anvil version broke by stamping `_anvil_managed`
                // (opencode rejects unknown top-level keys). Install must repair it.
                let json = serde_json::json!({
                    "_anvil_managed": { "gate": "cooperative" },
                    "mcp": { "other": { "type": "local", "command": ["user-own-server"] } }
                });
                let mut out = Context::new();
                out.set(CONFIG_KEY, serde_json::to_string_pretty(&json).unwrap());
                Ok(out)
            },
        ),
        // ---- When: install / uninstall ----
        install_step("the Claude Code adapter installs with command {string} and timeout {int}", Harness::ClaudeCode),
        install_step("the Codex adapter installs with command {string} and timeout {int}", Harness::Codex),
        step_def(
            "the Codex adapter absorbs the legacy global route for plugin management",
            &[(CONFIG_KEY, "String")],
            &[(CONFIG_KEY, "String")],
            |mut ctx, _params| {
                let existing = ctx.take::<String>(CONFIG_KEY).unwrap_or_default();
                let next = codex::absorb_legacy_global_route(&existing)?;
                let mut out = Context::new();
                out.set(CONFIG_KEY, next);
                Ok(out)
            },
        ),
        step_def(
            "anvil-hooks installs automatically in Codex plugin-managed mode",
            &[(CLI_HOME_KEY, "PathBuf")],
            &[
                (CLI_HOME_KEY, "PathBuf"),
                (CLI_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, _| {
                anvil_test_support::harness::ensure_binary("anvil-hooks");
                let binary = anvil_test_support::harness::binary_path("anvil-hooks");
                let home = ctx.get::<PathBuf>(CLI_HOME_KEY).ok_or("missing home")?;
                let output = Command::new(binary)
                    .args([
                        "install",
                        "--harness",
                        "auto",
                        "--codex-plugin-managed",
                    ])
                    .env("HOME", home)
                    .output()
                    .map_err(|e| e.to_string())?;
                if !output.status.success() {
                    return Err(format!(
                        "anvil-hooks failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    ));
                }
                let mut out = Context::new();
                if let Some(path) = ctx.get::<PathBuf>(CLI_HOME_KEY) {
                    out.set(CLI_HOME_KEY, path.clone());
                }
                anvil_test_support::carry_retained_temp_dir(&ctx, &mut out, CLI_HOME_HANDLE_KEY);
                Ok(out)
            },
        ),
        install_step("the Kiln adapter installs with command {string} and timeout {int}", Harness::Kiln),
        install_step("the Hermes adapter installs with command {string} and timeout {int}", Harness::Hermes),
        install_step("the Grok adapter installs with command {string} and timeout {int}", Harness::Grok),
        install_step("the opencode adapter installs with command {string} and timeout {int}", Harness::OpenCode),
        uninstall_step("the Claude Code adapter uninstalls", Harness::ClaudeCode),
        uninstall_step("the Codex adapter uninstalls", Harness::Codex),
        step_def(
            "the Codex adapter install is attempted",
            &[(CONFIG_KEY, "String")],
            &[(CONFIG_KEY, "String"), (ERR_KEY, "String")],
            |mut ctx, _p| {
                let existing = ctx.take::<String>(CONFIG_KEY).unwrap_or_default();
                let spec = InstallSpec {
                    command: "anvil-hooks gate-check".to_string(),
                    timeout_ms: 5000,
                    ..InstallSpec::default()
                };
                let mut out = Context::new();
                match Harness::Codex.adapter().install(&existing, &spec) {
                    // Success: carry the transformed config, no error.
                    Ok(next) => {
                        out.set(CONFIG_KEY, next);
                        out.set(ERR_KEY, String::new());
                    }
                    // Fail open: leave the ORIGINAL config untouched, record the error.
                    Err(e) => {
                        out.set(CONFIG_KEY, existing);
                        out.set(ERR_KEY, e);
                    }
                }
                Ok(out)
            },
        ),
        uninstall_step("the Hermes adapter uninstalls", Harness::Hermes),
        uninstall_step("the Grok adapter uninstalls", Harness::Grok),
        uninstall_step("the opencode adapter uninstalls", Harness::OpenCode),
        uninstall_step("the Kiln adapter uninstalls", Harness::Kiln),
        // ---- Then: assertions ----
        check_def(
            "the Claude Code settings has a PreToolUse hook with matcher {string}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected matcher")?;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let detail = claude_code::managed_pretool_detail(cfg)
                    .ok_or("No anvil-managed PreToolUse hook")?;
                if detail.0 == want {
                    Ok(())
                } else {
                    Err(format!("matcher: expected {}, got {}", want, detail.0))
                }
            },
        ),
        check_def(
            "the Claude Code settings PreToolUse hook command is {string}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected command")?;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let detail = claude_code::managed_pretool_detail(cfg)
                    .ok_or("No anvil-managed PreToolUse hook")?;
                if detail.1 == want {
                    Ok(())
                } else {
                    Err(format!("command: expected {}, got {}", want, detail.1))
                }
            },
        ),
        check_def(
            "the Claude Code settings PreToolUse hook timeout is {int}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_int(0).ok_or("Expected timeout")? as u64;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let detail = claude_code::managed_pretool_detail(cfg)
                    .ok_or("No anvil-managed PreToolUse hook")?;
                if detail.2 == want {
                    Ok(())
                } else {
                    Err(format!("timeout: expected {}, got {}", want, detail.2))
                }
            },
        ),
        check_def(
            "the Claude Code settings has exactly {int} anvil-managed PreToolUse hook",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_int(0).ok_or("Expected count")? as usize;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let got = claude_code::managed_pretool_count(cfg);
                if got == want {
                    Ok(())
                } else {
                    Err(format!("managed count: expected {}, got {}", want, got))
                }
            },
        ),
        check_def(
            "the Claude Code settings has no untagged anvil hook",
            &[(CONFIG_KEY, "String")],
            |ctx, _params| {
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let got = claude_code::untagged_anvil_count(cfg);
                if got == 0 {
                    Ok(())
                } else {
                    Err(format!(
                        "expected 0 untagged anvil hooks, found {} (each is a duplicate waiting to happen)",
                        got
                    ))
                }
            },
        ),
        check_def(
            "the Claude Code settings has a subagent-route PreToolUse hook with matcher {string} and command {string}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want_matcher = params.get_string(0).ok_or("Expected matcher")?;
                let want_cmd = params.get_string(1).ok_or("Expected command")?;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let (m, c, _t) = claude_code::managed_subagent_detail(cfg)
                    .ok_or("No anvil-managed subagent-route (Task) PreToolUse hook")?;
                if m == want_matcher && c == want_cmd {
                    Ok(())
                } else {
                    Err(format!("subagent hook: matcher {:?} cmd {:?}", m, c))
                }
            },
        ),
        check_def(
            "the Claude Code settings has a UserPromptSubmit hook with command {string}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected command")?;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let got = claude_code::managed_turn_command(cfg)
                    .ok_or("No anvil-managed UserPromptSubmit hook")?;
                if got == want {
                    Ok(())
                } else {
                    Err(format!("turn command: expected {}, got {}", want, got))
                }
            },
        ),
        check_def(
            "the Claude Code settings has exactly {int} anvil-managed UserPromptSubmit hook",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_int(0).ok_or("Expected count")? as usize;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let got = claude_code::managed_turn_count(cfg);
                if got == want {
                    Ok(())
                } else {
                    Err(format!("turn count: expected {}, got {}", want, got))
                }
            },
        ),
        check_def(
            "the Hermes config has a pre_llm_call entry command {string}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected command")?;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let detail = hermes::managed_turn_detail(cfg)
                    .ok_or("No anvil-managed pre_llm_call entry")?;
                if detail.0 == want {
                    Ok(())
                } else {
                    Err(format!("turn command: expected {}, got {}", want, detail.0))
                }
            },
        ),
        check_def(
            "the Hermes config has exactly {int} anvil-managed pre_llm_call entry",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_int(0).ok_or("Expected count")? as usize;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let got = hermes::managed_turn_count(cfg);
                if got == want {
                    Ok(())
                } else {
                    Err(format!("turn count: expected {}, got {}", want, got))
                }
            },
        ),
        check_def(
            "the Claude Code settings has no anvil-managed PreToolUse hook",
            &[(CONFIG_KEY, "String")],
            |ctx, _p| {
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                if claude_code::managed_pretool_count(cfg) == 0 {
                    Ok(())
                } else {
                    Err("anvil-managed PreToolUse hook still present".to_string())
                }
            },
        ),
        check_def(
            "the Claude Code settings still has the unrelated user hook",
            &[(CONFIG_KEY, "String")],
            |ctx, _p| {
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                if cfg.contains("user-own-hook") {
                    Ok(())
                } else {
                    Err("unrelated user hook was removed".to_string())
                }
            },
        ),
        check_def(
            "the Claude Code settings still has key {string} value {string}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected key")?;
                let val = params.get_string(1).ok_or("Expected value")?;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let parsed: serde_json::Value =
                    serde_json::from_str(cfg).map_err(|e| format!("parse: {}", e))?;
                let got = parsed.get(key).and_then(|v| v.as_str());
                if got == Some(val) {
                    Ok(())
                } else {
                    Err(format!("key {}: expected {:?}, got {:?}", key, val, got))
                }
            },
        ),
        check_def(
            "the Codex hooks has a UserPromptSubmit hook with command {string}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected command")?;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let got = codex::managed_turn_command(cfg)
                    .ok_or("No anvil-managed UserPromptSubmit hook")?;
                if got == want {
                    Ok(())
                } else {
                    Err(format!("turn command: expected {}, got {}", want, got))
                }
            },
        ),
        check_def(
            "Claude Code has its normal native Anvil hooks",
            &[(CLI_HOME_KEY, "PathBuf")],
            |ctx, _| {
                let home = ctx.get::<PathBuf>(CLI_HOME_KEY).ok_or("missing home")?;
                let text = std::fs::read_to_string(home.join(".claude/settings.json"))
                    .map_err(|e| e.to_string())?;
                (claude_code::managed_pretool_count(&text) == 2
                    && claude_code::managed_turn_count(&text) == 1)
                    .then_some(())
                    .ok_or_else(|| format!("Claude native hooks not installed: {text}"))
            },
        ),
        check_def(
            "Codex has no global Anvil route or gate",
            &[(CLI_HOME_KEY, "PathBuf")],
            |ctx, _| {
                let home = ctx.get::<PathBuf>(CLI_HOME_KEY).ok_or("missing home")?;
                let text = std::fs::read_to_string(home.join(".codex/hooks.json"))
                    .map_err(|e| e.to_string())?;
                (codex::managed_turn_count(&text) == 0
                    && codex::managed_gate_count(&text) == 0
                    && !text.contains("anvil-hooks route-turn"))
                    .then_some(())
                    .ok_or_else(|| format!("Codex global hooks remain: {text}"))
            },
        ),
        check_def(
            "the Codex hooks has exactly {int} anvil-managed UserPromptSubmit hook",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_int(0).ok_or("Expected count")? as usize;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let got = codex::managed_turn_count(cfg);
                if got == want {
                    Ok(())
                } else {
                    Err(format!("UserPromptSubmit count: expected {}, got {}", want, got))
                }
            },
        ),
        check_def(
            "the Codex hooks has exactly {int} anvil-managed PreToolUse hook",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_int(0).ok_or("Expected count")? as usize;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let got = codex::managed_gate_count(cfg);
                if got == want {
                    Ok(())
                } else {
                    Err(format!("PreToolUse count: expected {want}, got {got}"))
                }
            },
        ),
        check_def(
            "the Codex hooks has a PreToolUse hook with matcher {string} and command {string}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let matcher = params.get_string(0).ok_or("Expected matcher")?;
                let command = params.get_string(1).ok_or("Expected command")?;
                let cfg: serde_json::Value = serde_json::from_str(
                    ctx.get::<String>(CONFIG_KEY).ok_or("No config")?,
                )
                .map_err(|e| e.to_string())?;
                let entry = cfg
                    .pointer("/hooks/PreToolUse")
                    .and_then(|value| value.as_array())
                    .and_then(|entries| entries.iter().find(|entry| {
                        entry.get("_anvil_managed").and_then(|value| value.as_bool()) == Some(true)
                    }))
                    .ok_or("No managed PreToolUse entry")?;
                let actual_matcher = entry.get("matcher").and_then(|value| value.as_str());
                let actual_command = entry
                    .pointer("/hooks/0/command")
                    .and_then(|value| value.as_str());
                if actual_matcher == Some(matcher) && actual_command == Some(command) {
                    Ok(())
                } else {
                    Err(format!(
                        "Codex gate mismatch: matcher {actual_matcher:?}, command {actual_command:?}"
                    ))
                }
            },
        ),
        check_def(
            "the Codex hooks still has the operator hook",
            &[(CONFIG_KEY, "String")],
            |ctx, _p| {
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                if cfg.contains("operator-own-hook") {
                    Ok(())
                } else {
                    Err("operator hook was removed".to_string())
                }
            },
        ),
        check_def(
            "the Codex hooks still has the operator audit hook",
            &[(CONFIG_KEY, "String")],
            |ctx, _p| {
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                if cfg.contains("anvil-hooks audit") {
                    Ok(())
                } else {
                    Err("operator's anvil-hooks audit hook was removed".to_string())
                }
            },
        ),
        check_def(
            "the Codex hooks.json is exactly the codex route-hook shape with command {string} and timeout {int}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let cmd = params.get_string(0).ok_or("Expected command")?;
                let secs = params.get_int(1).ok_or("Expected timeout")?;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                // The COMPLETE codex 0.144 hooks.json shape after a fresh install:
                // exact nesting, a single tagged UserPromptSubmit entry with one
                // command handler whose timeout is in SECONDS.
                let expected = serde_json::json!({
                    "hooks": {
                        "UserPromptSubmit": [
                            {
                                "_anvil_managed": true,
                                "hooks": [
                                    { "type": "command", "command": cmd, "timeout": secs }
                                ]
                            }
                        ],
                        "PreToolUse": [
                            {
                                "_anvil_managed": true,
                                "matcher": "apply_patch|Bash",
                                "hooks": [
                                    { "type": "command", "command": "anvil-hooks gate-check --source codex --hard-enforce track --internal-deadline-ms 4000", "timeout": secs }
                                ]
                            }
                        ]
                    }
                });
                let got: serde_json::Value = serde_json::from_str(cfg)
                    .map_err(|e| format!("config is not valid JSON: {}", e))?;
                if got == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "hooks.json shape mismatch:\nexpected {:#}\ngot      {:#}",
                        expected, got
                    ))
                }
            },
        ),
        check_def(
            "the Codex adapter install reports an error",
            &[(ERR_KEY, "String")],
            |ctx, _p| {
                let err = ctx.get::<String>(ERR_KEY).ok_or("No error recorded")?;
                if err.is_empty() {
                    Err("expected install to fail open, but it succeeded".to_string())
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the Codex hooks config is left untouched",
            &[(CONFIG_KEY, "String")],
            |ctx, _p| {
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                if cfg.contains("totally-not-an-array")
                    && codex::managed_turn_count(cfg) == 0
                    && codex::managed_gate_count(cfg) == 0
                {
                    Ok(())
                } else {
                    Err(format!("malformed config was modified:\n{}", cfg))
                }
            },
        ),
        check_def(
            "the Kiln config has an anvil-managed pre-tool hook",
            &[(CONFIG_KEY, "String")],
            |ctx, _p| {
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                if kiln::has_managed_pretool(cfg) {
                    Ok(())
                } else {
                    Err("no anvil-managed pre-tool block".to_string())
                }
            },
        ),
        check_def(
            "the Kiln config PreToolUse hook has matcher {string}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected matcher")?;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let detail =
                    kiln::managed_pretool_detail(cfg).ok_or("No anvil-managed PreToolUse hook")?;
                if detail.0 == want {
                    Ok(())
                } else {
                    Err(format!("matcher: expected {}, got {}", want, detail.0))
                }
            },
        ),
        check_def(
            "the Kiln config PreToolUse hook command is {string}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected command")?;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let detail =
                    kiln::managed_pretool_detail(cfg).ok_or("No anvil-managed PreToolUse hook")?;
                if detail.1 == want {
                    Ok(())
                } else {
                    Err(format!("command: expected {}, got {}", want, detail.1))
                }
            },
        ),
        check_def(
            "the Kiln config PreToolUse hook timeout is {int}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_int(0).ok_or("Expected timeout")? as u64;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let detail =
                    kiln::managed_pretool_detail(cfg).ok_or("No anvil-managed PreToolUse hook")?;
                if detail.2 == want {
                    Ok(())
                } else {
                    Err(format!("timeout: expected {}, got {}", want, detail.2))
                }
            },
        ),
        check_def(
            "the Kiln config has a UserPromptSubmit hook with command {string}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected command")?;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let got = kiln::managed_turn_command(cfg)
                    .ok_or("No anvil-managed UserPromptSubmit hook")?;
                if got == want {
                    Ok(())
                } else {
                    Err(format!("turn command: expected {}, got {}", want, got))
                }
            },
        ),
        check_def(
            "the Kiln config has exactly {int} anvil-managed PreToolUse hook",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_int(0).ok_or("Expected count")? as usize;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let got = kiln::managed_pretool_count(cfg);
                if got == want {
                    Ok(())
                } else {
                    Err(format!("PreToolUse count: expected {}, got {}", want, got))
                }
            },
        ),
        check_def(
            "the Kiln config has exactly {int} anvil-managed UserPromptSubmit hook",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_int(0).ok_or("Expected count")? as usize;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let got = kiln::managed_turn_count(cfg);
                if got == want {
                    Ok(())
                } else {
                    Err(format!("UserPromptSubmit count: expected {}, got {}", want, got))
                }
            },
        ),
        check_def(
            "the Kiln config still has the operator hook",
            &[(CONFIG_KEY, "String")],
            |ctx, _p| {
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                if cfg.contains("operator-own-hook") {
                    Ok(())
                } else {
                    Err("operator hook was removed".to_string())
                }
            },
        ),
        check_def(
            "the Hermes config has an anvil-managed pre-tool hook",
            &[(CONFIG_KEY, "String")],
            |ctx, _p| {
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                if hermes::has_managed_pretool(cfg) {
                    Ok(())
                } else {
                    Err("no anvil-managed pre-tool block".to_string())
                }
            },
        ),
        check_def(
            "the Hermes config has no anvil-managed pre-tool hook",
            &[(CONFIG_KEY, "String")],
            |ctx, _p| {
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                if !hermes::has_managed_pretool(cfg) {
                    Ok(())
                } else {
                    Err("anvil-managed pre-tool block still present".to_string())
                }
            },
        ),
        check_def(
            "the Hermes config still has key {string} value {string}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected key")?;
                let val = params.get_string(1).ok_or("Expected value")?;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let needle = format!("{}: {}", key, val);
                if cfg.contains(&needle) {
                    Ok(())
                } else {
                    Err(format!("missing line '{}' in:\n{}", needle, cfg))
                }
            },
        ),
        check_def(
            "the Hermes config parses as valid YAML",
            &[(CONFIG_KEY, "String")],
            |ctx, _params| {
                // The defect this guards was NOT a wrong value — it was a file no parser would
                // accept (sequence items sitting as siblings of a mapping's keys). Counting
                // entries alone would have passed straight through it, because the counters read
                // the text line-by-line and never parse.
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                serde_yaml::from_str::<serde_yaml::Value>(cfg)
                    .map(|_| ())
                    .map_err(|e| format!("the written Hermes config is not valid YAML: {e}\n{cfg}"))
            },
        ),
        check_def(
            "the Hermes config has a pre_tool_call entry with matcher {string}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected matcher")?;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let detail = hermes::managed_pretool_detail(cfg)
                    .ok_or("No anvil-managed pre_tool_call entry")?;
                if detail.0 == want {
                    Ok(())
                } else {
                    Err(format!("matcher: expected {}, got {}", want, detail.0))
                }
            },
        ),
        check_def(
            "the Hermes config pre_tool_call entry command is {string}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected command")?;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let detail = hermes::managed_pretool_detail(cfg)
                    .ok_or("No anvil-managed pre_tool_call entry")?;
                if detail.1 == want {
                    Ok(())
                } else {
                    Err(format!("command: expected {}, got {}", want, detail.1))
                }
            },
        ),
        check_def(
            "the Hermes config pre_tool_call entry timeout is {int}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_int(0).ok_or("Expected timeout")? as u64;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let detail = hermes::managed_pretool_detail(cfg)
                    .ok_or("No anvil-managed pre_tool_call entry")?;
                if detail.2 == want {
                    Ok(())
                } else {
                    Err(format!("timeout: expected {}, got {}", want, detail.2))
                }
            },
        ),
        check_def(
            "the Hermes config has exactly {int} anvil-managed pre_tool_call entry",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_int(0).ok_or("Expected count")? as usize;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let got = hermes::managed_pretool_count(cfg);
                if got == want {
                    Ok(())
                } else {
                    Err(format!("managed count: expected {}, got {}", want, got))
                }
            },
        ),
        check_def(
            "the Hermes config still has the user pre_tool_call hook",
            &[(CONFIG_KEY, "String")],
            |ctx, _p| {
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                if cfg.contains("block-rm-rf.sh") && cfg.contains("matcher: \"terminal\"") {
                    Ok(())
                } else {
                    Err(format!("user pre_tool_call hook was lost in:\n{}", cfg))
                }
            },
        ),
        check_def(
            "the Hermes config still has the leading comment",
            &[(CONFIG_KEY, "String")],
            |ctx, _p| {
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                if cfg.contains("# Hermes hand-maintained config (do not clobber my comments!)") {
                    Ok(())
                } else {
                    Err(format!("leading comment was lost in:\n{}", cfg))
                }
            },
        ),
        check_def(
            "the Grok config has an anvil-managed cooperative block",
            &[(CONFIG_KEY, "String")],
            |ctx, _p| {
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                if grok::has_managed_block(cfg) {
                    Ok(())
                } else {
                    Err("no anvil-managed cooperative block".to_string())
                }
            },
        ),
        check_def(
            "the Grok config has no anvil-managed cooperative block",
            &[(CONFIG_KEY, "String")],
            |ctx, _p| {
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                if !grok::has_managed_block(cfg) {
                    Ok(())
                } else {
                    Err("anvil-managed cooperative block still present".to_string())
                }
            },
        ),
        check_def(
            "the Grok config still has the line {string}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected line")?;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                if cfg.lines().any(|l| l.trim() == want) {
                    Ok(())
                } else {
                    Err(format!("missing line '{}' in:\n{}", want, cfg))
                }
            },
        ),
        check_def(
            "the config contains {string}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected substring")?;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                if cfg.contains(want) {
                    Ok(())
                } else {
                    Err(format!("config does not contain '{}':\n{}", want, cfg))
                }
            },
        ),
        check_def(
            "the opencode config has an anvil-managed cooperative marker",
            &[(CONFIG_KEY, "String")],
            |ctx, _p| {
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                if opencode::has_managed_marker(cfg) {
                    Ok(())
                } else {
                    Err("no anvil-managed cooperative marker".to_string())
                }
            },
        ),
        check_def(
            "the opencode config has exactly {int} anvil-managed cooperative marker",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let want = params.get_int(0).ok_or("Expected count")? as usize;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let got = opencode::managed_marker_count(cfg);
                if got == want {
                    Ok(())
                } else {
                    Err(format!("marker count: expected {}, got {}", want, got))
                }
            },
        ),
        check_def(
            "the opencode config still has mcp server {string}",
            &[(CONFIG_KEY, "String")],
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected server name")?;
                let cfg = ctx.get::<String>(CONFIG_KEY).ok_or("No config")?;
                let parsed: serde_json::Value =
                    serde_json::from_str(cfg).map_err(|e| format!("parse: {}", e))?;
                if parsed.get("mcp").and_then(|m| m.get(name)).is_some() {
                    Ok(())
                } else {
                    Err(format!("mcp server '{}' was lost in:\n{}", name, cfg))
                }
            },
        ),
        check_def(
            "an artifact is installed at {string}",
            &[(ART_PATHS_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected path")?;
                let paths = ctx.get::<String>(ART_PATHS_KEY).ok_or("No artifacts captured")?;
                if paths.lines().any(|l| l == want) {
                    Ok(())
                } else {
                    Err(format!("no artifact at '{}'; got:\n{}", want, paths))
                }
            },
        ),
        check_def(
            "an installed artifact contains {string}",
            &[(ART_CONTENT_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected substring")?;
                let content = ctx
                    .get::<String>(ART_CONTENT_KEY)
                    .ok_or("No artifacts captured")?;
                if content.contains(want) {
                    Ok(())
                } else {
                    Err(format!("no artifact contains '{}'; got:\n{}", want, content))
                }
            },
        ),
        check_def(
            "the Grok adapter reports gate capability {string}",
            &[(CAP_KEY, "String")],
            cap_check(GateCapability::Cooperative),
        ),
        check_def(
            "the opencode adapter reports gate capability {string}",
            &[(CAP_KEY, "String")],
            cap_check(GateCapability::Cooperative),
        ),
        check_def(
            "the Claude Code adapter reports gate capability {string}",
            &[(CAP_KEY, "String")],
            cap_check(GateCapability::Hard),
        ),
        check_def(
            "the Codex adapter reports gate capability {string}",
            &[(CAP_KEY, "String")],
            cap_check(GateCapability::Hard),
        ),
        check_def(
            "the Kiln adapter reports gate capability {string}",
            &[(CAP_KEY, "String")],
            cap_check(GateCapability::Hard),
        ),
        check_def(
            "the Hermes adapter reports gate capability {string}",
            &[(CAP_KEY, "String")],
            cap_check(GateCapability::Hard),
        ),
    ]
}

fn install_step(pattern: &'static str, harness: Harness) -> StepDef {
    step_def(
        pattern,
        &[],
        &[
            (CONFIG_KEY, "String"),
            (CAP_KEY, "String"),
            (ART_PATHS_KEY, "String"),
            (ART_CONTENT_KEY, "String"),
        ],
        move |mut ctx, params| {
            let command = params.get_string(0).ok_or("Expected command")?.to_string();
            let timeout = params.get_int(1).ok_or("Expected timeout")? as u64;
            install(&mut ctx, harness, &command, timeout)?;
            let mut out = Context::new();
            carry(&mut ctx, &mut out);
            Ok(out)
        },
    )
}

fn uninstall_step(pattern: &'static str, harness: Harness) -> StepDef {
    step_def(
        pattern,
        &[(CONFIG_KEY, "String")],
        &[(CONFIG_KEY, "String"), (CAP_KEY, "String")],
        move |mut ctx, _params| {
            uninstall(&mut ctx, harness)?;
            let mut out = Context::new();
            carry(&mut ctx, &mut out);
            Ok(out)
        },
    )
}

fn cap_check(
    expected: GateCapability,
) -> impl Fn(Context, &brine_core::step_types::Params) -> Result<(), String> + Send + Sync + 'static
{
    move |ctx, params| {
        let want = params.get_string(0).ok_or("Expected capability")?;
        // The capability is the adapter's, recorded at install; assert both the
        // stored value and the Gherkin-supplied expectation agree.
        let stored = ctx.get::<String>(CAP_KEY).ok_or("No capability recorded")?;
        if stored == want && stored == expected.as_str() {
            Ok(())
        } else {
            Err(format!(
                "capability: gherkin wants {}, adapter reports {}, expected {}",
                want,
                stored,
                expected.as_str()
            ))
        }
    }
}
