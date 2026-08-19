Feature: Per-harness MCP registration writes an idempotent, reversible anvil MCP server entry
  The anvil MCP server is a KIT component. Just like the hooks, the anvil-hooks
  installer registers it into every harness's NATIVE MCP config with a
  VERSION-STABLE command, so projects no longer hand-wire stale/foreign binary
  paths. Each per-harness MCP writer transforms the harness's native MCP config:
  install injects an anvil-managed `anvil` server entry (stdio transport); a
  pre-existing `anvil` entry is REPLACED (correcting stale paths); uninstall
  removes ONLY the anvil entry, preserving every other MCP server and all
  surrounding content/comments.

  Claude Code registers into the USER config (`~/.claude.json` mcpServers map),
  Codex into `[mcp_servers.anvil-mcp]` in config.toml, Claude Desktop into its
  mcpServers map, and Kiln into a JSON ARRAY of `McpServerSpec` in
  `~/.kiln/mcp.json` (element named `anvil`, command split into `command` +
  `args`). Only Hermes has no discoverable MCP-server config, so it is skipped.

  Scenario: the Claude Code MCP writer registers an anvil server with the stable command
    Given an empty Claude Code user MCP config
    When the Claude Code MCP writer registers anvil with command "/kit/mcp/anvil-mcp"
    Then the Claude Code MCP config has an anvil server with command "/kit/mcp/anvil-mcp"
    And the Claude Code MCP config anvil server transport is "stdio"

  Scenario: a second Claude Code MCP registration is a no-op (idempotent)
    Given an empty Claude Code user MCP config
    When the Claude Code MCP writer registers anvil with command "/kit/mcp/anvil-mcp"
    And the Claude Code MCP writer registers anvil with command "/kit/mcp/anvil-mcp"
    Then the Claude Code MCP config has exactly 1 anvil server

  Scenario: a pre-existing stale anvil entry is replaced and other servers are preserved
    Given a Claude Code user MCP config with a stale anvil entry and an unrelated server "other"
    When the Claude Code MCP writer registers anvil with command "/kit/mcp/anvil-mcp"
    Then the Claude Code MCP config has an anvil server with command "/kit/mcp/anvil-mcp"
    And the Claude Code MCP config has exactly 1 anvil server
    And the Claude Code MCP config still has server "other"

  Scenario: uninstall removes only the anvil server and preserves the rest
    Given a Claude Code user MCP config with a stale anvil entry and an unrelated server "other"
    When the Claude Code MCP writer registers anvil with command "/kit/mcp/anvil-mcp"
    And the Claude Code MCP writer unregisters anvil
    Then the Claude Code MCP config has exactly 0 anvil server
    And the Claude Code MCP config still has server "other"

  Scenario: the Codex MCP writer registers an anvil-mcp server in config.toml
    Given an empty Codex MCP config file
    When the Codex MCP writer registers anvil with command "/kit/mcp/anvil-mcp"
    Then the Codex config has an mcp_servers anvil-mcp entry with command "/kit/mcp/anvil-mcp"

  Scenario: a second Codex MCP registration is a no-op (idempotent)
    Given an empty Codex MCP config file
    When the Codex MCP writer registers anvil with command "/kit/mcp/anvil-mcp"
    And the Codex MCP writer registers anvil with command "/kit/mcp/anvil-mcp"
    Then the Codex config has exactly 1 mcp_servers anvil-mcp entry

  Scenario: a pre-existing stale Codex anvil-mcp entry is replaced, preserving comments and keys
    Given a realistic Codex config with a comment, an unrelated key, and a stale mcp_servers anvil-mcp entry
    When the Codex MCP writer registers anvil with command "/kit/mcp/anvil-mcp"
    Then the Codex config has an mcp_servers anvil-mcp entry with command "/kit/mcp/anvil-mcp"
    And the Codex config has exactly 1 mcp_servers anvil-mcp entry
    And the Codex config still has the leading comment
    And the Codex config still has key "approval_policy" value "on-request"

  Scenario: Codex uninstall removes only the anvil-mcp entry, preserving comments and keys
    Given a realistic Codex config with a comment, an unrelated key, and a stale mcp_servers anvil-mcp entry
    When the Codex MCP writer registers anvil with command "/kit/mcp/anvil-mcp"
    And the Codex MCP writer unregisters anvil
    Then the Codex config has exactly 0 mcp_servers anvil-mcp entry
    And the Codex config still has the leading comment
    And the Codex config still has key "approval_policy" value "on-request"

  Scenario: the Kiln MCP writer registers an anvil server element in mcp.json
    Given an empty Kiln MCP config file
    When the Kiln MCP writer registers anvil with command "/kit/mcp/anvil-mcp"
    Then the Kiln MCP config has an anvil server with command "/kit/mcp/anvil-mcp"

  Scenario: the Kiln MCP writer splits a command with args into separate command + args fields
    Given an empty Kiln MCP config file
    When the Kiln MCP writer registers anvil with command "/kit/mcp/anvil-mcp --hearth /h"
    Then the Kiln MCP config anvil server command field is "/kit/mcp/anvil-mcp"
    And the Kiln MCP config anvil server args are "--hearth,/h"

  Scenario: a second Kiln MCP registration is byte-identical (idempotent)
    Given an empty Kiln MCP config file
    When the Kiln MCP writer registers anvil with command "/kit/mcp/anvil-mcp"
    And the Kiln MCP writer registers anvil with command "/kit/mcp/anvil-mcp"
    Then the Kiln MCP config has exactly 1 anvil server
    And registering Kiln anvil twice with command "/kit/mcp/anvil-mcp" is byte-identical to once

  Scenario: a pre-existing stale Kiln anvil entry is replaced and other servers are preserved unchanged
    Given a Kiln MCP config with a stale anvil entry and an unrelated server "other"
    When the Kiln MCP writer registers anvil with command "/kit/mcp/anvil-mcp"
    Then the Kiln MCP config has an anvil server with command "/kit/mcp/anvil-mcp"
    And the Kiln MCP config has exactly 1 anvil server
    And the Kiln MCP config still has server "other" unchanged

  Scenario: Kiln uninstall removes only the anvil server and preserves the rest
    Given a Kiln MCP config with a stale anvil entry and an unrelated server "other"
    When the Kiln MCP writer registers anvil with command "/kit/mcp/anvil-mcp"
    And the Kiln MCP writer unregisters anvil
    Then the Kiln MCP config has exactly 0 anvil server
    And the Kiln MCP config still has server "other"
