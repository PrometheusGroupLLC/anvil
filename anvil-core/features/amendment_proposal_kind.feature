Feature: Amendment — proposal kind (AC-1, AC-3, AC-4, AC-5, Phase-4)

  # The proposal content-element schema declares:
  #   - problem    (singleton)   — the motivating problem statement
  #   - approach   (singleton)   — the proposed solution approach
  #   - slice      (slice-<N>)   — an incremental delivery slice
  #   - risk       (risk-<slug>) — an identified risk
  # Legal ops: Revise for singletons; Add/Revise/Retire/Reorder for list kinds.
  # All core functions (validate/apply/conflict/reverse) are reused without
  # modification; the only new deliverable is seeds/proposal.rs (KD-3).

  # ------------------------------------------------------------------
  # AC-1: Schema validation — proposal kind
  # ------------------------------------------------------------------

  Scenario: a legal Revise op on the problem singleton validates OK
    Given a proposal base document with elements:
      | id      | kind    | body                              |
      | problem | problem | Agents lack structured amendments |
    When validate_op is called for a "revise" op targeting "problem"
    Then validation succeeds

  Scenario: an op kind not in the proposal schema is rejected (Retire on singleton)
    Given a proposal base document with elements:
      | id      | kind    | body                              |
      | problem | problem | Agents lack structured amendments |
    When validate_op is called for a "retire" op targeting "problem"
    Then validation fails with code "amendment_op_not_in_schema"

  Scenario: an op targeting an undeclared element ID is rejected
    Given a proposal base document with elements:
      | id      | kind    | body    |
      | problem | problem | Problem |
    When validate_op is called for a "revise" op targeting "risk-foo"
    Then validation fails with code "amendment_unknown_element"

  Scenario: an Add minting a duplicate element ID is rejected
    Given a proposal base document with elements:
      | id       | kind  | body       |
      | slice-1  | slice | First slice |
    When validate_op is called for an "add" op minting "slice-1" of kind "slice" anchored "at_end"
    Then validation fails with code "amendment_duplicate_element_id"

  # ------------------------------------------------------------------
  # AC-3: Apply — proposal kind
  # ------------------------------------------------------------------

  Background:
    Given a proposal base document with elements:
      | id        | kind     | body                                |
      | problem   | problem  | Agents lack structured amendments   |
      | approach  | approach | Build a per-kind semantic diff core |
      | slice-1   | slice    | Core framework + spec kind          |
      | slice-2   | slice    | Fan-out to remaining kinds          |
      | risk-perf | risk     | Schema resolution overhead          |

  Scenario: revise the approach singleton
    Given an empty op log
    And the log has a "revise" op "op-1" on "approach" with body "Build a pure fold over an op log" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the proposal base and log
    Then apply succeeds
    And the rendered document has 5 elements
    And rendered element "approach" has body "Build a pure fold over an op log"

  Scenario: add a slice anchored after slice-1
    Given an empty op log
    And the log has an "add" op "op-1" minting "slice-3" of kind "slice" with body "B5b — engine wiring" anchored "after:slice-2" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the proposal base and log
    Then apply succeeds
    And the rendered document has 6 elements
    And rendered element at position 4 has id "slice-3"

  Scenario: retire a risk omits it from rendered body but retains the op in the log
    Given an empty op log
    And the log has a "retire" op "op-r" on "risk-perf" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the proposal base and log
    Then apply succeeds
    And the rendered document has 4 elements
    And the rendered document does not contain element "risk-perf"
    And the op log still contains op "op-r"

  Scenario: a Revise targets a slice added by a prior Add in the same log (dynamic element set)
    Given an empty op log
    And the log has an "add" op "op-add" minting "slice-3" of kind "slice" with body "initial" anchored "at_end" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-rev" on "slice-3" with body "revised slice body" accepted at "2026-06-02T00:00:00Z"
    When apply is called on the proposal base and log
    Then apply succeeds
    And the rendered document has 6 elements
    And rendered element "slice-3" has body "revised slice body"

  # ------------------------------------------------------------------
  # AC-4: Conflict detection — proposal kind
  # ------------------------------------------------------------------

  Scenario: two ops on the same slice target_id conflict
    Given an empty op log
    And the log has a "revise" op "op-1" on "slice-1" with body "first revision" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-2" on "slice-1" with body "second revision" accepted at "2026-06-02T00:00:00Z"
    When detect_conflicts is called on the log
    Then the conflicts contain target "slice-1"
    And the conflicts count is 1

  Scenario: a Retire and Revise on the same risk ID conflict
    Given an empty op log
    And the log has a "retire" op "op-1" on "risk-perf" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-2" on "risk-perf" with body "revised risk" accepted at "2026-06-02T00:00:00Z"
    When detect_conflicts is called on the log
    Then the conflicts contain target "risk-perf"
    And the conflicts count is 1

  # ------------------------------------------------------------------
  # AC-5: Reversal — proposal kind
  # ------------------------------------------------------------------

  Scenario: reversing a revise on approach returns projection to pre-op state
    Given an empty op log
    And the log has a "revise" op "op-1" on "approach" with body "mutated approach" accepted at "2026-06-01T00:00:00Z"
    When reverse is called on the log for op "op-1"
    And apply is called on the proposal base and the reversed log
    Then apply succeeds
    And rendered element "approach" has body "Build a per-kind semantic diff core"

  Scenario: reversing a retire on a slice restores it in the rendered projection
    Given an empty op log
    And the log has a "retire" op "op-r" on "slice-2" accepted at "2026-06-01T00:00:00Z"
    When reverse is called on the log for op "op-r"
    And apply is called on the proposal base and the reversed log
    Then apply succeeds
    And the rendered document has 5 elements
    And rendered element "slice-2" has body "Fan-out to remaining kinds"
