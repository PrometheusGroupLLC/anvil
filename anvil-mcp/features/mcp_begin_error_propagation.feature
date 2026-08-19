Feature: MCP begin error propagation
  The MCP shim preserves the engine's structured error message
  end-to-end so the agent can extract the fallback skill name and
  route accordingly.

  Scenario: ModeNotImplemented surfaces with forge:implement intact in JSON
    Given a hearth directory with the following structure:
      | path                                              | state         |
      | proposals/20260411T2021_anvil_workflow_engine/     | active        |
      | tracks/20260414T0405_review_spec_strand/           | completed     |
    And a context file "spec-review.md" in the hearth with content "review protocol"
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a checkin tools/call is sent with role "resumer" and:
      | field          | value           |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-6 |
      | actor_provider | anthropic       |
    When a begin tools/call is sent with identifier "20260414T0405_review_spec_strand"
    Then the MCP response is a tool error containing "forge:implement"

  # proposal is now engine-driven: a reviewer begin on a non-gate state (draft) no
  # longer surfaces the forge:review fallback error; the engine resolves the proposal
  # machine and returns the current state. (Error propagation for genuinely
  # unsupported combos is still covered by the ModeNotImplemented case above.)
  Scenario: Reviewer begin on engine-driven proposal-in-draft returns the state
    Given a hearth directory with the following structure:
      | path                                              | state  |
      | proposals/20260411T2021_anvil_workflow_engine/     | draft  |
    And a context file "spec-review.md" in the hearth with content "review protocol"
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a checkin tools/call is sent with role "reviewer" and:
      | field          | value           |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-6 |
      | actor_provider | anthropic       |
    When a begin tools/call is sent with identifier "20260411T2021_anvil_workflow_engine"
    Then the begin response has state "draft"
