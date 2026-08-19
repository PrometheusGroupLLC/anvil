Feature: Snapshot RPC happy path
  A track transitioning spec → spec_review through the Snapshot RPC
  records the transition, moves the tracks.md entry, and updates
  execution.md. The response echoes timestamp, actor_name, and
  projections_updated.

  Scenario: Track spec → spec_review via RPC
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260417T1000_rpc_track/                   | spec    |
    And the track "20260417T1000_rpc_track" has spec.md with content "# Rpc Track\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Rpc Track](tracks/20260417T1000_rpc_track/) — rpc track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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

      ## Spec (0)

      ## Spec Review (0)
      """
    And the engine is started with that hearth
    When the snapshot RPC is called with:
      | artifact_path        | tracks/20260417T1000_rpc_track |
      | to_state             | spec_review                    |
      | actor_name           | Rpc-Test-111111                |
      | actor_role           | review                         |
      | actor_type           | agent                          |
      | actor_model          | claude-opus-4-7                |
      | actor_provider       | anthropic                      |
      | actor_context_window | 1000000                        |
      | actor_sdk_version    | 0.2.111                        |
      | actor_entrypoint     | claude-desktop                 |
    Then the snapshot RPC response success is "true"
    And the snapshot RPC response status_updated is "true"
    And the snapshot RPC response registry_updated is "true"
    And the snapshot RPC response projections_updated contains "execution.md"
    And the snapshot RPC response timestamp matches "^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$"
    And the snapshot RPC response actor_name is "Rpc-Test-111111"
