Feature: Begin Spec Not Ready For Review
  Post-cutover (Slice A of spec_phase_complete_happy_path): a reviewer
  calling begin(identifier) on a track still in `spec` is rejected with
  `SpecNotReadyForReview`. The spec → spec_review transition is driven
  by the doer's `complete` call (no satisfaction); the reviewer must
  wait for that before entering.

  Scenario: Reviewer begin on track in spec returns spec_not_ready_for_review
    Given an in-memory query adapter with track "20260414T0405_review_spec_strand" in state "spec" and spec content "# Review Spec Strand\n\nSpec body."
    When begin is called via query adapter with identifier "20260414T0405_review_spec_strand" and session_role "reviewer"
    Then the begin outcome is a SpecNotReadyForReview error
    And the begin outcome error message contains "spec_not_ready_for_review"
    And the begin outcome error message contains "complete"
    And the handler emitted no events
