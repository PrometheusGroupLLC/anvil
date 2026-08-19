Feature: JoinCoverage RPC — the conversation-scoped join at the wire

  The dashboard client's seam. What is proven here is the WIRING: the RPC reads
  the real sinks through the real ports over real hearths on disk, resolves the
  requested hearth set, and returns the fold's per-hearth report. The matching
  arithmetic is proven at the core seam and is deliberately not re-proven here.

  Scenario: the RPC reports every kind-bearing delivery row in the window in its episode denominator
    Given a delivery log primary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome    | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-a            | track         | single     | true              |
      | 2026-08-01T01:00:00Z | conv-a            | proposal      | single     | true              |
      | 2026-08-01T02:00:00Z | conv-b            | track         | single     | true              |
      | 2026-08-01T03:00:00Z | conv-b            | -             | candidates | true              |
      | 2026-08-01T04:00:00Z | conv-c            | -             | no_match   | false             |
      | 2026-08-05T00:00:00Z | conv-d            | track         | single     | true              |
    And the engine is started with that hearth
    When the JoinCoverage RPC is called for the window "2026-08-01T00:00:00Z" to "2026-08-02T00:00:00Z"
    Then the per-hearth report for "hearth-primary" counts "delivery_rows_read" as 6
    And the per-hearth report for "hearth-primary" counts "episode_denominator" as 3
    And the per-hearth report for "hearth-primary" counts "menu_delivered" as 1
    And the per-hearth report for "hearth-primary" counts "nothing_delivered" as 1

  Scenario: the buckets sum to the declared denominators, per hearth
    Given a delivery log primary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-a            | track         | single  | true              |
      | 2026-08-01T01:00:00Z | conv-b            | track         | single  | true              |
      | 2026-08-01T02:00:00Z | conv-c            | proposal      | single  | true              |
    And the primary hearth activity log is seeded with turns:
      | command | artifact_kind | at                   | conversation_hash | playbook_run_id | to_state    |
      | begin   | track         | 2026-08-01T00:30:00Z | conv-a            | run-1           | in_progress |
      | begin   | proposal      | 2026-08-01T03:00:00Z | conv-z            | run-2           | in_progress |
    And the engine is started with that hearth
    When the JoinCoverage RPC is called
    Then the per-hearth report for "hearth-primary" counts "joined" as 1
    And the per-hearth report for "hearth-primary" buckets sum to its declared denominators

  Scenario: two hearths return two per-hearth reports and no pooled total
    Given a delivery log primary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-a            | track         | single  | true              |
    And a delivery log secondary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-b            | proposal      | single  | true              |
    And the engine is started with both permitted hearths
    When the JoinCoverage RPC is called naming both hearths in the hearth list
    Then the JoinCoverage response has 2 per-hearth reports
    And the per-hearth report labels are "hearth-primary,hearth-secondary" in any order
    And the serialized report exposes no pooled coverage key

  Scenario: hearth_path and hearth_paths supplied together yield one report per distinct hearth
    Given a delivery log primary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-a            | track         | single  | true              |
    And a delivery log secondary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-b            | proposal      | single  | true              |
    And the engine is started with both permitted hearths
    When the JoinCoverage RPC is called naming the primary hearth in both request fields and the secondary hearth in the hearth list
    Then the JoinCoverage response has 2 per-hearth reports
    And the per-hearth report labels are "hearth-primary,hearth-secondary" in any order

  Scenario: the same hearth named twice in two spellings yields one report whose delivery denominator counts each row once
    Given a delivery log primary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-a            | track         | single  | true              |
      | 2026-08-01T01:00:00Z | conv-b            | track         | single  | true              |
      | 2026-08-01T02:00:00Z | conv-c            | proposal      | single  | true              |
    And the engine is started with that hearth
    When the JoinCoverage RPC is called naming the primary hearth twice in two spellings
    Then the JoinCoverage response has 1 per-hearth reports
    And the per-hearth report for "hearth-primary" counts "delivery_rows_read" as 3
    And the per-hearth report for "hearth-primary" counts "episode_denominator" as 3

  Scenario: an empty entry in the hearth list beside a valid one yields exactly one report and does not fold the engine default
    Given a delivery log primary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-a            | track         | single  | true              |
    And a delivery log secondary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-b            | proposal      | single  | true              |
    And the engine is started with both permitted hearths
    When the JoinCoverage RPC is called naming the secondary hearth beside an empty hearth list entry
    Then the JoinCoverage response has 1 per-hearth reports
    And the per-hearth report labels are "hearth-secondary" in any order

  Scenario: a request naming one permitted and one non-permitted hearth returns permission_denied and no report
    Given a delivery log primary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-a            | track         | single  | true              |
    And the engine is started with that hearth
    When the JoinCoverage RPC is called naming the primary hearth and the unpermitted hearth
    Then the JoinCoverage RPC fails with "PermissionDenied"
    And no JoinCoverage report was returned

  Scenario: two requests naming the same two hearths in opposite order return identical responses
    Given a delivery log primary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-a            | track         | single  | true              |
    And a delivery log secondary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-b            | proposal      | single  | true              |
    And the engine is started with both permitted hearths
    When the JoinCoverage RPC is called naming both hearths in the hearth list
    And the same JoinCoverage request is reissued with its hearth list reversed
    Then the two JoinCoverage responses are identical

  Scenario: two hearths whose directory names are identical get distinct labels, and an episode in one never joins a begin in the other
    Given two same-named delivery log hearths seeded with rows:
      | hearth    | at                   | conversation_hash | guidance_kind | outcome | guidance_produced |
      | primary   | 2026-08-01T00:00:00Z | conv-a            | track         | single  | true              |
      | secondary | 2026-08-01T00:00:00Z | conv-b            | track         | single  | true              |
    And the secondary hearth activity log is seeded with turns:
      | command | artifact_kind | at                   | conversation_hash | playbook_run_id | to_state    |
      | begin   | track         | 2026-08-01T01:00:00Z | conv-a            | run-1           | in_progress |
    And the engine is started with both permitted hearths
    When the JoinCoverage RPC is called naming both hearths in the hearth list
    Then the per-hearth report labels are "shared-hearth#1,shared-hearth#2" in any order
    And the per-hearth report for "shared-hearth#1" counts "joined" as 0
    And the per-hearth report for "shared-hearth#2" counts "joined" as 0
    And the per-hearth report for "shared-hearth#2" counts "begin_unjoin.conversation_absent_from_delivery_side" as 1

  Scenario: every per-hearth entry in one report carries the same key epoch
    Given a delivery log primary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-a            | track         | single  | true              |
    And a delivery log secondary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-b            | proposal      | single  | true              |
    And a telemetry salt file containing "deployment-salt-v1" in the "secondary" hearth
    And a telemetry salt file containing "a-local-drifted-salt" in the "primary" hearth
    And the engine is started with both permitted hearths
    When the JoinCoverage RPC is called naming both hearths in the hearth list
    Then every per-hearth report carries the same key epoch
    And every per-hearth report key epoch is 12 lowercase hex characters or the unknown sentinel

  Scenario: a hearth whose telemetry salt epoch differs from the effective one reports DiffersFromEffective with every count unchanged
    Given a delivery log primary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-a            | track         | single  | true              |
      | 2026-08-01T01:00:00Z | conv-b            | track         | single  | true              |
      | 2026-08-01T02:00:00Z | conv-c            | proposal      | single  | true              |
    And a delivery log secondary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-d            | proposal      | single  | true              |
    And the primary hearth activity log is seeded with turns:
      | command | artifact_kind | at                   | conversation_hash | playbook_run_id | to_state    |
      | begin   | track         | 2026-08-01T00:30:00Z | conv-a            | run-1           | in_progress |
    And a telemetry salt file containing "deployment-salt-v1" in the "secondary" hearth
    And a telemetry salt file containing "a-local-drifted-salt" in the "primary" hearth
    And the engine is started with both permitted hearths
    When the JoinCoverage RPC is called naming both hearths in the hearth list
    Then the per-hearth report for "hearth-primary" declares "key_epoch_reconciliation" as "DiffersFromEffective"
    And the per-hearth report for "hearth-secondary" declares "key_epoch_reconciliation" as "MatchesEffective"
    And the per-hearth report for "hearth-primary" counts "delivery_rows_read" as 3
    And the per-hearth report for "hearth-primary" counts "episode_denominator" as 3
    And the per-hearth report for "hearth-primary" counts "joined" as 1

  Scenario: a read against a deployment with no resolvable salt reports the unknown key epoch and creates no telemetry salt file
    Given a delivery log primary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-a            | track         | single  | true              |
    And a delivery log secondary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-b            | proposal      | single  | true              |
    And the engine is started with both permitted hearths
    When the JoinCoverage RPC is called naming both hearths in the hearth list
    Then every per-hearth report declares "key_epoch" as "unknown_key_epoch"
    And no telemetry salt file exists in the "primary" hearth
    And no telemetry salt file exists in the "secondary" hearth

  Scenario: the response carries no salt value and no telemetry salt contents anywhere
    Given a delivery log primary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-a            | track         | single  | true              |
    And a delivery log secondary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-b            | proposal      | single  | true              |
    And a telemetry salt file containing "deployment-salt-v1" in the "secondary" hearth
    And a telemetry salt file containing "a-local-drifted-salt" in the "primary" hearth
    And the engine is started with both permitted hearths
    When the JoinCoverage RPC is called naming both hearths in the hearth list
    Then the serialized JoinCoverage response does not contain raw text "deployment-salt-v1"
    And the serialized JoinCoverage response does not contain raw text "a-local-drifted-salt"

  Scenario: a hearth with no delivery log returns a zeroed per-hearth report, not an error
    Given a delivery log primary hearth with no sinks
    And the engine is started with that hearth
    When the JoinCoverage RPC is called
    Then the JoinCoverage response has 1 per-hearth reports
    And the per-hearth report for "hearth-primary" counts "delivery_rows_read" as 0
    And the per-hearth report for "hearth-primary" counts "read_defects" as 0
    And the per-hearth report for "hearth-primary" counts "episode_denominator" as 0
    And the per-hearth report for "hearth-primary" counts "begin_rows_read" as 0

  Scenario: the report carries the window, the rows scanned, the rows retained, the filter version and a per-hearth key epoch
    Given a delivery log primary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-a            | track         | single  | true              |
      | 2026-08-01T01:00:00Z | conv-b            | track         | single  | true              |
    And the primary hearth activity log is seeded with turns:
      | command | artifact_kind | at                   | conversation_hash | playbook_run_id | to_state    |
      | begin   | track         | 2026-08-01T00:30:00Z | conv-a            | run-1           | in_progress |
      | begin   | track         | 2026-08-01T01:30:00Z | conv-b            | run-2           | in_progress |
      | route   | track         | 2026-08-01T02:00:00Z | conv-a            | -               | -           |
      | route   | track         | 2026-08-01T02:30:00Z | conv-b            | -               | -           |
    And the engine is started with that hearth
    When the JoinCoverage RPC is called for the window "2026-08-01T00:00:00Z" to "2026-09-01T00:00:00Z"
    Then the per-hearth report for "hearth-primary" declares "window_start" as "2026-08-01T00:00:00Z"
    And the per-hearth report for "hearth-primary" declares "window_end" as "2026-09-01T00:00:00Z"
    And the per-hearth report for "hearth-primary" counts "activity_rows_scanned" as 4
    And the per-hearth report for "hearth-primary" counts "activity_rows_retained" as 2
    And every per-hearth report declares a non-empty "key_epoch"
    And the report filter version is the "JOIN_FILTER_VERSION" constant

  Scenario: the report's filter version is the JOIN_FILTER_VERSION constant
    Given a delivery log primary hearth with no sinks
    And the engine is started with that hearth
    When the JoinCoverage RPC is called
    Then the report filter version is the "JOIN_FILTER_VERSION" constant

  Scenario: the response carries no raw salt, no raw conversation id and no path
    Given a delivery log primary hearth seeded with rows:
      | at                   | conversation_hash        | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-raw-identity-marker | track         | single  | true              |
    And a telemetry salt file containing "brine-test-salt" in the "primary" hearth
    And the engine is started with that hearth
    When the JoinCoverage RPC is called
    Then the serialized JoinCoverage response does not contain raw text "conv-raw-identity-marker"
    And the serialized JoinCoverage response does not contain raw text "brine-test-salt"
    And the serialized JoinCoverage response contains no hearth path

  Scenario: reading a hearth whose activity log holds far more non-join-relevant rows than join-relevant ones retains only the latter
    Given a delivery log primary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-a            | track         | single  | true              |
    And the primary hearth activity log is seeded with turns:
      | command  | artifact_kind | at                   | conversation_hash | playbook_run_id | to_state    |
      | route    | track         | 2026-08-01T00:01:00Z | conv-a            | -               | -           |
      | route    | track         | 2026-08-01T00:02:00Z | conv-a            | -               | -           |
      | route    | track         | 2026-08-01T00:03:00Z | conv-a            | -               | -           |
      | route    | track         | 2026-08-01T00:04:00Z | conv-a            | -               | -           |
      | route    | track         | 2026-08-01T00:05:00Z | conv-a            | -               | -           |
      | route    | track         | 2026-08-01T00:06:00Z | conv-a            | -               | -           |
      | begin    | track         | 2026-08-01T00:30:00Z | conv-a            | run-1           | in_progress |
      | snapshot | track         | 2026-08-01T00:40:00Z | conv-a            | run-1           | reviewing   |
    And the engine is started with that hearth
    When the JoinCoverage RPC is called
    Then the per-hearth report for "hearth-primary" counts "activity_rows_scanned" as 8
    And the per-hearth report for "hearth-primary" counts "activity_rows_retained" as 2

  # THE END-TO-END PROOF. Every scenario above seeds its sinks through the
  # production adapters, which proves the RPC reads what the ports write — but
  # not that the production WRITERS produce rows the fold can pair. This one
  # drives the real `anvil-hooks` binary and the real begin RPC against one
  # engine, then reads the result back through the shipped RPC. It is the first
  # non-zero join in the repository, and it is evidence about the HOOK: the fold
  # was already proven on seeded vectors, so a green fold cannot stand in for a
  # working writer.
  Scenario: a turn routed and begun end-to-end through the real binaries appears as a joined episode
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the kiln router HTTP stub returns verdict kind "daily_recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with stdin UserPromptSubmit session_id "sess-join-e2e-1" prompt "daily recap" against that engine
    And the begin RPC is called to create a "daily_recap" artifact named "the end to end recap" with no parent for conversation "sess-join-e2e-1" and project root "/tmp/anvil-join-e2e-project"
    And the JoinCoverage RPC is called
    Then the activity log command "begin" carries correlation keys for project root "/tmp/anvil-join-e2e-project"
    And the only per-hearth report counts "episode_denominator" as 1
    And the only per-hearth report counts "begin_denominator" as 1
    And the only per-hearth report counts "joined" as 1
