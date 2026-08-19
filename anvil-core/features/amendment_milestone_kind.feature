Feature: Amendment — milestone kind (AC-1, AC-3, AC-4, AC-5, Phase-6)

  # The milestone content-element schema declares:
  #   - outcome            (singleton)    — the milestone's primary outcome statement
  #   - success_criterion  (SC<N>)        — a named success criterion
  #   - scope              (scope-<slug>) — a named scope item (in or out of scope)
  # Legal ops: Revise only for outcome (singleton); Add/Revise/Retire/Reorder
  # for success_criterion and scope. All core functions reused without modification;
  # only seeds/milestone.rs is the new deliverable (KD-3).

  # ------------------------------------------------------------------
  # AC-1: Schema validation — milestone kind
  # ------------------------------------------------------------------

  Scenario: a legal Revise op on the outcome singleton validates OK
    Given a milestone base document with elements:
      | id      | kind    | body                              |
      | outcome | outcome | Anvil amendment core is complete  |
    When validate_op is called for a "revise" op targeting "outcome"
    Then validation succeeds

  Scenario: an op kind not in the milestone schema is rejected (Retire on singleton)
    Given a milestone base document with elements:
      | id      | kind    | body    |
      | outcome | outcome | Outcome |
    When validate_op is called for a "retire" op targeting "outcome"
    Then validation fails with code "amendment_op_not_in_schema"

  Scenario: an op targeting an undeclared element ID is rejected
    Given a milestone base document with elements:
      | id  | kind               | body             |
      | SC1 | success_criterion  | All tests green  |
    When validate_op is called for a "revise" op targeting "SC99"
    Then validation fails with code "amendment_unknown_element"

  Scenario: an Add minting a duplicate element ID is rejected
    Given a milestone base document with elements:
      | id          | kind  | body          |
      | scope-alpha | scope | In: core only |
    When validate_op is called for an "add" op minting "scope-alpha" of kind "scope" anchored "at_end"
    Then validation fails with code "amendment_duplicate_element_id"

  # ------------------------------------------------------------------
  # AC-3: Apply — milestone kind
  # ------------------------------------------------------------------

  Background:
    Given a milestone base document with elements:
      | id          | kind               | body                                    |
      | outcome     | outcome            | Anvil amendment core is complete        |
      | SC1         | success_criterion  | All 8 kind schemas return non-empty     |
      | SC2         | success_criterion  | apply/conflict/reverse all pass brine   |
      | scope-in    | scope              | In: pure anvil-core, no engine changes  |
      | scope-out   | scope              | Out: proto/engine/MCP wiring (B5b)      |

  Scenario: revise the outcome singleton
    Given an empty op log
    And the log has a "revise" op "op-1" on "outcome" with body "B5a core frozen and green" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the milestone base and log
    Then apply succeeds
    And the rendered document has 5 elements
    And rendered element "outcome" has body "B5a core frozen and green"

  Scenario: add a success criterion anchored after SC1
    Given an empty op log
    And the log has an "add" op "op-1" minting "SC3" of kind "success_criterion" with body "workspace cargo test green" anchored "after:SC1" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the milestone base and log
    Then apply succeeds
    And the rendered document has 6 elements
    And rendered element at position 2 has id "SC3"

  Scenario: retire a scope item omits it from rendered body but retains the op
    Given an empty op log
    And the log has a "retire" op "op-r" on "scope-out" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the milestone base and log
    Then apply succeeds
    And the rendered document has 4 elements
    And the rendered document does not contain element "scope-out"
    And the op log still contains op "op-r"

  Scenario: a Revise targets an SC added by a prior Add in the same log (dynamic element set)
    Given an empty op log
    And the log has an "add" op "op-add" minting "SC3" of kind "success_criterion" with body "initial" anchored "at_end" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-rev" on "SC3" with body "revised SC body" accepted at "2026-06-02T00:00:00Z"
    When apply is called on the milestone base and log
    Then apply succeeds
    And the rendered document has 6 elements
    And rendered element "SC3" has body "revised SC body"

  # ------------------------------------------------------------------
  # AC-4: Conflict detection — milestone kind
  # ------------------------------------------------------------------

  Scenario: two ops on the same success_criterion target_id conflict
    Given an empty op log
    And the log has a "revise" op "op-1" on "SC1" with body "first revision" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-2" on "SC1" with body "second revision" accepted at "2026-06-02T00:00:00Z"
    When detect_conflicts is called on the log
    Then the conflicts contain target "SC1"
    And the conflicts count is 1

  Scenario: a Retire and Revise on the same scope ID conflict
    Given an empty op log
    And the log has a "retire" op "op-1" on "scope-in" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-2" on "scope-in" with body "revised scope" accepted at "2026-06-02T00:00:00Z"
    When detect_conflicts is called on the log
    Then the conflicts contain target "scope-in"
    And the conflicts count is 1

  # ------------------------------------------------------------------
  # AC-5: Reversal — milestone kind
  # ------------------------------------------------------------------

  Scenario: reversing a revise on outcome returns projection to pre-op state
    Given an empty op log
    And the log has a "revise" op "op-1" on "outcome" with body "mutated outcome" accepted at "2026-06-01T00:00:00Z"
    When reverse is called on the log for op "op-1"
    And apply is called on the milestone base and the reversed log
    Then apply succeeds
    And rendered element "outcome" has body "Anvil amendment core is complete"

  Scenario: reversing a retire on a success_criterion restores it in the rendered projection
    Given an empty op log
    And the log has a "retire" op "op-r" on "SC2" accepted at "2026-06-01T00:00:00Z"
    When reverse is called on the log for op "op-r"
    And apply is called on the milestone base and the reversed log
    Then apply succeeds
    And the rendered document has 5 elements
    And rendered element "SC2" has body "apply/conflict/reverse all pass brine"
