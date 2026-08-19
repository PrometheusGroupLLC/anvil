Feature: Terminal State Filtering
  The catalog excludes artifacts in terminal states from active listings.
  Terminal states are: completed, superseded, abandoned, retired.

  Scenario: Only non-terminal artifacts are returned
    Given a hearth with the following artifacts:
      | id                          | type     | state       | summary                        |
      | 20260403T1500_forge         | proposal | active      | Forge lifecycle                |
      | 20260404T2116_initiatives   | proposal | completed   | Forge initiatives              |
      | 20260411T2345_mcp_server    | track    | implementing| MCP server foundation          |
      | 20260406T1315_snapshot      | track    | completed   | Snapshot service core           |
      | 20260409T0559_edit_eff      | track    | abandoned   | Snapshot edit efficiency        |
      | 20260405T2245_milestone     | milestone| active      | Brine rollout readiness        |
      | follow-forge-lifecycle      | initiative| promoted   | Follow forge lifecycle          |
      | test-driven-development     | initiative| retired    | Test-driven development        |
    When listing active artifacts
    Then 4 artifacts are returned
    And the returned artifacts include "20260403T1500_forge" with type "proposal" and state "active"
    And the returned artifacts include "20260411T2345_mcp_server" with type "track" and state "implementing"
    And the returned artifacts include "20260405T2245_milestone" with type "milestone" and state "active"
    And the returned artifacts include "follow-forge-lifecycle" with type "initiative" and state "promoted"
    And the returned artifacts do not include "20260404T2116_initiatives"
    And the returned artifacts do not include "20260406T1315_snapshot"
    And the returned artifacts do not include "20260409T0559_edit_eff"
    And the returned artifacts do not include "test-driven-development"
