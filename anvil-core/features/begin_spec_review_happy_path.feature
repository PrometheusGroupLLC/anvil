Feature: Begin Spec Review Happy Path
  When a reviewer session calls begin(identifier) on a track already in
  spec_review state, the handler returns a BeginResult with the expected
  state and context fields populated. Post-cutover (Slice A of
  spec_phase_complete_happy_path), begin(identifier, reviewer) is a
  context-delivery call only — no transition is recorded. The
  spec → spec_review edge is driven by the doer's `complete` call.

  Scenario: Reviewer begin on track in spec_review delivers context
    Given an in-memory query adapter with track "20260414T0405_review_spec_strand" in state "spec_review" and spec content "# Review Spec Strand\n\nExample spec body."
    When begin is called via query adapter with identifier "20260414T0405_review_spec_strand" and session_role "reviewer"
    Then the begin outcome result state is "spec_review"
    And the begin outcome result artifact_text contains "Example spec body"
    And the begin outcome result review_context_text contains "Spec Review Criteria"
    And the begin outcome result review_context_text contains "Dispatch to criteria"
    And the begin outcome result review_doc_path is set
    And the handler emitted a ReviewDocCreated event with doc name "spec.review.md"
    And the handler emitted a BeginMarkerWritten event with state "spec_review"
