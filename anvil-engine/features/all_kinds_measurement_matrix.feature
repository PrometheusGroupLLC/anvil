Feature: All registered kinds have executable measurement coverage
  The measurement boundary must stay exhaustive as registered playbook kinds change.

  Scenario: Every in-boundary registered kind emits every measurement granularity
    Given the live registered-kind measurement matrix
    When every registered kind is exercised across its measurement boundary
    Then the measurement coverage report names every registered kind
    And every covered kind reports step, transition, and workflow measurement
    And every skipped kind is named with its asserted exclusion reason
    And no registered kind is silently omitted

  Scenario: Projection-only snapshots are outside the measurement boundary
    Given the live registered-kind measurement matrix
    When a projection-only snapshot is exercised
    Then no step, transition, or workflow measurement is emitted
    And the measurement coverage report names "spark" with reason "projection-only snapshots are outside the measurement boundary"

  Scenario: The matrix identifies an uncovered registered kind
    Given a completed registered-kind measurement matrix
    When the matrix observations for registered kind "learning" are removed
    Then the measurement coverage matrix fails naming kind "learning"
    And the failure names missing granularities "step, transition, workflow"
