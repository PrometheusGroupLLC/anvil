Feature: MCP shim verifies engine wire compatibility before Begin
  A shim may dial a shared engine endpoint, but it must not use that endpoint
  after HealthCheck reports a wire protocol version different from the shim's
  expected version. It must return a stable version-mismatch code instead of
  spawning a competing engine on shared hearths.

  Background:
    Given a hearth directory with the following structure:
      | path                                              | state  |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
    And a playbook hook body for the spec doer hook "spec-writing.md" with content "Wire handshake spec writing."
    And the engine is started with that hearth
    And a .hearth file pointing to that directory while the running engine remains active

  Scenario: Matching wire version reuses the running engine for Begin
    Given the MCP shim is started in that working directory with the running engine endpoint
    And the MCP session is initialized while the running engine remains active
    When a checkin tools/call is sent with role "creator" and:
      | field          | value          |
      | actor_name     | WireActor-0001 |
      | actor_type     | agent          |
      | actor_model    | test-model     |
      | actor_provider | test           |
    When a begin tools/call is sent with:
      | field          | value                              |
      | artifact_type  | track                              |
      | parent_id      | 20260411T2021_anvil_workflow_engine |
      | track_name     | wire-compatible-track              |
      | approver       | mark                               |
      | actor_name     | WireActor-0001                     |
      | actor_type     | agent                              |
      | actor_model    | test-model                         |
      | actor_provider | test                               |
    Then the begin response has state "spec"
    And the MCP shim spawned no engine of its own

  Scenario: Mismatched wire version returns the stable code
    Given the MCP shim is started in that working directory with the running engine endpoint and expected wire version "999"
    And the MCP session is initialized while the running engine remains active
    When a checkin tools/call is sent with role "creator" and:
      | field          | value          |
      | actor_name     | WireActor-0002 |
      | actor_type     | agent          |
      | actor_model    | test-model     |
      | actor_provider | test           |
    Then the MCP response is a tool error containing "client_engine_proto_version_mismatch"
    And the MCP response error message contains "wire v3"
    And the MCP response error message contains "(build"
    And the MCP response error message contains "expects wire v999"
    And the MCP response error message names the running engine port
    And the MCP response error message does not contain "invalid wire type"
    And the MCP shim spawned no engine of its own
