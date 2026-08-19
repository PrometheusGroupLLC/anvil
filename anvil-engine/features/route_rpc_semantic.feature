Feature: Route RPC optionally serves the semantic (Kiln) verdict, fail-open to lexical
  The engine Route RPC can serve the SEMANTIC router verdict instead of the pure
  lexical resolve_route result, gated behind the dark-launch flag
  ANVIL_SEMANTIC_ROUTE_RPC (default OFF). The semantic path feeds Kiln the GRANTED
  candidate set (the candidate_recall lever — wider than the lexical top-3 matching
  set, so the router can pick a kind the floor ranked low) and ALWAYS fails open: on a Kiln miss
  (unreachable / timeout / parse-gap) the RPC keeps the lexical resolution
  unchanged (the deliberate divergence from the hook, which goes silent). The
  verdict→resolution mapping (apply_semantic_verdict) is a pure, directly-testable
  function.

  # ── Flag OFF → lexical parity (regression guard) ──
  # With the dark flag unset (default OFF) the RPC returns exactly the lexical
  # resolution — byte-for-byte the route_rpc_resolution contract.
  Scenario: the dark flag defaults OFF and the RPC serves the lexical resolution
    Given a route hearth seeded with consulting recap fixtures plus seed playbooks
    And the engine is started with that hearth
    When the route RPC is called with message "daily recap" and ctx org "Consulting" role "read" clearance "internal"
    Then the route outcome is "candidates"
    And the route resolution outcome is "single"
    And the route selected kind is "daily_recap"
    And the route matching candidates are exactly "daily_recap"

  # ── Flag ON + Kiln unreachable → fail-open to lexical ──
  # With the dark flag ON but the Kiln gateway pointed at a dead port, the single
  # Kiln call transport-errors → RouterVerdict::Fallback → apply_semantic_verdict
  # keeps the lexical resolution UNCHANGED. The turn never degrades to no_match on
  # a Kiln blip.
  Scenario: flag ON with Kiln unreachable fails open to the lexical resolution
    Given a route hearth seeded with consulting recap fixtures plus seed playbooks
    And the engine is started with that hearth and the semantic route RPC flag on but Kiln unreachable
    When the route RPC is called with message "daily recap" and ctx org "Consulting" role "read" clearance "internal"
    Then the route outcome is "candidates"
    And the route resolution outcome is "single"
    And the route selected kind is "daily_recap"
    And the route matching candidates are exactly "daily_recap"

  # ── Pure verdict → resolution mapping (apply_semantic_verdict) ──
  # Directly exercise the pure fold over a lexical resolution whose matching set is
  # "daily_recap,weekly_recap".
  Scenario: a Pick naming a matching candidate collapses to Single on that kind
    Given a lexical route resolution with matching candidates "daily_recap,weekly_recap"
    When the semantic verdict is applied as pick "daily_recap"
    Then the semantic resolution outcome is "Single"
    And the semantic selected kind is "daily_recap"
    And the semantic matching candidates are exactly "daily_recap"

  # ── candidate_recall widen lever: the fed set is GRANTED, not just matching ──
  # granted ⊃ matching: the lexical floor only matched "daily_recap", but the
  # semantic router is fed the full granted set, so a Pick of a granted-but-not-
  # matching kind ("track") is honored → Single on track. Before the widen lever
  # (validate against matching) this was no_match; now it recovers the route.
  Scenario: a Pick of a granted-but-not-matching candidate collapses to Single (widen lever)
    Given a lexical route resolution with granted candidates "daily_recap,weekly_recap,track" and matching candidates "daily_recap"
    When the semantic verdict is applied as pick "track"
    Then the semantic resolution outcome is "Single"
    And the semantic selected kind is "track"
    And the semantic matching candidates are exactly "track"

  Scenario: a Pick naming a non-candidate maps to no_match (never invents a kind)
    Given a lexical route resolution with matching candidates "daily_recap,weekly_recap"
    When the semantic verdict is applied as pick "monthly_recap"
    Then the semantic resolution outcome is "NoMatch"
    And the semantic selected kind is unset
    And the semantic matching candidates are exactly ""

  Scenario: an Abstain verdict maps to no_match (the precision win)
    Given a lexical route resolution with matching candidates "daily_recap,weekly_recap"
    When the semantic verdict is applied as abstain
    Then the semantic resolution outcome is "NoMatch"
    And the semantic selected kind is unset
    And the semantic matching candidates are exactly ""

  Scenario: a Fallback verdict keeps the lexical resolution unchanged (fail-open)
    Given a lexical route resolution with matching candidates "daily_recap,weekly_recap"
    When the semantic verdict is applied as fallback
    Then the semantic resolution outcome is "Candidates"
    And the semantic selected kind is unset
    And the semantic matching candidates are exactly "daily_recap,weekly_recap"
