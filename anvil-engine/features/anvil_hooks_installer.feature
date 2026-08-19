Feature: anvil-hooks installs, uninstalls, and gate-checks across agent harnesses
  The anvil-hooks binary is the per-harness hook INSTALLER. It auto-detects each
  harness under a config dir (or takes --config-dir / --harness overrides), writes
  an idempotent, anvil-managed block into the harness's native config, and removes
  exactly that block on uninstall. Its gate-check subcommand resolves the edited
  file's enclosing forge artifact from a hearth and decides allow/block.

  These scenarios exercise the binary's FILE-I/O plumbing and CLI surface; the
  pure adapter transforms and the pure decision are proven in the anvil-core seam.

  Scenario: install into a detected Claude Code config dir writes a PreToolUse block
    Given an anvil-hooks config dir with a Claude Code settings file
    When anvil-hooks install runs for harness "claude-code" against that config dir
    Then the anvil-hooks command exits 0
    And the Claude Code settings file contains "anvil-hooks gate-check"
    And the anvil-hooks output reports harness "claude-code" written

  Scenario: a second install is idempotent (gate + subagent route, no duplication)
    Given an anvil-hooks config dir with a Claude Code settings file
    When anvil-hooks install runs for harness "claude-code" against that config dir
    And anvil-hooks install runs for harness "claude-code" against that config dir
    Then the anvil-hooks command exits 0
    And the Claude Code settings file has exactly 2 anvil-managed PreToolUse hook

  Scenario: uninstall removes only the anvil block and preserves the rest
    Given an anvil-hooks config dir with a Claude Code settings file carrying key "theme" value "dark"
    When anvil-hooks install runs for harness "claude-code" against that config dir
    And anvil-hooks uninstall runs for harness "claude-code" against that config dir
    Then the anvil-hooks command exits 0
    And the Claude Code settings file has exactly 0 anvil-managed PreToolUse hook
    And the Claude Code settings file still contains key "theme" value "dark"

  Scenario: auto-detect skips an absent harness with a clear report
    Given an anvil-hooks config dir with a Claude Code settings file
    When anvil-hooks install runs for harness "auto" against that config dir
    Then the anvil-hooks command exits 0
    And the anvil-hooks output reports harness "claude-code" written
    And the anvil-hooks output reports harness "hermes" skipped

  Scenario: install without --with-mcp leaves a pre-existing Foundry MCP entry byte-for-byte intact
    Given an anvil-hooks config dir with a Claude Code user MCP config carrying a Foundry-managed anvil server
    When anvil-hooks install runs for harness "claude-code" against that config dir
    Then the anvil-hooks command exits 0
    And the Claude Code settings file contains "anvil-hooks gate-check"
    And the Claude Code user MCP config file is byte-for-byte unchanged

  Scenario: uninstall without --with-mcp removes hooks only, leaving the Foundry MCP entry intact
    Given an anvil-hooks config dir with a Claude Code user MCP config carrying a Foundry-managed anvil server
    When anvil-hooks install runs for harness "claude-code" against that config dir
    And anvil-hooks uninstall runs for harness "claude-code" against that config dir
    Then the anvil-hooks command exits 0
    And the Claude Code settings file has exactly 0 anvil-managed PreToolUse hook
    And the Claude Code user MCP config file is byte-for-byte unchanged

  Scenario Outline: subcommand help never changes detected harness delivery
    Given an isolated home with detected configurations and hook artifacts for every supported harness
    When anvil-hooks "<subcommand>" with "<help_flag>" runs against the isolated home
    Then the anvil-hooks command exits 0
    And the anvil-hooks output shows usage
    And the isolated harness configuration tree is byte-for-byte unchanged

    Examples:
      | subcommand | help_flag |
      | install    | --help    |
      | install    | -h        |
      | uninstall  | --help    |
      | uninstall  | -h        |

  Scenario: gate-check blocks a hard-enforced artifact edit with no open begin
    Given an anvil-hooks hearth with a hard-enforced track artifact in state "spec" with no open begin
    When anvil-hooks gate-check runs for an edit inside that artifact
    Then the anvil-hooks gate-check decision is "block"

  Scenario: gate-check allows an edit to a path outside any forge artifact
    Given an anvil-hooks hearth with a hard-enforced track artifact in state "spec" with no open begin
    When anvil-hooks gate-check runs for an edit outside any artifact
    Then the anvil-hooks gate-check decision is "allow"
