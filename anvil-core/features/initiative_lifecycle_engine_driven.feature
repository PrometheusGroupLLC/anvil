Feature: Engine drives the initiative lifecycle
  Initiative is a free artifact kind retired from the old skill path.
  The engine owns create, hook context, describe actions, and routing exclusion.

  Scenario: begin creates an initiative draft with lifecycle files and hook context
    When begin create initiative is executed with a draft hook
    Then the initiative begin result is state "draft"
    And the initiative begin context contains "INITIATIVE DRAFT HOOK"
    And the initiative begin event scaffolds files:
      | definition.md |
      | evidence.md |
      | review.md |

  Scenario: initiative reports engine support from available type and describe actions
    When execution_route is computed for initiative available_type
    Then the initiative execution_route is "engine"
    When describe is called for an initiative in state "draft"
    Then the initiative describe actions include "draft_review" with execution_route "engine"

  Scenario: initiative is free, absent from route candidates, and still begin-able
    Then the initiative seed is register free
    When initiative seed route candidates are selected
    Then the initiative route candidates do not include initiative
    When route resolution is called for initiative-like input
    Then route selects no initiative kind
    When begin create initiative is executed with a draft hook
    Then the initiative begin result is state "draft"

  Scenario: initiative seed declares review, promotion, retirement, log, and reflect edges
    Then the initiative seed has transition "draft" to "draft_review" role "draft"
    And the initiative seed has review transition "draft_review" to "draft_revision" role "draft" satisfaction "needs_revision"
    And the initiative seed has review transition "draft_review" to "active" role "reviewer" satisfaction "satisfied"
    And the initiative seed has transition "active" to "promoted" role "promote" requiring approver
    And the initiative seed has transition "promoted" to "active" role "demote" requiring approver
    And the initiative seed has transition "active" to "retired" role "retire" requiring approver
    And the initiative seed has transition "active" to "active" role "log"
    And the initiative seed has transition "promoted" to "promoted" role "log"
    And the initiative seed has transition "active" to "active" role "reflect"
