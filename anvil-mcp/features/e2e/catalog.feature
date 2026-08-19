Feature: Catalog End-to-End
  The MCP shim starts the engine, connects via gRPC, and returns
  catalog results through the full MCP path.

  Scenario: Catalog via MCP returns active artifacts from the hearth
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260403T1500_forge_lifecycle/           | active       |
      | proposals/20260404T2116_forge_initiatives/         | completed    |
      | tracks/20260411T2345_mcp_server/                   | implementing |
      | initiatives/follow-forge-lifecycle/                | promoted     |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a tools/call request is sent for "catalog"
    Then the MCP response contains active artifacts
    And the MCP catalog result includes artifact "20260403T1500_forge_lifecycle"
    And the MCP catalog result includes artifact "20260411T2345_mcp_server"
    And the MCP catalog result includes artifact "follow-forge-lifecycle"
    And the MCP catalog result does not include "20260404T2116_forge_initiatives"
    And the MCP catalog result includes available type "proposal"
    And the MCP catalog result includes available type "track"
    And the MCP catalog result includes available playbook kind "track" with described "true" and triggers "true"
    And the MCP catalog result includes available playbook kind "playbook" with described "true" and triggers "true"
    And the MCP catalog result includes available playbook kind "decision" with described "true" and triggers "false"

  Scenario: Shim connects to an already-running engine
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260403T1500_forge_lifecycle/           | active       |
    And the engine is started with that hearth
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a tools/call request is sent for "catalog"
    Then the MCP response contains active artifacts
    And the MCP catalog result includes artifact "20260403T1500_forge_lifecycle"

  Scenario: Shim discovers hearth via MCP roots instead of cwd
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260403T1500_forge_lifecycle/           | active       |
    And a .hearth file pointing to that directory
    And the MCP shim is started without a working directory
    And the MCP session is initialized with roots pointing to the work directory
    When a tools/call request is sent for "catalog"
    Then the MCP response contains active artifacts
    And the MCP catalog result includes artifact "20260403T1500_forge_lifecycle"

  Scenario: Shim honors an explicit hearth path argument
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260403T1500_forge_lifecycle/           | active       |
    And no .hearth file in the working directory
    And the MCP shim is started in that working directory with explicit hearth path
    And the MCP session is initialized
    When a tools/call request is sent for "catalog"
    Then the MCP response contains active artifacts
    And the MCP catalog result includes artifact "20260403T1500_forge_lifecycle"

  Scenario: Missing .hearth file returns structured MCP error
    Given no .hearth file in the working directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a tools/call request is sent for "catalog"
    Then the MCP response is a tool error

  Scenario: Malformed hearth returns structured MCP error through full stack
    Given a hearth directory with a malformed status.yaml:
      | path                                  | content          |
      | proposals/20260403T1500_bad_proposal/  | not: valid: yaml: [ |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a tools/call request is sent for "catalog"
    Then the MCP response is a tool error
