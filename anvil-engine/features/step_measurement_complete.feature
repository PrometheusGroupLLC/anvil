Feature: Complete command RPC emits a step_measurement record
  On each complete call, the engine emits exactly one flat
  event_kind="step_measurement" record carrying the real from_state->to_state
  transition, the doer|reviewer role, the actor, the resolved playbook_id and
  track_id, and the (from_state, role) intent/expected_output from the state
  schema. The transition itself still records (the emit is additive). The
  tokens/duration_ms keys are absent this slice (D-4).

  Background:
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260419T1100_rpc_complete_track/           | spec    |
    And the track "20260419T1100_rpc_complete_track" has spec.md with content "# RPC Complete Track\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [RPC Complete Track](tracks/20260419T1100_rpc_complete_track/) — rpc complete track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## spec_review

      ## plan

      ## implementing
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-04-19T00:00:00Z
      last_updated: 2026-04-19T00:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Spec (0)

      ## Spec Review (0)

      ## Planned (0)

      ## Implementing (0)
      """
    And the engine is started with that hearth

  Scenario: Complete on a spec track emits one step_measurement with real from->to and intent
    When the complete RPC is called with:
      | artifact_path        | tracks/20260419T1100_rpc_complete_track |
      | actor_name           | Rpc-Doer-111111                         |
      | actor_type           | agent                                   |
      | actor_model          | claude-opus-4-7                         |
      | actor_provider       | anthropic                               |
      | actor_context_window | 200000                                  |
      | actor_entrypoint     | claude-code                             |
    Then the complete RPC response new_state is "spec_review"
    And the engine stderr contains exactly 1 JSON log records with event_kind "step_measurement"
    And the engine stderr contains a JSON log record with fields:
      | event_kind      | step_measurement                 |
      | playbook_id     | 20260419T1100_rpc_complete_track |
      | track_id        | track                            |
      | from_state      | spec                             |
      | to_state        | spec_review                      |
      | role            | doer                             |
      | actor           | Rpc-Doer-111111                 |
      | intent          | <non-empty>                     |
      | expected_output | <non-empty>                     |
      | at              | <non-empty>                     |

  Scenario: The step_measurement record omits tokens and duration_ms
    When the complete RPC is called with:
      | artifact_path        | tracks/20260419T1100_rpc_complete_track |
      | actor_name           | Rpc-Doer-111111                         |
      | actor_type           | agent                                   |
      | actor_model          | claude-opus-4-7                         |
      | actor_provider       | anthropic                               |
      | actor_context_window | 200000                                  |
      | actor_entrypoint     | claude-code                             |
    Then the complete RPC response new_state is "spec_review"
    And the engine stderr event_kind "step_measurement" log record has no "tokens" field
    And the engine stderr event_kind "step_measurement" log record has no "duration_ms" field
