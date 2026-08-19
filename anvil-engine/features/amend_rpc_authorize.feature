Feature: Amend RPC — authorize gatekeeper + actor identity (BP3, AC-7)
  The Amend RPC enforces the actor-identity quartet like the other mutating
  RPCs. Empty identity fields fail with INVALID_ARGUMENT before any op is
  recorded. Standalone behavior is unchanged (no session injection).

  Scenario: empty artifact_path returns INVALID_ARGUMENT
    Given a hearth directory with the following structure:
      | path                                          | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And the engine is started with that hearth
    When the amend RPC is called with:
      | artifact_path   |                 |
      | kind            | track           |
      | target_document | spec            |
      | target_id       | goal-x          |
      | op_kind         | add             |
      | new_kind        | goal            |
      | body            | body            |
      | actor_name      | Rpc-Doer-300020 |
      | actor_type      | agent           |
      | actor_model     | claude-opus-4-7 |
      | actor_provider  | anthropic       |
    Then the amend RPC returns gRPC status "INVALID_ARGUMENT"
    And the amend RPC error message contains "artifact_path_required"

  Scenario: amend empty actor_name returns INVALID_ARGUMENT
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260411T2021_anvil_workflow_engine/     | active       |
      | tracks/20260604T2114_amend_auth/                   | implementing |
    And the engine is started with that hearth
    When the amend RPC is called with:
      | artifact_path   | tracks/20260604T2114_amend_auth |
      | kind            | track                           |
      | target_document | spec                            |
      | target_id       | goal-x                          |
      | op_kind         | add                             |
      | new_kind        | goal                            |
      | body            | body                            |
      | actor_name      |                                 |
      | actor_type      | agent                           |
      | actor_model     | claude-opus-4-7                 |
      | actor_provider  | anthropic                       |
    Then the amend RPC returns gRPC status "INVALID_ARGUMENT"
    And the amend RPC error message contains "actor_name_required"

  Scenario: amend empty actor_provider returns INVALID_ARGUMENT
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260411T2021_anvil_workflow_engine/     | active       |
      | tracks/20260604T2114_amend_auth2/                  | implementing |
    And the engine is started with that hearth
    When the amend RPC is called with:
      | artifact_path   | tracks/20260604T2114_amend_auth2 |
      | kind            | track                            |
      | target_document | spec                             |
      | target_id       | goal-x                           |
      | op_kind         | add                              |
      | new_kind        | goal                             |
      | body            | body                             |
      | actor_name      | Rpc-Doer-300021                  |
      | actor_type      | agent                            |
      | actor_model     | claude-opus-4-7                  |
      | actor_provider  |                                  |
    Then the amend RPC returns gRPC status "INVALID_ARGUMENT"
    And the amend RPC error message contains "actor_params_required"
    And the amend RPC error message contains "actor_provider"
