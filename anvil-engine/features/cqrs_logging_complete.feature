Feature: Complete command RPC emits a structured CQRS log record
  A complete call that advances a track emits one JSON log record carrying
  the actor, resolved hearth, command name, the real emitted CompleteEvent
  variant name(s), and an ok outcome.

  Scenario: Complete on a spec track logs a JSON command-outcome record with event names
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
    When the complete RPC is called with:
      | artifact_path        | tracks/20260419T1100_rpc_complete_track |
      | actor_name           | Rpc-Doer-111111                         |
      | actor_type           | agent                                   |
      | actor_model          | claude-opus-4-7                         |
      | actor_provider       | anthropic                               |
      | actor_context_window | 200000                                  |
      | actor_entrypoint     | claude-code                             |
    Then the complete RPC response new_state is "spec_review"
    And the engine stderr contains a JSON log record with fields:
      | command | complete          |
      | actor   | Rpc-Doer-111111   |
      | hearth  | <non-empty>       |
      | events  | TransitionRecorded |
      | outcome | ok                |
