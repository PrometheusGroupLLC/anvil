Feature: route-turn distills the engine's route outcome into harness guidance
  The anvil-hooks route-turn subcommand ships every user turn into the engine's
  route RPC (which records the routing decision) and injects the routing guidance
  back to the agent. This seam proves the PURE half: given a distilled route
  outcome, the guidance text the harness injects; and the user-message extraction
  from a harness's user-prompt event JSON. The stdin/gRPC plumbing + fail-open are
  proven against the real binary in the engine seam.

  Scenario: a single-resolution outcome spoon-feeds purpose, begin call, and required fields
    Given a route-turn outcome single with kind "daily_recap" purpose "Summarize the day's progress into a recap." required fields "date,highlights"
    When the route-turn guidance is formatted
    Then the route-turn guidance contains "daily_recap"
    And the route-turn guidance contains "Summarize the day's progress into a recap"
    And the route-turn guidance contains "anvil_orchestrate(selection: \"daily_recap\")"
    And the route-turn guidance contains "date, highlights"
    And the route-turn guidance contains "BEGIN IT NOW"
    And the route-turn guidance contains "automatic measurement"
    And the route-turn guidance contains "A task is often BOTH a concrete deliverable AND this playbook's purpose"
    And the route-turn guidance contains "Skip only if the playbook is genuinely IRRELEVANT"
    And the route-turn guidance contains "Work outside a matched playbook is unguided and unmeasured"
    And the route-turn guidance does not contain "Skip ONLY if the user's intent clearly does not fit a workflow"
    And the route-turn guidance does not contain "say so explicitly in one line first"
    And the route-turn guidance does not contain "Do this unless the user's intent clearly doesn't fit."
    # The one-call begin lever: a ready-to-run anvil-hooks begin with generic fields as --field.
    And the route-turn guidance contains "anvil-hooks begin --artifact-type daily_recap --field date=<value> --field highlights=<value>"

  Scenario: a single-resolution outcome includes the resolved conversation id in the begin call
    Given a route-turn outcome single with kind "daily_recap" purpose "Summarize the day." required fields "date"
    And a route-turn conversation id "sess-route-123"
    When the route-turn guidance is formatted
    Then the route-turn guidance contains "anvil_orchestrate(selection: \"daily_recap\", conversation_id: \"sess-route-123\")"
    And the route-turn guidance does not contain "conversation_id: \"\""

  Scenario: a single-resolution outcome without a conversation id omits the field
    Given a route-turn outcome single with kind "daily_recap" purpose "Summarize the day." required fields "date"
    When the route-turn guidance is formatted
    Then the route-turn guidance contains "anvil_orchestrate(selection: \"daily_recap\")"
    And the route-turn guidance does not contain "conversation_id:"

  Scenario: a single-resolution outcome with no required fields says so
    Given a route-turn outcome single with kind "daily_recap" purpose "Summarize the day." required fields ""
    When the route-turn guidance is formatted
    Then the route-turn guidance contains "anvil_orchestrate(selection: \"daily_recap\")"
    And the route-turn guidance contains "no required fields"

  # harness_stdout_contract: Kiln's UserPromptSubmit reader injects ONLY a JSON
  # `additionalContext` field; raw text is dropped. The --source kiln turn must
  # therefore emit JSON so the guidance actually reaches the model.
  Scenario: a Kiln-source turn wraps the guidance as JSON additionalContext
    Given a route-turn outcome single with kind "daily_recap" purpose "Summarize the day." required fields "date"
    When the route-turn guidance is formatted
    And the route-turn guidance is shaped for source "kiln"
    Then the route-turn guidance is JSON with additionalContext containing "daily_recap"
    And the route-turn guidance is JSON with additionalContext containing "BEGIN IT NOW"

  Scenario: a non-Kiln source leaves the guidance as raw stdout text
    Given a route-turn outcome single with kind "daily_recap" purpose "Summarize the day." required fields "date"
    When the route-turn guidance is formatted
    And the route-turn guidance is shaped for source "claude-code"
    Then the route-turn guidance is not JSON
    And the route-turn guidance contains "daily_recap"

  Scenario: a playbook selection spoon-feeds the begin call and required fields
    Given a route-turn outcome single with kind "playbook" purpose "Define a new artifact lifecycle." required fields "playbook_name,parent_id,approver"
    When the route-turn guidance is formatted
    Then the route-turn guidance contains "playbook"
    And the route-turn guidance contains "Define a new artifact lifecycle"
    And the route-turn guidance contains "anvil_orchestrate(selection: \"playbook\")"
    And the route-turn guidance contains "playbook_name, parent_id, approver"
    # builtin fields map to their structured begin flags, not --field
    And the route-turn guidance contains "anvil-hooks begin --artifact-type playbook --playbook-name <playbook_name> --parent-id <parent_id> --approver <approver>"

  Scenario: a candidates outcome lists each candidate's exact begin call and required fields
    Given a route-turn outcome candidates with kinds purposes and required fields "daily_recap|Summarize the day's progress.|date,highlights;weekly_recap|Summarize the week's progress.|week,owner"
    When the route-turn guidance is formatted
    Then the route-turn guidance contains "daily_recap"
    And the route-turn guidance contains "Summarize the day's progress"
    And the route-turn guidance contains "anvil_orchestrate(selection: \"daily_recap\")"
    And the route-turn guidance contains "needs: date, highlights"
    And the route-turn guidance contains "weekly_recap"
    And the route-turn guidance contains "Summarize the week's progress"
    And the route-turn guidance contains "anvil_orchestrate(selection: \"weekly_recap\")"
    And the route-turn guidance contains "needs: week, owner"
    And the route-turn guidance contains "a task can be both a deliverable and a playbook's purpose"
    And the route-turn guidance contains "skip only if NONE is genuinely relevant"
    And the route-turn guidance does not contain "skip only if none genuinely fits, and say why"

  Scenario: a candidates route response renders only the ranked matching candidates
    Given a candidates route response with granted candidate kinds "daily_recap,weekly_recap,track,proposal,milestone" and matching candidate kinds "track,daily_recap,milestone"
    Then the route-turn outcome is candidates with kinds "track,daily_recap,milestone"
    When the route-turn guidance is formatted
    Then the route-turn guidance contains "anvil_orchestrate(selection: \"track\")"
    And the route-turn guidance contains "needs: field_track"
    And the route-turn guidance contains "Intent for track"
    And the route-turn guidance contains "step_track_one → step_track_two"
    And the route-turn guidance contains "why track fits"
    And the route-turn guidance contains "anvil_orchestrate(selection: \"daily_recap\")"
    And the route-turn guidance contains "anvil_orchestrate(selection: \"milestone\")"
    And the route-turn guidance does not contain "anvil_orchestrate(selection: \"weekly_recap\")"
    And the route-turn guidance does not contain "anvil_orchestrate(selection: \"proposal\")"

  Scenario: a single route response carries the router reason into the nudge
    Given a single route response with kind "track" why "this is a concrete feature implementation, so the track lifecycle will scope+measure it"
    When the route-turn guidance is formatted
    Then the route-turn guidance contains "track"
    And the route-turn guidance contains "this is a concrete feature implementation, so the track lifecycle will scope+measure it"
    And the route-turn guidance contains "BEGIN IT NOW"

  Scenario: a candidates outcome includes the resolved conversation id in each begin call
    Given a route-turn outcome candidates with kinds purposes and required fields "daily_recap|Summarize the day's progress.|date,highlights;weekly_recap|Summarize the week's progress.|week,owner"
    And a route-turn conversation id "sess-route-456"
    When the route-turn guidance is formatted
    Then the route-turn guidance contains "anvil_orchestrate(selection: \"daily_recap\", conversation_id: \"sess-route-456\")"
    And the route-turn guidance contains "anvil_orchestrate(selection: \"weekly_recap\", conversation_id: \"sess-route-456\")"
    And the route-turn guidance does not contain "conversation_id: \"\""

  Scenario: a candidates outcome without a conversation id omits the field
    Given a route-turn outcome candidates with kinds purposes and required fields "daily_recap|Summarize the day's progress.|date,highlights;weekly_recap|Summarize the week's progress.|week,owner"
    When the route-turn guidance is formatted
    Then the route-turn guidance contains "anvil_orchestrate(selection: \"daily_recap\")"
    And the route-turn guidance contains "anvil_orchestrate(selection: \"weekly_recap\")"
    And the route-turn guidance does not contain "conversation_id:"

  Scenario: a no-match outcome stays silent (empty guidance)
    Given a route-turn outcome no_match
    When the route-turn guidance is formatted
    Then the route-turn guidance is empty

  # route_response_mirrors_begin Phase 3: a single render carries the engine's
  # begin-equivalent guidance body in full, plus the begin call.
  Scenario: a single render includes the begin-equivalent guidance body and the begin call
    Given a route-turn outcome single with kind "daily_recap" guidance "RICH-FIRST-STEP-BODY for the daily recap"
    When the route-turn guidance is formatted
    Then the route-turn guidance contains "anvil_orchestrate(selection: \"daily_recap\")"
    And the route-turn guidance contains "RICH-FIRST-STEP-BODY for the daily recap"

  # An empty guidance degrades to the thin one-line nudge (fail-open).
  Scenario: a single render with empty guidance degrades to the thin nudge
    Given a route-turn outcome single with kind "daily_recap" guidance ""
    When the route-turn guidance is formatted
    Then the route-turn guidance contains "anvil_orchestrate(selection: \"daily_recap\")"

  # route_response_mirrors_begin Phase 3: each candidate is annotated with intent,
  # step_outline, and why_fits so the model chooses deliberately.
  Scenario: a candidates render annotates each option with intent, steps, and why_fits
    Given a route-turn outcome candidates annotated kind "daily_recap" intent "Collect the day's deduplicated evidence" steps "gathering,synthesizing,reporting" why "matched trigger \"daily recap\""
    When the route-turn guidance is formatted
    Then the route-turn guidance contains "Collect the day's deduplicated evidence"
    And the route-turn guidance contains "gathering → synthesizing → reporting"
    And the route-turn guidance contains "matched trigger \"daily recap\""
    And the route-turn guidance contains "anvil_orchestrate(selection: \"daily_recap\")"

  Scenario: the user message is extracted from a Claude Code UserPromptSubmit payload
    Given a user-prompt event JSON with prompt "ship the daily recap"
    When the route-turn user message is extracted
    Then the extracted user message is "ship the daily recap"

  Scenario: the user message is extracted from a Hermes pre_llm_call messages array
    Given a Hermes pre_llm_call event JSON whose last user message is "route this turn"
    When the route-turn user message is extracted
    Then the extracted user message is "route this turn"

  Scenario: the subagent mission is extracted from a Claude Code PreToolUse Task payload
    Given a PreToolUse Task event JSON whose subagent prompt is "audit the auth module"
    When the route-turn user message is extracted
    Then the extracted user message is "audit the auth module"

  Scenario: a malformed payload yields no message so the binary can fail open
    Given a user-prompt event JSON that carries no prompt
    When the route-turn user message is extracted
    Then no route-turn user message is extracted

  # resume_aware_routing H2: the conversation/session id is extracted from the
  # harness's user-prompt event JSON so the route-turn hook can carry it as the
  # RouteRequest.conversation_id — letting the engine's resume pre-check bridge a
  # continuation message to the conversation's open playbook.
  Scenario: the conversation id is extracted from a Claude Code UserPromptSubmit session_id
    Given a Claude Code UserPromptSubmit event JSON with session_id "sess-abc-123" and prompt "go"
    When the route-turn conversation id is extracted
    Then the extracted conversation id is "sess-abc-123"

  # Finding A (adversarial review): the route record hashes the EXTRACTED conversation id, while the
  # rendered begin call trims it — so unless extraction normalizes at the source, routed and begun
  # hash different strings (" sess " vs "sess"). This asserts extraction trims; that the rendered
  # call then carries that conversation id is covered by the conversation_id scenarios above.
  Scenario: a padded session id is normalized at extraction so routed and begun hash the same string
    Given a Claude Code UserPromptSubmit event JSON with session_id "  sess-pad  " and prompt "go"
    When the route-turn conversation id is extracted
    Then the extracted conversation id is "sess-pad"

  Scenario: a payload with no session id yields no conversation id so resume degrades gracefully
    Given a user-prompt event JSON that carries no session id
    When the route-turn conversation id is extracted
    Then no route-turn conversation id is extracted

  # LLM ROUTER (routing-precision lever): the LLM call itself is I/O, but the
  # Pick / Abstain / Fallback mapping is pure and proven here.

  Scenario: a single outcome passes through the hybrid narrowing untouched (no LLM)
    Given a route-turn outcome single with kind "daily_recap"
    When the route-turn outcome is narrowed with router verdict pick "weekly_recap"
    Then the narrowed outcome is single with kind "daily_recap"

  Scenario: a no-match outcome passes through the hybrid narrowing untouched (no LLM)
    Given a route-turn outcome no_match
    When the route-turn outcome is narrowed with router verdict abstain
    Then the narrowed outcome is no_match

  Scenario: the local router picks one candidate, collapsing the shortlist to a single
    Given a route-turn outcome candidates with kinds and purposes "daily_recap|Summarize the day's progress.;weekly_recap|Summarize the week's progress."
    When the route-turn outcome is narrowed with router verdict pick "weekly_recap"
    Then the narrowed outcome is single with kind "weekly_recap"

  Scenario: the local router pick carries its reason into the narrowed single
    Given a route-turn outcome candidates with kinds and purposes "daily_recap|Summarize the day's progress.;track|Scope and measure a concrete implementation."
    When the route-turn outcome is narrowed with router verdict pick "track" why "this is a concrete feature implementation, so the track lifecycle will scope+measure it"
    When the narrowed route-turn guidance is formatted
    Then the route-turn guidance contains "track"
    And the route-turn guidance contains "this is a concrete feature implementation, so the track lifecycle will scope+measure it"

  Scenario: the local router abstains, silencing the shortlist (the precision win)
    Given a route-turn outcome candidates with kinds and purposes "daily_recap|Summarize the day's progress.;weekly_recap|Summarize the week's progress."
    When the route-turn outcome is narrowed with router verdict abstain
    Then the narrowed outcome is no_match

  Scenario: a router pick outside the shortlist stays silent
    Given a route-turn outcome candidates with kinds and purposes "daily_recap|Summarize the day's progress.;weekly_recap|Summarize the week's progress."
    When the route-turn outcome is narrowed with router verdict pick "not_a_candidate"
    Then the narrowed outcome is no_match

  Scenario: an unreachable or unparseable router stays silent
    Given a route-turn outcome candidates with kinds and purposes "daily_recap|Summarize the day's progress.;weekly_recap|Summarize the week's progress."
    When the route-turn outcome is narrowed with router verdict fallback
    Then the narrowed outcome is no_match

  Scenario: the router prompt lists each candidate and demands abstain-or-JSON
    Given a route-turn outcome candidates with kinds and purposes "daily_recap|Summarize the day's progress.;weekly_recap|Summarize the week's progress."
    When the router prompt is built from the candidates
    Then the router prompt contains "daily_recap: Summarize the day's progress"
    And the router prompt contains "weekly_recap: Summarize the week's progress"
    And the router prompt contains "abstain"
    And the router prompt contains "{\"kind\":\"<kind-or-abstain>\",\"why\":\"<one short sentence: why THIS playbook fits THIS turn and what it'll do for the work>\"}"

  Scenario: the router prompt renders clean descriptions without trigger bloat
    Given a route-turn outcome candidates with kinds purposes and route triggers "track|Concrete implementation of a proposal slice.|implement a feature,add support for,start a track;daily_recap|Summarize the day's progress.|"
    When the router prompt is built from the candidates
    Then the router prompt contains "track: Concrete implementation of a proposal slice."
    And the router prompt contains "daily_recap: Summarize the day's progress."

  Scenario: the router prompt preserves route descriptions beyond the first sentence
    Given a route-turn outcome candidates with kinds purposes and route triggers "track|Route here when the user asks to IMPLEMENT a feature or bug fix. NOT for authoring a new playbook definition.|implement a feature;playbook_generation|Route here when the user wants to AUTHOR a brand-new PLAYBOOK DEFINITION itself. NOT for implementing a feature.|author a playbook"
    When the router prompt is built from the candidates
    Then the router prompt contains "track: Route here when the user asks to IMPLEMENT a feature or bug fix. NOT for authoring a new playbook definition."
    And the router prompt contains "playbook_generation: Route here when the user wants to AUTHOR a brand-new PLAYBOOK DEFINITION itself. NOT for implementing a feature."

  Scenario: the router reply JSON is parsed into the chosen kind
    Given a local router reply "{\"kind\":\"weekly_recap\"}"
    When the router reply is parsed
    Then the parsed router kind is "weekly_recap"

  Scenario: the router reply JSON is parsed into the chosen kind and reason
    Given a local router reply "{\"kind\":\"track\",\"why\":\"this is a concrete feature implementation, so the track lifecycle will scope+measure it\"}"
    When the router decision reply is parsed
    Then the parsed router kind is "track"
    And the parsed router why is "this is a concrete feature implementation, so the track lifecycle will scope+measure it"

  Scenario: a router reply with surrounding noise still parses the chosen kind
    Given a local router reply "sure, here you go: {\"kind\":\"daily_recap\"} done"
    When the router reply is parsed
    Then the parsed router kind is "daily_recap"

  Scenario: an unparseable router reply yields no kind so the caller can abstain
    Given a local router reply "I cannot answer that"
    When the router reply is parsed
    Then no router kind is parsed

  # resume_aware_routing Phase 3 — resume outcome render
  Scenario: a resume outcome points the agent back at the in-progress step with the advance action
    Given a route-turn outcome resume for kind "track" state "spec" artifact "20260623T0001_alpha" guidance "Write the spec for the slice." advance "complete → spec_review (role: reviewer)"
    When the route-turn guidance is formatted
    Then the route-turn guidance starts with "▶ Resume track (spec) — playbook 20260623T0001_alpha is in progress; continue via complete → spec_review (role: reviewer)."
    And the route-turn guidance contains "Resume track (spec)"
    And the route-turn guidance contains "20260623T0001_alpha"
    And the route-turn guidance contains "continue via complete → spec_review (role: reviewer)"
    And the route-turn guidance contains "Write the spec for the slice."
    And the route-turn guidance does not contain "Anvil matched several workflows"
    And the route-turn guidance does not contain "anvil_orchestrate(selection:"

  Scenario: a resume outcome with no guidance still names the playbook and advance action
    Given a route-turn outcome resume for kind "daily_recap" state "active" artifact "20260623T0002_beta" guidance "" advance "complete → completed (role: doer)"
    When the route-turn guidance is formatted
    Then the route-turn guidance contains "Resume daily_recap (active)"
    And the route-turn guidance contains "continue via complete → completed (role: doer)"

  # context_aware_routing: the router prompt weaves in recent conversation context
  # so a SHORT continuation ("go") is read against the prior intent instead of in
  # isolation — the recall case the flood-fix left conservative.
  Scenario: the router prompt carries recent context so a short continuation is not swallowed
    Given a route-turn outcome candidates with kinds and purposes "track|Concrete implementation of a proposal slice."
    When the context-aware router prompt is built with recent context "user: implement the caching layer\nassistant: I'll begin the track for that." current turn "go" and in-progress "none"
    Then the router prompt contains "Recent conversation context"
    And the router prompt contains "user: implement the caching layer"
    And the router prompt contains "Current turn: go"
    And the router prompt contains "Do NOT abstain merely because THIS turn is short"
    And the router prompt contains "track: Concrete implementation of a proposal slice."

  # context_aware_routing: an OPEN playbook → prefer continue/check-in of it and
  # abstain from routing a NEW playbook (defer to the engine's resume / check-in).
  Scenario: the router prompt defers to an open playbook rather than routing a new one
    Given a route-turn outcome candidates with kinds and purposes "track|Concrete implementation.;proposal|Draft a proposal."
    When the context-aware router prompt is built with recent context "user: keep going" current turn "add logging too" and in-progress "open:track"
    Then the router prompt contains "already IN PROGRESS"
    And the router prompt contains "\"track\" playbook is already IN PROGRESS"
    And the router prompt contains "abstain rather than routing to a NEW playbook"

  # context_aware_routing: playbook-shaped work happening with NO begin → push
  # toward a PICK so the work gets properly begun and measured.
  Scenario: the router prompt pushes work-without-a-begin toward a pick
    Given a route-turn outcome candidates with kinds and purposes "track|Concrete implementation of a proposal slice."
    When the context-aware router prompt is built with recent context "user: let's keep editing the spec" current turn "now write the plan" and in-progress "work-without-begin"
    Then the router prompt contains "WITHOUT a begin"
    And the router prompt contains "Prefer PICKING the best-fit playbook"

  # context_aware_routing: with no context and no in-progress signal the prompt is
  # the original no-context router prompt (additive — no behavior change).
  Scenario: the router prompt without context or signal stays the plain router prompt
    Given a route-turn outcome candidates with kinds and purposes "track|Concrete implementation of a proposal slice."
    When the context-aware router prompt is built with recent context "" current turn "" and in-progress "none"
    Then the router prompt contains "track: Concrete implementation of a proposal slice."
    And the router prompt contains "abstain"
    And the router prompt does not contain "Recent conversation context"
    And the router prompt does not contain "already IN PROGRESS"
    And the router prompt does not contain "WITHOUT a begin"

  # context_aware_routing: the transcript tail is distilled into a recent-context
  # digest AND an in-progress signal. An anvil begin with no later complete ⇒ an
  # OPEN playbook carrying its kind.
  Scenario: a transcript tail with an open begin yields the recent digest and an open-playbook signal
    Given a transcript tail:
      """
      {"type":"user","message":{"role":"user","content":"implement the caching layer"}}
      {"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Starting the track."},{"type":"tool_use","name":"mcp__anvil__begin","input":{"artifact_type":"track"}}]}}
      {"type":"user","message":{"role":"user","content":"go"}}
      """
    When the transcript context is extracted
    Then the recent context contains "user: implement the caching layer"
    And the recent context contains "assistant: Starting the track."
    And the recent context contains "user: go"
    And the in-progress signal is open playbook kind "track"

  # context_aware_routing: a begin FOLLOWED BY a complete is not open — the
  # playbook was properly begun and closed, so no in-progress signal fires.
  Scenario: a transcript tail with a begin then complete yields no open-playbook signal
    Given a transcript tail:
      """
      {"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"mcp__anvil__begin","input":{"artifact_type":"track"}}]}}
      {"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"mcp__anvil__complete","input":{}}]}}
      {"type":"user","message":{"role":"user","content":"what's next?"}}
      """
    When the transcript context is extracted
    Then the in-progress signal is none

  # context_aware_routing: playbook-shaped activity (a forge lifecycle skill) with
  # NO begin anywhere ⇒ work-happening-without-a-begin.
  Scenario: a transcript tail with forge-skill activity but no begin yields work-without-begin
    Given a transcript tail:
      """
      {"type":"user","message":{"role":"user","content":"write the spec for the auth refactor"}}
      {"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"Skill","input":{"skill":"forge:spec"}}]}}
      {"type":"user","message":{"role":"user","content":"looks good, continue"}}
      """
    When the transcript context is extracted
    Then the recent context contains "user: write the spec for the auth refactor"
    And the in-progress signal is work without begin

  # context_aware_routing: an anvil begin issued via the Bash CLI is detected and
  # its kind parsed from --artifact-type.
  Scenario: a transcript tail with a Bash anvil-hooks begin yields an open-playbook signal with the parsed kind
    Given a transcript tail:
      """
      {"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"Bash","input":{"command":"anvil-hooks begin --artifact-type proposal --name foo"}}]}}
      {"type":"user","message":{"role":"user","content":"continue"}}
      """
    When the transcript context is extracted
    Then the in-progress signal is open playbook kind "proposal"

  # context_aware_routing: an ordinary conversation with no lifecycle activity
  # yields a digest but NO in-progress signal (routing unchanged).
  Scenario: a transcript tail with no lifecycle activity yields no in-progress signal
    Given a transcript tail:
      """
      {"type":"user","message":{"role":"user","content":"what does this function do?"}}
      {"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"It parses the config."}]}}
      """
    When the transcript context is extracted
    Then the recent context contains "user: what does this function do?"
    And the in-progress signal is none

  # resume-signal context-awareness: the PARK HINT the engine surfaces (once) for
  # an open playbook the agent has moved on from is rendered into the injected
  # guidance so the affordance reaches the user — naming the dangling artifact,
  # its state, and the machine-declared park action.
  Scenario: a park hint renders the dangling artifact and its park action
    When the route-turn park hint is formatted for artifact "20260601T0001_alpha" kind "track" state "spec" park action "snapshot → abandoned (role: doer)"
    Then the route-turn guidance contains "20260601T0001_alpha"
    And the route-turn guidance contains "track"
    And the route-turn guidance contains "PARK"
    And the route-turn guidance contains "snapshot → abandoned (role: doer)"

  # An empty artifact id has nothing to surface → the render is silent (fail-open).
  Scenario: a park hint with an empty artifact id renders nothing
    When the route-turn park hint is formatted for artifact "" kind "track" state "spec" park action "snapshot → abandoned (role: doer)"
    Then the route-turn guidance is empty

  # The delivered kind: what the hook actually put in front of the model, which
  # the delivery row records so a begin can be attributed to the suggestion that
  # caused it. The kind is in scope at the hook and was discarded until now.
  Scenario: a single-resolution outcome yields its kind as the guidance kind
    Given a route-turn outcome single with kind "daily_recap" purpose "Summarize the day's progress into a recap." required fields "date,highlights"
    When the guidance kind is projected
    Then the guidance kind is "daily_recap"

  # A resume delivers the OPEN playbook's kind; reporting none would make every
  # resume delivery unattributable.
  Scenario: a resume outcome yields the open playbook's kind as the guidance kind
    Given a route-turn outcome resume for kind "track" state "spec" artifact "20260601T0001_alpha" guidance "write the spec" advance "complete → spec_review (role: reviewer)"
    When the guidance kind is projected
    Then the guidance kind is "track"

  # Several candidates means nothing was delivered as THE kind; picking the first
  # would fabricate a single delivery that never happened.
  Scenario: a candidates outcome yields no guidance kind
    Given a route-turn outcome candidates with kinds "track,proposal"
    When the guidance kind is projected
    Then the guidance kind is empty

  Scenario: a no-match outcome yields no guidance kind
    Given a route-turn outcome no_match
    When the guidance kind is projected
    Then the guidance kind is empty
