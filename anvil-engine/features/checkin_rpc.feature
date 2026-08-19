Feature: Checkin RPC
  The engine serves a reshaped checkin RPC that registers actor identity
  and returns a role-filtered view of the hearth.

  Scenario: Creator role returns active proposals and available types
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260413T1349_checkin_decomposition/        | implementing |
    And the engine is started with that hearth
    When the checkin RPC is called with role "creator"
    Then the checkin RPC response has a non-empty actor name
    And the checkin RPC filtered artifacts include "20260411T2021_anvil_workflow_engine"
    And the checkin RPC filtered artifacts do not include "20260413T1349_checkin_decomposition"
    And the checkin RPC available types include "track"

  @registration
  Scenario: Creator role advertises backlog_item creation as engine-supported
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
    And the engine is started with that hearth
    When the checkin RPC is called with role "creator"
    Then the checkin RPC available types include "backlog_item"
    And the checkin RPC available type "backlog_item" has execution_route "engine"

  Scenario: Resumer role returns doer-actionable artifacts
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260413T1349_checkin_decomposition/        | implementing |
    And the engine is started with that hearth
    When the checkin RPC is called with role "resumer"
    Then the checkin RPC response has a non-empty actor name
    And the checkin RPC filtered artifacts include "20260413T1349_checkin_decomposition"
    And the checkin RPC filtered artifacts do not include "20260411T2021_anvil_workflow_engine"

  Scenario: Invalid role returns gRPC INVALID_ARGUMENT
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
    And the engine is started with that hearth
    When the checkin RPC is called with role "admin"
    Then the checkin RPC returns gRPC status "INVALID_ARGUMENT"

  Scenario: Caller-supplied actor_name is echoed verbatim (spec R1)
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
    And the engine is started with that hearth
    When the checkin RPC is called with role "creator" and actor_name "TestActor-000042"
    Then the checkin RPC response actor_name is "TestActor-000042"
