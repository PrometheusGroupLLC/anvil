Feature: MCP checkin surfaces execution_route in JSON
  The MCP shim passes the engine's execution_route discriminator through
  to the JSON response so agents can route per-entry without parsing prose.

  Scenario: Reviewer checkin JSON carries execution_route on filtered_artifacts
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260411T2021_anvil_workflow_engine/     | active       |
      | tracks/20260414T0405_review_spec_strand/           | spec_review  |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a checkin tools/call is sent with role "reviewer" and:
      | field          | value           |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-6 |
      | actor_provider | anthropic       |
    Then the checkin response includes artifact "20260414T0405_review_spec_strand" with state "spec_review"
    And the checkin response artifact "20260414T0405_review_spec_strand" has execution_route "engine"

  Scenario: Creator checkin JSON carries execution_route on available_types
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a checkin tools/call is sent with role "creator" and:
      | field          | value           |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-6 |
      | actor_provider | anthropic       |
    Then the checkin response includes available type "track"
    And the checkin response available type "track" has execution_route "engine"
    And the checkin response available type "decision" has execution_route "engine"
    And the checkin response available type "proposal" has execution_route "engine"

  Scenario: Describe JSON reports engine for decision instance actions
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | decisions/engine-decision/                         | tension |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a describe tools/call is sent with id "engine-decision"
    Then the describe response includes action "tension_review"
    And the describe response has an action with execution_route "engine"

  Scenario: Resumer checkin JSON reports engine for track doer states
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260411T2021_anvil_workflow_engine/     | active       |
      | tracks/20260614T0650_mcp_plan/                     | plan         |
      | tracks/20260614T0651_mcp_implementing/             | implementing |
      | tracks/20260614T0652_mcp_reflecting/               | reflecting   |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a checkin tools/call is sent with role "resumer" and:
      | field          | value           |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-6 |
      | actor_provider | anthropic       |
    Then the checkin response artifact "20260614T0650_mcp_plan" has execution_route "engine"
    And the checkin response artifact "20260614T0651_mcp_implementing" has execution_route "engine"
    And the checkin response artifact "20260614T0652_mcp_reflecting" has execution_route "engine"
