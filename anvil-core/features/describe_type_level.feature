Feature: Describe Type Level
  Describe an artifact type to understand creation requirements.

  Scenario: Track type returns required fields and parent type
    Given a describe handler with type schemas
    When describe is called with identifier "track"
    Then the describe result is type info
    And the describe type name is "track"
    And the describe type has required field "name"
    And the describe type has required field "parent_id"
    And the describe type has required field "approver"
    And the describe type parent type is "proposal"

  Scenario: Proposal type has no parent
    Given a describe handler with type schemas
    When describe is called with identifier "proposal"
    Then the describe result is type info
    And the describe type name is "proposal"
    And the describe type has required field "name"
    And the describe type parent type is ""

  Scenario: Unknown type returns error
    Given a describe handler with type schemas
    When describe is called with identifier "widget"
    Then the describe result is an UnknownIdentifier error
