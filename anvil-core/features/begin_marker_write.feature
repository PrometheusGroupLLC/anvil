Feature: Begin marker write on the reviewer flow
  When a reviewer session calls begin(identifier) on a track already in
  spec_review state, the handler now appends a begin-marker to the artifact's
  activity log in addition to scaffolding the review doc. The begin-marker
  records (actor, state, kind=begin); it is NOT a state transition. The
  spec → spec_review edge is still driven by the doer's `complete` call, so
  no ReviewTransition is emitted (BP1, AC-1).

  Scenario: Reviewer begin appends a begin-marker activity entry and records no transition
    Given an in-memory query adapter with track "20260414T0405_review_spec_strand" in state "spec_review" and spec content "# Review Spec Strand\n\nExample spec body."
    When begin is called via query adapter with identifier "20260414T0405_review_spec_strand" and session_role "reviewer"
    Then the begin outcome result state is "spec_review"
    And the handler emitted a BeginMarkerWritten event with state "spec_review"
    And the handler emitted a BeginMarkerWritten event with kind "begin"
    And the handler emitted a ReviewDocCreated event with doc name "spec.review.md"
    And the handler emitted no ReviewTransition event
