Feature: Engine drives the milestone lifecycle end-to-end
  Milestone is a free artifact, but beginable and lifecycle-driven by the engine.

  Scenario: begin creates a milestone with lifecycle files and hook context
    Given a hearth seeded with the milestone_lifecycle machine
    And the engine is started with that hearth
    When the begin RPC is called to create a "milestone" artifact named "engine milestone" with no parent
    Then the begin RPC response track_path starts with "milestones/"
    And the begin RPC response has non-empty "context_text"
    And the begin RPC response track_path has file "definition.md"
    And the begin RPC response track_path has file "evidence.md"
    And the begin RPC response track_path has file "review.md"
    And the begin RPC response track_path has file "amendments.md"
    And the begin RPC response track_path file "definition.md" contains "# Milestone: engine milestone"
    And the begin RPC response track_path file "status.yaml" contains "state: draft"

  Scenario: drive a milestone through review amend reflection and completed
    Given a hearth seeded with the milestone_lifecycle machine
    And the engine is started with that hearth
    When the engine drives a milestone artifact through review amend reflection and completed
    Then the e2e final state is "completed"
    And the e2e artifact path starts with "milestones/"
    And the e2e artifact resolved state is "completed"
    And the e2e artifact status.yaml contains "kind: milestone"
    And the e2e artifact file "definition.md" contains "Milestone E2E Definition"
    And the e2e artifact file "amendments.md" contains "amend loop returned active"
    And the e2e artifact file "reflection.md" contains "reflection loop returned active"
    And the hearth file "milestones.md" contains "completed"
    And the hearth file "projections/intent.md" contains "Milestones"
