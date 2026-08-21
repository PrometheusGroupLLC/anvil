//! Session verification infrastructure for anvil-engine.
//!
//! Provides:
//! - `VerificationCache` — per-token cache keyed by raw token string,
//!   evicted at `expires_at`.  Satisfies spec Req 6: at most one broker
//!   round-trip per token per TTL window.
//! - `DynSessionVerifier` + `EngineVerifier::External` — the injection point
//!   for the production verifier, which is deliberately NOT in this crate.
//!
//! ## Where the production verifier went
//!
//! `BrokerSessionVerifier` used to live here, behind a `foundry-session`
//! feature that pulled in `foundry-kit-broker-client` by path. Cargo loads the
//! manifest of every reachable path dependency — optional and switched-off
//! ones included — so that dependency made anvil's workspace unenumerable from
//! a bare clone. It could not simply be inlined either: it parses JWTs and
//! verifies RS256 signatures, and `anvil_core::ports::session_verifier` records
//! the invariant that *anvil contains no inline JWT parsing, claim checking, or
//! signature logic*.
//!
//! So it MOVED rather than being copied: it now lives in the
//! `kit-build/anvil-kit-engine` package, in a second workspace the default one
//! cannot reach, and is injected here at startup through `DynSessionVerifier`.
//! This crate holds the SEAM and never the broker client.
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
use std::sync::{Arc, Mutex};
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

// ── DynSessionVerifier — the out-of-crate injection point ───────────────────

/// Dyn-compatible mirror of [`SessionVerifier`].
///
/// `SessionVerifier` uses AFIT (`async fn` in trait) and so has no vtable. This
/// trait boxes the future so a verifier implemented OUTSIDE this crate can be
/// injected at startup. That is what lets the Foundry-coupled verifier live in a
/// package the default workspace cannot reach: `anvil-engine` holds the seam,
/// never the broker client.
/// Seals [`DynSessionVerifier`]. NOT nameable outside this crate, so no foreign
/// type can satisfy the supertrait, so no foreign type can implement
/// `DynSessionVerifier` except through the blanket impl below.
///
/// The seal is load-bearing and was added after MEASURING that the blanket impl
/// alone does not deliver the property. A direct downstream
/// `impl DynSessionVerifier for RogueVerifier` — one that returns
/// `Ok(None)` without verifying anything — COMPILED CLEANLY against the blanket
/// impl, because rustc's overlap check is satisfied once it can see that
/// `RogueVerifier` does not implement `SessionVerifier`. "There is a blanket
/// impl" is therefore not the same claim as "the port is the only way in", and
/// the difference is a second, silent verification seam.
mod sealed {
    pub trait Sealed {}
    impl<T: super::SessionVerifier + ?Sized> Sealed for T {}
}

pub trait DynSessionVerifier: sealed::Sealed + Send + Sync {
    fn verify_dyn<'a>(
        &'a self,
        jwt: &'a str,
        expected_audience: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Option<VerifiedSession>, VerifyError>>
                + Send
                + 'a,
        >,
    >;
}

/// THE ONLY WAY TO BE A `DynSessionVerifier` IS TO BE A `SessionVerifier`.
///
/// Blanket, and deliberately the only impl anywhere. Written by hand in the
/// implementing crate instead, the "everything still goes through the port"
/// property would hold only by CUSTOM: nothing would stop a future verifier
/// implementing `verify_dyn` with its own signature checking and never touching
/// `SessionVerifier` at all — a second verification seam, indistinguishable
/// from the first at the call site, and the exact shape of bug the port
/// abstraction exists to prevent.
///
/// With this impl, `DynSessionVerifier` is not implementable directly: any type
/// that satisfies it does so BY satisfying the port. One seam becomes a fact
/// about the types rather than a claim in a comment.
///
/// This compiles because `SessionVerifier` already requires `Send + Sync` and
/// already declares its returned future `Send`, so there is nothing left to
/// prove about the boxed future.
impl<T: SessionVerifier + ?Sized> DynSessionVerifier for T {
    fn verify_dyn<'a>(
        &'a self,
        jwt: &'a str,
        expected_audience: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Option<VerifiedSession>, VerifyError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(self.verify(jwt, expected_audience))
    }
}

// ── StandaloneVerifier ──────────────────────────────────────────────────────
//
// A zero-dep concrete verifier that always reports "no session" (`Ok(None)`).
// Used as the `EngineVerifier::Standalone` arm, and as the fallback whenever no
// external verifier has been injected — which is every build of this crate's
// own `anvil-engine` bin.  Keeps `AnvilServer` non-generic and dep-free in
// standalone builds.

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
    /// A verifier supplied from OUTSIDE this crate — in production, the
    /// Foundry broker verifier that `kit-build/anvil-kit-engine` injects.
    ///
    /// Ungated on purpose. The arm is a plain injection point with no private
    /// dependency behind it, so there is nothing to conditionally compile; what
    /// varies is only whether any caller has something to put in it.
    External(Arc<dyn DynSessionVerifier>),
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
            EngineVerifier::External(v) => v.verify_dyn(jwt, expected_audience).await,
            #[cfg(debug_assertions)]
            EngineVerifier::Stub(v) => v.verify(jwt, expected_audience).await,
        }
    }
}
