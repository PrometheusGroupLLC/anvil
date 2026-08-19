Feature: Snapshot RPC surfaces the begin-adoption soft-warn (BP2)
  An actor transitioning a driven track via Snapshot with no matching
  open begin-marker still records the transition AND the engine surfaces
  the begin-adoption warning in SnapshotResponse.warnings (field 6). (AC-4)

  Scenario: Snapshot with no begin-marker warns and still transitions
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260604T1200_snap_warn/                   | spec    |
    And the track "20260604T1200_snap_warn" has spec.md with content "# Snap Warn\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Snap Warn](tracks/20260604T1200_snap_warn/) — snap warn — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## spec_review
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
      """
    And the engine is started with that hearth
    When the snapshot RPC is called with:
      | artifact_path        | tracks/20260604T1200_snap_warn |
      | to_state             | spec_review                    |
      | actor_name           | Rpc-Reviewer-600002            |
      | actor_role           | review                         |
      | actor_type           | agent                          |
      | actor_model          | claude-opus-4-7                |
      | actor_provider       | anthropic                      |
      | actor_context_window | 1000000                        |
      | actor_sdk_version    | 0.2.111                        |
      | actor_entrypoint     | claude-desktop                 |
    Then the snapshot RPC response success is "true"
    And the snapshot RPC response status_updated is "true"
    And the snapshot RPC response warnings contain "begin_adoption: actor Rpc-Reviewer-600002 transitioned tracks/20260604T1200_snap_warn in state spec without a prior begin"
    And the resolved state of "tracks/20260604T1200_snap_warn" in the hearth is "spec_review"
