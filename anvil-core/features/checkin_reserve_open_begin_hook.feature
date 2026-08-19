Feature: Checkin re-serves the open-begin state's hook content (T4)
  When a resumed/compacted session checks back in under its prior actor name,
  any artifact that actor still has an OPEN begin on should re-warm its standing
  context: the current `(state, role)` hook body is re-served. This reuses the
  same budget-capped `resolve_and_read_hook` path as `begin`, keyed by the
  shared `has_open_begin` predicate, so a session that lost its window to
  compaction does not lose the per-step context the engine already delivered.

  Re-serve is purely additive — an actor with no open begin (never began, or
  already closed it with a transition) gets empty content, never an error.

  # AC: open begin on the current state → that state's hook is re-served verbatim
  Scenario: re-serve returns the open-begin state's hook body
    Given an in-memory query adapter seeded with artifact "tracks/20260604T7001_reserve_open" kind "track" state "implementing" with activity:
      | kind  | actor       | state        | at                   |
      | begin | Doer-700001 | implementing | 2026-06-04T09:00:00Z |
    And the in-memory query adapter has a reserve hook body for playbook "20260422T0000_track_lifecycle" filename "implement.md" with content "DISTINCTIVE-IMPLEMENT-REWARMED-HOOK"
    When reserve_hook_for_open_begin is evaluated for actor "Doer-700001" on "tracks/20260604T7001_reserve_open" kind "track" state "implementing" with a registry declaring state "implementing" role "doer" hook "implement.md"
    Then the reserve hook content contains "DISTINCTIVE-IMPLEMENT-REWARMED-HOOK"

  # AC: no open begin (never began) → empty re-serve, no error
  Scenario: re-serve returns empty when the actor never began
    Given an in-memory query adapter seeded with artifact "tracks/20260604T7002_reserve_never" kind "track" state "implementing"
    And the in-memory query adapter has a reserve hook body for playbook "20260422T0000_track_lifecycle" filename "implement.md" with content "SHOULD-NOT-BE-SERVED"
    When reserve_hook_for_open_begin is evaluated for actor "Doer-700002" on "tracks/20260604T7002_reserve_never" kind "track" state "implementing" with a registry declaring state "implementing" role "doer" hook "implement.md"
    Then the reserve hook content is empty

  # AC: a closing transition by the actor closes the marker → empty re-serve
  Scenario: re-serve returns empty after the actor's closing transition
    Given an in-memory query adapter seeded with artifact "tracks/20260604T7003_reserve_closed" kind "track" state "implementing" with activity:
      | kind  | actor       | state        | at                   |
      | begin | Doer-700003 | implementing | 2026-06-04T09:00:00Z |
    And the in-memory query adapter has a closing transition for artifact "tracks/20260604T7003_reserve_closed" actor "Doer-700003" at "2026-06-04T10:00:00Z"
    And the in-memory query adapter has a reserve hook body for playbook "20260422T0000_track_lifecycle" filename "implement.md" with content "SHOULD-NOT-BE-SERVED"
    When reserve_hook_for_open_begin is evaluated for actor "Doer-700003" on "tracks/20260604T7003_reserve_closed" kind "track" state "implementing" with a registry declaring state "implementing" role "doer" hook "implement.md"
    Then the reserve hook content is empty
