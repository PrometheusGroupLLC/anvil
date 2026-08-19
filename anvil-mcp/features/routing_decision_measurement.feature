Feature: anvil_orchestrate emits routing_decision selection records
  The MCP handoff lets the surface route first, select later, and compose both
  routing_decision records by the surface-supplied conversation_id.

  Scenario: anvil_orchestrate input schema exposes optional confidence
    Given a hearth directory with the following structure:
      | path                                            | state  |
      | proposals/20260411T2021_anvil_workflow_engine/  | active |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a tools/list request is sent
    Then the "anvil_orchestrate" tool inputSchema has property "confidence"
    And the "anvil_orchestrate" tool inputSchema has property "routing_hint"
    And the "anvil_orchestrate" tool inputSchema has property "selection"
    And the "anvil_orchestrate" tool inputSchema has property "candidate_set"

  Scenario: begin-mode emits selection half joined to route-mode by turn_id
    When a surface routes and then selects "knowledge_lifecycle" for "knowledge lifecycle" with conversation_id "turn-mcp-routing" and confidence "0.82"
    Then the handoff is not an error
    And the routing_decision route-mode record has turn_id "turn-mcp-routing" and candidate "knowledge_lifecycle"
    And the routing_decision begin-mode record has turn_id "turn-mcp-routing" selected "knowledge_lifecycle" confidence "0.82"

  Scenario: begin-mode accepts UTF-8 multiline input while emitting the selection half
    When a surface selects "knowledge_lifecycle" for a UTF-8 multiline message with conversation_id "turn-mcp-utf8" and confidence "0.91"
    Then the handoff is not an error
    And the routing_decision begin-mode record has turn_id "turn-mcp-utf8" selected "knowledge_lifecycle" confidence "0.91"

  Scenario: route then select emits exactly one route half and one selection half
    When a surface routes and then selects "knowledge_lifecycle" for "knowledge lifecycle" with conversation_id "turn-mcp-exactly-once" and confidence "0.77"
    Then the handoff is not an error
    And exactly one routing_decision route-mode record has turn_id "turn-mcp-exactly-once"
    And exactly one routing_decision begin-mode record has turn_id "turn-mcp-exactly-once" selected "knowledge_lifecycle" confidence "0.77"

  Scenario: route-only single advisory emits one route phase and no begin phase
    When a surface routes "Ingest the Q3 research" with routing_hint "knowledge_lifecycle" and conversation_id "turn-mcp-route-only"
    Then the handoff is not an error
    And the handoff is advisory single for kind "knowledge_lifecycle"
    And exactly one routing_decision route-mode record has turn_id "turn-mcp-route-only"
    And no routing_decision begin-mode record has turn_id "turn-mcp-route-only"

  Scenario: route-mode next_call preserves routing_hint for the selected begin
    When a surface routes "Ingest the Q3 research" with routing_hint "knowledge_lifecycle" and conversation_id "turn-mcp-next-hint"
    Then the handoff is not an error
    And the handoff next_call arguments include routing_hint "knowledge_lifecycle"

  Scenario: route-mode forwards conversation_id and surface to durable route records
    When a surface routes "daily recap" into anvil_orchestrate in route-mode with conversation_id "turn-mcp-route-correlation" surface "claude-code" and ctx org "Consulting"
    Then the handoff is not an error
    And the anvil_orchestrate route durable records carry conversation_hash and source "claude-code"

  Scenario: hinted route followed through next_call records the same input in both halves
    When a surface routes with routing_hint "knowledge_lifecycle" and follows next_call selecting "knowledge_lifecycle" for "Ingest the Q3 research" with conversation_id "turn-mcp-follow-hint" and confidence "0.86"
    Then the handoff is not an error
    And the routing_decision route-mode record has turn_id "turn-mcp-follow-hint" and candidate "knowledge_lifecycle"
    And the routing_decision begin-mode record has turn_id "turn-mcp-follow-hint" selected "knowledge_lifecycle" confidence "0.86"
    And the routing_decision route-mode and begin-mode records for turn_id "turn-mcp-follow-hint" both have input "Ingest the Q3 research knowledge_lifecycle"

  Scenario: hinted begin-mode records the same routed input as route-mode
    When a surface routes with routing_hint "knowledge_lifecycle" and then selects "knowledge_lifecycle" for "Ingest the Q3 research" with conversation_id "turn-mcp-hinted-input" and confidence "0.86"
    Then the handoff is not an error
    And the routing_decision route-mode record has turn_id "turn-mcp-hinted-input" and candidate "knowledge_lifecycle"
    And the routing_decision begin-mode record has turn_id "turn-mcp-hinted-input" selected "knowledge_lifecycle" confidence "0.86"
    And the routing_decision route-mode and begin-mode records for turn_id "turn-mcp-hinted-input" both have input "Ingest the Q3 research knowledge_lifecycle"
