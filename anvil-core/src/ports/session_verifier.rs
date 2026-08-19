//! `SessionVerifier` port — the single validation seam (spec Req 2).
//!
//! All session-verification in both `anvil-mcp` and `anvil-engine` MUST go
//! through this port.  Anvil contains no inline JWT parsing, claim checking,
//! or signature logic.  The sole production implementation wraps
//! `foundry-kit-broker-client::verify_session` / `verify_session_from_env`.
//!
//! `SessionMode` is a pure decision type.  Its `decide` method is the single
//! place in the codebase that maps the helper's tri-state outcome to an
//! operating mode — grep-provable at this layer (spec Req 2 AC "Single seam").

use std::fmt;

/// A successfully-verified Foundry session, mirroring
/// `foundry_kit_broker_client::VerifiedSession` without a compile-time dep on
/// that crate at the `anvil-core` layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedSession {
    /// Foundry user id (`sub` JWT claim). Bound as the actor principal.
    pub sub: String,
    /// Foundry session id (`sid` JWT claim).
    pub sid: String,
    /// Upstream authentication provider (`google`, `microsoft`, …).
    pub provider: String,
    /// Scopes granted to this token (e.g. `["mcp:anvil-kit"]`).
    pub scopes: Vec<String>,
    /// The verified audience (`foundry-mcp:anvil-kit`).
    pub audience: String,
    /// Unix-seconds expiry timestamp.
    pub expires_at: i64,
}

/// Why session verification failed.  Variants mirror
/// `foundry_kit_broker_client::SessionVerifyError`; duplicated here to keep
/// `anvil-core` dep-free at this layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    Malformed,
    BadSignature,
    KidMismatch,
    WrongIssuer,
    WrongAudience,
    Expired,
    NotYetValid,
    KeyFetch,
    KeyDecode,
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerifyError::Malformed => write!(f, "malformed token"),
            VerifyError::BadSignature => write!(f, "signature verification failed"),
            VerifyError::KidMismatch => write!(f, "kid mismatch"),
            VerifyError::WrongIssuer => write!(f, "issuer mismatch"),
            VerifyError::WrongAudience => write!(f, "audience mismatch"),
            VerifyError::Expired => write!(f, "token expired"),
            VerifyError::NotYetValid => write!(f, "token not yet valid"),
            VerifyError::KeyFetch => write!(f, "could not fetch broker public key"),
            VerifyError::KeyDecode => write!(f, "broker public key not usable"),
        }
    }
}

/// The port that abstracts `foundry-kit-broker-client` verification.
///
/// The sole production implementation calls `verify_session` /
/// `verify_session_from_env`.  Tests supply a `StubSessionVerifier` that
/// returns controlled outcomes without a live broker.
///
/// Uses Rust 1.75+ native async fn in traits (AFIT) — no `async-trait` dep.
pub trait SessionVerifier: Send + Sync {
    /// Verify `jwt` against the kit's expected audience.
    ///
    /// Returns the same tri-state as the foundry helper:
    /// - `Ok(None)` — no token present (standalone);
    /// - `Ok(Some(session))` — present and fully verified (Foundry mode);
    /// - `Err(e)` — present but invalid (refuse).
    fn verify(
        &self,
        jwt: &str,
        expected_audience: &str,
    ) -> impl std::future::Future<Output = Result<Option<VerifiedSession>, VerifyError>> + Send;
}

/// The operating mode decided from the helper's tri-state outcome.
///
/// This enum is the single place in the codebase that maps
/// `Ok(None)` / `Ok(Some)` / `Err` to a mode.  No other code may branch on
/// session token presence — use `SessionMode::decide` instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionMode {
    /// Token absent or empty-after-trim.  Standalone operation; today's
    /// behavior is preserved unchanged.
    Standalone,
    /// Token present and cryptographically verified.  All operations are
    /// gated on `session.sub`.
    Foundry(VerifiedSession),
    /// Token present but verification failed.  The request MUST be refused
    /// with `not_authenticated`.
    Refuse,
}

impl SessionMode {
    /// Decide the operating mode.
    ///
    /// `token_present_and_trimmed` — `true` iff the raw env/metadata value
    /// was non-empty after trimming whitespace.
    ///
    /// `verify_outcome` — the result the `SessionVerifier` (or
    /// `verify_session_from_env`) returned for that token.
    ///
    /// Mapping (spec Req 1):
    /// - empty/whitespace-only token ⇒ `Standalone` (never `Refuse`, even if the
    ///   helper returns `Err` for an absent input);
    /// - `Ok(None)` ⇒ `Standalone`;
    /// - `Ok(Some(session))` ⇒ `Foundry(session)`;
    /// - `Err(_)` ⇒ `Refuse`.
    pub fn decide(
        token_present_and_trimmed: bool,
        verify_outcome: Result<Option<VerifiedSession>, VerifyError>,
    ) -> SessionMode {
        if !token_present_and_trimmed {
            // Whitespace-only or absent token: always standalone.
            return SessionMode::Standalone;
        }
        match verify_outcome {
            Ok(None) => SessionMode::Standalone,
            Ok(Some(session)) => SessionMode::Foundry(session),
            Err(_) => SessionMode::Refuse,
        }
    }
}
