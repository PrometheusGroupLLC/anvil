Feature: Amendment apply — spec kind (AC-3)

  # apply(schema, base, log) is a pure fold: base + an op log ordered by
  # accept-timestamp (FIFO tie-break on insertion seq) → rendered projection.
  # No file or prose input. Retire omits the element from the rendered body but
  # the Retire op stays in the log (KD-4). A Revise can target an element added
  # by a prior Add in the same log (dynamic element set).

  Background:
    Given a spec base document with elements:
      | id  | kind                 | body              |
      | R1  | requirement          | The system shall X. |
      | AC1 | acceptance_criterion | X is observable.  |
      | AC2 | acceptance_criterion | X is reversible.  |

  Scenario: revise an AC body
    Given an empty op log
    And the log has a "revise" op "op-1" on "AC1" with body "X is observable in the UI." accepted at "2026-06-01T00:00:00Z"
    When apply is called on the spec base and log
    Then apply succeeds
    And the rendered document has 3 elements
    And rendered element "AC1" has body "X is observable in the UI."

  Scenario: retire an element omits it from the rendered body but keeps the op in the log
    Given an empty op log
    And the log has a "retire" op "op-1" on "AC2" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the spec base and log
    Then apply succeeds
    And the rendered document has 2 elements
    And the rendered document does not contain element "AC2"
    And the op log still contains op "op-1"

  Scenario: add an AC after an anchor
    Given an empty op log
    And the log has an "add" op "op-1" minting "AC3" of kind "acceptance_criterion" with body "X is idempotent." anchored "after:AC1" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the spec base and log
    Then apply succeeds
    And the rendered document has 4 elements
    And rendered element at position 2 has id "AC3"

  Scenario: two ops with out-of-order accept timestamps apply in timestamp order
    Given an empty op log
    And the log has a "revise" op "op-late" on "AC1" with body "second wins" accepted at "2026-06-02T00:00:00Z"
    And the log has a "revise" op "op-early" on "AC1" with body "first" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the spec base and log
    Then apply succeeds
    And rendered element "AC1" has body "second wins"

  Scenario: two ops with equal accept timestamps apply in FIFO insertion-seq order
    Given an empty op log
    And the log has a "revise" op "op-a" on "AC1" with body "first pushed" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-b" on "AC1" with body "second pushed wins" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the spec base and log
    Then apply succeeds
    And rendered element "AC1" has body "second pushed wins"

  Scenario: a Revise targets an element introduced by a prior Add in the same log
    Given an empty op log
    And the log has an "add" op "op-add" minting "AC3" of kind "acceptance_criterion" with body "added body" anchored "at_end" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-rev" on "AC3" with body "revised after add" accepted at "2026-06-02T00:00:00Z"
    When apply is called on the spec base and log
    Then apply succeeds
    And the rendered document has 4 elements
    And rendered element "AC3" has body "revised after add"
