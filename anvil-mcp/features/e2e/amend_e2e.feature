Feature: Amend end-to-end — shim → engine → op recorded on disk
  An agent calls the `amend` MCP tool via the shim. The shim forwards the call
  to the engine, which validates the op, appends it to the per-document
  op-log file, and returns the op_id. Actor identity is forwarded verbatim
  (no session injection). For a track not in `completed`, new_state is absent
  (record-only path).

  Scenario: Amend records op to amendments.yaml for a spec track
    Given a hearth directory with the following structure:
      | path                                              | state |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
      | tracks/20260419T1400_amend_e2e/                    | spec  |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When an amend tools/call is sent with:
      | artifact_path   | tracks/20260419T1400_amend_e2e |
      | kind            | spec                           |
      | target_document | spec                           |
      | target_id       | AC-E2E-1                       |
      | op_kind         | add                            |
      | new_kind        | acceptance_criterion           |
      | body            | Spec body content.             |
      | actor_name      | Amend-E2E-111111               |
      | actor_type      | agent                          |
      | actor_model     | claude-sonnet-4-6              |
      | actor_provider  | anthropic                      |
    Then the amend response op_id is non-empty
    And the amend response has no new_state
    And the hearth file "tracks/20260419T1400_amend_e2e/spec.amendments.yaml" contains "AC-E2E-1"
    And the hearth file "tracks/20260419T1400_amend_e2e/spec.amendments.yaml" contains "add"
    And the hearth file "tracks/20260419T1400_amend_e2e/status.yaml" contains "Amend-E2E-111111"

  Scenario: Amend forwards actor identity verbatim
    Given a hearth directory with the following structure:
      | path                                              | state |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
      | tracks/20260419T1401_amend_identity/               | spec  |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When an amend tools/call is sent with:
      | artifact_path   | tracks/20260419T1401_amend_identity |
      | kind            | spec                                |
      | target_document | spec                                |
      | target_id       | AC-ID-1                             |
      | op_kind         | add                                 |
      | new_kind        | acceptance_criterion                |
      | body            | Identity test.                      |
      | actor_name      | Identity-Forward-999888             |
      | actor_type      | agent                               |
      | actor_model     | claude-opus-4-7                     |
      | actor_provider  | anthropic                           |
    Then the amend response op_id is non-empty
    And the hearth file "tracks/20260419T1401_amend_identity/status.yaml" contains "Identity-Forward-999888"
    And the hearth file "tracks/20260419T1401_amend_identity/status.yaml" contains "model: claude-opus-4-7"
    And the hearth file "tracks/20260419T1401_amend_identity/status.yaml" contains "type: agent"
    And the hearth file "tracks/20260419T1401_amend_identity/status.yaml" contains "provider: anthropic"

  Scenario: Amend drives completed to amend state on a completed track
    Given a hearth directory with the following structure:
      | path                                              | state     |
      | proposals/20260411T2021_anvil_workflow_engine/     | active    |
      | tracks/20260419T1402_amend_transition/             | completed |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When an amend tools/call is sent with:
      | artifact_path   | tracks/20260419T1402_amend_transition |
      | kind            | track                                 |
      | target_document | spec                                  |
      | target_id       | goal-tr-1                             |
      | op_kind         | add                                   |
      | new_kind        | goal                                  |
      | body            | Track transition test.                |
      | actor_name      | Amend-Doer-777777                     |
      | actor_type      | agent                                 |
      | actor_model     | claude-sonnet-4-6                     |
      | actor_provider  | anthropic                             |
    Then the amend response op_id is non-empty
    And the amend response new_state is "amend"
    And the hearth file "tracks/20260419T1402_amend_transition/spec.amendments.yaml" contains "goal-tr-1"
    And a hearth transition event for "tracks/20260419T1402_amend_transition" contains "to: amend"
