Feature: describe next_step varies by subject
  The engine generates different next_step text for a type-level describe
  versus an instance-level describe.

  Scenario: Type-level and instance-level describes have distinct next-step text
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260414T0405_review_spec_strand/           | spec    |
    And the engine is started with that hearth
    When the describe RPC for type "track" is called
    And the describe RPC for instance id "20260414T0405_review_spec_strand" is called
    Then the describe next_step differs between type-level and instance-level
