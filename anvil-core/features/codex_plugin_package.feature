Feature: The Anvil kit carries one Codex-native hard-gate plugin
  Eligible Foundry installs consume an Anvil-owned Codex plugin manifest and hook
  asset. The asset is rendered from the same contract as standalone installation,
  while global migration only absorbs prior Foundry routes.

  Scenario: the Codex plugin manifest references the distinct native hook asset
    Given the Anvil Codex plugin package is staged
    Then the staged Codex plugin manifest references "./codex-hooks/hooks.json"
    And the referenced Codex hook asset has exactly one source-codex route
    And the referenced Codex hook asset has exactly one apply_patch-or-Bash hard gate
    And the referenced Codex hard gate carries the track enforcement policy
    And the Codex internal deadline is shorter than the outer hook timeout
    And the staged Codex hook wrapper reaches the native host binary
    And the Anvil kit manifest keeps its global hard policy disabled

  Scenario: build-kit stages reachable cross-platform Codex hook commands
    Given build-kit stages the Anvil Codex plugin distribution
    Then the staged Codex hooks use plugin-root commands for POSIX and Windows
    And the staged Codex hook wrappers reach each packaged platform binary
    When the staged POSIX Codex gate command runs outside every Anvil hearth
    Then the staged Codex gate command exits successfully

  Scenario: a zero hook timeout still leaves room for the internal deadline
    Given the Anvil Codex plugin package is staged with a zero timeout
    Then the Codex internal deadline is shorter than the outer hook timeout

  Scenario: repeated install update and repair remain singular
    Given a Codex home with an unrelated global hook and an exact legacy Foundry route
    And an older staged Anvil plugin routes turns with source claude-code
    And the Anvil Codex plugin package is staged
    When plugin-managed migration is applied for install update and repair
    Then the Codex global hooks have zero Foundry-owned routes
    And the Codex global hooks still have the unrelated hook
    And the staged plugin still has one route and one gate
    And no staged Anvil plugin route uses source claude-code

  Scenario: plugin uninstall is scoped to the Foundry-owned plugin configuration
    Given a Codex home with an unrelated global hook and an exact legacy Foundry route
    And the Anvil Codex plugin package is staged
    When plugin-managed migration is applied and the staged plugin is unloaded
    Then the Codex global hooks have zero Foundry-owned routes
    And the Codex global hooks still have the unrelated hook
    And the staged Codex plugin configuration is absent

  Scenario: real Codex accepts the staged hook-bearing plugin
    Given an isolated Codex home and local marketplace containing the staged Anvil plugin
    When real Codex registers the marketplace and installs the Anvil plugin
    Then real Codex reports installed Anvil plugin version from the kit manifest
    And the staged Codex hook wrapper reaches the native host binary
