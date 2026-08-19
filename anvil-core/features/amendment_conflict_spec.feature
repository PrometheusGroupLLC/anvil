Feature: Amendment conflict detection — spec kind (AC-4)

  # detect_conflicts(log) flags two accepted ops targeting the same element ID.
  # Pure fold over the ordered log; no prose. A Retire + Revise pair on the same
  # target_id is the canonical conflict (KD-4 retire semantics). Ops on distinct
  # IDs are not flagged.

  Scenario: two ops on the same target_id are flagged
    Given an empty op log
    And the log has a "revise" op "op-1" on "AC1" with body "first" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-2" on "AC1" with body "second" accepted at "2026-06-02T00:00:00Z"
    When detect_conflicts is called on the log
    Then the conflicts contain target "AC1"
    And the conflicts count is 1

  Scenario: a Retire and a Revise on the same target_id conflict
    Given an empty op log
    And the log has a "retire" op "op-r" on "AC1" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-v" on "AC1" with body "revise after retire" accepted at "2026-06-02T00:00:00Z"
    When detect_conflicts is called on the log
    Then the conflicts contain target "AC1"
    And the conflicts count is 1

  Scenario: ops on distinct target_ids are not flagged
    Given an empty op log
    And the log has a "revise" op "op-1" on "AC1" with body "a" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-2" on "AC2" with body "b" accepted at "2026-06-02T00:00:00Z"
    When detect_conflicts is called on the log
    Then the conflicts count is 0
