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

  # THIS ASSERTION DISCRIMINATES A MINTER THAT LOOKED FROM ONE THAT DID NOT.
  #
  # Its ORIGINAL job was to catch a harness divergence: `ensure_binary` built
  # with no features while `scripts/build-kit.sh` shipped
  # `--features anvil-engine/foundry-session`, so every scenario above ran
  # against a binary in which precedence 2 (mint from the broker) was compiled
  # OUT and replaced by a flat refusal. That went unnoticed because a
  # features-off build refuses too, and "it was refused" is satisfied by EITHER
  # cause — no credential, or no broker client compiled in.
  #
  # THAT FEATURE NO LONGER EXISTS. The minter is an inline UDS client with no
  # private dependency and is unconditional on unix, so "no broker client
  # compiled in" is not a state anvil can be built into any more. The assertion
  # is KEPT because what it actually pins was never the feature flag — it is
  # that the refusal REPORTS THE DIAL IT ATTEMPTED. `resolve_kit_bearer` has
  # four rungs and three distinct absences, and only the ones that reached the
  # broker path can name the socket variable:
  #
  #   minter present, no socket -> "...FOUNDRY_BROKER_SOCKET is unset, and no
  #                                 broker socket exists at the default path
  #                                 $HOME/.foundry/run/broker.sock..."      GREEN
  #   any refusal that short-circuits before choosing a dial                RED
  #
  # MEASURED, not asserted — this repository has shipped guards that were born
  # inert, so the claim above is the observed result of mutating the source:
  #
  #   short-circuit `resolve_kit_bearer` to refuse WITHOUT dialling (the exact
  #   shape of the historical features-off build)
  #     -> THIS scenario RED; "the refusal names the credential that was
  #        missing" stays GREEN.  <- the discrimination, still live
  #
  #   `choose_broker_dial()` returns None unconditionally
  #     -> THIS scenario stays GREEN (the no-socket refusal names the variable
  #        too, correctly), and the two rung-3 scenarios below go RED instead.
  #        So that regression IS caught — by them, not by this line.
  #
  # Both mutations reverted; the feature is 10/10 green on the restored tree.
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

  # THE RUNG THIS BRANCH EXISTS FOR.
  #
  # Rungs 1 and 2 both require ANOTHER program to have put something in this
  # process's environment. Harnesses snapshot their environment at session
  # start, so a variable added to a config file afterwards reaches only sessions
  # started later — which is how a whole fleet of already-running sessions
  # stayed locked out with no in-band way to recover, every turn answering
  # not_authenticated while a perfectly good broker sat listening on a
  # well-known path the process was not allowed to guess.
  #
  # Reading the PERSISTED transition actor is what makes this unsatisfiable by
  # an engine that never gated: `foundry:<sub>` appears only when the engine
  # verified a bearer and bound the principal to it. A standalone engine would
  # record the caller-supplied --actor-name verbatim instead.
  Scenario: with nothing in the environment naming a broker, the CLI finds the one at the default path
    Given the engine is started in Foundry mode with an accepting verifier for sub "user-abc"
    And a broker that mints for sub "user-def" sits at the default path in the scenario HOME
    When anvil-hooks begins a track under parent "20260411T2021_anvil_workflow_engine" with no Foundry credential and actor name "Imposter-000000"
    Then the anvil-hooks command succeeds
    And the anvil-hooks created track transition actor is "foundry:user-abc"
    And the anvil-hooks created track transition actor is not "Imposter-000000"

  # A SILENT BROKER MUST BE ABSENT, NOT A HANG.
  #
  # The broker client bounds only its UnixStream::connect (5s); both of its
  # response reads are unbounded. So a socket that accepts the connection and
  # never writes a line hangs the caller FOREVER — and this call site (`begin`)
  # sits under no cap of any kind, so nothing in the process would ever end it.
  #
  # The 1500ms ceiling is the assertion that matters. It is far under the
  # client's own 5s connect bound, so a pass cannot be explained by that bound
  # firing instead of the bearer's; and the message assertion alone would pass
  # just as well after a five-second hang, which is why the ceiling is here.
  Scenario: a broker that accepts and never answers resolves to absent inside the deadline
    Given the engine is started in Foundry mode with an accepting verifier for sub "user-abc"
    And a broker that accepts and never answers sits at the default path in the scenario HOME
    When anvil-hooks begins a track under parent "20260411T2021_anvil_workflow_engine" with no Foundry credential and actor name "Imposter-000000"
    Then the anvil-hooks command fails
    And the anvil-hooks output contains "not_authenticated"
    And the anvil-hooks output contains "did not answer within"
    And the anvil-hooks command completed within 1500 ms
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
