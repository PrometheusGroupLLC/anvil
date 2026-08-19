Feature: MCP snapshot projection-only invocation
  Projection-only invocation (spark / annotation events) skips status
  and registry writes; only the sparks projection is updated.

  Scenario: projection_only=true with event_type=spark updates sparks.md only
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a snapshot tools/call is sent with:
      | artifact_path   | sparks/sparks.md |
      | to_state        |                  |
      | actor_role      |                  |
      | projection_only | true             |
      | event_type      | spark            |
    Then the snapshot response projections_updated contains "sparks.md"
