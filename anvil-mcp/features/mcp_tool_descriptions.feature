Feature: MCP tool descriptions carry the new identity contract
  Per spec R5 of the checkin_backfill_spec_context track, the emitted
  tool descriptions for `checkin`, `begin`, and `snapshot` mention
  `actor_name` and do not reference session-cache-mediated identity.
  A strict grep (case-insensitive) on the descriptions forbids the
  keywords "session", "cache", "cached" so a future edit cannot
  silently re-introduce the removed discipline.

  Note: the negative check intentionally spans ALL tool descriptions in
  `tools/list`, not only the three identity-sensitive ones. A benign
  future description for `catalog` or `describe` that happens to use
  one of the forbidden keywords will fail this test — either reword
  the description or parametrize this check per-tool if the wider
  scope becomes a burden.

  Scenario: Tool descriptions mention actor_name
    Given the MCP shim is started
    And the MCP session is initialized
    When a tools/list request is sent
    Then the "checkin" tool description contains "actor_name"
    And the "begin" tool description contains "actor_name"
    And the "snapshot" tool description contains "actor_name"

  Scenario: Tool descriptions contain no session-cache language
    Given the MCP shim is started
    And the MCP session is initialized
    When a tools/list request is sent
    Then no tool description contains "session" (case-insensitive)
    And no tool description contains "cache" (case-insensitive)
    And no tool description contains "cached" (case-insensitive)
