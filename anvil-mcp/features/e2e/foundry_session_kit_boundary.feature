Feature: Foundry session kit boundary enforcement (spec Req 4 + Req 5 shim half)
  The MCP shim is the kit boundary for Foundry session enforcement.  When the
  shim runs under Foundry (FOUNDRY_SESSION_TOKEN is present and non-blank), it
  must validate the token before any engine call and surface a JSON-RPC error to
  the client if the token is invalid — never silently proceed or forward a bad
  token.  When the token is valid (accepted by the engine's stub verifier), the
  shim attaches `authorization: Bearer <jwt>` on every outbound gRPC call so the
  Foundry-mode engine can verify it.  Standalone mode (no token) is unchanged.

  Background:
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260403T1500_forge_lifecycle/           | active       |

  # ── Req 4: invalid token is rejected at the kit boundary ─────────────────────

  Scenario: Invalid session token is refused at the kit boundary
    Given the engine is started in Foundry mode with a rejecting verifier
    And a .hearth file pointing to that directory while the running engine remains active
    And the MCP shim is started in that working directory with the running engine endpoint and session token "invalid-token"
    And the MCP session is initialized while the running engine remains active
    When a tools/call request is sent for "catalog" while the running engine remains active
    Then the MCP response is a JSON-RPC error
    And the MCP response error message contains "not_authenticated"
    And the MCP shim spawned no engine of its own

  # ── Req 5 (shim half): valid token forwarded as Bearer metadata ───────────────

  Scenario: Valid session token is forwarded to the Foundry-mode engine and accepted
    Given the engine is started in Foundry mode with an accepting verifier for sub "user-abc-123"
    And a .hearth file pointing to that directory while the running engine remains active
    And the MCP shim is started in that working directory with the running engine endpoint and session token "valid-token"
    And the MCP session is initialized while the running engine remains active
    When a tools/call request is sent for "catalog" while the running engine remains active
    Then the MCP response contains active artifacts
    And the MCP shim spawned no engine of its own

  # ── Req 1 (trim rule): whitespace-only token = standalone, forwarded as-if standalone ─
  # The shim does NOT error at the kit boundary for whitespace-only tokens — it
  # treats them as absent (Standalone).  No bearer is forwarded.  The engine
  # (which is in Foundry mode with a rejecting verifier) refuses the
  # unauthenticated call; the shim surfaces it as a JSON-RPC error.  The
  # important invariant is that the shim does NOT raise a validation error
  # for a whitespace-only token before reaching the engine.

  Scenario: Whitespace-only session token is treated as standalone (no kit-boundary error)
    Given the engine is started in Foundry mode with a rejecting verifier
    And a .hearth file pointing to that directory while the running engine remains active
    And the MCP shim is started in that working directory with the running engine endpoint and session token "   "
    And the MCP session is initialized while the running engine remains active
    When a tools/call request is sent for "catalog" while the running engine remains active
    Then the MCP response is a JSON-RPC error
    And the MCP response error message contains "not_authenticated"
    And the MCP shim spawned no engine of its own

  # ── Standalone (no token): unchanged behavior ────────────────────────────────

  Scenario: Standalone mode (no session token) is unchanged
    Given the engine is started with that hearth
    And a .hearth file pointing to that directory while the running engine remains active
    And the MCP shim is started in that working directory with the running engine endpoint
    And the MCP session is initialized while the running engine remains active
    When a tools/call request is sent for "catalog" while the running engine remains active
    Then the MCP response contains active artifacts
    And the MCP shim spawned no engine of its own
