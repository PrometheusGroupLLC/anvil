Feature: Engine HTTP WebSocket + health bridge multiplexed on the gRPC port
  The anvil-kit React frontend talks to the engine over a loopback WebSocket
  using JSON-RPC 2.0, and Foundry health-probes the engine over HTTP. Both must
  be served on the SAME fixed port as the existing tonic gRPC service: gRPC is
  HTTP/2 (h2c, prior-knowledge) while /ws and /health are HTTP/1.1, and one
  TcpListener accepts both. The WS `playbook_activity` method MUST return the
  identical owner-grouped, call-counted data the gRPC `playbook_activity` RPC
  returns (same core query path), and the JSON-RPC error envelope MUST carry a
  STRING `error.data.code` — the UI keys on that string.

  Scenario: GET /health returns 200 OK on the engine port
    Given a playbook activity engine hearth with playbooks:
      | kind       | owner    | description       |
      | lore_query | lore-kit | Answer a question |
    And the engine is started with that hearth
    When an HTTP GET /health is sent to the engine port
    Then the /health response status is 200

  Scenario: playbook_activity over /ws returns owner-grouped entries with call counts
    Given a playbook activity engine hearth with playbooks:
      | kind         | owner    | description     |
      | activity_one | lore-kit | First activity  |
      | activity_two | lore-kit | Second activity |
    And the engine is started with that hearth
    When the route RPC is called with message "activity_one", signal "", conversation_id "ws-turn-1", ctx org "Foundation" role "read" clearance "internal"
    And the route RPC is called with message "activity_one", signal "", conversation_id "ws-turn-2", ctx org "Foundation" role "read" clearance "internal"
    And the route RPC is called with message "activity_two", signal "", conversation_id "ws-turn-3", ctx org "Foundation" role "read" clearance "internal"
    And a playbook_activity JSON-RPC request is sent over /ws with hearth_path ""
    Then the /ws JSON-RPC result groups owner "lore-kit" with kinds "activity_one,activity_two"
    And the /ws JSON-RPC result entry for kind "activity_one" has call count 2
    And the /ws JSON-RPC result entry for kind "activity_two" has call count 1
    And the /ws JSON-RPC result call_count for kind "activity_one" is a JSON number

  Scenario: an unknown method over /ws returns a JSON-RPC error with a string error.data.code
    Given a playbook activity engine hearth with playbooks:
      | kind       | owner    | description       |
      | lore_query | lore-kit | Answer a question |
    And the engine is started with that hearth
    When a JSON-RPC request for method "no_such_method" is sent over /ws
    Then the /ws JSON-RPC response is an error envelope
    And the /ws JSON-RPC error.data.code is a non-empty string

  Scenario: gRPC still works on the same port after the HTTP bridge is added
    Given a playbook activity engine hearth with playbooks:
      | kind       | owner    | description       |
      | lore_query | lore-kit | Answer a question |
    And the engine is started with that hearth
    Then a gRPC HealthCheck on the engine port succeeds
    And the PlaybookActivity RPC is called
    And the playbook activity RPC groups owner "lore-kit" with kinds "lore_query"
