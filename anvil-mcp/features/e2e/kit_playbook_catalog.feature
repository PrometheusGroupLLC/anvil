Feature: Kit-bundled playbook appears in catalog
  When the kit-bundled track_lifecycle playbook is copied into the hearth,
  the engine discovers it and reports "track" as an available artifact type
  in the catalog response.

  Scenario: Kit-bundled playbook appears in catalog available types
    Given a hearth directory with the following structure:
      | path                                          | state  |
      | proposals/20260503T1200_kit_catalog_test/     | active |
    And the kit-bundled playbook is copied into the hearth tmpdir
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a tools/call request is sent for "catalog"
    Then the MCP catalog result includes available type "track"
