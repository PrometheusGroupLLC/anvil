Feature: target_owner flows proto -> shim -> engine -> status.yaml end-to-end
  A creator begins a playbook_generation through the MCP shim, supplying
  target_owner in the begin tools/call arguments. The shim maps it onto the proto
  BeginRequest, the engine records it, and the created instance's status.yaml
  under workflow_generations/ carries the owner descriptor. Proves the full
  vertical is wired through the shim (Anvil-lane 1b; A4).

  Scenario: begin a playbook_generation through the shim with target_owner records it
    Given a hearth seeded with the builder machine and an active parent track "20260606T0000_builder_parent_track"
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a checkin tools/call is sent with role "creator" and:
      | field          | value           |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-6 |
      | actor_provider | anthropic       |
    Then the checkin response has a generated actor name
    When a begin tools/call is sent with:
      | field         | value                                |
      | artifact_type | playbook_generation                  |
      | parent_id     | 20260606T0000_builder_parent_track   |
      | track_name    | shim target owner e2e                |
      | playbook_name | temper                               |
      | approver      | nick                                 |
      | target_owner  | kit:test-owner                       |
    Then the hearth status.yaml under "workflow_generations" contains "target_owner: kit:test-owner"

  # Regression: anvil_orchestrate begin-mode (begin_selected) dropped playbook_name
  # (hardcoded empty), so playbook_generation could never be begun through the
  # PRIMARY begin path — the engine rejected it with missing_required_field:
  # playbook_name. The artifact only lands if playbook_name threads through.
  Scenario: begin a playbook_generation through anvil_orchestrate threads playbook_name end-to-end
    Given a hearth seeded with the builder machine and an active parent track "20260606T0000_builder_parent_track"
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When an anvil_orchestrate tools/call is sent with:
      | field         | value                                |
      | selection     | playbook_generation                  |
      | parent_id     | 20260606T0000_builder_parent_track   |
      | track_name    | orchestrate playbook_name e2e        |
      | playbook_name | temper                               |
      | approver      | nick                                 |
      | target_owner  | kit:test-owner                       |
    Then the hearth status.yaml under "workflow_generations" contains "target_owner: kit:test-owner"
