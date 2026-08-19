Feature: AC4 HEADLINE — the builder drives begin -> ... -> completed end-to-end through the engine
  A playbook_generation artifact is begun against an ACTIVE parent track and
  driven gathering -> ... -> completed (the happy path through all seven review
  gates) over the real engine, each hop machine-derived. 14 hops total. Mirrors
  the knowledge_lifecycle e2e drive but with the builder's active-parent
  enforcement (parent_kind: track requires the parent be state: active).

  Scenario: drive a playbook_generation artifact gathering -> completed
    Given a hearth seeded with the builder machine and an active parent track
    And the engine is started with that hearth
    When the engine drives a playbook_generation artifact from gathering to completed
    Then the e2e final state is "completed"
    And the e2e artifact resolved state is "completed"
    And the e2e artifact status.yaml contains "kind: playbook_generation"
    And the e2e artifact status.yaml contains "target_owner: kit:test-owner"
    And the e2e artifact path starts with "workflow_generations/"
