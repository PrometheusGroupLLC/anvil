Feature: Kit manifest app engine config
  The anvil kit must be a Foundry-supervised app, not only an on-demand MCP
  server. Its manifest declares one shared engine on the fixed endpoint that
  MCP shims can adopt.

  Background:
    Given the kit manifest at "kit/foundry-manifest.json" is loaded

  Scenario: app block parses as Foundry supervision config
    Then the manifest field "manifest_version" equals "2"
    And the manifest app block parses as a Foundry kit app manifest
    And the manifest app frontend has a build directory

  Scenario: app block has a frontend for Foundry scheme loading
    Then the manifest app block parses as a Foundry kit app manifest
    And the manifest app frontend has a build directory

  Scenario: app engine binds a fixed literal port outside the walking pool
    # A DECLARED fixed port (no ${ENGINE_PORT} template) makes Foundry treat anvil
    # as a fixed-port kit: it reaps a squatter on the literal port and never walks
    # the 14300-14399 pool across restarts, so the watchdog can't probe a stale
    # port. 50051 is anvil's built-in default (and != kiln's 8470).
    Then the manifest field "app.engine.command" equals "${KIT_ROOT}/app/engine/anvil-engine"
    And the manifest field "app.engine.args[0]" equals "--port"
    And the manifest field "app.engine.args[1]" equals "50051"
    And the manifest field "app.engine.health_check" equals "http://127.0.0.1:50051/health"
    And the manifest app engine uses literal port "50051" without ENGINE_PORT placeholders

  Scenario: app starts automatically with Foundry
    Then the manifest field "app.lifecycle" equals "always-on"
