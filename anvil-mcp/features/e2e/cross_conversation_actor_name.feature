Feature: Cross-conversation actor_name continuity end-to-end
  Per spec R1 of the checkin_backfill_spec_context track, a caller can
  carry identity across conversation boundaries by supplying an
  `actor_name` to `checkin`. The engine echoes it verbatim. A
  subsequent `begin` or `snapshot` call using the same `actor_name`
  writes the canonical identity to the target artifact's actors table
  — no second-conversation identity drift, no engine-side rename.

  Scenario: checkin echoes caller-supplied actor_name; begin uses it for the track's actors table
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
    And the kit-bundled playbook is copied into the hearth tmpdir
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a checkin tools/call is sent with role "creator" and:
      | field          | value                |
      | actor_name     | CarryForward-424242  |
      | actor_type     | agent                |
      | actor_model    | claude-opus-4-7      |
      | actor_provider | anthropic            |
    Then the checkin response actor_name is "CarryForward-424242"
    When a begin tools/call is sent with:
      | field          | value                                |
      | artifact_type  | track                                |
      | parent_id      | 20260411T2021_anvil_workflow_engine  |
      | track_name     | carry forward demo                   |
      | approver       | mark                                 |
      | actor_name     | CarryForward-424242                  |
      | actor_type     | agent                                |
      | actor_model    | claude-opus-4-7                      |
      | actor_provider | anthropic                            |
    Then the begin response has state "spec"
    And the hearth has a new track directory with status.yaml
    And the hearth's new track status.yaml contains "CarryForward-424242:"
    And the hearth's new track transition event contains "actor: CarryForward-424242"
