Feature: Engine drives the learning lifecycle end-to-end
  Learning is a free artifact, but beginable and lifecycle-driven by the engine.

  Scenario: begin creates a learning with lifecycle files and hook context
    Given a hearth seeded with the learning_lifecycle machine
    And the engine is started with that hearth
    When the begin RPC is called to create a "learning" artifact named "engine learning" with no parent
    Then the begin RPC response track_path starts with "learnings/"
    And the begin RPC response has non-empty "context_text"
    And the begin RPC response track_path has file "definition.md"
    And the begin RPC response track_path has file "evidence.md"
    And the begin RPC response track_path has file "review.md"
    And the begin RPC response track_path file "definition.md" contains "# Learning: engine learning"
    And the begin RPC response track_path file "status.yaml" contains "state: observation"

  Scenario: drive a learning through review, establish, amend, and retired
    Given a hearth seeded with the learning_lifecycle machine
    And the engine is started with that hearth
    When the engine drives a learning artifact through review establish amend and retired
    Then the e2e final state is "retired"
    And the e2e artifact path starts with "learnings/"
    And the e2e artifact resolved state is "retired"
    And the e2e artifact status.yaml contains "kind: learning"
    And the hearth file "learnings.md" contains "retired"
