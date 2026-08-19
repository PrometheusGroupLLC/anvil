Feature: Widen semantic routing eligibility only for a flagged lexical miss

  The Route RPC normally consults the semantic router only after lexical routing
  finds a matching candidate. The V1 gate-breadth experiment may also consult it
  for an otherwise empty match when at least one granted playbook has content
  overlap. The experiment is default-off and does not alter ordinary matched
  routes.

  Scenario: V1 off leaves a floor miss ineligible
    Given an empty lexical match with granted overlaps "daily_recap:0:1,track:0:0"
    And V1 gate breadth is off
    When semantic route eligibility is evaluated
    Then the semantic router is not eligible
    And the route variant is "control" with brief cap "all"

  Scenario: V1 opens the semantic gate when a granted playbook overlaps
    Given an empty lexical match with granted overlaps "daily_recap:0:1,track:0:0"
    And V1 gate breadth is on
    When semantic route eligibility is evaluated
    Then the semantic router is eligible
    And the semantic briefs are exactly "daily_recap,track"
    And the route variant is "v1_gate_breadth" with brief cap "all"

  Scenario: V1 leaves an all-zero lexical miss as no-match
    Given an empty lexical match with granted overlaps "daily_recap:0:0,track:0:0"
    And V1 gate breadth is on
    When semantic route eligibility is evaluated
    Then the semantic router is not eligible
    And the route outcome remains "NoMatch"
    And the route variant is "v1_gate_breadth" with brief cap "all"

  Scenario: V1 does not change an already eligible route
    Given a lexical match with granted overlaps "daily_recap:0:2,track:0:1" and matching candidates "daily_recap"
    And V1 gate breadth is on
    When semantic route eligibility is evaluated
    Then the semantic router is eligible
    And the semantic briefs are exactly "daily_recap,track"

  Scenario: V1 cannot reopen a no-task hard abstention at the live Route RPC
    Given a route hearth seeded with consulting recap fixtures plus seed playbooks
    And the engine is started with that hearth, semantic routing on, V1 "on", and V2 cap "unset"
    When the route RPC is called with message "<system-reminder>daily recap</system-reminder>" and ctx org "Consulting" role "read" clearance "internal"
    Then the route resolution outcome is "no_match"
    And the live semantic router was not called

  Scenario: V1 cannot reopen a free-kind hard abstention at the live Route RPC
    Given a route hearth seeded with consulting recap fixtures plus seed playbooks
    And the engine is started with that hearth, semantic routing on, V1 "on", and V2 cap "unset"
    When the route RPC is called with message "create a proposal for the daily recap" and ctx org "Consulting" role "read" clearance "internal"
    Then the route resolution outcome is "no_match"
    And the live semantic router was not called
