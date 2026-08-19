Feature: Complete spec review (reviewer satisfied) — end-to-end happy path
  A reviewer on a `spec_review` track calls `complete(artifact_path,
  satisfaction: "satisfied", actor_*)` via MCP and the track advances
  to `plan` with full bookkeeping.

  Scenario: Reviewer calls complete with satisfied on a spec_review track — advances to plan
    Given a hearth directory with the following structure:
      | path                                              | state       |
      | proposals/20260411T2021_anvil_workflow_engine/     | active      |
      | tracks/20260419T2000_complete_reviewer_e2e/        | spec_review |
    And the track "20260419T2000_complete_reviewer_e2e" has spec.md with content "# Complete Reviewer E2E\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      ## spec_review

      - [Complete Reviewer E2E](tracks/20260419T2000_complete_reviewer_e2e/) — complete reviewer e2e — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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

      ## Spec Review (1)

      | Track | Proposal |
      |-------|----------|
      | Complete Reviewer E2E |  |

      ## Planned (0)

      ## Implementing (0)
      """
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a complete tools/call is sent with:
      | artifact_path        | tracks/20260419T2000_complete_reviewer_e2e |
      | actor_name           | Reviewer-E2E-222222                        |
      | actor_type           | agent                                      |
      | actor_model          | claude-opus-4-7                            |
      | actor_provider       | anthropic                                  |
      | actor_context_window | 200000                                     |
      | actor_entrypoint     | claude-code                                |
      | satisfaction         | satisfied                                  |
    Then the complete response new_state is "plan"
    And the complete response transition_at is non-empty
    And the complete response artifact_path is "tracks/20260419T2000_complete_reviewer_e2e"
    And the resolved state of "tracks/20260419T2000_complete_reviewer_e2e" in the hearth is "plan"
    And a hearth transition event for "tracks/20260419T2000_complete_reviewer_e2e" contains "to: plan"
    And a hearth transition event for "tracks/20260419T2000_complete_reviewer_e2e" contains "role: review"
    And a hearth transition event for "tracks/20260419T2000_complete_reviewer_e2e" contains "actor: Reviewer-E2E-222222"
    And the hearth file "tracks/20260419T2000_complete_reviewer_e2e/status.yaml" contains "Reviewer-E2E-222222:"
    And the hearth file "tracks.md" contains "## plan"
    And the hearth file "tracks.md" does not contain "20260419T2000_complete_reviewer_e2e" under section "## spec_review"
    And the hearth file "projections/execution.md" contains "## Planned (1)"
    And the hearth file "projections/execution.md" contains "Complete Reviewer E2E"
