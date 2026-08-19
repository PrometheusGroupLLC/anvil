Feature: Complete RPC surfaces the begin-adoption soft-warn (BP2)
  An actor completing a driven track with no matching open begin-marker
  still records the transition (soft, non-blocking) AND the engine
  surfaces the begin-adoption warning in CompleteResponse.warnings
  (proto field 7). (AC-3)

  Scenario: Doer complete with no begin-marker warns and still transitions
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260604T1100_warn_track/                  | spec    |
    And the track "20260604T1100_warn_track" has spec.md with content "# Warn Track\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Warn Track](tracks/20260604T1100_warn_track/) — warn track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## spec_review

      ## plan
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-06-04T00:00:00Z
      last_updated: 2026-06-04T00:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Spec (0)

      ## Spec Review (0)

      ## Planned (0)
      """
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path        | tracks/20260604T1100_warn_track |
      | actor_name           | Rpc-Doer-600001                 |
      | actor_type           | agent                           |
      | actor_model          | claude-opus-4-7                 |
      | actor_provider       | anthropic                       |
      | actor_context_window | 200000                          |
      | actor_entrypoint     | claude-code                     |
    Then the complete RPC response new_state is "spec_review"
    And the complete RPC response warnings contain "begin_adoption: actor Rpc-Doer-600001 transitioned tracks/20260604T1100_warn_track in state spec without a prior begin"
    And the resolved state of "tracks/20260604T1100_warn_track" in the hearth is "spec_review"
