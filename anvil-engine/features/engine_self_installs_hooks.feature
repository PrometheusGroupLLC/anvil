Feature: Engine startup self-installs anvil route hooks
  The anvil engine owns universal front-door presence. On startup it performs
  the hooks-only equivalent of `anvil-hooks install --harness auto`, while
  remaining fail-open and opt-out friendly for supervised environments.

  Scenario: startup self-install writes detected harness hooks idempotently
    Given a detected Claude Code harness config under a temp self-install root
    When engine startup self-install runs
    And engine startup self-install runs again
    Then the self-install report has harness "claude-code" installed
    And the self-install report has harness "hermes" skipped
    And the Claude Code self-install settings file contains "anvil-hooks route-turn --source claude-code"
    And the Claude Code self-install settings file has exactly 1 anvil-managed UserPromptSubmit hook
    And the Claude Code self-install settings file has exactly 2 anvil-managed PreToolUse hook

  Scenario: startup self-install captures installer errors fail-open
    Given a detected Claude Code harness config under a temp self-install root that cannot be written
    When engine startup self-install runs
    Then the self-install report has harness "claude-code" errored
    And engine startup self-install completed fail-open

  Scenario: ANVIL_SKIP_HOOK_INSTALL skips startup self-install
    Given a detected Claude Code harness config under a temp self-install root
    When engine startup self-install runs with ANVIL_SKIP_HOOK_INSTALL set
    Then the self-install report says startup install was skipped by env
    And the Claude Code self-install settings file does not contain "anvil-hooks"

  Scenario: startup self-install is hooks-only and preserves Foundry MCP registration
    Given a detected Claude Code harness config with a Foundry-managed MCP entry under a temp self-install root
    When engine startup self-install runs
    Then the self-install report has harness "claude-code" installed
    And the Claude Code self-install settings file contains "anvil-hooks gate-check"
    And the Claude Code self-install MCP config file is byte-for-byte unchanged
