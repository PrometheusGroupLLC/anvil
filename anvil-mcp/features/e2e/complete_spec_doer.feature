Feature: Complete spec (doer) — end-to-end happy path
  An agent acting as doer on a `spec` track calls `complete(artifact_path,
  actor_*)` via MCP and the track advances to `spec_review` with full
  bookkeeping.

  Scenario: Doer calls complete on a spec track — advances to spec_review
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260419T1000_complete_doer_e2e/            | spec    |
    And the track "20260419T1000_complete_doer_e2e" has spec.md with content "# Complete Doer E2E\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Complete Doer E2E](tracks/20260419T1000_complete_doer_e2e/) — complete doer e2e — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a complete tools/call is sent with:
      | artifact_path        | tracks/20260419T1000_complete_doer_e2e |
      | actor_name           | Doer-E2E-111111                        |
      | actor_type           | agent                                  |
      | actor_model          | claude-opus-4-7                        |
      | actor_provider       | anthropic                              |
      | actor_context_window | 200000                                 |
      | actor_entrypoint     | claude-code                            |
    Then the complete response new_state is "spec_review"
    And the complete response transition_at is non-empty
    And the complete response artifact_path is "tracks/20260419T1000_complete_doer_e2e"
    And the resolved state of "tracks/20260419T1000_complete_doer_e2e" in the hearth is "spec_review"
    And a hearth transition event for "tracks/20260419T1000_complete_doer_e2e" contains "to: spec_review"
    And a hearth transition event for "tracks/20260419T1000_complete_doer_e2e" contains "role: spec"
    And a hearth transition event for "tracks/20260419T1000_complete_doer_e2e" contains "actor: Doer-E2E-111111"
    And the hearth file "tracks/20260419T1000_complete_doer_e2e/status.yaml" contains "Doer-E2E-111111:"
    And the hearth file "tracks.md" contains "## spec_review"
    And the hearth file "tracks.md" does not contain "20260419T1000_complete_doer_e2e" under section "## spec"
    And the hearth file "projections/execution.md" contains "## Spec Review (1)"
