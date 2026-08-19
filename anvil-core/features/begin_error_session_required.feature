Feature: Begin Error — Session Required
  begin(identifier) without a session_role returns SessionRequired.

  Scenario: Empty session_role with identifier returns SessionRequired
    Given an in-memory query adapter with track "20260414T0405_review_spec_strand" in state "spec" and spec content "body"
    When begin is called via query adapter with identifier "20260414T0405_review_spec_strand" and session_role ""
    Then the begin outcome is a SessionRequired error
    And the begin outcome error message contains "checkin"
    And the handler emitted no events
