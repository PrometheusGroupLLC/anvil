Feature: K8 legal-transition table (17 rows, exact roles, guards)
  The seed and validate_transition admit exactly the (from, to, driver_role)
  tuples printed in D2.1 — the 16 non-genesis logical rows expanded only by
  their printed roles (29 tuples). Every off-table pair, wrong role, terminal
  outgoing attempt, and unmet guard is non-OK with byte-identical residue.

  Background:
    Given a backlog fixture

  @snapshot
  Scenario Outline: Every printed human/loop tuple lands through governed Snapshot
    Given a source backlog item in state "<from>" from provenance "row_<row>"
    When a backlog transition from "<from>" to "<to>" is attempted with role "<role>"
    Then the backlog operation succeeds
    And the resulting backlog state is "<to>"

    Examples:
      | row | from       | to         | role         |
      | 1   | candidate  | ready      | organ_loop   |
      | 1   | candidate  | ready      | orchestrator |
      | 2   | candidate  | parked     | nick_shape   |
      | 2   | candidate  | parked     | organ_loop   |
      | 3   | candidate  | superseded | nick_shape   |
      | 3   | candidate  | superseded | orchestrator |
      | 5   | ready      | in_flight  | track_driver |
      | 6   | ready      | candidate  | organ_loop   |
      | 6   | ready      | candidate  | orchestrator |
      | 6   | ready      | candidate  | nick_shape   |
      | 7   | ready      | parked     | nick_shape   |
      | 7   | ready      | parked     | organ_loop   |
      | 8   | ready      | superseded | nick_shape   |
      | 8   | ready      | superseded | orchestrator |
      | 10  | in_flight  | done       | nick_shape   |
      | 11  | in_flight  | parked     | track_driver |
      | 11  | in_flight  | parked     | nick_shape   |
      | 12  | in_flight  | superseded | nick_shape   |
      | 12  | in_flight  | superseded | orchestrator |
      | 13  | parked     | candidate  | nick_shape   |
      | 14  | parked     | ready      | nick_shape   |
      | 15  | parked     | superseded | nick_shape   |
      | 15  | parked     | superseded | orchestrator |

  @evaluate
  Scenario Outline: Every printed engine-auto tuple lands only through evaluation origin
    Given a source backlog item in state "<from>" from provenance "row_<row>"
    When a backlog transition from "<from>" to "<to>" is attempted with role "engine_auto"
    Then the backlog operation succeeds
    And the resulting backlog state is "<to>"

    Examples:
      | row | from       | to         |
      | 4   | candidate  | aged_out   |
      | 9   | ready      | aged_out   |
      | 10  | in_flight  | done       |
      | 13  | parked     | candidate  |
      | 14  | parked     | ready      |
      | 16  | parked     | aged_out   |

  @snapshot
  Scenario Outline: A wrong role for a legal pair is refused with byte-identical residue
    Given a source backlog item in state "<from>" from provenance "row_<row>"
    When a backlog transition from "<from>" to "<to>" is attempted with role "<role>"
    Then the backlog operation is rejected because "role"
    And no backlog residue remains under "backlog_items"

    Examples:
      | row | from       | to         | role         |
      | 1   | candidate  | ready      | track_driver |
      | 1   | candidate  | ready      | nick_shape   |
      | 4   | candidate  | aged_out   | nick_shape   |
      | 5   | ready      | in_flight  | orchestrator |
      | 9   | ready      | aged_out   | orchestrator |
      | 10  | in_flight  | done       | track_driver |
      | 16  | parked     | aged_out   | nick_shape   |

  @snapshot
  Scenario: A caller-forged engine_auto role on Snapshot is refused for evaluation-only rows
    Given a source backlog item in state "candidate" from provenance "row_4"
    When a raw Snapshot backlog transition from "candidate" to "aged_out" is attempted with role "engine_auto"
    Then the backlog operation is rejected because "engine_auto"
    And no backlog residue remains under "backlog_items"

  @snapshot
  Scenario Outline: Off-table state pairs are refused
    Given a source backlog item in state "<from>" from provenance "off_table"
    When a backlog transition from "<from>" to "<to>" is attempted with role "nick_shape"
    Then the backlog operation is rejected because "no such transition"
    And no backlog residue remains under "backlog_items"

    Examples:
      | from       | to         |
      | candidate  | in_flight  |
      | ready      | done       |
      | in_flight  | ready      |
      | in_flight  | candidate  |
      | in_flight  | aged_out   |
      | parked     | in_flight  |
      | candidate  | done       |

  @snapshot
  Scenario Outline: A terminal state has no outgoing transition
    Given a source backlog item in state "<from>" from provenance "terminal"
    When a backlog transition from "<from>" to "candidate" is attempted with role "nick_shape"
    Then the backlog operation is rejected because "terminal"
    And no backlog residue remains under "backlog_items"

    Examples:
      | from       |
      | done       |
      | superseded |
      | aged_out   |

  @snapshot
  Scenario Outline: A legal pair with an unmet guard is refused
    Given a source backlog item in state "<from>" from provenance "<provenance>"
    When a backlog transition from "<from>" to "<to>" is attempted with role "<role>"
    Then the backlog operation is rejected because "<guard>"
    And no backlog residue remains under "backlog_items"

    Examples:
      | from       | to         | role         | provenance          | guard             |
      | candidate  | ready      | organ_loop   | unranked            | rank              |
      | ready      | in_flight  | track_driver | no_attend_approval  | approval          |
      | ready      | in_flight  | track_driver | stale_binding_stamp | binding_stamp     |
      | candidate  | superseded | nick_shape   | no_superseded_by    | superseded_by     |
      | candidate  | parked     | nick_shape   | no_wake_condition   | wake              |
      | in_flight  | parked     | track_driver | missing_binding     | binding           |
      | in_flight  | superseded | nick_shape   | missing_binding     | binding           |

  @mutations
  Scenario: An approved top-ready pickup with a fresh unconsumed stamp lands #5
    Given a source backlog item in state "ready" from provenance "approved_top_ready_stamped"
    When a backlog transition from "ready" to "in_flight" is attempted with role "track_driver"
    Then the backlog operation succeeds
    And the resulting backlog state is "in_flight"

  @mutations
  Scenario Outline: An #11-retained binding is provenance-only and cannot authorize a new #5 without a fresh stamp
    Given a source backlog item in state "ready" from provenance "<provenance>"
    When a backlog transition from "ready" to "in_flight" is attempted with role "track_driver"
    Then the backlog operation is rejected because "binding_stamp"
    And no backlog residue remains under "backlog_items"

    Examples:
      | provenance                                |
      | parked_from_11_woken_14_no_restamp        |
      | parked_from_11_woken_13_then_1_no_restamp |

  @mutations
  Scenario Outline: After a fresh stamp on a re-entered item, #5 lands and consumes the new binding_stamp_seq
    Given a source backlog item in state "ready" from provenance "<provenance>"
    When a backlog transition from "ready" to "in_flight" is attempted with role "track_driver"
    Then the backlog operation succeeds
    And the resulting backlog state is "in_flight"

    Examples:
      | provenance                                |
      | restamped_after_consumed                  |
      | parked_from_11_woken_14_restamped         |
      | parked_from_11_woken_13_then_1_restamped  |

  @mutations
  Scenario: #11 preserves both pickup bindings byte-for-byte into parked
    Given a source backlog item in state "in_flight" from provenance "bound_with_wake_intent"
    When a backlog transition from "in_flight" to "parked" is attempted with role "track_driver"
    Then the backlog operation succeeds
    And the resulting backlog state is "parked"
    And the carried pickup bindings are byte-preserved

  @mutations
  Scenario: #12 carries both pickup bindings byte-for-byte into superseded
    Given a source backlog item in state "in_flight" from provenance "bound_with_supersede_intent"
    When a backlog transition from "in_flight" to "superseded" is attempted with role "nick_shape"
    Then the backlog operation succeeds
    And the resulting backlog state is "superseded"
    And the carried pickup bindings are byte-preserved

  @mutations
  Scenario: The #7-origin #15 lands with no pickup bindings
    Given a source backlog item in state "parked" from provenance "parked_from_7_ranked"
    When a backlog transition from "parked" to "superseded" is attempted with role "nick_shape"
    Then the backlog operation succeeds
    And the resulting backlog state is "superseded"

  @mutations
  Scenario: The #11-origin #15 lands and keeps its pickup bindings byte-equal
    Given a source backlog item in state "parked" from provenance "parked_from_11_ranked"
    When a backlog transition from "parked" to "superseded" is attempted with role "orchestrator"
    Then the backlog operation succeeds
    And the resulting backlog state is "superseded"
    And the carried pickup bindings are byte-preserved

  @mutations
  Scenario: A rankless #2 source rejects a direct #15 only at complete-target validation
    Given a source backlog item in state "parked" from provenance "parked_from_2_rankless"
    When a backlog transition from "parked" to "superseded" is attempted with role "nick_shape"
    Then the backlog operation is rejected because "rank"
    And no backlog residue remains under "backlog_items"
