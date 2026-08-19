Feature: Amendment reversal by identity — spec kind (AC-5)

  # reverse(log, op_id) removes the entry with that op_id; re-folding the result
  # returns the projection to its pre-op state. Pure function over the log;
  # reversal keys on op_id (KD-4).

  Background:
    Given a spec base document with elements:
      | id  | kind                 | body              |
      | AC1 | acceptance_criterion | X is observable.  |
      | AC2 | acceptance_criterion | X is reversible.  |

  Scenario: reversing a revise returns the projection to its pre-op state
    Given an empty op log
    And the log has a "revise" op "op-1" on "AC1" with body "changed body" accepted at "2026-06-01T00:00:00Z"
    When reverse is called on the log for op "op-1"
    And apply is called on the spec base and the reversed log
    Then apply succeeds
    And rendered element "AC1" has body "X is observable."

  Scenario: reversing a retire restores the omitted element
    Given an empty op log
    And the log has a "retire" op "op-r" on "AC2" accepted at "2026-06-01T00:00:00Z"
    When reverse is called on the log for op "op-r"
    And apply is called on the spec base and the reversed log
    Then apply succeeds
    And the rendered document has 2 elements
    And rendered element "AC2" has body "X is reversible."

  Scenario: reversing one op among several leaves the others applied
    Given an empty op log
    And the log has a "revise" op "op-1" on "AC1" with body "new AC1" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-2" on "AC2" with body "new AC2" accepted at "2026-06-02T00:00:00Z"
    When reverse is called on the log for op "op-1"
    And apply is called on the spec base and the reversed log
    Then apply succeeds
    And rendered element "AC1" has body "X is observable."
    And rendered element "AC2" has body "new AC2"
