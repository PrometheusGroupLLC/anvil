Feature: Route resolution exposes ranking signals for every granted workflow
  The router reports the lexical ranking inputs for every granted workflow so
  later evaluation can inspect kinds excluded from the live shortlist. This
  observability is additive: trigger-tier filtering, the three-candidate cap,
  selection, and route outcome remain unchanged.

  Background:
    Given an empty full-signal route registry
    And a full-signal request context

  Scenario: a workflow eliminated by the winning trigger tier remains observable
    Given a full-signal workflow "long_trigger" described as "alpha delivery" with triggers "alpha delivery"
    And a full-signal workflow "short_trigger" described as "alpha notes" with triggers "alpha"
    When the full-signal route resolver runs with input "alpha delivery"
    Then the full granted signals are "long_trigger:14:2,short_trigger:5:1"
    And the matching route candidates are exactly "long_trigger"

  Scenario: the full signals are not limited by the matching candidate cap
    Given a full-signal workflow "alpha" described as "shared routing terms" with no triggers
    And a full-signal workflow "bravo" described as "shared routing terms" with no triggers
    And a full-signal workflow "charlie" described as "shared routing terms" with no triggers
    And a full-signal workflow "delta" described as "shared routing terms" with no triggers
    And a full-signal workflow "echo" described as "shared routing terms" with no triggers
    When the full-signal route resolver runs with input "shared routing terms"
    Then the full granted signal count is 5
    And the matching route candidate count is at most 3

  Scenario: zero-overlap workflows remain observable with a zero score
    Given a full-signal workflow "matching" described as "shared routing terms" with no triggers
    And a full-signal workflow "unrelated" described as "copper lantern archive" with no triggers
    When the full-signal route resolver runs with input "shared routing terms"
    Then the full granted signals include "unrelated" with trigger tier 0 and content overlap 0

  Scenario: full signals have deterministic ranking order
    Given a full-signal workflow "tier_two" described as "focus extra" with triggers "focus extra"
    And a full-signal workflow "tier_one_high" described as "focus extra detail" with triggers "focus"
    And a full-signal workflow "zeta" described as "focus detail" with triggers "focus"
    And a full-signal workflow "alpha" described as "focus detail" with triggers "focus"
    And a full-signal workflow "no_trigger" described as "focus extra detail" with no triggers
    When the full-signal route resolver runs with input "focus extra detail"
    Then the full granted signal kinds are ordered "tier_two,tier_one_high,alpha,zeta,no_trigger"

  Scenario: adding full signals does not change the route decision
    Given a full-signal workflow "long_trigger" described as "alpha delivery" with triggers "alpha delivery"
    And a full-signal workflow "short_trigger" described as "alpha notes" with triggers "alpha"
    And a full-signal workflow "unrelated" described as "copper lantern archive" with no triggers
    When the full-signal route resolver runs with input "alpha delivery"
    Then the matching route candidates are exactly "long_trigger"
    And the selected route kind is "long_trigger"
    And the route resolution outcome is "Single"
