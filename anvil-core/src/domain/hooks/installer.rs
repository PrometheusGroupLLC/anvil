//! The install/uninstall ORCHESTRATION: read a harness's native config file,
//! apply the adapter's pure transform, write it back, and report what happened.
//! Auto-detect probes each harness's conventional config dir (overridable per
//! call), and `auto` installs into every detected harness, skipping the absent
//! ones with a clear report.
//!
//! The filesystem is the only side effect here; the content transforms live in
//! the pure per-harness adapters. Install-time computes the installable hook set
//! DIRECTLY from the hearth via the caller (anvil-core's `fold_hook_manifest`) —
//! no running daemon is required to install.

use super::{Harness, HookAdapter, InstallSpec};
use std::path::{Path, PathBuf};

/// The outcome of acting on a single harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HarnessOutcome {
    /// The config was written (install) or cleaned (uninstall).
    Written,
    /// The harness's config dir was not detected, so it was skipped.
    Skipped,
    /// The action failed; carries the reason.
    Failed(String),
}

impl HarnessOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            HarnessOutcome::Written => "written",
            HarnessOutcome::Skipped => "skipped",
            HarnessOutcome::Failed(_) => "failed",
        }
    }
}

/// A per-harness install/uninstall report line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessReport {
    pub harness: Harness,
    pub config_path: PathBuf,
    pub outcome: HarnessOutcome,
    pub gate_capability: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMode {
    Standalone,
    CodexPluginManaged,
}

/// Resolve the config FILE path for a harness given an explicit config DIR
/// override (the `--config-dir` flag) or the harness's conventional dir.
pub fn config_path_for(harness: Harness, config_dir: &Path) -> PathBuf {
    config_dir.join(harness.config_filename())
}

/// Whether a harness is "present" at `config_dir`: the directory exists. (The
/// config FILE itself may not exist yet — install creates it.)
pub fn is_detected(config_dir: &Path) -> bool {
    config_dir.is_dir()
}

/// Install the anvil-managed hook block into one harness's config file at
/// `config_dir`. Creates the config file when absent; idempotent on re-run.
///
/// MCP registration is OPT-IN via `with_mcp`. Default (`false`) is HOOKS ONLY —
/// the harness's MCP config is never touched (Foundry's `wire.rs` owns the
/// version-stable anvil-mcp entry). `true` is the standalone path: also register
/// the anvil MCP server for users running anvil WITHOUT Foundry.
pub fn install_one(
    harness: Harness,
    config_dir: &Path,
    spec: &InstallSpec,
    with_mcp: bool,
) -> HarnessReport {
    let adapter = harness.adapter();
    let path = config_path_for(harness, config_dir);
    // Fail open: a MISSING file is an empty config (install creates it), but any
    // OTHER read error — permission, transient IO, INVALID UTF-8 — must NOT be
    // treated as empty, or we would overwrite existing content we couldn't read.
    let existing = match read_existing(&path) {
        Ok(s) => s,
        Err(e) => {
            return HarnessReport {
                harness,
                config_path: path,
                outcome: HarnessOutcome::Failed(e),
                gate_capability: adapter.gate_capability().as_str(),
            };
        }
    };
    let outcome = match adapter.install(&existing, spec) {
        Ok(next) => match write_config(&path, &next) {
            // Config written — now write any plugin/artifact files (grok plugin,
            // opencode JS plugin). An artifact failure surfaces as Failed.
            Ok(()) => match write_artifacts(&*adapter, config_dir, spec) {
                // Hooks written — only ALSO register the anvil MCP server when the
                // standalone opt-in is set. An MCP failure surfaces as Failed.
                Ok(()) if with_mcp => match register_mcp(harness, config_dir, spec) {
                    Ok(()) => HarnessOutcome::Written,
                    Err(e) => HarnessOutcome::Failed(e),
                },
                Ok(()) => HarnessOutcome::Written,
                Err(e) => HarnessOutcome::Failed(e),
            },
            Err(e) => HarnessOutcome::Failed(e),
        },
        Err(e) => HarnessOutcome::Failed(e),
    };
    HarnessReport {
        harness,
        config_path: path,
        outcome,
        gate_capability: adapter.gate_capability().as_str(),
    }
}

/// Register (or refresh) the anvil MCP server in this harness's MCP config, if it
/// has an MCP writer. A harness without one (Hermes / Kiln) is a no-op success.
fn register_mcp(harness: Harness, config_dir: &Path, spec: &InstallSpec) -> Result<(), String> {
    let (Some(writer), Some(filename)) = (harness.mcp_writer(), harness.mcp_config_filename())
    else {
        return Ok(());
    };
    let path = config_dir.join(filename);
    // ── C-d.1 round 8, H-3: THE HIGHEST-BLAST-RADIUS SITE ON THE TRACK ──
    //
    // This was `read_to_string(&path).unwrap_or_default()`. `parse_json("")`
    // returns an empty map, so `register` built a config containing ONLY the
    // anvil entry, and `write_config` wrote it over the real one. The write is
    // `std::fs::write`, which needs write permission and not read permission —
    // so mode `0200` is reachable with nothing exotic, and `EIO`/`ESTALE` reach
    // it at any mode.
    //
    // Measured on unmutated `63df2ff`, through the public `install_one`:
    //
    //   .claude.json seeded with mcpServers{kiln, lore} + otherUserSettings{theme}
    //   chmod 0200 (present, statable, unreadable)
    //
    //     outcome reported : "written"   <-- SUCCESS
    //     kiln : GONE   lore : GONE   theme : GONE
    //
    // Three aggravations the seven read-then-write sites round 7 closed do not
    // have: the destroyed data belongs to OTHER TOOLS, it is on the USER'S OWN
    // MACHINE, and the call REPORTS SUCCESS rather than refusing.
    //
    // `unregister_mcp`, twelve lines below, was already correct — it declines to
    // write when it cannot read. This is now the same posture, and the asymmetry
    // inside one file is closed.
    //
    // ABSENT is still an answer: a config file that is not there genuinely holds
    // no entries, and a first install must be able to create one.
    let existing = match std::fs::read_to_string(&path) {
        Ok(body) => body,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            return Err(format!(
                "mcp_config_uninspectable: {} exists and could not be read: {e}. Refusing to \
                 write. Reading it as EMPTY would build a config holding only anvil's entry and \
                 write that over the user's — destroying every other tool's MCP registration and \
                 every unrelated setting in the file — and would report success while doing it.",
                path.display()
            ))
        }
    };
    let next = writer.register(&existing, &spec.mcp_command)?;
    write_config(&path, &next)
}

/// Unregister the anvil MCP server from this harness's MCP config, if it has an
/// MCP writer. A missing config file or absent writer is a no-op success.
fn unregister_mcp(harness: Harness, config_dir: &Path) -> Result<(), String> {
    let (Some(writer), Some(filename)) = (harness.mcp_writer(), harness.mcp_config_filename())
    else {
        return Ok(());
    };
    let path = config_dir.join(filename);
    let Ok(existing) = std::fs::read_to_string(&path) else {
        return Ok(()); // No MCP config file → nothing anvil-owned to remove.
    };
    let next = writer.unregister(&existing)?;
    write_config(&path, &next)
}

/// Remove the anvil-managed hook block from one harness's config file. A missing
/// file is a no-op success (nothing to remove).
///
/// MCP unregistration is OPT-IN via `with_mcp`. Default (`false`) removes ONLY the
/// hooks, leaving any MCP entry alone (Foundry's `wire.rs` owns it). `true` is the
/// standalone path: also remove the anvil MCP entry.
pub fn uninstall_one(harness: Harness, config_dir: &Path, with_mcp: bool) -> HarnessReport {
    let adapter = harness.adapter();
    let path = config_path_for(harness, config_dir);
    // Remove any plugin/artifact files first (best-effort cleanup — a missing
    // artifact is fine; the config uninstall below is the primary outcome).
    let _ = remove_artifacts(&*adapter, config_dir);
    let outcome = match std::fs::read_to_string(&path) {
        Ok(existing) => match adapter.uninstall(&existing) {
            Ok(next) => match write_config(&path, &next) {
                // Hooks removed — only ALSO unregister the anvil MCP server when
                // the standalone opt-in is set.
                Ok(()) if with_mcp => match unregister_mcp(harness, config_dir) {
                    Ok(()) => HarnessOutcome::Written,
                    Err(e) => HarnessOutcome::Failed(e),
                },
                Ok(()) => HarnessOutcome::Written,
                Err(e) => HarnessOutcome::Failed(e),
            },
            Err(e) => HarnessOutcome::Failed(e),
        },
        // No hook config file. With the standalone opt-in, still try to remove an
        // anvil MCP entry that may exist independently (Claude Code's separate
        // .claude.json). Default leaves any MCP entry untouched.
        Err(_) if with_mcp => match unregister_mcp(harness, config_dir) {
            Ok(()) => HarnessOutcome::Skipped,
            Err(e) => HarnessOutcome::Failed(e),
        },
        Err(_) => HarnessOutcome::Skipped,
    };
    HarnessReport {
        harness,
        config_path: path,
        outcome,
        gate_capability: adapter.gate_capability().as_str(),
    }
}

/// Install across every harness in `targets`, resolving each one's config dir via
/// `config_dir_for`. A harness whose dir is not detected is reported `Skipped`
/// (never written). Used by `auto` (all harnesses) and explicit single-harness
/// installs alike.
pub fn install_all(
    targets: &[Harness],
    config_dir_for: impl Fn(Harness) -> PathBuf,
    spec: &InstallSpec,
    with_mcp: bool,
) -> Vec<HarnessReport> {
    install_all_with_mode(
        targets,
        config_dir_for,
        spec,
        with_mcp,
        InstallMode::Standalone,
    )
}

pub fn install_all_with_mode(
    targets: &[Harness],
    config_dir_for: impl Fn(Harness) -> PathBuf,
    spec: &InstallSpec,
    with_mcp: bool,
    mode: InstallMode,
) -> Vec<HarnessReport> {
    targets
        .iter()
        .map(|&harness| {
            let dir = config_dir_for(harness);
            if is_detected(&dir) {
                if mode == InstallMode::CodexPluginManaged && harness == Harness::Codex {
                    absorb_codex_global(&dir)
                } else {
                    install_one(harness, &dir, spec, with_mcp)
                }
            } else {
                HarnessReport {
                    harness,
                    config_path: config_path_for(harness, &dir),
                    outcome: HarnessOutcome::Skipped,
                    gate_capability: harness.adapter().gate_capability().as_str(),
                }
            }
        })
        .collect()
}

fn absorb_codex_global(config_dir: &Path) -> HarnessReport {
    let harness = Harness::Codex;
    let path = config_path_for(harness, config_dir);
    let outcome = match read_existing(&path) {
        Ok(existing) => {
            match super::codex::absorb_legacy_global_route(&existing)
                .and_then(|next| write_config(&path, &next))
            {
                Ok(()) => HarnessOutcome::Written,
                Err(error) => HarnessOutcome::Failed(error),
            }
        }
        Err(error) => HarnessOutcome::Failed(error),
    };
    HarnessReport {
        harness,
        config_path: path,
        outcome,
        gate_capability: harness.adapter().gate_capability().as_str(),
    }
}

/// Uninstall across every harness in `targets`.
pub fn uninstall_all(
    targets: &[Harness],
    config_dir_for: impl Fn(Harness) -> PathBuf,
    with_mcp: bool,
) -> Vec<HarnessReport> {
    targets
        .iter()
        .map(|&harness| {
            let dir = config_dir_for(harness);
            if is_detected(&dir) {
                uninstall_one(harness, &dir, with_mcp)
            } else {
                HarnessReport {
                    harness,
                    config_path: config_path_for(harness, &dir),
                    outcome: HarnessOutcome::Skipped,
                    gate_capability: harness.adapter().gate_capability().as_str(),
                }
            }
        })
        .collect()
}

/// Read an existing config file for transform. A MISSING file is `Ok("")` — the
/// adapter treats it as empty and install creates it. Any OTHER read error
/// (permission, transient IO, INVALID UTF-8) is an `Err` so the caller FAILS OPEN
/// rather than treating unreadable content as empty and overwriting it.
///
/// (Uninstall's own read at [`uninstall_one`] already fails open by SKIPPING on any
/// read error — it never writes fresh content, so it can't clobber an unreadable
/// file the way an install would.)
fn read_existing(path: &Path) -> Result<String, String> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(format!("read {}: {}", path.display(), e)),
    }
}

/// Write `content` to `path`, creating parent dirs as needed.
fn write_config(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create dir {}: {}", parent.display(), e))?;
    }
    std::fs::write(path, content).map_err(|e| format!("write {}: {}", path.display(), e))
}

/// Write an adapter's plugin/artifact files under `config_dir` (each `rel_path`
/// is joined to it), creating parent dirs and setting the executable bit when
/// requested. A config-only adapter returns no artifacts, so this is a no-op.
fn write_artifacts(
    adapter: &dyn HookAdapter,
    config_dir: &Path,
    spec: &InstallSpec,
) -> Result<(), String> {
    for art in adapter.artifacts(spec) {
        let path = config_dir.join(&art.rel_path);
        write_config(&path, &art.content)?;
        if art.executable {
            set_executable(&path)?;
        }
    }
    Ok(())
}

/// Remove an adapter's artifact paths under `config_dir` (a file or a directory,
/// removed recursively). A path that does not exist is a no-op success.
fn remove_artifacts(adapter: &dyn HookAdapter, config_dir: &Path) -> Result<(), String> {
    for rel in adapter.artifact_paths() {
        let path = config_dir.join(&rel);
        if path.is_dir() {
            std::fs::remove_dir_all(&path)
                .map_err(|e| format!("remove dir {}: {}", path.display(), e))?;
        } else if path.exists() {
            std::fs::remove_file(&path).map_err(|e| format!("remove {}: {}", path.display(), e))?;
        }
    }
    Ok(())
}

/// Mark a file executable (0o755). No-op on non-unix.
#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)
        .map_err(|e| format!("stat {}: {}", path.display(), e))?
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).map_err(|e| format!("chmod {}: {}", path.display(), e))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// Render a one-line human report for a [`HarnessReport`].
pub fn render_report(report: &HarnessReport) -> String {
    match &report.outcome {
        HarnessOutcome::Written => format!(
            "{}: written ({}) -> {} [gate: {}]",
            report.harness.id(),
            report.harness.config_filename(),
            report.config_path.display(),
            report.gate_capability,
        ),
        HarnessOutcome::Skipped => format!(
            "{}: skipped (config dir not detected) -> {}",
            report.harness.id(),
            report.config_path.display(),
        ),
        HarnessOutcome::Failed(reason) => {
            format!("{}: failed -> {}", report.harness.id(), reason)
        }
    }
}
