Feature: Kit manifest jq-validity
  The kit/foundry-manifest.json file must exist, be valid JSON, and conform
  to the kit-builder-guide v0.3.0 schema. These scenarios verify the manifest
  is present and structurally correct before the kit is assembled.

  Background:
    Given the kit manifest at "kit/foundry-manifest.json" is loaded

  Scenario: kit/foundry-manifest.json exists at the repo root
    Then the file "kit/foundry-manifest.json" exists in the workspace

  Scenario: manifest is valid JSON (jq-parseable)
    Then the manifest field "kit.id" equals "anvil-kit"

  Scenario: kit.id equals "anvil-kit"
    Then the manifest field "kit.id" equals "anvil-kit"

  Scenario: kit.components contains "app", "mcp", and "playbook"
    Then the manifest field "kit.components" contains the value "app"
    Then the manifest field "kit.components" contains the value "mcp"
    And the manifest field "kit.components" contains the value "playbook"

  Scenario: kit.components does not contain "ui"
    Then the manifest field "kit.components" does not contain the value "ui"

  Scenario: mcp.servers[0].command starts with "${KIT_ROOT}/mcp/"
    Then the manifest field "mcp.servers[0].command" starts with "${KIT_ROOT}/mcp/"

  Scenario: playbooks.definitions[0].path is a bare relative path (no ${KIT_ROOT} prefix)
    Then the manifest field "playbooks.definitions[0].path" does not start with "${KIT_ROOT}"

  Scenario: all required kit.* fields are present
    Then the manifest has all required kit fields
