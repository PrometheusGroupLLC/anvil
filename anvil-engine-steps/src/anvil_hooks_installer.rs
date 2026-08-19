//! Step module for `anvil_hooks_installer.feature` (engine seam).
//!
//! Exercises the real `anvil-hooks` BINARY end to end: install/uninstall against
//! a temp config dir (filesystem only — no engine), and gate-check against a
//! temp hearth fixture (disk-based open-begin resolution — no engine). The pure
//! adapter transforms + decision are proven in the anvil-core seam; here we prove
//! the CLI surface, file I/O, auto-detect, and exit codes.

use anvil_test_support::retained_temp_dir;
use anvil_core::domain::hooks::{claude_code, mcp_registration};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

const CONFIG_DIR_KEY: &str = "ah_config_dir";
const CONFIG_DIR_HANDLE: &str = "ah_config_dir_handle";
const HOME_SNAPSHOT_KEY: &str = "ah_home_snapshot";
const HEARTH_DIR_KEY: &str = "ah_hearth_dir";
const HEARTH_DIR_HANDLE: &str = "ah_hearth_dir_handle";
const ARTIFACT_REL_KEY: &str = "ah_artifact_rel";
const EXIT_KEY: &str = "ah_exit";
const STDOUT_KEY: &str = "ah_stdout";
/// The original bytes of the Claude Code user MCP config, captured at seed time so
/// a default (hooks-only) install can be asserted byte-for-byte unchanged.
const MCP_SNAPSHOT_KEY: &str = "ah_mcp_snapshot";

fn bin() -> PathBuf {
    anvil_test_support::harness::ensure_binary("anvil-hooks");
    anvil_test_support::harness::binary_path("anvil-hooks")
}

fn run(args: &[&str]) -> (i32, String) {
    let output = Command::new(bin())
        .args(args)
        .output()
        .expect("failed to run anvil-hooks");
    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    (code, stdout)
}

fn run_with_home(args: &[&str], home: &std::path::Path) -> (i32, String) {
    let output = Command::new(bin())
        .args(args)
        .env("HOME", home)
        .output()
        .expect("failed to run anvil-hooks");
    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    (code, stdout)
}

fn snapshot_tree(root: &std::path::Path) -> Result<BTreeMap<PathBuf, Option<Vec<u8>>>, String> {
    fn visit(
        root: &std::path::Path,
        current: &std::path::Path,
        snapshot: &mut BTreeMap<PathBuf, Option<Vec<u8>>>,
    ) -> Result<(), String> {
        let mut entries = std::fs::read_dir(current)
            .map_err(|error| format!("read {}: {error}", current.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("read {} entry: {error}", current.display()))?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .map_err(|error| format!("strip {}: {error}", path.display()))?
                .to_path_buf();
            let file_type = entry
                .file_type()
                .map_err(|error| format!("type {}: {error}", path.display()))?;
            if file_type.is_dir() {
                snapshot.insert(relative, None);
                visit(root, &path, snapshot)?;
            } else {
                let bytes = std::fs::read(&path)
                    .map_err(|error| format!("read {}: {error}", path.display()))?;
                snapshot.insert(relative, Some(bytes));
            }
        }
        Ok(())
    }

    let mut snapshot = BTreeMap::new();
    visit(root, root, &mut snapshot)?;
    Ok(snapshot)
}

/// Build a minimal valid hearth with a single track artifact in `state`. When
/// `open_begin` is true, the artifact's status.yaml carries an unclosed begin
/// activity entry; otherwise it has none (no open begin).
fn seed_hearth_with_track(root: &std::path::Path, state: &str, open_begin: bool) -> String {
    // Minimal hearth markers the FileSystemQueryAdapter / hearth validators expect.
    std::fs::create_dir_all(root.join("tracks")).unwrap();
    std::fs::write(root.join("tracks.md"), "# Tracks\n").unwrap();

    let artifact_rel = "tracks/20260617T0000_gate_fixture";
    let artifact_dir = root.join(artifact_rel);
    std::fs::create_dir_all(&artifact_dir).unwrap();
    std::fs::write(artifact_dir.join("spec.md"), "# Spec\n").unwrap();

    let activity_block = if open_begin {
        format!(
            "activity:\n  - kind: begin\n    actor: Tester-000001\n    state: {state}\n    at: 2026-06-17T00:00:00Z\n"
        )
    } else {
        String::new()
    };
    let status_yaml = format!(
        "version: 1\nkind: track\nstate: {state}\ntransitions:\n  - to: {state}\n    at: 2026-06-17T00:00:00Z\n    actor: Tester-000001\n    role: doer\n{activity}",
        state = state,
        activity = activity_block,
    );
    std::fs::write(artifact_dir.join("status.yaml"), status_yaml).unwrap();
    artifact_rel.to_string()
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ---- Given: config dir fixtures ----
        claude_config_step(
            "an anvil-hooks config dir with a Claude Code settings file",
            None,
            None,
        ),
        step_def(
            "an anvil-hooks config dir with a Claude Code settings file carrying key {string} value {string}",
            &[],
            &[(CONFIG_DIR_KEY, "PathBuf"), (CONFIG_DIR_HANDLE, "RetainedTempDir")],
            |_ctx, params| {
                let key = params.get_string(0).ok_or("Expected key")?.to_string();
                let val = params.get_string(1).ok_or("Expected value")?.to_string();
                let (handle, dir) = retained_temp_dir("anvil-hooks-cfg")
                    .map_err(|e| e)?;
                let cc = dir.join("claude-code");
                std::fs::create_dir_all(&cc).map_err(|e| e.to_string())?;
                let json = serde_json::json!({ key: val });
                std::fs::write(cc.join("settings.json"), serde_json::to_string_pretty(&json).unwrap())
                    .map_err(|e| e.to_string())?;
                let mut out = Context::new();
                out.set(CONFIG_DIR_KEY, dir);
                out.set(CONFIG_DIR_HANDLE, handle);
                Ok(out)
            },
        ),
        step_def(
            "an anvil-hooks config dir with a Claude Code user MCP config carrying server {string}",
            &[],
            &[(CONFIG_DIR_KEY, "PathBuf"), (CONFIG_DIR_HANDLE, "RetainedTempDir")],
            |_ctx, params| {
                let server = params.get_string(0).ok_or("Expected server name")?.to_string();
                let (handle, dir) = retained_temp_dir("anvil-hooks-cfg")?;
                let cc = dir.join("claude-code");
                std::fs::create_dir_all(&cc).map_err(|e| e.to_string())?;
                std::fs::write(cc.join("settings.json"), "{}\n").map_err(|e| e.to_string())?;
                let json = serde_json::json!({
                    "mcpServers": { server: { "command": "/usr/bin/other-mcp", "type": "stdio" } }
                });
                std::fs::write(
                    cc.join(".claude.json"),
                    serde_json::to_string_pretty(&json).unwrap(),
                )
                .map_err(|e| e.to_string())?;
                let mut out = Context::new();
                out.set(CONFIG_DIR_KEY, dir);
                out.set(CONFIG_DIR_HANDLE, handle);
                Ok(out)
            },
        ),
        step_def(
            "an anvil-hooks config dir with a Claude Code user MCP config carrying a Foundry-managed anvil server",
            &[],
            &[
                (CONFIG_DIR_KEY, "PathBuf"),
                (CONFIG_DIR_HANDLE, "RetainedTempDir"),
                (MCP_SNAPSHOT_KEY, "String"),
            ],
            |_ctx, _params| {
                let (handle, dir) = retained_temp_dir("anvil-hooks-cfg")?;
                let cc = dir.join("claude-code");
                std::fs::create_dir_all(&cc).map_err(|e| e.to_string())?;
                std::fs::write(cc.join("settings.json"), "{}\n").map_err(|e| e.to_string())?;
                // Foundry's wire.rs owns the version-stable anvil entry, stamped
                // with its OWN `_foundry_kit` marker (not anvil's `_anvil_managed`).
                let json = serde_json::json!({
                    "mcpServers": {
                        "anvil": {
                            "_foundry_kit": true,
                            "command": "/kit/mcp/anvil-mcp",
                            "type": "stdio"
                        }
                    }
                });
                let content = serde_json::to_string_pretty(&json).unwrap();
                std::fs::write(cc.join(".claude.json"), &content).map_err(|e| e.to_string())?;
                let mut out = Context::new();
                out.set(CONFIG_DIR_KEY, dir);
                out.set(CONFIG_DIR_HANDLE, handle);
                out.set(MCP_SNAPSHOT_KEY, content);
                Ok(out)
            },
        ),
        step_def(
            "an anvil-hooks config dir with a Codex config file",
            &[],
            &[(CONFIG_DIR_KEY, "PathBuf"), (CONFIG_DIR_HANDLE, "RetainedTempDir")],
            |_ctx, _p| {
                let (handle, dir) = retained_temp_dir("anvil-hooks-cfg")?;
                let cx = dir.join("codex");
                std::fs::create_dir_all(&cx).map_err(|e| e.to_string())?;
                std::fs::write(cx.join("config.toml"), "").map_err(|e| e.to_string())?;
                let mut out = Context::new();
                out.set(CONFIG_DIR_KEY, dir);
                out.set(CONFIG_DIR_HANDLE, handle);
                Ok(out)
            },
        ),
        step_def(
            "an isolated home with detected configurations and hook artifacts for every supported harness",
            &[],
            &[
                (CONFIG_DIR_KEY, "PathBuf"),
                (CONFIG_DIR_HANDLE, "RetainedTempDir"),
                (HOME_SNAPSHOT_KEY, "BTreeMap<PathBuf, Option<Vec<u8>>>"),
            ],
            |_ctx, _params| {
                let (handle, home) = retained_temp_dir("anvil-hooks-home")?;
                let files = [
                    (".claude/settings.json", b"{\"theme\":\"dark\"}\n".as_slice()),
                    (".codex/hooks.json", b"{\"operator\":\"codex\"}\n".as_slice()),
                    (".kiln/hooks.json", b"{\"operator\":\"kiln\"}\n".as_slice()),
                    (".hermes/config.yaml", b"model: operator\n".as_slice()),
                    (".grok/config.toml", b"model = \"operator\"\n".as_slice()),
                    (
                        ".config/opencode/opencode.jsonc",
                        b"{\"theme\":\"operator\"}\n".as_slice(),
                    ),
                    (
                        ".grok/plugins/anvil-route-turn/hooks/hooks.json",
                        b"preexisting-grok-hook\n".as_slice(),
                    ),
                    (
                        ".config/opencode/plugin/anvil-route-turn.js",
                        b"preexisting-opencode-hook\n".as_slice(),
                    ),
                ];
                for (relative, content) in files {
                    let path = home.join(relative);
                    let parent = path
                        .parent()
                        .ok_or_else(|| format!("no parent for {}", path.display()))?;
                    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
                    std::fs::write(&path, content).map_err(|error| error.to_string())?;
                }
                let snapshot = snapshot_tree(&home)?;
                Ok(Context::new()
                    .with(CONFIG_DIR_KEY, home)
                    .with(CONFIG_DIR_HANDLE, handle)
                    .with(HOME_SNAPSHOT_KEY, snapshot))
            },
        ),
        // ---- Given: hearth fixture for gate-check ----
        step_def(
            "an anvil-hooks hearth with a hard-enforced track artifact in state {string} with no open begin",
            &[],
            &[
                (HEARTH_DIR_KEY, "PathBuf"),
                (HEARTH_DIR_HANDLE, "RetainedTempDir"),
                (ARTIFACT_REL_KEY, "String"),
            ],
            |_ctx, params| {
                let state = params.get_string(0).ok_or("Expected state")?.to_string();
                let (handle, dir) = retained_temp_dir("anvil-hooks-hearth").map_err(|e| e)?;
                let rel = seed_hearth_with_track(&dir, &state, false);
                let mut out = Context::new();
                out.set(HEARTH_DIR_KEY, dir);
                out.set(HEARTH_DIR_HANDLE, handle);
                out.set(ARTIFACT_REL_KEY, rel);
                Ok(out)
            },
        ),
        // ---- When: install / uninstall ----
        // Default (hooks-only) install/uninstall: NO --with-mcp flag.
        action_step(
            "anvil-hooks install runs for harness {string} against that config dir",
            Action::Install,
            false,
        ),
        action_step(
            "anvil-hooks uninstall runs for harness {string} against that config dir",
            Action::Uninstall,
            false,
        ),
        // Standalone uninstall: --with-mcp also removes the anvil MCP entry.
        action_step(
            "anvil-hooks uninstall runs for harness {string} with-mcp against that config dir",
            Action::Uninstall,
            true,
        ),
        // Default install carrying an --mcp-command but NO --with-mcp: MCP stays untouched.
        mcp_install_step(
            "anvil-hooks install runs for harness {string} with mcp-command {string} against that config dir",
            false,
            true,
        ),
        // Standalone install: --with-mcp + an explicit --mcp-command.
        mcp_install_step(
            "anvil-hooks install runs for harness {string} with-mcp and mcp-command {string} against that config dir",
            true,
            true,
        ),
        // Standalone install: --with-mcp, command derived from the sibling binary.
        mcp_install_step(
            "anvil-hooks install runs for harness {string} with-mcp and no mcp-command against that config dir",
            true,
            false,
        ),
        step_def(
            "anvil-hooks {string} with {string} runs against the isolated home",
            &[(CONFIG_DIR_KEY, "PathBuf")],
            &[
                (CONFIG_DIR_KEY, "PathBuf"),
                (CONFIG_DIR_HANDLE, "RetainedTempDir"),
                (HOME_SNAPSHOT_KEY, "BTreeMap<PathBuf, Option<Vec<u8>>>"),
                (EXIT_KEY, "i64"),
                (STDOUT_KEY, "String"),
            ],
            |ctx, params| {
                let subcommand = params
                    .get_string(0)
                    .ok_or("Expected subcommand")?
                    .to_string();
                let help_flag = params
                    .get_string(1)
                    .ok_or("Expected help flag")?
                    .to_string();
                let home = ctx
                    .get::<PathBuf>(CONFIG_DIR_KEY)
                    .ok_or("No isolated home")?
                    .clone();
                let snapshot = ctx
                    .get::<BTreeMap<PathBuf, Option<Vec<u8>>>>(HOME_SNAPSHOT_KEY)
                    .ok_or("No isolated-home snapshot")?
                    .clone();
                let (code, stdout) = run_with_home(&[&subcommand, &help_flag], &home);
                let mut out = Context::new()
                    .with(CONFIG_DIR_KEY, home)
                    .with(HOME_SNAPSHOT_KEY, snapshot)
                    .with(EXIT_KEY, code as i64)
                    .with(STDOUT_KEY, stdout);
                anvil_test_support::carry_retained_temp_dir(&ctx, &mut out, CONFIG_DIR_HANDLE);
                Ok(out)
            },
        ),
        // ---- When: gate-check ----
        step_def(
            "anvil-hooks gate-check runs for an edit inside that artifact",
            &[(HEARTH_DIR_KEY, "PathBuf"), (ARTIFACT_REL_KEY, "String")],
            &[(EXIT_KEY, "i64"), (STDOUT_KEY, "String")],
            |mut ctx, _params| {
                let hearth = ctx.take::<PathBuf>(HEARTH_DIR_KEY).ok_or("No hearth")?;
                let rel = ctx.take::<String>(ARTIFACT_REL_KEY).ok_or("No artifact")?;
                let edited = hearth.join(&rel).join("spec.md");
                let (code, stdout) = run(&[
                    "gate-check",
                    "--path",
                    edited.to_str().unwrap(),
                    "--hearth",
                    hearth.to_str().unwrap(),
                    "--hard-enforce",
                    "track",
                ]);
                let mut out = Context::new();
                out.set(HEARTH_DIR_KEY, hearth);
                out.set(ARTIFACT_REL_KEY, rel);
                out.set(EXIT_KEY, code as i64);
                out.set(STDOUT_KEY, stdout);
                Ok(out)
            },
        ),
        step_def(
            "anvil-hooks gate-check runs for an edit outside any artifact",
            &[(HEARTH_DIR_KEY, "PathBuf")],
            &[(EXIT_KEY, "i64"), (STDOUT_KEY, "String")],
            |mut ctx, _params| {
                let hearth = ctx.take::<PathBuf>(HEARTH_DIR_KEY).ok_or("No hearth")?;
                // An edit directly in the hearth root (no enclosing status.yaml).
                let edited = hearth.join("tracks.md");
                let (code, stdout) = run(&[
                    "gate-check",
                    "--path",
                    edited.to_str().unwrap(),
                    "--hearth",
                    hearth.to_str().unwrap(),
                    "--hard-enforce",
                    "track",
                ]);
                let mut out = Context::new();
                out.set(HEARTH_DIR_KEY, hearth);
                out.set(EXIT_KEY, code as i64);
                out.set(STDOUT_KEY, stdout);
                Ok(out)
            },
        ),
        // ---- Then: assertions ----
        check_def(
            "the anvil-hooks command exits 0",
            &[(EXIT_KEY, "i64")],
            |ctx, _p| {
                let code = ctx.get::<i64>(EXIT_KEY).ok_or("No exit code")?;
                if *code == 0 {
                    Ok(())
                } else {
                    Err(format!("expected exit 0, got {}", code))
                }
            },
        ),
        check_def(
            "the anvil-hooks output shows usage",
            &[(STDOUT_KEY, "String")],
            |ctx, _params| {
                let stdout = ctx.get::<String>(STDOUT_KEY).ok_or("No output")?;
                if stdout.contains("USAGE:") {
                    Ok(())
                } else {
                    Err(format!("expected usage output, got:\n{stdout}"))
                }
            },
        ),
        check_def(
            "the isolated harness configuration tree is byte-for-byte unchanged",
            &[
                (CONFIG_DIR_KEY, "PathBuf"),
                (HOME_SNAPSHOT_KEY, "BTreeMap<PathBuf, Option<Vec<u8>>>"),
            ],
            |ctx, _params| {
                let home = ctx.get::<PathBuf>(CONFIG_DIR_KEY).ok_or("No isolated home")?;
                let before = ctx
                    .get::<BTreeMap<PathBuf, Option<Vec<u8>>>>(HOME_SNAPSHOT_KEY)
                    .ok_or("No isolated-home snapshot")?;
                let after = snapshot_tree(home)?;
                if &after == before {
                    Ok(())
                } else {
                    let before_paths = before.keys().collect::<Vec<_>>();
                    let after_paths = after.keys().collect::<Vec<_>>();
                    Err(format!(
                        "isolated home changed\nbefore paths: {before_paths:?}\nafter paths: {after_paths:?}"
                    ))
                }
            },
        ),
        check_def(
            "the Claude Code settings file contains {string}",
            &[(CONFIG_DIR_KEY, "PathBuf")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let content = read_claude_settings(&ctx)?;
                if content.contains(needle) {
                    Ok(())
                } else {
                    Err(format!("settings.json does not contain '{}'", needle))
                }
            },
        ),
        check_def(
            "the Claude Code settings file has exactly {int} anvil-managed PreToolUse hook",
            &[(CONFIG_DIR_KEY, "PathBuf")],
            |ctx, params| {
                let want = params.get_int(0).ok_or("Expected count")? as usize;
                let content = read_claude_settings(&ctx)?;
                let got = claude_code::managed_pretool_count(&content);
                if got == want {
                    Ok(())
                } else {
                    Err(format!("managed count: expected {}, got {}", want, got))
                }
            },
        ),
        check_def(
            "the Claude Code settings file still contains key {string} value {string}",
            &[(CONFIG_DIR_KEY, "PathBuf")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected key")?;
                let val = params.get_string(1).ok_or("Expected value")?;
                let content = read_claude_settings(&ctx)?;
                let parsed: serde_json::Value =
                    serde_json::from_str(&content).map_err(|e| format!("parse: {}", e))?;
                if parsed.get(key).and_then(|v| v.as_str()) == Some(val) {
                    Ok(())
                } else {
                    Err(format!("key {} != {}", key, val))
                }
            },
        ),
        check_def(
            "the anvil-hooks output reports harness {string} written",
            &[(STDOUT_KEY, "String")],
            |ctx, params| {
                let h = params.get_string(0).ok_or("Expected harness")?;
                let out = ctx.get::<String>(STDOUT_KEY).ok_or("No output")?;
                let line_ok = out
                    .lines()
                    .any(|l| l.starts_with(&format!("{}: written", h)));
                if line_ok {
                    Ok(())
                } else {
                    Err(format!("no 'written' report line for {} in:\n{}", h, out))
                }
            },
        ),
        check_def(
            "the anvil-hooks output reports harness {string} skipped",
            &[(STDOUT_KEY, "String")],
            |ctx, params| {
                let h = params.get_string(0).ok_or("Expected harness")?;
                let out = ctx.get::<String>(STDOUT_KEY).ok_or("No output")?;
                let line_ok = out
                    .lines()
                    .any(|l| l.starts_with(&format!("{}: skipped", h)));
                if line_ok {
                    Ok(())
                } else {
                    Err(format!("no 'skipped' report line for {} in:\n{}", h, out))
                }
            },
        ),
        check_def(
            "the anvil-hooks gate-check decision is {string}",
            &[(EXIT_KEY, "i64")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected decision")?;
                let code = *ctx.get::<i64>(EXIT_KEY).ok_or("No exit code")?;
                // allow → exit 0; block → exit 2.
                let got = match code {
                    0 => "allow",
                    2 => "block",
                    other => return Err(format!("unexpected exit code {}", other)),
                };
                if got == want {
                    Ok(())
                } else {
                    Err(format!("decision: expected {}, got {} (exit {})", want, got, code))
                }
            },
        ),
        // ---- Then: MCP registration assertions ----
        check_def(
            "the Claude Code user MCP config file has an anvil server with command {string}",
            &[(CONFIG_DIR_KEY, "PathBuf")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected command")?;
                let content = read_claude_user_mcp(&ctx)?;
                let got = mcp_registration::claude_anvil_command(&content)
                    .ok_or("No anvil MCP server")?;
                if got == want {
                    Ok(())
                } else {
                    Err(format!("command: expected {}, got {}", want, got))
                }
            },
        ),
        check_def(
            "the Claude Code user MCP config file has exactly {int} anvil server",
            &[(CONFIG_DIR_KEY, "PathBuf")],
            |ctx, params| {
                let want = params.get_int(0).ok_or("Expected count")? as usize;
                let content = read_claude_user_mcp(&ctx)?;
                let got = mcp_registration::claude_anvil_count(&content);
                if got == want {
                    Ok(())
                } else {
                    Err(format!("anvil count: expected {}, got {}", want, got))
                }
            },
        ),
        check_def(
            "the Claude Code user MCP config file anvil server command ends with {string}",
            &[(CONFIG_DIR_KEY, "PathBuf")],
            |ctx, params| {
                let suffix = params.get_string(0).ok_or("Expected suffix")?;
                let content = read_claude_user_mcp(&ctx)?;
                let got = mcp_registration::claude_anvil_command(&content)
                    .ok_or("No anvil MCP server")?;
                if got.ends_with(suffix) {
                    Ok(())
                } else {
                    Err(format!("command {} does not end with {}", got, suffix))
                }
            },
        ),
        check_def(
            "the Claude Code user MCP config file still has server {string}",
            &[(CONFIG_DIR_KEY, "PathBuf")],
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected server")?;
                let content = read_claude_user_mcp(&ctx)?;
                if mcp_registration::claude_has_server(&content, name) {
                    Ok(())
                } else {
                    Err(format!("server {} was lost", name))
                }
            },
        ),
        check_def(
            "the Claude Code user MCP config file is byte-for-byte unchanged",
            &[(CONFIG_DIR_KEY, "PathBuf"), (MCP_SNAPSHOT_KEY, "String")],
            |ctx, _params| {
                let want = ctx.get::<String>(MCP_SNAPSHOT_KEY).ok_or("No MCP snapshot")?;
                let got = read_claude_user_mcp(&ctx)?;
                if &got == want {
                    Ok(())
                } else {
                    Err(format!(
                        "MCP config changed.\n--- before ---\n{}\n--- after ---\n{}",
                        want, got
                    ))
                }
            },
        ),
        check_def(
            "the Codex config file has an mcp_servers anvil-mcp entry with command {string}",
            &[(CONFIG_DIR_KEY, "PathBuf")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected command")?;
                let content = read_codex_config(&ctx)?;
                let got = mcp_registration::codex_anvil_command(&content)
                    .ok_or("No anvil-mcp entry")?;
                if got == want {
                    Ok(())
                } else {
                    Err(format!("command: expected {}, got {}", want, got))
                }
            },
        ),
    ]
}

/// Whether a CLI action installs or uninstalls.
#[derive(Clone, Copy)]
enum Action {
    Install,
    Uninstall,
}

impl Action {
    fn subcommand(self) -> &'static str {
        match self {
            Action::Install => "install",
            Action::Uninstall => "uninstall",
        }
    }
}

/// An install step. `with_mcp` adds the `--with-mcp` flag (opt-in MCP
/// registration); `with_cmd` adds an explicit `--mcp-command` (else the binary
/// derives the sibling path). Runs against a named harness's seeded subdir.
fn mcp_install_step(pattern: &'static str, with_mcp: bool, with_cmd: bool) -> StepDef {
    step_def(
        pattern,
        &[(CONFIG_DIR_KEY, "PathBuf")],
        &[
            (CONFIG_DIR_KEY, "PathBuf"),
            (CONFIG_DIR_HANDLE, "RetainedTempDir"),
            (MCP_SNAPSHOT_KEY, "String"),
            (EXIT_KEY, "i64"),
            (STDOUT_KEY, "String"),
        ],
        move |mut ctx, params| {
            let harness = params.get_string(0).ok_or("Expected harness")?.to_string();
            let mcp_command = if with_cmd {
                params
                    .get_string(1)
                    .ok_or("Expected mcp-command")?
                    .to_string()
            } else {
                String::new()
            };
            let dir = ctx
                .get::<PathBuf>(CONFIG_DIR_KEY)
                .ok_or("No config dir")?
                .clone();
            let sub_dir = dir.join(&harness);
            let sub_dir_str = sub_dir.to_str().unwrap().to_string();
            let mut args: Vec<&str> = vec![
                "install",
                "--harness",
                &harness,
                "--config-dir",
                &sub_dir_str,
            ];
            if with_mcp {
                args.push("--with-mcp");
            }
            if with_cmd {
                args.push("--mcp-command");
                args.push(&mcp_command);
            }
            let (code, stdout) = run(&args);
            let mut out = Context::new();
            out.set(CONFIG_DIR_KEY, dir);
            anvil_test_support::carry_retained_temp_dir(&ctx, &mut out, CONFIG_DIR_HANDLE);
            carry_snapshot(&ctx, &mut out);
            out.set(EXIT_KEY, code as i64);
            out.set(STDOUT_KEY, stdout);
            Ok(out)
        },
    )
}

/// Carry the MCP snapshot bytes forward through a step, when present.
fn carry_snapshot(ctx: &Context, out: &mut Context) {
    if let Some(snap) = ctx.get::<String>(MCP_SNAPSHOT_KEY) {
        out.set(MCP_SNAPSHOT_KEY, snap.clone());
    }
}

/// Read the Claude Code USER MCP config (`.claude.json`) under the seeded
/// `claude-code` subdir. A MISSING file is empty content — the default
/// (hooks-only) path never creates this file, and empty parses to 0 anvil
/// servers, which is the correct assertion target.
fn read_claude_user_mcp(ctx: &Context) -> Result<String, String> {
    let dir = ctx.get::<PathBuf>(CONFIG_DIR_KEY).ok_or("No config dir")?;
    let path = dir.join("claude-code").join(".claude.json");
    Ok(std::fs::read_to_string(&path).unwrap_or_default())
}

/// Read the Codex config (`config.toml`) under the seeded `codex` subdir.
fn read_codex_config(ctx: &Context) -> Result<String, String> {
    let dir = ctx.get::<PathBuf>(CONFIG_DIR_KEY).ok_or("No config dir")?;
    let path = dir.join("codex").join("config.toml");
    std::fs::read_to_string(&path).map_err(|e| format!("read {}: {}", path.display(), e))
}

/// A Given step that seeds a config dir with an empty (or keyed) Claude Code
/// settings file under `<dir>/claude-code/settings.json`.
fn claude_config_step(
    pattern: &'static str,
    _key: Option<&'static str>,
    _val: Option<&'static str>,
) -> StepDef {
    step_def(
        pattern,
        &[],
        &[
            (CONFIG_DIR_KEY, "PathBuf"),
            (CONFIG_DIR_HANDLE, "RetainedTempDir"),
        ],
        move |_ctx, _params| {
            let (handle, dir) = retained_temp_dir("anvil-hooks-cfg").map_err(|e| e)?;
            let cc = dir.join("claude-code");
            std::fs::create_dir_all(&cc).map_err(|e| e.to_string())?;
            std::fs::write(cc.join("settings.json"), "{}\n").map_err(|e| e.to_string())?;
            let mut out = Context::new();
            out.set(CONFIG_DIR_KEY, dir);
            out.set(CONFIG_DIR_HANDLE, handle);
            Ok(out)
        },
    )
}

/// An install/uninstall action step. `action` selects the subcommand; `with_mcp`
/// adds the opt-in `--with-mcp` flag.
fn action_step(pattern: &'static str, action: Action, with_mcp: bool) -> StepDef {
    step_def(
        pattern,
        &[(CONFIG_DIR_KEY, "PathBuf")],
        &[
            (CONFIG_DIR_KEY, "PathBuf"),
            (CONFIG_DIR_HANDLE, "RetainedTempDir"),
            (MCP_SNAPSHOT_KEY, "String"),
            (EXIT_KEY, "i64"),
            (STDOUT_KEY, "String"),
        ],
        move |mut ctx, params| {
            let harness = params.get_string(0).ok_or("Expected harness")?.to_string();
            let dir = ctx
                .get::<PathBuf>(CONFIG_DIR_KEY)
                .ok_or("No config dir")?
                .clone();
            let sub = action.subcommand();
            // For `auto`, point the probe root at the temp dir (each harness
            // resolves to <dir>/<harness-id>). For a named harness, point
            // --config-dir at the harness's seeded subdir.
            let dir_str = dir.to_str().unwrap().to_string();
            let sub_dir = dir.join(&harness);
            let sub_dir_str = sub_dir.to_str().unwrap().to_string();
            let mut args: Vec<&str> = if harness == "auto" {
                vec![sub, "--harness", "auto", "--config-dir", &dir_str]
            } else {
                vec![sub, "--harness", &harness, "--config-dir", &sub_dir_str]
            };
            if with_mcp {
                args.push("--with-mcp");
            }
            let (code, stdout) = run(&args);
            let mut out = Context::new();
            out.set(CONFIG_DIR_KEY, dir);
            anvil_test_support::carry_retained_temp_dir(&ctx, &mut out, CONFIG_DIR_HANDLE);
            carry_snapshot(&ctx, &mut out);
            out.set(EXIT_KEY, code as i64);
            out.set(STDOUT_KEY, stdout);
            Ok(out)
        },
    )
}

/// Read the Claude Code settings.json the steps seeded/wrote under the temp dir.
fn read_claude_settings(ctx: &Context) -> Result<String, String> {
    let dir = ctx.get::<PathBuf>(CONFIG_DIR_KEY).ok_or("No config dir")?;
    let path = dir.join("claude-code").join("settings.json");
    std::fs::read_to_string(&path).map_err(|e| format!("read {}: {}", path.display(), e))
}
