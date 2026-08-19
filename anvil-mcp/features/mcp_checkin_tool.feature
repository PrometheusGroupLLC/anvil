Feature: MCP Checkin Tool
  The MCP shim exposes the reshaped checkin tool with role-based filtering.

  Scenario: Tools list includes the reshaped checkin tool
    Given the MCP shim is started
    And the MCP session is initialized
    When a tools/list request is sent
    Then the response contains a tool named "checkin"
    And the checkin tool has a description
    And the checkin tool requires "role"
    And the checkin tool requires "actor_type"
    And the checkin tool requires "actor_model"
    And the checkin tool requires "actor_provider"
