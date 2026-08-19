Feature: Begin RPC — doer-entry states return engine-native next_step
  When a doer re-enters a track at a doer-actionable state the engine's Begin
  RPC must return a non-empty next_step that tells the doer that the NEXT call
  is `complete` (which advances the state so the engine serves the next phase).
  The next_step must NOT instruct the doer to invoke a retired forge skill —
  the engine drives the lifecycle. Per the engine-driven pull model.

  Scenario: begin (resumer) on a track in plan returns a complete-oriented, skill-free next_step
    Given a hearth directory with the following structure:
      | path                                              | state  |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
      | tracks/20260620T0900_doer_entry_plan/              | plan   |
    And the track "20260620T0900_doer_entry_plan" has spec.md with content "# Doer Entry\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## plan

      - [Doer Entry](tracks/20260620T0900_doer_entry_plan/) — doer entry — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-06-20T00:00:00Z
      last_updated: 2026-06-20T00:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Planned (1)

      - [Doer Entry](tracks/20260620T0900_doer_entry_plan/)
      """
    And a playbook hook body for the track hook "plan-writing.md" with content "# Plan Writing Context\n\nWrite plan.md."
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260620T0900_doer_entry_plan" and session_role "resumer" and actor_name "Resumer-620001"
    Then the begin RPC response state is "plan"
    And the begin RPC response has non-empty next_step
    And the begin RPC response next_step contains "complete"
    And the begin RPC response next_step does not contain "forge:plan"
    And the begin RPC response next_step does not contain "forge:review"

  Scenario: begin (resumer) on a track in implementing returns a complete-oriented, skill-free next_step
    Given a hearth directory with the following structure:
      | path                                                 | state        |
      | proposals/20260411T2021_anvil_workflow_engine/        | active       |
      | tracks/20260620T0901_doer_entry_impl/                 | implementing |
    And the track "20260620T0901_doer_entry_impl" has spec.md with content "# Doer Entry\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## implementing

      - [Doer Entry](tracks/20260620T0901_doer_entry_impl/) — doer entry — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-06-20T00:00:00Z
      last_updated: 2026-06-20T00:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Implementing (1)

      - [Doer Entry](tracks/20260620T0901_doer_entry_impl/)
      """
    And a playbook hook body for the track hook "implementing.md" with content "# Implementing Context\n\nImplement per plan.md."
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260620T0901_doer_entry_impl" and session_role "resumer" and actor_name "Resumer-620001"
    Then the begin RPC response state is "implementing"
    And the begin RPC response has non-empty next_step
    And the begin RPC response next_step contains "complete"
    And the begin RPC response next_step does not contain "forge:implement"

  Scenario: begin (resumer) on a track in reflecting returns a complete-oriented, skill-free next_step
    Given a hearth directory with the following structure:
      | path                                                 | state      |
      | proposals/20260411T2021_anvil_workflow_engine/        | active     |
      | tracks/20260620T0902_doer_entry_reflect/              | reflecting |
    And the track "20260620T0902_doer_entry_reflect" has spec.md with content "# Doer Entry\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## reflecting

      - [Doer Entry](tracks/20260620T0902_doer_entry_reflect/) — doer entry — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-06-20T00:00:00Z
      last_updated: 2026-06-20T00:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Reflecting (1)

      - [Doer Entry](tracks/20260620T0902_doer_entry_reflect/)
      """
    And a playbook hook body for the track hook "reflecting.md" with content "# Reflecting Context\n\nWrite the reflection."
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260620T0902_doer_entry_reflect" and session_role "resumer" and actor_name "Resumer-620001"
    Then the begin RPC response state is "reflecting"
    And the begin RPC response has non-empty next_step
    And the begin RPC response next_step contains "complete"
    And the begin RPC response next_step does not contain "forge:reflect"
