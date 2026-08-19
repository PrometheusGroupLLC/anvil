Feature: the route hook's single-tier router backend (Kiln → Fireworks, the ONLY path)
  The Kiln router client makes exactly ONE call, under one bounded wall-clock cap, to
  the local Kiln gateway (which routes to Fireworks). There is NO fallback backend and
  NO retry: reliability for a flaky Fireworks is KILN's responsibility. A parseable
  Pick/Abstain becomes the verdict; the gateway unreachable, hung past the time-box, or
  returning an unparseable reply FAILS OPEN to NoMatch — the turn simply does not route
  that turn and proceeds unharmed. A single per-call telemetry record (served_by +
  outcome) is emitted so hit-rate vs fail-open is observable.

  These scenarios drive the REAL anvil-hooks binary against a real engine, injecting one
  stub Kiln port. The pure prompt build / reply parse live in the anvil-core seam.

  Scenario: the Kiln call selects a candidate and the router serves it
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" kiln "answer:daily_recap" token "" model "" enabled "on" timeout 3000 against that engine
    Then the anvil-hooks route-turn command exits 0
    And the anvil-hooks route-turn output contains "daily_recap"
    And the kiln stub captured a request
    And the router telemetry contains "\"served_by\":\"kiln\""
    And the router telemetry contains "\"tier\":\"kiln\",\"leg\":\"hook\",\"outcome\":\"hit\""
    And the router telemetry file contains "\"event\":\"router_tier_attempts\""
    And the router telemetry file contains "\"served_by\":\"kiln\""

  # AMENDED by T-RDG: this used to assert an EMPTY stdout. A router that failed to
  # answer now says so on the turn, so the property this scenario really guards —
  # no playbook was routed — is asserted directly instead of inferred from
  # silence. `transport_err` keeps its meaning here and only here: nothing is
  # listening, so no HTTP reply ever arrived.
  Scenario: a down Kiln gateway fails open to NoMatch with no fallback backend
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" kiln "closed" token "" model "" enabled "on" timeout 3000 against that engine
    Then the anvil-hooks route-turn command exits 0
    And the anvil-hooks route-turn output does not contain "daily_recap"
    And the anvil-hooks route-turn output contains "the kiln gateway was unreachable"
    And the router telemetry contains "\"served_by\":\"none\""
    And the router telemetry contains "\"tier\":\"kiln\",\"leg\":\"hook\",\"outcome\":\"transport_err\""
    And the router telemetry does not contain "\"tier\":\"fallback\""
    And the router telemetry does not contain "\"tier\":\"primary\""

  Scenario: a hung Kiln gateway is time-boxed and fails open within the cap
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" kiln "hang" token "" model "" enabled "on" timeout 500 against that engine
    Then the anvil-hooks route-turn command exits 0
    And the anvil-hooks route-turn output does not contain "daily_recap"
    And the anvil-hooks route-turn output contains "the kiln call timed out"
    And the router telemetry contains "\"tier\":\"kiln\",\"leg\":\"hook\",\"outcome\":\"timeout\""
    And the router telemetry contains "\"served_by\":\"none\""
    And the route-turn elapsed under 3000 ms

  Scenario: the configured model reaches the Kiln request
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" kiln "answer:daily_recap" token "" model "accounts/fireworks/models/deepseek-v4-flash" enabled "on" timeout 3000 against that engine
    Then the anvil-hooks route-turn command exits 0
    And the anvil-hooks route-turn output contains "daily_recap"
    And the kiln captured request contains "accounts/fireworks/models/deepseek-v4-flash"

  Scenario: the Kiln request carries the gateway bearer but no Fireworks provider key
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" kiln "answer:daily_recap" token "gw-secret-abc" model "" enabled "on" timeout 3000 against that engine
    Then the anvil-hooks route-turn command exits 0
    And the anvil-hooks route-turn output contains "daily_recap"
    And the kiln captured request contains "Authorization: Bearer gw-secret-abc"
    And the kiln captured request contains "x-kiln-sensitivity: public"
    And the kiln captured request does not contain "sk-"

  Scenario: with no gateway token the Kiln request carries no Authorization header
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" kiln "answer:daily_recap" token "" model "" enabled "on" timeout 3000 against that engine
    Then the anvil-hooks route-turn command exits 0
    And the anvil-hooks route-turn output contains "daily_recap"
    And the kiln captured request does not contain "Authorization"

  # The kill switch: ANVIL_ROUTER_ENABLED=off makes NO Kiln call at all — routing fails
  # open and the stub is never dialed.
  Scenario: routing disabled makes no Kiln call and fails open
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" kiln "answer:daily_recap" token "" model "" enabled "off" timeout 3000 against that engine
    Then the anvil-hooks route-turn command exits 0
    And the anvil-hooks route-turn output is empty
    And the kiln stub captured no request
    And the router telemetry contains "\"served_by\":\"none\""

  # ── the durable config file (~/.anvil/router.json) ──
  # The model + kill-switch are file-configurable (harness-agnostic). Precedence is
  # ENV > ~/.anvil/router.json > built-in default; a malformed file is treated as absent.

  Scenario: the config file model reaches the Kiln request when the env is unset
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" kiln "answer:daily_recap" router-file "enabled:on" model "accounts/fireworks/models/deepseek-v4-flash" timeout 3000 against that engine
    Then the anvil-hooks route-turn command exits 0
    And the anvil-hooks route-turn output contains "daily_recap"
    And the kiln captured request contains "accounts/fireworks/models/deepseek-v4-flash"

  Scenario: the config file kill switch disables routing
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" kiln "answer:daily_recap" router-file "enabled:off" model "" timeout 3000 against that engine
    Then the anvil-hooks route-turn command exits 0
    And the anvil-hooks route-turn output is empty
    And the kiln stub captured no request

  Scenario: a malformed config file is treated as absent and routing still fires
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" kiln "answer:daily_recap" router-file "malformed" model "" timeout 3000 against that engine
    Then the anvil-hooks route-turn command exits 0
    And the anvil-hooks route-turn output contains "daily_recap"
    And the kiln stub captured a request

  # Anvil dials Kiln TWICE per turn — engine-side (full granted candidate set) and
  # hook-side (transcript tail + in-progress signal). Until `leg` existed the two were
  # indistinguishable in telemetry, so "which call is slow" and "what does the second
  # one cost" could not be answered, and the argument about collapsing them was being
  # had on inference. Cost rides along for the same reason: a leg that is cheap and
  # fast is not the one to cut.
  Scenario: the router records WHICH leg made the call, and what it cost
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" kiln "answer:daily_recap" token "" model "" enabled "on" timeout 3000 against that engine
    Then the anvil-hooks route-turn command exits 0
    And the router telemetry contains "\"leg\":\"hook\""
    And the router telemetry contains "\"total_tokens\""

