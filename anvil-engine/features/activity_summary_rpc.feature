Feature: ActivitySummary RPC folds the universal activity log
  The engine's ActivitySummary read folds the durable, redacted activity-log
  sink into the dashboard's usage summary: total turns, per-command counts,
  per-route-outcome counts, per-playbook counts, and per-period buckets carrying
  total turns and the distinct salted-actor count. The gRPC RPC and the loopback
  `/ws` JSON-RPC method fold the SAME core path, so the two surfaces can never
  diverge; counts are JSON numbers, never strings.

  Scenario: the gRPC ActivitySummary folds the seeded turns
    Given an activity log engine hearth seeded with turns:
      | command | outcome  | artifact_kind | actor_hash | at                   |
      | route   | single   | track         | aaaa1111   | 2026-06-15T09:00:00Z |
      | route   | no_match |               | aaaa1111   | 2026-06-15T09:10:00Z |
      | begin   | ok       | track         | aaaa1111   | 2026-06-15T09:20:00Z |
      | begin   | ok       | track         | bbbb2222   | 2026-06-15T10:00:00Z |
      | catalog | ok       |               | -          | 2026-06-15T11:00:00Z |
    And the engine is started with that hearth
    When the ActivitySummary RPC is called with granularity "day"
    Then the activity summary RPC has total turns 5
    And the activity summary RPC by_command "begin" has count 2
    And the activity summary RPC by_command "route" has count 2
    And the activity summary RPC by_command "catalog" has count 1
    And the activity summary RPC by_route_outcome "single" has count 1
    And the activity summary RPC by_route_outcome "no_match" has count 1
    And the activity summary RPC by_artifact_kind "track" has count 3
    And the activity summary RPC bucket "2026-06-15" has distinct actors 2

  Scenario: the /ws activity_summary folds identically to the gRPC RPC
    Given an activity log engine hearth seeded with turns:
      | command | outcome  | artifact_kind | actor_hash | at                   |
      | route   | single   | track         | aaaa1111   | 2026-06-15T09:00:00Z |
      | begin   | ok       | track         | aaaa1111   | 2026-06-15T09:20:00Z |
      | begin   | ok       | milestone     | bbbb2222   | 2026-06-16T10:00:00Z |
    And the engine is started with that hearth
    When an activity_summary JSON-RPC request is sent over /ws with granularity "day"
    Then the /ws activity summary has total turns 3
    And the /ws activity summary total_turns is a JSON number
    And the /ws activity summary by_command "begin" has count 2
    And the /ws activity summary by_route_outcome "single" has count 1
    And the /ws activity summary by_command "route" has count 1

  Scenario: by_source folds per-harness over seeded route turns (empty as unknown)
    Given an activity log engine hearth seeded with turns:
      | command | outcome  | artifact_kind | source      | actor_hash | at                   |
      | route   | single   | track         | claude-code | aaaa1111   | 2026-06-15T09:00:00Z |
      | route   | no_match |               | codex       | aaaa1111   | 2026-06-15T09:10:00Z |
      | route   | single   | track         | claude-code | bbbb2222   | 2026-06-15T09:20:00Z |
      | route   | no_match |               |             | bbbb2222   | 2026-06-15T09:30:00Z |
      | begin   | ok       | track         | claude-code | aaaa1111   | 2026-06-15T09:40:00Z |
    And the engine is started with that hearth
    When the ActivitySummary RPC is called with granularity "day"
    Then the activity summary RPC by_source "claude-code" has count 2
    And the activity summary RPC by_source "codex" has count 1
    And the activity summary RPC by_source "unknown" has count 1

  Scenario: a route RPC call tags the recorded turn with its harness source
    Given an activity log engine hearth seeded with turns:
      | command | outcome | artifact_kind | actor_hash | at |
    And the engine is started with that hearth
    When the route RPC is called with message "ship the watchdog fix" and source "claude-code"
    And the ActivitySummary RPC is called with granularity "day"
    Then the activity summary RPC by_source "claude-code" has count 1

  Scenario: an empty sink folds to a zeroed summary
    Given an activity log engine hearth seeded with turns:
      | command | outcome | artifact_kind | actor_hash | at |
    And the engine is started with that hearth
    When the ActivitySummary RPC is called with granularity "day"
    Then the activity summary RPC has total turns 0
