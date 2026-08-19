Feature: Describe Playbook Type
  Describe the playbook artifact type to understand creation requirements.
  The playbook kind is the 6th creation-surface kind; its parent is a track.

  # R1 AC: describe(playbook) returns creation schema with required fields, parent kind, description.

  Scenario: Playbook type returns required fields and parent type
    Given a describe handler with type schemas
    When describe is called with identifier "playbook"
    Then the describe result is type info
    And the describe type name is "playbook"
    And the describe type has required field "playbook_name"
    And the describe type has required field "parent_id"
    And the describe type has required field "approver"
    And the describe type parent type is "track"

  Scenario: Playbook type description names the kind's purpose
    Given a describe handler with type schemas
    When describe is called with identifier "playbook"
    Then the describe result is type info
    And the describe type description contains "lifecycle"
