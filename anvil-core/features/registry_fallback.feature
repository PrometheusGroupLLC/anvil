Feature: Registry Fallback for Summaries
  When an artifact's primary document is missing, the hearth reader
  derives the summary from the corresponding registry file entry.

  Scenario: Summary derived from registry when primary document unavailable
    Given a hearth directory with artifacts and a registry:
      | artifact_path                                     | state  |
      | proposals/20260403T1500_forge_lifecycle/           | active |
      | tracks/20260411T2345_mcp_server_foundation/        | implementing |
    And the proposals registry contains:
      """
      # Proposals

      ## active

      - [Forge Lifecycle](proposals/20260403T1500_forge_lifecycle/) — event-sourced development playbook
      """
    And the tracks registry contains:
      """
      # Tracks

      ## implementing

      - [MCP Server Foundation](tracks/20260411T2345_mcp_server_foundation/) — MCP server foundation
      """
    When the filesystem hearth reader lists artifacts
    Then the artifact "20260403T1500_forge_lifecycle" has summary "event-sourced development playbook"
    And the artifact "20260411T2345_mcp_server_foundation" has summary "MCP server foundation"
