Feature: Complete spec_revision (doer revision done) — end-to-end
  A doer who finished addressing findings calls `complete(artifact_path)` with
  no satisfaction via MCP and the `spec_revision` track advances back to
  `spec_review`. Per spec R3.

  Scenario: Doer calls complete (no satisfaction) on a spec_revision track — advances to spec_review
    Given a hearth directory with the following structure:
      | path                                              | state         |
      | proposals/20260411T2021_anvil_workflow_engine/     | active        |
      | tracks/20260419T2300_revision_done_e2e/            | spec_revision |
    And the track "20260419T2300_revision_done_e2e" has spec.md with content "# Revision Done E2E\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Revision Done E2E](tracks/20260419T2300_revision_done_e2e/) — revision done e2e — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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

      | Track | Proposal |
      |-------|----------|
      | Revision Done E2E |  |

      ## Planned (0)

      ## Implementing (0)
      """
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a complete tools/call is sent with:
      | artifact_path        | tracks/20260419T2300_revision_done_e2e |
      | actor_name           | Doer-RD-E2E-222222                      |
      | actor_type           | agent                                  |
      | actor_model          | claude-opus-4-7                        |
      | actor_provider       | anthropic                              |
      | actor_context_window | 200000                                 |
      | actor_entrypoint     | claude-code                            |
    Then the complete response new_state is "spec_review"
    And the complete response transition_at is non-empty
    And the complete response artifact_path is "tracks/20260419T2300_revision_done_e2e"
    And the resolved state of "tracks/20260419T2300_revision_done_e2e" in the hearth is "spec_review"
    And a hearth transition event for "tracks/20260419T2300_revision_done_e2e" contains "to: spec_review"
    And a hearth transition event for "tracks/20260419T2300_revision_done_e2e" contains "role: spec"
    And a hearth transition event for "tracks/20260419T2300_revision_done_e2e" contains "actor: Doer-RD-E2E-222222"
