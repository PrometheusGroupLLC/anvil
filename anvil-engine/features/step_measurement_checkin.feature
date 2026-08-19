Feature: Checkin query RPC emits a degenerate step_measurement record
  checkin is orientation, not a state transition — there is no artifact, kind,
  or state in scope. It still emits one flat event_kind="step_measurement"
  record (best-effort, D-3): role + actor + at populated, every other contract
  key present-but-empty. tokens/duration_ms are absent (D-4).

  Scenario: Checkin emits one step_measurement with role + actor + at and empty step fields
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
    And the engine is started with that hearth
    When the checkin RPC is called with role "creator" and actor_name "Checkin-Actor-000042"
    Then the checkin RPC response actor_name is "Checkin-Actor-000042"
    And the engine stderr contains exactly 1 JSON log records with event_kind "step_measurement"
    And the engine stderr contains a JSON log record with fields:
      | event_kind      | step_measurement     |
      | role            | creator              |
      | actor           | Checkin-Actor-000042 |
      | at              | <non-empty>          |
      | playbook_id     |                      |
      | track_id        |                      |
      | from_state      |                      |
      | to_state        |                      |
      | intent          |                      |
      | expected_output |                      |
    And the engine stderr event_kind "step_measurement" log record has no "tokens" field
    And the engine stderr event_kind "step_measurement" log record has no "duration_ms" field
