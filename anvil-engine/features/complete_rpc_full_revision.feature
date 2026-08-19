Feature: Complete RPC — reviewer full_revision (spec_review → spec_revision)
  A reviewer calling Complete with satisfaction "full_revision" on a
  `spec_review` track advances it to `spec_revision` (Slice B). The response
  carries new_state, transition_at, and artifact_path. The engine writes
  status.yaml (role: review) and the consolidated registry; transitions into a
  revision state skip the execution projection. Per spec R1.

  Scenario: Reviewer complete with full_revision on spec_review track via RPC — advances to spec_revision
    Given a hearth directory with the following structure:
      | path                                              | state       |
      | proposals/20260411T2021_anvil_workflow_engine/     | active      |
      | tracks/20260419T1500_rpc_full_revision/            | spec_review |
    And the track "20260419T1500_rpc_full_revision" has spec.md with content "# RPC Full Revision\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [RPC Full Revision](tracks/20260419T1500_rpc_full_revision/) — rpc full revision — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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

      ## Spec Review (1)

      - [RPC Full Revision](tracks/20260419T1500_rpc_full_revision/)

      ## Planned (0)

      ## Implementing (0)
      """
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path        | tracks/20260419T1500_rpc_full_revision |
      | actor_name           | Rpc-Reviewer-555555                    |
      | actor_type           | agent                                  |
      | actor_model          | claude-opus-4-7                        |
      | actor_provider       | anthropic                              |
      | actor_context_window | 200000                                 |
      | actor_entrypoint     | claude-code                            |
      | satisfaction         | full_revision                          |
    Then the complete RPC response new_state is "spec_revision"
    And the complete RPC response transition_at matches "^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$"
    And the complete RPC response artifact_path is "tracks/20260419T1500_rpc_full_revision"
    And the resolved state of "tracks/20260419T1500_rpc_full_revision" in the hearth is "spec_revision"
    And a hearth transition event for "tracks/20260419T1500_rpc_full_revision" contains "to: spec_revision"
    And a hearth transition event for "tracks/20260419T1500_rpc_full_revision" contains "role: review"
    And a hearth transition event for "tracks/20260419T1500_rpc_full_revision" contains "actor: Rpc-Reviewer-555555"
    And the hearth file "tracks/20260419T1500_rpc_full_revision/status.yaml" contains "Rpc-Reviewer-555555:"
