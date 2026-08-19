Feature: Cap semantic routing briefs by full-set signal rank

  The V2 brief-breadth experiment limits the granted briefs sent to an eligible
  semantic route. When enabled with a valid positive cap, briefs rank by trigger
  tier descending, then content overlap descending, then kind ascending. V2 never
  changes whether a semantic route is eligible. Invalid caps preserve today's
  all-brief behavior and are observable in logs.

  Scenario: V2 ranks by every ordering key before applying the cap
    Given a lexical match with granted overlaps "beta:1:9,alpha:2:1,gamma:2:3,delta:2:3" and matching candidates "alpha"
    And V1 gate breadth is off
    And V2 brief breadth is on with cap "4"
    When semantic brief breadth is evaluated
    Then the semantic router is eligible
    And the semantic briefs are exactly "delta,gamma,alpha,beta"
    And the route variant is "v2_brief_breadth" with brief cap "4"

  Scenario: V2 ordering is deterministic when every score ties
    Given a lexical match with granted overlaps "zeta:2:3,alpha:2:3,middle:2:3" and matching candidates "middle"
    And V1 gate breadth is off
    And V2 brief breadth is on with cap "3"
    When semantic brief breadth is evaluated
    Then the semantic briefs are exactly "alpha,middle,zeta"
    And repeated semantic brief breadth evaluation produces the same briefs

  Scenario: V2 truncates the ranked briefs at N
    Given a lexical match with granted overlaps "alpha:3:1,beta:2:5,gamma:1:9" and matching candidates "alpha"
    And V1 gate breadth is off
    And V2 brief breadth is on with cap "2"
    When semantic brief breadth is evaluated
    Then the semantic router is eligible
    And the semantic briefs are exactly "alpha,beta"
    And the route variant is "v2_brief_breadth" with brief cap "2"

  Scenario: A V2 cap greater than the granted count sends all briefs
    Given a lexical match with granted overlaps "beta:1:1,alpha:2:1" and matching candidates "alpha"
    And V1 gate breadth is off
    And V2 brief breadth is on with cap "9"
    When semantic brief breadth is evaluated
    Then the semantic briefs are exactly "alpha,beta"
    And the route variant is "v2_brief_breadth" with brief cap "9"

  Scenario Outline: An invalid V2 cap falls back to all briefs and logs
    Given a lexical match with granted overlaps "beta:1:1,alpha:2:1" and matching candidates "alpha"
    And V1 gate breadth is off
    And V2 brief breadth is on with cap "<cap>"
    When semantic brief breadth is evaluated
    Then the semantic router is eligible
    And the semantic briefs are exactly "beta,alpha"
    And the route variant is "v2_brief_breadth" with brief cap "all"

    Examples:
      | cap                                      |
      | 0                                        |
      | -2                                       |
      | not-a-cap                                |
      | 1844674407370955161618446744073709551615 |

  Scenario Outline: An invalid V2 cap emits its structured warning at the live engine seam
    Given a route hearth seeded with consulting recap fixtures plus seed playbooks
    And the engine is started with that hearth, semantic routing on, V1 "off", and V2 cap "<cap>"
    When the route RPC is called with message "daily recap" and ctx org "Consulting" role "read" clearance "internal"
    Then the engine warning records invalid brief cap "<cap>"

    Examples:
      | cap                                      |
      | 0                                        |
      | -2                                       |
      | not-a-cap                                |
      | 1844674407370955161618446744073709551615 |

  Scenario: V2 off preserves today's all-brief ordering
    Given a lexical match with granted overlaps "zeta:0:1,alpha:0:4,beta:0:2" and matching candidates "beta"
    And V1 gate breadth is off
    And V2 brief breadth is off
    When semantic brief breadth is evaluated
    Then the semantic router is eligible
    And the semantic briefs are exactly "zeta,alpha,beta"
    And the route variant is "control" with brief cap "all"

  Scenario: V2 does not make an ineligible lexical miss eligible
    Given an empty lexical match with granted overlaps "daily_recap:0:2,track:0:1"
    And V1 gate breadth is off
    And V2 brief breadth is on with cap "1"
    When semantic brief breadth is evaluated
    Then the semantic router is not eligible
    And the semantic briefs are empty
    And the route outcome remains "NoMatch"
    And the route variant is "v2_brief_breadth" with brief cap "1"

  Scenario: V1 opens the gate before V2 caps the newly eligible call
    Given an empty lexical match with granted overlaps "daily_recap:1:2,track:2:1,proposal:0:4"
    And V1 gate breadth is on
    And V2 brief breadth is on with cap "2"
    When semantic brief breadth is evaluated
    Then the semantic router is eligible
    And the semantic briefs are exactly "track,daily_recap"
    And the route outcome remains "NoMatch"
    And the route variant is "v1_gate_breadth+v2_brief_breadth" with brief cap "2"

  Scenario: V2 rejects a semantic pick outside the capped brief authorization set
    Given a lexical match with granted overlaps "alpha:3:3,beta:2:2,gamma:1:1" and matching candidates "alpha"
    And V1 gate breadth is off
    And V2 brief breadth is on with cap "2"
    When semantic brief breadth is evaluated
    And the semantic verdict picks "gamma" through the planned briefs
    Then the semantic resolution outcome is "NoMatch"
    And the semantic selected kind is unset
