Feature: Engine fixed-port endpoint serves both dial and TCP-connect probe
  The shared engine listens on a single fixed gRPC port. That one listener must
  satisfy BOTH the shim's gRPC dial (HealthCheck) AND Foundry's TCP-connect
  health probe — there is no second listener and no HTTP server. This scenario
  uses an ephemeral port in the harness to preserve test isolation; it pins the
  dial + raw-TCP-connect behaviors, not the literal production port number
  (the literal 50051 is asserted against the manifest in Phase 5).

  Scenario: one listener answers a gRPC HealthCheck and a raw TCP connection
    Given the engine is started with a minimal hearth
    Then a gRPC HealthCheck on the engine port succeeds
    And a raw TCP connection to the engine port succeeds
