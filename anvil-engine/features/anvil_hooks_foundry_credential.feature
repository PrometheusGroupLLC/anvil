Feature: anvil-hooks presents a Foundry credential on the direct-engine channel
  The hooks CLI dials the engine directly, bypassing the anvil MCP server. That
  bypass was long documented as "broker-independent — works when the broker is
  down". Against a Foundry-mode engine it never was; it only LOOKED true while
  the shipped engine had its session verifier compiled out, so an uncredentialed
  call and an authorized one were indistinguishable.

  With the verifier restored, a Foundry-mode engine gates every RPC this CLI
  makes, and the CLI had no credential path at all — so `begin`, `snapshot`,
  `complete` and `amend` all answered `not_authenticated` for any caller outside
  the supervisor's process env. The fix is for the CLI to obtain a real ticket,
  NOT for the engine to relax.

  These scenarios pin BOTH halves. The success path must not be satisfiable by
  an engine that never gated: each one reads back the PERSISTED transition
  actor, which is `foundry:<sub>` only when the engine actually verified a
  bearer and bound the principal to it. A standalone engine records the
  caller-supplied `--actor-name` verbatim instead, so the two are
  distinguishable by construction. The refusal path is the control: the same
  binary against the same engine with the credential suppressed must still be
  refused, or the hole is back.

  Driven against the REAL spawned engine binary and the REAL anvil-hooks binary
  with a hermetic accepting verifier (ANVIL_TEST_SESSION_VERIFIER=stub_accept
  bound to a known sub), so no live broker is required.

  Scenario: begin carries the inherited session token and the engine binds the principal
    Given the engine is started in Foundry mode with an accepting verifier for sub "user-abc"
    When anvil-hooks begins a track under parent "20260411T2021_anvil_workflow_engine" with session token "valid.jwt.token" and actor name "Imposter-000000"
    Then the anvil-hooks command succeeds
    And the anvil-hooks created track transition actor is "foundry:user-abc"
    And the anvil-hooks created track transition actor is not "Imposter-000000"

  Scenario: begin with the credential suppressed is still refused by the same engine
    Given the engine is started in Foundry mode with an accepting verifier for sub "user-abc"
    When anvil-hooks begins a track under parent "20260411T2021_anvil_workflow_engine" with no Foundry credential and actor name "Imposter-000000"
    Then the anvil-hooks command fails
    And the anvil-hooks output contains "not_authenticated"
    And anvil-hooks created no track

  # The refusal alone would not test the CLI here: the engine ALSO rejects a
  # blank bearer, so "it was refused" is satisfied whether or not the CLI
  # treated the blank value as a credential. The provenance line is what
  # discriminates — the CLI must report having presented NO bearer, not one
  # inherited from the environment.
  Scenario: a blank session token is not a credential
    Given the engine is started in Foundry mode with an accepting verifier for sub "user-abc"
    When anvil-hooks begins a track under parent "20260411T2021_anvil_workflow_engine" with session token "   " and actor name "Imposter-000000"
    Then the anvil-hooks command fails
    And the anvil-hooks output contains "not_authenticated"
    And the anvil-hooks output contains "no bearer presented"
    And anvil-hooks created no track

  Scenario: the refusal names the credential that was missing
    Given the engine is started in Foundry mode with an accepting verifier for sub "user-abc"
    When anvil-hooks begins a track under parent "20260411T2021_anvil_workflow_engine" with no Foundry credential and actor name "Imposter-000000"
    Then the anvil-hooks command fails
    And the anvil-hooks output contains "FOUNDRY_SESSION_TOKEN"

  # THIS SCENARIO EXISTS TO CATCH A DIVERGENCE IN THE HARNESS, NOT IN THE CLI.
  #
  # `ensure_binary` built these binaries with `cargo build --bin <name>` — no
  # features — while `scripts/build-kit.sh` ships them with
  # `--features anvil-engine/foundry-session`. So every scenario above ran
  # against a binary in which precedence 2 (mint from the broker) is compiled
  # OUT and replaced by a refusal. The suite was testing a binary unlike the one
  # that ships: the same class of defect as the original bug, one layer down.
  #
  # It went unnoticed because a features-off build refuses too, and "it was
  # refused" is satisfied by EITHER cause — no credential, or no broker client
  # compiled in. This assertion is the discriminator, because only a build with
  # the broker client can name the socket the operator has to set:
  #
  #   features ON,  no socket -> "...broker socket missing or unreachable:
  #                               FOUNDRY_BROKER_SOCKET env var not set"   GREEN
  #   features OFF            -> "...this build has no broker client
  #                               compiled in"                             RED
  #
  # It is also a real user-facing property, not merely a build assertion: a
  # refusal that does not name the variable to set leaves the operator with
  # nothing to do about it.
  Scenario: the refusal names the broker socket the operator must set
    Given the engine is started in Foundry mode with an accepting verifier for sub "user-abc"
    When anvil-hooks begins a track under parent "20260411T2021_anvil_workflow_engine" with no Foundry credential and actor name "Imposter-000000"
    Then the anvil-hooks command fails
    And the anvil-hooks output contains "FOUNDRY_BROKER_SOCKET"
    And anvil-hooks created no track

  Scenario: complete carries the inherited session token and the engine binds the principal
    Given the engine is started in Foundry mode with an accepting verifier for sub "user-abc"
    When anvil-hooks completes artifact "tracks/20260417T1000_principal_track" with session token "valid.jwt.token" and actor name "Imposter-000000"
    Then the anvil-hooks command succeeds
    And the persisted transition actor in "tracks/20260417T1000_principal_track" is "foundry:user-abc"
    And the persisted transition actor in "tracks/20260417T1000_principal_track" is not "Imposter-000000"

  Scenario: complete with the credential suppressed is still refused by the same engine
    Given the engine is started in Foundry mode with an accepting verifier for sub "user-abc"
    When anvil-hooks completes artifact "tracks/20260417T1000_principal_track" with no Foundry credential and actor name "Imposter-000000"
    Then the anvil-hooks command fails
    And the anvil-hooks output contains "not_authenticated"

  Scenario: against a standalone engine the CLI still needs no credential
    Given the engine is started in standalone mode over a principal-binding hearth
    When anvil-hooks completes artifact "tracks/20260417T1000_principal_track" with no Foundry credential and actor name "Caller-555555"
    Then the anvil-hooks command succeeds
    And the persisted transition actor in "tracks/20260417T1000_principal_track" is "Caller-555555"
