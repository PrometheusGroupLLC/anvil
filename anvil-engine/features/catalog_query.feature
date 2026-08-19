Feature: Catalog Query
  The engine serves a catalog query that returns active artifacts
  and available artifact types via gRPC.

  @registration
  Scenario: Catalog returns active artifacts and available types
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260403T1500_forge_lifecycle/           | active       |
      | proposals/20260404T2116_forge_initiatives/         | completed    |
      | tracks/20260411T2345_mcp_server/                   | implementing |
      | milestones/20260404T0700_brine_rollout/            | active       |
      | initiatives/follow-forge-lifecycle/                | promoted     |
      | decisions/event-format-choice/                     | decided      |
    And the engine is started with that hearth
    When the catalog RPC is called
    Then the catalog response contains 5 active artifacts
    And the catalog response includes artifact "20260403T1500_forge_lifecycle" with type "proposal"
    And the catalog response includes artifact "20260411T2345_mcp_server" with type "track"
    And the catalog response includes artifact "20260404T0700_brine_rollout" with type "milestone"
    And the catalog response includes artifact "follow-forge-lifecycle" with type "initiative"
    And the catalog response includes artifact "event-format-choice" with type "decision"
    And the catalog response does not include "20260404T2116_forge_initiatives"
    And the catalog response contains 7 available artifact types
    And the available types include "backlog_item" with no parent required
    And the available types include "proposal" with description containing "Strategic direction"
    And the available types include "track" requiring parent "proposal"
    And the available types include "milestone" with no parent required

  Scenario: Catalog returns error for nonexistent hearth
    Given the engine is started with hearth path "/tmp/anvil-nonexistent-hearth"
    When the catalog RPC is called
    Then the catalog RPC returns a gRPC error
