Feature: anvil-hooks --with-mcp registers the anvil MCP server into each harness
  MCP registration is OPT-IN. By default `anvil-hooks install` delivers HOOKS ONLY
  and never touches any harness's MCP config — Foundry's wire.rs owns the
  version-stable anvil-mcp registration. The standalone path (for users running
  anvil WITHOUT Foundry, where nothing else registers MCP) passes `--with-mcp` to
  ALSO register the anvil MCP server into each detected harness's native MCP config
  with a VERSION-STABLE command. Foundry, when it does drive this, would pass
  `--mcp-command ${KIT_ROOT}/mcp/anvil-mcp`; standalone the command is derived from
  the running anvil-hooks binary's sibling anvil-mcp. Claude Code registers into the
  USER config `.claude.json` (one entry covers all projects). `uninstall --with-mcp`
  removes the anvil MCP entry. The pure per-harness transforms are proven in the
  anvil-core seam; here we prove the CLI surface and file I/O.

  Scenario: install without --with-mcp does NOT register any anvil MCP entry
    Given an anvil-hooks config dir with a Claude Code settings file
    When anvil-hooks install runs for harness "claude-code" with mcp-command "/kit/mcp/anvil-mcp" against that config dir
    Then the anvil-hooks command exits 0
    And the Claude Code user MCP config file has exactly 0 anvil server

  Scenario: install --with-mcp registers the anvil MCP server in the Claude Code user config
    Given an anvil-hooks config dir with a Claude Code settings file
    When anvil-hooks install runs for harness "claude-code" with-mcp and mcp-command "/kit/mcp/anvil-mcp" against that config dir
    Then the anvil-hooks command exits 0
    And the Claude Code user MCP config file has an anvil server with command "/kit/mcp/anvil-mcp"

  Scenario: a second --with-mcp install is idempotent (exactly one anvil MCP entry)
    Given an anvil-hooks config dir with a Claude Code settings file
    When anvil-hooks install runs for harness "claude-code" with-mcp and mcp-command "/kit/mcp/anvil-mcp" against that config dir
    And anvil-hooks install runs for harness "claude-code" with-mcp and mcp-command "/kit/mcp/anvil-mcp" against that config dir
    Then the anvil-hooks command exits 0
    And the Claude Code user MCP config file has exactly 1 anvil server

  Scenario: install --with-mcp without --mcp-command derives the sibling anvil-mcp path
    Given an anvil-hooks config dir with a Claude Code settings file
    When anvil-hooks install runs for harness "claude-code" with-mcp and no mcp-command against that config dir
    Then the anvil-hooks command exits 0
    And the Claude Code user MCP config file anvil server command ends with "anvil-mcp"

  Scenario: uninstall --with-mcp removes the anvil MCP entry, preserving an unrelated server
    Given an anvil-hooks config dir with a Claude Code user MCP config carrying server "other"
    When anvil-hooks install runs for harness "claude-code" with-mcp and mcp-command "/kit/mcp/anvil-mcp" against that config dir
    And anvil-hooks uninstall runs for harness "claude-code" with-mcp against that config dir
    Then the anvil-hooks command exits 0
    And the Claude Code user MCP config file has exactly 0 anvil server
    And the Claude Code user MCP config file still has server "other"

  Scenario: install --with-mcp registers the anvil MCP server in the Codex config
    Given an anvil-hooks config dir with a Codex config file
    When anvil-hooks install runs for harness "codex" with-mcp and mcp-command "/kit/mcp/anvil-mcp" against that config dir
    Then the anvil-hooks command exits 0
    And the Codex config file has an mcp_servers anvil-mcp entry with command "/kit/mcp/anvil-mcp"
