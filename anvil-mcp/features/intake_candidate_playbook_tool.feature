Feature: MCP candidate_playbook_intake tool
  The MCP shim advertises the CandidatePlaybook intake surface.

  Scenario: tools list advertises candidate intake schema
    Given the MCP shim is started
    And the MCP session is initialized
    When a tools/list request is sent
    Then the response contains a tool named "candidate_playbook_intake"
    And the response does not contain a tool named "candidate_workflow_intake"
    And the anvil_orchestrate no_match handoff tool is advertised
    And the "candidate_playbook_intake" tool requires field "target_owner"
    And the "candidate_playbook_intake" tool has schema property "proposed_states"
    And the "candidate_playbook_intake" tool has schema property "intent"
