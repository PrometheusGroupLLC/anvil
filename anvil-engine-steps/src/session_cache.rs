//! Step definitions for the session verification cache scenarios.
//!
//! Uses `StubSessionVerifier` (an invocation-counting test double) to drive
//! `VerificationCache` through the `SessionVerifier` port without a live broker.

use anvil_core::ports::session_verifier::{SessionVerifier, VerifiedSession, VerifyError};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{async_step_def, check_def, StepDef};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

// Import the cache from anvil-engine.  This module is only compiled when
// included by the anvil-engine brine runner (which has anvil-engine as a dep).
use anvil_engine::session::VerificationCache;

/// Type alias for the concrete cache type used in tests (avoids repetition).
type StubCache = VerificationCache<StubSessionVerifier>;

// ── StubSessionVerifier ────────────────────────────────────────────────────

/// A `SessionVerifier` test double that records how many times `verify` is
/// called and returns a pre-configured outcome.
#[derive(Clone)]
struct StubSessionVerifier {
    call_count: Arc<AtomicUsize>,
    outcome: StubVerifyOutcome,
}

#[derive(Clone)]
enum StubVerifyOutcome {
    OkSession(VerifiedSession),
    Err(VerifyError),
}

impl SessionVerifier for StubSessionVerifier {
    async fn verify(
        &self,
        _jwt: &str,
        _expected_audience: &str,
    ) -> Result<Option<VerifiedSession>, VerifyError> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        match &self.outcome {
            StubVerifyOutcome::OkSession(s) => Ok(Some(s.clone())),
            StubVerifyOutcome::Err(e) => Err(e.clone()),
        }
    }
}

// ── Context key types ──────────────────────────────────────────────────────

/// Wraps the atomic call counter so it can live in the brine Context.
#[derive(Clone)]
struct CallCounter(Arc<AtomicUsize>);

/// Wraps the cache lookup result for the fail-closed scenario.
#[derive(Clone, Debug)]
enum CacheLookupResult {
    Ok(VerifiedSession),
    Err(VerifyError),
}

// ── Step definitions ───────────────────────────────────────────────────────

pub fn steps() -> Vec<StepDef> {
    vec![
        // ── Given ──────────────────────────────────────────────────────────

        // "a counting stub verifier returning a valid session for token {string}"
        async_step_def(
            "a counting stub verifier returning a valid session for token {string}",
            &[],
            &[
                ("verification_cache", "StubCache"),
                ("call_counter", "CallCounter"),
                ("expected_token", "String"),
            ],
            |_ctx, params| async move {
                let token = params
                    .get_string(0)
                    .ok_or("Expected token string")?
                    .to_string();
                let counter = Arc::new(AtomicUsize::new(0));
                let session = VerifiedSession {
                    sub: "stub-user".to_string(),
                    sid: "stub-sid".to_string(),
                    provider: "test".to_string(),
                    scopes: vec![],
                    audience: "foundry-mcp:anvil-kit".to_string(),
                    // far future: token is not expired
                    expires_at: i64::MAX,
                };
                let verifier = StubSessionVerifier {
                    call_count: counter.clone(),
                    outcome: StubVerifyOutcome::OkSession(session),
                };
                let cache = VerificationCache::new(verifier);
                let mut out = Context::new();
                out.set("verification_cache", cache);
                out.set("call_counter", CallCounter(counter));
                out.set("expected_token", token);
                Ok(out)
            },
        ),

        // "a counting stub verifier returning a session with expires_at in the past for token {string}"
        async_step_def(
            "a counting stub verifier returning a session with expires_at in the past for token {string}",
            &[],
            &[
                ("verification_cache", "StubCache"),
                ("call_counter", "CallCounter"),
                ("expected_token", "String"),
            ],
            |_ctx, params| async move {
                let token = params
                    .get_string(0)
                    .ok_or("Expected token string")?
                    .to_string();
                let counter = Arc::new(AtomicUsize::new(0));
                let session = VerifiedSession {
                    sub: "stub-user".to_string(),
                    sid: "stub-sid".to_string(),
                    provider: "test".to_string(),
                    scopes: vec![],
                    audience: "foundry-mcp:anvil-kit".to_string(),
                    // already expired
                    expires_at: 1_000_000,
                };
                let verifier = StubSessionVerifier {
                    call_count: counter.clone(),
                    outcome: StubVerifyOutcome::OkSession(session),
                };
                let cache = VerificationCache::new(verifier);
                let mut out = Context::new();
                out.set("verification_cache", cache);
                out.set("call_counter", CallCounter(counter));
                out.set("expected_token", token);
                Ok(out)
            },
        ),

        // "a counting stub verifier returning Err(KeyFetch) for token {string}"
        async_step_def(
            "a counting stub verifier returning Err(KeyFetch) for token {string}",
            &[],
            &[
                ("verification_cache", "StubCache"),
                ("call_counter", "CallCounter"),
                ("expected_token", "String"),
            ],
            |_ctx, params| async move {
                let token = params
                    .get_string(0)
                    .ok_or("Expected token string")?
                    .to_string();
                let counter = Arc::new(AtomicUsize::new(0));
                let verifier = StubSessionVerifier {
                    call_count: counter.clone(),
                    outcome: StubVerifyOutcome::Err(VerifyError::KeyFetch),
                };
                let cache = VerificationCache::new(verifier);
                let mut out = Context::new();
                out.set("verification_cache", cache);
                out.set("call_counter", CallCounter(counter));
                out.set("expected_token", token);
                Ok(out)
            },
        ),

        // ── When ───────────────────────────────────────────────────────────

        // "the same token {string} is looked up 3 times through the cache"
        async_step_def(
            "the same token {string} is looked up 3 times through the cache",
            &[
                ("verification_cache", "StubCache"),
                ("call_counter", "CallCounter"),
            ],
            &[
                ("verification_cache", "StubCache"),
                ("call_counter", "CallCounter"),
                ("lookup_results", "Vec<Option<VerifiedSession>>"),
            ],
            |mut ctx, params| async move {
                let token = params
                    .get_string(0)
                    .ok_or("Expected token string")?
                    .to_string();
                let cache = ctx
                    .take::<StubCache>("verification_cache")
                    .ok_or("No verification_cache")?;
                let counter = ctx
                    .take::<CallCounter>("call_counter")
                    .ok_or("No call_counter")?;
                let mut results: Vec<Option<VerifiedSession>> = Vec::new();
                for _ in 0..3 {
                    let r = cache.lookup(&token, "foundry-mcp:anvil-kit").await;
                    results.push(r.ok().flatten());
                }
                let mut out = Context::new();
                out.set("verification_cache", cache);
                out.set("call_counter", counter);
                out.set("lookup_results", results);
                Ok(out)
            },
        ),

        // "the token {string} is looked up once through the cache"
        async_step_def(
            "the token {string} is looked up once through the cache",
            &[
                ("verification_cache", "StubCache"),
                ("call_counter", "CallCounter"),
            ],
            &[
                ("verification_cache", "StubCache"),
                ("call_counter", "CallCounter"),
                ("last_lookup_result", "CacheLookupResult"),
            ],
            |mut ctx, params| async move {
                let token = params
                    .get_string(0)
                    .ok_or("Expected token string")?
                    .to_string();
                let cache = ctx
                    .take::<StubCache>("verification_cache")
                    .ok_or("No verification_cache")?;
                let counter = ctx
                    .take::<CallCounter>("call_counter")
                    .ok_or("No call_counter")?;
                let result = cache.lookup(&token, "foundry-mcp:anvil-kit").await;
                let lookup = match result {
                    Ok(Some(s)) => CacheLookupResult::Ok(s),
                    Ok(None) => return Err("Expected Some session, got None".into()),
                    Err(e) => CacheLookupResult::Err(e),
                };
                let mut out = Context::new();
                out.set("verification_cache", cache);
                out.set("call_counter", counter);
                out.set("last_lookup_result", lookup);
                Ok(out)
            },
        ),

        // "the token {string} is looked up again through the cache"
        async_step_def(
            "the token {string} is looked up again through the cache",
            &[
                ("verification_cache", "StubCache"),
                ("call_counter", "CallCounter"),
            ],
            &[
                ("verification_cache", "StubCache"),
                ("call_counter", "CallCounter"),
                ("last_lookup_result", "CacheLookupResult"),
            ],
            |mut ctx, params| async move {
                let token = params
                    .get_string(0)
                    .ok_or("Expected token string")?
                    .to_string();
                let cache = ctx
                    .take::<StubCache>("verification_cache")
                    .ok_or("No verification_cache")?;
                let counter = ctx
                    .take::<CallCounter>("call_counter")
                    .ok_or("No call_counter")?;
                let result = cache.lookup(&token, "foundry-mcp:anvil-kit").await;
                let lookup = match result {
                    Ok(Some(s)) => CacheLookupResult::Ok(s),
                    Ok(None) => return Err("Expected Some session, got None".into()),
                    Err(e) => CacheLookupResult::Err(e),
                };
                let mut out = Context::new();
                out.set("verification_cache", cache);
                out.set("call_counter", counter);
                out.set("last_lookup_result", lookup);
                Ok(out)
            },
        ),

        // "the token {string} is looked up through the cache"
        async_step_def(
            "the token {string} is looked up through the cache",
            &[
                ("verification_cache", "StubCache"),
                ("call_counter", "CallCounter"),
            ],
            &[
                ("verification_cache", "StubCache"),
                ("call_counter", "CallCounter"),
                ("last_lookup_result", "CacheLookupResult"),
            ],
            |mut ctx, params| async move {
                let token = params
                    .get_string(0)
                    .ok_or("Expected token string")?
                    .to_string();
                let cache = ctx
                    .take::<StubCache>("verification_cache")
                    .ok_or("No verification_cache")?;
                let counter = ctx
                    .take::<CallCounter>("call_counter")
                    .ok_or("No call_counter")?;
                let result = cache.lookup(&token, "foundry-mcp:anvil-kit").await;
                let lookup = match result {
                    Ok(Some(s)) => CacheLookupResult::Ok(s),
                    Ok(None) => return Err("Expected Some session, got None".into()),
                    Err(e) => CacheLookupResult::Err(e),
                };
                let mut out = Context::new();
                out.set("verification_cache", cache);
                out.set("call_counter", counter);
                out.set("last_lookup_result", lookup);
                Ok(out)
            },
        ),

        // ── Then ───────────────────────────────────────────────────────────

        check_def(
            "the stub verifier was invoked exactly {int} time",
            &[("call_counter", "CallCounter")],
            |ctx, params| {
                let expected = params
                    .get_int(0)
                    .ok_or("Expected int")? as usize;
                let counter = ctx
                    .get::<CallCounter>("call_counter")
                    .ok_or("No call_counter")?;
                let actual = counter.0.load(Ordering::SeqCst);
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected verifier to be called {} time(s), got {}",
                        expected, actual
                    ))
                }
            },
        ),

        check_def(
            "the stub verifier was invoked exactly {int} times",
            &[("call_counter", "CallCounter")],
            |ctx, params| {
                let expected = params
                    .get_int(0)
                    .ok_or("Expected int")? as usize;
                let counter = ctx
                    .get::<CallCounter>("call_counter")
                    .ok_or("No call_counter")?;
                let actual = counter.0.load(Ordering::SeqCst);
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected verifier to be called {} times, got {}",
                        expected, actual
                    ))
                }
            },
        ),

        check_def(
            "all 3 lookups returned a valid session",
            &[("lookup_results", "Vec<Option<VerifiedSession>>")],
            |ctx, _params| {
                let results = ctx
                    .get::<Vec<Option<VerifiedSession>>>("lookup_results")
                    .ok_or("No lookup_results")?;
                if results.len() != 3 {
                    return Err(format!(
                        "Expected 3 results, got {}",
                        results.len()
                    ));
                }
                for (i, r) in results.iter().enumerate() {
                    if r.is_none() {
                        return Err(format!("Lookup {} returned None (expected a session)", i));
                    }
                }
                Ok(())
            },
        ),

        check_def(
            "the cache lookup returned a verification error",
            &[("last_lookup_result", "CacheLookupResult")],
            |ctx, _params| {
                let result = ctx
                    .get::<CacheLookupResult>("last_lookup_result")
                    .ok_or("No last_lookup_result")?;
                match result {
                    CacheLookupResult::Err(_) => Ok(()),
                    CacheLookupResult::Ok(_) => Err(
                        "Expected a verification error but got a valid session".into(),
                    ),
                }
            },
        ),
    ]
}
