Feature: Route RPC carries begin-equivalent guidance and per-candidate annotations
  route_response_mirrors_begin Phase 2/3: a SINGLE route returns the begin-equivalent
  guidance (the same resolved + budget-capped, PRE-interpolation hook body begin reads
  for the selected kind's initial (state, doer)); a CANDIDATES route annotates each
  matching candidate with intent (the (initial_state, doer) MeasurementSpec.intent),
  step_outline (the machine's state names in order), and why_fits (the concrete match
  signal). A granted-but-not-matching playbook is not surfaced as a matched candidate
  (no why_fits). Routing selection is unchanged.

  Scenario: a single route returns guidance equal to begin's served hook body
    Given a route hearth seeded with consulting recap fixtures plus seed playbooks
    And the engine is started with that hearth
    When the route RPC is called with message "daily recap" and ctx org "Consulting" role "read" clearance "internal"
    Then the route resolution outcome is "single"
    And the route selected kind is "daily_recap"
    And the route guidance equals the playbook hook body "gathering.md" for "daily_recap" in the route hearth
    And the route guidance contains "Daily Recap Gathering"

  Scenario: a single route annotates the selected candidate with intent, step_outline, and why_fits
    Given a route hearth seeded with consulting recap fixtures plus seed playbooks
    And the engine is started with that hearth
    When the route RPC is called with message "daily recap" and ctx org "Consulting" role "read" clearance "internal"
    Then the route resolution outcome is "single"
    And the route candidate "daily_recap" intent contains "deduplicated"
    And the route candidate "daily_recap" step_outline starts with "gathering" and has at least 1 step
    And the route candidate "daily_recap" why_fits contains "daily recap"

  Scenario: a candidates route annotates each matching candidate and cites its trigger
    Given a route hearth seeded with consulting recap fixtures plus seed playbooks
    And the engine is started with that hearth
    When the route RPC is called with message "recap" and ctx org "Consulting" role "read" clearance "internal"
    Then the route resolution outcome is "candidates"
    And the route matching candidates are exactly "daily_recap,weekly_recap"
    And the route candidate "daily_recap" route_triggers contain "daily recap"
    And the route candidate "daily_recap" why_fits contains "recap"
    And the route candidate "daily_recap" step_outline starts with "gathering" and has at least 1 step
    And the route candidate "weekly_recap" why_fits contains "recap"
    And the route guidance is empty

  # M4: the legacy `candidates` field keeps the granted set (existing contract),
  # but a granted-but-not-matching playbook (e.g. "track") is NOT a MATCHED
  # candidate: it is absent from `matching_candidates` AND carries no why_fits /
  # intent / step_outline annotations. Only the matching candidate is annotated.
  Scenario: a granted-but-not-matching playbook is not surfaced as a matched candidate
    Given a route hearth seeded with consulting recap fixtures plus seed playbooks
    And the engine is started with that hearth
    When the route RPC is called with message "daily recap" and ctx org "Consulting" role "read" clearance "internal"
    Then the route matching candidates are exactly "daily_recap"
    And the route candidates include kind "track"
    And the route candidate "track" has no why_fits
    And the route candidate "daily_recap" why_fits contains "daily recap"

  # H2: a candidate set whose full annotations exceed ROUTE_RESPONSE_BUDGET_BYTES
  # (8192) is truncated deterministically — step_outline dropped first, then
  # collapsed to kind + description — so the serialized total stays within budget.
  Scenario: an over-budget candidate set is truncated to stay within the response budget
    Given a route hearth seeded with two oversized-annotation driven machines sharing trigger "do the big thing"
    And the engine is started with that hearth
    When the route RPC is called with message "do the big thing" and ctx org "Consulting" role "read" clearance "internal"
    Then the route resolution outcome is "candidates"
    And the route matching candidates are exactly "budget_alpha,budget_beta"
    And every route candidate has its step_outline dropped
    And the route candidate set total is within 8192 bytes

  # H3: enrichment fail-open at the engine source. A single-resolution machine
  # whose declared hook file is missing makes the guidance read error; the Route
  # RPC must still return a valid THIN response (single resolution, candidate
  # surfaced, empty guidance) — never a gRPC error.
  Scenario: a single route with a failing guidance enrichment degrades to a thin response
    Given a route hearth with one driven machine kind "brittle" trigger "do the brittle thing" declaring a missing hook
    And the engine is started with that hearth
    When the route RPC is called with message "do the brittle thing"
    Then the route resolution outcome is "single"
    And the route selected kind is "brittle"
    And the route candidates include kind "brittle"
    And the route guidance is empty

  # H3 (genuine-error-still-surfaces): the enrichment fail-open is SCOPED to the
  # guidance read only. A genuine engine error that occurs BEFORE enrichment — the
  # Foundry-mode auth gate refusing an unauthenticated call — still propagates as a
  # gRPC error rather than being swallowed into a thin route.
  Scenario: a genuine engine error still surfaces as a gRPC error
    Given the engine is started in Foundry mode with a rejecting verifier
    When the route RPC is called with message "do the brittle thing"
    Then the route RPC returns a gRPC error containing "not_authenticated"
