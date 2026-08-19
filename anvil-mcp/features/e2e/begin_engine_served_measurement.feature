Feature: MCP begin uses engine-served measurement over a shared daemon
  A shim may run from a project whose local hearth differs from the daemon's
  hearth. Begin handoff metadata must come from BeginResponse, not by reading
  local machine.yaml.

  Scenario: Local missing playbook does not override daemon begin measurement
    Given a request hearth without knowledge_lifecycle and a global playbooks hearth with measured knowledge_lifecycle intent "DAEMON INTENT" expected_output "DAEMON OUTPUT" body "DAEMON BODY"
    And the hearth-less engine is started with the global playbooks hearth
    And a .hearth file pointing to that directory while the running engine remains active
    And the MCP shim is started in that working directory with the running engine endpoint
    And the MCP session is initialized while the running engine remains active
    And a checkin tools/call is sent with:
      | field          | value    |
      | role           | creator  |
      | actor_type     | agent    |
      | actor_model    | test     |
      | actor_provider | test     |
    When a begin tools/call is sent with:
      | field          | value                |
      | artifact_type  | knowledge_lifecycle  |
      | track_name     | daemon correctness   |
      | actor_name     | Shim-B2-000001       |
      | actor_type     | agent                |
      | actor_model    | test-model           |
      | actor_provider | test                 |
    Then the begin response has state "ingesting"
    And the begin response field "playbook_id" is exactly "20260529T0409_knowledge_lifecycle"
    And the begin response field "intent" is exactly "DAEMON INTENT"
    And the begin response field "expected_output" is exactly "DAEMON OUTPUT"
