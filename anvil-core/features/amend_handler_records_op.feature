Feature: AmendCommandHandler records a valid op (BP2/BP4, AC-2/AC-5)

  # The pure handler validates a candidate op against schema_for_kind(request.kind)
  # + the replayed op log (empty base + add-chain, Q-B), and emits OpRecorded +
  # ActorUpserted. When the artifact's lifecycle-kind machine declares a
  # `<current_state> → amend` edge (BP4: track in `completed`), it also emits
  # TransitionRecorded and sets new_state "amend". op_id is engine-stamped:
  # op-<compact_at>-<seq>, compact_at strips '-' and ':'.

  Scenario: valid Add on a track NOT in an amend-source state is record-only, new_state absent
    Given an in-memory amend query adapter with artifact "tracks/20260604T2114_amend" kind "track" state "implementing"
    When amend is called via the seed registry with:
      | artifact_path   | tracks/20260604T2114_amend |
      | kind            | track                      |
      | target_document | spec                       |
      | target_id       | goal-ship                  |
      | op_kind         | add                        |
      | new_kind        | goal                       |
      | body            | Ship the amend surface     |
      | actor_name      | Doer-200001                |
      | actor_type      | agent                      |
      | actor_model     | claude-opus-4-7            |
      | actor_provider  | anthropic                  |
      | at              | 2026-06-04T21:15:00Z       |
    Then the amend outcome is successful
    And the amend outcome has 2 events
    And the amend outcome event 0 is OpRecorded
    And the amend outcome event 1 is ActorUpserted
    And the amend outcome op_id is "op-20260604T211500Z-0"
    And the amend outcome OpRecorded entry op_id is "op-20260604T211500Z-0"
    And the amend outcome OpRecorded entry target_id is "goal-ship"
    And the amend outcome OpRecorded target_document is "spec"
    And the amend outcome ActorUpserted actor_name is "Doer-200001"
    And the amend outcome new_state is absent

  # BP4: the seed track machine now declares `completed → amend`, so a track in
  # `completed` drives the transition — three events, new_state "amend".
  Scenario: valid Add on a track in completed drives the amend transition
    Given an in-memory amend query adapter with artifact "tracks/20260604T2114_amend_done" kind "track" state "completed"
    When amend is called via the seed registry with:
      | artifact_path   | tracks/20260604T2114_amend_done |
      | kind            | track                           |
      | target_document | spec                            |
      | target_id       | goal-extra                      |
      | op_kind         | add                             |
      | new_kind        | goal                            |
      | body            | An amended goal                 |
      | actor_name      | Doer-200002                     |
      | actor_type      | agent                           |
      | actor_model     | claude-opus-4-7                 |
      | actor_provider  | anthropic                       |
      | at              | 2026-06-04T21:15:00Z            |
    Then the amend outcome is successful
    And the amend outcome has 3 events
    And the amend outcome event 0 is OpRecorded
    And the amend outcome event 1 is ActorUpserted
    And the amend outcome event 2 is TransitionRecorded
    And the amend outcome TransitionRecorded to_state is "amend"
    And the amend outcome new_state is "amend"
