Feature: UsageTimeSeries folds routing activity into time buckets
  The UsageTimeSeries read-side folds the durable, redacted routing-activity
  record stream into time buckets keyed by the calendar day (or ISO week) of
  each record's `at` timestamp. Each bucket carries the total call count for the
  period and a per-playbook breakdown. Buckets are ordered ascending by
  period_start; within a bucket the per-playbook breakdown is ordered ascending
  by kind. An empty record stream folds to an empty bucket list (no error) — a
  fresh hearth simply has no activity yet.

  Scenario: records across two days bucket into one bucket per day
    Given a usage timeseries record stream:
      | kind       | outcome | at                   |
      | lore_query | single  | 2026-06-15T09:00:00Z |
      | lore_query | single  | 2026-06-15T14:00:00Z |
      | lore_query | single  | 2026-06-16T10:00:00Z |
    When the usage timeseries is folded by day
    Then the usage timeseries has 2 buckets
    And the usage timeseries bucket "2026-06-15" has total calls 2
    And the usage timeseries bucket "2026-06-16" has total calls 1
    And the usage timeseries buckets are ordered ascending by period start

  Scenario: a bucket carries a per-playbook breakdown ordered by kind
    Given a usage timeseries record stream:
      | kind        | outcome | at                   |
      | lore_query  | single  | 2026-06-15T09:00:00Z |
      | lore_query  | single  | 2026-06-15T11:00:00Z |
      | measurement | single  | 2026-06-15T10:00:00Z |
    When the usage timeseries is folded by day
    Then the usage timeseries has 1 buckets
    And the usage timeseries bucket "2026-06-15" has total calls 3
    And the usage timeseries bucket "2026-06-15" per-playbook kind "lore_query" has call count 2
    And the usage timeseries bucket "2026-06-15" per-playbook kind "measurement" has call count 1
    And the usage timeseries bucket "2026-06-15" per-playbook is ordered ascending by kind

  Scenario: week granularity buckets records into the Monday of their ISO week
    Given a usage timeseries record stream:
      | kind       | outcome | at                   |
      | lore_query | single  | 2026-06-15T09:00:00Z |
      | lore_query | single  | 2026-06-17T09:00:00Z |
      | lore_query | single  | 2026-06-22T09:00:00Z |
    When the usage timeseries is folded by week
    Then the usage timeseries has 2 buckets
    And the usage timeseries bucket "2026-06-15" has total calls 2
    And the usage timeseries bucket "2026-06-22" has total calls 1

  Scenario: records with an empty kind do not contribute a call
    Given a usage timeseries record stream:
      | kind       | outcome  | at                   |
      | lore_query | single   | 2026-06-15T09:00:00Z |
      |            | no_match | 2026-06-15T09:30:00Z |
    When the usage timeseries is folded by day
    Then the usage timeseries has 1 buckets
    And the usage timeseries bucket "2026-06-15" has total calls 1

  Scenario: an empty record stream folds to no buckets
    Given an empty usage timeseries record stream
    When the usage timeseries is folded by day
    Then the usage timeseries has 0 buckets

  Scenario: distinct actors are counted per bucket from salted actor hashes
    Given a usage timeseries record stream:
      | kind       | outcome | at                   |
      | lore_query | single  | 2026-06-15T09:00:00Z |
      | lore_query | single  | 2026-06-15T14:00:00Z |
      | lore_query | single  | 2026-06-16T10:00:00Z |
    And a usage timeseries actor step stream:
      | actor_hash | at                   |
      | aaaa1111   | 2026-06-15T09:00:00Z |
      | bbbb2222   | 2026-06-15T14:00:00Z |
      | aaaa1111   | 2026-06-16T10:00:00Z |
    When the usage timeseries is folded by day
    Then the usage timeseries has 2 buckets
    And the usage timeseries bucket "2026-06-15" has distinct actors 2
    And the usage timeseries bucket "2026-06-16" has distinct actors 1

  Scenario: actor records with no hash are excluded from the distinct set
    Given a usage timeseries record stream:
      | kind       | outcome | at                   |
      | lore_query | single  | 2026-06-15T09:00:00Z |
    And a usage timeseries actor step stream:
      | actor_hash | at                   |
      | aaaa1111   | 2026-06-15T09:00:00Z |
      | -          | 2026-06-15T11:00:00Z |
    When the usage timeseries is folded by day
    Then the usage timeseries bucket "2026-06-15" has distinct actors 1

  Scenario: distinct actors collapse the same hash within a week bucket
    Given an empty usage timeseries record stream
    And a usage timeseries actor step stream:
      | actor_hash | at                   |
      | aaaa1111   | 2026-06-15T09:00:00Z |
      | aaaa1111   | 2026-06-17T09:00:00Z |
      | bbbb2222   | 2026-06-22T09:00:00Z |
    When the usage timeseries is folded by week
    Then the usage timeseries has 2 buckets
    And the usage timeseries bucket "2026-06-15" has distinct actors 1
    And the usage timeseries bucket "2026-06-22" has distinct actors 1

  Scenario: cross-hearth merge sums calls and unions distinct actors
    Given hearth A routing+actor stream:
      | kind       | outcome | at                   | actor_hash |
      | lore_query | single  | 2026-06-15T09:00:00Z | aaaa1111   |
      | lore_query | single  | 2026-06-15T10:00:00Z | bbbb2222   |
    And hearth B routing+actor stream:
      | kind       | outcome | at                   | actor_hash |
      | lore_query | single  | 2026-06-15T11:00:00Z | aaaa1111   |
      | lore_query | single  | 2026-06-15T12:00:00Z | cccc3333   |
    When the usage timeseries is folded across both hearths by day
    Then the usage timeseries has 1 buckets
    And the usage timeseries bucket "2026-06-15" has total calls 4
    And the usage timeseries bucket "2026-06-15" has distinct actors 3
    And the usage timeseries bucket "2026-06-15" per-playbook kind "lore_query" has call count 4
