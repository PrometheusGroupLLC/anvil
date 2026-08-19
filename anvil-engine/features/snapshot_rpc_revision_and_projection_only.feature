Feature: Snapshot RPC revision mode and projection-only mode
  Revision-mode transitions skip projection updates. Projection-only
  mode skips status.yaml and registry writes and targets sparks.md only.

  Scenario: Target state spec_revision leaves projections_updated empty
    Given a hearth directory with the following structure:
      | path                                              | state       |
      | proposals/20260411T2021_anvil_workflow_engine/     | active      |
      | tracks/20260417T1400_rev_track/                   | spec_review |
    And the track "20260417T1400_rev_track" has spec.md with content "# Rev Track"
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Rev Track](tracks/20260417T1400_rev_track/) — rev track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)
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
      | artifact_path        | tracks/20260417T1400_rev_track |
      | to_state             | spec_revision                  |
      | actor_name           | Rpc-Test-2                     |
      | actor_role           | spec                           |
      | actor_type           | agent                          |
      | actor_model          | claude-opus-4-7                |
      | actor_provider       | anthropic                      |
      | actor_context_window | 1000000                        |
      | actor_sdk_version    | 0.2.111                        |
      | actor_entrypoint     | claude-desktop                 |
    Then the snapshot RPC response success is "true"
    And the snapshot RPC response projections_updated is empty

  Scenario: Projection-only spark event updates only sparks.md
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
    And the engine is started with that hearth
    When the snapshot RPC is called with:
      | artifact_path   | sparks/sparks.md |
      | projection_only | true             |
      | event_type      | spark            |
    Then the snapshot RPC response success is "true"
    And the snapshot RPC response status_updated is "false"
    And the snapshot RPC response registry_updated is "false"
