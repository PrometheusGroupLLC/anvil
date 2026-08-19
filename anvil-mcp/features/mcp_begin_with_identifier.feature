Feature: MCP begin accepts identifier and surfaces review payload
  The MCP shim accepts the begin tool call with an identifier argument,
  populates session_role from the prior checkin, and surfaces the
  engine's review payload (artifact_text, review_context_text,
  review_doc_path) in the JSON response.

  Scenario: Reviewer begin(identifier) surfaces review fields
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260411T2021_anvil_workflow_engine/     | active       |
      | tracks/20260414T0405_review_spec_strand/           | spec_review  |
    And the track "20260414T0405_review_spec_strand" has spec.md with content "# Spec body"
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      ## spec_review

      - [review spec strand](tracks/20260414T0405_review_spec_strand/) — review spec strand
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
    And a playbook hook body for the spec_review reviewer hook "spec-review.md" with content "review protocol"
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a checkin tools/call is sent with role "reviewer" and:
      | field          | value           |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-6 |
      | actor_provider | anthropic       |
    When a begin tools/call is sent with identifier "20260414T0405_review_spec_strand"
    Then the begin response has state "spec_review"
    And the begin response has artifact_text containing "Spec body"
    And the begin response has review_context_text containing "review protocol"
    And the begin response has a review_doc_path
