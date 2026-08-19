Feature: Begin command RPC emits a step_measurement record
  On each begin call, the engine emits exactly one flat
  event_kind="step_measurement" record. begin models "entering to_state to
  work", so from_state is always empty; to_state is the state entered, role is
  the doer|reviewer axis of the flow, and intent/expected_output come from the
  (to_state, role) schema. Covers create (->spec, doer), review (->spec_review,
  reviewer), and playbook-creation (->draft, doer; no measurement declared, so
  empty intent). tokens/duration_ms are absent (D-4).

  Scenario: Begin create-track emits one step_measurement with to_state spec and doer intent
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
    And a playbook hook body for the spec doer hook "spec-writing.md" with content "Spec writing guidance."
    And the engine is started with that hearth
    When the begin RPC is called to create a track named "measure begin create" under parent "20260411T2021_anvil_workflow_engine"
    Then the begin RPC response has non-empty "track_path"
    And the engine stderr contains exactly 1 JSON log records with event_kind "step_measurement"
    And the engine stderr contains a JSON log record with fields:
      | event_kind      | step_measurement              |
      | playbook_id     | <non-empty>                   |
      | track_id        | track                         |
      | from_state      |                               |
      | to_state        | spec                          |
      | role            | doer                          |
      | actor           | Rpc-Test-000000               |
      | intent          | <non-empty>                   |
      | expected_output | <non-empty>                   |
      | at              | <non-empty>                   |
    And the engine stderr event_kind "step_measurement" log record has no "tokens" field
    And the engine stderr event_kind "step_measurement" log record has no "duration_ms" field

  Scenario: Begin review emits one step_measurement with to_state spec_review and reviewer intent
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260411T2021_anvil_workflow_engine/     | active       |
      | tracks/20260414T0405_review_spec_strand/           | spec_review  |
    And the track "20260414T0405_review_spec_strand" has spec.md with content "# Review Spec Strand\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      ## spec_review

      - [review spec strand](tracks/20260414T0405_review_spec_strand/) — review spec strand
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-04-14T00:00:00Z
      last_updated: 2026-04-14T00:00:00Z
      after_event: "seed"
      ---

      # Anvil — State of Execution

      ## Spec Review (1)

      | Track | Proposal |
      |-------|----------|
      | review spec strand | anvil-playbook-engine |

      ## Spec (0)
      """
    And a playbook hook body for the spec_review reviewer hook "spec-review.md" with content "Combined review protocol and spec criteria."
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260414T0405_review_spec_strand" and session_role "reviewer"
    Then the begin RPC response state is "spec_review"
    And the engine stderr contains exactly 1 JSON log records with event_kind "step_measurement"
    And the engine stderr contains a JSON log record with fields:
      | event_kind      | step_measurement                 |
      | playbook_id     | 20260414T0405_review_spec_strand |
      | track_id        | track                            |
      | from_state      |                                  |
      | to_state        | spec_review                      |
      | role            | reviewer                         |
      | intent          | <non-empty>                     |
      | expected_output | <non-empty>                     |
      | at              | <non-empty>                     |
    And the engine stderr event_kind "step_measurement" log record has no "tokens" field
    And the engine stderr event_kind "step_measurement" log record has no "duration_ms" field
