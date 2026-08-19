Feature: MCP surfaces engine identity-required errors end-to-end
  Per spec R2/R3 of the checkin_backfill_spec_context track, `begin`
  and `snapshot` require non-empty `actor_name` and `actor_*` runtime
  params on every call. When any required field is empty the engine
  returns INVALID_ARGUMENT and the shim surfaces it as an MCP tool
  error. The shim does not synthesize identity from session state —
  the engine's error reaches the caller regardless of whether a prior
  `checkin` was made.

  Scenario: Begin without actor_name surfaces actor_name_required
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260414T0405_review_spec_strand/           | spec    |
    And a context file "spec-review.md" in the hearth with content "review protocol"
    And the track "20260414T0405_review_spec_strand" has spec.md with content "# Body"
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Review Spec Strand](tracks/20260414T0405_review_spec_strand/) — review spec strand — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## spec_review
      """
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a checkin tools/call is sent with role "reviewer" and:
      | field          | value           |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    And a begin tools/call is sent with:
      | field          | value                            |
      | identifier     | 20260414T0405_review_spec_strand |
      | actor_name     |                                  |
      | actor_type     | agent                            |
      | actor_model    | claude-opus-4-7                  |
      | actor_provider | anthropic                        |
    Then the MCP response is a tool error containing "actor_name"

  Scenario: Begin without actor_type surfaces actor_params_required
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260414T0405_review_spec_strand/           | spec    |
    And a context file "spec-review.md" in the hearth with content "review protocol"
    And the track "20260414T0405_review_spec_strand" has spec.md with content "# Body"
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Review Spec Strand](tracks/20260414T0405_review_spec_strand/) — review spec strand — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## spec_review
      """
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a checkin tools/call is sent with role "reviewer" and:
      | field          | value           |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    And a begin tools/call is sent with:
      | field          | value                            |
      | identifier     | 20260414T0405_review_spec_strand |
      | actor_name     | Reviewer-111                     |
      | actor_type     |                                  |
      | actor_model    | claude-opus-4-7                  |
      | actor_provider | anthropic                        |
    Then the MCP response is a tool error containing "actor_type"

  Scenario: Snapshot without actor_name surfaces actor_name_required (no prior checkin)
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260417T2400_snap_track/                  | spec    |
    And the track "20260417T2400_snap_track" has spec.md with content "# Snap Track"
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Snap Track](tracks/20260417T2400_snap_track/) — snap track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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

      ## Spec Review (0)
      """
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a snapshot tools/call is sent with:
      | artifact_path | tracks/20260417T2400_snap_track |
      | to_state      | spec_review                     |
      | actor_role    | review                          |
      | actor_type    | agent                           |
      | actor_model   | claude-opus-4-7                 |
      | actor_provider| anthropic                       |
    Then the MCP response is a tool error containing "actor_name"

  Scenario: Snapshot without actor_name surfaces actor_name_required (even with a prior checkin session)
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260417T2400_snap_track/                  | spec    |
    And the track "20260417T2400_snap_track" has spec.md with content "# Snap Track"
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Snap Track](tracks/20260417T2400_snap_track/) — snap track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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

      ## Spec Review (0)
      """
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a checkin tools/call is sent with role "reviewer" and:
      | field          | value           |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    And a snapshot tools/call is sent with:
      | artifact_path | tracks/20260417T2400_snap_track |
      | to_state      | spec_review                     |
      | actor_role    | review                          |
      | actor_type    | agent                           |
      | actor_model   | claude-opus-4-7                 |
      | actor_provider| anthropic                       |
    Then the MCP response is a tool error containing "actor_name"

  Scenario: Snapshot without actor_provider surfaces actor_params_required
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260417T2400_snap_track/                  | spec    |
    And the track "20260417T2400_snap_track" has spec.md with content "# Snap Track"
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Snap Track](tracks/20260417T2400_snap_track/) — snap track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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

      ## Spec Review (0)
      """
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a snapshot tools/call is sent with:
      | artifact_path | tracks/20260417T2400_snap_track |
      | to_state      | spec_review                     |
      | actor_role    | review                          |
      | actor_name    | Snapper-111                     |
      | actor_type    | agent                           |
      | actor_model   | claude-opus-4-7                 |
      | actor_provider|                                 |
    Then the MCP response is a tool error containing "actor_provider"
