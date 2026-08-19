Feature: Decomposed Checkin End-to-End
  Full stack verification of the decomposed creator playbook:
  checkin → describe → begin via MCP.
  The creator declares intent, discovers what's needed, then creates.

  Scenario: Creator flow creates track via checkin, describe, begin
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
    And a playbook hook body for the spec doer hook "spec-writing.md" with content "Write the spec for this track."
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a checkin tools/call is sent with role "creator" and:
      | field          | value           |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-6 |
      | actor_provider | anthropic       |
    Then the checkin response has a generated actor name
    And the checkin response includes available type "track"
    And the checkin response includes artifact "20260411T2021_anvil_workflow_engine" with state "active"
    When a describe tools/call is sent with type "track"
    Then the describe response has type name "track"
    And the describe response has required field "name"
    And the describe response has parent type "proposal"
    When a begin tools/call is sent with:
      | field         | value                              |
      | artifact_type | track                              |
      | parent_id     | 20260411T2021_anvil_workflow_engine |
      | track_name    | my new track                       |
      | approver      | mark                               |
    Then the begin response has state "spec"
    And the begin response has a track path
    And the begin response has context text containing "Write the spec"
    And the hearth has a new track directory with status.yaml
    And the hearth tracks.md contains the new track under "## spec"
    And the hearth execution.md contains the new track in the "Spec" section
