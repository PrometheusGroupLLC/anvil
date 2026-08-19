Feature: Begin plan (resumer) carry-forward delivery — end-to-end
  A plan-phase doer re-entering a `plan`-state track calls `checkin(resumer)`
  then `begin(identifier)` via MCP. When carry-forward.md is present, the begin
  response context_text contains the labeled carry-forward section with the
  verbatim findings. When absent, no section is delivered and no error occurs.

  Scenario: resumer begin on a plan track WITH carry-forward.md delivers the findings section
    Given a hearth directory with the following structure:
      | path                                              | state |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
      | tracks/20260419T2300_plan_resumer_with_cf/         | plan  |
    And the track "20260419T2300_plan_resumer_with_cf" has spec.md with content "# Plan Resumer With CF\n\nSpec body."
    And the track "20260419T2300_plan_resumer_with_cf" has a file "carry-forward.md" with content "---\nfindings_from: spec_review\nsatisfied_by: Reviewer-CF\nat: 2026-04-19T00:00:00Z\nsatisfaction: address_in_next_step\n---\n\n# Carry-forward from spec review\n\n_Reviewer: Reviewer-CF · 2026-04-19T00:00:00Z_\n\nConfirm the migration ordering during implementation."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## plan

      - [plan resumer with cf](tracks/20260419T2300_plan_resumer_with_cf/) — plan resumer with cf

      ## implementing
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-04-19T00:00:00Z
      last_updated: 2026-04-19T00:00:00Z
      after_event: "seed"
      ---

      # Anvil — State of Execution

      ## Planned (1)

      - [plan resumer with cf](tracks/20260419T2300_plan_resumer_with_cf/)
      """
    And a playbook hook body for the track hook "plan-writing.md" with content "PLAN-PREAMBLE"
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a checkin tools/call is sent with role "resumer" and:
      | field          | value           |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-6 |
      | actor_provider | anthropic       |
    When a begin tools/call is sent with identifier "20260419T2300_plan_resumer_with_cf"
    Then the begin response has state "plan"
    And the begin response has context text containing "## Carry-forward findings from spec review"
    And the begin response has context text containing "Confirm the migration ordering during implementation."

  Scenario: resumer begin on a plan track WITHOUT carry-forward.md delivers no section, no error
    Given a hearth directory with the following structure:
      | path                                              | state |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
      | tracks/20260419T2301_plan_resumer_no_cf/           | plan  |
    And the track "20260419T2301_plan_resumer_no_cf" has spec.md with content "# Plan Resumer No CF\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## plan

      - [plan resumer no cf](tracks/20260419T2301_plan_resumer_no_cf/) — plan resumer no cf

      ## implementing
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-04-19T00:00:00Z
      last_updated: 2026-04-19T00:00:00Z
      after_event: "seed"
      ---

      # Anvil — State of Execution

      ## Planned (1)

      - [plan resumer no cf](tracks/20260419T2301_plan_resumer_no_cf/)
      """
    And a playbook hook body for the track hook "plan-writing.md" with content "PLAN-PREAMBLE"
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a checkin tools/call is sent with role "resumer" and:
      | field          | value           |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-6 |
      | actor_provider | anthropic       |
    When a begin tools/call is sent with identifier "20260419T2301_plan_resumer_no_cf"
    Then the begin response has state "plan"
    And the begin response has context text containing "PLAN-PREAMBLE"
    And the begin response context text does not contain "## Carry-forward findings from spec review"
