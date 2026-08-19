Feature: Begin Error — Role/State Mismatch
  begin(identifier) with session_role=creator returns RoleStateMismatch
  because creator operates on artifact types, not ids.

  Scenario: Creator with identifier returns RoleStateMismatch
    Given an in-memory query adapter with track "20260414T0405_review_spec_strand" in state "spec" and spec content "body"
    When begin is called via query adapter with identifier "20260414T0405_review_spec_strand" and session_role "creator"
    Then the begin outcome is a RoleStateMismatch error for role "creator"
    And the handler emitted no events
