//! Engine addressing — the on-disk rendezvous contract anvil publishes for
//! Foundry, and the client-side resolver that reads it back.
//!
//! Anvil's engine binds a **dynamic** TCP port (Foundry's supervisor assigns
//! `--port`), but clients historically defaulted to a hard-coded URL with no
//! shared discovery — so the engine could be alive while every client got
//! connection-refused. This module closes that gap from both sides:
//!
//! - **Engine side ([`publish`] / [`cleanup`]):** at the moment the real bound
//!   port is known, the engine atomically writes a [`RendezvousRecord`] to
//!   `<dir>/engine.json` (`<dir>` is `~/.anvil/`). On clean shutdown it removes
//!   the file.
//! - **Client side ([`resolve`]):** `anvil-hooks` resolves the engine URL in one
//!   fixed order — env override, then the rendezvous file (its URL is
//!   **honestly health-probed** over HTTP before being trusted), then a
//!   configured default.
//!
//! ## Honest health (the load-bearing rule)
//!
//! A rendezvous file is a **hint, not truth**. An HTTP `GET <url><health_path>`
//! with a short timeout is **authoritative**: only a 2xx response trusts the
//! file. The recorded `pid` is consulted via `kill(pid, 0)` purely as an
//! *advisory* stale-file signal (it is logged, never allowed to override a
//! failed HTTP probe — that avoids the pid-reuse race). A dead probe falls
//! through cleanly; the resolver never hangs and never returns a silent wrong
//! default.
//!
//! ## Provenance
//!
//! This was `foundry-engine-addressing`, a sibling crate that was never
//! published. The on-disk format and the resolution order are a cross-kit
//! contract shared with Foundry, so both are preserved byte-for-byte; only the
//! parts anvil actually calls are kept.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read as _, Write as _};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

// ── The record ───────────────────────────────────────────────────────────────

/// Schema version embedded in every [`RendezvousRecord`]. Bump when the wire
/// shape changes incompatibly; readers can branch on it.
pub const SCHEMA_VERSION: u32 = 1;

/// Filename, under the rendezvous directory, that holds the published record.
const RENDEZVOUS_FILENAME: &str = "engine.json";

/// The contents of `<dir>/engine.json`.
///
/// JSON field names are camelCase where the design specifies them
/// (`startedAt`) and snake_case for `schema_version`; the rest map 1:1.
///
/// ```json
/// {
///   "schema_version": 1,
///   "url": "http://127.0.0.1:14312",
///   "pid": 80123,
///   "version": "0.4.1",
///   "startedAt": 1718205600
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RendezvousRecord {
    /// On-disk schema version. Always [`SCHEMA_VERSION`] for records we write.
    pub schema_version: u32,
    /// Full base URL the engine is reachable at, e.g. `http://127.0.0.1:14312`.
    /// Stored as a complete string (no separate host/port split) so future
    /// transports (unix sockets, alternate schemes) don't break the contract.
    pub url: String,
    /// OS process id of the engine. Advisory only — used as a stale-file hint,
    /// never as authoritative liveness (see the module docs).
    pub pid: u32,
    /// Engine/kit version string, for diagnostics.
    pub version: String,
    /// Epoch seconds at which the engine published this record.
    #[serde(rename = "startedAt")]
    pub started_at: u64,
    /// Optional backing-store identity (e.g. the engine's `db_path`). Anvil does
    /// not set it; the field stays in the shape so records written by other
    /// Foundry kits round-trip unchanged, and it is skipped on serialization
    /// when absent (back-compat with readers that predate it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db_path: Option<String>,
}

impl RendezvousRecord {
    /// Build a record stamped with the current [`SCHEMA_VERSION`] and the
    /// current process id. `started_at` should be epoch seconds.
    pub fn new(url: impl Into<String>, version: impl Into<String>, started_at: u64) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            url: url.into(),
            pid: std::process::id(),
            version: version.into(),
            started_at,
            db_path: None,
        }
    }
}

// ── Engine side: publish / cleanup ───────────────────────────────────────────

/// Atomically publish `record` to `<dir>/engine.json`.
///
/// The write is crash-safe and never leaves a partial `engine.json`:
///
/// 1. `<dir>` is created if missing.
/// 2. The record is written to a per-pid temp file `engine.json.tmp.<pid>`.
/// 3. The temp file is **fsync'd** so its bytes hit disk.
/// 4. The temp file is **renamed** over `engine.json` (atomic on the same
///    filesystem — a concurrent reader sees either the old complete file or
///    the new complete file, never a torn write).
/// 5. The **parent directory is fsync'd** so the rename itself is durable
///    across power loss (important on macOS/APFS).
///
/// On any error the temp file is best-effort removed so a failed publish does
/// not leak `engine.json.tmp.<pid>`.
pub fn publish(dir: &Path, record: &RendezvousRecord) -> io::Result<()> {
    fs::create_dir_all(dir)?;

    let final_path = dir.join(RENDEZVOUS_FILENAME);
    let tmp_path = dir.join(format!("{RENDEZVOUS_FILENAME}.tmp.{}", std::process::id()));

    // Serialize first; if this fails we have not touched the filesystem.
    let mut bytes = serde_json::to_vec_pretty(record)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    bytes.push(b'\n');

    // Write + fsync the temp file. Scope the File so it is closed before the
    // rename. Clean up the temp file on any failure.
    let write_result = (|| -> io::Result<()> {
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp_path)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        Ok(())
    })();
    if let Err(e) = write_result {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }

    // Atomic swap into place.
    if let Err(e) = fs::rename(&tmp_path, &final_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }

    // fsync the parent directory so the rename survives a crash. Failure to
    // open/sync the directory is non-fatal for visibility (the file is already
    // in place).
    if let Ok(dir_file) = File::open(dir) {
        let _ = dir_file.sync_all();
    }

    Ok(())
}

/// Best-effort, idempotent removal of `<dir>/engine.json`.
///
/// Intended for clean shutdown. A missing file is **not** an error — calling
/// this twice, or on a never-published dir, is a no-op.
pub fn cleanup(dir: &Path) {
    let final_path = dir.join(RENDEZVOUS_FILENAME);
    match fs::remove_file(&final_path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => {
            eprintln!(
                "engine-addressing: cleanup of {} failed: {e}",
                final_path.display()
            );
        }
    }
}

// ── Client side: resolve ─────────────────────────────────────────────────────

/// Default health endpoint path. Override per caller via
/// [`ResolveOpts::with_health_path`].
const DEFAULT_HEALTH_PATH: &str = "/health";

/// Health-probe timeout budget (~400ms per the contract). Bounds connect,
/// write, AND read independently.
const PROBE_TIMEOUT: Duration = Duration::from_millis(400);

/// Inputs to [`resolve`].
#[derive(Debug, Clone)]
pub struct ResolveOpts {
    /// Env var checked first as an explicit override, e.g. `ANVIL_ENGINE_URL`.
    pub env_var: String,
    /// Rendezvous directory, e.g. `~/.anvil/`. `<dir>/engine.json` is read.
    pub dir: PathBuf,
    /// Last-resort URL if neither env nor a live rendezvous file resolves.
    /// `None` means "no default" → a [`Resolution`] with `url: None`.
    pub default_url: Option<String>,
    /// Health endpoint path appended to the rendezvous URL when probing.
    /// Typically `/health`.
    pub health_path: String,
}

impl ResolveOpts {
    /// Construct opts with the default `/health` path and no default URL.
    pub fn new(env_var: impl Into<String>, dir: impl Into<PathBuf>) -> Self {
        Self {
            env_var: env_var.into(),
            dir: dir.into(),
            default_url: None,
            health_path: DEFAULT_HEALTH_PATH.to_string(),
        }
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
}

/// The outcome of [`resolve`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// Resolved base URL, or `None` when nothing resolved — no env, no live
    /// rendezvous, no default. Clients must treat `None` as "no engine
    /// reachable", never as a silent fallback.
    pub url: Option<String>,
}

/// Resolve the engine URL.
///
/// Order:
/// 1. **Env** — if `env(opts.env_var)` is set and non-empty, use it verbatim.
///    No probe.
/// 2. **Rendezvous** — read `<dir>/engine.json`.
///    - *Absent* → fall through cleanly.
///    - *Corrupt/unreadable* → warn, fall through (never hang).
///    - *Present* → HTTP `GET <url><health_path>` (~400ms) is **authoritative**.
///      2xx → that URL wins. `kill(pid, 0)` is logged as an advisory hint only.
///      A failed probe falls through.
/// 3. **Default** — `opts.default_url` if set, else `url: None`.
pub fn resolve(opts: &ResolveOpts) -> Resolution {
    // (1) Env override — verbatim, no probe.
    if let Ok(val) = std::env::var(&opts.env_var) {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            return Resolution {
                url: Some(trimmed.to_string()),
            };
        }
    }

    // (2) Rendezvous file.
    let path = opts.dir.join(RENDEZVOUS_FILENAME);
    match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<RendezvousRecord>(&bytes) {
            Ok(record) => {
                // HTTP probe is authoritative. pid is advisory only.
                let pid_alive = pid_is_alive(record.pid);
                match probe_health(&record.url, &opts.health_path, PROBE_TIMEOUT) {
                    ProbeOutcome::Healthy => {
                        return Resolution {
                            url: Some(record.url),
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
            eprintln!(
                "engine-addressing: rendezvous {} unreadable ({e}); falling through",
                path.display()
            );
        }
    }

    // (3) Default, else unresolved.
    Resolution {
        url: opts.default_url.clone(),
    }
}

/// Advisory liveness hint via `kill(pid, 0)`. **Never** authoritative — see the
/// module docs on the pid-reuse race. A `true` here means only "some process
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

// ── The authoritative HTTP health probe ──────────────────────────────────────

/// Result of a single health probe. Private: the resolver is the only caller,
/// and it only distinguishes healthy from everything else (the rest is logged).
#[derive(Debug, Clone, PartialEq, Eq)]
enum ProbeOutcome {
    /// Server answered with an HTTP 2xx status.
    Healthy,
    /// Server answered, but not with 2xx (e.g. 404/500). Status kept for logs.
    Unhealthy(u16),
    /// Could not connect / timed out / malformed response. Carries a short
    /// reason for logging.
    Unreachable(String),
}

/// Probe `<base_url><health_path>` with the given timeout budget.
///
/// A deliberately tiny, dependency-free, blocking HTTP/1.1 `GET` over
/// [`std::net::TcpStream`] with hard connect/read/write timeouts. It does not
/// pull `reqwest`/`hyper`/`tokio` — this runs on the `anvil-hooks` hot path,
/// which is a short-lived synchronous process. We only need to know "did the
/// engine answer 2xx within the budget"; the body is not parsed.
///
/// Only `http://` URLs are supported; anything else is
/// [`ProbeOutcome::Unreachable`]. The same `timeout` bounds connect, write, and
/// read independently — the call returns well within a small multiple of
/// `timeout` and **never hangs**.
fn probe_health(base_url: &str, health_path: &str, timeout: Duration) -> ProbeOutcome {
    let (host, port, host_header) = match parse_http_authority(base_url) {
        Ok(t) => t,
        Err(e) => return ProbeOutcome::Unreachable(e),
    };

    let path = if health_path.starts_with('/') {
        health_path.to_string()
    } else {
        format!("/{health_path}")
    };

    // Resolve + connect with a timeout. Try each resolved address until one
    // connects within the budget.
    let addrs: Vec<SocketAddr> = match (host.as_str(), port).to_socket_addrs() {
        Ok(it) => it.collect(),
        Err(e) => return ProbeOutcome::Unreachable(format!("resolve {host}:{port}: {e}")),
    };
    if addrs.is_empty() {
        return ProbeOutcome::Unreachable(format!("no addresses for {host}:{port}"));
    }

    let mut stream = None;
    let mut last_err = String::from("no address tried");
    for addr in &addrs {
        match TcpStream::connect_timeout(addr, timeout) {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(e) => last_err = format!("connect {addr}: {e}"),
        }
    }
    let mut stream = match stream {
        Some(s) => s,
        None => return ProbeOutcome::Unreachable(last_err),
    };

    if let Err(e) = stream.set_read_timeout(Some(timeout)) {
        return ProbeOutcome::Unreachable(format!("set read timeout: {e}"));
    }
    if let Err(e) = stream.set_write_timeout(Some(timeout)) {
        return ProbeOutcome::Unreachable(format!("set write timeout: {e}"));
    }

    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host_header}\r\nConnection: close\r\nAccept: */*\r\n\r\n"
    );
    if let Err(e) = stream.write_all(request.as_bytes()) {
        return ProbeOutcome::Unreachable(format!("write request: {e}"));
    }

    // We only need the status line, so stop at the first CRLF.
    let mut buf = Vec::with_capacity(512);
    let mut chunk = [0u8; 512];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.windows(2).any(|w| w == b"\r\n") || buf.len() >= 4096 {
                    break;
                }
            }
            Err(e) => return ProbeOutcome::Unreachable(format!("read response: {e}")),
        }
    }

    parse_status(&buf)
}

/// Parse the HTTP status code out of a raw response prefix.
fn parse_status(buf: &[u8]) -> ProbeOutcome {
    let text = String::from_utf8_lossy(buf);
    let first_line = text.lines().next().unwrap_or("");
    // Expected: "HTTP/1.1 200 OK"
    let mut parts = first_line.split_whitespace();
    let proto = parts.next().unwrap_or("");
    if !proto.starts_with("HTTP/") {
        return ProbeOutcome::Unreachable(format!("malformed status line: {first_line:?}"));
    }
    match parts.next().and_then(|c| c.parse::<u16>().ok()) {
        Some(code) if (200..300).contains(&code) => ProbeOutcome::Healthy,
        Some(code) => ProbeOutcome::Unhealthy(code),
        None => ProbeOutcome::Unreachable(format!("no status code in: {first_line:?}")),
    }
}

/// Extract `(host, port, host_header)` from an `http://host:port` base URL.
/// `host_header` preserves the original `host:port` (or default-port host) for
/// the `Host:` request header.
fn parse_http_authority(base_url: &str) -> Result<(String, u16, String), String> {
    let rest = base_url
        .strip_prefix("http://")
        .ok_or_else(|| format!("only http:// URLs are supported, got {base_url:?}"))?;
    // Authority is everything up to the first '/'.
    let authority = rest.split('/').next().unwrap_or(rest);
    if authority.is_empty() {
        return Err(format!("empty authority in {base_url:?}"));
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => {
            let port: u16 = p
                .parse()
                .map_err(|_| format!("invalid port in {base_url:?}"))?;
            (h.to_string(), port)
        }
        None => (authority.to_string(), 80u16),
    };
    Ok((host, port, authority.to_string()))
}
