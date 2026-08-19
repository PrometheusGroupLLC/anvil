Feature: Begin Review Session Error End-to-End
  Verifies that begin(identifier) without a prior checkin returns a
  structured session error through the full MCP stack.

  Scenario: Begin with identifier but no checkin returns session error
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/    | active  |
      | tracks/20260414T0405_review_spec_strand/          | spec    |
    And the track "20260414T0405_review_spec_strand" has spec.md with content "# spec body"
    And a context file "spec-review.md" in the hearth with content "review protocol."
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a begin tools/call is sent with identifier "20260414T0405_review_spec_strand"
    Then the MCP response is a tool error containing "No active session"
