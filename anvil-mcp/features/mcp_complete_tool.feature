Feature: MCP complete tool — shim accepts call and relays structured response
  The MCP shim exposes a `complete` tool. When called, it forwards the
  caller-supplied identity verbatim (no session injection) and returns the
  structured `{new_state, transition_at, artifact_path}` response.

  Scenario: complete tool appears in tools/list
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260419T1200_mcp_shim_track/               | spec    |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a tools/list request is sent
    Then the tools list contains "complete"

  Scenario: Shim forwards complete call with caller-supplied identity verbatim
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260419T1201_shim_complete_fwd/            | spec    |
    And the track "20260419T1201_shim_complete_fwd" has spec.md with content "# Shim Complete Fwd\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Shim Complete Fwd](tracks/20260419T1201_shim_complete_fwd/) — shim complete fwd — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a complete tools/call is sent with:
      | artifact_path        | tracks/20260419T1201_shim_complete_fwd |
      | actor_name           | Shim-Doer-999999                       |
      | actor_type           | agent                                  |
      | actor_model          | claude-sonnet-4-6                      |
      | actor_provider       | anthropic                              |
      | actor_context_window | 200000                                 |
      | actor_entrypoint     | claude-code                            |
    Then the complete response new_state is "spec_review"
    And the complete response transition_at is non-empty
    And the complete response artifact_path is "tracks/20260419T1201_shim_complete_fwd"
    And a hearth transition event for "tracks/20260419T1201_shim_complete_fwd" contains "actor: Shim-Doer-999999"
    And the hearth file "tracks/20260419T1201_shim_complete_fwd/status.yaml" contains "model: claude-sonnet-4-6"
    And the hearth file "tracks/20260419T1201_shim_complete_fwd/status.yaml" contains "type: agent"
    And the hearth file "tracks/20260419T1201_shim_complete_fwd/status.yaml" contains "provider: anthropic"

  Scenario: Shim forwards satisfaction field verbatim on reviewer-path complete call
    Given a hearth directory with the following structure:
      | path                                              | state       |
      | proposals/20260411T2021_anvil_workflow_engine/     | active      |
      | tracks/20260419T1202_shim_satisfaction_fwd/        | spec_review |
    And the track "20260419T1202_shim_satisfaction_fwd" has spec.md with content "# Shim Satisfaction Fwd\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      ## spec_review

      - [Shim Satisfaction Fwd](tracks/20260419T1202_shim_satisfaction_fwd/) — shim satisfaction fwd — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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

      ## Spec Review (1)

      - [Shim Satisfaction Fwd](tracks/20260419T1202_shim_satisfaction_fwd/)

      ## Planned (0)

      ## Implementing (0)
      """
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a complete tools/call is sent with:
      | artifact_path        | tracks/20260419T1202_shim_satisfaction_fwd |
      | actor_name           | Shim-Reviewer-444444                       |
      | actor_type           | agent                                      |
      | actor_model          | claude-sonnet-4-6                          |
      | actor_provider       | anthropic                                  |
      | actor_context_window | 200000                                     |
      | actor_entrypoint     | claude-code                                |
      | satisfaction         | satisfied                                  |
    Then the complete response new_state is "plan"
    And the complete response transition_at is non-empty
    And the complete response artifact_path is "tracks/20260419T1202_shim_satisfaction_fwd"
    And a hearth transition event for "tracks/20260419T1202_shim_satisfaction_fwd" contains "role: review"
    And a hearth transition event for "tracks/20260419T1202_shim_satisfaction_fwd" contains "actor: Shim-Reviewer-444444"
