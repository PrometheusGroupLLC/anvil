Feature: Complete RPC happy path
  A doer calling Complete on a `spec` track advances it to `spec_review`.
  The response carries new_state, transition_at, and artifact_path.
  The engine writes status.yaml, tracks.md, and execution.md side effects.

  Scenario: Doer complete on spec track via RPC — advances to spec_review
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260419T1100_rpc_complete_track/           | spec    |
    And the track "20260419T1100_rpc_complete_track" has spec.md with content "# RPC Complete Track\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [RPC Complete Track](tracks/20260419T1100_rpc_complete_track/) — rpc complete track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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
      | artifact_path        | tracks/20260419T1100_rpc_complete_track |
      | actor_name           | Rpc-Doer-111111                         |
      | actor_type           | agent                                   |
      | actor_model          | claude-opus-4-7                         |
      | actor_provider       | anthropic                               |
      | actor_context_window | 200000                                  |
      | actor_entrypoint     | claude-code                             |
    Then the complete RPC response new_state is "spec_review"
    And the complete RPC response transition_at matches "^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$"
    And the complete RPC response artifact_path is "tracks/20260419T1100_rpc_complete_track"
    And the resolved state of "tracks/20260419T1100_rpc_complete_track" in the hearth is "spec_review"
    And a hearth transition event for "tracks/20260419T1100_rpc_complete_track" contains "to: spec_review"
    And a hearth transition event for "tracks/20260419T1100_rpc_complete_track" contains "role: spec"
    And a hearth transition event for "tracks/20260419T1100_rpc_complete_track" contains "actor: Rpc-Doer-111111"
    And the hearth file "tracks.md" contains "## spec_review"
    And the hearth file "tracks.md" does not contain "20260419T1100_rpc_complete_track" under section "## spec"
    And the hearth file "projections/execution.md" contains "## Spec Review (1)"

  Scenario: Doer complete with review hook fixture places registry entry under spec_review
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/    | active  |
      | tracks/20260414T0405_review_spec_strand/          | spec    |
    And the track "20260414T0405_review_spec_strand" has spec.md with content "# Review Spec Strand\n\nExample spec body for E2E."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [review spec strand](tracks/20260414T0405_review_spec_strand/) — review spec strand — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## spec_review

      ## plan

      ## completed
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-04-14T00:00:00Z
      last_updated: 2026-04-14T00:00:00Z
      after_event: "seed"
      ---

      # Anvil — State of Execution

      ## Planned (0)

      ## Spec Review (0)

      ## Spec (1)

      | Track | Proposal |
      |-------|----------|
      | review spec strand | anvil-playbook-engine |
      """
    And a playbook hook body for the spec_review reviewer hook "spec-review.md" with content "Combined review protocol and spec criteria."
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path  | tracks/20260414T0405_review_spec_strand |
      | actor_name     | DoerActor-E2E-100000                    |
      | actor_type     | agent                                   |
      | actor_model    | claude-opus-4-6                         |
      | actor_provider | anthropic                               |
    Then the complete RPC response new_state is "spec_review"
    And the hearth file "tracks.md" contains "20260414T0405_review_spec_strand" under section "## spec_review"
    And the hearth file "tracks.md" does not contain "20260414T0405_review_spec_strand" under section "## spec"
    And the hearth file "projections/execution.md" contains "## Spec Review (1)"
