Feature: Begin Spec Revision Doer Happy Path
  When a doer session (wire role "creator") calls begin(identifier) on a track
  in spec_revision state, the handler returns revision context from the
  (spec_revision, doer) hook (spec-revision.md), a computed review_doc_path, and
  an empty artifact_text — and records NO state transition. Per spec R2.

  Scenario: Doer (creator) begin on track in spec_revision delivers revision context, no transition
    Given an in-memory query adapter with track "20260419T0900_revision_doer" in state "spec_revision" seeded with the revision hook
    When begin is called via query adapter with identifier "20260419T0900_revision_doer" and session_role "creator"
    Then the begin outcome result state is "spec_revision"
    And the begin outcome result context_text contains "Will address"
    And the begin outcome result context_text contains "Acknowledged"
    And the begin outcome result context_text contains "complete"
    And the begin outcome result artifact_text is empty
    And the begin outcome result review_doc_path is set
    And the handler did not emit a ReviewDocCreated event
    And the handler emitted a BeginMarkerWritten event with state "spec_revision"

  Scenario: Doer (creator) begin on a spec track still returns RoleStateMismatch (no regression)
    Given an in-memory query adapter with track "20260419T0901_creator_on_spec" in state "spec" and spec content "# Creator On Spec\n\nSpec body."
    When begin is called via query adapter with identifier "20260419T0901_creator_on_spec" and session_role "creator"
    Then the begin outcome is a RoleStateMismatch error for role "creator"
