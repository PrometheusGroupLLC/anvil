Feature: Session Verification Cache
  The VerificationCache satisfies spec Req 6: at most one broker round-trip
  per token per TTL window. A StubSessionVerifier with an observable
  invocation count proves no per-RPC call occurs once a token is cached.

  Scenario: Same unexpired token verified only once (cache hit on second call)
    Given a counting stub verifier returning a valid session for token "tok-abc"
    When the same token "tok-abc" is looked up 3 times through the cache
    Then the stub verifier was invoked exactly 1 time
    And all 3 lookups returned a valid session

  Scenario: Expired token triggers re-verification (cache miss on expiry)
    Given a counting stub verifier returning a session with expires_at in the past for token "tok-exp"
    When the token "tok-exp" is looked up once through the cache
    And the token "tok-exp" is looked up again through the cache
    Then the stub verifier was invoked exactly 2 times

  Scenario: Broker unreachable on a cache miss propagates the error (fail-closed)
    # NOTE (Q1): this scenario's "returns Unauthenticated" assertion is
    # at the cache-layer edge only (verify returns Err).  The gRPC
    # Unauthenticated status finalization lands at 3.3 when authorize()
    # is wired to the handlers.  This scenario turns fully green at 2.4
    # (the cache propagates the error rather than bypassing it); the
    # gRPC-level assertion is a Phase 3 concern.
    Given a counting stub verifier returning Err(KeyFetch) for token "tok-bad"
    When the token "tok-bad" is looked up through the cache
    Then the cache lookup returned a verification error
    And the stub verifier was invoked exactly 1 time
