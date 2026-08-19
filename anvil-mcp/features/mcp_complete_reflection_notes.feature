Feature: MCP complete tool — reflection_notes key-presence contract
  The MCP shim omits `reflection_path` from the JSON response when
  `reflection_notes` is absent or empty. When non-empty, the key is
  present with the file path. Covers the "absent vs empty" split at the
  wire layer (spec R3.2).

  Scenario: Non-empty reflection_notes on doer path — response has reflection_path key
    Given a hearth directory with the following structure:
      | path                                              | state |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
      | tracks/20260420T0600_shim_reflect_doer/            | spec   |
    And the track "20260420T0600_shim_reflect_doer" has spec.md with content "# Shim Reflect Doer\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Shim Reflect Doer](tracks/20260420T0600_shim_reflect_doer/) — shim reflect doer — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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
      | artifact_path        | tracks/20260420T0600_shim_reflect_doer |
      | actor_name           | ShimDoer-666666                        |
      | actor_type           | agent                                  |
      | actor_model          | claude-sonnet-4-6                      |
      | actor_provider       | anthropic                              |
      | actor_context_window | 200000                                 |
      | actor_entrypoint     | claude-code                            |
      | reflection_notes     | Notes from doer pass.                  |
    Then the complete response new_state is "spec_review"
    And the complete response has key "reflection_path"

  Scenario: Non-empty reflection_notes on reviewer satisfied path — response has reflection_path key
    Given a hearth directory with the following structure:
      | path                                              | state       |
      | proposals/20260411T2021_anvil_workflow_engine/     | active      |
      | tracks/20260420T0601_shim_reflect_reviewer/        | spec_review |
    And the track "20260420T0601_shim_reflect_reviewer" has spec.md with content "# Shim Reflect Reviewer\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      ## spec_review

      - [Shim Reflect Reviewer](tracks/20260420T0601_shim_reflect_reviewer/) — shim reflect reviewer — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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

      - [Shim Reflect Reviewer](tracks/20260420T0601_shim_reflect_reviewer/)

      ## Planned (0)

      ## Implementing (0)
      """
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a complete tools/call is sent with:
      | artifact_path        | tracks/20260420T0601_shim_reflect_reviewer |
      | actor_name           | ShimReviewer-777777                        |
      | actor_type           | agent                                      |
      | actor_model          | claude-sonnet-4-6                          |
      | actor_provider       | anthropic                                  |
      | actor_context_window | 200000                                     |
      | actor_entrypoint     | claude-code                                |
      | satisfaction         | satisfied                                  |
      | reflection_notes     | Notes from reviewer pass.                  |
    Then the complete response new_state is "plan"
    And the complete response has key "reflection_path"

  Scenario: reflection_notes omitted — response does NOT have reflection_path key
    Given a hearth directory with the following structure:
      | path                                              | state |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
      | tracks/20260420T0602_shim_no_reflect_omit/         | spec   |
    And the track "20260420T0602_shim_no_reflect_omit" has spec.md with content "# Shim No Reflect Omit\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Shim No Reflect Omit](tracks/20260420T0602_shim_no_reflect_omit/) — shim no reflect omit — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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
      | artifact_path        | tracks/20260420T0602_shim_no_reflect_omit |
      | actor_name           | ShimDoer-888888                           |
      | actor_type           | agent                                     |
      | actor_model          | claude-sonnet-4-6                         |
      | actor_provider       | anthropic                                 |
      | actor_context_window | 200000                                    |
      | actor_entrypoint     | claude-code                               |
    Then the complete response new_state is "spec_review"
    And the complete response does not have key "reflection_path"

  Scenario: reflection_notes is empty or whitespace-only — response does NOT have reflection_path key
    Given a hearth directory with the following structure:
      | path                                              | state |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
      | tracks/20260420T0603_shim_no_reflect_empty/        | spec   |
    And the track "20260420T0603_shim_no_reflect_empty" has spec.md with content "# Shim No Reflect Empty\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Shim No Reflect Empty](tracks/20260420T0603_shim_no_reflect_empty/) — shim no reflect empty — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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
      | artifact_path        | tracks/20260420T0603_shim_no_reflect_empty |
      | actor_name           | ShimDoer-999990                            |
      | actor_type           | agent                                      |
      | actor_model          | claude-sonnet-4-6                          |
      | actor_provider       | anthropic                                  |
      | actor_context_window | 200000                                     |
      | actor_entrypoint     | claude-code                                |
      | reflection_notes     |                                            |
    Then the complete response new_state is "spec_review"
    And the complete response does not have key "reflection_path"
