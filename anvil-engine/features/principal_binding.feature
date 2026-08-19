Feature: Principal binds to sub; self-asserted name never persisted under Foundry (spec Req 7)
  Under Foundry the engine derives the persisted principal from the verified
  session's `sub`, NOT from the caller-supplied `actor_name`. A caller may send
  any `actor_name` it likes; the engine overrides it with the sub-derived
  principal before the domain command persists anything. This closes the
  principal-laundering gap: a caller cannot write a self-asserted identity into
  the permanent event log under Foundry.

  All three actor-persisting RPCs — begin, snapshot, complete — must enforce the
  binding (Req 7 applies to every surface that writes the transition actor).

  The accept path is driven against the REAL spawned engine binary with a
  hermetic ACCEPTING verifier (ANVIL_TEST_SESSION_VERIFIER=stub_accept bound to
  a known sub), so no live broker is required. Each scenario reads back the
  PERSISTED status.yaml transition actor — the event log — not just the RPC
  response.

  Scenario: begin binds the persisted principal to sub, dropping the divergent caller name
    Given the engine is started in Foundry mode with an accepting verifier for sub "user-abc"
    When the begin RPC is called with bearer token "valid.jwt.token" and actor_name "Imposter-000000" to create track "laundered" under parent "20260411T2021_anvil_workflow_engine"
    Then the begin RPC response state is "spec"
    And the begin RPC response track_path status.yaml transition actor is "foundry:user-abc"
    And the begin RPC response track_path status.yaml transition actor is not "Imposter-000000"

  Scenario: snapshot binds the persisted transition actor to sub, dropping the divergent caller name
    Given the engine is started in Foundry mode with an accepting verifier for sub "user-abc"
    When the snapshot RPC is called with bearer token "valid.jwt.token" and actor_name "Imposter-000000" on artifact "tracks/20260417T1000_principal_track" to state "plan"
    Then the snapshot RPC response success is "true"
    And the persisted transition actor in "tracks/20260417T1000_principal_track" is "foundry:user-abc"
    And the persisted transition actor in "tracks/20260417T1000_principal_track" is not "Imposter-000000"

  Scenario: complete binds the persisted transition actor to sub, dropping the divergent caller name
    Given the engine is started in Foundry mode with an accepting verifier for sub "user-abc"
    When the complete RPC is called with bearer token "valid.jwt.token" and actor_name "Imposter-000000" on artifact "tracks/20260417T1000_principal_track" with satisfaction "satisfied"
    Then the complete RPC response new_state is "plan"
    And the persisted transition actor in "tracks/20260417T1000_principal_track" is "foundry:user-abc"
    And the persisted transition actor in "tracks/20260417T1000_principal_track" is not "Imposter-000000"

  Scenario: standalone persists the caller-supplied actor_name verbatim (Req 3 no-regression)
    Given the engine is started in standalone mode over a principal-binding hearth
    When the snapshot RPC is called with bearer token "" and actor_name "Caller-555555" on artifact "tracks/20260417T1000_principal_track" to state "plan"
    Then the snapshot RPC response success is "true"
    And the persisted transition actor in "tracks/20260417T1000_principal_track" is "Caller-555555"
