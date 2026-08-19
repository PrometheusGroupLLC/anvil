//! Authoritative HTTP health probe.
//!
//! The probe is a deliberately tiny, dependency-free, blocking HTTP/1.1 `GET`
//! over [`std::net::TcpStream`] with hard connect/read/write timeouts. It does
//! **not** pull `reqwest`/`hyper`/`tokio` — keeping this crate linkable into
//! thin MCP/CLI clients. We only need to know "did the engine answer `2xx` at
//! `<url><health_path>` within the budget"; we do not parse the body.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

/// Result of a single health probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// Server answered with an HTTP 2xx status (and, when an [`IdentityCheck`]
    /// was requested, the body identity matched — or the field was absent).
    Healthy,
    /// Server answered, but not with 2xx (e.g. 404/500). The status code is
    /// included for logging.
    Unhealthy(u16),
    /// The server answered 2xx but its health body reported a DIFFERENT
    /// identity than the rendezvous record claimed — i.e. a different engine
    /// reused the port. Carries `(field, expected, got)` for logging. Treated
    /// as "do not trust this rendezvous" (the resolver falls through).
    IdentityMismatch {
        field: String,
        expected: String,
        got: String,
    },
    /// Could not connect / timed out / malformed response. Carries a short
    /// reason for logging.
    Unreachable(String),
}

/// An opt-in body-identity guard for [`probe_health`]: after a 2xx, the health
/// response body (JSON) must report `field == expected`, else the probe is an
/// [`ProbeOutcome::IdentityMismatch`]. Mirrors lore's db_path guard against
/// port reuse — a stale rendezvous pointing at a port a DIFFERENT engine now
/// owns is rejected. An ABSENT field is accepted (the engine simply does not
/// report its identity), matching lore's lenient `unwrap_or(true)`.
#[derive(Debug, Clone)]
pub struct IdentityCheck {
    /// JSON field in the health body to compare, e.g. `"db_path"`.
    pub field: String,
    /// Value the rendezvous record claims, e.g. the published `db_path`.
    pub expected: String,
}

impl ProbeOutcome {
    /// `true` only for [`ProbeOutcome::Healthy`].
    pub fn is_healthy(&self) -> bool {
        matches!(self, ProbeOutcome::Healthy)
    }
}

/// Probe `<base_url><health_path>` with the given total timeout budget.
///
/// `base_url` is the record's `url` (e.g. `http://127.0.0.1:14312`).
/// `health_path` is e.g. `/health`. Only `http://` URLs are supported in T0;
/// anything else returns [`ProbeOutcome::Unreachable`].
///
/// The same `timeout` bounds connect, write, and read independently — the call
/// returns well within a small multiple of `timeout` and **never hangs**.
pub fn probe_health(
    base_url: &str,
    health_path: &str,
    timeout: Duration,
    identity: Option<&IdentityCheck>,
) -> ProbeOutcome {
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

    // When no identity guard is requested we only need the status line, so we
    // stop at the first CRLF (fast path). With an identity guard we must read
    // the whole (small) health body, so we read until the server closes the
    // connection (`Connection: close`) or we hit the 64KiB cap.
    let want_body = identity.is_some();
    let mut buf = Vec::with_capacity(512);
    let mut chunk = [0u8; 512];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if want_body {
                    if buf.len() >= 65536 {
                        break;
                    }
                } else if buf.windows(2).any(|w| w == b"\r\n") || buf.len() >= 4096 {
                    break;
                }
            }
            Err(e) => return ProbeOutcome::Unreachable(format!("read response: {e}")),
        }
    }

    match parse_status(&buf) {
        // 2xx + a guard requested → the body identity must match (or be absent).
        ProbeOutcome::Healthy => match identity {
            Some(check) => evaluate_identity(&buf, check),
            None => ProbeOutcome::Healthy,
        },
        other => other,
    }
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

/// After a 2xx, enforce the body identity: parse the HTTP body (everything past
/// the `\r\n\r\n` header/body separator) as JSON and compare `check.field`. A
/// matching value → Healthy; a different value → IdentityMismatch; an ABSENT
/// field (or unparseable body) → Healthy (lenient, like lore's `unwrap_or(true)`
/// — the engine simply does not advertise its identity, so we trust the 2xx).
fn evaluate_identity(buf: &[u8], check: &IdentityCheck) -> ProbeOutcome {
    let text = String::from_utf8_lossy(buf);
    let body = match text.split_once("\r\n\r\n") {
        Some((_headers, body)) => body,
        None => return ProbeOutcome::Healthy,
    };
    let json: serde_json::Value = match serde_json::from_str(body.trim()) {
        Ok(v) => v,
        Err(_) => return ProbeOutcome::Healthy,
    };
    match json.get(&check.field).and_then(|v| v.as_str()) {
        Some(got) if got == check.expected => ProbeOutcome::Healthy,
        Some(got) => ProbeOutcome::IdentityMismatch {
            field: check.field.clone(),
            expected: check.expected.clone(),
            got: got.to_string(),
        },
        None => ProbeOutcome::Healthy,
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
