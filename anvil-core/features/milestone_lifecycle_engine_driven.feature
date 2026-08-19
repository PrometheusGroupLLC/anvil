Feature: Engine drives the milestone lifecycle
  Milestone is a free artifact kind retired from the old milestone skill path.
  The engine owns create, hook context, describe actions, and routing exclusion.

  Scenario: begin creates a milestone draft with lifecycle files and hook context
    When begin create milestone is executed with a draft hook
    Then the milestone begin result is state "draft"
    And the milestone begin context contains "MILESTONE DRAFT HOOK"
    And the milestone begin event scaffolds files:
      | definition.md |
      | evidence.md |
      | review.md |
      | amendments.md |

  Scenario: milestone reports engine support from available type and describe actions
    When execution_route is computed for milestone available_type
    Then the milestone execution_route is "engine"
    When describe is called for a milestone in state "draft"
    Then the milestone describe actions include "draft_review" with execution_route "engine"
    When describe is called for a milestone in state "active"
    Then the milestone describe actions include "completed" with execution_route "engine"

  Scenario: milestone drives draft to active through review
    Then the milestone seed has transition from "draft" to "draft_review" with role "doer"
    And the milestone seed has review transition from "draft_review" to "draft_revision" with role "doer" and satisfaction "needs_revision"
    And the milestone seed has transition from "draft_revision" to "draft_review" with role "doer"
    And the milestone seed has review transition from "draft_review" to "active" with role "reviewer" and satisfaction "satisfied"

  Scenario: milestone drives amend and reflection loops
    Then the milestone seed has transition from "active" to "amend" with role "amend"
    And the milestone seed has transition from "amend" to "amend_review" with role "amend"
    And the milestone seed has review transition from "amend_review" to "amend_revision" with role "amend" and satisfaction "needs_revision"
    And the milestone seed has transition from "amend_revision" to "amend_review" with role "amend"
    And the milestone seed has review transition from "amend_review" to "active" with role "reviewer" and satisfaction "satisfied"
    And the milestone seed has transition from "active" to "reflecting" with role "reflect"
    And the milestone seed has transition from "reflecting" to "reflection_review" with role "reflect"
    And the milestone seed has review transition from "reflection_review" to "reflection_revision" with role "reflect" and satisfaction "needs_revision"
    And the milestone seed has transition from "reflection_revision" to "reflection_review" with role "reflect"
    And the milestone seed has review transition from "reflection_review" to "active" with role "reviewer" and satisfaction "satisfied"

  Scenario: milestone declares completed and abandoned terminal gates
    Then the milestone seed has gated transition from "active" to "completed" with role "doer"
    And the milestone seed has gated transition from "active" to "superseded" with role "doer"
    And the milestone seed has gated transition from "active" to "abandoned" with role "doer"

  Scenario: milestone is free, absent from route candidates, and still begin-able
    Then the milestone seed is register free
    When milestone seed route candidates are selected
    Then the milestone route candidates do not include milestone
    When route resolution is called for milestone-like input
    Then route selects no milestone kind
    When begin create milestone is executed with a draft hook
    Then the milestone begin result is state "draft"
