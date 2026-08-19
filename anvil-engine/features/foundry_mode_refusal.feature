Feature: Engine refuses unauthenticated calls under Foundry (spec Req 5)
  When the engine runs in Foundry mode (FOUNDRY_SESSION_TOKEN +
  FOUNDRY_BROKER_SOCKET present at startup), it becomes the gatekeeper: every
  gated RPC must carry a valid bearer session token. A missing, malformed,
  wrong-audience, expired, or bad-signature token is refused with gRPC
  Unauthenticated and the canonical not_authenticated message. This is the
  headline security property — the engine no longer trusts loopback alone.

  The reject paths are driven against the REAL spawned engine binary with a
  hermetic rejecting verifier (ANVIL_TEST_SESSION_VERIFIER=stub_reject), so no
  live broker is required: the engine refuses at or before verification.

  Scenario: A Foundry-mode RPC with no bearer token is refused
    Given the engine is started in Foundry mode with a rejecting verifier
    When the catalog RPC is called with no bearer token
    Then the catalog RPC returns gRPC status "UNAUTHENTICATED"
    And the catalog RPC error message contains "not_authenticated"

  Scenario: A Foundry-mode RPC with a malformed token is refused
    Given the engine is started in Foundry mode with a rejecting verifier
    When the catalog RPC is called with bearer token "not-a-jwt"
    Then the catalog RPC returns gRPC status "UNAUTHENTICATED"
    And the catalog RPC error message contains "not_authenticated"

  Scenario: A Foundry-mode RPC with a wrong-audience token is refused
    Given the engine is started in Foundry mode with a rejecting verifier
    When the catalog RPC is called with bearer token "wrong-audience-token"
    Then the catalog RPC returns gRPC status "UNAUTHENTICATED"
    And the catalog RPC error message contains "not_authenticated"

  Scenario: A Foundry-mode RPC with an expired token is refused
    Given the engine is started in Foundry mode with a rejecting verifier
    When the catalog RPC is called with bearer token "expired-token"
    Then the catalog RPC returns gRPC status "UNAUTHENTICATED"
    And the catalog RPC error message contains "not_authenticated"

  Scenario: A Foundry-mode RPC with a bad-signature token is refused
    Given the engine is started in Foundry mode with a rejecting verifier
    When the catalog RPC is called with bearer token "bad-signature-token"
    Then the catalog RPC returns gRPC status "UNAUTHENTICATED"
    And the catalog RPC error message contains "not_authenticated"

  Scenario: Broker-unreachable on a Foundry-mode RPC refuses (fail-closed, spec Req 6)
    Given the engine is started in Foundry mode with an unreachable broker
    When the catalog RPC is called with bearer token "some-token"
    Then the catalog RPC returns gRPC status "UNAUTHENTICATED"
    And the catalog RPC error message contains "not_authenticated"

  Scenario: A whitespace-only session token at startup means Standalone (spec Req 1)
    Given the engine is started in Foundry mode with a whitespace-only session token
    When the catalog RPC is called with no bearer token
    Then the catalog RPC succeeds
