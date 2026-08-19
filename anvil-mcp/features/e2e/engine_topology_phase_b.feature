Feature: MCP shim daemon topology
  The MCP shim dials a shared daemon by discovery. It never starts a local
  engine of its own.

  Scenario: Missing daemon returns daemon_unreachable and spawns nothing
    Given a hearth directory with the following structure:
      | path                                      | state  |
      | proposals/20260403T1500_forge_lifecycle/ | active |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory with a dead engine endpoint
    And the MCP session is initialized
    When a tools/call request is sent for "catalog"
    Then the MCP response is a tool error
    And the MCP response error message contains "daemon_unreachable:"
    And the MCP response error message contains "anvil engine not reachable on 127.0.0.1:"
    And the MCP response error message contains "start Foundry (or launch anvil-engine manually for local dev)"
    And the MCP shim spawned no engine of its own

  Scenario: Canonical daemon is dialed without spawning its own engine
    Given a hearth directory with the following structure:
      | path                                      | state  |
      | proposals/20260403T1500_forge_lifecycle/ | active |
    And a .hearth file pointing to that directory
    And the canonical engine is started with that hearth
    And the MCP shim is started in that working directory with canonical daemon discovery
    And the MCP session is initialized while the canonical engine remains active
    When a tools/call request is sent for "catalog" while the canonical engine remains active
    Then the MCP response contains active artifacts
    And the MCP catalog result includes artifact "20260403T1500_forge_lifecycle"
    And the MCP shim spawned no engine of its own

  Scenario: Wire-version refusal does not spawn an engine
    Given a hearth directory with the following structure:
      | path                                          | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And the engine is started with that hearth
    And a .hearth file pointing to that directory while the running engine remains active
    And the MCP shim is started in that working directory with the running engine endpoint and expected wire version "999"
    And the MCP session is initialized while the running engine remains active
    When a checkin tools/call is sent with role "creator" and:
      | field          | value          |
      | actor_name     | WireActor-0004 |
      | actor_type     | agent          |
      | actor_model    | test-model     |
      | actor_provider | test           |
    Then the MCP response is a tool error containing "client_engine_proto_version_mismatch"
    And the MCP response error message contains "expects wire v999"
    And the MCP shim spawned no engine of its own
