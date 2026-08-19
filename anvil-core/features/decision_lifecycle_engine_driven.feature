Feature: Engine drives the decision lifecycle
  Decision is the first free artifact kind retired from the old skill path.
  The engine owns create, hook context, describe actions, and routing exclusion.

  Scenario: begin creates a decision tension with four lifecycle files and hook context
    When begin create decision is executed with a tension hook
    Then the decision begin result is state "tension"
    And the decision begin context contains "DECISION CREATE HOOK"
    And the decision begin event scaffolds files:
      | definition.md |
      | evidence.md |
      | review.md |
      | amendments.md |

  Scenario: decision reports engine support from available type and describe actions
    When execution_route is computed for decision available_type
    Then the execution_route is "engine"
    When describe is called for a decision in state "tension"
    Then the decision describe actions include "tension_review" with execution_route "engine"

  Scenario: decision is free, absent from route candidates, and still begin-able
    Then the decision seed is register free
    When decision seed route candidates are selected
    Then the decision route candidates do not include decision
    When route resolution is called for decision-like input
    Then route selects no decision kind
    When begin create decision is executed with a tension hook
    Then the decision begin result is state "tension"
