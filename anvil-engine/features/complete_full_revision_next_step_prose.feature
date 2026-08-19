Feature: Complete and Begin RPC next_step prose for the revision path
  The engine's next_step prose references the per-entry execution_route
  discriminator for the new spec_revision routing entries. Per spec R7.3:
  - complete(full_revision) → next_step names the spec_revision doer path.
  - begin on a spec_revision doer session → next_step names complete as follow-up.

  Scenario: complete(full_revision) response next_step references the spec_revision doer path
    Given a hearth directory with the following structure:
      | path                                              | state       |
      | proposals/20260411T2021_anvil_workflow_engine/     | active      |
      | tracks/20260419T1900_fr_next_step/                 | spec_review |
    And the track "20260419T1900_fr_next_step" has spec.md with content "# FR Next Step\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [FR Next Step](tracks/20260419T1900_fr_next_step/) — fr next step — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## plan
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

      - [FR Next Step](tracks/20260419T1900_fr_next_step/)
      """
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path        | tracks/20260419T1900_fr_next_step |
      | actor_name           | Reviewer-FR-NS-777777             |
      | actor_type           | agent                             |
      | actor_model          | claude-opus-4-7                   |
      | actor_provider       | anthropic                         |
      | satisfaction         | full_revision                     |
    Then the complete RPC response new_state is "spec_revision"
    And the complete RPC response next_step contains "execution_route"
    # Machine-derived generic next_step: the spec_revision doer state has a
    # single outgoing edge back to spec_review, so the guidance names `complete`
    # as the follow-up and the spec_review target (no hardcoded session-role
    # prose; the engine drives the lifecycle).
    And the complete RPC response next_step contains "complete"
    And the complete RPC response next_step contains "spec_review"

  Scenario: begin response for a spec_revision doer session next_step references complete
    Given a hearth directory with the following structure:
      | path                                              | state         |
      | proposals/20260411T2021_anvil_workflow_engine/     | active        |
      | tracks/20260419T1901_begin_rev_next_step/          | spec_revision |
    And the track "20260419T1901_begin_rev_next_step" has spec.md with content "# Begin Rev Next Step\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [begin rev next step](tracks/20260419T1901_begin_rev_next_step/) — begin rev next step

      ## plan
      """
    And a playbook hook body for the track hook "spec-revision.md" with content "# Spec Revision Context\n\nWill address / Acknowledged, not addressing."
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260419T1901_begin_rev_next_step" and session_role "creator"
    Then the begin RPC response state is "spec_revision"
    And the begin RPC response next_step contains "execution_route"
    And the begin RPC response next_step contains "complete"
