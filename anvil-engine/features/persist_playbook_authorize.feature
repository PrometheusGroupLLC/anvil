Feature: PersistPlaybook RPC — authorize gatekeeper + actor identity (track 1a, BP3, A6)
  The PersistPlaybook RPC enforces the actor-identity quartet like the other
  mutating RPCs (mirror amend). Empty identity fields fail with INVALID_ARGUMENT
  before any machine is written. Standalone behavior is unchanged.

  Scenario: PersistPlaybook empty actor_name returns INVALID_ARGUMENT
    Given a hearth directory with the following structure:
      | path                                          | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And the engine is started with that hearth
    And a separate temp owner-home directory
    When the PersistPlaybook RPC is called for kind "throwaway_kind" under that owner-home with:
      | actor_name     |                 |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-8 |
      | actor_provider | anthropic       |
    Then the PersistPlaybook RPC returns gRPC status "INVALID_ARGUMENT"
    And the PersistPlaybook RPC error message contains "actor_name_required"

  Scenario: PersistPlaybook empty actor_provider returns INVALID_ARGUMENT
    Given a hearth directory with the following structure:
      | path                                          | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And the engine is started with that hearth
    And a separate temp owner-home directory
    When the PersistPlaybook RPC is called for kind "throwaway_kind" under that owner-home with:
      | actor_name     | Persist-Doer-700010 |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-8 |
      | actor_provider |                 |
    Then the PersistPlaybook RPC returns gRPC status "INVALID_ARGUMENT"
    And the PersistPlaybook RPC error message contains "actor_params_required"
    And the PersistPlaybook RPC error message contains "actor_provider"
