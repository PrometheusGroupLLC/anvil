Feature: MCP shims adopt a running engine
  When a shared engine daemon is already answering, repeated MCP shim
  invocations must dial that daemon instead of starting duplicate engines.

  Scenario: two shim invocations reuse the running engine
    Given a hearth directory with the following structure:
      | path                                      | state  |
      | proposals/20260403T1500_forge_lifecycle/ | active |
    And the engine is started with that hearth
    And a .hearth file pointing to that directory while the running engine remains active
    When two MCP shim invocations call catalog using the running engine endpoint
    Then both MCP shims spawned no fallback engine
    And a gRPC HealthCheck on the engine port succeeds
