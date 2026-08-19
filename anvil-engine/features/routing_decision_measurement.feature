Feature: Route RPC emits routing_decision records
  Every route(input) call emits one flat event_kind="routing_decision" record
  on the same tracing sink as step_measurement. The route-time half carries the
  granted candidate set, the deterministic resolver outcome, and the selected
  kind only when the route auto-resolves to a single playbook. Confidence is
  present but empty on route-mode records because this rule is deterministic,
  not scored.

  Scenario: default-off Route RPC response and routing record retain the main-branch golden shape
    Given a route hearth seeded with consulting recap fixtures plus seed playbooks
    And the engine is started with that hearth
    When the route RPC is called with message "daily recap", signal "", conversation_id "turn-control-golden", ctx org "Foundation" role "read" clearance "public"
    Then the serialized route RPC response matches the main-branch no-match golden
    And the engine stderr event_kind "routing_decision" log record has no "route_variant" field
    And the engine stderr event_kind "routing_decision" log record has no "brief_cap" field

  Scenario: single-resolution route emits selected playbook and resolver outcome
    Given a route hearth seeded with consulting recap fixtures plus seed playbooks
    And the engine is started with that hearth
    When the route RPC is called with message "daily recap", signal "", conversation_id "turn-route-single", ctx org "Consulting" role "read" clearance "internal"
    Then the route outcome is "candidates"
    And the route resolution outcome is "single"
    And the route selected kind is "daily_recap"
    And the engine stderr contains exactly 1 JSON log records with event_kind "routing_decision"
    And the engine stderr contains a JSON log record with fields:
      | event_kind         | routing_decision                       |
      | phase              | route                                  |
      | turn_id            | turn-route-single                      |
      | input              | daily recap                            |
      | candidate_set      | daily_recap,playbook,track,weekly_recap |
      | selected           | daily_recap                            |
      | resolution_outcome | single                                 |
      | confidence         |                                        |
      | at                 | <non-empty>                            |

  Scenario: candidates route emits empty selected playbook and resolver outcome
    Given a route hearth seeded with consulting recap fixtures plus seed playbooks
    And the engine is started with that hearth
    When the route RPC is called with message "recap", signal "", conversation_id "turn-route-candidates", ctx org "Consulting" role "read" clearance "internal"
    Then the route outcome is "candidates"
    And the route resolution outcome is "candidates"
    And no route selected kind is set
    And the engine stderr contains exactly 1 JSON log records with event_kind "routing_decision"
    And the engine stderr contains a JSON log record with fields:
      | event_kind         | routing_decision                       |
      | phase              | route                                  |
      | turn_id            | turn-route-candidates                  |
      | input              | recap                                  |
      | candidate_set      | daily_recap,playbook,track,weekly_recap |
      | selected           |                                        |
      | resolution_outcome | candidates                             |
      | confidence         |                                        |
      | at                 | <non-empty>                            |

  Scenario: no_match route emits empty candidate set and resolver outcome
    Given a route hearth seeded with consulting recap fixtures plus seed playbooks
    And the engine is started with that hearth
    When the route RPC is called with message "daily recap", signal "", conversation_id "turn-route-nomatch", ctx org "Foundation" role "read" clearance "public"
    Then the route outcome is "no_match"
    And the route resolution outcome is "no_match"
    And no route selected kind is set
    And the engine stderr contains exactly 1 JSON log records with event_kind "routing_decision"
    And the engine stderr contains a JSON log record with fields:
      | event_kind         | routing_decision  |
      | phase              | route             |
      | turn_id            | turn-route-nomatch |
      | input              | daily recap        |
      | candidate_set      |                    |
      | selected           |                    |
      | resolution_outcome | no_match           |
      | confidence         |                    |
      | at                 | <non-empty>        |

  Scenario: routed begin rejects a free selection in the engine begin path
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth also has a free "spark" machine
    And the engine is started with that hearth
    When the routed begin RPC is called to create a "spark" artifact named "free spark" with selected "spark" and turn_id "turn-engine-free-selection"
    Then the begin RPC returns gRPC status "FAILED_PRECONDITION"
    And the begin RPC error message contains "not a driven candidate"
    And the engine stderr contains exactly 0 JSON log records with event_kind "routing_decision"

  Scenario: routed begin rejects a selected kind that differs from the begun artifact type
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth also has a free "spark" machine
    And the engine is started with that hearth
    When the routed begin RPC is called to create a "spark" artifact named "free spark" with selected "knowledge_lifecycle" and turn_id "turn-engine-mismatch"
    Then the begin RPC returns gRPC status "FAILED_PRECONDITION"
    And the begin RPC error message contains "routed selection mismatch"
    And the hearth contains no artifact directories under "sparks"
    And the engine stderr contains exactly 0 JSON log records with event_kind "routing_decision"

  Scenario: routed begin emits one selection half and no route half
    Given a hearth seeded with the knowledge_lifecycle machine
    And the engine is started with that hearth
    When the routed begin RPC is called to create a "knowledge_lifecycle" artifact named "research note" with selected "knowledge_lifecycle" and turn_id "turn-engine-selection-only"
    Then the begin RPC response state is "ingesting"
    And the engine stderr contains exactly 1 JSON log records with event_kind "routing_decision"
    And the engine stderr contains a JSON log record with fields:
      | event_kind    | routing_decision           |
      | phase         | begin                      |
      | turn_id       | turn-engine-selection-only |
      | input         | research note triage       |
      | candidate_set | knowledge_lifecycle        |
      | selected      | knowledge_lifecycle        |
      | confidence    | 0.88                       |
      | at            | <non-empty>                |

  Scenario: direct begin is not driven-guarded and emits no routing_decision
    Given a hearth seeded with the knowledge_lifecycle machine
    And the engine is started with that hearth
    When the begin RPC is called to create a "knowledge_lifecycle" artifact named "direct research" with no parent
    Then the begin RPC response state is "ingesting"
    And the engine stderr contains exactly 0 JSON log records with event_kind "routing_decision"
