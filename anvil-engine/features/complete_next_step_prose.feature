Feature: Complete and Begin RPC next_step prose
  The engine's Complete and Begin RPC responses carry a `next_step` field
  that references the per-entry `execution_route` discriminator for the
  new spec-phase routing entries (per spec R5.3 and plan L4 disposition).
  These scenarios prevent silent regression on the prose when the routing
  table or proto evolves.

  Scenario: Complete response for doer on spec track carries next_step referencing execution_route
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260419T1500_next_step_doer/               | spec    |
    And the track "20260419T1500_next_step_doer" has spec.md with content "# Next Step Doer\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Next Step Doer](tracks/20260419T1500_next_step_doer/) — next step doer — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path        | tracks/20260419T1500_next_step_doer |
      | actor_name           | Doer-NextStep-111111                |
      | actor_type           | agent                               |
      | actor_model          | claude-sonnet-4-6                   |
      | actor_provider       | anthropic                           |
      | actor_context_window | 200000                              |
      | actor_entrypoint     | claude-code                         |
    Then the complete RPC response new_state is "spec_review"
    And the complete RPC response next_step contains "execution_route"
    And the complete RPC response next_step contains "complete"

  Scenario: Complete response for reviewer on spec_review track carries next_step referencing execution_route
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260411T2021_anvil_workflow_engine/     | active       |
      | tracks/20260419T1501_next_step_reviewer/           | spec_review  |
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      ## spec_review

      - [Next Step Reviewer](tracks/20260419T1501_next_step_reviewer/) — next step reviewer

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
      | next step reviewer |  |

      ## Planned (0)

      ## Implementing (0)
      """
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path        | tracks/20260419T1501_next_step_reviewer |
      | actor_name           | Reviewer-NextStep-222222               |
      | actor_type           | agent                                  |
      | actor_model          | claude-sonnet-4-6                      |
      | actor_provider       | anthropic                              |
      | actor_context_window | 200000                                 |
      | actor_entrypoint     | claude-code                            |
      | satisfaction         | satisfied                              |
    Then the complete RPC response new_state is "plan"
    And the complete RPC response next_step contains "execution_route"
    And the complete RPC response next_step contains "plan"

  Scenario: Begin response for reviewer on spec_review track carries next_step referencing complete and execution_route
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260411T2021_anvil_workflow_engine/     | active       |
      | tracks/20260419T1502_begin_next_step/              | spec_review  |
    And the track "20260419T1502_begin_next_step" has spec.md with content "# Begin Next Step\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      ## spec_review

      - [Begin Next Step](tracks/20260419T1502_begin_next_step/) — begin next step

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
      | begin next step |  |

      ## Planned (0)

      ## Implementing (0)
      """
    And a playbook hook body for the spec_review reviewer hook "spec-review.md" with content "Combined review protocol and spec criteria."
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260419T1502_begin_next_step" and session_role "reviewer"
    Then the begin RPC response state is "spec_review"
    And the begin RPC response next_step contains "execution_route"
    And the begin RPC response next_step contains "complete"

  # Regression guard per spec R8 and plan Phase 3 Task 6: reflection_notes is
  # an input-shape addition, not a playbook. The next_step field must NOT advertise
  # reflection_notes. Adding reflection_notes to a call must leave next_step unchanged.
  Scenario: Complete response with reflection_notes carries unchanged next_step (no reflection_notes mention)
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260420T1503_next_step_reflect_notes/      | spec    |
    And the track "20260420T1503_next_step_reflect_notes" has spec.md with content "# Next Step Reflect Notes\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Next Step Reflect Notes](tracks/20260420T1503_next_step_reflect_notes/) — next step reflect notes — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path        | tracks/20260420T1503_next_step_reflect_notes |
      | actor_name           | Doer-ReflectNotes-333333                     |
      | actor_type           | agent                                        |
      | actor_model          | claude-sonnet-4-6                            |
      | actor_provider       | anthropic                                    |
      | actor_context_window | 200000                                       |
      | actor_entrypoint     | claude-code                                  |
      | reflection_notes     | A note that should not affect next_step.     |
    Then the complete RPC response new_state is "spec_review"
    And the complete RPC response next_step contains "execution_route"
    And the complete RPC response next_step contains "complete"
    And the complete RPC response next_step does not contain "reflection_notes"
