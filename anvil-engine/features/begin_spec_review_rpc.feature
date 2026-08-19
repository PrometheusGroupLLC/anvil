Feature: Begin Spec Review RPC
  The engine's Begin RPC accepts identifier + session_role for a
  reviewer on a track already in spec_review, returning a BeginResponse
  with artifact_text, review_context_text, and review_doc_path populated.
  Post-cutover (Slice A of spec_phase_complete_happy_path): this is a
  context-delivery call only — no transition is recorded.

  Scenario: begin(identifier, reviewer) on track in spec_review returns full review payload
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260411T2021_anvil_workflow_engine/     | active       |
      | tracks/20260414T0405_review_spec_strand/           | spec_review  |
    And the track "20260414T0405_review_spec_strand" has spec.md with content "# Review Spec Strand\n\nSpec body."
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
    And a playbook hook body for the spec_review reviewer hook "spec-review.md" with content "Combined review protocol and spec criteria."
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260414T0405_review_spec_strand" and session_role "reviewer"
    Then the begin RPC response state is "spec_review"
    And the begin RPC response has non-empty "artifact_text"
    And the begin RPC response has non-empty "review_context_text"
    And the begin RPC response has non-empty "review_doc_path"
