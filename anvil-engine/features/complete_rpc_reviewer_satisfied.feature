Feature: Complete RPC — reviewer satisfied (spec_review → plan)
  A reviewer calling Complete with satisfaction "satisfied" on a
  `spec_review` track advances it to `plan`. The response carries
  new_state, transition_at, and artifact_path. The engine writes
  status.yaml, tracks.md, and execution.md side effects.

  Scenario: Reviewer complete with satisfied on spec_review track via RPC — advances to plan
    Given a hearth directory with the following structure:
      | path                                              | state       |
      | proposals/20260411T2021_anvil_workflow_engine/     | active      |
      | tracks/20260419T1300_rpc_reviewer_satisfied/       | spec_review |
    And the track "20260419T1300_rpc_reviewer_satisfied" has spec.md with content "# RPC Reviewer Satisfied\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      ## spec_review

      - [RPC Reviewer Satisfied](tracks/20260419T1300_rpc_reviewer_satisfied/) — rpc reviewer satisfied — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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

      - [RPC Reviewer Satisfied](tracks/20260419T1300_rpc_reviewer_satisfied/)

      ## Planned (0)

      ## Implementing (0)
      """
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path        | tracks/20260419T1300_rpc_reviewer_satisfied |
      | actor_name           | Rpc-Reviewer-333333                         |
      | actor_type           | agent                                       |
      | actor_model          | claude-opus-4-7                             |
      | actor_provider       | anthropic                                   |
      | actor_context_window | 200000                                      |
      | actor_entrypoint     | claude-code                                 |
      | satisfaction         | satisfied                                   |
    Then the complete RPC response new_state is "plan"
    And the complete RPC response transition_at matches "^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$"
    And the complete RPC response artifact_path is "tracks/20260419T1300_rpc_reviewer_satisfied"
    And the resolved state of "tracks/20260419T1300_rpc_reviewer_satisfied" in the hearth is "plan"
    And a hearth transition event for "tracks/20260419T1300_rpc_reviewer_satisfied" contains "to: plan"
    And a hearth transition event for "tracks/20260419T1300_rpc_reviewer_satisfied" contains "role: review"
    And a hearth transition event for "tracks/20260419T1300_rpc_reviewer_satisfied" contains "actor: Rpc-Reviewer-333333"
    And the hearth file "tracks/20260419T1300_rpc_reviewer_satisfied/status.yaml" contains "Rpc-Reviewer-333333:"
    And the hearth file "tracks.md" contains "## plan"
    And the hearth file "tracks.md" does not contain "20260419T1300_rpc_reviewer_satisfied" under section "## spec_review"
    And the hearth file "projections/execution.md" contains "## Planned (1)"
