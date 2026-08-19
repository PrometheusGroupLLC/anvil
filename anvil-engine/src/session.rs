//! Session verification infrastructure for anvil-engine.
//!
//! Provides:
//! - `VerificationCache` — per-token cache keyed by raw token string,
//!   evicted at `expires_at`.  Satisfies spec Req 6: at most one broker
//!   round-trip per token per TTL window.
//! - `BrokerSessionVerifier` (Unix + `foundry-session` feature only) —
//!   the sole production `SessionVerifier` implementation; delegates to
//!   `foundry_kit_broker_client::verify_session_from_env`.
//!
//! ## Revocation-latency bound (spec Req 6 / G1)
//!
//! The cache retains a verified session until `expires_at` (Unix seconds).
//! Tokens are issued with a 1-hour TTL by the Foundry broker, so the
//! maximum latency between revocation at the broker and enforcement here is
//! ≤ 1 hour (the remaining TTL at the time of revocation).  Operators who
//! require shorter latency should configure shorter token TTLs on the broker
//! side; there is no separate anvil-side refresh interval.
//!
//! ## Lock discipline (spec D3, P2 — CRITICAL)
//!
//! `VerificationCache` acquires its `Mutex` guard ONLY for the hash-map
//! read and write operations.  The guard is DROPPED before `verify().await`
//! is called and re-acquired for the insert.  This ensures the lock is never
//! held across an `.await` point, preventing deadlocks and back-pressure on
//! callers while the broker round-trip is in flight.

use anvil_core::ports::session_verifier::{SessionVerifier, VerifiedSession, VerifyError};
use std::collections::HashMap;
use std::sync::Mutex;
// `Arc` is only used by the feature-gated `EngineVerifier::Broker` arm.
#[cfg(all(unix, feature = "foundry-session"))]
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ── VerificationCache ──────────────────────────────────────────────────────

/// Per-token verification cache.
///
/// Keyed by the raw token string.  An entry is considered valid until
/// `session.expires_at > now_secs()`.  Expired entries are evicted on read
/// (lazy eviction — the map never grows unboundedly as long as tokens cycle).
///
/// See module-level doc for the revocation-latency bound.
///
/// Generic over `V: SessionVerifier` because the AFIT-based `SessionVerifier`
/// trait is not dyn-compatible (Rust does not yet support vtable dispatch for
/// `impl Future` return types).  Use `VerificationCache<Arc<YourVerifier>>`
/// or wrap in a newtype if you need type erasure.
pub struct VerificationCache<V: SessionVerifier> {
    /// Inner: raw token → VerifiedSession.
    ///
    /// Lock discipline (P2 — CRITICAL): the guard must NEVER be held across
    /// an `.await` point.  Acquire for lookup, drop, then call
    /// `verifier.verify().await`, re-acquire to insert.  See `lookup`.
    inner: Mutex<HashMap<String, VerifiedSession>>,
    verifier: V,
}

impl<V: SessionVerifier> VerificationCache<V> {
    /// Construct a new empty cache backed by `verifier`.
    pub fn new(verifier: V) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            verifier,
        }
    }

    /// Look up `token` in the cache, calling the verifier on a miss.
    ///
    /// Returns:
    /// - `Ok(Some(session))` — valid session (from cache or fresh verify);
    /// - `Ok(None)` — verifier returned `Ok(None)` (standalone, no token);
    /// - `Err(e)` — verifier returned an error; error propagated (fail-closed,
    ///   spec Req 6 / P3: broker-unreachable on miss ⇒ refuse, not bypass).
    ///
    /// Lock discipline (P2 — CRITICAL): guard acquired for lookup, DROPPED
    /// before `.await`, re-acquired for insert.  The lock is never held
    /// across the `verify().await` call.
    pub async fn lookup(
        &self,
        token: &str,
        expected_audience: &str,
    ) -> Result<Option<VerifiedSession>, VerifyError> {
        let now = now_secs();

        // --- ACQUIRE guard for cache read, then DROP before await ---
        // Poison-resilient: a panic elsewhere while this lock was held must not
        // wedge every subsequent auth check. The cache is a plain token->session
        // map (no cross-entry invariant), so recovering a poisoned guard is safe.
        let cached = {
            let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            guard.get(token).filter(|s| s.expires_at > now).cloned()
            // guard dropped here
        };

        if let Some(session) = cached {
            return Ok(Some(session));
        }

        // Cache miss (or expired entry) — call the verifier.
        // Guard is NOT held here.
        let result = self.verifier.verify(token, expected_audience).await;

        // On success, insert into the cache.
        if let Ok(Some(ref session)) = result {
            // --- ACQUIRE guard for insert, then DROP ---
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            // Only insert if not yet expired (another task may have beaten us).
            if session.expires_at > now {
                guard.insert(token.to_string(), session.clone());
            }
            // guard dropped here
        }

        result
    }
}

// ── BrokerSessionVerifier ──────────────────────────────────────────────────
//
// Unix-only, behind the `foundry-session` feature flag.  The sole production
// `SessionVerifier`; delegates to `KitBrokerClient::verify_session`, honoring
// the FORWARDED bearer token from the gRPC request metadata (J5).  The broker
// client is constructed from the env (`FOUNDRY_BROKER_SOCKET`); the JWT and
// expected audience are the values passed by `authorize()`, NOT re-read from
// `FOUNDRY_SESSION_TOKEN`.

#[cfg(all(unix, feature = "foundry-session"))]
pub struct BrokerSessionVerifier {
    /// The kit id for this deployment (e.g. `"anvil-kit"`).
    kit_id: String,
}

#[cfg(all(unix, feature = "foundry-session"))]
impl BrokerSessionVerifier {
    /// Construct a new `BrokerSessionVerifier` for the given kit.
    pub fn new(kit_id: impl Into<String>) -> Self {
        Self {
            kit_id: kit_id.into(),
        }
    }
}

#[cfg(all(unix, feature = "foundry-session"))]
impl SessionVerifier for BrokerSessionVerifier {
    async fn verify(
        &self,
        jwt: &str,
        expected_audience: &str,
    ) -> Result<Option<VerifiedSession>, VerifyError> {
        // Honor the PASSED token (the forwarded bearer from gRPC metadata),
        // not `FOUNDRY_SESSION_TOKEN` (J5).  `from_env` builds the broker
        // client from `FOUNDRY_BROKER_SOCKET`; `verify_session` then verifies
        // exactly the JWT and audience `authorize()` handed us.  A failure to
        // construct the client (e.g. broker socket unreachable) surfaces as a
        // refuse-able error (fail-closed, spec Req 6) — never a bypass.
        use foundry_kit_broker_client::KitBrokerClient;

        let client = KitBrokerClient::from_env(self.kit_id.clone()).map_err(map_broker_error)?;
        // `verify_session` returns the verified session directly (not an
        // Option); any verification failure maps to a refuse-able `VerifyError`.
        match client.verify_session(jwt, expected_audience).await {
            Ok(fvs) => Ok(Some(map_verified_session(fvs))),
            Err(e) => Err(map_verify_error(e)),
        }
    }
}

/// Map `foundry_kit_broker_client::KitBrokerError` (client-construction /
/// transport failures) to a refuse-able `VerifyError`.  Every variant is a
/// broker-reachability failure ⇒ `KeyFetch` ⇒ refuse (fail-closed, Req 6).
#[cfg(all(unix, feature = "foundry-session"))]
fn map_broker_error(_e: foundry_kit_broker_client::KitBrokerError) -> VerifyError {
    VerifyError::KeyFetch
}

// ── StandaloneVerifier ──────────────────────────────────────────────────────
//
// A zero-dep concrete verifier that always reports "no session" (`Ok(None)`).
// Used as the `EngineVerifier::Standalone` arm and on the feature-off /
// non-unix build, where the broker dep is compiled out.  Keeps `AnvilServer`
// non-generic and dep-free in standalone builds.

#[derive(Clone)]
pub struct StandaloneVerifier;

impl SessionVerifier for StandaloneVerifier {
    async fn verify(
        &self,
        _jwt: &str,
        _expected_audience: &str,
    ) -> Result<Option<VerifiedSession>, VerifyError> {
        Ok(None)
    }
}

// ── StubSessionVerifier (debug builds only — MUST NOT ship in release) ───────
//
// A hermetic test double selected at engine construction via the test-only env
// switch `ANVIL_TEST_SESSION_VERIFIER` (D4).  Gated behind
// `#[cfg(debug_assertions)]` so it is absent from `--release` builds (P6).
// It lets the R1 reject-path features drive the REAL spawned engine binary
// without a live broker: `Reject` refuses every token, `Unreachable` simulates
// a broker-unreachable cache miss (`KeyFetch`).  `Accept` is the R2 accept-path
// arm (principal binding); included here so the enum is complete.

#[cfg(debug_assertions)]
#[derive(Clone)]
pub enum StubSessionVerifier {
    /// Reject every presented token with `Malformed` (covers no/malformed/
    /// wrong-aud/expired/bad-sig uniformly — all map to refuse, D4).
    Reject,
    /// Simulate the broker being unreachable on every lookup (`KeyFetch`) —
    /// drives the fail-closed path (spec Req 6).
    Unreachable,
    /// Accept every presented token, returning a canned `VerifiedSession`
    /// whose `sub` is the carried value (D4 accept path).  Drives the R2
    /// principal-binding scenarios.
    Accept { sub: String },
}

#[cfg(debug_assertions)]
impl SessionVerifier for StubSessionVerifier {
    async fn verify(
        &self,
        _jwt: &str,
        expected_audience: &str,
    ) -> Result<Option<VerifiedSession>, VerifyError> {
        match self {
            StubSessionVerifier::Reject => Err(VerifyError::Malformed),
            StubSessionVerifier::Unreachable => Err(VerifyError::KeyFetch),
            StubSessionVerifier::Accept { sub } => Ok(Some(VerifiedSession {
                sub: sub.clone(),
                sid: "stub-sid".to_string(),
                provider: "stub".to_string(),
                scopes: vec![],
                audience: expected_audience.to_string(),
                expires_at: i64::MAX,
            })),
        }
    }
}

// ── EngineVerifier — concrete enum-dispatch (J4) ────────────────────────────
//
// `AnvilServer` holds `VerificationCache<EngineVerifier>`, a CONCRETE
// monomorphized type — NOT a type parameter.  This keeps `AnvilServer` and the
// `impl AnvilService for AnvilServer` handler surface non-generic (J4): there
// is exactly one production verifier, so a generic `AnvilServer<V>` would be
// pure infection for zero benefit.  The `SessionVerifier` trait uses AFIT and
// is not dyn-compatible, so we erase the verifier choice with a hand-written
// enum + one forwarding `impl`, rather than `Arc<dyn>`.

#[derive(Clone)]
pub enum EngineVerifier {
    /// Standalone mode: no session, verify => Ok(None).
    Standalone,
    /// Production Foundry verifier (Unix + feature only).
    #[cfg(all(unix, feature = "foundry-session"))]
    Broker(Arc<BrokerSessionVerifier>),
    /// Hermetic test double (debug builds only; never ships in release).
    #[cfg(debug_assertions)]
    Stub(StubSessionVerifier),
}

impl SessionVerifier for EngineVerifier {
    async fn verify(
        &self,
        jwt: &str,
        expected_audience: &str,
    ) -> Result<Option<VerifiedSession>, VerifyError> {
        match self {
            EngineVerifier::Standalone => Ok(None),
            #[cfg(all(unix, feature = "foundry-session"))]
            EngineVerifier::Broker(v) => v.verify(jwt, expected_audience).await,
            #[cfg(debug_assertions)]
            EngineVerifier::Stub(v) => v.verify(jwt, expected_audience).await,
        }
    }
}

/// Map the foundry crate's `VerifiedSession` to the anvil-core domain mirror.
#[cfg(all(unix, feature = "foundry-session"))]
fn map_verified_session(fvs: foundry_kit_broker_client::VerifiedSession) -> VerifiedSession {
    VerifiedSession {
        sub: fvs.sub,
        sid: fvs.sid,
        provider: fvs.provider,
        scopes: fvs.scopes,
        audience: fvs.audience,
        expires_at: fvs.expires_at,
    }
}

/// Map `foundry_kit_broker_client::SessionVerifyError` to `anvil_core::ports::session_verifier::VerifyError`.
#[cfg(all(unix, feature = "foundry-session"))]
fn map_verify_error(e: foundry_kit_broker_client::SessionVerifyError) -> VerifyError {
    use foundry_kit_broker_client::SessionVerifyError;
    match e {
        SessionVerifyError::Malformed(_) => VerifyError::Malformed,
        SessionVerifyError::BadSignature => VerifyError::BadSignature,
        SessionVerifyError::KidMismatch { .. } => VerifyError::KidMismatch,
        SessionVerifyError::WrongIssuer { .. } => VerifyError::WrongIssuer,
        SessionVerifyError::WrongAudience { .. } => VerifyError::WrongAudience,
        SessionVerifyError::Expired { .. } => VerifyError::Expired,
        SessionVerifyError::NotYetValid { .. } => VerifyError::NotYetValid,
        SessionVerifyError::KeyFetch(_) => VerifyError::KeyFetch,
        SessionVerifyError::KeyDecode(_) => VerifyError::KeyDecode,
    }
}
