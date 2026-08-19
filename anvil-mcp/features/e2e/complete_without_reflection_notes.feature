Feature: Complete without reflection_notes — no-op path
  Covers spec R10.1(c) and (d): when `reflection_notes` is absent or
  empty/whitespace, no reflection file is written and the response does
  not contain a `reflection_path` key.

  Scenario: reflection_notes omitted — no reflection file, no reflection_path key in response
    Given a hearth directory with the following structure:
      | path                                              | state |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
      | tracks/20260420T0510_no_reflection_omitted_e2e/    | spec   |
    And the track "20260420T0510_no_reflection_omitted_e2e" has spec.md with content "# No Reflection Omitted E2E\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [No Reflection Omitted E2E](tracks/20260420T0510_no_reflection_omitted_e2e/) — no reflection omitted e2e — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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
      | artifact_path        | tracks/20260420T0510_no_reflection_omitted_e2e |
      | actor_name           | Doer-NoReflect-333333                          |
      | actor_type           | agent                                          |
      | actor_model          | claude-opus-4-7                                |
      | actor_provider       | anthropic                                      |
      | actor_context_window | 200000                                         |
      | actor_entrypoint     | claude-code                                    |
    Then the complete response new_state is "spec_review"
    And the complete response does not have key "reflection_path"
    And the hearth directory "tracks/20260420T0510_no_reflection_omitted_e2e/spec_reflection" does not exist

  Scenario: reflection_notes is empty string — no reflection file, no reflection_path key in response
    Given a hearth directory with the following structure:
      | path                                              | state |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
      | tracks/20260420T0511_no_reflection_empty_e2e/      | spec   |
    And the track "20260420T0511_no_reflection_empty_e2e" has spec.md with content "# No Reflection Empty E2E\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [No Reflection Empty E2E](tracks/20260420T0511_no_reflection_empty_e2e/) — no reflection empty e2e — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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
      | artifact_path        | tracks/20260420T0511_no_reflection_empty_e2e |
      | actor_name           | Doer-NoReflect-444444                        |
      | actor_type           | agent                                        |
      | actor_model          | claude-opus-4-7                              |
      | actor_provider       | anthropic                                    |
      | actor_context_window | 200000                                       |
      | actor_entrypoint     | claude-code                                  |
      | reflection_notes     |                                              |
    Then the complete response new_state is "spec_review"
    And the complete response does not have key "reflection_path"
    And the hearth directory "tracks/20260420T0511_no_reflection_empty_e2e/spec_reflection" does not exist

  Scenario: reflection_notes is whitespace-only (blank cell) — treated identically to empty, no reflection file
    Given a hearth directory with the following structure:
      | path                                              | state |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
      | tracks/20260420T0512_no_reflection_whitespace_e2e/ | spec   |
    And the track "20260420T0512_no_reflection_whitespace_e2e" has spec.md with content "# No Reflection Whitespace E2E\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [No Reflection Whitespace E2E](tracks/20260420T0512_no_reflection_whitespace_e2e/) — no reflection whitespace e2e — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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
      | artifact_path        | tracks/20260420T0512_no_reflection_whitespace_e2e |
      | actor_name           | Doer-NoReflect-555555                             |
      | actor_type           | agent                                             |
      | actor_model          | claude-opus-4-7                                   |
      | actor_provider       | anthropic                                         |
      | actor_context_window | 200000                                            |
      | actor_entrypoint     | claude-code                                       |
      | reflection_notes     |                                                   |
    Then the complete response new_state is "spec_review"
    And the complete response does not have key "reflection_path"
    And the hearth directory "tracks/20260420T0512_no_reflection_whitespace_e2e/spec_reflection" does not exist
