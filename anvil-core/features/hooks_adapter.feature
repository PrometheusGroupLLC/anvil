Feature: Per-harness hook adapters synthesize idempotent, reversible config blocks
  The anvil-hooks installer delivers Anvil kit hooks into any agent harness via a
  per-harness adapter. Each adapter writes into a clearly DELIMITED, anvil-managed
  block inside the harness's native config so install is idempotent (re-run = no-op)
  and uninstall removes ONLY anvil's block, never the user's other settings.

  Claude Code is the hard-gate-capable target: its PreToolUse hook can BLOCK (exit
  code 2). Codex 0.144 also has a blocking PreToolUse hook, so its adapter is HARD.
  Kiln and Hermes are our own harnesses and get native force+validate hooks.
  Grok (TOML config) and opencode (JSON config) have no global config-declarable
  pre-tool block either, so both degrade to COOPERATIVE advisory markers.

  Scenario: the Claude Code adapter writes a PreToolUse gate hook into settings.json
    Given an empty Claude Code settings file
    When the Claude Code adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then the Claude Code settings has a PreToolUse hook with matcher "Edit|Write|MultiEdit|NotebookEdit"
    And the Claude Code settings PreToolUse hook command is "anvil-hooks gate-check"
    And the Claude Code settings PreToolUse hook timeout is 5000
    And the Claude Code adapter reports gate capability "hard"

  Scenario: a second Claude Code install is a no-op (idempotent — gate + subagent route)
    Given an empty Claude Code settings file
    When the Claude Code adapter installs with command "anvil-hooks gate-check" and timeout 5000
    And the Claude Code adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then the Claude Code settings has exactly 2 anvil-managed PreToolUse hook

  Scenario: install absorbs untagged anvil hooks left by an older version (no duplicate)
    Given a Claude Code settings file with untagged anvil gate and turn hooks
    When the Claude Code adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then the Claude Code settings has exactly 2 anvil-managed PreToolUse hook
    And the Claude Code settings has exactly 1 anvil-managed UserPromptSubmit hook
    And the Claude Code settings has no untagged anvil hook

  Scenario: the Claude Code adapter writes a subagent route hook matching both Task and Agent
    # Claude Code renamed the subagent-spawn tool Task -> Agent in v2.1.63; the matcher
    # must cover BOTH or the hook silently stops firing on current versions and all
    # subagent work goes unrouted/unmeasured.
    Given an empty Claude Code settings file
    When the Claude Code adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then the Claude Code settings has a subagent-route PreToolUse hook with matcher "Task|Agent" and command "anvil-hooks route-turn --source claude-code-subagent"

  Scenario: Claude Code uninstall removes the subagent route hook too
    Given an empty Claude Code settings file
    When the Claude Code adapter installs with command "anvil-hooks gate-check" and timeout 5000
    And the Claude Code adapter uninstalls
    Then the Claude Code settings has exactly 0 anvil-managed PreToolUse hook

  Scenario: the Claude Code adapter also writes a UserPromptSubmit turn hook
    Given an empty Claude Code settings file
    When the Claude Code adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then the Claude Code settings has a UserPromptSubmit hook with command "anvil-hooks route-turn --source claude-code"

  Scenario: a second Claude Code install keeps exactly one turn hook (idempotent)
    Given an empty Claude Code settings file
    When the Claude Code adapter installs with command "anvil-hooks gate-check" and timeout 5000
    And the Claude Code adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then the Claude Code settings has exactly 1 anvil-managed UserPromptSubmit hook

  Scenario: uninstall removes the Claude Code turn hook too
    Given an empty Claude Code settings file
    When the Claude Code adapter installs with command "anvil-hooks gate-check" and timeout 5000
    And the Claude Code adapter uninstalls
    Then the Claude Code settings has exactly 0 anvil-managed UserPromptSubmit hook

  Scenario: uninstall removes only the anvil block and preserves unrelated settings
    Given a Claude Code settings file with an unrelated user hook and key "theme" value "dark"
    When the Claude Code adapter installs with command "anvil-hooks gate-check" and timeout 5000
    And the Claude Code adapter uninstalls
    Then the Claude Code settings has no anvil-managed PreToolUse hook
    And the Claude Code settings still has the unrelated user hook
    And the Claude Code settings still has key "theme" value "dark"

  Scenario: the Codex adapter writes a real UserPromptSubmit route hook into hooks.json
    # Codex (>= 0.144, hooks stable) reads ~/.codex/hooks.json using Claude Code's hook
    # schema. A UserPromptSubmit command hook fires per user turn and its stdout /
    # hookSpecificOutput.additionalContext is injected back as developer context —
    # a REAL per-turn route channel, so the adapter installs the route-turn hook.
    Given an empty Codex hooks file
    When the Codex adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then the Codex hooks has a UserPromptSubmit hook with command "anvil-hooks route-turn --source codex"
    And the Codex hooks has a PreToolUse hook with matcher "apply_patch|Bash" and command "anvil-hooks gate-check --source codex --hard-enforce track --internal-deadline-ms 4000"
    And the Codex adapter reports gate capability "hard"

  Scenario: a second Codex install keeps exactly one route hook (idempotent)
    Given an empty Codex hooks file
    When the Codex adapter installs with command "anvil-hooks gate-check" and timeout 5000
    And the Codex adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then the Codex hooks has exactly 1 anvil-managed UserPromptSubmit hook
    And the Codex hooks has exactly 1 anvil-managed PreToolUse hook

  Scenario: install absorbs an untagged anvil route hook left by an older version (no duplicate)
    Given a Codex hooks.json with an untagged anvil route hook
    When the Codex adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then the Codex hooks has exactly 1 anvil-managed UserPromptSubmit hook

  Scenario: Codex uninstall removes the route hook
    Given an empty Codex hooks file
    When the Codex adapter installs with command "anvil-hooks gate-check" and timeout 5000
    And the Codex adapter uninstalls
    Then the Codex hooks has exactly 0 anvil-managed UserPromptSubmit hook

  Scenario: Codex install/uninstall preserves an operator-authored hook untouched
    Given a Codex hooks.json with an operator-authored UserPromptSubmit hook
    When the Codex adapter installs with command "anvil-hooks gate-check" and timeout 5000
    And the Codex adapter uninstalls
    Then the Codex hooks has exactly 0 anvil-managed UserPromptSubmit hook
    And the Codex hooks still has the operator hook

  Scenario: the Codex route hook emits codex 0.144's exact shape with the timeout in SECONDS
    # codex 0.144 reads ~/.codex/hooks.json and expects the hook `timeout` in
    # SECONDS, not milliseconds. Assert the COMPLETE emitted document shape (exact
    # nesting, type:command, command string, timeout:5) so a unit drift (e.g.
    # emitting 5000 ms) can't recur.
    Given an empty Codex hooks file
    When the Codex adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then the Codex hooks.json is exactly the codex route-hook shape with command "anvil-hooks route-turn --source codex" and timeout 5

  Scenario: install absorbs only the canonical route shape, preserving an operator anvil-hooks audit hook
    # Absorption on install matches ONLY the canonical legacy route command
    # (`anvil-hooks route-turn`), never an operator's own `anvil-hooks audit` hook.
    Given a Codex hooks.json with an operator-authored anvil-hooks audit hook
    When the Codex adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then the Codex hooks has exactly 1 anvil-managed UserPromptSubmit hook
    And the Codex hooks still has the operator audit hook

  Scenario: uninstall removes only tagged entries, preserving an operator anvil-hooks audit hook
    Given a Codex hooks.json with an operator-authored anvil-hooks audit hook
    When the Codex adapter installs with command "anvil-hooks gate-check" and timeout 5000
    And the Codex adapter uninstalls
    Then the Codex hooks has exactly 0 anvil-managed UserPromptSubmit hook
    And the Codex hooks still has the operator audit hook

  Scenario: install surgically prunes our stale handler from a mixed group, keeping the operator sibling
    Given a Codex hooks.json with a mixed group of a stale anvil route handler and an operator handler
    When the Codex adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then the Codex hooks has exactly 1 anvil-managed UserPromptSubmit hook
    And the Codex hooks still has the operator hook

  Scenario: uninstall of a mixed-group install removes our hook and leaves the operator sibling
    Given a Codex hooks.json with a mixed group of a stale anvil route handler and an operator handler
    When the Codex adapter installs with command "anvil-hooks gate-check" and timeout 5000
    And the Codex adapter uninstalls
    Then the Codex hooks has exactly 0 anvil-managed UserPromptSubmit hook
    And the Codex hooks still has the operator hook

  Scenario: plugin-managed install absorbs global Foundry routes without re-adding hooks
    Given a Codex hooks.json with an exact managed global route and an unrelated operator route
    When the Codex adapter absorbs the legacy global route for plugin management
    Then the Codex hooks has exactly 0 anvil-managed UserPromptSubmit hook
    And the Codex hooks has exactly 0 anvil-managed PreToolUse hook
    And the Codex hooks still has the operator hook

  Scenario: plugin-managed absorption preserves operator siblings in mixed groups
    Given a Codex hooks.json with a mixed group of a stale anvil route handler and an operator handler
    When the Codex adapter absorbs the legacy global route for plugin management
    Then the Codex hooks has exactly 0 anvil-managed UserPromptSubmit hook
    And the Codex hooks still has the operator hook

  Scenario: auto plugin-managed install keeps other harnesses native and Codex absorb-only
    Given detected Claude Code and Codex config directories with legacy Anvil hooks
    When anvil-hooks installs automatically in Codex plugin-managed mode
    Then Claude Code has its normal native Anvil hooks
    And Codex has no global Anvil route or gate

  Scenario: install fails open on a malformed Codex hooks container (leaves the file untouched)
    # A malformed-but-valid-JSON hooks structure must NOT be silently replaced; the
    # adapter fails open so the installer reports a clear error and never overwrites
    # operator content it can't safely parse.
    Given a Codex hooks.json with a malformed UserPromptSubmit container
    When the Codex adapter install is attempted
    Then the Codex adapter install reports an error
    And the Codex hooks config is left untouched

  Scenario: install fails open on a malformed Codex PreToolUse container
    Given a Codex hooks.json with a malformed PreToolUse container
    When the Codex adapter install is attempted
    Then the Codex adapter install reports an error
    And the Codex hooks config is left untouched

  Scenario: the Kiln adapter writes a PreToolUse gate hook into hooks.json with hard gate
    Given an empty Kiln config file
    When the Kiln adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then the Kiln config has an anvil-managed pre-tool hook
    And the Kiln config PreToolUse hook has matcher "write|edit|bash"
    And the Kiln config PreToolUse hook command is "anvil-hooks gate-check"
    And the Kiln config PreToolUse hook timeout is 5000
    And the Kiln adapter reports gate capability "hard"

  Scenario: the Kiln adapter also writes a UserPromptSubmit turn hook tagged with its source
    Given an empty Kiln config file
    When the Kiln adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then the Kiln config has a UserPromptSubmit hook with command "anvil-hooks route-turn --source kiln"

  Scenario: a second Kiln install is idempotent (exactly one gate + one turn hook)
    Given an empty Kiln config file
    When the Kiln adapter installs with command "anvil-hooks gate-check" and timeout 5000
    And the Kiln adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then the Kiln config has exactly 1 anvil-managed PreToolUse hook
    And the Kiln config has exactly 1 anvil-managed UserPromptSubmit hook

  Scenario: Kiln uninstall removes only anvil entries, preserving the operator's hook
    Given a Kiln hooks.json with an operator-authored PreToolUse hook
    When the Kiln adapter installs with command "anvil-hooks gate-check" and timeout 5000
    And the Kiln adapter uninstalls
    Then the Kiln config has exactly 0 anvil-managed PreToolUse hook
    And the Kiln config still has the operator hook

  Scenario: the Hermes adapter writes a real pre_tool_call gate hook with hard gate
    Given an empty Hermes config file
    When the Hermes adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then the Hermes config has an anvil-managed pre-tool hook
    And the Hermes config has a pre_tool_call entry with matcher "write_file|patch"
    And the Hermes config pre_tool_call entry command is "anvil-hooks gate-check"
    And the Hermes config pre_tool_call entry timeout is 5
    And the Hermes adapter reports gate capability "hard"

  Scenario: a second Hermes install is a no-op (idempotent)
    Given an empty Hermes config file
    When the Hermes adapter installs with command "anvil-hooks gate-check" and timeout 5000
    And the Hermes adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then the Hermes config has exactly 1 anvil-managed pre_tool_call entry

  Scenario: the Hermes adapter also writes a pre_llm_call turn hook
    Given an empty Hermes config file
    When the Hermes adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then the Hermes config has a pre_llm_call entry command "anvil-hooks route-turn --source hermes"
    And the Hermes config has exactly 1 anvil-managed pre_llm_call entry

  Scenario: a second Hermes install keeps exactly one turn entry (idempotent)
    Given an empty Hermes config file
    When the Hermes adapter installs with command "anvil-hooks gate-check" and timeout 5000
    And the Hermes adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then the Hermes config has exactly 1 anvil-managed pre_llm_call entry

  Scenario: uninstall removes the Hermes turn entry too, preserving the user's hook
    Given a realistic Hermes config with comments, an unrelated key, and a user pre_tool_call hook
    When the Hermes adapter installs with command "anvil-hooks gate-check" and timeout 5000
    And the Hermes adapter uninstalls
    Then the Hermes config has exactly 0 anvil-managed pre_llm_call entry
    And the Hermes config still has the user pre_tool_call hook
    And the Hermes config still has key "approvals_mode" value "smart"

  Scenario: install preserves a pre-existing user hook, comments, and unrelated keys
    Given a realistic Hermes config with comments, an unrelated key, and a user pre_tool_call hook
    When the Hermes adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then the Hermes config has a pre_tool_call entry with matcher "write_file|patch"
    And the Hermes config still has the user pre_tool_call hook
    And the Hermes config still has the leading comment
    And the Hermes config still has key "approvals_mode" value "smart"

  Scenario: uninstall removes only the anvil entry and preserves the user's hook and the rest
    Given a realistic Hermes config with comments, an unrelated key, and a user pre_tool_call hook
    When the Hermes adapter installs with command "anvil-hooks gate-check" and timeout 5000
    And the Hermes adapter uninstalls
    Then the Hermes config has no anvil-managed pre-tool hook
    And the Hermes config still has the user pre_tool_call hook
    And the Hermes config still has the leading comment
    And the Hermes config still has key "approvals_mode" value "smart"

  # Foundry's Hermes MCP writer round-trips the whole config through serde, which carries no
  # comments — so every `foundry kit install` strips the markers off anvil's own entries and
  # re-emits them in serde's sequence style (the `-` at the parent key's indent). Recognizing only
  # the marker made the next install insert a SECOND, nested copy, leaving the file with sequence
  # items as siblings of a mapping's keys: not valid YAML, so every later read failed to parse.
  Scenario: re-installing over entries that lost their markers replaces them instead of duplicating
    Given a Hermes config whose anvil entries lost their markers to a serde round-trip
    When the Hermes adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then the Hermes config has exactly 1 anvil-managed pre_tool_call entry
    And the Hermes config has exactly 1 anvil-managed pre_llm_call entry
    And the Hermes config parses as valid YAML
    And the Hermes config still has key "approvals_mode" value "smart"

  Scenario: the Grok adapter writes a cooperative block and claims no hard gate
    Given an empty Grok config file
    When the Grok adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then the Grok config has an anvil-managed cooperative block
    And the Grok adapter reports gate capability "cooperative"

  Scenario: the Grok cooperative block carries the source-tagged turn command
    Given an empty Grok config file
    When the Grok adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then the config contains "anvil-hooks route-turn --source grok"

  Scenario: a second Grok install is a no-op (idempotent) and uninstall removes the block
    Given an empty Grok config file
    When the Grok adapter installs with command "anvil-hooks gate-check" and timeout 5000
    And the Grok adapter installs with command "anvil-hooks gate-check" and timeout 5000
    And the Grok adapter uninstalls
    Then the Grok config has no anvil-managed cooperative block

  Scenario: the Grok adapter preserves an unrelated user key through install and uninstall
    Given a Grok config file with key "yolo" value "false"
    When the Grok adapter installs with command "anvil-hooks gate-check" and timeout 5000
    And the Grok adapter uninstalls
    Then the Grok config has no anvil-managed cooperative block
    And the Grok config still has the line "yolo = false"

  Scenario: the Grok adapter ships a drop-in UserPromptSubmit plugin (the forced route hook)
    Given an empty Grok config file
    When the Grok adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then an artifact is installed at "plugins/anvil-route-turn/.claude-plugin/plugin.json"
    And an artifact is installed at "plugins/anvil-route-turn/hooks/hooks.json"
    And an installed artifact contains "UserPromptSubmit"
    And an installed artifact contains "anvil-hooks route-turn --source grok"

  Scenario: the opencode adapter writes NO config marker (it would break opencode) and claims no hard gate
    Given an empty opencode config file
    When the opencode adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then the opencode config has exactly 0 anvil-managed cooperative marker
    And the opencode adapter reports gate capability "cooperative"

  Scenario: the opencode adapter ships a drop-in plugin carrying the source-tagged turn command
    Given an empty opencode config file
    When the opencode adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then an artifact is installed at "plugin/anvil-route-turn.js"
    And an installed artifact contains "chat.message"
    And an installed artifact contains "route-turn"
    And an installed artifact contains "--source"
    And an installed artifact contains "opencode"

  Scenario: opencode install repairs a config broken by a legacy _anvil_managed key
    Given an opencode config broken by a legacy anvil marker
    When the opencode adapter installs with command "anvil-hooks gate-check" and timeout 5000
    Then the opencode config has exactly 0 anvil-managed cooperative marker
    And the opencode config still has mcp server "other"

  Scenario: opencode install/uninstall preserves an existing mcp server untouched
    Given an opencode config with an existing mcp server "other"
    When the opencode adapter installs with command "anvil-hooks gate-check" and timeout 5000
    And the opencode adapter uninstalls
    Then the opencode config has exactly 0 anvil-managed cooperative marker
    And the opencode config still has mcp server "other"
