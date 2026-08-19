Feature: MCP shim dials an existing engine
  The kit MCP shim should reuse a supervised engine endpoint when one is
  available. That keeps Foundry on a single shared engine instead of creating
  duplicate per-shim engines.

  Scenario: ANVIL_ENGINE_PORT points the shim at an already-running engine
    Given a hearth directory with the following structure:
      | path                                      | state  |
      | proposals/20260403T1500_forge_lifecycle/ | active |
    And the engine is started with that hearth
    And a .hearth file pointing to that directory while the running engine remains active
    And the MCP shim is started in that working directory with the running engine endpoint
    And the MCP session is initialized while the running engine remains active
    When a tools/call request is sent for "catalog" while the running engine remains active
    Then the MCP response contains active artifacts
    And the MCP catalog result includes artifact "20260403T1500_forge_lifecycle"
    And the MCP shim spawned no engine of its own
