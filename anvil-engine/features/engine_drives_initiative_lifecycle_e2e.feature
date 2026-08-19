Feature: Engine drives the initiative lifecycle end-to-end
  Initiative is a free artifact, but beginable and lifecycle-driven by the engine.

  Scenario: begin creates an initiative with lifecycle files and hook context
    Given a hearth seeded with the initiative_lifecycle machine
    And the engine is started with that hearth
    When the begin RPC is called to create a "initiative" artifact named "engine initiative" with no parent
    Then the begin RPC response track_path starts with "initiatives/"
    And the begin RPC response has non-empty "context_text"
    And the begin RPC response track_path has file "definition.md"
    And the begin RPC response track_path has file "evidence.md"
    And the begin RPC response track_path has file "review.md"
    And the begin RPC response track_path file "definition.md" contains "# Initiative: engine initiative"
    And the begin RPC response track_path file "status.yaml" contains "state: draft"

  Scenario: drive an initiative through review, promotion, demotion, log, reflect, and retired
    Given a hearth seeded with the initiative_lifecycle machine
    And the engine is started with that hearth
    When the engine drives an initiative artifact through review promote demote log reflect and retired
    Then the e2e final state is "retired"
    And the e2e artifact path starts with "initiatives/"
    And the e2e artifact resolved state is "retired"
    And the e2e artifact status.yaml contains "kind: initiative"
    And the e2e artifact file "evidence.md" contains "log self-edge kept active"
    And the e2e artifact file "reflection.md" contains "reflect self-edge kept active"
    And the hearth file "initiatives.md" contains "retired"
