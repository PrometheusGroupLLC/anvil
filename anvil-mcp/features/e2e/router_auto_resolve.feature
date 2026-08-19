Feature: anvil_orchestrate route-mode is advisory for a single match

  route_response_mirrors_begin H4 (human decision: ADVISORY ONLY, uniform): a
  natural surface turn can resolve to a single playbook, but route-mode NO LONGER
  auto-begins it. A single resolution returns the begin-equivalent guidance + the
  begin call (next_call) + required_fields, and performs NO begin / NO state
  transition — uniform with the passive route-turn hook ("LLM selects, engine
  executes"). The model still calls anvil_orchestrate(selection: ...) to start it.
  Route-mode still returns candidates when the request is ambiguous or access
  filtering removes the matching playbook.

  Scenario: a consulting daily recap request returns advisory guidance, not a begin
    When a surface ships "daily recap" into anvil_orchestrate in route-mode with conversation_id "turn-mcp-advisory" and ctx org "Consulting"
    Then the handoff is not an error
    And the handoff is advisory single for kind "daily_recap"
    And the handoff next_call begins kind "daily_recap"
    And the handoff carries a non-empty route guidance
    And no daily_recap instance was created by the route call
    And exactly one routing_decision route-mode record has turn_id "turn-mcp-advisory" selected "daily_recap" resolution_outcome "single"
    And no routing_decision begin-mode record has turn_id "turn-mcp-advisory"

  Scenario: a directory-less lore query returns advisory guidance, not a begin
    When a surface ships "ask lore about plans" into anvil_orchestrate in route-mode with conversation_id "turn-mcp-advisory-lore" and ctx org "Foundation"
    Then the handoff is not an error
    And the handoff is advisory single for kind "lore_query"
    And the handoff next_call begins kind "lore_query"
    And exactly one routing_decision route-mode record has turn_id "turn-mcp-advisory-lore" selected "lore_query" resolution_outcome "single"
    And no routing_decision begin-mode record has turn_id "turn-mcp-advisory-lore"

  Scenario: an ambiguous consulting recap request returns candidates without beginning
    When a surface ships "recap" into anvil_orchestrate in route-mode with ctx org "Consulting"
    Then the handoff is not an error
    And the handoff outcome is "candidates"
    And the handoff candidates include kind "daily_recap"
    And the handoff candidates include kind "weekly_recap"
    And the handoff did not auto-begin

  # router_relevance_ranker: a consulting recap is access-excluded for Foundation, so a
  # "daily recap" request surfaces no consulting recap candidate and never auto-begins
  # one — whether the ranker returns other candidates or abstains. The access-exclusion
  # invariant is what this scenario guards (not a specific outcome label).
  Scenario: Foundation access excludes consulting recap playbooks
    When a surface ships "daily recap" into anvil_orchestrate in route-mode with ctx org "Foundation"
    Then the handoff is not an error
    And the handoff candidates do not include kind "daily_recap"
    And the handoff did not auto-begin

  # route_response_mirrors_begin H3 (MCP route-mode fail-open): a single resolution
  # whose guidance enrichment fails on the engine (declared hook missing) returns a
  # SUCCESSFUL advisory thin route (empty guidance + the begin call), NOT a tool
  # error.
  Scenario: a single route whose guidance enrichment fails returns a successful thin advisory route
    When a surface ships "do the brittle thing" into anvil_orchestrate in route-mode with conversation_id "turn-mcp-failopen" and ctx org "Foundation"
    Then the handoff is not an error
    And the handoff is advisory single for kind "brittle"
    And the handoff next_call begins kind "brittle"
    And the handoff carries an empty route guidance

  # resume_aware_routing C1 (MCP route-mode resume): when the conversation has an
  # open (begun, non-terminal) playbook and the surface ships a continuation token
  # ("go"), route-mode resolves to RESUME and surfaces the open playbook back to
  # the caller — NOT an empty "candidates" response. The handoff names the open
  # artifact (id + kind + state), the current-step guidance, and the supported
  # advance action so the model can continue.
  Scenario: a continuation token with an open playbook resumes it through route-mode
    Given the route-mode fixture has an open "daily_recap" artifact "20260601T0001_recap" in state "gathering" begun for conversation "turn-mcp-resume"
    When a surface ships "go" into anvil_orchestrate in route-mode with conversation_id "turn-mcp-resume" and ctx org "Consulting" resuming the seeded playbook
    Then the handoff is not an error
    And the handoff outcome is "resume"
    And the handoff resumes artifact "20260601T0001_recap" of kind "daily_recap" in state "gathering"
    And the handoff carries a non-empty resume advance action
