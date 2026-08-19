Feature: Snapshot RPC requires explicit actor_name
  Per spec R3 of the checkin_backfill_spec_context track, the snapshot
  RPC requires the caller to supply a non-empty `actor_name`. An empty
  value returns `INVALID_ARGUMENT` (the engine maps `ActorNameRequired`
  to that gRPC status); the engine no longer generates names.

  Scenario: Empty actor_name → INVALID_ARGUMENT
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260417T1100_gen_track/                   | spec    |
    And the track "20260417T1100_gen_track" has spec.md with content "# Gen Track"
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Gen Track](tracks/20260417T1100_gen_track/) — gen track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## spec_review
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-04-16T00:00:00Z
      last_updated: 2026-04-16T00:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Spec Review (0)
      """
    And the engine is started with that hearth
    When the snapshot RPC is called with:
      | artifact_path        | tracks/20260417T1100_gen_track |
      | to_state             | spec_review                    |
      | actor_name           |                                |
      | actor_role           | review                         |
      | actor_type           | agent                          |
      | actor_model          | claude-opus-4-7                |
      | actor_provider       | anthropic                      |
      | actor_context_window | 1000000                        |
      | actor_sdk_version    | 0.2.111                        |
      | actor_entrypoint     | claude-desktop                 |
    Then the snapshot RPC returns gRPC status "INVALID_ARGUMENT"
    And the snapshot RPC error message contains "actor_name"
