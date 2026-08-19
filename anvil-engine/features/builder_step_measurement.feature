Feature: AC5 — driving the builder emits per-step Temper measurement with real specs
  Driving the builder through the engine emits a step_measurement record per
  (state, role) carrying the measurement_by_role {intent, expected_output} from
  the machine schema (not empty), via the existing M-P3 emit. A working hop keys
  measurement under "doer"; a review-gate hop keys under "reviewer". The begin
  emit carries the gathering doer intent. Empty intent would prove the map was
  keyed by the working role instead of doer/reviewer — a machine fix, not engine.

  Scenario: the begin and first two hops emit non-empty step_measurement records
    Given a hearth seeded with the builder machine and an active parent track
    And the engine is started with that hearth
    When the engine drives a playbook_generation artifact from gathering to completed
    Then the engine stderr contains a JSON log record with fields:
      | event_kind      | step_measurement                   |
      | track_id        | playbook_generation                |
      | playbook_id     | <non-empty>                        |
      | to_state        | gathering                          |
      | role            | doer                               |
      | intent          | <non-empty>                        |
      | expected_output | <non-empty>                        |
    And the engine stderr contains a JSON log record with fields:
      | event_kind      | step_measurement                   |
      | track_id        | playbook_generation                |
      | from_state      | gathering                          |
      | to_state        | gathering_review                   |
      | role            | doer                               |
      | intent          | <non-empty>                        |
      | expected_output | <non-empty>                        |
    And the engine stderr contains a JSON log record with fields:
      | event_kind      | step_measurement                   |
      | track_id        | playbook_generation                |
      | from_state      | gathering_review                   |
      | to_state        | analyzing                          |
      | role            | reviewer                           |
      | intent          | <non-empty>                        |
      | expected_output | <non-empty>                        |
