Feature: MCP Tools List
  The MCP shim advertises available tools via the tools/list method.

  Scenario: Tools list includes all five tools
    Given the MCP shim is started
    And the MCP session is initialized
    When a tools/list request is sent
    Then the response contains a tool named "catalog"
    And the response contains a tool named "checkin"
    And the response contains a tool named "describe"
    And the response contains a tool named "begin"
    And the response contains a tool named "snapshot"
    And the response contains a tool named "persist_playbook"
    And the response contains a tool named "candidate_playbook_intake"
    And the response does not contain a tool named "persist_workflow"
    And the response does not contain a tool named "candidate_workflow_intake"
    And the catalog tool has a description
    And the catalog tool has an input schema

  Scenario: Hearth-routing fields are optional on hearth-backed tools
    Given the MCP shim is started
    And the MCP session is initialized
    When a tools/list request is sent
    Then the "catalog" tool has schema property "hearth"
    And the "catalog" tool has schema property "project"
    And the "checkin" tool has schema property "hearth"
    And the "checkin" tool has schema property "project"
    And the "describe" tool has schema property "hearth"
    And the "describe" tool has schema property "project"
    And the "begin" tool has schema property "hearth"
    And the "begin" tool has schema property "project"
    And the "begin" tool has schema property "conversation_id"
    And the "begin" tool has schema property "surface"
    And the "anvil_orchestrate" tool has schema property "hearth"
    And the "anvil_orchestrate" tool has schema property "project"
    # Creation-mode builtin fields must be discoverable top-level (not guessed into `fields`)
    And the "anvil_orchestrate" tool has schema property "playbook_name"
    And the "anvil_orchestrate" tool does not have schema property "workflow_name"
    And the "anvil_orchestrate" tool has schema property "target_owner"
    And the "anvil_orchestrate" tool has schema property "fields"
    And the "snapshot" tool has schema property "hearth"
    And the "snapshot" tool has schema property "project"
    And the "complete" tool has schema property "hearth"
    And the "complete" tool has schema property "project"
    And the "amend" tool has schema property "hearth"
    And the "amend" tool has schema property "project"
    And the "begin" tool has schema property "playbook_name"
    And the "begin" tool does not have schema property "workflow_name"
