Feature: All write paths share the one HearthLocks instance (engine seam, N3)
  The single process-wide snapshot_lock is replaced by one HearthLocks instance
  per canonical hearth (spec Req 5, plan N3). ALL write RPCs — begin, snapshot,
  complete — obtain their per-hearth guard from that one instance. This scenario
  drives a snapshot (projection-only) and a complete to the SAME hearth
  concurrently against one engine and asserts both succeed: both routes share
  the lock (no self-deadlock, no torn registry — atomic_write guards tearing).
  The deterministic serialization / no-lost-update proof lives at the
  anvil-core HearthLocks seam (hearth_locks_serialization.feature, AC5).

  Scenario: Concurrent same-hearth snapshot + complete both succeed
    Given two hearth directories X and Y each with the standard structure
    When concurrent snapshot and complete are issued to the same hearth
    Then the snapshot RPC response success is "true"
    And the complete RPC response new_state is "spec_review"
    And the complete RPC response resolved_hearth is the hearth "<hearth_x>"
