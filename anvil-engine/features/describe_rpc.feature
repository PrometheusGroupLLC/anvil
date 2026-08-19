Feature: Describe RPC
  The engine serves a describe RPC for type-level and instance-level queries.

  Scenario: Type-level describe returns creation schema
    Given the engine is started with a minimal hearth
    When the describe RPC is called with identifier "track"
    Then the describe RPC returns type info with name "track"
    And the describe RPC type info has parent type "proposal"

  Scenario: Unknown identifier returns gRPC NOT_FOUND
    Given the engine is started with a minimal hearth
    When the describe RPC is called with identifier "nonexistent_widget"
    Then the describe RPC returns gRPC status "NOT_FOUND"
