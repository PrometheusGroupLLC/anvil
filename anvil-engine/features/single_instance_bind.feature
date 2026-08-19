Feature: Engine first-bind-wins on a fixed endpoint
  The shared engine uses one TCP listener as the arbitration point. If an
  engine is already bound to a port, a second engine on that same port must
  fail fast instead of creating a duplicate daemon or stealing the listener.

  Scenario: a second engine on the same port fails fast
    Given the engine is started with a minimal hearth
    When a second engine is started on the same port
    Then the second engine fails fast because the port is already in use
    And a gRPC HealthCheck on the engine port succeeds
