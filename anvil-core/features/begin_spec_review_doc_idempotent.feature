Feature: Begin Spec Review Document Is Idempotent
  When a reviewer begins on a track already in spec_review where
  spec.review.md already exists, the handler does NOT overwrite the
  existing document. No transition is recorded (post-cutover:
  context-delivery only); the idempotency guarantee applies to the
  scaffold step. The ReviewDocCreated event is still emitted because
  events describe decisions, not outcomes.

  Scenario: Review doc creation is skipped when the file already exists
    Given an in-memory query adapter with track "20260414T0405_review_spec_strand" in state "spec_review" and spec content "body" and pre-existing review doc
    When begin is called via query adapter with identifier "20260414T0405_review_spec_strand" and session_role "reviewer"
    Then the begin outcome result state is "spec_review"
    And the begin outcome result review_doc_path is set
    And the handler emitted a ReviewDocCreated event with doc name "spec.review.md"
