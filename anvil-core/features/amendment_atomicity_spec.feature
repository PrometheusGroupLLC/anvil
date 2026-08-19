Feature: Amendment apply atomicity — spec kind (AC-2)

  # Model: validate-the-whole-log-then-apply. apply validates every op against
  # the element set as of its position in ordered(); if any op is invalid the
  # whole batch is rejected with a typed error and the document is left
  # unchanged (the mutating fold never begins).

  Background:
    Given a spec base document with elements:
      | id  | kind                 | body              |
      | R1  | requirement          | The system shall X. |
      | AC1 | acceptance_criterion | X is observable.  |
      | AC2 | acceptance_criterion | X is reversible.  |

  Scenario: a 3-op log with op 2 invalid rejects the whole batch and leaves the doc unchanged
    Given an empty op log
    And the log has a "revise" op "op-1" on "AC1" with body "valid revise" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-2" on "R99" with body "targets undeclared" accepted at "2026-06-02T00:00:00Z"
    And the log has a "revise" op "op-3" on "AC2" with body "also valid" accepted at "2026-06-03T00:00:00Z"
    When apply is called on the spec base and log
    Then apply fails with code "amendment_unknown_element"
    When render is called on the spec base and an empty log
    Then the rendered document equals the base document
