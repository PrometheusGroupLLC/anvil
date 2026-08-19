Feature: checkin next_step is keyed to response content
  The engine generates next_step text from the response content rather than
  templating it statically. Different content shapes produce distinct text
  that reflects what is actually in the response.

  Scenario: Empty reviewer list produces a specific "none pending" message
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
    And the engine is started with that hearth
    When the checkin RPC is called with role "reviewer"
    Then the checkin RPC next_step text contains "No artifacts are awaiting review"

  Scenario: Populated reviewer list produces a specific per-artifact message
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260414T0405_review_spec_strand/           | spec    |
    And the engine is started with that hearth
    When the checkin RPC is called with role "reviewer"
    Then the checkin RPC next_step text contains "artifact(s) await review"
    And the checkin RPC next_step text contains "execution_route"
