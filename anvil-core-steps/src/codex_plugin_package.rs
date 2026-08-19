use anvil_test_support::{carry_retained_temp_dir, retained_temp_dir, RetainedTempDir};
use anvil_core::domain::hooks::{codex, InstallSpec};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

const ROOT: &str = "cpp_root";
const HANDLE: &str = "cpp_handle";
const GLOBAL: &str = "cpp_global";
const CODEX_HOME: &str = "cpp_codex_home";
const MARKETPLACE: &str = "cpp_marketplace";
const CODEX_LIST: &str = "cpp_codex_list";
const STAGED_EXIT: &str = "cpp_staged_exit";

fn workspace_root() -> PathBuf {
    PathBuf::from(anvil_test_support::TEST_SUPPORT_DIR)
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn carry(ctx: &Context, out: &mut Context) {
    if let Some(root) = ctx.get::<PathBuf>(ROOT) {
        out.set(ROOT, root.clone());
    }
    if let Some(global) = ctx.get::<String>(GLOBAL) {
        out.set(GLOBAL, global.clone());
    }
    if let Some(codex_home) = ctx.get::<PathBuf>(CODEX_HOME) {
        out.set(CODEX_HOME, codex_home.clone());
    }
    if let Some(marketplace) = ctx.get::<PathBuf>(MARKETPLACE) {
        out.set(MARKETPLACE, marketplace.clone());
    }
    if let Some(list) = ctx.get::<String>(CODEX_LIST) {
        out.set(CODEX_LIST, list.clone());
    }
    carry_retained_temp_dir(ctx, out, HANDLE);
}

fn plugin_manifest(root: &std::path::Path) -> PathBuf {
    root.join(".codex-plugin/plugin.json")
}

fn plugin_hooks(root: &std::path::Path) -> PathBuf {
    root.join("codex-hooks/hooks.json")
}

fn native_host_binary_name() -> Result<&'static str, String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("anvil-hooks-darwin-arm64"),
        ("macos", "x86_64") => Ok("anvil-hooks-darwin-x64"),
        other => Err(format!("unsupported Codex plugin test host: {other:?}")),
    }
}

fn stage_plugin_assets(root: &std::path::Path) -> Result<(), String> {
    let workspace = workspace_root();
    std::fs::create_dir_all(root.join(".codex-plugin")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(root.join("codex-hooks")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(root.join("app/hooks")).map_err(|e| e.to_string())?;
    // Stage the plugin manifest the way PRODUCTION stages it: scripts/build-kit.sh
    // DERIVES the plugin version from kit/foundry-manifest.json rather than trusting
    // the literal in kit/.codex-plugin/plugin.json. A raw copy here staged the source
    // literal instead, so the test exercised an artifact production never ships.
    //
    // That is not hypothetical: the source literal is 0.2.247 while the manifest is
    // 0.2.251 — it drifts every time CI bumps the kit and nothing bumps the plugin.
    // Deriving here means the staged artifact matches the shipped one, and the version
    // assertion compares Codex against the single source of truth instead of against
    // another copy of the same stale value.
    {
        let raw = std::fs::read_to_string(workspace.join("kit/.codex-plugin/plugin.json"))
            .map_err(|e| e.to_string())?;
        let mut plugin: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        let manifest_raw =
            std::fs::read_to_string(workspace.join("kit/foundry-manifest.json"))
                .map_err(|e| e.to_string())?;
        let manifest: serde_json::Value =
            serde_json::from_str(&manifest_raw).map_err(|e| e.to_string())?;
        let version = manifest
            .get("kit")
            .and_then(|k| k.get("version"))
            .and_then(|v| v.as_str())
            .ok_or("kit manifest declares no kit.version")?;
        plugin["version"] = serde_json::Value::String(version.to_string());
        std::fs::write(
            plugin_manifest(root),
            serde_json::to_string_pretty(&plugin).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    }
    std::fs::write(
        plugin_hooks(root),
        codex::render_plugin_hooks(&InstallSpec::default())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::copy(
        workspace.join("scripts/kit-wrappers/anvil-hooks.posix"),
        root.join("codex-hooks/anvil-hooks"),
    )
    .map_err(|e| e.to_string())?;
    std::fs::copy(
        workspace.join("scripts/kit-wrappers/anvil-hooks.cmd"),
        root.join("codex-hooks/anvil-hooks.cmd"),
    )
    .map_err(|e| e.to_string())?;
    let status = Command::new("cargo")
        .args(["build", "-q", "-p", "anvil-engine", "--bin", "anvil-hooks"])
        .current_dir(&workspace)
        .status()
        .map_err(|e| format!("build native anvil-hooks: {e}"))?;
    if !status.success() {
        return Err(format!("native anvil-hooks build failed with {status}"));
    }
    std::fs::copy(
        workspace.join("target/debug/anvil-hooks"),
        root.join("app/hooks").join(native_host_binary_name()?),
    )
    .map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [
            root.join("codex-hooks/anvil-hooks"),
            root.join("app/hooks").join(native_host_binary_name()?),
        ] {
            let mut permissions = std::fs::metadata(&path)
                .map_err(|e| e.to_string())?
                .permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(path, permissions).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn stage(ctx: &Context) -> Result<Context, String> {
    let (handle, root) = if let Some(root) = ctx.get::<PathBuf>(ROOT) {
        (
            ctx.get::<RetainedTempDir>(HANDLE)
                .cloned()
                .ok_or("missing retained package")?,
            root.clone(),
        )
    } else {
        retained_temp_dir("anvil-codex-package-")?
    };
    stage_plugin_assets(&root)?;
    let mut out = Context::new();
    carry(ctx, &mut out);
    out.set(ROOT, root);
    out.set(HANDLE, handle);
    Ok(out)
}

fn parsed(path: PathBuf) -> Result<serde_json::Value, String> {
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))
}

fn run_codex(codex_home: &std::path::Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("codex")
        .args(args)
        .env("CODEX_HOME", codex_home)
        .env(
            "HOME",
            codex_home.parent().ok_or("Codex home has no parent")?,
        )
        .output()
        .map_err(|e| format!("launch codex {}: {e}", args.join(" ")))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if output.status.success() {
        Ok(stdout)
    } else {
        Err(format!(
            "codex {} failed with {}:\nstdout: {stdout}\nstderr: {stderr}",
            args.join(" "),
            output.status
        ))
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "the Anvil Codex plugin package is staged",
            &[],
            &[
                (ROOT, "PathBuf"),
                (HANDLE, "Arc<Mutex<Option<TempDir>>>"),
                (GLOBAL, "String"),
            ],
            |ctx, _| stage(&ctx),
        ),
        step_def(
            "the Anvil Codex plugin package is staged with a zero timeout",
            &[],
            &[(ROOT, "PathBuf"), (HANDLE, "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, _| {
                let (handle, root) = retained_temp_dir("anvil-zero-timeout-codex-package-")?;
                stage_plugin_assets(&root)?;
                let mut spec = InstallSpec::default();
                spec.timeout_ms = 0;
                std::fs::write(plugin_hooks(&root), codex::render_plugin_hooks(&spec)?)
                    .map_err(|e| e.to_string())?;
                let mut out = Context::new();
                out.set(ROOT, root);
                out.set(HANDLE, handle);
                Ok(out)
            },
        ),
        step_def(
            "build-kit stages the Anvil Codex plugin distribution",
            &[],
            &[(ROOT, "PathBuf"), (HANDLE, "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, _| {
                let (handle, temp_root) = retained_temp_dir("anvil-build-kit-codex-")?;
                let staged = temp_root.join("anvil-kit");
                let status = Command::new("bash")
                    .arg("scripts/build-kit.sh")
                    .current_dir(workspace_root())
                    .env("KIT_DIR", &staged)
                    .env("ANVIL_BUILD_KIT_USE_COMMITTED_FRONTEND_DIST", "1")
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .map_err(|e| format!("run build-kit: {e}"))?;
                if !status.success() {
                    return Err(format!("build-kit failed with {status}"));
                }
                let mut out = Context::new();
                out.set(ROOT, staged);
                out.set(HANDLE, handle);
                Ok(out)
            },
        ),
        step_def(
            "a Codex home with an unrelated global hook and an exact legacy Foundry route",
            &[],
            &[(GLOBAL, "String")],
            |_ctx, _| {
                let mut out = Context::new();
                out.set(
                    GLOBAL,
                    serde_json::json!({
                        "hooks": {"UserPromptSubmit": [
                            {"hooks":[{"type":"command","command":"operator-own-hook"}]},
                            {"hooks":[{"type":"command","command":"anvil-hooks route-turn --source codex"}]}
                        ]}
                    })
                    .to_string(),
                );
                Ok(out)
            },
        ),
        step_def(
            "an older staged Anvil plugin routes turns with source claude-code",
            &[],
            &[
                (ROOT, "PathBuf"),
                (HANDLE, "Arc<Mutex<Option<TempDir>>>"),
                (GLOBAL, "String"),
            ],
            |ctx, _| {
                let (handle, root) = retained_temp_dir("anvil-old-codex-package-")?;
                std::fs::create_dir_all(root.join("codex-hooks")).map_err(|e| e.to_string())?;
                std::fs::write(
                    plugin_hooks(&root),
                    r#"{"hooks":{"UserPromptSubmit":[{"hooks":[{"type":"command","command":"anvil-hooks route-turn --source claude-code"}]}]}}"#,
                )
                .map_err(|e| e.to_string())?;
                let mut out = Context::new();
                carry(&ctx, &mut out);
                out.set(ROOT, root);
                out.set(HANDLE, handle);
                Ok(out)
            },
        ),
        step_def(
            "plugin-managed migration is applied for install update and repair",
            &[(GLOBAL, "String"), (ROOT, "PathBuf")],
            &[
                (GLOBAL, "String"),
                (ROOT, "PathBuf"),
                (HANDLE, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, _| {
                let mut global = ctx.get::<String>(GLOBAL).cloned().ok_or("missing global")?;
                for _ in 0..3 {
                    global = codex::absorb_legacy_global_route(&global)?;
                }
                let mut out = Context::new();
                carry(&ctx, &mut out);
                out.set(GLOBAL, global);
                Ok(out)
            },
        ),
        step_def(
            "plugin-managed migration is applied and the staged plugin is unloaded",
            &[(GLOBAL, "String"), (ROOT, "PathBuf")],
            &[
                (GLOBAL, "String"),
                (ROOT, "PathBuf"),
                (HANDLE, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, _| {
                let global = codex::absorb_legacy_global_route(
                    ctx.get::<String>(GLOBAL).ok_or("missing global")?,
                )?;
                let root = ctx.get::<PathBuf>(ROOT).ok_or("missing root")?;
                std::fs::remove_dir_all(root.join(".codex-plugin")).map_err(|e| e.to_string())?;
                std::fs::remove_dir_all(root.join("codex-hooks")).map_err(|e| e.to_string())?;
                let mut out = Context::new();
                carry(&ctx, &mut out);
                out.set(GLOBAL, global);
                Ok(out)
            },
        ),
        step_def(
            "an isolated Codex home and local marketplace containing the staged Anvil plugin",
            &[],
            &[
                (ROOT, "PathBuf"),
                (HANDLE, "Arc<Mutex<Option<TempDir>>>"),
                (CODEX_HOME, "PathBuf"),
                (MARKETPLACE, "PathBuf"),
            ],
            |_ctx, _| {
                let (handle, marketplace) = retained_temp_dir("anvil-real-codex-marketplace-")?;
                let plugin_root = marketplace.join("plugins/anvil-kit");
                stage_plugin_assets(&plugin_root)?;
                let registry = marketplace.join(".agents/plugins");
                std::fs::create_dir_all(&registry).map_err(|e| e.to_string())?;
                std::fs::write(
                    registry.join("marketplace.json"),
                    serde_json::to_vec_pretty(&serde_json::json!({
                        "name": "anvil-real-codex",
                        "plugins": [{
                            "name": "anvil-kit",
                            "source": {
                                "source": "local",
                                "path": "./plugins/anvil-kit"
                            },
                            "policy": {
                                "installation": "AVAILABLE",
                                "authentication": "ON_INSTALL"
                            }
                        }]
                    }))
                    .map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                let codex_home = marketplace.join("codex-home");
                std::fs::create_dir_all(&codex_home).map_err(|e| e.to_string())?;
                let mut out = Context::new();
                out.set(ROOT, plugin_root);
                out.set(HANDLE, handle);
                out.set(CODEX_HOME, codex_home);
                out.set(MARKETPLACE, marketplace);
                Ok(out)
            },
        ),
        step_def(
            "real Codex registers the marketplace and installs the Anvil plugin",
            &[(CODEX_HOME, "PathBuf"), (MARKETPLACE, "PathBuf")],
            &[
                (ROOT, "PathBuf"),
                (HANDLE, "Arc<Mutex<Option<TempDir>>>"),
                (CODEX_HOME, "PathBuf"),
                (MARKETPLACE, "PathBuf"),
                (CODEX_LIST, "String"),
            ],
            |ctx, _| {
                let codex_home = ctx.get::<PathBuf>(CODEX_HOME).ok_or("missing Codex home")?;
                let marketplace = ctx
                    .get::<PathBuf>(MARKETPLACE)
                    .ok_or("missing marketplace")?;
                let marketplace_text = marketplace
                    .to_str()
                    .ok_or("marketplace path is not UTF-8")?;
                run_codex(
                    codex_home,
                    &["plugin", "marketplace", "add", marketplace_text, "--json"],
                )?;
                run_codex(
                    codex_home,
                    &["plugin", "add", "anvil-kit@anvil-real-codex", "--json"],
                )?;
                let list = run_codex(codex_home, &["plugin", "list", "--json"])?;
                let mut out = Context::new();
                carry(&ctx, &mut out);
                out.set(CODEX_LIST, list);
                Ok(out)
            },
        ),
        check_def(
            "the staged Codex plugin manifest references {string}",
            &[(ROOT, "PathBuf")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("missing reference")?;
                let root = ctx.get::<PathBuf>(ROOT).ok_or("missing root")?;
                let got = parsed(plugin_manifest(root))?
                    .get("hooks")
                    .and_then(|value| value.as_str())
                    .map(str::to_string);
                (got.as_deref() == Some(want))
                    .then_some(())
                    .ok_or_else(|| format!("hooks reference: expected {want}, got {got:?}"))
            },
        ),
        check_def(
            "the referenced Codex hook asset has exactly one source-codex route",
            &[(ROOT, "PathBuf")],
            |ctx, _| {
                let root = ctx.get::<PathBuf>(ROOT).ok_or("missing root")?;
                let value = parsed(plugin_hooks(root))?;
                let entries = value
                    .pointer("/hooks/UserPromptSubmit")
                    .and_then(|value| value.as_array())
                    .ok_or("missing route array")?;
                (entries.len() == 1
                    && entries[0]
                        .pointer("/hooks/0/command")
                        .and_then(|v| v.as_str())
                        == Some(
                            r#""${PLUGIN_ROOT}/codex-hooks/anvil-hooks" route-turn --source codex"#,
                        ))
                .then_some(())
                .ok_or_else(|| format!("route contract drifted: {value}"))
            },
        ),
        check_def(
            "the referenced Codex hook asset has exactly one apply_patch-or-Bash hard gate",
            &[(ROOT, "PathBuf")],
            |ctx, _| {
                let root = ctx.get::<PathBuf>(ROOT).ok_or("missing root")?;
                let value = parsed(plugin_hooks(root))?;
                let entries = value
                    .pointer("/hooks/PreToolUse")
                    .and_then(|value| value.as_array())
                    .ok_or("missing gate array")?;
                (entries.len() == 1
                    && entries[0].get("matcher").and_then(|v| v.as_str())
                        == Some("apply_patch|Bash")
                    && entries[0]
                        .pointer("/hooks/0/command")
                        .and_then(|v| v.as_str())
                        == Some(
                            r#""${PLUGIN_ROOT}/codex-hooks/anvil-hooks" gate-check --source codex --hard-enforce track --internal-deadline-ms 4000"#,
                        ))
                .then_some(())
                .ok_or_else(|| format!("gate contract drifted: {value}"))
            },
        ),
        check_def(
            "the referenced Codex hard gate carries the track enforcement policy",
            &[(ROOT, "PathBuf")],
            |ctx, _| {
                let root = ctx.get::<PathBuf>(ROOT).ok_or("missing root")?;
                let value = parsed(plugin_hooks(root))?;
                let command = value
                    .pointer("/hooks/PreToolUse/0/hooks/0/command")
                    .and_then(|v| v.as_str());
                (command
                    == Some(
                        r#""${PLUGIN_ROOT}/codex-hooks/anvil-hooks" gate-check --source codex --hard-enforce track --internal-deadline-ms 4000"#,
                    ))
                    .then_some(())
                    .ok_or_else(|| format!("Codex hard policy missing from command: {value}"))
            },
        ),
        check_def(
            "the Codex internal deadline is shorter than the outer hook timeout",
            &[(ROOT, "PathBuf")],
            |ctx, _| {
                let root = ctx.get::<PathBuf>(ROOT).ok_or("missing root")?;
                let value = parsed(plugin_hooks(root))?;
                let gate = value
                    .pointer("/hooks/PreToolUse/0/hooks/0")
                    .ok_or("missing Codex gate")?;
                let command = gate
                    .get("command")
                    .and_then(|v| v.as_str())
                    .ok_or("missing gate command")?;
                let deadline_ms = command
                    .split_ascii_whitespace()
                    .collect::<Vec<_>>()
                    .windows(2)
                    .find_map(|pair| {
                        (pair[0] == "--internal-deadline-ms")
                            .then(|| pair[1].parse::<u64>().ok())
                            .flatten()
                    })
                    .ok_or("missing internal deadline")?;
                let outer_ms = gate
                    .get("timeout")
                    .and_then(|v| v.as_u64())
                    .ok_or("missing outer timeout")?
                    * 1000;
                (deadline_ms < outer_ms).then_some(()).ok_or_else(|| {
                    format!(
                        "internal deadline {deadline_ms}ms is not shorter than outer {outer_ms}ms"
                    )
                })
            },
        ),
        check_def(
            "the staged Codex hook wrapper reaches the native host binary",
            &[(ROOT, "PathBuf")],
            |ctx, _| {
                let root = ctx.get::<PathBuf>(ROOT).ok_or("missing staged plugin")?;
                let wrapper = root.join("codex-hooks/anvil-hooks");
                let native = root.join("app/hooks").join(native_host_binary_name()?);
                let output = Command::new(&wrapper)
                    .arg("render-codex-plugin-hooks")
                    .output()
                    .map_err(|e| format!("execute {}: {e}", wrapper.display()))?;
                (wrapper.exists()
                    && native.exists()
                    && output.status.success()
                    && serde_json::from_slice::<serde_json::Value>(&output.stdout).is_ok())
                .then_some(())
                .ok_or_else(|| {
                    format!(
                        "wrapper {} does not reach {}: status={}, stderr={}",
                        wrapper.display(),
                        native.display(),
                        output.status,
                        String::from_utf8_lossy(&output.stderr)
                    )
                })
            },
        ),
        check_def(
            "the Anvil kit manifest keeps its global hard policy disabled",
            &[],
            |_, _| {
                let value = parsed(workspace_root().join("kit/foundry-manifest.json"))?;
                (value
                    .pointer("/hooks/hard_enforce")
                    .and_then(|v| v.as_bool())
                    == Some(false))
                .then_some(())
                .ok_or_else(|| format!("Anvil kit hard policy drifted: {value}"))
            },
        ),
        check_def(
            "the staged Codex hooks use plugin-root commands for POSIX and Windows",
            &[(ROOT, "PathBuf")],
            |ctx, _| {
                let root = ctx.get::<PathBuf>(ROOT).ok_or("missing staged kit")?;
                let value = parsed(plugin_hooks(root))?;
                for event in ["UserPromptSubmit", "PreToolUse"] {
                    let handler = value
                        .pointer(&format!("/hooks/{event}/0/hooks/0"))
                        .ok_or_else(|| format!("missing {event} handler"))?;
                    let posix = handler
                        .get("command")
                        .and_then(|v| v.as_str())
                        .ok_or("missing POSIX command")?;
                    let windows = handler
                        .get("commandWindows")
                        .and_then(|v| v.as_str())
                        .ok_or("missing Windows command")?;
                    if !posix.contains("${PLUGIN_ROOT}/codex-hooks/anvil-hooks")
                        || !windows.contains("%PLUGIN_ROOT%\\codex-hooks\\anvil-hooks.cmd")
                    {
                        return Err(format!("plugin-root command missing: {handler}"));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "the staged Codex hook wrappers reach each packaged platform binary",
            &[(ROOT, "PathBuf")],
            |ctx, _| {
                let root = ctx.get::<PathBuf>(ROOT).ok_or("missing staged kit")?;
                let posix = std::fs::read_to_string(root.join("codex-hooks/anvil-hooks"))
                    .map_err(|e| e.to_string())?;
                let windows = std::fs::read_to_string(root.join("codex-hooks/anvil-hooks.cmd"))
                    .map_err(|e| e.to_string())?;
                for mapping in [
                    "\"Darwin arm64\") PLATFORM=darwin-arm64",
                    "\"Darwin x86_64\") PLATFORM=darwin-x64",
                ] {
                    if !posix.contains(mapping) {
                        return Err(format!("POSIX wrapper omits {mapping}"));
                    }
                }
                if !posix.contains("app/hooks/anvil-hooks-$PLATFORM") {
                    return Err("POSIX wrapper omits platform binary dispatch".to_string());
                }
                if root.join("app/hooks/anvil-hooks-windows-x64.exe").exists()
                    && !windows.contains("anvil-hooks-windows-x64.exe")
                {
                    return Err("Windows wrapper omits windows-x64".to_string());
                }
                Ok(())
            },
        ),
        step_def(
            "the staged POSIX Codex gate command runs outside every Anvil hearth",
            &[(ROOT, "PathBuf")],
            &[
                (ROOT, "PathBuf"),
                (HANDLE, "Arc<Mutex<Option<TempDir>>>"),
                (STAGED_EXIT, "i32"),
            ],
            |ctx, _| {
                let root = ctx.get::<PathBuf>(ROOT).ok_or("missing staged kit")?;
                let hooks = parsed(plugin_hooks(root))?;
                let command = hooks
                    .pointer("/hooks/PreToolUse/0/hooks/0/command")
                    .and_then(|v| v.as_str())
                    .ok_or("missing staged gate command")?;
                let outside = root.join("codex-runtime-outside");
                std::fs::create_dir_all(&outside).map_err(|e| e.to_string())?;
                let target = outside.join("notes.md");
                std::fs::write(&target, "# outside\n").map_err(|e| e.to_string())?;
                let mut child = Command::new("sh")
                    .args(["-c", command])
                    .current_dir(&outside)
                    .env("PLUGIN_ROOT", root)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|e| e.to_string())?;
                use std::io::Write;
                child
                    .stdin
                    .take()
                    .ok_or("missing stdin")?
                    .write_all(
                        serde_json::json!({
                            "tool_name": "Bash",
                            "cwd": outside,
                            "tool_input": {"command": format!("touch {}", target.display())}
                        })
                        .to_string()
                        .as_bytes(),
                    )
                    .map_err(|e| e.to_string())?;
                let code = child
                    .wait()
                    .map_err(|e| e.to_string())?
                    .code()
                    .unwrap_or(-1);
                let mut out = Context::new();
                carry(&ctx, &mut out);
                out.set(STAGED_EXIT, code);
                Ok(out)
            },
        ),
        check_def(
            "the staged Codex gate command exits successfully",
            &[(STAGED_EXIT, "i32")],
            |ctx, _| {
                let code = *ctx.get::<i32>(STAGED_EXIT).ok_or("missing staged exit")?;
                (code == 0)
                    .then_some(())
                    .ok_or_else(|| format!("staged gate exited {code}"))
            },
        ),
        check_def(
            "the Codex global hooks have zero Foundry-owned routes",
            &[(GLOBAL, "String")],
            |ctx, _| {
                let global = ctx.get::<String>(GLOBAL).ok_or("missing global")?;
                (!global.contains("anvil-hooks route-turn"))
                    .then_some(())
                    .ok_or_else(|| format!("Foundry route remains: {global}"))
            },
        ),
        check_def(
            "the Codex global hooks still have the unrelated hook",
            &[(GLOBAL, "String")],
            |ctx, _| {
                ctx.get::<String>(GLOBAL)
                    .is_some_and(|global| global.contains("operator-own-hook"))
                    .then_some(())
                    .ok_or("operator hook missing".to_string())
            },
        ),
        check_def(
            "the staged plugin still has one route and one gate",
            &[(ROOT, "PathBuf")],
            |ctx, _| {
                let root = ctx.get::<PathBuf>(ROOT).ok_or("missing root")?;
                let value = parsed(plugin_hooks(root))?;
                let routes = value
                    .pointer("/hooks/UserPromptSubmit")
                    .and_then(|v| v.as_array())
                    .map(Vec::len);
                let gates = value
                    .pointer("/hooks/PreToolUse")
                    .and_then(|v| v.as_array())
                    .map(Vec::len);
                (routes == Some(1) && gates == Some(1))
                    .then_some(())
                    .ok_or_else(|| format!("route/gate singularity failed: {value}"))
            },
        ),
        check_def(
            "no staged Anvil plugin route uses source claude-code",
            &[(ROOT, "PathBuf")],
            |ctx, _| {
                let root = ctx.get::<PathBuf>(ROOT).ok_or("missing root")?;
                let text =
                    std::fs::read_to_string(plugin_hooks(root)).map_err(|e| e.to_string())?;
                (!text.contains("--source claude-code"))
                    .then_some(())
                    .ok_or("stale Claude source remains".to_string())
            },
        ),
        check_def(
            "the staged Codex plugin configuration is absent",
            &[(ROOT, "PathBuf")],
            |ctx, _| {
                let root = ctx.get::<PathBuf>(ROOT).ok_or("missing root")?;
                (!plugin_manifest(root).exists() && !plugin_hooks(root).exists())
                    .then_some(())
                    .ok_or("plugin configuration remains".to_string())
            },
        ),
        check_def(
            // The expected version is READ FROM THE KIT MANIFEST, not typed into
            // the Gherkin. `a83a5fc` made the plugin derive its version from the
            // manifest "instead of hand-syncing" and left this literal
            // hand-synced: at `0.2.247` the scenario still asserted `0.2.246`
            // and was red on origin/main itself. Deriving it here keeps the
            // assertion FAILABLE — an installed plugin whose version disagrees
            // with the manifest still reds, which is the drift the scenario is
            // for — while removing the second copy that has to be kept in step.
            "real Codex reports installed Anvil plugin version from the kit manifest",
            &[(CODEX_LIST, "String")],
            |ctx, _params| {
                let manifest_path = workspace_root().join("kit/foundry-manifest.json");
                let manifest_text = std::fs::read_to_string(&manifest_path)
                    .map_err(|e| format!("read {}: {e}", manifest_path.display()))?;
                let manifest: serde_json::Value = serde_json::from_str(&manifest_text)
                    .map_err(|e| format!("parse kit manifest: {e}"))?;
                let version = manifest
                    .get("kit")
                    .and_then(|k| k.get("version"))
                    .and_then(|v| v.as_str())
                    .ok_or("kit manifest declares no kit.version")?
                    .to_string();
                let version = version.as_str();
                let list = ctx
                    .get::<String>(CODEX_LIST)
                    .ok_or("missing Codex plugin list")?;
                let value: serde_json::Value =
                    serde_json::from_str(list).map_err(|e| format!("parse Codex list: {e}"))?;
                let installed = value
                    .get("installed")
                    .and_then(|plugins| plugins.as_array())
                    .is_some_and(|plugins| {
                        plugins.iter().any(|plugin| {
                            plugin.get("name").and_then(|v| v.as_str()) == Some("anvil-kit")
                                && plugin.get("version").and_then(|v| v.as_str()) == Some(version)
                                && plugin.get("installed").and_then(|v| v.as_bool()) == Some(true)
                        })
                    });
                installed
                    .then_some(())
                    .ok_or_else(|| format!("installed anvil-kit {version} not reported: {value}"))
            },
        ),
    ]
}
