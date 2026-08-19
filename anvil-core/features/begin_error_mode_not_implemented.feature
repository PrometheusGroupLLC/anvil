Feature: Begin Error — Mode Not Implemented
  begin(identifier) with session_role=resumer on a non-doer state returns
  ModeNotImplemented naming forge:implement as the fallback skill.

  Scenario: Resumer with identifier returns ModeNotImplemented
    Given an in-memory query adapter with track "20260414T0405_review_spec_strand" in state "plan_review" and spec content "body"
    When begin is called via query adapter with identifier "20260414T0405_review_spec_strand" and session_role "resumer"
    Then the begin outcome is a ModeNotImplemented error naming "forge:implement"
    And the begin outcome error message contains "forge:implement"
    And the handler emitted no events
