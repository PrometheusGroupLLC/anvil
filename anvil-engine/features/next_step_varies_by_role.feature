Feature: checkin next_step varies by role
  The engine generates different next_step text for creator vs reviewer
  checkin responses. The text references the per-entry execution_route
  discriminator rather than enumerating engine-supported combinations in
  prose.

  Scenario: Creator and reviewer see different next-step instructions
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260414T0405_review_spec_strand/           | spec    |
    And the engine is started with that hearth
    When the checkin RPC for role "creator" is called
    And the checkin RPC for role "reviewer" is called
    Then the checkin next_step for creator differs from reviewer
    And the checkin next_step for creator references the execution_route field
    And the checkin next_step for reviewer references the execution_route field
