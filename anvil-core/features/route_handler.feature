Feature: Route domain handler — candidate set and typed no_match

  playbook_routing_layer BP2 (core seam): the pure RouteQueryHandler returns the
  driven candidate set, or — when the registry has no driven machines — a typed
  no_match → candidate_playbook_intake outcome carrying the originating intent
  (M-3). This is the honest AC3-v0 proof: an empty (real) registry forces the
  no_match branch. The handler never errors. (At the engine RPC the composite
  always includes the compiled-in driven seeds, so no_match is unreachable there
  in v0 — see RISK R-B; this core feature is where the typed outcome is proven.)

  Scenario: an empty driven set yields the typed no_match outcome (AC3-v0)
    Given a route registry with no driven machines
    When the route handler runs with message "nothing routable here"
    Then the route result is no_match
    And the no_match handoff is "candidate_playbook_intake"
    And the no_match intent echoes "nothing routable here"

  Scenario: a non-empty driven set yields candidates carrying metadata (AC1)
    Given a route registry with a driven "knowledge_lifecycle" machine
    When the route handler runs with message "a transcript"
    Then the route result is candidates
    And the candidate set includes kind "knowledge_lifecycle"

  Scenario: free machines never appear in the candidate set (AC4)
    Given a route registry with a driven "track" machine and a free "spark" machine
    When the route handler runs with message "anything"
    Then the route result is candidates
    And the candidate set includes kind "track"
    And the candidate set excludes kind "spark"
