Feature: Begin Error — Not Found
  begin(identifier) for a nonexistent artifact returns NotFound.

  Scenario: Nonexistent identifier returns NotFound
    Given an empty in-memory query adapter
    When begin is called via query adapter with identifier "nonexistent_track_id" and session_role "reviewer"
    Then the begin outcome is a NotFound error for "nonexistent_track_id"
    And the handler emitted no events
