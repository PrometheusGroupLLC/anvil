Feature: AmendCommandHandler op-log-only validation (BP2, AC-3, Q-B)

  # Op-log-only: the base is empty; the known-element set is base ∪ prior-Add ops
  # in the same document's log. An op targeting an element introduced by a prior
  # Add validates; an op targeting a never-Added id is rejected
  # amendment_unknown_element. Pre-existing markdown elements are OUT of scope
  # (they need the deferred markdown ingest follow-on).

  Scenario: Revise of an element added by a prior op in the same log validates (add-chain)
    Given an in-memory amend query adapter with artifact "tracks/20260604T2114_amend" kind "track" state "completed"
    And the amend query adapter has op log for "tracks/20260604T2114_amend" document "spec" with op_id "op-20260604T211500Z-0" accepted_at "2026-06-04T21:15:00Z" seq 0 target_id "goal-ship" op_kind "add" new_kind "goal" body "Ship it"
    When amend is called via the seed registry with:
      | artifact_path   | tracks/20260604T2114_amend |
      | kind            | track                      |
      | target_document | spec                       |
      | target_id       | goal-ship                  |
      | op_kind         | revise                     |
      | body            | Ship it well               |
      | actor_name      | Doer-200002                |
      | actor_type      | agent                      |
      | actor_model     | claude-opus-4-7            |
      | actor_provider  | anthropic                  |
      | at              | 2026-06-04T21:16:00Z       |
    Then the amend outcome is successful
    And the amend outcome op_id is "op-20260604T211600Z-1"
    And the amend outcome OpRecorded entry op_id is "op-20260604T211600Z-1"

  Scenario: Revise of a never-Added id is rejected amendment_unknown_element
    Given an in-memory amend query adapter with artifact "tracks/20260604T2114_amend" kind "track" state "completed"
    When amend is called via the seed registry with:
      | artifact_path   | tracks/20260604T2114_amend |
      | kind            | track                      |
      | target_document | spec                       |
      | target_id       | goal-preexisting           |
      | op_kind         | revise                     |
      | body            | revise a markdown element  |
      | actor_name      | Doer-200003                |
      | actor_type      | agent                      |
      | actor_model     | claude-opus-4-7            |
      | actor_provider  | anthropic                  |
      | at              | 2026-06-04T21:17:00Z       |
    Then the amend outcome is an AmendError containing "amendment_unknown_element"
