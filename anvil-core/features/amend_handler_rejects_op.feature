Feature: AmendCommandHandler rejects invalid ops with B5a typed codes (BP2, AC-1)

  # Validation delegates to B5a apply/validate_op; the handler wraps the typed
  # error so its code() propagates.

  Scenario: Add whose new_kind only allows Revise is rejected amendment_op_not_in_schema
    Given an in-memory amend query adapter with artifact "tracks/20260604T2114_amend" kind "track" state "completed"
    When amend is called via the seed registry with:
      | artifact_path   | tracks/20260604T2114_amend |
      | kind            | track                      |
      | target_document | spec                       |
      | target_id       | overview                   |
      | op_kind         | add                        |
      | new_kind        | overview                   |
      | body            | a second overview          |
      | actor_name      | Doer-200004                |
      | actor_type      | agent                      |
      | actor_model     | claude-opus-4-7            |
      | actor_provider  | anthropic                  |
      | at              | 2026-06-04T21:15:00Z       |
    Then the amend outcome is an AmendError containing "amendment_op_not_in_schema"

  Scenario: Add missing body is rejected amendment_invalid_add
    Given an in-memory amend query adapter with artifact "tracks/20260604T2114_amend" kind "track" state "completed"
    When amend is called via the seed registry with:
      | artifact_path   | tracks/20260604T2114_amend |
      | kind            | track                      |
      | target_document | spec                       |
      | target_id       | goal-nobody                |
      | op_kind         | add                        |
      | new_kind        | goal                       |
      | actor_name      | Doer-200005                |
      | actor_type      | agent                      |
      | actor_model     | claude-opus-4-7            |
      | actor_provider  | anthropic                  |
      | at              | 2026-06-04T21:15:00Z       |
    Then the amend outcome is an AmendError containing "amendment_invalid_add"

  Scenario: an unknown amendment kind is rejected
    Given an in-memory amend query adapter with artifact "tracks/20260604T2114_amend" kind "track" state "completed"
    When amend is called via the seed registry with:
      | artifact_path   | tracks/20260604T2114_amend |
      | kind            | not_a_kind                 |
      | target_document | spec                       |
      | target_id       | goal-x                     |
      | op_kind         | add                        |
      | new_kind        | goal                       |
      | body            | body                       |
      | actor_name      | Doer-200006                |
      | actor_type      | agent                      |
      | actor_model     | claude-opus-4-7            |
      | actor_provider  | anthropic                  |
      | at              | 2026-06-04T21:15:00Z       |
    Then the amend outcome is an AmendError containing "amend_unknown_amendment_kind"

  Scenario: an empty actor_name is rejected
    Given an in-memory amend query adapter with artifact "tracks/20260604T2114_amend" kind "track" state "completed"
    When amend is called via the seed registry with:
      | artifact_path   | tracks/20260604T2114_amend |
      | kind            | track                      |
      | target_document | spec                       |
      | target_id       | goal-x                     |
      | op_kind         | add                        |
      | new_kind        | goal                       |
      | body            | body                       |
      | actor_type      | agent                      |
      | actor_model     | claude-opus-4-7            |
      | actor_provider  | anthropic                  |
      | at              | 2026-06-04T21:15:00Z       |
    Then the amend outcome is an AmendError containing "actor_name_required"
