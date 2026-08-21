//! Kit-side bearer resolution — the ONE place an anvil kit binary decides what
//! `authorization: Bearer <jwt>` (if any) it presents to the engine.
//!
//! # Why this module exists
//!
//! A Foundry-mode engine (`FOUNDRY_SESSION_TOKEN` present at its own startup)
//! gatekeeps every gated RPC: absence of a credential is a REFUSAL, never a
//! downgrade to standalone. That is deliberate and correct — it is the property
//! that closed the hole where a garbage bearer and no bearer returned byte-
//! identical live data.
//!
//! The consequence is that every kit-side caller needs a credential of its own.
//! The supervisor supplies one to processes it spawns (`FOUNDRY_SESSION_TOKEN`
//! in the kit process env), which is why the supervised path is healthy. A
//! caller launched OUTSIDE that env — a harness hook, an operator shell, a
//! CI step — inherits nothing and is locked out. `anvil-hooks` was exactly
//! that caller: it had no credential path at all.
//!
//! The fix is not to weaken the engine. It is for the caller to obtain a real
//! ticket the same way every other Foundry client does: ask the broker.
//!
//! # Precedence
//!
//! 1. An explicit non-blank [`FOUNDRY_SESSION_TOKEN_ENV`] in the environment —
//!    what the supervisor supplies. Forward it verbatim. This is first because
//!    it is the credential the supervisor deliberately bound to THIS process's
//!    session; re-minting over the top would silently swap the principal.
//! 2. Otherwise, if [`FOUNDRY_BROKER_SOCKET_ENV`] is set and reachable, MINT a
//!    ticket for anvil's kit audience and forward that. `acquire_kit_ticket`
//!    needs no pre-existing session id — the broker resolves the active
//!    signed-in session itself — which is precisely the empty-env case.
//! 3. Otherwise, if the broker's WELL-KNOWN default socket
//!    ([`DEFAULT_BROKER_SOCKET_RELATIVE`] under `$HOME`) exists and is a
//!    socket, mint from that. See "Why rung 3 exists" below.
//! 4. Otherwise send no bearer, and let the engine refuse. The refusal is
//!    reported by name (see [`BearerSource::Absent`]) so an operator learns
//!    which credential was missing rather than reading `not_authenticated` and
//!    guessing.
//!
//! There is deliberately NO loopback exemption, NO "local channel" bypass, and
//! NO "absent means standalone" downgrade. Any of those would reopen the hole.
//!
//! # Why rung 3 exists
//!
//! Rungs 1 and 2 both require ANOTHER program to have put something in this
//! process's environment. Harnesses snapshot their environment at session
//! start, so a variable added to a config file afterwards reaches only sessions
//! started later — which meant a fleet of already-running sessions stayed
//! locked out with no in-band way to recover. A credential path that depends on
//! someone else's environment is not a credential path; it is a hope. Rung 3
//! lets the kit find a running broker on its own.
//!
//! # Why the broker client is inline
//!
//! This module used to reach for `foundry-kit-broker-client`, a path dependency
//! on a private sibling checkout. Cargo loads the manifest of every reachable
//! path dependency — optional, switched-off ones included — so that one line
//! made anvil's whole workspace unenumerable from a bare clone
//! (`cargo metadata --no-deps` exited 101). What anvil needed from that crate
//! HERE is a client fetching its own credential over a documented,
//! newline-delimited JSON protocol: no crypto, no trust decision, fails closed.
//! Inlining it costs ~60 lines and buys a repository that builds for a stranger.
//!
//! (The VERIFIER is the opposite case and is NOT inlined — it parses JWTs and
//! checks RS256 signatures, and `anvil_core::ports::session_verifier` records
//! the invariant that anvil contains no such logic. It lives in the separate
//! `kit-build/anvil-kit-engine` package instead, where the path dep is
//! unreachable from the default workspace.)
//!
//! # Why every broker dial carries its own deadline
//!
//! The dial is one connection carrying two request/response lines: connect,
//! write `broker.handshake`, read a line, write `broker.acquire_kit_ticket`,
//! read a line. Neither read is bounded on its own. A socket that ACCEPTS the
//! connection and never writes a line therefore hangs the caller forever; one
//! that closes fails fast. Only the polite-but-silent socket is fatal, and it
//! is the realistic failure of a wedged supervisor.
//!
//! That matters most on the route hot path, whose whole turn is capped at
//! `ROUTE_TURN_TIMEOUT_MS` (90000): an unbounded dial there silently eats the
//! turn. But it matters MORE on the four
//! `anvil-hooks` verbs (`begin`, `snapshot`, `complete`, `amend`) that sit under
//! no cap at all — there, an unbounded dial has no timer anywhere in the process
//! to end it.
//!
//! So the deadline is not an optimisation bolted onto the rung; it is half of
//! it. [`KIT_BEARER_MINT_TIMEOUT_MS`] wraps the WHOLE dial (connect + handshake
//! + `acquire_kit_ticket`) on BOTH broker-dialling rungs. On expiry the dial is
//! abandoned and resolution falls through to absent — a delivered answer or a
//! named refusal, never a burnt budget.

use tonic::metadata::{Ascii, MetadataValue};
use tonic::Request;

/// anvil's kit id. The broker derives the audience `foundry-mcp:anvil-kit`
/// from it, which is what the engine's verifier expects.
pub const ANVIL_KIT_ID: &str = "anvil-kit";

// ── The three values the inlined minter shares with Foundry ─────────────────
//
// These are `pub` and named, rather than literals buried in the `json!` that
// builds the wire payload, for ONE reason: they are anvil's half of a protocol
// whose other half lives in `foundry-kit-broker-client`
// (`CLIENT_PROTOCOL_VERSION`, `audience_for_kit`, `scopes_for_kit`). Named, they
// can be COMPARED — `kit-build/anvil-kit-engine` sees both crates and asserts
// equality (`tests/minter_parity.rs`). Inline, they could only be eyeballed, and
// a divergence would surface as a mint that silently returns nothing.
//
// They must therefore stay the ONLY source of these values inside this module.
// A second `format!("foundry-mcp:{…}")` anywhere below would be a paraphrase the
// parity test cannot see, which is precisely the drift it exists to catch.

/// The broker protocol version this client speaks. Bumped in lockstep with the
/// broker's own `BROKER_PROTOCOL_VERSION`; a mismatch surfaces from the
/// handshake with the broker's `min_supported_version` named, because "upgrade
/// the kit" and "the broker is wedged" are not the same fix.
pub const BROKER_PROTOCOL_VERSION: u32 = 1;

/// The broker audience a kit's tokens are minted against: `foundry-mcp:<kit_id>`.
pub fn audience_for(kit_id: &str) -> String {
    format!("foundry-mcp:{kit_id}")
}

/// The scopes a kit's tokens carry: exactly one, `mcp:<kit_id>`.
///
/// A `Vec` rather than a `String` because the wire field is a list, and the
/// count is part of what the broker is being asked for.
pub fn scopes_for(kit_id: &str) -> Vec<String> {
    vec![format!("mcp:{kit_id}")]
}

/// The env var the Foundry supervisor injects into kit processes it spawns.
pub const FOUNDRY_SESSION_TOKEN_ENV: &str = "FOUNDRY_SESSION_TOKEN";

/// The env var naming the broker's `0600` user-owned Unix socket.
pub const FOUNDRY_BROKER_SOCKET_ENV: &str = "FOUNDRY_BROKER_SOCKET";

/// The broker's well-known socket, relative to `$HOME`. This is where Foundry
/// puts it; [`FOUNDRY_BROKER_SOCKET_ENV`] exists to POINT ELSEWHERE, not to
/// make the default unknowable.
pub const DEFAULT_BROKER_SOCKET_RELATIVE: &str = ".foundry/run/broker.sock";

/// Cap on the WHOLE broker dial — connect, handshake, and ticket — for every
/// rung that dials. See the module docs for why this is not optional.
///
/// 1000ms is chosen against the measured cost of a real mint (~0.2-0.5s), with
/// room to spare. It is bounded INDEPENDENTLY of the route cap so that neither
/// can silently consume the other — which is the property that matters, and the
/// reason this number does NOT move when the route cap moves. (It was once
/// justified by arithmetic against a 8000ms route cap; that cap is now 90000ms
/// and the arithmetic is no longer what picks this value.)
///
/// It is a latency bound, not a success threshold. Nothing asserts a number of
/// successful mints, and a broker slower than this is a broker to fix, not a
/// number to raise on a hunch — override via
/// [`KIT_BEARER_MINT_TIMEOUT_ENV`] and revise the default from measurement.
pub const KIT_BEARER_MINT_TIMEOUT_MS: u64 = 1000;

/// Override for [`KIT_BEARER_MINT_TIMEOUT_MS`].
pub const KIT_BEARER_MINT_TIMEOUT_ENV: &str = "ANVIL_KIT_BEARER_MINT_TIMEOUT_MS";

/// Effective broker-dial deadline.
///
/// A zero or unparseable override falls back to the default rather than meaning
/// "no deadline": the whole point of this module is that no credential-bearing
/// invocation is unbounded, and an env var must not be able to switch that off.
fn kit_bearer_mint_timeout_ms() -> u64 {
    std::env::var(KIT_BEARER_MINT_TIMEOUT_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(KIT_BEARER_MINT_TIMEOUT_MS)
}

/// Where the presented bearer came from — or why there isn't one.
///
/// Carried out of [`resolve_kit_bearer`] so the caller can say, in its own
/// error text, which credential path was taken. A caller that only knows
/// "the engine said not_authenticated" cannot tell an operator what to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BearerSource {
    /// Precedence 1 — forwarded from the environment the supervisor set.
    InheritedEnv,
    /// Precedence 2 — freshly minted from the broker named by
    /// [`FOUNDRY_BROKER_SOCKET_ENV`].
    BrokerMinted,
    /// Precedence 3 — freshly minted from the broker found at the well-known
    /// default path, with nothing in the environment naming it.
    ///
    /// Distinct from [`BearerSource::BrokerMinted`] on purpose: an operator
    /// debugging a principal needs to know whether this process was TOLD where
    /// the broker was or went looking for it.
    BrokerMintedDefaultPath,
    /// Precedence 4 — no credential could be obtained. The payload names the
    /// reason, and is intended to be shown to a human verbatim.
    ///
    /// The reason distinguishes THREE absences that must never collapse into
    /// one: no socket anywhere, a socket that answered with a failure, and a
    /// socket that did not answer in time. Collapsing them is what makes a slow
    /// broker unreadable as anything other than a missing one.
    Absent(String),
}

impl BearerSource {
    /// A short operator-facing description of how the credential was obtained,
    /// suitable for appending to a refusal message.
    pub fn describe(&self) -> String {
        match self {
            BearerSource::InheritedEnv => {
                format!("bearer inherited from {FOUNDRY_SESSION_TOKEN_ENV}")
            }
            BearerSource::BrokerMinted => {
                format!("bearer minted from the broker at {FOUNDRY_BROKER_SOCKET_ENV}")
            }
            BearerSource::BrokerMintedDefaultPath => format!(
                "bearer minted from the broker at the default path \
                 $HOME/{DEFAULT_BROKER_SOCKET_RELATIVE} ({FOUNDRY_BROKER_SOCKET_ENV} was unset)"
            ),
            BearerSource::Absent(reason) => format!("no bearer presented: {reason}"),
        }
    }
}

/// A resolved kit credential: the token to present, and where it came from.
#[derive(Debug, Clone)]
pub struct KitBearer {
    /// The raw JWT to present, or `None` when no credential could be obtained.
    pub token: Option<String>,
    /// Provenance, for operator-facing diagnostics.
    pub source: BearerSource,
}

impl KitBearer {
    /// Construct the absent case with a named reason.
    fn absent(reason: impl Into<String>) -> Self {
        Self {
            token: None,
            source: BearerSource::Absent(reason.into()),
        }
    }
}

/// Attach `authorization: Bearer <token>` to a tonic request's metadata.
///
/// This is the single implementation shared by every anvil kit-side caller
/// (the MCP shim and the hooks CLI). A second copy would be free to drift from
/// the header shape the engine's gatekeeper actually parses.
pub fn attach_bearer<T>(mut request: Request<T>, token: &str) -> Result<Request<T>, String> {
    let header_value = format!("Bearer {token}")
        .parse::<MetadataValue<Ascii>>()
        .map_err(|e| format!("Invalid bearer token for gRPC metadata: {e}"))?;
    request.metadata_mut().insert("authorization", header_value);
    Ok(request)
}

/// Attach the resolved bearer when there is one, leaving the request untouched
/// when there is not.
///
/// Deliberately NOT an error when the bearer is absent: the engine is the
/// gatekeeper and must be the one to refuse. A caller that short-circuited here
/// would be re-implementing the authorization decision on the client side,
/// where it can be edited out.
pub fn attach_kit_bearer<T>(request: Request<T>, bearer: &KitBearer) -> Result<Request<T>, String> {
    match bearer.token.as_deref() {
        Some(token) => attach_bearer(request, token),
        None => Ok(request),
    }
}

/// Read the environment for an explicitly supplied session token (precedence 1).
///
/// A whitespace-only value is treated as ABSENT — matching the engine's own
/// reading of a blank bearer — so a caller does not present a header the
/// gatekeeper will reject as blank anyway.
fn inherited_env_token() -> Option<String> {
    let raw = std::env::var(FOUNDRY_SESSION_TOKEN_ENV).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Resolve the credential this process should present to the engine.
///
/// See the module docs for the precedence and why each rung is where it is.
pub async fn resolve_kit_bearer() -> KitBearer {
    // Precedence 1 — the supervisor's own token.
    if let Some(token) = inherited_env_token() {
        return KitBearer {
            token: Some(token),
            source: BearerSource::InheritedEnv,
        };
    }

    // Precedence 2 and 3 — mint from the broker the environment named, or from
    // the one at the well-known default path. Both under one deadline.
    //
    // UNCONDITIONAL ON UNIX. This was once additionally gated on a
    // `foundry-session` cargo feature that only the kit build turned on, so the
    // standalone release binary (built with default features — see
    // `.github/workflows/anvil-release-binaries.yml`) silently could not mint at
    // all: it reported "no broker client compiled in" no matter how healthy the
    // broker was. The minter is ~60 lines of newline-delimited JSON with no
    // private dependency, so there is nothing left to gate it on.
    #[cfg(unix)]
    {
        match mint_from_broker().await {
            Ok((token, source)) => {
                return KitBearer {
                    token: Some(token),
                    source,
                }
            }
            Err(reason) => return KitBearer::absent(reason),
        }
    }

    // Precedence 4 — nothing to present. Name what was missing.
    //
    // The broker speaks Unix domain sockets and nothing else, so on a non-unix
    // target there is no dial to attempt — not "the feature is off", which is
    // what this used to say and is no longer true anywhere.
    #[cfg(not(unix))]
    {
        KitBearer::absent(format!(
            "{FOUNDRY_SESSION_TOKEN_ENV} is unset and this is not a Unix target, \
             so the broker — which is reachable only over a Unix domain socket — \
             cannot be dialled and no ticket can be minted"
        ))
    }
}

/// Mint a fresh kit ticket from the broker for `foundry-mcp:anvil-kit`.
///
/// Deliberately NOT a token-holder / expiry-aware-refresh abstraction. Every
/// caller here constructs its credential fresh, per invocation, so a cache
/// would be empty on every lookup and the "re-mint before expiry" rule would
/// never fire once. It would be pure ceremony over a single `acquire`.
///
/// Returns the JWT, or a NAMED reason the mint failed. The reason is surfaced
/// to the operator — "broker socket missing" and "no signed-in session" demand
/// completely different fixes and must never collapse into one opaque failure.
#[cfg(unix)]
async fn mint_from_broker() -> Result<(String, BearerSource), String> {
    let (socket, source) = match choose_broker_dial() {
        Some(pair) => pair,
        // Neither rung has anywhere to dial. Name BOTH variables and the
        // default path that was tried, because an operator who is told only
        // that "the broker is unreachable" has nothing to act on.
        None => {
            return Err(format!(
                "{FOUNDRY_SESSION_TOKEN_ENV} is unset, {FOUNDRY_BROKER_SOCKET_ENV} \
                 is unset, and no broker socket exists at the default path \
                 $HOME/{DEFAULT_BROKER_SOCKET_RELATIVE}; set \
                 {FOUNDRY_BROKER_SOCKET_ENV} to the running Foundry broker \
                 socket, or run against a standalone engine"
            ))
        }
    };

    let budget = std::time::Duration::from_millis(kit_bearer_mint_timeout_ms());

    // THE DEADLINE. It wraps the WHOLE dial — connect, handshake, and ticket —
    // because none of the three is bounded on its own and a socket that accepts
    // and then says nothing would otherwise hang forever. Dropping the future
    // on expiry cancels the dial.
    match tokio::time::timeout(budget, acquire_kit_ticket(&socket)).await {
        Ok(Ok(token)) => Ok((token, source)),
        Ok(Err(e)) => Err(format!(
            "{FOUNDRY_SESSION_TOKEN_ENV} is unset and minting a ticket for \
             `{}` from the broker at {} failed ({e}); \
             sign in to Foundry, or run against a standalone engine",
            audience_for(ANVIL_KIT_ID),
            socket.display()
        )),
        // Expiry is ABSENCE, named. Distinct from "no socket" so a slow broker
        // is never read as a missing one.
        Err(_) => Err(format!(
            "{FOUNDRY_SESSION_TOKEN_ENV} is unset and the broker at {} \
             did not answer within {}ms, so no ticket was minted; the socket is \
             present but the broker is not answering",
            socket.display(),
            budget.as_millis()
        )),
    }
}

/// Which socket to dial, and what provenance a ticket from it carries.
///
/// Rung 2 is the env-directed socket: its mere PRESENCE routes the connection,
/// matching `KitBrokerClient::from_env`, so an operator who points the variable
/// at a wrong path gets an error about that path rather than a silent fallback
/// to a different broker than the one they named.
///
/// Rung 3 is the default path, tried only when the variable is unset.
#[cfg(unix)]
fn choose_broker_dial() -> Option<(std::path::PathBuf, BearerSource)> {
    if let Some(raw) = std::env::var_os(FOUNDRY_BROKER_SOCKET_ENV) {
        if !raw.is_empty() {
            return Some((std::path::PathBuf::from(raw), BearerSource::BrokerMinted));
        }
    }
    default_broker_socket().map(|p| (p, BearerSource::BrokerMintedDefaultPath))
}

/// `$HOME/.foundry/run/broker.sock`, but ONLY when it exists AND is a socket.
///
/// The is-a-socket check is not pedantry: a leftover regular file at that path
/// would otherwise turn every dial into a connect error charged against the
/// deadline, on every invocation, for as long as the file sat there.
#[cfg(unix)]
fn default_broker_socket() -> Option<std::path::PathBuf> {
    use std::os::unix::fs::FileTypeExt;
    let home = std::env::var_os("HOME")?;
    if home.is_empty() {
        return None;
    }
    let path = std::path::PathBuf::from(home).join(DEFAULT_BROKER_SOCKET_RELATIVE);
    let meta = std::fs::metadata(&path).ok()?;
    meta.file_type().is_socket().then_some(path)
}

// ── The inline broker client ────────────────────────────────────────────────
//
// The broker's kit protocol, verified live against a running broker: one
// connection per exchange, newline-delimited JSON, handshake first.
//
//   -> {"version":1,"request_id":"…","method":"broker.handshake",
//       "params":{"client_version":1}}
//   <- {"version":1,"request_id":"…","result":{"server_version":1,
//       "min_supported_version":1,…}}
//   -> {"version":1,"request_id":"…","method":"broker.acquire_kit_ticket",
//       "params":{"audience":"foundry-mcp:anvil-kit","scopes":["mcp:anvil-kit"]}}
//   <- {"version":1,"request_id":"…","result":{"jwt":"…","expires_at":"…"}}
//
// A failure comes back as `{"error":{"code":…,"message":…,"details":{…}}}`.
//
// `acquire_kit_ticket` needs no pre-existing session id: the broker resolves
// the active signed-in session itself over its `0600` user-owned socket. That
// is exactly the empty-env case rung 2 and rung 3 exist for.

/// A random hex request id. The broker only echoes it, so uniqueness within a
/// connection is all that is required — no `uuid` dependency is warranted for
/// a value nothing ever reads back.
#[cfg(unix)]
fn request_id() -> String {
    use rand::Rng;
    let bytes: [u8; 16] = rand::thread_rng().gen();
    bytes.iter().fold(String::with_capacity(32), |mut acc, b| {
        use std::fmt::Write;
        let _ = write!(acc, "{b:02x}");
        acc
    })
}

/// Connect, handshake, and acquire a kit ticket. Returns the raw JWT.
///
/// Carries NO deadline of its own — the caller owns the one deadline (see
/// [`mint_from_broker`]), so there is exactly one number to reason about
/// instead of an inner bound that can silently outlive the outer one.
///
/// The error strings mirror the shapes the previous broker-client dependency
/// produced, so operator-facing text and any log-grepping survive the inlining.
#[cfg(unix)]
async fn acquire_kit_ticket(socket: &std::path::Path) -> Result<String, String> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    let stream = UnixStream::connect(socket)
        .await
        .map_err(|e| format!("broker socket missing or unreachable: connect: {e}"))?;
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    async fn write_line<W: tokio::io::AsyncWrite + Unpin>(
        w: &mut W,
        value: &serde_json::Value,
    ) -> Result<(), String> {
        let mut buf = serde_json::to_vec(value).map_err(|e| format!("decode: {e}"))?;
        buf.push(b'\n');
        w.write_all(&buf).await.map_err(|e| format!("io: {e}"))?;
        w.flush().await.map_err(|e| format!("io: {e}"))
    }

    async fn read_line<R: tokio::io::AsyncBufRead + Unpin>(
        r: &mut R,
    ) -> Result<serde_json::Value, String> {
        let mut line = String::new();
        let n = r
            .read_line(&mut line)
            .await
            .map_err(|e| format!("io: {e}"))?;
        if n == 0 {
            return Err("io: broker closed connection".to_string());
        }
        serde_json::from_str(line.trim_end()).map_err(|e| format!("decode: {e}"))
    }

    fn error_envelope(response: &serde_json::Value) -> Option<&serde_json::Value> {
        response.get("error")
    }

    // 1. Handshake. The broker rejects any other first envelope.
    write_line(
        &mut write_half,
        &serde_json::json!({
            "version": BROKER_PROTOCOL_VERSION,
            "request_id": request_id(),
            "method": "broker.handshake",
            "params": { "client_version": BROKER_PROTOCOL_VERSION },
        }),
    )
    .await?;
    let handshake = read_line(&mut reader).await?;
    if let Some(err) = error_envelope(&handshake) {
        let code = err.get("code").and_then(|v| v.as_str()).unwrap_or("");
        // A version mismatch is reported TYPED, naming the version the broker
        // requires. "protocol mismatch" with no number tells an operator only
        // that something is wrong, not which side to move.
        if code == "protocol_version_mismatch" {
            let min = err
                .get("details")
                .and_then(|d| d.get("min_supported_version"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            return Err(format!(
                "protocol mismatch: client speaks {BROKER_PROTOCOL_VERSION}, \
                 broker requires >= {min}"
            ));
        }
        return Err(format!(
            "broker error [{code}]: {}",
            err.get("message").and_then(|v| v.as_str()).unwrap_or("")
        ));
    }

    // 2. The ticket.
    write_line(
        &mut write_half,
        &serde_json::json!({
            "version": BROKER_PROTOCOL_VERSION,
            "request_id": request_id(),
            "method": "broker.acquire_kit_ticket",
            "params": {
                "audience": audience_for(ANVIL_KIT_ID),
                "scopes": scopes_for(ANVIL_KIT_ID),
            },
        }),
    )
    .await?;
    let response = read_line(&mut reader).await?;
    if let Some(err) = error_envelope(&response) {
        return Err(format!(
            "broker error [{}]: {}",
            err.get("code")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown"),
            err.get("message").and_then(|v| v.as_str()).unwrap_or("")
        ));
    }

    response
        .get("result")
        .and_then(|r| r.get("jwt"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        // A `result` with no `jwt` is a broker that answered without answering.
        // Naming it is the difference between a five-minute fix and a hunt.
        .ok_or_else(|| "decode: broker result carried no `jwt` field".to_string())
}
