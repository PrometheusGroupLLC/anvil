Feature: Engine drives the learning lifecycle
  Learning is a free artifact kind engine-driven from the old skill path.
  The engine owns create, hook context, describe actions, and routing exclusion.

  Scenario: begin creates a learning observation with three lifecycle files and hook context
    When begin create learning is executed with a capture hook
    Then the learning begin result is state "observation"
    And the learning begin context contains "LEARNING CAPTURE HOOK"
    And the learning begin event scaffolds files:
      | definition.md |
      | evidence.md |
      | review.md |

  Scenario: learning reports engine support from available type and describe actions
    When execution_route is computed for learning available_type
    Then the learning execution_route is "engine"
    When describe is called for a learning in state "observation"
    Then the learning describe actions include "observation_review" with execution_route "engine"

  Scenario: learning is free, absent from route candidates, and still begin-able
    Then the learning seed is register free
    When learning seed route candidates are selected
    Then the learning route candidates do not include learning
    When route resolution is called for learning-like input
    Then route selects no learning kind
    When begin create learning is executed with a capture hook
    Then the learning begin result is state "observation"
