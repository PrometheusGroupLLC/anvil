Feature: Begin Session Error End-to-End
  Verifies that begin without a prior checkin returns a structured session error.

  Scenario: Begin without prior checkin returns session error
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a begin tools/call is sent with:
      | field         | value                              |
      | artifact_type | track                              |
      | parent_id     | 20260411T2021_anvil_workflow_engine |
      | track_name    | test track                         |
      | approver      | mark                               |
    Then the MCP response is a tool error containing "No active session"
