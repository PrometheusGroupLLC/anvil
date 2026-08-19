Feature: Session Role Switch End-to-End
  Verifies that a second checkin replaces the prior session role,
  and begin(identifier) uses the latest (reviewer) role — not the
  original (creator) role.

  Scenario: checkin(creator) then checkin(reviewer) then begin(identifier) uses reviewer
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260411T2021_anvil_workflow_engine/    | active       |
      | tracks/20260414T0405_review_spec_strand/          | spec_review  |
    And the track "20260414T0405_review_spec_strand" has spec.md with content "# Review Spec Strand\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      ## spec_review

      - [review spec strand](tracks/20260414T0405_review_spec_strand/) — review spec strand

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

      ## Spec Review (1)

      | Track | Proposal |
      |-------|----------|
      | review spec strand | anvil-playbook-engine |

      ## Spec (0)
      """
    And a playbook hook body for the spec_review reviewer hook "spec-review.md" with content "Combined review protocol and spec criteria."
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a checkin tools/call is sent with role "creator" and:
      | field          | value           |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-6 |
      | actor_provider | anthropic       |
    When a checkin tools/call is sent with role "reviewer" and:
      | field          | value           |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-6 |
      | actor_provider | anthropic       |
    When a begin tools/call is sent with identifier "20260414T0405_review_spec_strand"
    Then the begin response has state "spec_review"
    And the hearth tracks.md contains "20260414T0405_review_spec_strand" under "## spec_review"
