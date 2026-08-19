Feature: Engine drives the decision lifecycle end-to-end
  Decision is a free artifact, but beginable and lifecycle-driven by the engine.

  Scenario: begin creates a decision with lifecycle files and hook context
    Given a hearth seeded with the decision_lifecycle machine
    And the engine is started with that hearth
    When the begin RPC is called to create a "decision" artifact named "engine decision" with no parent
    Then the begin RPC response track_path starts with "decisions/"
    And the begin RPC response has non-empty "context_text"
    And the begin RPC response track_path has file "definition.md"
    And the begin RPC response track_path has file "evidence.md"
    And the begin RPC response track_path has file "review.md"
    And the begin RPC response track_path has file "amendments.md"
    And the begin RPC response track_path file "definition.md" contains "# Decision: engine decision"
    And the begin RPC response track_path file "status.yaml" contains "state: tension"

  Scenario: drive a decision through review, amend, and retired
    Given a hearth seeded with the decision_lifecycle machine
    And the engine is started with that hearth
    When the engine drives a decision artifact through review amend and retired
    Then the e2e final state is "retired"
    And the e2e artifact path starts with "decisions/"
    And the e2e artifact resolved state is "retired"
    And the e2e artifact status.yaml contains "kind: decision"
    And the hearth file "decisions.md" contains "retired"
    And the hearth file "projections/decisions.md" contains "Decisions"
