Feature: UsageTimeSeries + PlaybookStepVolume query surface (gRPC + /ws)
  The dashboard charts a playbook's call volume over time and drills into a
  playbook's per-step volume. Both queries are served over BOTH the gRPC API and
  the loopback /ws JSON-RPC bridge, returning identical folded data. They read
  the durable, redacted sinks the engine already writes (routing-activity.jsonl
  for time series, step-measurement.jsonl for step volume), fold them, and
  return them with the same hearth resolution and read posture as
  PlaybookActivity (standalone no-op, no new auth). Empty sinks fold to empty
  results (no error).

  Scenario: UsageTimeSeries gRPC RPC buckets routing activity by day
    Given a usage query engine hearth seeded with routing activity:
      | kind        | outcome | at                   |
      | lore_query  | single  | 2026-06-15T09:00:00Z |
      | lore_query  | single  | 2026-06-15T14:00:00Z |
      | measurement | single  | 2026-06-16T10:00:00Z |
    And the engine is started with that hearth
    When the UsageTimeSeries RPC is called with granularity "day"
    Then the usage timeseries RPC has 2 buckets
    And the usage timeseries RPC bucket "2026-06-15" has total calls 2
    And the usage timeseries RPC bucket "2026-06-16" has total calls 1
    And the usage timeseries RPC bucket "2026-06-15" per-playbook kind "lore_query" has call count 2

  Scenario: UsageTimeSeries counts ALL command turns (all-turns denominator), not just routes
    # begin + complete + catalog = 3 turns; only the two with a artifact_kind feed
    # per_artifact_kind. The old routing-sink fold would have reported 0 here.
    Given a usage query engine hearth seeded with activity log turns:
      | command  | artifact_kind | at                   |
      | begin    | track         | 2026-06-15T09:00:00Z |
      | complete | track         | 2026-06-15T10:00:00Z |
      | catalog  |               | 2026-06-15T11:00:00Z |
    And the engine is started with that hearth
    When the UsageTimeSeries RPC is called with granularity "day"
    Then the usage timeseries RPC bucket "2026-06-15" has total calls 3
    And the usage timeseries RPC bucket "2026-06-15" per-playbook kind "track" has call count 2

  Scenario: UsageTimeSeries reports begin and complete counts by day
    Given a usage query engine hearth seeded with activity log turns:
      | command  | artifact_kind | at                   |
      | route    | track         | 2026-06-15T09:00:00Z |
      | begin    | track         | 2026-06-15T09:05:00Z |
      | begin    | decision      | 2026-06-15T10:00:00Z |
      | complete | track         | 2026-06-15T11:00:00Z |
      | route    | decision      | 2026-06-16T08:00:00Z |
      | complete | decision      | 2026-06-16T09:00:00Z |
      | begin    | learning      | 2026-06-17T12:00:00Z |
    And the engine is started with that hearth
    When the UsageTimeSeries RPC is called with granularity "day"
    Then the usage timeseries RPC has 3 buckets
    And the usage timeseries RPC bucket "2026-06-15" has total calls 4
    And the usage timeseries RPC bucket "2026-06-15" has begin count 2
    And the usage timeseries RPC bucket "2026-06-15" has complete count 1
    And the usage timeseries RPC bucket "2026-06-16" has begin count 0
    And the usage timeseries RPC bucket "2026-06-16" has complete count 1
    And the usage timeseries RPC bucket "2026-06-17" has total calls 1
    And the usage timeseries RPC bucket "2026-06-17" has begin count 1
    And the usage timeseries RPC bucket "2026-06-17" has complete count 0

  Scenario: UsageTimeSeries over /ws returns the same bucketed data with numeric counts
    Given a usage query engine hearth seeded with routing activity:
      | kind       | outcome | at                   |
      | lore_query | single  | 2026-06-15T09:00:00Z |
      | lore_query | single  | 2026-06-16T10:00:00Z |
    And the engine is started with that hearth
    When a usage_timeseries JSON-RPC request is sent over /ws with granularity "day"
    Then the /ws usage timeseries result has 2 buckets
    And the /ws usage timeseries bucket "2026-06-15" total_calls is a JSON number
    And the /ws usage timeseries bucket "2026-06-15" has total calls 1

  Scenario: PlaybookStepVolume gRPC RPC groups step measurements for a kind
    Given a usage query engine hearth seeded with step measurements:
      | kind       | from_state  | to_state    | role     | at                   |
      | lore_query | active      | in_progress | doer     | 2026-06-15T09:00:00Z |
      | lore_query | active      | in_progress | doer     | 2026-06-15T10:00:00Z |
      | lore_query | in_progress | completed   | reviewer | 2026-06-15T11:00:00Z |
    And the engine is started with that hearth
    When the PlaybookStepVolume RPC is called for kind "lore_query"
    Then the playbook step volume RPC has 2 steps
    And the playbook step volume RPC step from "active" to "in_progress" role "doer" has call count 2

  Scenario: PlaybookStepVolume over /ws returns the same step data
    Given a usage query engine hearth seeded with step measurements:
      | kind       | from_state | to_state    | role | at                   |
      | lore_query | active     | in_progress | doer | 2026-06-15T09:00:00Z |
      | lore_query | active     | in_progress | doer | 2026-06-15T10:00:00Z |
    And the engine is started with that hearth
    When a playbook_step_volume JSON-RPC request is sent over /ws for kind "lore_query"
    Then the /ws playbook step volume result has 1 steps
    And the /ws playbook step volume step from "active" to "in_progress" role "doer" has call count 2

  Scenario: an empty hearth folds to empty results over gRPC
    Given a usage query engine hearth with no sinks
    And the engine is started with that hearth
    When the UsageTimeSeries RPC is called with granularity "day"
    Then the usage timeseries RPC has 0 buckets
    When the PlaybookStepVolume RPC is called for kind "lore_query"
    Then the playbook step volume RPC has 0 steps

  Scenario: UsageTimeSeries reports distinct actors per bucket over gRPC and /ws
    Given a usage query primary hearth seeded with calls:
      | kind       | outcome | at                   | actor_hash |
      | lore_query | single  | 2026-06-15T09:00:00Z | aaaa1111   |
      | lore_query | single  | 2026-06-15T14:00:00Z | bbbb2222   |
      | lore_query | single  | 2026-06-16T10:00:00Z | aaaa1111   |
    And the engine is started with that hearth
    When the UsageTimeSeries RPC is called with granularity "day"
    Then the usage timeseries RPC bucket "2026-06-15" has distinct actors 2
    And the usage timeseries RPC bucket "2026-06-16" has distinct actors 1
    When a usage_timeseries JSON-RPC request is sent over /ws with granularity "day"
    Then the /ws usage timeseries bucket "2026-06-15" has distinct actors 2
    And the /ws usage timeseries bucket "2026-06-15" distinct_actors is a JSON number

  Scenario: all_hearths aggregates two seeded hearths over gRPC
    Given a usage query primary hearth seeded with calls:
      | kind       | outcome | at                   | actor_hash |
      | lore_query | single  | 2026-06-15T09:00:00Z | aaaa1111   |
      | lore_query | single  | 2026-06-15T10:00:00Z | bbbb2222   |
    And a usage query secondary hearth seeded with calls:
      | kind       | outcome | at                   | actor_hash |
      | lore_query | single  | 2026-06-15T11:00:00Z | aaaa1111   |
      | lore_query | single  | 2026-06-15T12:00:00Z | cccc3333   |
    And the engine is started with both permitted hearths
    When the UsageTimeSeries RPC is called with granularity "day" across all hearths
    Then the usage timeseries RPC has 1 buckets
    And the usage timeseries RPC bucket "2026-06-15" has total calls 4
    And the usage timeseries RPC bucket "2026-06-15" has distinct actors 3
    And the usage timeseries RPC included 2 hearths
    And the usage timeseries RPC resolved hearth is the all-hearths sentinel

  Scenario: all_hearths aggregates two seeded hearths over /ws
    Given a usage query primary hearth seeded with calls:
      | kind       | outcome | at                   | actor_hash |
      | lore_query | single  | 2026-06-15T09:00:00Z | aaaa1111   |
    And a usage query secondary hearth seeded with calls:
      | kind       | outcome | at                   | actor_hash |
      | lore_query | single  | 2026-06-15T11:00:00Z | bbbb2222   |
    And the engine is started with both permitted hearths
    When a usage_timeseries JSON-RPC request is sent over /ws with granularity "day" across all hearths
    Then the /ws usage timeseries bucket "2026-06-15" has total calls 2
    And the /ws usage timeseries bucket "2026-06-15" has distinct actors 2
    And the /ws usage timeseries included 2 hearths

  Scenario: PlaybookStepVolume all_hearths sums per-step counts across hearths
    Given a usage query primary hearth seeded with calls:
      | kind       | outcome | at                   | actor_hash |
      | lore_query | single  | 2026-06-15T09:00:00Z | aaaa1111   |
    And a usage query secondary hearth seeded with calls:
      | kind       | outcome | at                   | actor_hash |
      | lore_query | single  | 2026-06-15T11:00:00Z | bbbb2222   |
    And the engine is started with both permitted hearths
    When the PlaybookStepVolume RPC is called for kind "lore_query" across all hearths
    Then the playbook step volume RPC has 1 steps
    And the playbook step volume RPC step from "" to "in_progress" role "doer" has call count 2
    And the playbook step volume RPC included 2 hearths
