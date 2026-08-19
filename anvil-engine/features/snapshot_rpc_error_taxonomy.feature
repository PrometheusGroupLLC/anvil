Feature: Snapshot RPC error taxonomy
  Each SnapshotError variant maps to a distinct gRPC Status code.

  Scenario: Empty artifact_path → INVALID_ARGUMENT
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
    And the engine is started with that hearth
    When the snapshot RPC is called with:
      | artifact_path | |
      | to_state      | spec |
      | actor_name    | Rpc-Test-1 |
      | actor_role    | spec |
    Then the snapshot RPC returns gRPC status "INVALID_ARGUMENT"

  Scenario: Empty to_state → INVALID_ARGUMENT
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260417T1200_err_track/                   | spec    |
    And the engine is started with that hearth
    When the snapshot RPC is called with:
      | artifact_path | tracks/20260417T1200_err_track |
      | to_state      |                                |
      | actor_name    | Rpc-Test-1                     |
      | actor_role    | spec                           |
    Then the snapshot RPC returns gRPC status "INVALID_ARGUMENT"

  Scenario: Empty actor_role → INVALID_ARGUMENT
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260417T1300_err_track/                   | spec    |
    And the engine is started with that hearth
    When the snapshot RPC is called with:
      | artifact_path | tracks/20260417T1300_err_track |
      | to_state      | spec_review                    |
      | actor_name    | Rpc-Test-1                     |
      | actor_role    |                                |
    Then the snapshot RPC returns gRPC status "INVALID_ARGUMENT"

  Scenario: Non-existent artifact_path → NOT_FOUND
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
    And the engine is started with that hearth
    When the snapshot RPC is called with:
      | artifact_path  | tracks/no_such_track |
      | to_state       | spec_review          |
      | actor_name     | Rpc-Test-1           |
      | actor_role     | review               |
      | actor_type     | agent                |
      | actor_model    | claude-opus-4-7      |
      | actor_provider | anthropic            |
    Then the snapshot RPC returns gRPC status "NOT_FOUND"

  Scenario: Empty actor_name → INVALID_ARGUMENT (ActorNameRequired)
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260417T1400_name_track/                  | spec    |
    And the engine is started with that hearth
    When the snapshot RPC is called with:
      | artifact_path  | tracks/20260417T1400_name_track |
      | to_state       | spec_review                     |
      | actor_name     |                                 |
      | actor_role     | review                          |
      | actor_type     | agent                           |
      | actor_model    | claude-opus-4-7                 |
      | actor_provider | anthropic                       |
    Then the snapshot RPC returns gRPC status "INVALID_ARGUMENT"
    And the snapshot RPC error message contains "actor_name"

  Scenario: Empty actor_type → INVALID_ARGUMENT (ActorParamsRequired)
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260417T1500_id_track/                    | spec    |
    And the engine is started with that hearth
    When the snapshot RPC is called with:
      | artifact_path | tracks/20260417T1500_id_track |
      | to_state      | spec_review                   |
      | actor_name    | Rpc-Test-1                    |
      | actor_role    | review                        |
      | actor_type    |                               |
      | actor_model   | claude-opus-4-7               |
      | actor_provider| anthropic                     |
    Then the snapshot RPC returns gRPC status "INVALID_ARGUMENT"
    And the snapshot RPC error message contains "actor_type"

  Scenario: Empty actor_model → INVALID_ARGUMENT (ActorParamsRequired)
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260417T1500_id_track/                    | spec    |
    And the engine is started with that hearth
    When the snapshot RPC is called with:
      | artifact_path | tracks/20260417T1500_id_track |
      | to_state      | spec_review                   |
      | actor_name    | Rpc-Test-1                    |
      | actor_role    | review                        |
      | actor_type    | agent                         |
      | actor_model   |                               |
      | actor_provider| anthropic                     |
    Then the snapshot RPC returns gRPC status "INVALID_ARGUMENT"
    And the snapshot RPC error message contains "actor_model"

  Scenario: Empty actor_provider → INVALID_ARGUMENT (ActorParamsRequired)
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260417T1500_id_track/                    | spec    |
    And the engine is started with that hearth
    When the snapshot RPC is called with:
      | artifact_path | tracks/20260417T1500_id_track |
      | to_state      | spec_review                   |
      | actor_name    | Rpc-Test-1                    |
      | actor_role    | review                        |
      | actor_type    | agent                         |
      | actor_model   | claude-opus-4-7               |
      | actor_provider|                               |
    Then the snapshot RPC returns gRPC status "INVALID_ARGUMENT"
    And the snapshot RPC error message contains "actor_provider"

  Scenario: Malformed status.yaml → INTERNAL
    Given a hearth directory with a malformed status.yaml:
      | path                                   | content                       |
      | tracks/20260417T1600_malformed_track/  | version: 1\nstate: [this is   |
    And the engine is started with that hearth
    When the snapshot RPC is called with:
      | artifact_path  | tracks/20260417T1600_malformed_track |
      | to_state       | spec_review                           |
      | actor_name     | Rpc-Test-1                            |
      | actor_role     | review                                |
      | actor_type     | agent                                 |
      | actor_model    | claude-opus-4-7                       |
      | actor_provider | anthropic                             |
    Then the snapshot RPC returns gRPC status "INTERNAL"
