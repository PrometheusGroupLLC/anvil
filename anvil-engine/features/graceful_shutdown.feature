Feature: Engine graceful shutdown on signal
  Foundry supervises the shared anvil engine and stops/restarts it with POSIX
  signals. The engine must shut down cleanly on SIGTERM and SIGINT (Ctrl-C) and
  release its listening port so a fresh engine can rebind it. Without a signal
  handler the engine blocks forever on `.serve(addr)` and Foundry cannot
  supervise its lifecycle.

  Scenario: the engine exits cleanly on SIGTERM and releases its port
    Given the engine is started with a minimal hearth
    When the engine is sent SIGTERM
    Then the engine exits within 5 seconds
    And the engine port can be re-bound

  Scenario: the engine exits cleanly on SIGINT and releases its port
    Given the engine is started with a minimal hearth
    When the engine is sent SIGINT
    Then the engine exits within 5 seconds
    And the engine port can be re-bound
