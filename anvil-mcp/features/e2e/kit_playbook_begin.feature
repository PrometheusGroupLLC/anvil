Feature: Kit-bundled playbook drives begin end-to-end
  When the kit-bundled track_lifecycle playbook is copied into the hearth,
  a creator can checkin and begin a new track artifact through the MCP shim.
  This proves AC 5(b): the playbook shipped in dist/anvil-kit/ is wired
  end-to-end — its presence in the hearth is what enables artifact creation.

  Scenario: Begin creates a track instance from the kit-bundled playbook
    Given a hearth directory with the following structure:
      | path                                                | state  |
      | proposals/20260503T1200_kit_begin_test/             | active |
    And the kit-bundled playbook is copied into the hearth tmpdir
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
      | field         | value                              |
      | artifact_type | track                              |
      | parent_id     | 20260503T1200_kit_begin_test       |
      | track_name    | kit-bundled track creation         |
      | approver      | nick                               |
    Then the begin response has state "spec"
    And the begin response has a track path
    And the begin response has context text containing "Spec Writing Context"
    And the hearth has a new track directory with status.yaml
