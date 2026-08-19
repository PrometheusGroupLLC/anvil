Feature: K8 backlog_item engine gRPC API
  The engine exposes genesis through the existing Begin RPC, lifecycle through
  the existing Snapshot RPC, and three new typed RPCs (BacklogMutate,
  BacklogEvaluate, BacklogQueue). The engine — never the caller — stamps audit
  data, history sequence, actor role, and IDs. Alternate writers (Complete,
  Amend, adoption, internal execute, raw append) are rejected for K8.

  Background:
    Given a running backlog engine

  @genesis
  Scenario: Begin creates an exact-ID candidate at backlog_items/bi_...
    When a backlog Begin genesis RPC with input "valid_candidate"
    Then the backlog RPC succeeds
    And the backlog RPC state is "candidate"
    And the created backlog path matches "backlog_items/bi_"

  @genesis
  Scenario Outline: Begin genesis rejects forged or malformed input with no residue
    When a backlog Begin genesis RPC with input "<input>"
    Then the backlog RPC returns a non-OK status
    And the backlog RPC error contains "<reason>"
    And no backlog directory exists under "backlog_items"

    Examples:
      | input                    | reason              |
      | caller_supplied_id       | `backlog_item_id` is engine_owned |
      | caller_supplied_state    | `state` is engine_owned |
      | caller_supplied_rank     | `rank` is engine_owned |
      | caller_supplied_history  | `history` is engine_owned |
      | unknown_key              | unknown field `totally_unknown` |
      | missing_evidence         | evidence            |
      | missing_origin           | origin              |
      | both_predictor_ids       | predictor           |
      | predictor_without_value  | predictor           |
      | invalid_bn_id            | bn_                 |

  @genesis
  Scenario: A genesis restart persists exactly created(seq:0) with no partial write
    Given a backlog item created through Begin with input "valid_candidate"
    When a backlog Begin genesis RPC with input "reread_after_restart"
    Then the backlog RPC succeeds
    And the backlog RPC state is "candidate"

  @snapshot
  Scenario Outline: Governed Snapshot lands the printed public rows
    Given a backlog item created through Begin with input "row_<row>"
    When a backlog Snapshot RPC from "<from>" to "<to>" with role "<role>"
    Then the backlog RPC succeeds
    And the backlog RPC state is "<to>"

    Examples:
      | row | from       | to         | role         |
      | 1   | candidate  | ready      | organ_loop   |
      | 3   | candidate  | superseded | nick_shape   |
      | 5   | ready      | in_flight  | track_driver |
      | 10  | in_flight  | done       | nick_shape   |
      | 13  | parked     | candidate  | nick_shape   |

  @snapshot
  Scenario: Row #10 engine_auto lands only with a stored reading through PublicSnapshot
    Given a backlog item created through Begin with input "row_10_reading"
    When a backlog Snapshot RPC from "in_flight" to "done" with role "engine_auto"
    Then the backlog RPC succeeds
    And the backlog RPC state is "done"

  @snapshot
  Scenario Outline: A raw Snapshot cannot forge engine_auto for the evaluation-only rows
    Given a backlog item created through Begin with input "row_<row>"
    When a backlog Snapshot RPC from "<from>" to "<to>" with role "engine_auto"
    Then the backlog RPC returns a non-OK status
    And the backlog RPC error contains "engine_auto"
    And the backlog engine files are byte-identical

    Examples:
      | row | from       | to         |
      | 4   | candidate  | aged_out   |
      | 9   | ready      | aged_out   |
      | 16  | parked     | aged_out   |

  @snapshot
  Scenario Outline: Alternate state writers are rejected for backlog_item
    Given a backlog item created through Begin with input "valid_candidate"
    When a raw Snapshot bypass attempt via "<route>"
    Then the backlog RPC returns a non-OK status
    And the backlog engine files are byte-identical

    Examples:
      | route            |
      | complete         |
      | amend            |
      | adoption         |
      | internal_execute |
      | raw_append       |

  # 27 became 28 with T-RD-PBK-DEPTH's read-only `RunDetail`, declared beside the
  # other read RPCs rather than after the K8 three — so the "K8 appended LAST, in
  # order" half of this check (asserted in the step) is untouched.
  # 28 became 29 with T1's read-only `JoinCoverage`, declared in the same place
  # and for the same reason: the K8 three are still the last three.
  @transport
  Scenario: The service exposes exactly 29 RPCs at the unchanged wire version
    When the backlog service RPC count is inspected
    Then the backlog RPC succeeds
    And the backlog RPC state is "29"

  @transport
  Scenario Outline: BacklogMutate performs each named operation with its fixed role
    Given a backlog item created through Begin with input "mutable_<operation>"
    When a BacklogMutate RPC for operation "<operation>" with role "<role>"
    Then the backlog RPC succeeds
    And the backlog RPC state is "<operation>"
    And the stored backlog history ends with "<kind>" by role "<role>"
    And the stored backlog item shows "<effect>"

    Examples:
      | operation               | role         | kind            | effect                        |
      | shape_edit              | organ_loop   | shape_edited    | effort_class_and_playbook     |
      | recompute_rank          | organ_loop   | rank_recomputed | materialized_rank             |
      | stamp_execution_binding | track_driver | binding_stamped | execution_and_outcome_binding |
      | record_outcome_signoff  | nick_shape   | signoff         | nick_signoff                  |
      | propose_reshuffle       | orchestrator | rank_proposed   | materialized_rank             |
      | commit_reshuffle        | nick_shape   | rank_committed  | rank_position_1               |
      | reject_reshuffle        | nick_shape   | rank_rejected   | materialized_rank             |
      | veto_age_out            | nick_shape   | veto            | veto_set                      |
      | lift_age_out_veto       | nick_shape   | veto            | veto_lift                     |

  @transport
  Scenario: BacklogEvaluate stamps engine_auto and returns the batch result
    Given a backlog item created through Begin with input "shaped_over_budget"
    When a BacklogEvaluate RPC for organ "bn_0rgan00001"
    Then the backlog RPC succeeds
    And the evaluation batch names the item
    And the stored backlog history ends with "state_change" by role "engine_auto"
    And the stored backlog item shows "state_aged_out"

  @transport
  Scenario: BacklogQueue returns ranked and unranked partitions
    Given a backlog item created through Begin with input "ranked_organ"
    When a BacklogQueue RPC for organ "bn_0rgan00001"
    Then the backlog RPC succeeds

  @transport
  Scenario: A domain rejection propagates as a non-OK status
    When a BacklogMutate RPC for operation "shape_edit" with role "engine_auto"
    Then the backlog RPC returns a non-OK status
    And the backlog RPC error contains "role"

  @transport
  Scenario: One hearth's policy flag never changes another hearth's evaluation
    Given a backlog item created through Begin with input "shaped_over_budget"
    And a second backlog hearth sets age budget "99"
    When a BacklogEvaluate RPC for organ "bn_0rgan00001"
    Then the backlog RPC succeeds
    And the second-hearth backlog policy is unaffected
