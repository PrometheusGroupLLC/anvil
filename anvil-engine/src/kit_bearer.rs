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
//! 3. Otherwise send no bearer, and let the engine refuse. The refusal is
//!    reported by name (see [`BearerSource::Absent`]) so an operator learns
//!    which credential was missing rather than reading `not_authenticated` and
//!    guessing.
//!
//! There is deliberately NO loopback exemption, NO "local channel" bypass, and
//! NO "absent means standalone" downgrade. Any of those would reopen the hole.

use tonic::metadata::{Ascii, MetadataValue};
use tonic::Request;

/// anvil's kit id. The broker derives the audience `foundry-mcp:anvil-kit`
/// from it, which is what the engine's verifier expects.
pub const ANVIL_KIT_ID: &str = "anvil-kit";

/// The env var the Foundry supervisor injects into kit processes it spawns.
pub const FOUNDRY_SESSION_TOKEN_ENV: &str = "FOUNDRY_SESSION_TOKEN";

/// The env var naming the broker's `0600` user-owned Unix socket.
pub const FOUNDRY_BROKER_SOCKET_ENV: &str = "FOUNDRY_BROKER_SOCKET";

/// Where the presented bearer came from — or why there isn't one.
///
/// Carried out of [`resolve_kit_bearer`] so the caller can say, in its own
/// error text, which credential path was taken. A caller that only knows
/// "the engine said not_authenticated" cannot tell an operator what to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BearerSource {
    /// Precedence 1 — forwarded from the environment the supervisor set.
    InheritedEnv,
    /// Precedence 2 — freshly minted from the broker for `foundry-mcp:anvil-kit`.
    BrokerMinted,
    /// Precedence 3 — no credential could be obtained. The payload names the
    /// reason, and is intended to be shown to a human verbatim.
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

    // Precedence 2 — mint from the broker.
    #[cfg(all(unix, feature = "foundry-session"))]
    {
        match mint_from_broker().await {
            Ok(token) => {
                return KitBearer {
                    token: Some(token),
                    source: BearerSource::BrokerMinted,
                }
            }
            Err(reason) => return KitBearer::absent(reason),
        }
    }

    // Precedence 3 — nothing to present. Name what was missing.
    #[cfg(not(all(unix, feature = "foundry-session")))]
    {
        KitBearer::absent(format!(
            "{FOUNDRY_SESSION_TOKEN_ENV} is unset and this build has no broker \
             client compiled in (the `foundry-session` feature is off), so no \
             ticket can be minted"
        ))
    }
}

/// Mint a fresh kit ticket from the broker for `foundry-mcp:anvil-kit`.
///
/// Uses [`KitTokenHolder`] rather than calling `acquire_kit_ticket` raw so the
/// expiry-aware refresh rule lives in exactly one place. In a one-shot CLI the
/// holder mints once and the freshness check is trivially true; the value is
/// that a long-lived caller adopting this helper inherits re-minting for free
/// instead of re-deriving it.
///
/// Returns the JWT, or a NAMED reason the mint failed. The reason is surfaced
/// to the operator — "broker socket missing" and "no signed-in session" demand
/// completely different fixes and must never collapse into one opaque failure.
#[cfg(all(unix, feature = "foundry-session"))]
async fn mint_from_broker() -> Result<String, String> {
    use foundry_kit_broker_client::{token_holder::KitTokenHolder, KitBrokerClient};

    // `from_env` reads FOUNDRY_BROKER_SOCKET. Absence is the ordinary
    // outside-Foundry case, so say so plainly rather than as an error code.
    let client = KitBrokerClient::from_env(ANVIL_KIT_ID).map_err(|e| {
        format!(
            "{FOUNDRY_SESSION_TOKEN_ENV} is unset and the broker is unreachable \
             ({e}); set {FOUNDRY_BROKER_SOCKET_ENV} to the running Foundry \
             broker socket, or run against a standalone engine"
        )
    })?;

    let holder = KitTokenHolder::with_default_skew(client);
    holder.current_bearer().await.map_err(|e| {
        format!(
            "{FOUNDRY_SESSION_TOKEN_ENV} is unset and minting a ticket for \
             `foundry-mcp:{ANVIL_KIT_ID}` from the broker failed ({e}); sign in \
             to Foundry, or run against a standalone engine"
        )
    })
}
