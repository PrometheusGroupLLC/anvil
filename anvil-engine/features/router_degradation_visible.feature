Feature: a degraded router says so — by name, on the turn, and in the log
  The seam is anvil-engine and the user is the harness running the hook. These
  scenarios drive the REAL `anvil-hooks route-turn` binary against a REAL engine
  with a stub kiln gateway returning a real kiln error envelope, and assert what
  a reader can actually see afterwards: the telemetry line, stdout, and the
  delivery row.

  Two properties, and the second is the one that was missing.

  NAMED. `post_chat_completion` used to return `None` on any non-2xx, so a
  rotated bearer, an exhausted budget, a retired model id and a genuine connect
  failure were one label: `transport_err`. Every scenario below that drives a
  status-carrying reply asserts BOTH its own cause and the absence of
  `transport_err`, because the defect was never a missing cause — it was a
  confidently wrong one.

  SAID. The fail-open contract is untouched: a hook NEVER breaks a turn, and
  every scenario here still exits 0. "Returns an error" therefore means the
  reader is TOLD, on the same stdout channel the guidance already uses.

  Scenario: an exhausted kiln budget is named as a budget, on the turn and in both logs
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" kiln "error:403:budget_ceiling_reached:daily ceiling of $25.00 reached" token "" model "" enabled "on" timeout 3000 against that engine
    Then the anvil-hooks route-turn command exits 0
    And the router telemetry contains "\"outcome\":\"budget_exhausted\""
    And the router telemetry does not contain "\"outcome\":\"transport_err\""
    And the anvil-hooks route-turn output contains "playbook selection degraded"
    And the anvil-hooks route-turn output contains "kiln budget exhausted"
    And the delivery log records router_cause "budget_exhausted"
    # The notice is NOT guidance. It goes to the same stdout, so only the ORDER —
    # row computed, then notice appended — keeps a turn that suggested nothing out
    # of the delivered denominator of the sink this change is extending.
    And the delivery log records guidance_produced "false"

  Scenario: a rejected gateway credential is named as a credential problem, not a network one
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" kiln "error:401:unauthorized:missing or invalid bearer" token "" model "" enabled "on" timeout 3000 against that engine
    Then the anvil-hooks route-turn command exits 0
    And the router telemetry contains "\"outcome\":\"unauthorized\""
    And the router telemetry does not contain "\"outcome\":\"transport_err\""
    And the anvil-hooks route-turn output contains "kiln rejected the gateway credential"
    And the delivery log records router_cause "unauthorized"

  # The live 2026-08-16 shape: the configured model was an alias kiln still
  # advertised in /v1/models that the provider had stopped serving, and it
  # arrived as a 502 `upstream_error` whose message carried the upstream 404.
  Scenario: a model the provider no longer serves is named as a missing model
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" kiln "error:502:upstream_error:upstream status 404 Not Found" token "" model "" enabled "on" timeout 3000 against that engine
    Then the anvil-hooks route-turn command exits 0
    And the router telemetry contains "\"outcome\":\"model_unavailable\""
    And the router telemetry does not contain "\"outcome\":\"transport_err\""
    And the anvil-hooks route-turn output contains "the routing model is not served"
    And the delivery log records router_cause "model_unavailable"

  # The 1500ms wall WAS a timeout. Suppressing the transient causes is exactly
  # how that outage stayed invisible for a day, so a timed-out turn says so too.
  Scenario: a timed-out router call is a degradation and says so on the turn
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" kiln "hang" token "" model "" enabled "on" timeout 500 against that engine
    Then the anvil-hooks route-turn command exits 0
    And the anvil-hooks route-turn output contains "the kiln call timed out"
    And the delivery log records router_cause "timeout"

  # The selector working is not degradation. A hit writes no notice at all, and
  # its delivery row carries no cause — otherwise the notice would be noise on
  # every turn and the cause column would answer nothing.
  Scenario: a turn the router answered carries no notice and no cause
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" kiln "answer:daily_recap" token "" model "" enabled "on" timeout 3000 against that engine
    Then the anvil-hooks route-turn command exits 0
    And the anvil-hooks route-turn output contains "daily_recap"
    And the anvil-hooks route-turn output does not contain "playbook selection degraded"
    And the delivery log records router_cause ""

  # Routing switched OFF is a CONFIGURATION, not a degradation. No call is made,
  # so there is no cause to name and nothing to warn about; a notice here would
  # nag every turn of a deliberately-disabled router.
  Scenario: routing disabled is not a degradation and writes no notice
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" kiln "answer:daily_recap" token "" model "" enabled "off" timeout 3000 against that engine
    Then the anvil-hooks route-turn command exits 0
    And the kiln stub captured no request
    And the anvil-hooks route-turn output does not contain "playbook selection degraded"
    And the delivery log records router_cause ""
