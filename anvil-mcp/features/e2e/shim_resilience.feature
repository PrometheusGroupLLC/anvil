Feature: MCP shim resilience
  The MCP shim survives an engine restart mid-session and never lets a
  single bad request crash the whole process. Engine churn (Foundry-wide
  restarts) must be transparent, not a "drop".

  Scenario: A transient transport failure on the first call is retried and succeeds
    # The engine is up the whole time, but the shim's first call is forced to
    # fail as if the engine had just restarted. The shim must reconnect and
    # retry the same call once, so the catalog still returns.
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260403T1500_forge_lifecycle/           | active       |
    And the engine is started with that hearth
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory with the running engine endpoint and a forced first-call transport failure
    And the MCP session is initialized
    When a tools/call request is sent for "catalog"
    Then the MCP response contains active artifacts
    And the MCP catalog result includes artifact "20260403T1500_forge_lifecycle"

  Scenario: A handler that cannot serialize its result returns a tool error and the shim stays alive
    # With result serialization forced to fail, the catalog handler would
    # previously panic the whole shim. Now it returns a tool error, and the
    # shim is still alive to serve a subsequent call.
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260403T1500_forge_lifecycle/           | active       |
    And the engine is started with that hearth
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory with the running engine endpoint and forced result serialization failure
    And the MCP session is initialized while the running engine remains active
    When a tools/call request is sent for "catalog"
    Then the MCP response is a tool error
    When a tools/call request is sent for "catalog"
    Then the MCP response is a tool error
