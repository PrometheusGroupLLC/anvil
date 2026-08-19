Feature: anvil_orchestrate — the universal surface→Anvil handoff

  One anvil-mcp tool ships a surface's message into Anvil. Two-phase
  (playbook_routing_layer R-2, LLM-selects / engine-executes): a route-mode call
  (no `selection`) returns the active driven candidate artifact_kinds for the
  surface LLM to choose; a begin-mode call (`selection` set) validates the chosen
  driven kind and begins it, serving the first per-(state,role) hook + a
  next_call. Driven against the REAL anvil-engine + anvil-mcp over a throwaway
  fixture hearth (no mocks). The AC1/AC2/AC3/AC6 hint-bearing scenarios now run
  begin-mode (the hint maps to a `selection`).

  Scenario: AC1 — the handoff returns a guided first step
    When a surface ships "Capture my call with Wendy" into anvil_orchestrate with hint "track_lifecycle"
    Then the handoff routes to playbook "track_lifecycle" at state "spec" role "doer"
    And the handoff carries a non-empty hook context_text
    And the handoff carries the step intent and expected_output
    And the handoff next_call advances via "complete"

  Scenario: AC2 — the handoff is surface-agnostic
    When two surfaces "claude-desktop" and "kiln" ship "Draft the spec for project X" into anvil_orchestrate with hint "track_lifecycle"
    Then both surfaces routed identically

  Scenario: AC3 — issuing next_call advances the instance and the step is measured
    When a surface ships "Advance the lifecycle" into anvil_orchestrate with hint "track_lifecycle" and advances the step
    Then the playbook advanced to state "spec_review" and the step was measured

  # AC5 MIGRATED (playbook_routing_layer BP3): the former "missing hint → typed
  # routing_unavailable error" is replaced by the real two-phase behavior — a
  # route-mode call (no selection) returns either a single advisory response for
  # confident matches or the candidate set for ambiguous matches (NOT an error),
  # with a next_call re-invoking anvil_orchestrate carrying a `selection`.
  # The typed no_match → candidate_playbook_intake
  # outcome (the other former routing_unavailable site) is proven at the engine
  # seam in anvil-core route_handler.feature: the engine's composite registry
  # always includes the compiled-in driven seeds, so an empty driven set is
  # unreachable at this e2e seam (RISK R-B) — we do NOT fake it here.
  # router_relevance_ranker: "knowledge lifecycle" is now confidently resolved
  # to the knowledge_lifecycle machine, so route-mode returns advisory
  # begin-equivalent guidance and a next_call to begin with that selection.
  Scenario: AC5 — a route-mode call returns a single advisory handoff for the LLM to begin
    When a surface ships "knowledge lifecycle" into anvil_orchestrate in route-mode
    Then the handoff is not an error
    And the handoff is advisory single for kind "knowledge_lifecycle"
    And the handoff next_call begins kind "knowledge_lifecycle"
    And the handoff next_call re-invokes "anvil_orchestrate" carrying a selection

  # AC5b (post free-artifact migration): register:free kinds (decision, initiative,
  # proposal, milestone, learning) are now ENGINE-routed route candidates — selecting
  # one via anvil_orchestrate is ACCEPTED (no "not a driven candidate" rejection): the
  # engine resolves the machine and begins it, asking for the kind's required creation
  # fields. Here selecting "decision" (required field: name) is accepted and the engine
  # requests `name` — proving the free-artifact kind is selectable, not rejected. (The
  # old "free kind rejected in begin-mode" invariant is obsolete for well-known kinds;
  # the rejection invariant is preserved only for genuinely unknown kinds, below.)
  Scenario: AC5b — selecting a free-artifact kind is accepted (it is a route candidate)
    When a surface ships "record this question" into anvil_orchestrate selecting "decision"
    Then the handoff missing fields are "name"

  # The rejection invariant now applies to UNREGISTERED/unknown kinds only: a kind
  # that is not in the registry at all (neither driven nor free) cannot be begun and
  # yields a typed error (UnsupportedType / unknown-kind path).
  Scenario: AC5c — selecting an unknown kind is rejected
    When a surface ships "record this question" into anvil_orchestrate selecting "glossary"
    Then the handoff is an error

  # Keystone wiring (PRESERVED through the two-phase rewrite): a
  # `knowledge_lifecycle` selection begins the generalized machine at its initial
  # state (ingesting) with the doer role — proving the surface→Anvil handoff
  # drives a non-track machine.
  Scenario: AC6 — a knowledge_lifecycle selection routes to the domain machine
    When a surface ships "Ingest the Q3 research" into anvil_orchestrate with hint "knowledge_lifecycle"
    Then the handoff routes to playbook "knowledge_lifecycle" at state "ingesting" role "doer"
    And the handoff next_call advances via "complete"
