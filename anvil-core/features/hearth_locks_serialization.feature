Feature: HearthLocks serializes same-hearth writers, never blocks different hearths
  The process-wide snapshot lock is replaced by one lock instance per canonical
  hearth, held for the process lifetime (spec Req 5, AC "Per-hearth lock").
  Concurrent operations on the SAME hearth MUST serialize so no update is lost;
  operations on DIFFERENT hearths MUST NOT block each other.

  Exercised deterministically in-process at the anvil-core library seam via an
  injected barrier — no flaky multi-process repro.

  Scenario: Two same-hearth writers serialize with no lost update
    Given a HearthLocks primitive
    When two writers race a read-modify-write on the same hearth through HearthLocks
    Then the final committed value is 2

  Scenario: Two different-hearth writers do not block each other
    Given a HearthLocks primitive
    When a writer parks holding one hearth's lock and a second writer locks a different hearth
    Then the second writer completes while the first is still parked
