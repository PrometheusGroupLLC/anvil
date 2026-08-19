Feature: Begin Spec Revision Not Ready For Review
  Slice B (R4.2 / R8.1(f)): a reviewer calling begin(identifier) on a track in
  `spec_revision` is rejected with `SpecNotReadyForReview` — the same error code
  as the `spec` case, with a distinct message naming the revision case. The
  reviewer must wait for the doer's `complete` call to advance the track back to
  `spec_review`.

  Scenario: Reviewer begin on track in spec_revision returns spec_not_ready_for_review with the revision message
    Given an in-memory query adapter with track "20260419T1000_revision_reviewer" in state "spec_revision" and spec content "# Revision Reviewer\n\nSpec body."
    When begin is called via query adapter with identifier "20260419T1000_revision_reviewer" and session_role "reviewer"
    Then the begin outcome is a SpecNotReadyForReview error
    And the begin outcome error message contains "spec_not_ready_for_review"
    And the begin outcome error message contains "revision"
    And the begin outcome error message contains "re-review"
    And the handler emitted no events
