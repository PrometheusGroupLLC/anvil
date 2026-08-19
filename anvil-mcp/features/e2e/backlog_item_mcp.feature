Feature: K8 backlog_item MCP end-to-end journey
  Claude Code drives the K8 lifecycle through the MCP shim: begin with a closed
  item object, describe, the twelve backlog_* tools mapping to three RPCs, and
  the queue tools. Tools carry no caller-controlled sequence or audit stamp;
  engine unavailability, wrong wire, invalid payload, and domain rejection stay
  loud. This exercises a real shim over a real engine (Task 9).

  Background:
    Given a backlog MCP session

  @transport
  Scenario: The tool list advertises the twelve backlog tools
    When the tool list is requested
    Then the tool list advertises "backlog_shape_edit"
    And the tool list advertises "backlog_recompute_rank"
    And the tool list advertises "backlog_stamp_execution_binding"
    And the tool list advertises "backlog_record_outcome_signoff"
    And the tool list advertises "backlog_propose_reshuffle"
    And the tool list advertises "backlog_commit_reshuffle"
    And the tool list advertises "backlog_reject_reshuffle"
    And the tool list advertises "backlog_veto_age_out"
    And the tool list advertises "backlog_lift_age_out_veto"
    And the tool list advertises "backlog_evaluate"
    And the tool list advertises "backlog_organ_queue"
    And the tool list advertises "backlog_cross_organ_view"

  @transport
  Scenario: The full pickup-to-done journey lands through the shim
    When a begin tool call for a backlog item with input "valid_candidate"
    Then the tool result state is "candidate"
    When a describe tool call for "backlog_item"
    Then the tool result contains "item"
    When a "backlog_shape_edit" tool call with role "organ_loop"
    Then the tool call returns a successful result
    When a "backlog_recompute_rank" tool call with role "organ_loop"
    Then the tool call returns a successful result
    When a Snapshot tool call from "candidate" to "ready" with role "organ_loop"
    Then the tool result state is "ready"
    When a "backlog_stamp_execution_binding" tool call with role "track_driver"
    Then the tool call returns a successful result
    When a Snapshot tool call from "ready" to "in_flight" with role "track_driver"
    Then the tool result state is "in_flight"
    When a "backlog_record_outcome_signoff" tool call with role "nick_shape"
    Then the tool call returns a successful result
    When a Snapshot tool call from "in_flight" to "done" with role "nick_shape"
    Then the tool result state is "done"

  @transport
  Scenario: The organ queue surfaces ranked output and an explicit unranked pre-triage item
    When a backlog_organ_queue tool call for "bn_0rgan00001"
    Then the tool call returns a successful result
    And the tool result contains "route_to_intake"
    And the tool result contains "rank_not_materialized"

  @transport
  Scenario: The cross-organ view aggregates ranked and unranked partitions across organs
    When a backlog_cross_organ_view tool call
    Then the tool call returns a successful result
    And the tool result contains "route_to_intake"

  @transport
  Scenario: The begin K8 branch rejects a pre-stringified item and forged keys
    When a begin tool call for a backlog item with input "prestringified_item"
    Then the tool call returns a JSON-RPC error
    And the tool call error contains "item"

  @transport
  Scenario Outline: The begin K8 branch admits exactly the three origin shapes
    When a begin tool call for a backlog item with input "<input>"
    Then the tool result state is "candidate"

    Examples:
      | input                   |
      | no_predictor            |
      | council_with_prediction |
      | experiment_with_prediction |

  @transport
  Scenario Outline: The begin K8 branch refuses a forged key or a predictor/value mismatch
    When a begin tool call for a backlog item with input "<input>"
    Then the tool call returns a JSON-RPC error
    And the tool call error contains "<reason>"

    Examples:
      | input                     | reason         |
      | forged_engine_owned_key   | engine owns    |
      | both_predictor_ids        | never both     |
      | predictor_without_value   | predicted_value |
      | value_without_predictor   | predictor      |
      | sibling_field             | sibling        |

  @transport
  Scenario Outline: A domain rejection surfaces as a loud JSON-RPC error
    When a "<tool>" tool call with role "<role>"
    Then the tool call returns a JSON-RPC error
    And the tool call error contains "<reason>"

    Examples:
      | tool                    | role         | reason |
      | backlog_shape_edit      | engine_auto  | role   |
      | backlog_stamp_execution_binding | nick_shape | role |
      | backlog_recompute_rank  | orchestrator | role   |
