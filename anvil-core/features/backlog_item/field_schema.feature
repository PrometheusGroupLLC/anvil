Feature: K8 backlog_item field schema and semantic validators
  The closed value model (D1/D1.2) is deny-unknown-fields and every enum,
  ID grammar, and cross-field invariant is validated before any write. These
  scenarios pin the frozen field rules the Task 2 validators must enforce.

  Background:
    Given a backlog fixture

  @schema
  Scenario: A well-formed candidate genesis input validates
    When the backlog field schema is validated against input "valid_candidate"
    Then the backlog operation succeeds

  @schema
  Scenario Outline: Malformed or forbidden fields reject before any write
    When the backlog field schema is validated against input "<input>"
    Then the backlog field rule "<rule>" holds
    And no backlog residue remains under "backlog_items"

    Examples:
      | input                         | rule                              |
      | unknown_key                   | deny_unknown_fields               |
      | caller_supplied_id            | engine_owned_id                   |
      | caller_supplied_state         | engine_owned_state               |
      | caller_supplied_rank          | engine_owned_rank                 |
      | caller_supplied_history       | engine_owned_history              |
      | uppercase_bi_id               | crockford_lowercase               |
      | excluded_letter_bn_id         | crockford_excludes_iluo           |
      | short_bn_body                 | bn_min_body_10                    |
      | short_lr_body                 | lr_min_body_26                    |
      | bad_action_class              | closed_action_enum                |
      | bad_effort_class              | closed_effort_enum                |
      | bad_evidence_kind             | closed_evidence_kind              |
      | empty_evidence_refs           | intake_evidence_nonempty          |
      | non_finite_magnitude          | numeric_finite                    |
      | rank_position_zero            | rank_position_min_1               |
      | effort_mirror_mismatch        | effort_class_mirror_equal         |
      | non_temper_value_gap_ref      | value_gap_temper_only             |
      | both_predictor_ids            | origin_predictors_mutually_exclusive |
      | predictor_without_value       | origin_predictor_requires_value   |
      | value_without_predictor       | origin_value_requires_predictor   |
      | null_playbook_without_route   | null_playbook_implies_route       |
      | bad_initial_reading_status    | outcome_initial_status_closed     |

  @schema
  Scenario Outline: The three exact origin predictor/value cases are accepted
    When the backlog field schema is validated against input "<input>"
    Then the backlog operation succeeds

    Examples:
      | input                         |
      | origin_both_null              |
      | origin_council_finite         |
      | origin_experiment_finite      |
