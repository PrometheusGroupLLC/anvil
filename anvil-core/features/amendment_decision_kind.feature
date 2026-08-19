Feature: Amendment — decision kind (AC-1, AC-3, AC-4, AC-5, Phase-8)

  # The decision content-element schema declares:
  #   - question          (singleton)  — the decision question
  #   - answer            (singleton)  — the resolution/answer
  #   - rationale         (singleton)  — the rationale for the answer
  #   - validity_condition (vc-<N>)   — a named condition under which the decision holds
  # Legal ops: Revise only for singleton kinds (question/answer/rationale);
  # Add/Revise/Retire/Reorder for validity_condition. All core functions reused
  # without modification; only seeds/decision.rs is the new deliverable (KD-3).

  # ------------------------------------------------------------------
  # AC-1: Schema validation — decision kind
  # ------------------------------------------------------------------

  Scenario: a legal Revise op on the question singleton validates OK
    Given a decision base document with elements:
      | id       | kind     | body                                     |
      | question | question | Should amendments use structured diffs?  |
    When validate_op is called for a "revise" op targeting "question"
    Then validation succeeds

  Scenario: an op kind not in the decision schema is rejected (Retire on singleton)
    Given a decision base document with elements:
      | id     | kind   | body   |
      | answer | answer | Yes.   |
    When validate_op is called for a "retire" op targeting "answer"
    Then validation fails with code "amendment_op_not_in_schema"

  Scenario: an op targeting an undeclared element ID is rejected
    Given a decision base document with elements:
      | id       | kind     | body     |
      | question | question | Question |
    When validate_op is called for a "revise" op targeting "vc-99"
    Then validation fails with code "amendment_unknown_element"

  Scenario: an Add minting a duplicate validity_condition ID is rejected
    Given a decision base document with elements:
      | id   | kind               | body                  |
      | vc-1 | validity_condition | While Pilot is active |
    When validate_op is called for an "add" op minting "vc-1" of kind "validity_condition" anchored "at_end"
    Then validation fails with code "amendment_duplicate_element_id"

  # ------------------------------------------------------------------
  # AC-3: Apply — decision kind
  # ------------------------------------------------------------------

  Background:
    Given a decision base document with elements:
      | id        | kind               | body                                                    |
      | question  | question           | Should forge amendments use structured semantic diffs?  |
      | answer    | answer             | Yes — structured diffs for Pilot production readiness  |
      | rationale | rationale          | Mechanical conflict detection and computable projection |
      | vc-1      | validity_condition | While a Pilot production target remains active         |
      | vc-2      | validity_condition | While prose parsing remains out of scope for B5a        |

  Scenario: revise the answer singleton
    Given an empty op log
    And the log has a "revise" op "op-1" on "answer" with body "Structured diffs — decided by Nick" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the decision base and log
    Then apply succeeds
    And the rendered document has 5 elements
    And rendered element "answer" has body "Structured diffs — decided by Nick"

  Scenario: add a validity_condition anchored after vc-1
    Given an empty op log
    And the log has an "add" op "op-1" minting "vc-3" of kind "validity_condition" with body "While structured schemas per kind are maintained" anchored "after:vc-1" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the decision base and log
    Then apply succeeds
    And the rendered document has 6 elements
    And rendered element at position 4 has id "vc-3"

  Scenario: retire a validity_condition omits it from rendered body but retains the op
    Given an empty op log
    And the log has a "retire" op "op-r" on "vc-2" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the decision base and log
    Then apply succeeds
    And the rendered document has 4 elements
    And the rendered document does not contain element "vc-2"
    And the op log still contains op "op-r"

  Scenario: a Revise targets a vc added by a prior Add in the same log (dynamic element set)
    Given an empty op log
    And the log has an "add" op "op-add" minting "vc-3" of kind "validity_condition" with body "initial condition" anchored "at_end" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-rev" on "vc-3" with body "revised condition body" accepted at "2026-06-02T00:00:00Z"
    When apply is called on the decision base and log
    Then apply succeeds
    And the rendered document has 6 elements
    And rendered element "vc-3" has body "revised condition body"

  # ------------------------------------------------------------------
  # AC-4: Conflict detection — decision kind
  # ------------------------------------------------------------------

  Scenario: two ops on the same validity_condition target_id conflict
    Given an empty op log
    And the log has a "revise" op "op-1" on "vc-1" with body "first revision" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-2" on "vc-1" with body "second revision" accepted at "2026-06-02T00:00:00Z"
    When detect_conflicts is called on the log
    Then the conflicts contain target "vc-1"
    And the conflicts count is 1

  Scenario: a Retire and Revise on the same rationale conflict
    Given an empty op log
    And the log has a "retire" op "op-1" on "rationale" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-2" on "rationale" with body "revised rationale" accepted at "2026-06-02T00:00:00Z"
    When detect_conflicts is called on the log
    Then the conflicts contain target "rationale"
    And the conflicts count is 1

  # ------------------------------------------------------------------
  # AC-5: Reversal — decision kind
  # ------------------------------------------------------------------

  Scenario: reversing a revise on answer returns projection to pre-op state
    Given an empty op log
    And the log has a "revise" op "op-1" on "answer" with body "mutated answer" accepted at "2026-06-01T00:00:00Z"
    When reverse is called on the log for op "op-1"
    And apply is called on the decision base and the reversed log
    Then apply succeeds
    And rendered element "answer" has body "Yes — structured diffs for Pilot production readiness"

  Scenario: reversing a retire on a validity_condition restores it in the rendered projection
    Given an empty op log
    And the log has a "retire" op "op-r" on "vc-2" accepted at "2026-06-01T00:00:00Z"
    When reverse is called on the log for op "op-r"
    And apply is called on the decision base and the reversed log
    Then apply succeeds
    And the rendered document has 5 elements
    And rendered element "vc-2" has body "While prose parsing remains out of scope for B5a"
