Feature: Complete RPC drives track doer-resume phases
  Doer complete advances plan, implementing, and reflecting to their review
  gates without fallback.

  Scenario: doer complete from plan advances to plan_review
    Given a hearth directory with the following structure:
      | path                                              | state  |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
      | tracks/20260614T0630_complete_plan/                | plan   |
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## plan

      - [Doer Resume](tracks/20260614T0630_complete_plan/) — doer resume — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## plan_review
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

      - [Doer Resume](tracks/20260614T0630_complete_plan/)
      """
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path        | tracks/20260614T0630_complete_plan |
      | actor_name           | Doer-220001                        |
      | actor_type           | agent                              |
      | actor_model          | test-model                         |
      | actor_provider       | test                               |
      | actor_context_window | 200000                             |
      | actor_entrypoint     | codex                              |
    Then the complete RPC response new_state is "plan_review"
    And the resolved state of "tracks/20260614T0630_complete_plan" in the hearth is "plan_review"
    And a hearth transition event for "tracks/20260614T0630_complete_plan" contains "role: plan"

  Scenario: doer complete from implementing advances to impl_review
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260411T2021_anvil_workflow_engine/     | active       |
      | tracks/20260614T0631_complete_implement/           | implementing |
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## implementing

      - [Doer Resume](tracks/20260614T0631_complete_implement/) — doer resume — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## impl_review
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

      ## Implementing (1)

      - [Doer Resume](tracks/20260614T0631_complete_implement/)
      """
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path        | tracks/20260614T0631_complete_implement |
      | actor_name           | Doer-220001                             |
      | actor_type           | agent                                   |
      | actor_model          | test-model                              |
      | actor_provider       | test                                    |
      | actor_context_window | 200000                                  |
      | actor_entrypoint     | codex                                   |
    Then the complete RPC response new_state is "impl_review"
    And the resolved state of "tracks/20260614T0631_complete_implement" in the hearth is "impl_review"
    And a hearth transition event for "tracks/20260614T0631_complete_implement" contains "role: implement"

  Scenario: doer complete from reflecting advances to reflection_review
    Given a hearth directory with the following structure:
      | path                                              | state      |
      | proposals/20260411T2021_anvil_workflow_engine/     | active     |
      | tracks/20260614T0632_complete_reflect/             | reflecting |
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## reflecting

      - [Doer Resume](tracks/20260614T0632_complete_reflect/) — doer resume — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## reflection_review
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

      ## Reflecting (1)

      - [Doer Resume](tracks/20260614T0632_complete_reflect/)
      """
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path        | tracks/20260614T0632_complete_reflect |
      | actor_name           | Doer-220001                           |
      | actor_type           | agent                                 |
      | actor_model          | test-model                            |
      | actor_provider       | test                                  |
      | actor_context_window | 200000                                |
      | actor_entrypoint     | codex                                 |
    Then the complete RPC response new_state is "reflection_review"
    And the resolved state of "tracks/20260614T0632_complete_reflect" in the hearth is "reflection_review"
    And a hearth transition event for "tracks/20260614T0632_complete_reflect" contains "role: reflect"
