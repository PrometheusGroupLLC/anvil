Feature: Complete RPC — reflection_notes written end-to-end via gRPC
  Covers the engine seam: reflection_notes is threaded from the proto request
  through to the domain handler and the resulting file is created on disk.
  reflection_path is returned in the CompleteResponse.

  Scenario: Doer-complete with reflection_notes via RPC — reflection file written, reflection_path in response
    Given a hearth directory with the following structure:
      | path                                              | state |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
      | tracks/20260420T1200_rpc_reflect_doer/             | spec   |
    And the track "20260420T1200_rpc_reflect_doer" has spec.md with content "# RPC Reflect Doer\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [RPC Reflect Doer](tracks/20260420T1200_rpc_reflect_doer/) — rpc reflect doer — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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
      | artifact_path        | tracks/20260420T1200_rpc_reflect_doer |
      | actor_name           | Rpc-Reflect-111111                    |
      | actor_type           | agent                                 |
      | actor_model          | claude-opus-4-7                       |
      | actor_provider       | anthropic                             |
      | actor_context_window | 200000                                |
      | actor_entrypoint     | claude-code                           |
      | reflection_notes     | Engine-level doer reflection note.    |
    Then the complete RPC response new_state is "spec_review"
    And the complete RPC response reflection_path contains "spec_reflection/"
    And the complete RPC response reflection_path ends with "Rpc-Reflect-111111.md"
    And the resolved state of "tracks/20260420T1200_rpc_reflect_doer" in the hearth is "spec_review"
    And the hearth directory "tracks/20260420T1200_rpc_reflect_doer/spec_reflection" exists

  Scenario: Reviewer-complete satisfied with reflection_notes via RPC — reflection file in spec_review_reflection/
    Given a hearth directory with the following structure:
      | path                                              | state       |
      | proposals/20260411T2021_anvil_workflow_engine/     | active      |
      | tracks/20260420T1201_rpc_reflect_reviewer/         | spec_review |
    And the track "20260420T1201_rpc_reflect_reviewer" has spec.md with content "# RPC Reflect Reviewer\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      ## spec_review

      - [RPC Reflect Reviewer](tracks/20260420T1201_rpc_reflect_reviewer/) — rpc reflect reviewer — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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

      ## Spec Review (1)

      - [RPC Reflect Reviewer](tracks/20260420T1201_rpc_reflect_reviewer/)

      ## Planned (0)

      ## Implementing (0)
      """
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path        | tracks/20260420T1201_rpc_reflect_reviewer |
      | actor_name           | Rpc-Reviewer-222222                       |
      | actor_type           | agent                                     |
      | actor_model          | claude-opus-4-7                           |
      | actor_provider       | anthropic                                 |
      | actor_context_window | 200000                                    |
      | actor_entrypoint     | claude-code                               |
      | satisfaction         | satisfied                                 |
      | reflection_notes     | Engine-level reviewer reflection note.    |
    Then the complete RPC response new_state is "plan"
    And the complete RPC response reflection_path contains "spec_review_reflection/"
    And the complete RPC response reflection_path ends with "Rpc-Reviewer-222222.md"
    And the resolved state of "tracks/20260420T1201_rpc_reflect_reviewer" in the hearth is "plan"
    And the hearth directory "tracks/20260420T1201_rpc_reflect_reviewer/spec_review_reflection" exists
