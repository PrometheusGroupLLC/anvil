Feature: Amendment op-log serde round-trip (KD-2)

  # An OpLog serializes and deserializes to an identical log; ordered() returns
  # the same sequence before and after the round-trip. accepted_at + seq are
  # persisted fields, so the total order is stable across serialization.

  Scenario: an op log with varying accept timestamps round-trips identically and ordered() is stable
    Given an empty op log
    And the log has a "revise" op "op-c" on "AC1" with body "c" accepted at "2026-06-03T00:00:00Z"
    And the log has a "revise" op "op-a" on "AC1" with body "a" accepted at "2026-06-01T00:00:00Z"
    And the log has a "revise" op "op-b" on "AC1" with body "b" accepted at "2026-06-01T00:00:00Z"
    When the op log is serialized and deserialized
    Then the deserialized log equals the original log
    And the deserialized log ordered() sequence equals the original ordered() sequence
    And the ordered() op_id sequence is "op-a,op-b,op-c"
