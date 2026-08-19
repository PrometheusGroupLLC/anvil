Feature: K8 conformance round-trip and ID grammar
  A D1.3-covering item and an interleaved history/transition fixture round-trip
  byte-stably. All IDs are opaque byte strings validated only for the pinned
  Crockford grammar; playbook_run_id is never constrained and no live K1a
  lookup is performed.

  Background:
    Given a backlog fixture

  @schema
  Scenario: A fully-populated item round-trips byte-stably
    Given a source backlog item in state "in_flight" from provenance "d1_3_full"
    When the backlog item is serialized and reloaded
    Then the backlog operation succeeds
    And the backlog item round-trips byte-stably

  @schema
  Scenario: An interleaved history and transition ledger round-trips byte-stably
    Given a source backlog item in state "parked" from provenance "interleaved_history"
    When the backlog item is serialized and reloaded
    Then the backlog operation succeeds
    And the backlog item round-trips byte-stably

  @schema
  Scenario Outline: Only pinned ID grammars are accepted
    When the backlog field schema is validated against input "<input>"
    Then the backlog field rule "<rule>" holds

    Examples:
      | input                    | rule                    |
      | uppercase_bi_id          | crockford_lowercase     |
      | excluded_letter_bn_id    | crockford_excludes_iluo |
      | short_bn_body            | bn_min_body_10          |
      | short_lr_body            | lr_min_body_26          |
      | lp_prefixed_id           | pinned_prefix           |

  @schema
  Scenario: A free-form playbook_run_id is accepted without a K1a lookup
    When the backlog field schema is validated against input "opaque_playbook_run_id"
    Then the backlog operation succeeds
