Feature: MCP shim never self-spawns an engine
  The MCP shim discovers and connects to an explicitly running engine. It never
  starts its own anvil-engine process.

  Scenario: No reachable engine returns a tool error and spawns nothing
    Given a hearth directory with the following structure:
      | path                                      | state  |
      | proposals/20260403T1500_forge_lifecycle/ | active |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory with a dead engine endpoint
    And the MCP session is initialized
    When a tools/call request is sent for "catalog"
    Then the MCP response is a tool error containing "daemon_unreachable"
    And the MCP response error message contains "not reachable"
    And the MCP shim spawned no engine of its own

  Scenario: Explicit engine endpoint succeeds and the shim spawns nothing
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
