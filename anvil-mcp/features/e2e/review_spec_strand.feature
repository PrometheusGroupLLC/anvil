Feature: Review Spec Strand End-to-End
  Full stack verification of the spec-phase cutover (Slice A of
  spec_phase_complete_happy_path): the doer's `complete` call drives
  the spec → spec_review transition, the reviewer's begin(identifier)
  delivers context (no transition), and the reviewer's
  complete(satisfied) drives spec_review → plan.

  Scenario: Doer completes spec, reviewer gets context, reviewer completes with satisfied
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/    | active  |
      | tracks/20260414T0405_review_spec_strand/          | spec    |
    And the track "20260414T0405_review_spec_strand" has spec.md with content "# Review Spec Strand\n\nExample spec body for E2E."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [review spec strand](tracks/20260414T0405_review_spec_strand/) — review spec strand — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## spec_review

      ## plan

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

      ## Planned (0)

      ## Spec Review (0)

      ## Spec (1)

      | Track | Proposal |
      |-------|----------|
      | review spec strand | anvil-playbook-engine |
      """
    And the hearth forward.md is seeded with:
      """
      # Anvil — Forward Projection

      seeded forward content; must not change.
      """
    And a playbook hook body for the spec_review reviewer hook "spec-review.md" with content "Combined review protocol and spec criteria."
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    # === Step 1: doer completes spec ===
    When a complete tools/call is sent with:
      | field          | value                                              |
      | artifact_path  | tracks/20260414T0405_review_spec_strand            |
      | actor_name     | DoerActor-E2E-100000                               |
      | actor_type     | agent                                              |
      | actor_model    | claude-opus-4-6                                    |
      | actor_provider | anthropic                                          |
    Then the complete response new_state is "spec_review"
    And the hearth tracks.md contains "20260414T0405_review_spec_strand" under "## spec_review"
    And the hearth tracks.md does not contain "20260414T0405_review_spec_strand" under "## spec"
    # === Step 2: reviewer checks in and begins to get context ===
    When a checkin tools/call is sent with role "reviewer" and:
      | field          | value           |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-6 |
      | actor_provider | anthropic       |
    Then the checkin response has a generated actor name
    And the checkin response includes artifact "20260414T0405_review_spec_strand" with state "spec_review"
    And the checkin response artifact "20260414T0405_review_spec_strand" has execution_route "engine"
    When a describe tools/call is sent with id "20260414T0405_review_spec_strand"
    Then the describe response has a non-empty next_step text
    When a begin tools/call is sent with:
      | field                | value                                   |
      | identifier           | 20260414T0405_review_spec_strand        |
      | actor_name           | ReviewerActor-E2E-200000                |
      | actor_type           | agent                                   |
      | actor_model          | claude-opus-4-6                         |
      | actor_provider       | anthropic                               |
    Then the begin response has state "spec_review"
    And the begin response has a track path
    And the begin response has artifact_text containing "Example spec body"
    And the begin response has review_context_text containing "Combined review protocol"
    And the begin response has a review_doc_path
    And the hearth has spec.review.md at track "20260414T0405_review_spec_strand" starting with:
      """
      # Review: review spec strand

      ## Round 1
      """
    # === Step 3: reviewer completes with satisfied ===
    When a complete tools/call is sent with:
      | field          | value                                              |
      | artifact_path  | tracks/20260414T0405_review_spec_strand            |
      | actor_name     | ReviewerActor-E2E-200000                           |
      | actor_type     | agent                                              |
      | actor_model    | claude-opus-4-6                                    |
      | actor_provider | anthropic                                          |
      | satisfaction   | satisfied                                          |
    Then the complete response new_state is "plan"
    And the hearth tracks.md contains "20260414T0405_review_spec_strand" under "## plan"
    And the hearth tracks.md does not contain "20260414T0405_review_spec_strand" under "## spec_review"
    And the hearth execution.md contains "review spec strand" in the "Planned" section
    And the hearth forward.md is unchanged
