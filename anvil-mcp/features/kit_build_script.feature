Feature: Kit build script produces dist/anvil-kit/
  The scripts/build-kit.sh script assembles the distributable kit in dist/anvil-kit/.
  These scenarios assert the script exists, runs successfully, and produces the
  expected output layout with all required files present and valid.

  Scenario: scripts/build-kit.sh exists and is executable
    Then the build script "scripts/build-kit.sh" exists and is executable

  Scenario: running scripts/build-kit.sh exits 0
    Then executing the script "scripts/build-kit.sh" exits 0

  Scenario: dist/anvil-kit/mcp/anvil-mcp exists and is executable after build
    Given the build script has been executed
    Then the dist file "dist/anvil-kit/mcp/anvil-mcp" exists and is executable

  Scenario: dist/anvil-kit/mcp/anvil-mcp resolves and passes a hearth path after build
    Given the build script has been executed
    Then the dist file "dist/anvil-kit/mcp/anvil-mcp" contains "ANVIL_HEARTH_PATH"
    And the dist file "dist/anvil-kit/mcp/anvil-mcp" contains ".hearth"
    And the dist file "dist/anvil-kit/mcp/anvil-mcp" contains "--hearth"

  Scenario: dist/anvil-kit/app/engine/anvil-engine exists and is executable after build
    Given the build script has been executed
    Then the dist file "dist/anvil-kit/app/engine/anvil-engine" exists and is executable

  Scenario: dist/anvil-kit/app/frontend/dist/index.html exists after build
    Given the build script has been executed
    Then the dist file "dist/anvil-kit/app/frontend/dist/index.html" exists

  Scenario: dist/anvil-kit/foundry-manifest.json exists and is jq-parseable after build
    Given the build script has been executed
    Then the dist file "dist/anvil-kit/foundry-manifest.json" exists and is jq-parseable

  Scenario: dist/anvil-kit/foundry-manifest.json declares packaged playbooks after build
    Given the build script has been executed
    Then the dist manifest "dist/anvil-kit/foundry-manifest.json" declares playbook definition "track_lifecycle" at path "playbooks/track_lifecycle/" with anvil kind "track"
    And the dist manifest "dist/anvil-kit/foundry-manifest.json" does not declare playbook definition "daily_recap"

  Scenario: dist/anvil-kit/playbooks/track_lifecycle/machine.yaml exists after build
    Given the build script has been executed
    Then the dist file "dist/anvil-kit/playbooks/track_lifecycle/machine.yaml" exists

  Scenario: dist/anvil-kit/playbooks/track_lifecycle/skills/info/SKILL.md exists after build
    Given the build script has been executed
    Then the dist file "dist/anvil-kit/playbooks/track_lifecycle/skills/info/SKILL.md" exists

  Scenario: dist/anvil-kit does not include daily_recap after build
    Given the build script has been executed
    Then the dist path "dist/anvil-kit/playbooks/daily_recap" does not exist
