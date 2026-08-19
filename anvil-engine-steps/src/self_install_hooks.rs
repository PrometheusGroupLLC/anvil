//! Step module for `engine_self_installs_hooks.feature` (engine startup seam).
//!
//! Exercises the engine library seam that production startup calls after bind.
//! The scenarios drive real installer file I/O against temp harness config roots
//! and assert fail-open report behavior without raw Rust behavior tests.

use anvil_test_support::retained_temp_dir;
use anvil_core::domain::hooks::claude_code;
use anvil_core::domain::hooks::installer::HarnessOutcome;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;

const ROOT_KEY: &str = "self_install_root";
const ROOT_HANDLE_KEY: &str = "self_install_root_handle";
const REPORT_KEY: &str = "self_install_report";
const MCP_SNAPSHOT_KEY: &str = "self_install_mcp_snapshot";

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a detected Claude Code harness config under a temp self-install root",
            &[],
            &[(ROOT_KEY, "PathBuf"), (ROOT_HANDLE_KEY, "RetainedTempDir")],
            |_ctx, _params| {
                let (handle, root) = retained_temp_dir("anvil-self-install")?;
                let cc = root.join("claude-code");
                std::fs::create_dir_all(&cc).map_err(|e| e.to_string())?;
                std::fs::write(cc.join("settings.json"), "{}\n").map_err(|e| e.to_string())?;
                let mut out = Context::new();
                out.set(ROOT_KEY, root);
                out.set(ROOT_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "a detected Claude Code harness config under a temp self-install root that cannot be written",
            &[],
            &[(ROOT_KEY, "PathBuf"), (ROOT_HANDLE_KEY, "RetainedTempDir")],
            |_ctx, _params| {
                let (handle, root) = retained_temp_dir("anvil-self-install")?;
                let cc = root.join("claude-code");
                std::fs::create_dir_all(cc.join("settings.json")).map_err(|e| e.to_string())?;
                let mut out = Context::new();
                out.set(ROOT_KEY, root);
                out.set(ROOT_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "a detected Claude Code harness config with a Foundry-managed MCP entry under a temp self-install root",
            &[],
            &[
                (ROOT_KEY, "PathBuf"),
                (ROOT_HANDLE_KEY, "RetainedTempDir"),
                (MCP_SNAPSHOT_KEY, "String"),
            ],
            |_ctx, _params| {
                let (handle, root) = retained_temp_dir("anvil-self-install")?;
                let cc = root.join("claude-code");
                std::fs::create_dir_all(&cc).map_err(|e| e.to_string())?;
                std::fs::write(cc.join("settings.json"), "{}\n").map_err(|e| e.to_string())?;
                let json = serde_json::json!({
                    "mcpServers": {
                        "anvil": {
                            "_foundry_kit": true,
                            "command": "/kit/mcp/anvil-mcp",
                            "type": "stdio"
                        }
                    }
                });
                let content = serde_json::to_string_pretty(&json).unwrap_or_else(|_| "{}".into());
                std::fs::write(cc.join(".claude.json"), &content).map_err(|e| e.to_string())?;
                let mut out = Context::new();
                out.set(ROOT_KEY, root);
                out.set(ROOT_HANDLE_KEY, handle);
                out.set(MCP_SNAPSHOT_KEY, content);
                Ok(out)
            },
        ),
        step_def(
            "engine startup self-install runs",
            &[(ROOT_KEY, "PathBuf")],
            &[
                (ROOT_KEY, "PathBuf"),
                (ROOT_HANDLE_KEY, "RetainedTempDir"),
                (MCP_SNAPSHOT_KEY, "String"),
                (REPORT_KEY, "SelfInstallReport"),
            ],
            |ctx, _params| {
                let root = ctx.get::<PathBuf>(ROOT_KEY).ok_or("No self-install root")?.clone();
                let report = anvil_engine::startup_hooks::self_install_hooks(Some(&root), false);
                let mut out = Context::new();
                out.set(ROOT_KEY, root);
                anvil_test_support::carry_retained_temp_dir(&ctx, &mut out, ROOT_HANDLE_KEY);
                carry_mcp_snapshot(&ctx, &mut out);
                out.set(REPORT_KEY, report);
                Ok(out)
            },
        ),
        step_def(
            "engine startup self-install runs again",
            &[(ROOT_KEY, "PathBuf")],
            &[
                (ROOT_KEY, "PathBuf"),
                (ROOT_HANDLE_KEY, "RetainedTempDir"),
                (MCP_SNAPSHOT_KEY, "String"),
                (REPORT_KEY, "SelfInstallReport"),
            ],
            |ctx, _params| {
                let root = ctx.get::<PathBuf>(ROOT_KEY).ok_or("No self-install root")?.clone();
                let report = anvil_engine::startup_hooks::self_install_hooks(Some(&root), false);
                let mut out = Context::new();
                out.set(ROOT_KEY, root);
                anvil_test_support::carry_retained_temp_dir(&ctx, &mut out, ROOT_HANDLE_KEY);
                carry_mcp_snapshot(&ctx, &mut out);
                out.set(REPORT_KEY, report);
                Ok(out)
            },
        ),
        step_def(
            "engine startup self-install runs with ANVIL_SKIP_HOOK_INSTALL set",
            &[(ROOT_KEY, "PathBuf")],
            &[
                (ROOT_KEY, "PathBuf"),
                (ROOT_HANDLE_KEY, "RetainedTempDir"),
                (REPORT_KEY, "SelfInstallReport"),
            ],
            |ctx, _params| {
                let root = ctx.get::<PathBuf>(ROOT_KEY).ok_or("No self-install root")?.clone();
                let report = anvil_engine::startup_hooks::self_install_hooks(Some(&root), true);
                let mut out = Context::new();
                out.set(ROOT_KEY, root);
                anvil_test_support::carry_retained_temp_dir(&ctx, &mut out, ROOT_HANDLE_KEY);
                out.set(REPORT_KEY, report);
                Ok(out)
            },
        ),
        check_def(
            "the self-install report has harness {string} installed",
            &[(REPORT_KEY, "SelfInstallReport")],
            |ctx, params| assert_harness_status(&ctx, params.get_string(0), "installed"),
        ),
        check_def(
            "the self-install report has harness {string} skipped",
            &[(REPORT_KEY, "SelfInstallReport")],
            |ctx, params| assert_harness_status(&ctx, params.get_string(0), "skipped"),
        ),
        check_def(
            "the self-install report has harness {string} errored",
            &[(REPORT_KEY, "SelfInstallReport")],
            |ctx, params| assert_harness_status(&ctx, params.get_string(0), "errored"),
        ),
        check_def(
            "engine startup self-install completed fail-open",
            &[(REPORT_KEY, "SelfInstallReport")],
            |ctx, _params| {
                let report = ctx
                    .get::<anvil_engine::startup_hooks::SelfInstallReport>(REPORT_KEY)
                    .ok_or("No self-install report")?;
                if report.fatal_error.is_none() {
                    Ok(())
                } else {
                    Err(format!(
                        "expected installer error to be contained per harness, got fatal error {:?}",
                        report.fatal_error
                    ))
                }
            },
        ),
        check_def(
            "the self-install report says startup install was skipped by env",
            &[(REPORT_KEY, "SelfInstallReport")],
            |ctx, _params| {
                let report = ctx
                    .get::<anvil_engine::startup_hooks::SelfInstallReport>(REPORT_KEY)
                    .ok_or("No self-install report")?;
                if report.skipped_by_env && report.reports.is_empty() && report.fatal_error.is_none()
                {
                    Ok(())
                } else {
                    Err(format!("unexpected skip report: {:?}", report))
                }
            },
        ),
        check_def(
            "the Claude Code self-install settings file contains {string}",
            &[(ROOT_KEY, "PathBuf")],
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
            "the Claude Code self-install settings file does not contain {string}",
            &[(ROOT_KEY, "PathBuf")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let content = read_claude_settings(&ctx)?;
                if content.contains(needle) {
                    Err(format!("settings.json unexpectedly contains '{}'", needle))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the Claude Code self-install settings file has exactly {int} anvil-managed PreToolUse hook",
            &[(ROOT_KEY, "PathBuf")],
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
            "the Claude Code self-install settings file has exactly {int} anvil-managed UserPromptSubmit hook",
            &[(ROOT_KEY, "PathBuf")],
            |ctx, params| {
                let want = params.get_int(0).ok_or("Expected count")? as usize;
                let content = read_claude_settings(&ctx)?;
                let got = claude_code::managed_turn_count(&content);
                if got == want {
                    Ok(())
                } else {
                    Err(format!("turn count: expected {}, got {}", want, got))
                }
            },
        ),
        check_def(
            "the Claude Code self-install MCP config file is byte-for-byte unchanged",
            &[(ROOT_KEY, "PathBuf"), (MCP_SNAPSHOT_KEY, "String")],
            |ctx, _params| {
                let want = ctx.get::<String>(MCP_SNAPSHOT_KEY).ok_or("No MCP snapshot")?;
                let got = read_claude_mcp(&ctx)?;
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
    ]
}

fn carry_mcp_snapshot(ctx: &Context, out: &mut Context) {
    if let Some(snap) = ctx.get::<String>(MCP_SNAPSHOT_KEY) {
        out.set(MCP_SNAPSHOT_KEY, snap.clone());
    }
}

fn assert_harness_status(
    ctx: &Context,
    harness: Option<&str>,
    expected_status: &str,
) -> Result<(), String> {
    let harness = harness.ok_or("Expected harness")?;
    let report = ctx
        .get::<anvil_engine::startup_hooks::SelfInstallReport>(REPORT_KEY)
        .ok_or("No self-install report")?;
    let got = report
        .reports
        .iter()
        .find(|line| line.harness.id() == harness)
        .map(|line| match line.outcome {
            HarnessOutcome::Written => "installed",
            HarnessOutcome::Skipped => "skipped",
            HarnessOutcome::Failed(_) => "errored",
        })
        .ok_or_else(|| format!("no report line for harness {}", harness))?;
    if got == expected_status {
        Ok(())
    } else {
        Err(format!(
            "harness {}: expected {}, got {} in {:?}",
            harness, expected_status, got, report
        ))
    }
}

fn read_claude_settings(ctx: &Context) -> Result<String, String> {
    let root = ctx.get::<PathBuf>(ROOT_KEY).ok_or("No self-install root")?;
    let path = root.join("claude-code").join("settings.json");
    std::fs::read_to_string(&path).map_err(|e| format!("read {}: {}", path.display(), e))
}

fn read_claude_mcp(ctx: &Context) -> Result<String, String> {
    let root = ctx.get::<PathBuf>(ROOT_KEY).ok_or("No self-install root")?;
    let path = root.join("claude-code").join(".claude.json");
    std::fs::read_to_string(&path).map_err(|e| format!("read {}: {}", path.display(), e))
}
