Feature: Begin RPC resumer on plan delivers carry-forward findings
  Slice C consumer side (R4): a resumer begin on a plan-state track appends a
  labeled carry-forward section to context_text when carry-forward.md is present,
  carrying the verbatim findings forward. Absent the file, context_text is
  delivered normally with no section and no error. The file is read fresh from
  disk on every begin (no cache) and is never deleted or marked consumed.

  Scenario: resumer begin on plan WITH carry-forward.md appends the findings section (R4.1)
    Given a hearth directory with the following structure:
      | path                                              | state  |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
      | tracks/20260614T0720_plan_with_cf/                 | plan   |
    And the track "20260614T0720_plan_with_cf" has spec.md with content "# Plan With CF\n\nSpec body."
    And the track "20260614T0720_plan_with_cf" has a file "carry-forward.md" with content "---\nfindings_from: spec_review\nsatisfied_by: Reviewer-CF\nat: 2026-06-14T00:00:00Z\nsatisfaction: address_in_next_step\n---\n\n# Carry-forward from spec review\n\n_Reviewer: Reviewer-CF · 2026-06-14T00:00:00Z_\n\nConfirm the migration ordering and the index rebuild during implementation."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## plan

      - [Plan With CF](tracks/20260614T0720_plan_with_cf/) — plan with cf — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## implementing
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-06-14T00:00:00Z
      last_updated: 2026-06-14T00:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Planned (1)

      - [Plan With CF](tracks/20260614T0720_plan_with_cf/)
      """
    And a playbook hook body for the track hook "plan-writing.md" with content "PLAN-PREAMBLE"
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260614T0720_plan_with_cf" and session_role "resumer" and actor_name "Resumer-720001"
    Then the begin RPC response state is "plan"
    And the begin RPC response "context_text" contains "## Carry-forward findings from spec review"
    And the begin RPC response "context_text" contains "Confirm the migration ordering and the index rebuild during implementation."
    And the begin RPC response "context_text" contains "PLAN-PREAMBLE"

  Scenario: resumer begin on plan WITHOUT carry-forward.md delivers no section and no error (R4.2)
    Given a hearth directory with the following structure:
      | path                                              | state  |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
      | tracks/20260614T0721_plan_no_cf/                   | plan   |
    And the track "20260614T0721_plan_no_cf" has spec.md with content "# Plan No CF\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## plan

      - [Plan No CF](tracks/20260614T0721_plan_no_cf/) — plan no cf — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## implementing
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-06-14T00:00:00Z
      last_updated: 2026-06-14T00:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Planned (1)

      - [Plan No CF](tracks/20260614T0721_plan_no_cf/)
      """
    And a playbook hook body for the track hook "plan-writing.md" with content "PLAN-PREAMBLE"
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260614T0721_plan_no_cf" and session_role "resumer" and actor_name "Resumer-721001"
    Then the begin RPC response state is "plan"
    And the begin RPC response "context_text" is exactly "PLAN-PREAMBLE"
    And the begin RPC response "context_text" does not contain "## Carry-forward findings from spec review"

  Scenario: resumer begin reads carry-forward.md fresh on each call (R4.5 — no cache)
    Given a hearth directory with the following structure:
      | path                                              | state  |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
      | tracks/20260614T0722_plan_fresh/                   | plan   |
    And the track "20260614T0722_plan_fresh" has spec.md with content "# Plan Fresh\n\nSpec body."
    And the track "20260614T0722_plan_fresh" has a file "carry-forward.md" with content "---\nfindings_from: spec_review\nsatisfied_by: Reviewer-CF\nat: 2026-06-14T00:00:00Z\nsatisfaction: address_in_next_step\n---\n\n# Carry-forward from spec review\n\n_Reviewer: Reviewer-CF · 2026-06-14T00:00:00Z_\n\nBODY-A original findings"
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## plan

      - [Plan Fresh](tracks/20260614T0722_plan_fresh/) — plan fresh — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## implementing
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-06-14T00:00:00Z
      last_updated: 2026-06-14T00:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Planned (1)

      - [Plan Fresh](tracks/20260614T0722_plan_fresh/)
      """
    And a playbook hook body for the track hook "plan-writing.md" with content "PLAN-PREAMBLE"
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260614T0722_plan_fresh" and session_role "resumer" and actor_name "Resumer-722001"
    Then the begin RPC response "context_text" contains "BODY-A original findings"
    When the carry-forward file for "20260614T0722_plan_fresh" is overwritten with "---\nfindings_from: spec_review\nsatisfied_by: Reviewer-CF\nat: 2026-06-14T00:00:00Z\nsatisfaction: address_in_next_step\n---\n\n# Carry-forward from spec review\n\n_Reviewer: Reviewer-CF · 2026-06-14T00:00:00Z_\n\nBODY-B corrected findings"
    And the begin RPC is called with identifier "20260614T0722_plan_fresh" and session_role "resumer" and actor_name "Resumer-722001"
    Then the begin RPC response "context_text" contains "BODY-B corrected findings"
    And the begin RPC response "context_text" does not contain "BODY-A original findings"
