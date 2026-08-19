Feature: ActivitySummary folds the universal activity log into a usage summary
  The ActivitySummary read-side folds the durable, redacted activity-log record
  stream (one record per command turn the engine serves) into the dashboard's
  usage summary: a total turn count, per-command counts, per-route-outcome
  counts (over route turns only), per-playbook counts (records with no resolved
  kind excluded), and per-period buckets carrying total turns and the distinct
  non-None actor count. Counts are ordered descending by count then ascending by
  name; buckets ascending by period_start. An empty stream folds to a zeroed
  result (no error) — a fresh hearth simply has no activity yet.

  Scenario: total turns and per-command counts fold from the stream
    Given an activity summary record stream:
      | command  | outcome  | artifact_kind | actor_hash | at                   |
      | route    | single   | track         | aaaa1111   | 2026-06-15T09:00:00Z |
      | begin    | ok       | track         | aaaa1111   | 2026-06-15T09:05:00Z |
      | begin    | ok       | track         | bbbb2222   | 2026-06-15T10:00:00Z |
      | catalog  | ok       |               | -          | 2026-06-15T11:00:00Z |
    When the activity summary is folded by day
    Then the activity summary has total turns 4
    And the activity summary by_command "begin" has count 2
    And the activity summary by_command "route" has count 1
    And the activity summary by_command "catalog" has count 1
    And the activity summary by_command is ordered descending by count

  Scenario: route outcomes fold only from route turns
    Given an activity summary record stream:
      | command | outcome    | artifact_kind | actor_hash | at                   |
      | route   | single     | track         | aaaa1111   | 2026-06-15T09:00:00Z |
      | route   | no_match   |               | aaaa1111   | 2026-06-15T09:10:00Z |
      | route   | no_match   |               | bbbb2222   | 2026-06-15T09:20:00Z |
      | route   | candidates |               | bbbb2222   | 2026-06-15T09:30:00Z |
      | begin   | ok         | track         | aaaa1111   | 2026-06-15T09:40:00Z |
    When the activity summary is folded by day
    Then the activity summary by_route_outcome "no_match" has count 2
    And the activity summary by_route_outcome "single" has count 1
    And the activity summary by_route_outcome "candidates" has count 1
    And the activity summary by_route_outcome has 3 entries

  Scenario: per-source counts fold only from route turns, empty source as unknown
    Given an activity summary record stream:
      | command | outcome  | artifact_kind | source      | actor_hash | at                   |
      | route   | single   | track         | claude-code | aaaa1111   | 2026-06-15T09:00:00Z |
      | route   | no_match |               | claude-code | aaaa1111   | 2026-06-15T09:10:00Z |
      | route   | single   | track         | codex       | bbbb2222   | 2026-06-15T09:20:00Z |
      | route   | no_match |               |             | bbbb2222   | 2026-06-15T09:30:00Z |
      | begin   | ok       | track         | claude-code | aaaa1111   | 2026-06-15T09:40:00Z |
    When the activity summary is folded by day
    Then the activity summary by_source "claude-code" has count 2
    And the activity summary by_source "codex" has count 1
    And the activity summary by_source "unknown" has count 1
    And the activity summary by_source has 3 entries

  Scenario: per-playbook counts exclude records with no resolved kind
    Given an activity summary record stream:
      | command | outcome  | artifact_kind | actor_hash | at                   |
      | begin   | ok       | track         | aaaa1111   | 2026-06-15T09:00:00Z |
      | begin   | ok       | track         | aaaa1111   | 2026-06-15T09:05:00Z |
      | begin   | ok       | milestone     | bbbb2222   | 2026-06-15T10:00:00Z |
      | route   | no_match |               | bbbb2222   | 2026-06-15T11:00:00Z |
      | catalog | ok       |               | -          | 2026-06-15T12:00:00Z |
    When the activity summary is folded by day
    Then the activity summary by_artifact_kind "track" has count 2
    And the activity summary by_artifact_kind "milestone" has count 1
    And the activity summary by_artifact_kind has 2 entries

  Scenario: buckets carry total turns and distinct actors per day
    Given an activity summary record stream:
      | command | outcome | artifact_kind | actor_hash | at                   |
      | begin   | ok      | track         | aaaa1111   | 2026-06-15T09:00:00Z |
      | begin   | ok      | track         | bbbb2222   | 2026-06-15T14:00:00Z |
      | begin   | ok      | track         | aaaa1111   | 2026-06-16T10:00:00Z |
    When the activity summary is folded by day
    Then the activity summary has 2 buckets
    And the activity summary bucket "2026-06-15" has total turns 2
    And the activity summary bucket "2026-06-15" has distinct actors 2
    And the activity summary bucket "2026-06-16" has total turns 1
    And the activity summary bucket "2026-06-16" has distinct actors 1
    And the activity summary buckets are ordered ascending by period start

  Scenario: actor records with no hash are excluded from the distinct set
    Given an activity summary record stream:
      | command | outcome | artifact_kind | actor_hash | at                   |
      | begin   | ok      | track         | aaaa1111   | 2026-06-15T09:00:00Z |
      | catalog | ok      |               | -          | 2026-06-15T11:00:00Z |
    When the activity summary is folded by day
    Then the activity summary bucket "2026-06-15" has total turns 2
    And the activity summary bucket "2026-06-15" has distinct actors 1

  Scenario: week granularity buckets records into the Monday of their ISO week
    Given an activity summary record stream:
      | command | outcome | artifact_kind | actor_hash | at                   |
      | begin   | ok      | track         | aaaa1111   | 2026-06-15T09:00:00Z |
      | begin   | ok      | track         | aaaa1111   | 2026-06-17T09:00:00Z |
      | begin   | ok      | track         | bbbb2222   | 2026-06-22T09:00:00Z |
    When the activity summary is folded by week
    Then the activity summary has 2 buckets
    And the activity summary bucket "2026-06-15" has total turns 2
    And the activity summary bucket "2026-06-22" has total turns 1

  Scenario: per-conversation conversion folds routed vs engaged conversations
    # Honest adoption: of the distinct conversations anvil ROUTED, how many also
    # ENGAGED a playbook (begin/snapshot/complete/amend) — not begins / route-turns.
    # Routes for one conversation may fire many times per session; dedup by
    # conversation_hash. A conversation that engaged but was never routed is not
    # counted in the routed denominator.
    Given an activity summary record stream:
      | command  | outcome  | artifact_kind | conversation_hash | at                   |
      | route    | single   | track         | conv_a            | 2026-06-15T09:00:00Z |
      | route    | single   | track         | conv_a            | 2026-06-15T09:01:00Z |
      | begin    | ok       | track         | conv_a            | 2026-06-15T09:05:00Z |
      | route    | single   | track         | conv_b            | 2026-06-15T10:00:00Z |
      | route    | no_match |               | conv_c            | 2026-06-15T11:00:00Z |
      | begin    | ok       | track         | conv_d            | 2026-06-15T12:00:00Z |
    When the activity summary is folded by day
    Then the activity summary has 3 routed conversations
    And the activity summary has 1 converted conversations

  Scenario: an empty stream folds to a zeroed summary
    Given an empty activity summary record stream
    When the activity summary is folded by day
    Then the activity summary has total turns 0
    And the activity summary has 0 buckets
    And the activity summary by_command has 0 entries
    And the activity summary has 0 routed conversations
    And the activity summary has 0 converted conversations
    And the activity summary has 0 playbook step turns

  Scenario: per-call-state counts fold over route turns, missing field as unknown
    # The call-state classification distinguishes a fresh start-opportunity from a
    # mid-playbook continuation so coverage and start-conversion use the correct
    # denominators. Counted over route turns only; a route record lacking the
    # field buckets under "unknown". Non-route turns are excluded.
    Given an activity summary record stream:
      | command | outcome  | artifact_kind | call_state        | at                   |
      | route   | single   | track         | start_opportunity | 2026-06-15T09:00:00Z |
      | route   | single   | track         | mid_playbook_run      | 2026-06-15T09:10:00Z |
      | route   | single   | track         | mid_playbook_run      | 2026-06-15T09:20:00Z |
      | route   | no_match |               | no_playbook_run       | 2026-06-15T09:30:00Z |
      | route   | single   | track         |                   | 2026-06-15T09:40:00Z |
      | begin   | ok       | track         | start_opportunity | 2026-06-15T09:50:00Z |
    When the activity summary is folded by day
    Then the activity summary by_call_state "mid_playbook_run" has count 2
    And the activity summary by_call_state "start_opportunity" has count 1
    And the activity summary by_call_state "no_playbook_run" has count 1
    And the activity summary by_call_state "unknown" has count 1
    And the activity summary by_call_state has 4 entries

  Scenario: playbook step turns count begin/snapshot/complete/amend volume
    # The VOLUME of actual playbook-driving turns (not a per-conversation flag):
    # begin fires once, the phase steps fire many times per playbook. Routing,
    # describe, catalog, and checkin turns are excluded.
    Given an activity summary record stream:
      | command  | outcome | artifact_kind | at                   |
      | route    | single  | track         | 2026-06-15T09:00:00Z |
      | begin    | ok      | track         | 2026-06-15T09:05:00Z |
      | snapshot | ok      | track         | 2026-06-15T09:10:00Z |
      | snapshot | ok      | track         | 2026-06-15T09:15:00Z |
      | complete | ok      | track         | 2026-06-15T09:20:00Z |
      | amend    | ok      | track         | 2026-06-15T09:25:00Z |
      | catalog  | ok      |               | 2026-06-15T09:30:00Z |
    When the activity summary is folded by day
    Then the activity summary has 5 playbook step turns

  Scenario: cross-hearth merge sums turns and unions distinct actors
    Given activity summary hearth A stream:
      | command | outcome | artifact_kind | actor_hash | at                   |
      | begin   | ok      | track         | aaaa1111   | 2026-06-15T09:00:00Z |
      | begin   | ok      | track         | bbbb2222   | 2026-06-15T10:00:00Z |
    And activity summary hearth B stream:
      | command | outcome | artifact_kind | actor_hash | at                   |
      | begin   | ok      | track         | aaaa1111   | 2026-06-15T11:00:00Z |
      | begin   | ok      | track         | cccc3333   | 2026-06-15T12:00:00Z |
    When the activity summary is folded across both hearths by day
    Then the activity summary has total turns 4
    And the activity summary by_artifact_kind "track" has count 4
    And the activity summary bucket "2026-06-15" has total turns 4
    And the activity summary bucket "2026-06-15" has distinct actors 3
