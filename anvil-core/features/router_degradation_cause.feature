Feature: router degradation is named, not flattened into transport_err
  When the Kiln router produces no verdict, WHY matters: a rotated bearer, an
  exhausted spend ceiling, a model the provider stopped serving and a genuine
  connect failure need four different fixes. Until now they were one label —
  `transport_err` — because `post_chat_completion` returned `None` on any
  non-2xx and the caller had nothing else it could say.

  That label sends every reader hunting a network problem. Measured 2026-08-16:
  kiln refused hours of routing calls with 403 `budget_ceiling_reached` and the
  log said `transport_err`; diagnosis cost four wrong turns and one retracted
  cause.

  The seam is anvil-core and the user is the engine. The engine owns the socket;
  the judgement is pure and lives here, so it is provable without one. Kiln's
  error envelope is `{"error":{"message":…,"type":"kiln","code":"<code>"}}`
  (kiln-serve/src/handler.rs), so the code is preferred and the status class is
  the fallback. There is no `unknown` arm and no arm returns `transport_err`:
  this function only ever sees a reply that ARRIVED.

  Scenario: kiln's own budget code on a 403 is named as an exhausted budget
    When a kiln reply with status 403 code "budget_ceiling_reached" message "daily ceiling reached" is classified
    Then the classified router cause is "budget_exhausted"

  Scenario: kiln's own unauthorized code on a 401 is named as a credential problem
    When a kiln reply with status 401 code "unauthorized" message "missing or invalid bearer" is classified
    Then the classified router cause is "unauthorized"

  # The live 2026-08-16 shape. Kiln's code is `upstream_error` for BOTH a broken
  # upstream and a model the upstream no longer serves; the only thing separating
  # them is the message. A 404 from the provider means the configured model id is
  # not served — a config fix, not a retry — so the message is read.
  Scenario: a 502 upstream_error whose message names an upstream 404 is named as a missing model
    When a kiln reply with status 502 code "upstream_error" message "upstream status 404 Not Found" is classified
    Then the classified router cause is "model_unavailable"

  # The other arm of the same code, so the discriminator is proven to be READ and
  # not merely correlated: same code, same status, no 404 in the message.
  Scenario: a 502 upstream_error with no 404 in its message stays an upstream error
    When a kiln reply with status 502 code "upstream_error" message "connection reset by peer" is classified
    Then the classified router cause is "upstream_error"

  Scenario: kiln's no-target code is named as a missing model
    When a kiln reply with status 400 code "no_target_for_model" message "no compliant target serves that model" is classified
    Then the classified router cause is "model_unavailable"

  # No envelope to read — a proxy's HTML error page, a truncated body, an empty
  # one. The status class still has to produce a name.
  Scenario: an unparseable body falls back to the status class
    When a kiln reply with status 401 and body "<html>401 Unauthorized</html>" is classified
    Then the classified router cause is "unauthorized"

  Scenario: an unparseable 5xx body falls back to an upstream error
    When a kiln reply with status 503 and body "" is classified
    Then the classified router cause is "upstream_error"

  # A 403 with no kiln code at all, but a body that names the ceiling. The status
  # alone cannot tell a budget refusal from any other refusal, so the words are
  # what decide it.
  Scenario: a 403 body naming a ceiling is named as an exhausted budget without a code
    When a kiln reply with status 403 and body "spend ceiling reached for this org" is classified
    Then the classified router cause is "budget_exhausted"

  # The unmodelled arm. A refusal this taxonomy does not know is still a refusal
  # and never a network fault — which is the whole point of the track.
  Scenario: an unmodelled refusal is named refused and never transport_err
    When a kiln reply with status 409 code "sensitivity_escalation" message "continuation escalated sensitivity" is classified
    Then the classified router cause is "refused"
    And the classified router cause is not "transport_err"

  # The turn-visible half. The fail-open contract says a hook NEVER breaks a
  # turn, so "the lexical fallback returns an error" can only mean the reader is
  # TOLD. The notice names the cause in human words.
  Scenario: the notice for a degraded turn names the cause and the lexical fallback
    When the degradation notice is rendered for cause "budget_exhausted" degrading to lexical
    Then the degradation notice is "playbook selection degraded: kiln budget exhausted; routing fell back to lexical"

  # The same cause, the other tail. Degrading to the lexical pick and suggesting
  # nothing at all are two different outcomes and the notice says which.
  Scenario: the notice for a silent turn names the cause and says nothing was suggested
    When the degradation notice is rendered for cause "budget_exhausted" with no playbook suggested
    Then the degradation notice is "playbook selection degraded: kiln budget exhausted; no playbook was suggested this turn"

  # The 1500ms wall WAS a timeout. A notice that omits the common transient
  # causes teaches its reader that silence means health, which is exactly how
  # that outage stayed invisible for a day.
  Scenario: a timeout is a degradation and says so in words
    When the degradation notice is rendered for cause "timeout" with no playbook suggested
    Then the degradation notice is "playbook selection degraded: the kiln call timed out; no playbook was suggested this turn"

  Scenario: an unreachable gateway is named as unreachable, not as a generic failure
    When the degradation notice is rendered for cause "transport_err" with no playbook suggested
    Then the degradation notice contains "the kiln gateway was unreachable"

  # A cause this renderer does not know still reaches the reader verbatim rather
  # than being swallowed — the failure mode this whole track is about.
  Scenario: an unfamiliar cause still reaches the reader
    When the degradation notice is rendered for cause "some_future_cause" with no playbook suggested
    Then the degradation notice contains "some_future_cause"
