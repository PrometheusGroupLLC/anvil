Feature: Complete with reflection_notes — end-to-end happy path
  Covers spec R10.1(a) and (b): when `reflection_notes` is non-empty the
  engine writes a reflection file and returns `reflection_path` in the
  response. Standard Slice A bookkeeping is unchanged.

  Scenario: Doer-complete on spec with reflection_notes — writes reflection file and returns path
    Given a hearth directory with the following structure:
      | path                                              | state |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
      | tracks/20260420T0500_reflection_doer_e2e/          | spec   |
    And the track "20260420T0500_reflection_doer_e2e" has spec.md with content "# Reflection Doer E2E\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Reflection Doer E2E](tracks/20260420T0500_reflection_doer_e2e/) — reflection doer e2e — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a complete tools/call is sent with:
      | artifact_path        | tracks/20260420T0500_reflection_doer_e2e                            |
      | actor_name           | Doer-Reflect-111111                                                 |
      | actor_type           | agent                                                               |
      | actor_model          | claude-opus-4-7                                                     |
      | actor_provider       | anthropic                                                           |
      | actor_context_window | 200000                                                              |
      | actor_entrypoint     | claude-code                                                         |
      | reflection_notes     | Surprised that the registry move landed before projection; noting for next pass. |
    Then the complete response new_state is "spec_review"
    And the complete response transition_at is non-empty
    And the complete response artifact_path is "tracks/20260420T0500_reflection_doer_e2e"
    And the complete response reflection_path contains "spec_reflection/"
    And the complete response reflection_path ends with "Doer-Reflect-111111.md"
    And the resolved state of "tracks/20260420T0500_reflection_doer_e2e" in the hearth is "spec_review"
    And a hearth transition event for "tracks/20260420T0500_reflection_doer_e2e" contains "to: spec_review"
    And a hearth transition event for "tracks/20260420T0500_reflection_doer_e2e" contains "role: spec"
    And a hearth transition event for "tracks/20260420T0500_reflection_doer_e2e" contains "actor: Doer-Reflect-111111"
    And the complete response reflection_path file contains "source_state: spec"
    And the complete response reflection_path file contains "actor: Doer-Reflect-111111"
    And the complete response reflection_path file does not contain "satisfaction:"
    And the complete response reflection_path file contains "Surprised that the registry move landed before projection"

  Scenario: Reviewer-complete with satisfied and reflection_notes — writes reflection file in spec_review_reflection/
    Given a hearth directory with the following structure:
      | path                                              | state       |
      | proposals/20260411T2021_anvil_workflow_engine/     | active      |
      | tracks/20260420T0501_reflection_reviewer_e2e/      | spec_review |
    And the track "20260420T0501_reflection_reviewer_e2e" has spec.md with content "# Reflection Reviewer E2E\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      ## spec_review

      - [Reflection Reviewer E2E](tracks/20260420T0501_reflection_reviewer_e2e/) — reflection reviewer e2e — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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

      - [Reflection Reviewer E2E](tracks/20260420T0501_reflection_reviewer_e2e/)

      ## Planned (0)

      ## Implementing (0)
      """
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a complete tools/call is sent with:
      | artifact_path        | tracks/20260420T0501_reflection_reviewer_e2e                        |
      | actor_name           | Reviewer-Reflect-222222                                             |
      | actor_type           | agent                                                               |
      | actor_model          | claude-opus-4-7                                                     |
      | actor_provider       | anthropic                                                           |
      | actor_context_window | 200000                                                              |
      | actor_entrypoint     | claude-code                                                         |
      | satisfaction         | satisfied                                                           |
      | reflection_notes     | Review felt like a grammar pass — nothing structural caught me.     |
    Then the complete response new_state is "plan"
    And the complete response transition_at is non-empty
    And the complete response artifact_path is "tracks/20260420T0501_reflection_reviewer_e2e"
    And the complete response reflection_path contains "spec_review_reflection/"
    And the complete response reflection_path ends with "Reviewer-Reflect-222222.md"
    And the complete response reflection_path file contains "source_state: spec_review"
    And the complete response reflection_path file contains "actor: Reviewer-Reflect-222222"
    And the complete response reflection_path file contains "satisfaction: satisfied"
    And the complete response reflection_path file contains "Review felt like a grammar pass"
    And the resolved state of "tracks/20260420T0501_reflection_reviewer_e2e" in the hearth is "plan"
