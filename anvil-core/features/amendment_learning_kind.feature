Feature: Amendment — learning kind (AC-1, AC-3, AC-4, AC-5, Phase-9)

  # The learning content-element schema declares:
  #   - observation  (singleton)       — the observed phenomenon / what happened
  #   - conclusion   (singleton)       — the synthesized insight / so what
  #   - evidence     (evidence-<N>)    — a named piece of supporting evidence
  # Legal ops: Revise only for observation and conclusion (singletons);
  # Add/Revise/Retire/Reorder for evidence. All core functions reused without
  # modification; only seeds/learning.rs is the new deliverable (KD-3).

  # ------------------------------------------------------------------
  # AC-1: Schema validation — learning kind
  # ------------------------------------------------------------------

  Scenario: a legal Revise op on the observation singleton validates OK
    Given a learning base document with elements:
      | id          | kind        | body                                   |
      | observation | observation | Flat structured documents are testable |
    When validate_op is called for a "revise" op targeting "observation"
    Then validation succeeds

  Scenario: an op kind not in the learning schema is rejected (Retire on singleton)
    Given a learning base document with elements:
      | id         | kind       | body       |
      | conclusion | conclusion | Therefore… |
    When validate_op is called for a "retire" op targeting "conclusion"
    Then validation fails with code "amendment_op_not_in_schema"

  Scenario: an op targeting an undeclared element ID is rejected
    Given a learning base document with elements:
      | id          | kind        | body        |
      | observation | observation | Observation |
    When validate_op is called for a "revise" op targeting "evidence-99"
    Then validation fails with code "amendment_unknown_element"

  Scenario: an Add minting a duplicate evidence element ID is rejected
    Given a learning base document with elements:
      | id         | kind     | body          |
      | evidence-1 | evidence | First piece   |
    When validate_op is called for an "add" op minting "evidence-1" of kind "evidence" anchored "at_end"
    Then validation fails with code "amendment_duplicate_element_id"

  # ------------------------------------------------------------------
  # AC-3: Apply — learning kind
  # ------------------------------------------------------------------

  Background:
    Given a learning base document with elements:
      | id          | kind        | body                                              |
      | observation | observation | Flat doc + dotted IDs encode nesting without cost |
      | conclusion  | conclusion  | The core apply() is kind-agnostic by design       |
      | evidence-1  | evidence    | plan kind uses phase-N.task-M IDs cleanly         |
      | evidence-2  | evidence    | initiative kind uses expectation-N.evidence-M IDs |

  Scenario: revise the observation singleton
    Given an empty op log
    And the log has a "revise" op "op-1" on "observation" with body "Dotted IDs encode nesting on flat docs" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the learning base and log
    Then apply succeeds
    And the rendered document has 4 elements
    And rendered element "observation" has body "Dotted IDs encode nesting on flat docs"

  Scenario: add an evidence element anchored after evidence-1
    Given an empty op log
    And the log has an "add" op "op-1" minting "evidence-3" of kind "evidence" with body "all-kinds-present scenario green" anchored "after:evidence-1" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the learning base and log
    Then apply succeeds
    And the rendered document has 5 elements
    And rendered element at position 3 has id "evidence-3"

  Scenario: retire an evidence element omits it from rendered body but retains the op
    Given an empty op log
    And the log has a "retire" op "op-r" on "evidence-2" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the learning base and log
    Then apply succeeds
    And the rendered document has 3 elements
    And the rendered document does not contain element "evidence-2"
    And the op log still contains op "op-r"

  Scenario: a Revise targets evidence added by a prior Add in the same log (dynamic element set)
    Given an empty op log
    And the log has an "add" op "op-add" minting "evidence-3" of kind "evidence" with body "initial evidence" anchored "at_end" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-rev" on "evidence-3" with body "revised evidence body" accepted at "2026-06-02T00:00:00Z"
    When apply is called on the learning base and log
    Then apply succeeds
    And the rendered document has 5 elements
    And rendered element "evidence-3" has body "revised evidence body"

  # ------------------------------------------------------------------
  # AC-4: Conflict detection — learning kind
  # ------------------------------------------------------------------

  Scenario: two ops on the same evidence target_id conflict
    Given an empty op log
    And the log has a "revise" op "op-1" on "evidence-1" with body "first revision" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-2" on "evidence-1" with body "second revision" accepted at "2026-06-02T00:00:00Z"
    When detect_conflicts is called on the log
    Then the conflicts contain target "evidence-1"
    And the conflicts count is 1

  Scenario: a Retire and Revise on the same evidence ID conflict
    Given an empty op log
    And the log has a "retire" op "op-1" on "evidence-2" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-2" on "evidence-2" with body "revised evidence" accepted at "2026-06-02T00:00:00Z"
    When detect_conflicts is called on the log
    Then the conflicts contain target "evidence-2"
    And the conflicts count is 1

  # ------------------------------------------------------------------
  # AC-5: Reversal — learning kind
  # ------------------------------------------------------------------

  Scenario: reversing a revise on conclusion returns projection to pre-op state
    Given an empty op log
    And the log has a "revise" op "op-1" on "conclusion" with body "mutated conclusion" accepted at "2026-06-01T00:00:00Z"
    When reverse is called on the log for op "op-1"
    And apply is called on the learning base and the reversed log
    Then apply succeeds
    And rendered element "conclusion" has body "The core apply() is kind-agnostic by design"

  Scenario: reversing a retire on evidence restores it in the rendered projection
    Given an empty op log
    And the log has a "retire" op "op-r" on "evidence-1" accepted at "2026-06-01T00:00:00Z"
    When reverse is called on the log for op "op-r"
    And apply is called on the learning base and the reversed log
    Then apply succeeds
    And the rendered document has 4 elements
    And rendered element "evidence-1" has body "plan kind uses phase-N.task-M IDs cleanly"
