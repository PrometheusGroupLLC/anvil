Feature: Complete RPC — error taxonomy
  Each CompleteError variant maps to the correct gRPC Status code at the
  engine boundary. Complements the domain-level rejection features.

  Scenario: Empty artifact_path returns INVALID_ARGUMENT
    Given a hearth directory with the following structure:
      | path                                          | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path  |                 |
      | actor_name     | Doer-440001     |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the complete RPC returns gRPC status "INVALID_ARGUMENT"
    And the complete RPC error message contains "artifact_path_required"

  Scenario: Empty actor_name returns INVALID_ARGUMENT
    Given a hearth directory with the following structure:
      | path                                              | state  |
      | proposals/20260411T2021_anvil_workflow_engine/    | active |
      | tracks/20260419T1400_rpc_error_name/              | spec   |
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path  | tracks/20260419T1400_rpc_error_name |
      | actor_name     |                                     |
      | actor_type     | agent                               |
      | actor_model    | claude-opus-4-7                     |
      | actor_provider | anthropic                           |
    Then the complete RPC returns gRPC status "INVALID_ARGUMENT"
    And the complete RPC error message contains "actor_name_required"

  Scenario: Empty actor_type returns INVALID_ARGUMENT
    Given a hearth directory with the following structure:
      | path                                              | state  |
      | proposals/20260411T2021_anvil_workflow_engine/    | active |
      | tracks/20260419T1401_rpc_error_type/              | spec   |
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path  | tracks/20260419T1401_rpc_error_type |
      | actor_name     | Doer-440003                         |
      | actor_type     |                                     |
      | actor_model    | claude-opus-4-7                     |
      | actor_provider | anthropic                           |
    Then the complete RPC returns gRPC status "INVALID_ARGUMENT"
    And the complete RPC error message contains "actor_params_required"
    And the complete RPC error message contains "actor_type"

  Scenario: Empty actor_model returns INVALID_ARGUMENT
    Given a hearth directory with the following structure:
      | path                                              | state |
      | proposals/20260411T2021_anvil_workflow_engine/    | active |
      | tracks/20260419T1406_rpc_error_model/             | spec   |
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path  | tracks/20260419T1406_rpc_error_model |
      | actor_name     | Doer-440008                          |
      | actor_type     | agent                                |
      | actor_model    |                                      |
      | actor_provider | anthropic                            |
    Then the complete RPC returns gRPC status "INVALID_ARGUMENT"
    And the complete RPC error message contains "actor_params_required"
    And the complete RPC error message contains "actor_model"

  Scenario: Empty actor_provider returns INVALID_ARGUMENT
    Given a hearth directory with the following structure:
      | path                                              | state |
      | proposals/20260411T2021_anvil_workflow_engine/    | active |
      | tracks/20260419T1407_rpc_error_provider/          | spec   |
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path  | tracks/20260419T1407_rpc_error_provider |
      | actor_name     | Doer-440009                             |
      | actor_type     | agent                                   |
      | actor_model    | claude-opus-4-7                         |
      | actor_provider |                                         |
    Then the complete RPC returns gRPC status "INVALID_ARGUMENT"
    And the complete RPC error message contains "actor_params_required"
    And the complete RPC error message contains "actor_provider"

  Scenario: satisfaction "address_in_next_step" without findings returns INVALID_ARGUMENT with findings_required
    # Slice C: address_in_next_step is accepted; absent findings the engine
    # returns findings_required_for_address_in_next_step (INVALID_ARGUMENT),
    # NOT satisfaction_out_of_scope.
    Given a hearth directory with the following structure:
      | path                                              | state       |
      | proposals/20260411T2021_anvil_workflow_engine/    | active      |
      | tracks/20260419T1402_rpc_out_of_scope/            | spec_review |
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path  | tracks/20260419T1402_rpc_out_of_scope |
      | actor_name     | Reviewer-440004                       |
      | actor_type     | agent                                 |
      | actor_model    | claude-opus-4-7                       |
      | actor_provider | anthropic                             |
      | satisfaction   | address_in_next_step                  |
    Then the complete RPC returns gRPC status "INVALID_ARGUMENT"
    And the complete RPC error message contains "findings_required_for_address_in_next_step"

  Scenario: satisfaction "bogus" returns INVALID_ARGUMENT with satisfaction_unknown
    Given a hearth directory with the following structure:
      | path                                              | state       |
      | proposals/20260411T2021_anvil_workflow_engine/    | active      |
      | tracks/20260419T1403_rpc_unknown_satisfaction/    | spec_review |
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path  | tracks/20260419T1403_rpc_unknown_satisfaction |
      | actor_name     | Reviewer-440005                               |
      | actor_type     | agent                                         |
      | actor_model    | claude-opus-4-7                               |
      | actor_provider | anthropic                                     |
      | satisfaction   | bogus                                         |
    Then the complete RPC returns gRPC status "INVALID_ARGUMENT"
    And the complete RPC error message contains "satisfaction_unknown"
    And the complete RPC error message contains "bogus"

  Scenario: Doer-style complete on spec_review returns FAILED_PRECONDITION with wrong_state_for_complete
    Given a hearth directory with the following structure:
      | path                                              | state       |
      | proposals/20260411T2021_anvil_workflow_engine/    | active      |
      | tracks/20260419T1404_rpc_wrong_state/             | spec_review |
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path  | tracks/20260419T1404_rpc_wrong_state |
      | actor_name     | Doer-440006                          |
      | actor_type     | agent                                |
      | actor_model    | claude-opus-4-7                      |
      | actor_provider | anthropic                            |
    Then the complete RPC returns gRPC status "FAILED_PRECONDITION"
    And the complete RPC error message contains "wrong_state_for_complete"

  Scenario: Reviewer-style complete on spec returns FAILED_PRECONDITION with wrong_state_for_complete
    Given a hearth directory with the following structure:
      | path                                              | state  |
      | proposals/20260411T2021_anvil_workflow_engine/    | active |
      | tracks/20260419T1405_rpc_wrong_state_reviewer/    | spec   |
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path  | tracks/20260419T1405_rpc_wrong_state_reviewer |
      | actor_name     | Reviewer-440007                               |
      | actor_type     | agent                                         |
      | actor_model    | claude-opus-4-7                               |
      | actor_provider | anthropic                                     |
      | satisfaction   | satisfied                                     |
    Then the complete RPC returns gRPC status "FAILED_PRECONDITION"
    And the complete RPC error message contains "wrong_state_for_complete"
