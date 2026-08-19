Feature: K8 engine-auto evaluation through the shared prepared-transition seam
  evaluate_backlog is one caller-invoked, lock-held compound batch. It
  constructs the private Evaluation origin (role engine_auto) that no request
  can forge, recomputes rank/age, and derives #4/#9/#13/#14/#16 through the same
  prepare_backlog_transition. #4/#9 fire only when the incremented age strictly
  exceeds the budget; a standing veto blocks the edge while age still advances.

  Background:
    Given a backlog fixture

  @evaluate
  Scenario: A fully-shaped stale candidate ages out only when age exceeds budget
    Given a source backlog item in state "candidate" from provenance "shaped_over_budget"
    When backlog evaluation is run
    Then the backlog operation succeeds
    And the resulting backlog state is "aged_out"
    And the backlog history kinds are "created, shape_edited, rank_recomputed, state_change"

  @evaluate
  Scenario: A candidate at budget survives and is only re-ranked
    Given a source backlog item in state "candidate" from provenance "shaped_at_budget"
    When backlog evaluation is run
    Then the backlog operation succeeds
    And the resulting backlog state is "candidate"

  @evaluate
  Scenario: A standing veto blocks the age-out edge while age still increments
    Given a source backlog item in state "candidate" from provenance "shaped_over_budget"
    And a standing age-out veto is set
    When backlog evaluation is run
    Then the backlog operation succeeds
    And the resulting backlog state is "candidate"

  @evaluate
  Scenario: A ready item ages out with reason stale_no_pickup past its budget
    Given a source backlog item in state "ready" from provenance "ready_over_budget"
    When backlog evaluation is run
    Then the backlog operation succeeds
    And the resulting backlog state is "aged_out"

  @evaluate
  Scenario: A satisfied item_state wake takes #13 while a rankless source stays unranked
    Given a source backlog item in state "parked" from provenance "parked_from_2_wake_met"
    When backlog evaluation is run
    Then the backlog operation succeeds
    And the resulting backlog state is "candidate"
    And the item is reported in the unranked partition

  @evaluate
  Scenario: A satisfied rank-retaining dependency-ready wake takes #14
    Given a source backlog item in state "parked" from provenance "parked_from_7_wake_met"
    When backlog evaluation is run
    Then the backlog operation succeeds
    And the resulting backlog state is "ready"

  @evaluate
  Scenario Outline: Only the two local wake grammars can prove #16 wake-unreachable
    Given a source backlog item in state "parked" from provenance "<provenance>"
    When backlog evaluation is run
    Then the resulting backlog state is "<state>"

    Examples:
      | provenance                       | state    |
      | parked_from_7_wake_unreachable   | aged_out |
      | parked_from_11_wake_unreachable  | aged_out |
      | parked_from_7_manual_wake        | parked   |
      | parked_from_7_external_wake      | parked   |

  @evaluate
  Scenario: A pre-triage candidate never ages out until its first rank materializes
    Given a source backlog item in state "candidate" from provenance "pre_triage_over_budget"
    When backlog evaluation is run
    Then the backlog operation succeeds
    And the item is reported in the unranked partition

  @evaluate
  Scenario: A rankless #2 source rejects a direct #16 on target completeness
    Given a source backlog item in state "parked" from provenance "parked_from_2_wake_unreachable"
    When backlog evaluation is run
    Then the backlog operation is rejected because "rank"
    And no backlog residue remains under "backlog_items"

  @evaluate
  Scenario: A standing veto blocks the #16 age-out while the parked item is unchanged
    Given a source backlog item in state "parked" from provenance "parked_from_7_wake_unreachable"
    And a standing age-out veto is set
    When backlog evaluation is run
    Then the backlog operation succeeds
    And the resulting backlog state is "parked"

  # ── Policy parser (§6 Task 2/7, §9 "Policy parser fallback"): a PRESENT ──────
  # malformed ANVIL_BACKLOG_COMPARATOR/ANVIL_BACKLOG_AGE_BUDGET value is non-OK
  # and never silently defaults. Whitespace is trimmed around the whole value and
  # each comparator token; the budget parses only to a NonZeroU32.
  @evaluate
  Scenario: A valid reordered comparator and budget resolve the policy
    When the backlog policy is parsed with comparator "nick_weight_desc, value_gap_desc, age_desc, dependency_ready_first, effort_asc" and age budget "5"
    Then the backlog operation succeeds

  @evaluate
  Scenario: A present-but-empty comparator is rejected, never defaulted
    When the backlog policy is parsed with comparator "" and age budget "3"
    Then the backlog operation is rejected because "comparator"

  @evaluate
  Scenario Outline: A malformed comparator value is rejected and never silently defaults
    When the backlog policy is parsed with comparator "<comparator>" and age budget "3"
    Then the backlog operation is rejected because "comparator"

    Examples:
      | comparator                                                                                |
      | value_gap_desc,unknown_token,age_desc,nick_weight_desc,dependency_ready_first,effort_asc   |
      | value_gap_desc,value_gap_desc,age_desc,nick_weight_desc,dependency_ready_first,effort_asc  |
      | Value_Gap_Desc,nick_weight_desc,age_desc,dependency_ready_first,effort_asc                 |
      | value_gap_desc,nick_weight_desc,age_desc,dependency_ready_first                            |

  @evaluate
  Scenario Outline: A malformed age budget value is rejected and never silently defaults
    When the backlog policy is parsed with comparator "value_gap_desc,nick_weight_desc,age_desc,dependency_ready_first,effort_asc" and age budget "<budget>"
    Then the backlog operation is rejected because "budget"

    Examples:
      | budget       |
      | 0            |
      | -3           |
      | +3           |
      | 3.0          |
      | not_a_number |
      | 99999999999  |
