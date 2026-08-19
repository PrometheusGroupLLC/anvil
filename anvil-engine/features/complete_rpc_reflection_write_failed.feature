Feature: Complete RPC — reflection_write_failed error surfaces at gRPC layer
  Covers the engine seam for spec R10.1(h) and (l).
  When the filesystem write for the reflection file fails (here: by placing a regular
  file where the adapter expects to create the _reflection/ subdirectory, causing
  create_dir_all to fail), the engine returns gRPC INTERNAL with the
  reflection_write_failed error code. No state transition is recorded.

  Scenario: Reflection file write failure returns INTERNAL with reflection_write_failed
    Given a hearth directory with the following structure:
      | path                                                   | state |
      | proposals/20260411T2021_anvil_workflow_engine/         | active |
      | tracks/20260420T0830_rpc_write_fail/                   | spec   |
    And the track "20260420T0830_rpc_write_fail" has spec.md with content "# RPC Write Fail\n\nSpec body."
    And a file exists at hearth path "tracks/20260420T0830_rpc_write_fail/spec_reflection" with content "obstruction"
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [RPC Write Fail](tracks/20260420T0830_rpc_write_fail/) — rpc write fail — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## spec_review

      ## plan

      ## implementing
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-04-20T00:00:00Z
      last_updated: 2026-04-20T00:00:00Z
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
      | artifact_path        | tracks/20260420T0830_rpc_write_fail |
      | actor_name           | Rpc-WriteFail-111111                |
      | actor_type           | agent                               |
      | actor_model          | claude-opus-4-7                     |
      | actor_provider       | anthropic                           |
      | actor_context_window | 200000                              |
      | actor_entrypoint     | claude-code                         |
      | reflection_notes     | Noting this for the failure test.   |
    Then the complete RPC returns gRPC status "INTERNAL"
    And the complete RPC error message contains "reflection_write_failed"
    And the complete RPC error message contains "spec_reflection"
    And the hearth file "tracks/20260420T0830_rpc_write_fail/status.yaml" contains "state: spec"
    And the hearth file "tracks/20260420T0830_rpc_write_fail/status.yaml" does not contain "state: spec_review"
