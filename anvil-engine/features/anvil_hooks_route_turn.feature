Feature: anvil-hooks route-turn ships each user turn into the engine route RPC
  The route-turn subcommand is the harness-side turn hook. It takes the user's
  prompt, calls the engine's route RPC (which RECORDS the routing decision via the
  activity log + routing-activity sink — the measurement), and prints routing
  guidance to stdout for the harness to inject. It ALWAYS fails open: a missing or
  slow engine yields exit 0 with no output and never blocks the turn.

  This seam proves the real BINARY against a real engine subprocess; the pure
  formatting + extraction are proven in the anvil-core seam.

  Scenario: a single-resolution turn prints guidance and the engine records it
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the kiln router HTTP stub returns verdict kind "daily_recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" against that engine
    Then the anvil-hooks route-turn command exits 0
    And the anvil-hooks route-turn output contains "daily_recap"
    And the engine stderr contains exactly 1 JSON log records with event_kind "routing_decision"

  Scenario: a single-resolution turn spoon-feeds purpose, begin call, and required fields
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap" purpose "Summarize the day's progress into a recap" required fields "date,highlights"
    And the kiln router HTTP stub returns verdict kind "daily_recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" against that engine
    Then the anvil-hooks route-turn command exits 0
    And the anvil-hooks route-turn output contains "Summarize the day's progress into a recap"
    And the anvil-hooks route-turn output contains "anvil_orchestrate(selection: \"daily_recap\")"
    And the anvil-hooks route-turn output contains "date, highlights"
    And the anvil-hooks route-turn output contains "BEGIN IT NOW"
    And the anvil-hooks route-turn output contains "A task is often BOTH a concrete deliverable AND this playbook's purpose"
    And the anvil-hooks route-turn output contains "Skip only if the playbook is genuinely IRRELEVANT"
    And the anvil-hooks route-turn output contains "Work outside a matched playbook is unguided and unmeasured"
    And the anvil-hooks route-turn output does not contain "Skip ONLY if the user's intent clearly does not fit a workflow"
    And the anvil-hooks route-turn output does not contain "say so explicitly in one line first"
    And the anvil-hooks route-turn output does not contain "Do this unless the user's intent clearly doesn't fit."

  Scenario: a --source turn tags the recorded route turn with its harness
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" and source "claude-code" against that engine
    Then the anvil-hooks route-turn command exits 0
    And the ActivitySummary RPC is called with granularity "day"
    And the activity summary RPC by_source "claude-code" has count 1

  Scenario: a turn with no --source is tagged (unattributed), never an empty source
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" against that engine
    Then the anvil-hooks route-turn command exits 0
    And the ActivitySummary RPC is called with granularity "day"
    And the activity summary RPC by_source "(unattributed)" has count 1

  Scenario: route-turn fails open when the engine is unreachable
    When anvil-hooks route-turn runs with message "daily recap" against a dead engine
    Then the anvil-hooks route-turn command exits 0
    And the anvil-hooks route-turn output is empty

  # resume_aware_routing H2: the route-turn hook reads the conversation/session id
  # from the harness's UserPromptSubmit JSON on stdin and carries it as the
  # RouteRequest.conversation_id. When that conversation has an open playbook and
  # the user sends a continuation token ("go"), the engine's resume pre-check fires
  # and the hook renders the RESUME nudge — pointing the agent back at the
  # in-progress step rather than offering new candidates.
  Scenario: route-turn carries the stdin session_id so a continuation token resumes the open playbook
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the route hearth has an open "daily_recap" artifact "20260601T0001_recap" in state "active" begun for conversation "sess-resume-1" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with stdin UserPromptSubmit session_id "sess-resume-1" prompt "go" against that engine
    Then the anvil-hooks route-turn command exits 0
    And the anvil-hooks route-turn output contains "Resume daily_recap (active)"
    And the anvil-hooks route-turn output contains "20260601T0001_recap"
    And the anvil-hooks route-turn output contains "continue via complete → completed (role: doer)"
    And the anvil-hooks route-turn output does not contain "Anvil matched several workflows"
    And the anvil-hooks route-turn output does not contain "anvil_orchestrate(selection:"

  # continuation_recognition phase 1 — the OBSERVABILITY chain, built and proven
  # before any new recognition behaviour depends on it.
  #
  # `resume_source` has to survive engine decision -> RouteResponse -> hook ->
  # delivery-log.jsonl. That is six links, and a field that stops at link four is
  # invisible in exactly the way the 1500ms delivery outage was invisible: an
  # unlogged early return on this same path, which read as a quiet router for days
  # and produced three confident wrong diagnoses before anyone found it.
  #
  # So this scenario asserts the LOG, never the response. Only the end of the
  # chain proves the chain. Behaviour is unchanged here — this is today's
  # exact-token resume, now saying WHY it resumed.
  Scenario: a bare-token resume records resume_source continuation_token in the delivery log
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the route hearth has an open "daily_recap" artifact "20260601T0002_recap" in state "active" begun for conversation "sess-resume-2" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with stdin UserPromptSubmit session_id "sess-resume-2" prompt "go" against that engine
    Then the anvil-hooks route-turn command exits 0
    And the delivery log records resume_source "continuation_token"

  # route_response_mirrors_begin H3 (route-turn surface): when the guidance
  # enrichment fails (declared hook file missing), the engine returns a thin
  # candidate and a Kiln pick renders the thin nudge — the kind + begin call.
  Scenario: route-turn renders the thin nudge when guidance enrichment fails
    Given a route hearth with one driven machine kind "brittle" trigger "do the brittle thing" declaring a missing hook
    And the kiln router HTTP stub returns verdict kind "brittle"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "do the brittle thing" against that engine
    Then the anvil-hooks route-turn command exits 0
    And the anvil-hooks route-turn output contains "brittle"
    And the anvil-hooks route-turn output contains "anvil_orchestrate(selection: \"brittle\")"

  # router_precision fix #2: the Kiln router selects from the engine's CAPPED,
  # turn-relevant matching set — NOT the full access-scoped granted flood. For
  # "daily recap" the matching set is {daily_recap}; a Kiln pick of "track" (granted
  # but NOT matching this turn) is off-menu, so the hook stays silent rather than
  # nudging an irrelevant playbook. This is the flood-narrowing precision win.
  # CONTRACT CHANGE 2026-07-28. This scenario previously asserted the router could NOT
  # select outside the turn's MATCHING set. That constraint was the defect: winner-take-all
  # lexical narrowing collapses the matching set to one kind, so the router was asked
  # "does this one fit?" instead of "which of these fits?" and correctly declined wrong
  # picks — delivering the user nothing. Measured N=4: widening to the GRANTED set is
  # +5.25 hits per 40 frozen-H rows against a control with zero spread.
  #
  # The router may now select any GRANTED kind. The bound that remains real is that it
  # cannot invent a kind outside the granted set, which the second scenario pins.
  Scenario: the Kiln router may select a granted playbook outside the turn's matching set
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the route hearth also has a driven "track" machine with trigger "start a track"
    And the kiln router HTTP stub returns verdict kind "track"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" against that engine
    Then the anvil-hooks route-turn command exits 0
    And the anvil-hooks route-turn output contains "track"

  # The bound that survives the widening: granted is the ceiling. A verdict naming a kind
  # the caller was never granted must be refused, not rendered.
  Scenario: the Kiln router cannot select a playbook outside the GRANTED set
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the kiln router HTTP stub returns verdict kind "not_a_granted_kind"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" against that engine
    Then the anvil-hooks route-turn command exits 0
    And the anvil-hooks route-turn output is empty

  # router_precision fix #1 (NO-TASK) at the live seam: a system-reminder-only turn
  # carries no genuine task, so the engine abstains (empty matching set) and the hook
  # stays silent — even though the reminder text contains the "daily recap" trigger
  # phrase and Kiln is stubbed to pick it. The no-task gate wins over trigger match.
  Scenario: a no-task system-reminder turn leaves the hook silent
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the kiln router HTTP stub returns verdict kind "daily_recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "<system-reminder>daily recap reminder: settle up</system-reminder>" against that engine
    Then the anvil-hooks route-turn command exits 0
    And the anvil-hooks route-turn output is empty

  Scenario: the Kiln router reason is surfaced in the single-playbook nudge
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the route hearth also has a driven "track" machine with trigger "start a track"
    And the kiln router HTTP stub returns verdict kind "track" why "this is a concrete feature implementation, so the track lifecycle will scope+measure it"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "start a track" against that engine
    Then the anvil-hooks route-turn command exits 0
    And the anvil-hooks route-turn output contains "track"
    And the anvil-hooks route-turn output contains "this is a concrete feature implementation, so the track lifecycle will scope+measure it"
    And the anvil-hooks route-turn output contains "BEGIN IT NOW"

  Scenario: the Kiln router can abstain and leave the hook silent
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the route hearth also has a driven "track" machine with trigger "start a track"
    And the kiln router HTTP stub returns verdict abstain
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" against that engine
    Then the anvil-hooks route-turn command exits 0
    And the anvil-hooks route-turn output is empty

  # AMENDED by T-RDG: "silent" now means "routes no playbook", not "writes
  # nothing". A router that could not answer says so on the turn; asserting the
  # empty stream instead of the absent playbook would make the new notice look
  # like a regression when it is the requirement.
  Scenario: a closed Kiln router port leaves the hook routing no playbook, and says why
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the route hearth also has a driven "track" machine with trigger "start a track"
    And the kiln router port is closed
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" against that engine
    Then the anvil-hooks route-turn command exits 0
    And the anvil-hooks route-turn output does not contain "BEGIN IT NOW"
    And the anvil-hooks route-turn output contains "playbook selection degraded"

  # context_aware_routing: the real binary tails the transcript pointed at by the
  # UserPromptSubmit `transcript_path`, distills the recent-context digest + the
  # in-progress signal, and threads them into the Kiln router prompt. Proven by
  # capturing the request the binary sent to the (stubbed) Kiln router. Here the
  # transcript carries an OPEN begin (kind track) with no later complete → the
  # prompt must carry the "IN PROGRESS" bias AND the recent user turns.
  Scenario: the hook extracts recent context and an open-playbook signal from the transcript and threads them to Kiln
    Given a route hearth with one unrestricted driven machine kind "track" trigger "start a track"
    And the kiln router HTTP stub returns verdict kind "track", records the request, and the hearth has a transcript fixture:
      """
      {"type":"user","message":{"role":"user","content":"implement the caching layer for the API"}}
      {"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Beginning the track now."},{"type":"tool_use","name":"mcp__anvil__begin","input":{"artifact_type":"track"}}]}}
      {"type":"user","message":{"role":"user","content":"go"}}
      """
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with stdin UserPromptSubmit prompt "start a track for the caching layer" and the hearth transcript against that engine
    Then the anvil-hooks route-turn command exits 0
    And the captured kiln request contains "Recent conversation context"
    And the captured kiln request contains "implement the caching layer for the API"
    And the captured kiln request contains "already IN PROGRESS"

  # context_aware_routing: a transcript with playbook-shaped activity (a forge
  # lifecycle skill) but NO begin → the prompt must carry the work-without-a-begin
  # bias so the selector pushes the work into a begin.
  Scenario: the hook extracts a work-without-begin signal from the transcript and threads it to Kiln
    Given a route hearth with one unrestricted driven machine kind "track" trigger "start a track"
    And the kiln router HTTP stub returns verdict kind "track", records the request, and the hearth has a transcript fixture:
      """
      {"type":"user","message":{"role":"user","content":"write the spec for the auth refactor"}}
      {"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"Skill","input":{"skill":"forge:spec"}}]}}
      {"type":"user","message":{"role":"user","content":"keep going"}}
      """
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with stdin UserPromptSubmit prompt "start a track for the auth refactor" and the hearth transcript against that engine
    Then the anvil-hooks route-turn command exits 0
    And the captured kiln request contains "WITHOUT a begin"
    And the captured kiln request contains "write the spec for the auth refactor"

  # engine_call_failed named THREE different faults at once — an absent engine, a bad
  # reply, and a blown deadline — so the delivery log could not say whether to fix
  # availability or latency. The classification comment guessed the deadline was "the
  # common one"; the live log disagreed (33 failures, arriving in bursts, with ZERO
  # minutes containing both a failure and a success — the shape of absence, not
  # slowness). Each cause now names itself.
  Scenario: an unreachable engine is recorded as engine_unreachable, not a generic failure
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" against a closed port
    Then the anvil-hooks route-turn command exits 0
    And the delivery log records outcome "engine_unreachable"

  Scenario: an engine that never answers within the cap is recorded as engine_timeout
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" against that engine with a 1ms cap
    Then the anvil-hooks route-turn command exits 0
    And the delivery log records outcome "engine_timeout"


  # continuation_recognition phase 5 — the rejection reason.
  #
  # The rejection marker set is deliberately OVER-BROAD, on the grounds that a
  # missed continuation costs one turn of guidance while a mis-routed rejection
  # actions work the human refused. That trade is only defensible if the losses
  # are COUNTABLE, and an earlier revision claimed they would be because they
  # "land in the existing no_candidate bucket" — which was false. Flat reason
  # strings are exactly what made engine_call_failed unreadable.
  #
  # "don't stop, continue" is an ACCEPTED LOSS: a real continuation, declined.
  # It is asserted here so the price of the over-broad set is pinned by a test
  # rather than left as prose.
  Scenario: a rejected continuation is recorded under its own reason, not folded into no_candidate
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the route hearth has an open "daily_recap" artifact "20260601T0003_recap" in state "active" begun for conversation "sess-reject-1" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with stdin UserPromptSubmit session_id "sess-reject-1" prompt "don't stop, continue" against that engine
    Then the anvil-hooks route-turn command exits 0
    And the delivery log records outcome "continuation_rejected"
    And the delivery log records resume_source "rejected"
