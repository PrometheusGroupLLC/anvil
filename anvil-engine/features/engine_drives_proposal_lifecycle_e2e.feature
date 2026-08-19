Feature: Engine drives the proposal lifecycle end-to-end
  Proposal is a free artifact, but beginable and lifecycle-driven by the engine.

  Scenario: begin creates a proposal with lifecycle files and hook context
    Given a hearth seeded with the proposal_lifecycle machine
    And the engine is started with that hearth
    When the begin RPC is called to create a "proposal" artifact named "engine proposal" with no parent
    Then the begin RPC response track_path starts with "proposals/"
    And the begin RPC response has non-empty "context_text"
    And the begin RPC response track_path has file "definition.md"
    And the begin RPC response track_path has file "evidence.md"
    And the begin RPC response track_path has file "review.md"
    And the begin RPC response track_path has file "amendments.md"
    And the begin RPC response track_path file "definition.md" contains "# Proposal: engine proposal"
    And the begin RPC response track_path file "status.yaml" contains "state: vision"

  Scenario: drive a proposal through reviews amend reflection and completed
    Given a hearth seeded with the proposal_lifecycle machine
    And the engine is started with that hearth
    When the engine drives a proposal artifact through reviews amend reflection and completed
    Then the e2e final state is "completed"
    And the e2e artifact path starts with "proposals/"
    And the e2e artifact resolved state is "completed"
    And the e2e artifact status.yaml contains "kind: proposal"
    And the e2e artifact file "vision.md" contains "Proposal E2E Vision"
    And the e2e artifact file "proposal.md" contains "Proposal E2E Approach"
    And the e2e artifact file "proposal.amendments.md" contains "amend loop returned active"
    And the e2e artifact file "reflection.md" contains "reflection loop returned active"
    And the hearth file "proposals.md" contains "completed"
    And the hearth file "projections/intent.md" contains "Proposals"
