Feature: Spec revision multi-round cycle — end-to-end
  The spec_review → spec_revision → spec_review cycle may repeat with no upper
  bound. This scenario traverses two full revision rounds in a single run:
  reviewer full_revision → doer complete → reviewer full_revision → doer
  complete. The track ends in spec_review with four recorded transitions. Per
  spec R8.1(d) / R3.6.

  Scenario: Two revision rounds — reviewer full_revision and doer complete, twice
    Given a hearth directory with the following structure:
      | path                                              | state       |
      | proposals/20260411T2021_anvil_workflow_engine/     | active      |
      | tracks/20260419T2400_multi_round_e2e/              | spec_review |
    And the track "20260419T2400_multi_round_e2e" has spec.md with content "# Multi Round E2E\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Multi Round E2E](tracks/20260419T2400_multi_round_e2e/) — multi round e2e — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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
      | Multi Round E2E |  |

      ## Planned (0)

      ## Implementing (0)
      """
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    # Round 1: reviewer requires revision
    When a complete tools/call is sent with:
      | artifact_path        | tracks/20260419T2400_multi_round_e2e |
      | actor_name           | Reviewer-MR-111111                   |
      | actor_type           | agent                                |
      | actor_model          | claude-opus-4-7                      |
      | actor_provider       | anthropic                            |
      | satisfaction         | full_revision                        |
    Then the complete response new_state is "spec_revision"
    # Round 1: doer addresses findings
    When a complete tools/call is sent with:
      | artifact_path        | tracks/20260419T2400_multi_round_e2e |
      | actor_name           | Doer-MR-222222                       |
      | actor_type           | agent                                |
      | actor_model          | claude-opus-4-7                      |
      | actor_provider       | anthropic                            |
    Then the complete response new_state is "spec_review"
    # Round 2: reviewer requires another revision
    When a complete tools/call is sent with:
      | artifact_path        | tracks/20260419T2400_multi_round_e2e |
      | actor_name           | Reviewer-MR-111111                   |
      | actor_type           | agent                                |
      | actor_model          | claude-opus-4-7                      |
      | actor_provider       | anthropic                            |
      | satisfaction         | full_revision                        |
    Then the complete response new_state is "spec_revision"
    # Round 2: doer addresses findings again
    When a complete tools/call is sent with:
      | artifact_path        | tracks/20260419T2400_multi_round_e2e |
      | actor_name           | Doer-MR-222222                       |
      | actor_type           | agent                                |
      | actor_model          | claude-opus-4-7                      |
      | actor_provider       | anthropic                            |
    Then the complete response new_state is "spec_review"
    And the resolved state of "tracks/20260419T2400_multi_round_e2e" in the hearth is "spec_review"
    And a hearth transition event for "tracks/20260419T2400_multi_round_e2e" contains "to: spec_revision"
    And a hearth transition event for "tracks/20260419T2400_multi_round_e2e" contains "to: spec_review"
