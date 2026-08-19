Feature: Engine drives the proposal lifecycle
  Proposal is a free artifact kind retired from the old envision/propose skill path.
  The engine owns create, hook context, describe actions, and routing exclusion.

  Scenario: begin creates a proposal vision with lifecycle files and hook context
    When begin create proposal is executed with a vision hook
    Then the proposal begin result is state "vision"
    And the proposal begin context contains "PROPOSAL VISION HOOK"
    And the proposal begin event scaffolds files:
      | definition.md |
      | evidence.md |
      | review.md |
      | amendments.md |

  Scenario: proposal reports engine support from available type and describe actions
    When execution_route is computed for proposal available_type
    Then the proposal execution_route is "engine"
    When describe is called for a proposal in state "vision"
    Then the proposal describe actions include "vision_review" with execution_route "engine"
    When describe is called for a proposal in state "active"
    Then the proposal describe actions include "completed" with execution_route "engine"

  Scenario: proposal drives vision through draft proposal and active review gates
    Then the proposal seed has transition from "vision" to "vision_review" with role "envision"
    And the proposal seed has review transition from "vision_review" to "draft" with role "reviewer" and satisfaction "satisfied"
    And the proposal seed has transition from "draft" to "draft_review" with role "propose"
    And the proposal seed has review transition from "draft_review" to "proposal" with role "reviewer" and satisfaction "satisfied"
    And the proposal seed has transition from "proposal" to "proposal_review" with role "propose"
    And the proposal seed has review transition from "proposal_review" to "active" with role "reviewer" and satisfaction "satisfied"

  Scenario: proposal drives amend reflection and completed loops
    Then the proposal seed has transition from "active" to "amend" with role "amend"
    And the proposal seed has review transition from "amend_review" to "active" with role "reviewer" and satisfaction "satisfied"
    And the proposal seed has transition from "active" to "reflecting" with role "reflect"
    And the proposal seed has review transition from "reflection_review" to "active" with role "reviewer" and satisfaction "satisfied"
    And the proposal seed has gated transition from "active" to "completed" with role "doer"

  Scenario: proposal is free, absent from route candidates, and still begin-able
    Then the proposal seed is register free
    When proposal seed route candidates are selected
    Then the proposal route candidates do not include proposal
    When route resolution is called for proposal-like input
    Then route selects no proposal kind
    When begin create proposal is executed with a vision hook
    Then the proposal begin result is state "vision"
