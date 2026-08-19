Feature: MCP snapshot missing identity surfaces engine error
  With no prior `checkin` and no identity fields in the tool args, the
  engine returns INVALID_ARGUMENT (per spec R3 of the
  checkin_backfill_spec_context track, the missing field is `actor_name`
  — checked before the runtime params). The shim surfaces the error as
  MCP `isError` with the engine's message.

  Scenario: No session + no identity args → INVALID_ARGUMENT propagated
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260417T2100_miss_track/                  | spec    |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a snapshot tools/call is sent with:
      | artifact_path | tracks/20260417T2100_miss_track |
      | to_state      | spec_review                     |
      | actor_role    | review                          |
    Then the MCP response is a tool error containing "actor_name"
