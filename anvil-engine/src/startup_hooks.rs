//! Startup-owned hook self-install for universal front-door presence.
//!
//! Production startup calls this after the engine has bound its listener and
//! detaches the work onto a blocking task. The installer stays fail-open: file
//! errors are reported per harness, and even unexpected panics are caught into a
//! report instead of escaping the startup path.

use anvil_core::domain::hooks::installer::{self, HarnessOutcome, HarnessReport};
use anvil_core::domain::hooks::{
    Harness, InstallSpec, DEFAULT_GATE_COMMAND, DEFAULT_TURN_COMMAND, DEFAULT_TURN_TIMEOUT_MS,
};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

pub const SKIP_HOOK_INSTALL_ENV: &str = "ANVIL_SKIP_HOOK_INSTALL";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfInstallReport {
    pub skipped_by_env: bool,
    pub reports: Vec<HarnessReport>,
    pub fatal_error: Option<String>,
}

impl SelfInstallReport {
    fn skipped_by_env() -> Self {
        Self {
            skipped_by_env: true,
            reports: Vec::new(),
            fatal_error: None,
        }
    }

    fn fatal(error: String) -> Self {
        Self {
            skipped_by_env: false,
            reports: Vec::new(),
            fatal_error: Some(error),
        }
    }
}

/// Run the engine startup hook self-install.
///
/// `config_root_override` mirrors `anvil-hooks install --harness auto
/// --config-dir <root>` for tests: each harness probes `<root>/<harness-id>`.
/// Production passes `None`, which probes each harness's conventional user
/// config dir under `$HOME`. `skip_env` is the already-resolved opt-out flag.
pub fn self_install_hooks(
    config_root_override: Option<&Path>,
    skip_env: bool,
) -> SelfInstallReport {
    if skip_env {
        return SelfInstallReport::skipped_by_env();
    }

    let config_root_override = config_root_override.map(Path::to_path_buf);
    let result = catch_unwind(AssertUnwindSafe(move || {
        let targets = Harness::all();
        let spec = InstallSpec {
            command: DEFAULT_GATE_COMMAND.to_string(),
            timeout_ms: 5000,
            // The route turn makes model calls and was measured at 8.5-9.5s; the
            // gate does not. This is the value the engine's own startup self-install
            // writes into every harness config, so it is what a hand-edit is
            // OVERWRITTEN with on the next restart — the source of truth is here,
            // not in the file.
            turn_timeout_ms: DEFAULT_TURN_TIMEOUT_MS,
            turn_command: DEFAULT_TURN_COMMAND.to_string(),
            // Hooks-only startup path: with_mcp is false below, so this is never
            // written to any harness MCP config.
            mcp_command: String::new(),
        };
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let dir_for = move |harness: Harness| match config_root_override.as_ref() {
            Some(root) => root.join(harness.id()),
            None => harness.default_config_dir(&home),
        };

        installer::install_all(&targets, dir_for, &spec, false)
    }));

    match result {
        Ok(reports) => SelfInstallReport {
            skipped_by_env: false,
            reports,
            fatal_error: None,
        },
        Err(payload) => SelfInstallReport::fatal(panic_payload_to_string(payload)),
    }
}

pub fn skip_env_present() -> bool {
    std::env::var_os(SKIP_HOOK_INSTALL_ENV)
        .map(|value| !value.is_empty())
        .unwrap_or(false)
}

pub fn spawn_self_install_hooks_from_env(
    config_root_override: Option<PathBuf>,
) -> tokio::task::JoinHandle<SelfInstallReport> {
    let skip_env = skip_env_present();
    tokio::task::spawn_blocking(move || {
        let report = self_install_hooks(config_root_override.as_deref(), skip_env);
        log_self_install_report(&report);
        report
    })
}

pub fn log_self_install_report(report: &SelfInstallReport) {
    if report.skipped_by_env {
        tracing::info!(
            env = SKIP_HOOK_INSTALL_ENV,
            "anvil hook self-install skipped by env"
        );
        return;
    }

    if let Some(error) = report.fatal_error.as_deref() {
        tracing::warn!(
            error = error,
            "anvil hook self-install failed open after unexpected error"
        );
        return;
    }

    for harness_report in &report.reports {
        match &harness_report.outcome {
            HarnessOutcome::Written => tracing::info!(
                harness = harness_report.harness.id(),
                config = %harness_report.config_path.display(),
                gate = harness_report.gate_capability,
                "anvil hook self-install installed"
            ),
            HarnessOutcome::Skipped => tracing::info!(
                harness = harness_report.harness.id(),
                config = %harness_report.config_path.display(),
                gate = harness_report.gate_capability,
                "anvil hook self-install skipped"
            ),
            HarnessOutcome::Failed(reason) => tracing::warn!(
                harness = harness_report.harness.id(),
                config = %harness_report.config_path.display(),
                gate = harness_report.gate_capability,
                error = reason,
                "anvil hook self-install errored"
            ),
        }
    }
}

fn panic_payload_to_string(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic during hook self-install".to_string()
    }
}
