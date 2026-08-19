Feature: Engine HealthCheck reports wire compatibility metadata
  HealthCheck is the shim's safe preflight before it sends mutating requests.
  The response must expose the gRPC wire protocol version and the engine build
  version so a shim can reject an incompatible shared endpoint before Begin.

  Scenario: HealthCheck returns the current wire protocol version
    Given the engine is started with a minimal hearth
    When the gRPC HealthCheck is requested on the engine port
    Then the HealthCheck response reports the current wire protocol version
    And the HealthCheck response build_version is non-empty
