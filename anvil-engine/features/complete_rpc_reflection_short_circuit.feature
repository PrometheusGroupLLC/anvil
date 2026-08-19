Feature: Complete RPC — rejection paths do not produce reflection files
  Covers the engine seam for spec R10.1(i) and (j).
  For each Slice A rejection (wrong state, satisfaction out-of-scope, missing actor),
  the engine returns an error and no reflection subdirectory is created on disk.

  Scenario: Wrong state — doer-style call on spec_review produces no reflection file
    Given a hearth directory with the following structure:
      | path                                                   | state       |
      | proposals/20260411T2021_anvil_workflow_engine/         | active      |
      | tracks/20260420T0840_rpc_sc_state/                     | spec_review |
    And the track "20260420T0840_rpc_sc_state" has spec.md with content "# RPC SC State\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      ## spec_review

      - [RPC SC State](tracks/20260420T0840_rpc_sc_state/) — rpc sc state — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## plan

      ## implementing
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-04-20T00:00:00Z
      last_updated: 2026-04-20T00:00:00Z
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
      | artifact_path        | tracks/20260420T0840_rpc_sc_state |
      | actor_name           | Doer-840001                       |
      | actor_type           | agent                             |
      | actor_model          | claude-opus-4-7                   |
      | actor_provider       | anthropic                         |
      | actor_context_window | 200000                            |
      | actor_entrypoint     | claude-code                       |
      | reflection_notes     | irrelevant notes                  |
    Then the complete RPC returns gRPC status "FAILED_PRECONDITION"
    And the complete RPC error message contains "wrong_state_for_complete"
    And the hearth directory "tracks/20260420T0840_rpc_sc_state/spec_reflection" does not exist

  Scenario: Findings required — address_in_next_step without findings produces no reflection file
    Given a hearth directory with the following structure:
      | path                                                   | state       |
      | proposals/20260411T2021_anvil_workflow_engine/         | active      |
      | tracks/20260420T0841_rpc_sc_scope/                     | spec_review |
    And the track "20260420T0841_rpc_sc_scope" has spec.md with content "# RPC SC Scope\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      ## spec_review

      - [RPC SC Scope](tracks/20260420T0841_rpc_sc_scope/) — rpc sc scope — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## plan

      ## implementing
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-04-20T00:00:00Z
      last_updated: 2026-04-20T00:00:00Z
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
      | artifact_path        | tracks/20260420T0841_rpc_sc_scope |
      | actor_name           | Reviewer-841001                   |
      | actor_type           | agent                             |
      | actor_model          | claude-opus-4-7                   |
      | actor_provider       | anthropic                         |
      | actor_context_window | 200000                            |
      | actor_entrypoint     | claude-code                       |
      | satisfaction         | address_in_next_step              |
      | reflection_notes     | irrelevant notes                  |
    Then the complete RPC returns gRPC status "INVALID_ARGUMENT"
    And the complete RPC error message contains "findings_required_for_address_in_next_step"
    And the hearth directory "tracks/20260420T0841_rpc_sc_scope/spec_review_reflection" does not exist

  Scenario: Missing actor_name — produces no reflection file
    Given a hearth directory with the following structure:
      | path                                                   | state |
      | proposals/20260411T2021_anvil_workflow_engine/         | active |
      | tracks/20260420T0842_rpc_sc_actor/                     | spec   |
    And the track "20260420T0842_rpc_sc_actor" has spec.md with content "# RPC SC Actor\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [RPC SC Actor](tracks/20260420T0842_rpc_sc_actor/) — rpc sc actor — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## spec_review

      ## plan

      ## implementing
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-04-20T00:00:00Z
      last_updated: 2026-04-20T00:00:00Z
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
      | artifact_path        | tracks/20260420T0842_rpc_sc_actor |
      | actor_name           |                                   |
      | actor_type           | agent                             |
      | actor_model          | claude-opus-4-7                   |
      | actor_provider       | anthropic                         |
      | actor_context_window | 200000                            |
      | actor_entrypoint     | claude-code                       |
      | reflection_notes     | irrelevant notes                  |
    Then the complete RPC returns gRPC status "INVALID_ARGUMENT"
    And the complete RPC error message contains "actor_name_required"
    And the hearth directory "tracks/20260420T0842_rpc_sc_actor/spec_reflection" does not exist
