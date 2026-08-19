Feature: Full track lifecycle is engine-driven
  A track can move from spec through completed with begin and complete only.
  Every review gate now advances on an honest reviewer verdict —
  complete(satisfaction: "satisfied") — rather than an untyped snapshot escape
  hatch. Doer phases report engine support and no step exposes a fallback:forge
  playbook.

  Scenario: track reaches completed through engine begin and complete calls
    Given a hearth directory with the following structure:
      | path                                              | state |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
      | tracks/20260614T0640_full_engine_track/            | spec   |
    And the track "20260614T0640_full_engine_track" has spec.md with content "# Full Engine Track\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Full Engine Track](tracks/20260614T0640_full_engine_track/) — full engine track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## spec_review

      ## plan

      ## implementing

      ## reflecting

      ## completed
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

      ## Spec (1)

      - [Full Engine Track](tracks/20260614T0640_full_engine_track/)

      ## Spec Review (0)

      ## Planned (0)

      ## Implementing (0)

      ## Reflecting (0)

      ## Completed (0)
      """
    And a playbook hook body for the track hook "plan-writing.md" with content "FULL-LIFECYCLE-PLAN-HOOK"
    And a playbook hook body for the track hook "implementing.md" with content "FULL-LIFECYCLE-IMPLEMENT-HOOK"
    And a playbook hook body for the track hook "reflecting.md" with content "FULL-LIFECYCLE-REFLECT-HOOK"
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path  | tracks/20260614T0640_full_engine_track |
      | actor_name     | Doer-230001                            |
      | actor_type     | agent                                  |
      | actor_model    | test-model                             |
      | actor_provider | test                                   |
    Then the complete RPC response new_state is "spec_review"
    When the complete RPC is called with:
      | artifact_path  | tracks/20260614T0640_full_engine_track |
      | actor_name     | Reviewer-230002                        |
      | actor_type     | agent                                  |
      | actor_model    | test-model                             |
      | actor_provider | test                                   |
      | satisfaction   | satisfied                              |
    Then the complete RPC response new_state is "plan"
    When the begin RPC is called with identifier "20260614T0640_full_engine_track" and session_role "resumer"
    Then the begin RPC response state is "plan"
    And the begin RPC response "context_text" is exactly "FULL-LIFECYCLE-PLAN-HOOK"
    When the complete RPC is called with:
      | artifact_path  | tracks/20260614T0640_full_engine_track |
      | actor_name     | Doer-230003                            |
      | actor_type     | agent                                  |
      | actor_model    | test-model                             |
      | actor_provider | test                                   |
    Then the complete RPC response new_state is "plan_review"
    When the complete RPC is called with:
      | artifact_path  | tracks/20260614T0640_full_engine_track |
      | actor_name     | Reviewer-230004                        |
      | actor_type     | agent                                  |
      | actor_model    | test-model                             |
      | actor_provider | test                                   |
      | satisfaction   | satisfied                              |
    Then the complete RPC response new_state is "implementing"
    When the begin RPC is called with identifier "20260614T0640_full_engine_track" and session_role "resumer"
    Then the begin RPC response state is "implementing"
    And the begin RPC response "context_text" is exactly "FULL-LIFECYCLE-IMPLEMENT-HOOK"
    When the complete RPC is called with:
      | artifact_path  | tracks/20260614T0640_full_engine_track |
      | actor_name     | Doer-230005                            |
      | actor_type     | agent                                  |
      | actor_model    | test-model                             |
      | actor_provider | test                                   |
    Then the complete RPC response new_state is "impl_review"
    When the complete RPC is called with:
      | artifact_path  | tracks/20260614T0640_full_engine_track |
      | actor_name     | Reviewer-230006                        |
      | actor_type     | agent                                  |
      | actor_model    | test-model                             |
      | actor_provider | test                                   |
      | satisfaction   | satisfied                              |
    Then the complete RPC response new_state is "reflecting"
    When the begin RPC is called with identifier "20260614T0640_full_engine_track" and session_role "resumer"
    Then the begin RPC response state is "reflecting"
    And the begin RPC response "context_text" is exactly "FULL-LIFECYCLE-REFLECT-HOOK"
    When the complete RPC is called with:
      | artifact_path  | tracks/20260614T0640_full_engine_track |
      | actor_name     | Doer-230007                            |
      | actor_type     | agent                                  |
      | actor_model    | test-model                             |
      | actor_provider | test                                   |
    Then the complete RPC response new_state is "reflection_review"
    When the complete RPC is called with:
      | artifact_path  | tracks/20260614T0640_full_engine_track |
      | actor_name     | Reviewer-230008                        |
      | actor_type     | agent                                  |
      | actor_model    | test-model                             |
      | actor_provider | test                                   |
      | satisfaction   | satisfied                              |
    Then the complete RPC response new_state is "completed"
    And the resolved state of "tracks/20260614T0640_full_engine_track" in the hearth is "completed"
    And the hearth file "tracks/20260614T0640_full_engine_track/status.yaml" does not contain "fallback:forge:"
