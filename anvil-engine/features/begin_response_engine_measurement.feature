Feature: BeginResponse carries engine-resolved step measurement
  The engine, not the MCP shim, owns playbook source resolution. BeginResponse
  must therefore carry playbook_id, intent, and expected_output from the selected
  playbook source for every successful begin outcome, and malformed selected
  machines must fail closed before falling through to another tier.

  Scenario: Request-tier playbook fields are returned on begin
    Given a request hearth with measured knowledge_lifecycle intent "REQUEST INTENT" expected_output "REQUEST OUTPUT" body "REQUEST BODY" and a global playbooks hearth with measured knowledge_lifecycle intent "GLOBAL INTENT" expected_output "GLOBAL OUTPUT" body "GLOBAL BODY"
    And the hearth-less engine is started with the global playbooks hearth
    When the begin RPC is called for the request hearth to create a "knowledge_lifecycle" artifact named "request measured" with no parent
    Then the begin RPC response state is "ingesting"
    And the begin RPC response "context_text" is exactly "REQUEST BODY"
    And the begin RPC response "playbook_id" is exactly "20260529T0409_knowledge_lifecycle"
    And the begin RPC response "intent" is exactly "REQUEST INTENT"
    And the begin RPC response "expected_output" is exactly "REQUEST OUTPUT"

  Scenario: Global-tier playbook fields are returned when the request hearth lacks the kind
    Given a request hearth without knowledge_lifecycle and a global playbooks hearth with measured knowledge_lifecycle intent "GLOBAL INTENT" expected_output "GLOBAL OUTPUT" body "GLOBAL BODY"
    And the hearth-less engine is started with the global playbooks hearth
    When the begin RPC is called for the request hearth to create a "knowledge_lifecycle" artifact named "global measured" with no parent
    Then the begin RPC response state is "ingesting"
    And the begin RPC response "context_text" is exactly "GLOBAL BODY"
    And the begin RPC response "playbook_id" is exactly "20260529T0409_knowledge_lifecycle"
    And the begin RPC response "intent" is exactly "GLOBAL INTENT"
    And the begin RPC response "expected_output" is exactly "GLOBAL OUTPUT"

  Scenario: Request-tier playbook shadows a same-kind global playbook
    Given a request hearth with measured knowledge_lifecycle intent "REQUEST SHADOW INTENT" expected_output "REQUEST SHADOW OUTPUT" body "REQUEST SHADOW BODY" and a global playbooks hearth with measured knowledge_lifecycle intent "GLOBAL SHADOW INTENT" expected_output "GLOBAL SHADOW OUTPUT" body "GLOBAL SHADOW BODY"
    And the hearth-less engine is started with the global playbooks hearth
    When the begin RPC is called for the request hearth to create a "knowledge_lifecycle" artifact named "shadow measured" with no parent
    Then the begin RPC response state is "ingesting"
    And the begin RPC response "context_text" is exactly "REQUEST SHADOW BODY"
    And the begin RPC response "intent" is exactly "REQUEST SHADOW INTENT"
    And the begin RPC response "expected_output" is exactly "REQUEST SHADOW OUTPUT"

  Scenario: Origin-turn hit still returns playbook fields
    Given a request hearth with measured knowledge_lifecycle intent "REPEAT INTENT" expected_output "REPEAT OUTPUT" body "REPEAT BODY" and a global playbooks hearth with measured knowledge_lifecycle intent "GLOBAL INTENT" expected_output "GLOBAL OUTPUT" body "GLOBAL BODY"
    And the hearth-less engine is started with the global playbooks hearth
    When the routed begin RPC is called twice for the request hearth to create a "knowledge_lifecycle" artifact named "repeat measured" with selected "knowledge_lifecycle" and turn_id "turn-b2-repeat"
    Then the begin RPC response state is "ingesting"
    And the begin RPC response "playbook_id" is exactly "20260529T0409_knowledge_lifecycle"
    And the begin RPC response "intent" is exactly "REPEAT INTENT"
    And the begin RPC response "expected_output" is exactly "REPEAT OUTPUT"

  Scenario: Malformed selected request-tier machine fails closed before global fallback
    Given a request hearth with malformed knowledge_lifecycle and a global playbooks hearth with measured knowledge_lifecycle intent "GLOBAL INTENT" expected_output "GLOBAL OUTPUT" body "GLOBAL BODY"
    And the hearth-less engine is started with the global playbooks hearth
    When the begin RPC is called for the request hearth to create a "knowledge_lifecycle" artifact named "malformed selected" with no parent
    Then the begin RPC returns gRPC status "INVALID_ARGUMENT"
    And the begin RPC error message contains "playbook_yaml_parse_error"

  Scenario: Valid playbook without measurement returns empty response fields
    Given a hearth seeded with the knowledge_lifecycle machine
    And the engine is started with that hearth
    When the begin RPC is called to create a "knowledge_lifecycle" artifact named "honest absence" with no parent
    Then the begin RPC response state is "ingesting"
    And the begin RPC response "intent" is exactly ""
    And the begin RPC response "expected_output" is exactly ""
