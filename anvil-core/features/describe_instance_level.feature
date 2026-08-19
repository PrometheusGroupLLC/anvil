Feature: Describe Instance Level
  Describe an existing artifact to see its state and available actions.

  Scenario: Track in spec state returns state and available actions
    Given a describe handler with instances:
      | id                                      | kind  | state | transitions |
      | 20260413T1349_checkin_decomposition      | track | spec  | 1           |
    When describe is called with identifier "20260413T1349_checkin_decomposition"
    Then the describe result is instance info
    And the describe instance state is "spec"
    And the describe instance transition count is 1
    And the describe instance available actions include "spec_review" with role "spec"

  Scenario: Track in spec_review state shows the reviewer satisfied path
    Given a describe handler with instances:
      | id                                      | kind  | state       | transitions |
      | 20260413T1349_checkin_decomposition      | track | spec_review | 2           |
    When describe is called with identifier "20260413T1349_checkin_decomposition"
    Then the describe result is instance info
    And the describe instance state is "spec_review"
    And the describe instance available actions include "spec_revision" with role "reviewer"
    And the describe instance available actions include "plan" with role "reviewer"

  Scenario: Track in implementing state shows review options
    Given a describe handler with instances:
      | id                                      | kind  | state        | transitions |
      | 20260413T1349_checkin_decomposition      | track | implementing | 5           |
    When describe is called with identifier "20260413T1349_checkin_decomposition"
    Then the describe result is instance info
    And the describe instance state is "implementing"
    And the describe instance available actions include "impl_phase_review" with role "reviewer"
    And the describe instance available actions include "impl_review" with role "implement"

  Scenario: Unknown instance returns error
    Given a describe handler with instances:
      | id                                      | kind  | state | transitions |
      | 20260413T1349_checkin_decomposition      | track | spec  | 1           |
    When describe is called with identifier "nonexistent_artifact"
    Then the describe result is an UnknownIdentifier error
