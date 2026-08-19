Feature: Begin-adoption predicate (BP3)
  has_open_begin(activity, transitions, actor, state) is the canonical
  predicate shared by the complete/snapshot soft-warn (BP2) and the
  BeginAdoptionStatus query RPC (BP3). These scenarios pin its observable
  behaviour directly at the core seam using an in-memory query adapter,
  ensuring the predicate cannot diverge between call-sites.

  # True after a begin-marker with no later transition by that actor
  Scenario: has_open_begin returns true when a begin-marker exists with no closing transition
    Given an in-memory query adapter seeded with artifact "tracks/20260604T0030_pred_open" kind "track" state "spec_review" with activity:
      | kind  | actor           | state       | at                   |
      | begin | Reviewer-500001 | spec_review | 2026-06-04T09:00:00Z |
    When has_open_begin is evaluated for actor "Reviewer-500001" state "spec_review" on "tracks/20260604T0030_pred_open"
    Then has_open_begin result is true

  # False after the actor records a closing transition
  Scenario: has_open_begin returns false after the actor's closing transition
    Given an in-memory query adapter seeded with artifact "tracks/20260604T0031_pred_closed" kind "track" state "spec_review" with activity:
      | kind  | actor           | state       | at                   |
      | begin | Reviewer-500002 | spec_review | 2026-06-04T09:00:00Z |
    And the in-memory query adapter has a closing transition for artifact "tracks/20260604T0031_pred_closed" actor "Reviewer-500002" at "2026-06-04T10:00:00Z"
    When has_open_begin is evaluated for actor "Reviewer-500002" state "spec_review" on "tracks/20260604T0031_pred_closed"
    Then has_open_begin result is false

  # False when the actor never called begin
  Scenario: has_open_begin returns false when the actor never began
    Given an in-memory query adapter seeded with artifact "tracks/20260604T0032_pred_never" kind "track" state "spec_review"
    When has_open_begin is evaluated for actor "Reviewer-500003" state "spec_review" on "tracks/20260604T0032_pred_never"
    Then has_open_begin result is false

  # State-scoped: begin on spec_review does not satisfy a query for spec
  Scenario: has_open_begin is state-scoped — begin on spec_review does not satisfy query for spec
    Given an in-memory query adapter seeded with artifact "tracks/20260604T0033_pred_scoped" kind "track" state "spec_review" with activity:
      | kind  | actor           | state       | at                   |
      | begin | Reviewer-500004 | spec_review | 2026-06-04T09:00:00Z |
    When has_open_begin is evaluated for actor "Reviewer-500004" state "spec" on "tracks/20260604T0033_pred_scoped"
    Then has_open_begin result is false

  # A transition by a DIFFERENT actor does not close A's marker
  Scenario: has_open_begin is unaffected by a transition from a different actor
    Given an in-memory query adapter seeded with artifact "tracks/20260604T0034_pred_diff_actor" kind "track" state "spec_review" with activity:
      | kind  | actor           | state       | at                   |
      | begin | Reviewer-500005 | spec_review | 2026-06-04T09:00:00Z |
    And the in-memory query adapter has a closing transition for artifact "tracks/20260604T0034_pred_diff_actor" actor "Reviewer-500099" at "2026-06-04T10:00:00Z"
    When has_open_begin is evaluated for actor "Reviewer-500005" state "spec_review" on "tracks/20260604T0034_pred_diff_actor"
    Then has_open_begin result is true
