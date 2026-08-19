Feature: Amendment — plan kind (AC-1, AC-3, AC-4, AC-5, Phase-3)

  # The plan content-element schema declares phase elements (id: phase-N) and
  # task elements (id: phase-N.task-M, nested via the dotted-ID naming convention
  # on a flat document — KD-4). Legal ops: Add/Revise/Retire/Reorder at both
  # levels. All core functions (validate/apply/conflict/reverse) are reused
  # without modification; the only new deliverable is seeds/plan.rs (KD-3).

  # ------------------------------------------------------------------
  # AC-1: Schema validation — plan kind
  # ------------------------------------------------------------------

  Scenario: a legal Revise op on a phase element validates OK
    Given a plan base document with elements:
      | id      | kind  | body                     |
      | phase-1 | phase | Phase 1 — Scaffold       |
      | phase-2 | phase | Phase 2 — Core behaviour |
    When validate_op is called for a "revise" op targeting "phase-1"
    Then validation succeeds

  Scenario: a legal Revise op on a task element (dotted ID) validates OK
    Given a plan base document with elements:
      | id             | kind  | body                      |
      | phase-1        | phase | Phase 1 — Scaffold        |
      | phase-1.task-1 | task  | Stand up types            |
      | phase-1.task-2 | task  | Wire module into domain   |
    When validate_op is called for a "revise" op targeting "phase-1.task-1"
    Then validation succeeds

  Scenario: an op kind not in the plan schema is rejected
    # key_decision is not a declared plan element kind — expect rejection
    Given a plan base document with elements:
      | id      | kind  | body           |
      | phase-1 | phase | Phase 1        |
    When validate_op is called for an "add" op minting "phase-1.task-99" of kind "key_decision" anchored "at_end"
    Then validation fails with code "amendment_invalid_add"

  Scenario: an op targeting an undeclared element ID is rejected
    Given a plan base document with elements:
      | id      | kind  | body    |
      | phase-1 | phase | Phase 1 |
    When validate_op is called for a "revise" op targeting "phase-99"
    Then validation fails with code "amendment_unknown_element"

  Scenario: an Add minting a duplicate element ID is rejected
    Given a plan base document with elements:
      | id             | kind | body           |
      | phase-1.task-1 | task | Stand up types |
    When validate_op is called for an "add" op minting "phase-1.task-1" of kind "task" anchored "at_end"
    Then validation fails with code "amendment_duplicate_element_id"

  # ------------------------------------------------------------------
  # AC-3: Apply — plan kind (add task, revise phase, retire task)
  # ------------------------------------------------------------------

  Background:
    Given a plan base document with elements:
      | id             | kind  | body                   |
      | phase-1        | phase | Phase 1 — Scaffold     |
      | phase-1.task-1 | task  | Stand up module types  |
      | phase-1.task-2 | task  | Wire into domain/mod   |
      | phase-2        | phase | Phase 2 — Behaviour    |
      | phase-2.task-1 | task  | Write failing feature  |

  Scenario: revise a phase body
    Given an empty op log
    And the log has a "revise" op "op-1" on "phase-1" with body "Phase 1 — Scaffold (revised)" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the plan base and log
    Then apply succeeds
    And the rendered document has 5 elements
    And rendered element "phase-1" has body "Phase 1 — Scaffold (revised)"

  Scenario: add a task with a dotted ID anchored after another task
    Given an empty op log
    And the log has an "add" op "op-1" minting "phase-1.task-3" of kind "task" with body "Refactor step module" anchored "after:phase-1.task-2" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the plan base and log
    Then apply succeeds
    And the rendered document has 6 elements
    And rendered element at position 3 has id "phase-1.task-3"

  Scenario: retire a task omits it from the rendered body but retains the op in the log
    Given an empty op log
    And the log has a "retire" op "op-r" on "phase-1.task-2" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the plan base and log
    Then apply succeeds
    And the rendered document has 4 elements
    And the rendered document does not contain element "phase-1.task-2"
    And the op log still contains op "op-r"

  Scenario: a Revise targets a task added by a prior Add in the same log (dynamic element set)
    Given an empty op log
    And the log has an "add" op "op-add" minting "phase-2.task-2" of kind "task" with body "initial body" anchored "at_end" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-rev" on "phase-2.task-2" with body "revised body" accepted at "2026-06-02T00:00:00Z"
    When apply is called on the plan base and log
    Then apply succeeds
    And the rendered document has 6 elements
    And rendered element "phase-2.task-2" has body "revised body"

  # ------------------------------------------------------------------
  # CRITICAL: Reorder — exercises the live Reorder path in apply.rs
  # ------------------------------------------------------------------

  Scenario: reorder phase-2 before phase-1
    Given an empty op log
    And the log has a "reorder" op "op-ro" on "phase-2" anchored "before:phase-1" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the plan base and log
    Then apply succeeds
    And the rendered document has 5 elements
    And rendered element at position 0 has id "phase-2"
    And rendered element at position 1 has id "phase-1"

  Scenario: reorder a task within a phase (move task-2 before task-1)
    Given an empty op log
    And the log has a "reorder" op "op-ro" on "phase-1.task-2" anchored "before:phase-1.task-1" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the plan base and log
    Then apply succeeds
    And the rendered document has 5 elements
    And rendered element at position 1 has id "phase-1.task-2"
    And rendered element at position 2 has id "phase-1.task-1"

  # ------------------------------------------------------------------
  # AC-4: Conflict detection — plan kind
  # ------------------------------------------------------------------

  Scenario: two ops on the same phase target_id conflict
    Given an empty op log
    And the log has a "revise" op "op-1" on "phase-1" with body "first revision" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-2" on "phase-1" with body "second revision" accepted at "2026-06-02T00:00:00Z"
    When detect_conflicts is called on the log
    Then the conflicts contain target "phase-1"
    And the conflicts count is 1

  Scenario: two ops on the same dotted-ID task target conflict
    Given an empty op log
    And the log has a "revise" op "op-1" on "phase-1.task-1" with body "changed task" accepted at "2026-06-01T00:00:00Z"
    And the log has a "retire" op "op-2" on "phase-1.task-1" accepted at "2026-06-02T00:00:00Z"
    When detect_conflicts is called on the log
    Then the conflicts contain target "phase-1.task-1"
    And the conflicts count is 1

  # ------------------------------------------------------------------
  # AC-5: Reversal — plan kind
  # ------------------------------------------------------------------

  Scenario: reversing a revise on a phase returns projection to pre-op state
    Given an empty op log
    And the log has a "revise" op "op-1" on "phase-1" with body "mutated body" accepted at "2026-06-01T00:00:00Z"
    When reverse is called on the log for op "op-1"
    And apply is called on the plan base and the reversed log
    Then apply succeeds
    And rendered element "phase-1" has body "Phase 1 — Scaffold"

  Scenario: reversing a retire on a task restores the element in the rendered projection
    Given an empty op log
    And the log has a "retire" op "op-r" on "phase-1.task-2" accepted at "2026-06-01T00:00:00Z"
    When reverse is called on the log for op "op-r"
    And apply is called on the plan base and the reversed log
    Then apply succeeds
    And the rendered document has 5 elements
    And rendered element "phase-1.task-2" has body "Wire into domain/mod"
