Feature: Amendment — initiative kind (AC-1, AC-3, AC-4, AC-5, Phase-7)

  # The initiative content-element schema declares:
  #   - pattern     (singleton)        — the initiative's behavioural pattern
  #   - expectation (expectation-<N>)  — a named expected outcome
  #   - evidence    (expectation-<N>.evidence-<M>) — evidence nested under an
  #     expectation via dotted-ID naming convention on a flat document (KD-4).
  #     The core treats the ID as an opaque string — no dot-parsing.
  # Legal ops: Revise only for pattern (singleton); Add/Revise/Retire/Reorder
  # for expectation and evidence. All core functions reused without modification;
  # only seeds/initiative.rs is the new deliverable (KD-3).

  # ------------------------------------------------------------------
  # AC-1: Schema validation — initiative kind
  # ------------------------------------------------------------------

  Scenario: a legal Revise op on the pattern singleton validates OK
    Given an initiative base document with elements:
      | id      | kind    | body                                   |
      | pattern | pattern | All new behavior is brine-test-first   |
    When validate_op is called for a "revise" op targeting "pattern"
    Then validation succeeds

  Scenario: an op kind not in the initiative schema is rejected (Retire on singleton)
    Given an initiative base document with elements:
      | id      | kind    | body    |
      | pattern | pattern | Pattern |
    When validate_op is called for a "retire" op targeting "pattern"
    Then validation fails with code "amendment_op_not_in_schema"

  Scenario: an op targeting an undeclared element ID is rejected
    Given an initiative base document with elements:
      | id               | kind        | body           |
      | expectation-1    | expectation | First expected |
    When validate_op is called for a "revise" op targeting "expectation-99"
    Then validation fails with code "amendment_unknown_element"

  Scenario: an Add minting a duplicate evidence element ID is rejected
    Given an initiative base document with elements:
      | id                              | kind     | body        |
      | expectation-1.evidence-1        | evidence | First piece |
    When validate_op is called for an "add" op minting "expectation-1.evidence-1" of kind "evidence" anchored "at_end"
    Then validation fails with code "amendment_duplicate_element_id"

  # ------------------------------------------------------------------
  # AC-3: Apply — initiative kind
  # ------------------------------------------------------------------

  Background:
    Given an initiative base document with elements:
      | id                          | kind        | body                                      |
      | pattern                     | pattern     | All new behavior is brine-test-first      |
      | expectation-1               | expectation | Feature files are written before code     |
      | expectation-1.evidence-1    | evidence    | amendment_apply_spec.feature exists       |
      | expectation-2               | expectation | All 8 schemas resolve non-empty           |
      | expectation-2.evidence-1    | evidence    | all_kinds_schema_present.feature green    |

  Scenario: revise the pattern singleton
    Given an empty op log
    And the log has a "revise" op "op-1" on "pattern" with body "Brine-first, no raw unit tests" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the initiative base and log
    Then apply succeeds
    And the rendered document has 5 elements
    And rendered element "pattern" has body "Brine-first, no raw unit tests"

  Scenario: add an expectation anchored after expectation-1
    Given an empty op log
    And the log has an "add" op "op-1" minting "expectation-3" of kind "expectation" with body "Workspace tests stay green" anchored "after:expectation-1" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the initiative base and log
    Then apply succeeds
    And the rendered document has 6 elements
    And rendered element at position 2 has id "expectation-3"

  Scenario: retire an evidence element omits it from rendered body but retains the op
    Given an empty op log
    And the log has a "retire" op "op-r" on "expectation-2.evidence-1" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the initiative base and log
    Then apply succeeds
    And the rendered document has 4 elements
    And the rendered document does not contain element "expectation-2.evidence-1"
    And the op log still contains op "op-r"

  Scenario: a Revise targets evidence added by a prior Add in the same log (dynamic element set)
    Given an empty op log
    And the log has an "add" op "op-add" minting "expectation-1.evidence-2" of kind "evidence" with body "initial evidence" anchored "at_end" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-rev" on "expectation-1.evidence-2" with body "revised evidence body" accepted at "2026-06-02T00:00:00Z"
    When apply is called on the initiative base and log
    Then apply succeeds
    And the rendered document has 6 elements
    And rendered element "expectation-1.evidence-2" has body "revised evidence body"

  # ------------------------------------------------------------------
  # AC-4: Conflict detection — initiative kind
  # ------------------------------------------------------------------

  Scenario: two ops on the same expectation target_id conflict
    Given an empty op log
    And the log has a "revise" op "op-1" on "expectation-1" with body "first revision" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-2" on "expectation-1" with body "second revision" accepted at "2026-06-02T00:00:00Z"
    When detect_conflicts is called on the log
    Then the conflicts contain target "expectation-1"
    And the conflicts count is 1

  Scenario: a Retire and Revise on the same evidence ID conflict
    Given an empty op log
    And the log has a "retire" op "op-1" on "expectation-1.evidence-1" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-2" on "expectation-1.evidence-1" with body "revised evidence" accepted at "2026-06-02T00:00:00Z"
    When detect_conflicts is called on the log
    Then the conflicts contain target "expectation-1.evidence-1"
    And the conflicts count is 1

  # ------------------------------------------------------------------
  # AC-5: Reversal — initiative kind
  # ------------------------------------------------------------------

  Scenario: reversing a revise on pattern returns projection to pre-op state
    Given an empty op log
    And the log has a "revise" op "op-1" on "pattern" with body "mutated pattern" accepted at "2026-06-01T00:00:00Z"
    When reverse is called on the log for op "op-1"
    And apply is called on the initiative base and the reversed log
    Then apply succeeds
    And rendered element "pattern" has body "All new behavior is brine-test-first"

  Scenario: reversing a retire on evidence restores it in the rendered projection
    Given an empty op log
    And the log has a "retire" op "op-r" on "expectation-1.evidence-1" accepted at "2026-06-01T00:00:00Z"
    When reverse is called on the log for op "op-r"
    And apply is called on the initiative base and the reversed log
    Then apply succeeds
    And the rendered document has 5 elements
    And rendered element "expectation-1.evidence-1" has body "amendment_apply_spec.feature exists"
