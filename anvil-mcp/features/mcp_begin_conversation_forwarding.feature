Feature: MCP begin forwards conversation correlation

  Scenario: begin tool forwards conversation_id to the engine begin activity record
    Given a hearth seeded with the lore_query run-backed machine
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    And a checkin tools/call is sent with role "creator" and:
      | field          | value |
      | actor_type     | agent |
      | actor_model    | test  |
      | actor_provider | test  |
    When a begin tools/call is sent with:
      | field           | value                              |
      | artifact_type   | lore_query                         |
      | track_name      | ask about plans                    |
      | actor_name      | Begin-MCP-000001                   |
      | actor_type      | agent                              |
      | actor_model     | test-model                         |
      | actor_provider  | test                               |
      | conversation_id | surface-session-mcp-begin-activity |
      | surface         | claude-code                        |
    Then the begin response has a track path
    And the activity log command "begin" has a non-empty conversation_hash

  # The adoption join fix: when the caller omits conversation_id, the shim defaults
  # it to CLAUDE_CODE_SESSION_ID (the same id the route hook hashes) so the begin's
  # activity-log record still carries a conversation_hash and joins the route leg.
  Scenario: begin defaults conversation_id from CLAUDE_CODE_SESSION_ID when the call omits it
    Given a hearth seeded with the lore_query run-backed machine
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory with Claude Code session id "sess-env-default-0001"
    And the MCP session is initialized
    And a checkin tools/call is sent with role "creator" and:
      | field          | value |
      | actor_type     | agent |
      | actor_model    | test  |
      | actor_provider | test  |
    When a begin tools/call is sent with:
      | field          | value            |
      | artifact_type  | lore_query       |
      | track_name     | ask about plans  |
      | actor_name     | Begin-MCP-Env-01 |
      | actor_type     | agent            |
      | actor_model    | test-model       |
      | actor_provider | test             |
      | surface        | claude-code      |
    Then the begin response has a track path
    And the activity log command "begin" has a non-empty conversation_hash
