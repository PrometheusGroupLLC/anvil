Feature: Complete spec review (reviewer full_revision) — end-to-end
  A reviewer on a `spec_review` track calls `complete(artifact_path,
  satisfaction: "full_revision", actor_*)` via MCP and the track advances to
  `spec_revision` with full bookkeeping. Per spec R1.

  Scenario: Reviewer calls complete with full_revision on a spec_review track — advances to spec_revision
    Given a hearth directory with the following structure:
      | path                                              | state       |
      | proposals/20260411T2021_anvil_workflow_engine/     | active      |
      | tracks/20260419T2100_full_revision_e2e/            | spec_review |
    And the track "20260419T2100_full_revision_e2e" has spec.md with content "# Full Revision E2E\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Full Revision E2E](tracks/20260419T2100_full_revision_e2e/) — full revision e2e — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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
      | Full Revision E2E |  |

      ## Planned (0)

      ## Implementing (0)
      """
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a complete tools/call is sent with:
      | artifact_path        | tracks/20260419T2100_full_revision_e2e |
      | actor_name           | Reviewer-FR-E2E-111111                 |
      | actor_type           | agent                                  |
      | actor_model          | claude-opus-4-7                        |
      | actor_provider       | anthropic                              |
      | actor_context_window | 200000                                 |
      | actor_entrypoint     | claude-code                            |
      | satisfaction         | full_revision                          |
    Then the complete response new_state is "spec_revision"
    And the complete response transition_at is non-empty
    And the complete response artifact_path is "tracks/20260419T2100_full_revision_e2e"
    And the resolved state of "tracks/20260419T2100_full_revision_e2e" in the hearth is "spec_revision"
    And a hearth transition event for "tracks/20260419T2100_full_revision_e2e" contains "to: spec_revision"
    And a hearth transition event for "tracks/20260419T2100_full_revision_e2e" contains "role: review"
    And a hearth transition event for "tracks/20260419T2100_full_revision_e2e" contains "actor: Reviewer-FR-E2E-111111"
    And the hearth file "tracks/20260419T2100_full_revision_e2e/status.yaml" contains "Reviewer-FR-E2E-111111:"
