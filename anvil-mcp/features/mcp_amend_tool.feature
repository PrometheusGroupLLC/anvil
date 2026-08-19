Feature: MCP amend tool — shim exposes amend in tools/list with correct schema

  Scenario: amend tool appears in tools/list
    Given a hearth directory with the following structure:
      | path                                          | state |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
      | tracks/20260419T1300_mcp_amend_list/           | spec   |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a tools/list request is sent
    Then the tools list contains "amend"

  Scenario: amend tool inputSchema requires artifact_path
    Given a hearth directory with the following structure:
      | path                                          | state |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a tools/list request is sent
    Then the amend tool inputSchema requires "artifact_path"

  Scenario: amend tool inputSchema requires actor_name
    Given a hearth directory with the following structure:
      | path                                          | state |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a tools/list request is sent
    Then the amend tool inputSchema requires "actor_name"

  Scenario: amend tool inputSchema requires actor_type
    Given a hearth directory with the following structure:
      | path                                          | state |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a tools/list request is sent
    Then the amend tool inputSchema requires "actor_type"

  Scenario: amend tool inputSchema requires actor_model
    Given a hearth directory with the following structure:
      | path                                          | state |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a tools/list request is sent
    Then the amend tool inputSchema requires "actor_model"

  Scenario: amend tool inputSchema requires actor_provider
    Given a hearth directory with the following structure:
      | path                                          | state |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a tools/list request is sent
    Then the amend tool inputSchema requires "actor_provider"

  Scenario: amend tool inputSchema requires kind
    Given a hearth directory with the following structure:
      | path                                          | state |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a tools/list request is sent
    Then the amend tool inputSchema requires "kind"

  Scenario: amend tool inputSchema requires target_document
    Given a hearth directory with the following structure:
      | path                                          | state |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a tools/list request is sent
    Then the amend tool inputSchema requires "target_document"

  Scenario: amend tool inputSchema requires op_kind
    Given a hearth directory with the following structure:
      | path                                          | state |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a tools/list request is sent
    Then the amend tool inputSchema requires "op_kind"

  Scenario: amend tool inputSchema requires target_id
    Given a hearth directory with the following structure:
      | path                                          | state |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a tools/list request is sent
    Then the amend tool inputSchema requires "target_id"
