Feature: MCP Describe Tool
  The MCP shim exposes describe for type-level and instance-level queries.

  Scenario: Describe type via MCP returns creation schema
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a describe tools/call is sent with type "track"
    Then the describe response has type name "track"
    And the describe response has parent type "proposal"
    And the describe response has required field "name"
