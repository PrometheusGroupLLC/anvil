Feature: Indexed open-playbook lookup resolves from candidates without enumerating all artifacts
  open_marker_index Phase 1 — the open-marker index yields CANDIDATE artifact ids
  for a conversation so the lookup reads only those candidates (confirming each
  fresh: open begin marker + non-terminal current state) instead of enumerating
  EVERY artifact. The index gives candidates, not verdicts: a stale candidate that
  has gone terminal confirms to nothing. Semantics match the full scan exactly —
  empty conversation id resolves nothing, terminal is excluded, and the
  most-recently-begun open playbook wins on multiplicity. The fixture machines
  declare a non-terminal "active" state and a terminal "completed" state.

  Background:
    Given a routable playbooks fixture registry with authored route triggers

  Scenario: the indexed lookup returns the same open playbook the scan would, reading only the candidate
    Given an open playbook "20260623T0001_alpha" of kind "track" in state "active" begun for conversation "C1" at "2026-06-23T05:00:00Z"
    And the hearth also has 50 unrelated artifacts in state "active"
    When the indexed open-playbook lookup runs for conversation "C1" with candidates "20260623T0001_alpha"
    Then the indexed open-playbook lookup resolves artifact "20260623T0001_alpha" kind "track" state "active"
    And the indexed open-playbook lookup did not enumerate all artifacts

  Scenario: the indexed lookup and the full scan agree on a many-artifact hearth
    Given an open playbook "20260623T0001_alpha" of kind "track" in state "active" begun for conversation "C1" at "2026-06-23T05:00:00Z"
    And the hearth also has 50 unrelated artifacts in state "active"
    When the indexed open-playbook lookup runs for conversation "C1" with candidates "20260623T0001_alpha"
    And the full-scan open-playbook lookup runs for conversation "C1"
    Then the indexed and full-scan open-playbook lookups agree

  Scenario: a candidate that has gone terminal confirms to nothing (index gives candidates, not verdicts)
    Given an open playbook "20260623T0003_done" of kind "track" in state "completed" begun for conversation "C1" at "2026-06-23T05:00:00Z"
    When the indexed open-playbook lookup runs for conversation "C1" with candidates "20260623T0003_done"
    Then the indexed open-playbook lookup resolves nothing

  Scenario: a stale candidate id that no longer exists is skipped, not an error
    Given an open playbook "20260623T0001_alpha" of kind "track" in state "active" begun for conversation "C1" at "2026-06-23T05:00:00Z"
    When the indexed open-playbook lookup runs for conversation "C1" with candidates "20260623T9999_ghost,20260623T0001_alpha"
    Then the indexed open-playbook lookup resolves artifact "20260623T0001_alpha" kind "track" state "active"

  Scenario: two open candidates resolve the most-recently-begun
    Given an open playbook "20260623T0001_alpha" of kind "track" in state "active" begun for conversation "C1" at "2026-06-23T05:00:00Z"
    And an open playbook "20260623T0002_beta" of kind "daily_recap" in state "active" begun for conversation "C1" at "2026-06-23T06:00:00Z"
    When the indexed open-playbook lookup runs for conversation "C1" with candidates "20260623T0001_alpha,20260623T0002_beta"
    Then the indexed open-playbook lookup resolves artifact "20260623T0002_beta" kind "daily_recap" state "active"

  Scenario: an empty conversation id never resolves an open playbook
    Given an open playbook "20260623T0001_alpha" of kind "track" in state "active" begun for conversation "C1" at "2026-06-23T05:00:00Z"
    When the indexed open-playbook lookup runs for conversation "" with candidates "20260623T0001_alpha"
    Then the indexed open-playbook lookup resolves nothing

  Scenario: an empty candidate set resolves nothing without a scan
    Given an open playbook "20260623T0001_alpha" of kind "track" in state "active" begun for conversation "C1" at "2026-06-23T05:00:00Z"
    When the indexed open-playbook lookup runs for conversation "C1" with no candidates
    Then the indexed open-playbook lookup resolves nothing
    And the indexed open-playbook lookup did not enumerate all artifacts
