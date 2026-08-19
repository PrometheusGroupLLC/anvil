//! Client-side resolver: turn an env var + rendezvous dir + default into a
//! single [`Resolution`], probing the rendezvous URL honestly before trusting
//! it.

use std::path::PathBuf;
use std::time::Duration;

use crate::probe::{probe_health, IdentityCheck, ProbeOutcome};
use crate::record::RendezvousRecord;
use crate::writer::RENDEZVOUS_FILENAME;

/// Default health endpoint path. Override per kit (lore uses `/api/health`).
pub const DEFAULT_HEALTH_PATH: &str = "/health";

/// Default health-probe timeout budget (~400ms per the contract).
const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_millis(400);

/// Inputs to [`resolve`].
#[derive(Debug, Clone)]
pub struct ResolveOpts {
    /// Env var checked first as an explicit override, e.g. `LORE_ENGINE_URL`.
    pub env_var: String,
    /// Rendezvous directory, e.g. `~/.lore/`. `<dir>/engine.json` is read.
    pub dir: PathBuf,
    /// Last-resort URL if neither env nor a live rendezvous file resolves.
    /// `None` means "no default" → an [`ResolveSource::Unresolved`] result.
    pub default_url: Option<String>,
    /// Health endpoint path appended to the rendezvous URL when probing.
    /// Typically `/health`.
    pub health_path: String,
    /// Opt-in port-reuse guard: the JSON field name in the `/health` body whose
    /// value must equal the rendezvous record's `db_path` for the file to be
    /// trusted (e.g. `Some("db_path")`). When `None`, OR the record carries no
    /// `db_path`, no body identity is enforced — only the 2xx status (v1/v2
    /// behavior). Lore sets this to `"db_path"` to keep its db_path guard.
    pub health_identity_field: Option<String>,
    /// Health-probe timeout budget (bounds connect, write, AND read independently).
    /// Defaults to [`DEFAULT_PROBE_TIMEOUT`] (~400ms). Override per kit when the
    /// engine's health endpoint is legitimately slow — e.g. Lore's `/api/health`
    /// opens its ~400MB DB and can take >2s, which a 400ms budget reads as "down",
    /// silently breaking every resolve (and thus the capture-ingest sweep).
    pub probe_timeout: Duration,
}

impl ResolveOpts {
    /// Construct opts with the default `/health` path and no identity guard.
    pub fn new(env_var: impl Into<String>, dir: impl Into<PathBuf>) -> Self {
        Self {
            env_var: env_var.into(),
            dir: dir.into(),
            default_url: None,
            health_path: DEFAULT_HEALTH_PATH.to_string(),
            health_identity_field: None,
            probe_timeout: DEFAULT_PROBE_TIMEOUT,
        }
    }

    /// Override the health-probe timeout (default ~400ms). Use a larger budget for
    /// engines with a slow health endpoint (e.g. Lore's DB-opening `/api/health`).
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.probe_timeout = timeout;
        self
    }

    /// Set the last-resort default URL.
    pub fn with_default(mut self, default_url: impl Into<String>) -> Self {
        self.default_url = Some(default_url.into());
        self
    }

    /// Override the health endpoint path.
    pub fn with_health_path(mut self, health_path: impl Into<String>) -> Self {
        self.health_path = health_path.into();
        self
    }

    /// Enable the port-reuse guard: the named `/health` body field must equal
    /// the rendezvous record's `db_path`. No-op for records without a `db_path`.
    pub fn with_health_identity_field(mut self, field: impl Into<String>) -> Self {
        self.health_identity_field = Some(field.into());
        self
    }
}

/// Which input produced the resolved URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveSource {
    /// The env override won; used verbatim, not probed.
    Env,
    /// The rendezvous file won; its URL passed an HTTP health probe.
    Rendezvous,
    /// The configured default URL was used (env absent, rendezvous absent or
    /// dead).
    Default,
    /// Nothing resolved — no env, no live rendezvous, no default. `url` is
    /// `None`. Clients must treat this as "no engine reachable", never as a
    /// silent fallback.
    Unresolved,
}

/// The outcome of [`resolve`], carrying *why* so clients can log it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// Resolved base URL, or `None` when [`ResolveSource::Unresolved`].
    pub url: Option<String>,
    /// Which input produced `url`.
    pub source: ResolveSource,
    /// Whether an HTTP health probe was actually performed (only the
    /// rendezvous path probes).
    pub probed: bool,
    /// `true` if a rendezvous file existed but could not be parsed (garbage /
    /// wrong shape). Observable so clients can warn about a corrupt file even
    /// though resolution still falls through cleanly.
    pub corrupt: bool,
}

/// Resolve the engine URL.
///
/// Order:
/// 1. **Env** — if `env(opts.env_var)` is set and non-empty, use it verbatim
///    (`source: Env`, `probed: false`). No probe.
/// 2. **Rendezvous** — read `<dir>/engine.json`.
///    - *Absent* → fall through cleanly.
///    - *Corrupt/unreadable* → set `corrupt: true`, fall through (never hang).
///    - *Present* → HTTP `GET <url><health_path>` (~400ms) is **authoritative**.
///      2xx → `source: Rendezvous, probed: true`. `kill(pid, 0)` is logged as
///      an advisory hint only. A failed probe falls through.
/// 3. **Default** — if `opts.default_url` is set → `source: Default`; else
///    `source: Unresolved` (`url: None`).
pub fn resolve(opts: &ResolveOpts) -> Resolution {
    // (1) Env override — verbatim, no probe.
    if let Ok(val) = std::env::var(&opts.env_var) {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            return Resolution {
                url: Some(trimmed.to_string()),
                source: ResolveSource::Env,
                probed: false,
                corrupt: false,
            };
        }
    }

    // (2) Rendezvous file.
    let mut corrupt = false;
    // `probed` records whether an HTTP probe was actually ATTEMPTED — true even
    // when it fails and we fall through, so a client can tell "we verified via
    // probe" (Rendezvous) and "we tried a stale file then fell back" (Default
    // after a failed probe) apart from "no probe happened" (Env / no file).
    let mut probed = false;
    let path = opts.dir.join(RENDEZVOUS_FILENAME);
    match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<RendezvousRecord>(&bytes) {
            Ok(record) => {
                // HTTP probe is authoritative. pid is advisory only.
                let pid_alive = pid_is_alive(record.pid);
                probed = true;
                // Build the opt-in port-reuse guard: only when the caller named
                // a health identity field AND this record carries a db_path.
                let identity = match (&opts.health_identity_field, &record.db_path) {
                    (Some(field), Some(expected)) => Some(IdentityCheck {
                        field: field.clone(),
                        expected: expected.clone(),
                    }),
                    _ => None,
                };
                let outcome = probe_health(
                    &record.url,
                    &opts.health_path,
                    opts.probe_timeout,
                    identity.as_ref(),
                );
                match &outcome {
                    ProbeOutcome::Healthy => {
                        return Resolution {
                            url: Some(record.url),
                            source: ResolveSource::Rendezvous,
                            probed: true,
                            corrupt: false,
                        };
                    }
                    other => {
                        // Dead probe → fall through. Log the advisory pid hint
                        // so an operator can see the file looked live-ish but
                        // the authoritative probe failed.
                        eprintln!(
                            "engine-addressing: rendezvous {} probe failed ({other:?}); \
                             pid {} advisory-alive={pid_alive}; falling through",
                            path.display(),
                            record.pid
                        );
                    }
                }
            }
            Err(e) => {
                // Present but garbage. Observable, but still fall through.
                corrupt = true;
                eprintln!(
                    "engine-addressing: rendezvous {} is corrupt ({e}); falling through",
                    path.display()
                );
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Absent — clean fall through.
        }
        Err(e) => {
            // Unreadable for some other reason (permissions, etc.) — treat like
            // corrupt: observable, fall through, never hang.
            corrupt = true;
            eprintln!(
                "engine-addressing: rendezvous {} unreadable ({e}); falling through",
                path.display()
            );
        }
    }

    // (3) Default, else Unresolved.
    match &opts.default_url {
        Some(url) => Resolution {
            url: Some(url.clone()),
            source: ResolveSource::Default,
            probed,
            corrupt,
        },
        None => Resolution {
            url: None,
            source: ResolveSource::Unresolved,
            probed,
            corrupt,
        },
    }
}

/// Advisory liveness hint via `kill(pid, 0)`. **Never** authoritative — see
/// crate docs on the pid-reuse race. A `true` here means only "some process
/// with this pid exists", not "the engine is up".
#[cfg(unix)]
fn pid_is_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    // SAFETY: kill with signal 0 performs no signal delivery; it only checks
    // permission/existence of the target pid. No memory is touched.
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if rc == 0 {
        return true;
    }
    // EPERM means the process exists but we may not signal it — still "alive".
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Non-Unix: there is no cheap `kill(pid, 0)` equivalent, and this hint is only
/// advisory (the HTTP `/health` probe is authoritative), so report it as
/// unavailable rather than pulling in a Windows process-API dependency.
#[cfg(not(unix))]
fn pid_is_alive(_pid: u32) -> bool {
    false
}
