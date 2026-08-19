Feature: Complete RPC — doer revision done (spec_revision → spec_review)
  A doer calling Complete with no satisfaction on a `spec_revision` track
  advances it back to `spec_review` (Slice B), recording a transition with
  role "spec" and no approver. Per spec R3.

  Scenario: Doer complete with no satisfaction on spec_revision track via RPC — advances to spec_review
    Given a hearth directory with the following structure:
      | path                                              | state         |
      | proposals/20260411T2021_anvil_workflow_engine/     | active        |
      | tracks/20260419T1800_rpc_revision_done/            | spec_revision |
    And the track "20260419T1800_rpc_revision_done" has spec.md with content "# RPC Revision Done\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [RPC Revision Done](tracks/20260419T1800_rpc_revision_done/) — rpc revision done — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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

      - [RPC Revision Done](tracks/20260419T1800_rpc_revision_done/)

      ## Planned (0)

      ## Implementing (0)
      """
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path        | tracks/20260419T1800_rpc_revision_done |
      | actor_name           | Rpc-Doer-666666                        |
      | actor_type           | agent                                  |
      | actor_model          | claude-opus-4-7                        |
      | actor_provider       | anthropic                              |
      | actor_context_window | 200000                                 |
      | actor_entrypoint     | claude-code                            |
    Then the complete RPC response new_state is "spec_review"
    And the complete RPC response transition_at matches "^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$"
    And the complete RPC response artifact_path is "tracks/20260419T1800_rpc_revision_done"
    And the resolved state of "tracks/20260419T1800_rpc_revision_done" in the hearth is "spec_review"
    And a hearth transition event for "tracks/20260419T1800_rpc_revision_done" contains "to: spec_review"
    And a hearth transition event for "tracks/20260419T1800_rpc_revision_done" contains "role: spec"
    And a hearth transition event for "tracks/20260419T1800_rpc_revision_done" contains "actor: Rpc-Doer-666666"
    And the hearth file "tracks/20260419T1800_rpc_revision_done/status.yaml" contains "Rpc-Doer-666666:"
