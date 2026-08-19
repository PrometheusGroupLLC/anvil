Feature: BP2 — daily_recap drives end-to-end with per-step measurement
  A temp owner-home seeded from the daily_recap test fixture is registry-resolvable,
  and the real engine can begin, check in, and drive a daily_recap artifact through
  gathering -> gathering_review -> synthesizing -> synthesizing_review -> reporting
  -> reporting_review -> completed. Every begin/complete step emits a
  step_measurement record with the state, role, and non-empty machine-declared
  measurement fields.

  Scenario: drive a daily_recap artifact gathering -> completed with measurement per step
    Given a hearth seeded with the daily_recap machine
    And the temp owner-home registry resolves daily_recap with states and measurements
    And the engine is started with that hearth
    When the engine begins, checks in, and drives a daily_recap artifact from gathering to completed
    Then the e2e final state is "completed"
    And the e2e artifact resolved state is "completed"
    And the e2e artifact status.yaml contains "kind: daily_recap"
    And the e2e artifact path starts with "daily_recaps/"
    And the engine stderr contains exactly 7 JSON log records with event_kind "step_measurement", track_id "daily_recap", and to_state in:
      | gathering            |
      | gathering_review     |
      | synthesizing         |
      | synthesizing_review  |
      | reporting            |
      | reporting_review     |
      | completed            |
    And the engine stderr contains a JSON log record with fields:
      | event_kind      | step_measurement |
      | track_id        | daily_recap      |
      | playbook_id     | <non-empty>      |
      | to_state        | gathering        |
      | role            | doer             |
      | intent          | <non-empty>      |
      | expected_output | <non-empty>      |
    And the engine stderr contains a JSON log record with fields:
      | event_kind      | step_measurement |
      | track_id        | daily_recap      |
      | from_state      | gathering        |
      | to_state        | gathering_review |
      | role            | doer             |
      | intent          | <non-empty>      |
      | expected_output | <non-empty>      |
    And the engine stderr contains a JSON log record with fields:
      | event_kind      | step_measurement |
      | track_id        | daily_recap      |
      | from_state      | gathering_review |
      | to_state        | synthesizing     |
      | role            | reviewer         |
      | intent          | <non-empty>      |
      | expected_output | <non-empty>      |
    And the engine stderr contains a JSON log record with fields:
      | event_kind      | step_measurement    |
      | track_id        | daily_recap         |
      | from_state      | synthesizing        |
      | to_state        | synthesizing_review |
      | role            | doer                |
      | intent          | <non-empty>         |
      | expected_output | <non-empty>         |
    And the engine stderr contains a JSON log record with fields:
      | event_kind      | step_measurement    |
      | track_id        | daily_recap         |
      | from_state      | synthesizing_review |
      | to_state        | reporting           |
      | role            | reviewer            |
      | intent          | <non-empty>         |
      | expected_output | <non-empty>         |
    And the engine stderr contains a JSON log record with fields:
      | event_kind      | step_measurement |
      | track_id        | daily_recap      |
      | from_state      | reporting        |
      | to_state        | reporting_review |
      | role            | doer             |
      | intent          | <non-empty>      |
      | expected_output | <non-empty>      |
    And the engine stderr contains a JSON log record with fields:
      | event_kind      | step_measurement |
      | track_id        | daily_recap      |
      | from_state      | reporting_review |
      | to_state        | completed        |
      | role            | reviewer         |
      | intent          | <non-empty>      |
      | expected_output | <non-empty>      |
