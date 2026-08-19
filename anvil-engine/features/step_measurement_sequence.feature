Feature: A begin -> checkin -> complete sequence emits exactly three step_measurement records
  The DONE criterion (AC-7): each lifecycle call emits exactly one
  step_measurement record, so a begin -> checkin -> complete run produces
  EXACTLY 3 records total, each carrying the correct from_state/to_state/role/
  actor per D-3.

  Scenario: Three lifecycle calls produce three step_measurement records
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260419T1100_seq_complete_track/           | spec    |
    And the track "20260419T1100_seq_complete_track" has spec.md with content "# Seq Complete Track\n\nSpec body."
    And a playbook hook body for the spec doer hook "spec-writing.md" with content "Spec writing guidance."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Seq Complete Track](tracks/20260419T1100_seq_complete_track/) — seq complete track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## spec_review

      ## plan

      ## implementing
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-04-19T00:00:00Z
      last_updated: 2026-04-19T00:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Spec (0)

      ## Spec Review (0)

      ## Planned (0)

      ## Implementing (0)
      """
    And the engine is started with that hearth
    When the begin RPC is called to create a track named "seq begin track" under parent "20260411T2021_anvil_workflow_engine"
    # M7 — Begin event: playbook_id must equal the begin-created run instance id
    # (NOT the playbook definition id, NOT the kind). Asserted HERE, immediately
    # after begin, while the begin RPC response is still in context. track_id =
    # "track" (the kind) in the same record proves playbook_id != track_id —
    # matching the strength of the complete-event assertion below.
    And the engine stderr contains a JSON log record with playbook_id equal to the begin RPC response run id and fields:
      | event_kind  | step_measurement |
      | from_state  |                  |
      | to_state    | spec             |
      | role        | doer             |
      | track_id    | track            |
      | intent      | <non-empty>      |
    And the checkin RPC is called with role "creator" and actor_name "Seq-Checkin-000042"
    And the complete RPC is called with:
      | artifact_path        | tracks/20260419T1100_seq_complete_track |
      | actor_name           | Seq-Doer-111111                         |
      | actor_type           | agent                                   |
      | actor_model          | claude-opus-4-7                         |
      | actor_provider       | anthropic                               |
      | actor_context_window | 200000                                  |
      | actor_entrypoint     | claude-code                             |
    Then the complete RPC response new_state is "spec_review"
    And the engine stderr contains exactly 3 JSON log records with event_kind "step_measurement"
    And the engine stderr contains a JSON log record with fields:
      | event_kind | step_measurement   |
      | role       | creator            |
      | actor      | Seq-Checkin-000042 |
      | to_state   |                    |
      | track_id   |                    |
    And the engine stderr contains a JSON log record with fields:
      | event_kind  | step_measurement                 |
      | from_state  | spec                             |
      | to_state    | spec_review                      |
      | role        | doer                             |
      | track_id    | track                            |
      | playbook_id | 20260419T1100_seq_complete_track |
      | intent      | <non-empty>                      |
