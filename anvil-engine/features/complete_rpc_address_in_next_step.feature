Feature: Complete RPC — reviewer carry-forward (spec_review → plan, address_in_next_step)
  Slice C at the engine seam: a reviewer Complete with satisfaction
  "address_in_next_step" and non-empty findings on a spec_review track advances
  to plan, writes carry-forward.md with the verbatim findings, and returns
  carry_forward_path. Missing findings → INVALID_ARGUMENT
  (findings_required_for_address_in_next_step); wrong state → FAILED_PRECONDITION
  (wrong_state_for_complete).

  Scenario: Reviewer carry-forward complete via RPC — advances to plan and writes carry-forward.md
    Given a hearth directory with the following structure:
      | path                                              | state       |
      | proposals/20260411T2021_anvil_workflow_engine/     | active      |
      | tracks/20260419T1500_rpc_carry_forward/            | spec_review |
    And the track "20260419T1500_rpc_carry_forward" has spec.md with content "# RPC Carry Forward\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      ## spec_review

      - [RPC Carry Forward](tracks/20260419T1500_rpc_carry_forward/) — rpc carry forward — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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

      - [RPC Carry Forward](tracks/20260419T1500_rpc_carry_forward/)

      ## Planned (0)

      ## Implementing (0)
      """
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path        | tracks/20260419T1500_rpc_carry_forward         |
      | actor_name           | Rpc-Reviewer-555000                            |
      | actor_type           | agent                                          |
      | actor_model          | claude-opus-4-7                                |
      | actor_provider       | anthropic                                      |
      | actor_context_window | 200000                                         |
      | actor_entrypoint     | claude-code                                    |
      | satisfaction         | address_in_next_step                           |
      | findings             | Confirm X and Y during implementation.         |
    Then the complete RPC response new_state is "plan"
    And the complete RPC response transition_at matches "^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$"
    And the complete RPC response artifact_path is "tracks/20260419T1500_rpc_carry_forward"
    And the complete RPC response carry_forward_path is non-empty
    And the complete RPC response carry_forward_path ends with "tracks/20260419T1500_rpc_carry_forward/carry-forward.md"
    And the resolved state of "tracks/20260419T1500_rpc_carry_forward" in the hearth is "plan"
    And a hearth transition event for "tracks/20260419T1500_rpc_carry_forward" contains "to: plan"
    And a hearth transition event for "tracks/20260419T1500_rpc_carry_forward" contains "satisfaction: address_in_next_step"
    And the hearth file "tracks/20260419T1500_rpc_carry_forward/carry-forward.md" contains "findings_from: spec_review"
    And the hearth file "tracks/20260419T1500_rpc_carry_forward/carry-forward.md" contains "satisfied_by: Rpc-Reviewer-555000"
    And the hearth file "tracks/20260419T1500_rpc_carry_forward/carry-forward.md" contains "Confirm X and Y during implementation."
    And the hearth file "tracks.md" contains "## plan"
    And the hearth file "projections/execution.md" contains "## Planned (1)"

  Scenario: address_in_next_step without findings via RPC returns INVALID_ARGUMENT
    Given a hearth directory with the following structure:
      | path                                              | state       |
      | proposals/20260411T2021_anvil_workflow_engine/     | active      |
      | tracks/20260419T1501_rpc_cf_no_findings/           | spec_review |
    And the track "20260419T1501_rpc_cf_no_findings" has spec.md with content "# RPC CF No Findings\n\nSpec body."
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path  | tracks/20260419T1501_rpc_cf_no_findings |
      | actor_name     | Rpc-Reviewer-555001                     |
      | actor_type     | agent                                   |
      | actor_model    | claude-opus-4-7                         |
      | actor_provider | anthropic                               |
      | satisfaction   | address_in_next_step                    |
    Then the complete RPC returns gRPC status "INVALID_ARGUMENT"
    And the complete RPC error message contains "findings_required_for_address_in_next_step"

  Scenario: address_in_next_step on a non-spec_review track via RPC returns FAILED_PRECONDITION
    Given a hearth directory with the following structure:
      | path                                              | state |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
      | tracks/20260419T1502_rpc_cf_wrong_state/           | plan  |
    And the track "20260419T1502_rpc_cf_wrong_state" has spec.md with content "# RPC CF Wrong State\n\nSpec body."
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path  | tracks/20260419T1502_rpc_cf_wrong_state |
      | actor_name     | Rpc-Reviewer-555002                     |
      | actor_type     | agent                                   |
      | actor_model    | claude-opus-4-7                         |
      | actor_provider | anthropic                               |
      | satisfaction   | address_in_next_step                    |
      | findings       | premature carry-forward                 |
    Then the complete RPC returns gRPC status "FAILED_PRECONDITION"
    And the complete RPC error message contains "wrong_state_for_complete"
