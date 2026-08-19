Feature: MCP checkin surfaces next_step in JSON
  The MCP shim passes the engine's next_step guidance through to the JSON
  response so agents receive routing instructions.

  Scenario: Reviewer checkin JSON includes a non-empty next_step
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260414T0405_review_spec_strand/           | spec    |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a checkin tools/call is sent with role "reviewer" and:
      | field          | value           |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-6 |
      | actor_provider | anthropic       |
    Then the checkin response has a non-empty next_step text
