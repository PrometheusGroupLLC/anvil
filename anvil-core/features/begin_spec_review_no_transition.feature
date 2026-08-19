Feature: Begin Spec Review No Transition Regression Guard
  Post-cutover (Slice A of spec_phase_complete_happy_path), calling
  begin(identifier) as a reviewer on a track already in `spec_review`
  delivers context only — no ReviewTransition event is emitted.
  This is the R8.1(g) regression guard: if someone re-adds
  transition-recording logic to the reviewer `begin` path, this
  scenario will fail.

  Scenario: Reviewer begin on spec_review delivers context but emits no ReviewTransition
    Given an in-memory query adapter with track "20260414T0405_review_spec_strand" in state "spec_review" and spec content "# Review Spec Strand\n\nExample spec body."
    When begin is called via query adapter with identifier "20260414T0405_review_spec_strand" and session_role "reviewer"
    Then the begin outcome result state is "spec_review"
    And the begin outcome result artifact_text contains "Example spec body"
    And the begin outcome result review_context_text contains "Spec Review Criteria"
    And the begin outcome result review_doc_path is set
    And the handler emitted no ReviewTransition event
