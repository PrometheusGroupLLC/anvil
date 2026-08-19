Feature: Begin Review on an engine-driven proposal non-gate state End-to-End
  proposal is now engine-driven (register:free lifecycle). A reviewer begin(identifier)
  on a proposal in a non-review-gate state (draft) no longer falls back to forge:review;
  the engine resolves the proposal machine and returns the current state with no
  reviewer-actionable context. (The forge:review fallback now applies only to kinds
  still outside engine support, e.g. learning.)

  Scenario: Reviewer begin on engine-driven proposal-in-draft returns the state
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/    | draft   |
    And a context file "spec-review.md" in the hearth with content "review protocol."
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
