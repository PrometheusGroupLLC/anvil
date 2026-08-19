Feature: Begin RPC maps generic required-field failures to FAILED_PRECONDITION

  Scenario: track create missing required fields returns FAILED_PRECONDITION
    Given a hearth directory with the following structure:
      | path                                          | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a playbook hook body for the spec doer hook "spec-writing.md" with content "Spec writing guidance."
    And the engine is started with that hearth
    When the begin RPC is called to create a track with track_name "" parent "" approver ""
    Then the begin RPC returns gRPC status "FAILED_PRECONDITION"
    And the begin RPC error message contains "missing_required_field"
    And the begin RPC error message contains "name"
    And the begin RPC error message contains "parent_id"
    And the begin RPC error message contains "approver"
    And the hearth contains no artifact directories under "tracks"

