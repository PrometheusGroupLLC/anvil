Feature: Complete spec review (reviewer carry-forward) — end-to-end happy path
  A reviewer on a `spec_review` track calls `complete(artifact_path,
  satisfaction: "address_in_next_step", findings: <text>, actor_*)` via MCP. The
  track advances to `plan`, carry-forward.md is written with the verbatim
  findings, and the tool result carries new_state, transition_at, artifact_path,
  and carry_forward_path. End-to-end through the shim + engine subprocess.

  Scenario: Reviewer calls complete with address_in_next_step — advances to plan and writes carry-forward.md
    Given a hearth directory with the following structure:
      | path                                              | state       |
      | proposals/20260411T2021_anvil_workflow_engine/     | active      |
      | tracks/20260419T2100_complete_carry_forward_e2e/   | spec_review |
    And the track "20260419T2100_complete_carry_forward_e2e" has spec.md with content "# Complete Carry Forward E2E\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      ## spec_review

      - [Complete Carry Forward E2E](tracks/20260419T2100_complete_carry_forward_e2e/) — complete carry forward e2e — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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
      | Complete Carry Forward E2E |  |

      ## Planned (0)

      ## Implementing (0)
      """
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a complete tools/call is sent with:
      | artifact_path        | tracks/20260419T2100_complete_carry_forward_e2e |
      | actor_name           | Reviewer-E2E-211111                             |
      | actor_type           | agent                                           |
      | actor_model          | claude-opus-4-7                                 |
      | actor_provider       | anthropic                                       |
      | actor_context_window | 200000                                          |
      | actor_entrypoint     | claude-code                                     |
      | satisfaction         | address_in_next_step                            |
      | findings             | The spec is accurate but confirm X and Y during implementation. |
    Then the complete response new_state is "plan"
    And the complete response transition_at is non-empty
    And the complete response artifact_path is "tracks/20260419T2100_complete_carry_forward_e2e"
    And the complete response carry_forward_path ends with "tracks/20260419T2100_complete_carry_forward_e2e/carry-forward.md"
    And the resolved state of "tracks/20260419T2100_complete_carry_forward_e2e" in the hearth is "plan"
    And a hearth transition event for "tracks/20260419T2100_complete_carry_forward_e2e" contains "to: plan"
    And a hearth transition event for "tracks/20260419T2100_complete_carry_forward_e2e" contains "satisfaction: address_in_next_step"
    And the hearth file "tracks/20260419T2100_complete_carry_forward_e2e/carry-forward.md" contains "findings_from: spec_review"
    And the hearth file "tracks/20260419T2100_complete_carry_forward_e2e/carry-forward.md" contains "satisfied_by: Reviewer-E2E-211111"
    And the hearth file "tracks/20260419T2100_complete_carry_forward_e2e/carry-forward.md" contains "The spec is accurate but confirm X and Y during implementation."
    And the hearth file "tracks.md" contains "## plan"
    And the hearth file "projections/execution.md" contains "## Planned (1)"

  Scenario: Reviewer calls complete with address_in_next_step but no findings — tool error
    Given a hearth directory with the following structure:
      | path                                              | state       |
      | proposals/20260411T2021_anvil_workflow_engine/     | active      |
      | tracks/20260419T2101_carry_forward_no_findings/    | spec_review |
    And the track "20260419T2101_carry_forward_no_findings" has spec.md with content "# Carry Forward No Findings\n\nSpec body."
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a complete tools/call is sent with:
      | artifact_path  | tracks/20260419T2101_carry_forward_no_findings |
      | actor_name     | Reviewer-E2E-211112                            |
      | actor_type     | agent                                          |
      | actor_model    | claude-opus-4-7                                |
      | actor_provider | anthropic                                      |
      | satisfaction   | address_in_next_step                           |
    Then the complete response is a tool error containing "findings_required_for_address_in_next_step"
