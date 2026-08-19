Feature: Begin Error — Invalid Argument
  Setting both artifact_type and identifier (or neither) returns
  InvalidArgument with a message naming the conflict.

  Scenario: Both artifact_type and identifier set returns InvalidArgument
    Given an empty in-memory query adapter
    When begin is called via query adapter with both artifact_type "track" and identifier "20260414T0405_review_spec_strand"
    Then the begin outcome is an InvalidArgument error
    And the begin outcome error message contains "both"
    And the handler emitted no events

  Scenario: Neither artifact_type nor identifier returns InvalidArgument
    Given an empty in-memory query adapter
    When begin is called via query adapter with neither artifact_type nor identifier
    Then the begin outcome is an InvalidArgument error
    And the begin outcome error message contains "neither"
    And the handler emitted no events
