Feature: Snapshot End-to-End
  The MCP shim starts the engine, routes a `snapshot` tools/call through
  gRPC, and the engine writes status.yaml, tracks.md, and execution.md
  on the real hearth filesystem. Per spec R3 of the
  checkin_backfill_spec_context track, the caller supplies `actor_name`
  explicitly; the engine echoes it to the transition record.

  Scope note: this scenario exercises the add-if-absent leg of the
  uniform actor-write rule (the seeded status.yaml has no existing
  actors-table). Match-no-op and mismatch-append are covered at the FS
  seam by `anvil-core/features/actor_write_fs_three_leg.feature`.

  Scenario: Track spec → spec_review full-stack side effects on the hearth
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260417T2200_e2e_track/                   | spec    |
    And the track "20260417T2200_e2e_track" has spec.md with content "# E2E Track\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [E2E Track](tracks/20260417T2200_e2e_track/) — e2e track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## spec_review
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-04-16T00:00:00Z
      last_updated: 2026-04-16T00:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Spec (0)

      ## Spec Review (0)
      """
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a snapshot tools/call is sent with:
      | artifact_path        | tracks/20260417T2200_e2e_track |
      | to_state             | spec_review                    |
      | actor_name           | E2E-Reviewer-100               |
      | actor_role           | review                         |
      | actor_type           | agent                          |
      | actor_model          | claude-opus-4-7                |
      | actor_provider       | anthropic                      |
      | actor_context_window | 1000000                        |
      | actor_sdk_version    | 0.2.111                        |
      | actor_entrypoint     | claude-desktop                 |
    Then the snapshot response actor_name is "E2E-Reviewer-100"
    And the snapshot response projections_updated contains "execution.md"
    # Filesystem side effects — spec AC 16 deliverable proof
    And the resolved state of "tracks/20260417T2200_e2e_track" in the hearth is "spec_review"
    And a hearth transition event for "tracks/20260417T2200_e2e_track" contains "to: spec_review"
    And a hearth transition event for "tracks/20260417T2200_e2e_track" contains "role: review"
    And the hearth file "tracks/20260417T2200_e2e_track/status.yaml" contains "model: claude-opus-4-7"
    And the hearth file "tracks/20260417T2200_e2e_track/status.yaml" contains "provider: anthropic"
    And the hearth file "tracks.md" contains "## spec"
    And the hearth file "projections/execution.md" contains "## Spec Review (1)"
    And the hearth file "projections/execution.md" contains "E2E Track"
