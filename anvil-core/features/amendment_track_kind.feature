Feature: Amendment — track kind (AC-1, AC-3, AC-4, AC-5, Phase-5)

  # The track content-element schema declares:
  #   - overview   (singleton)       — one-line purpose summary
  #   - goal       (goal-<slug>)     — a named delivery goal
  #   - constraint (constraint-<N>)  — a named constraint or boundary
  #   - milestone_ref (mref-<slug>)  — a reference to a milestone
  # Legal ops: Revise only for overview (singleton); Add/Revise/Retire/Reorder
  # for the list kinds. All core functions reused without modification; only
  # seeds/track.rs is the new deliverable (KD-3).

  # ------------------------------------------------------------------
  # AC-1: Schema validation — track kind
  # ------------------------------------------------------------------

  Scenario: a legal Revise op on the overview singleton validates OK
    Given a track base document with elements:
      | id       | kind     | body                                   |
      | overview | overview | Implement structured amendment diffs   |
    When validate_op is called for a "revise" op targeting "overview"
    Then validation succeeds

  Scenario: an op kind not in the track schema is rejected (Retire on singleton)
    Given a track base document with elements:
      | id       | kind     | body     |
      | overview | overview | Overview |
    When validate_op is called for a "retire" op targeting "overview"
    Then validation fails with code "amendment_op_not_in_schema"

  Scenario: an op targeting an undeclared element ID is rejected
    Given a track base document with elements:
      | id         | kind | body       |
      | goal-alpha | goal | First goal |
    When validate_op is called for a "revise" op targeting "goal-beta"
    Then validation fails with code "amendment_unknown_element"

  Scenario: an Add minting a duplicate element ID is rejected
    Given a track base document with elements:
      | id           | kind       | body        |
      | constraint-1 | constraint | No markdown |
    When validate_op is called for an "add" op minting "constraint-1" of kind "constraint" anchored "at_end"
    Then validation fails with code "amendment_duplicate_element_id"

  # ------------------------------------------------------------------
  # AC-3: Apply — track kind
  # ------------------------------------------------------------------

  Background:
    Given a track base document with elements:
      | id           | kind         | body                                     |
      | overview     | overview     | Implement structured amendment diffs     |
      | goal-core    | goal         | Pure core apply/render/conflict/reverse  |
      | goal-kinds   | goal         | Per-kind schema for all 8 amendment kinds |
      | constraint-1 | constraint   | Pure anvil-core only (no engine/MCP)     |
      | mref-m1      | milestone_ref | Anvil readiness milestone                |

  Scenario: revise the overview singleton
    Given an empty op log
    And the log has a "revise" op "op-1" on "overview" with body "B5a: pure amendment core" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the track base and log
    Then apply succeeds
    And the rendered document has 5 elements
    And rendered element "overview" has body "B5a: pure amendment core"

  Scenario: add a goal anchored after goal-core
    Given an empty op log
    And the log has an "add" op "op-1" minting "goal-freeze" of kind "goal" with body "Stability freeze after all kinds" anchored "after:goal-core" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the track base and log
    Then apply succeeds
    And the rendered document has 6 elements
    And rendered element at position 2 has id "goal-freeze"

  Scenario: retire a milestone_ref omits it from rendered body but retains the op
    Given an empty op log
    And the log has a "retire" op "op-r" on "mref-m1" accepted at "2026-06-01T00:00:00Z"
    When apply is called on the track base and log
    Then apply succeeds
    And the rendered document has 4 elements
    And the rendered document does not contain element "mref-m1"
    And the op log still contains op "op-r"

  Scenario: a Revise targets a goal added by a prior Add in the same log (dynamic element set)
    Given an empty op log
    And the log has an "add" op "op-add" minting "goal-new" of kind "goal" with body "initial" anchored "at_end" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-rev" on "goal-new" with body "revised goal body" accepted at "2026-06-02T00:00:00Z"
    When apply is called on the track base and log
    Then apply succeeds
    And the rendered document has 6 elements
    And rendered element "goal-new" has body "revised goal body"

  # ------------------------------------------------------------------
  # AC-4: Conflict detection — track kind
  # ------------------------------------------------------------------

  Scenario: two ops on the same goal target_id conflict
    Given an empty op log
    And the log has a "revise" op "op-1" on "goal-core" with body "first revision" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-2" on "goal-core" with body "second revision" accepted at "2026-06-02T00:00:00Z"
    When detect_conflicts is called on the log
    Then the conflicts contain target "goal-core"
    And the conflicts count is 1

  Scenario: a Retire and Revise on the same constraint ID conflict
    Given an empty op log
    And the log has a "retire" op "op-1" on "constraint-1" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-2" on "constraint-1" with body "revised constraint" accepted at "2026-06-02T00:00:00Z"
    When detect_conflicts is called on the log
    Then the conflicts contain target "constraint-1"
    And the conflicts count is 1

  # ------------------------------------------------------------------
  # AC-5: Reversal — track kind
  # ------------------------------------------------------------------

  Scenario: reversing a revise on overview returns projection to pre-op state
    Given an empty op log
    And the log has a "revise" op "op-1" on "overview" with body "mutated overview" accepted at "2026-06-01T00:00:00Z"
    When reverse is called on the log for op "op-1"
    And apply is called on the track base and the reversed log
    Then apply succeeds
    And rendered element "overview" has body "Implement structured amendment diffs"

  Scenario: reversing a retire on a goal restores it in the rendered projection
    Given an empty op log
    And the log has a "retire" op "op-r" on "goal-kinds" accepted at "2026-06-01T00:00:00Z"
    When reverse is called on the log for op "op-r"
    And apply is called on the track base and the reversed log
    Then apply succeeds
    And the rendered document has 5 elements
    And rendered element "goal-kinds" has body "Per-kind schema for all 8 amendment kinds"
