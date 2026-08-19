Feature: Begin Validation
  The begin handler validates creation parameters before creating artifacts.

  Scenario: Unsupported artifact type returns error
    Given an in-memory query adapter with parent "20260411T2021_anvil_workflow_engine" in state "active"
    When begin is called via query adapter with artifact_type "unknown_type" and parent "20260411T2021_anvil_workflow_engine"
    Then the begin outcome is an UnsupportedType error for "unknown_type"
    And the handler emitted no events

  Scenario: Nonexistent parent returns error
    Given an in-memory query adapter with no parents
    When begin is called via query adapter with parent "nonexistent_proposal"
    Then the begin outcome is a ParentNotFound error for "nonexistent_proposal"
    And the handler emitted no events

  Scenario: Inactive parent returns error
    Given an in-memory query adapter with parent "20260411T2021_anvil_workflow_engine" in state "draft"
    When begin is called via query adapter with parent "20260411T2021_anvil_workflow_engine"
    Then the begin outcome is a ParentNotActive error for "20260411T2021_anvil_workflow_engine"
    And the handler emitted no events
