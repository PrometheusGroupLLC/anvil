Feature: Workflow measurement declaration and pure selection
  The measurement_by_role map on StateDefinition carries step-measurement specs
  (intent + expected_output) for each (state, role) pair. The pure selector
  state_role_measurement looks up the spec for a given state+role; it returns
  None for pairs with no declaration.

  Background:
    Given a fixture playbook machine with state measurements:
      | state        | role     | intent                                                                                                                                   | expected_output                                                                                                         |
      | spec         | doer     | Translate the approved proposal into a concrete, testable spec: define what to build and why, with acceptance criteria a reviewer can check. | A spec.md stating scope, acceptance criteria as brine-checkable statements, and explicit out-of-scope boundaries.       |
      | spec_review  | reviewer | Judge whether the spec's acceptance criteria are complete, unambiguous, and faithful to the proposal before plan work begins.             | A spec.review.md verdict (satisfied or findings) citing each acceptance criterion gap or confirming coverage.           |

  Scenario: state_role_measurement returns the spec doer measurement
    When state_role_measurement is called for state "spec" role "doer"
    Then the selected measurement has intent "Translate the approved proposal into a concrete, testable spec: define what to build and why, with acceptance criteria a reviewer can check."
    And the selected measurement has expected_output "A spec.md stating scope, acceptance criteria as brine-checkable statements, and explicit out-of-scope boundaries."

  Scenario: state_role_measurement returns the spec_review reviewer measurement
    When state_role_measurement is called for state "spec_review" role "reviewer"
    Then the selected measurement has intent "Judge whether the spec's acceptance criteria are complete, unambiguous, and faithful to the proposal before plan work begins."
    And the selected measurement has expected_output "A spec.review.md verdict (satisfied or findings) citing each acceptance criterion gap or confirming coverage."

  Scenario: state_role_measurement returns None for a state with no declaration
    When state_role_measurement is called for state "plan" role "doer"
    Then no measurement is selected

  Scenario: state_role_measurement returns None for a role with no declaration on a declared state
    When state_role_measurement is called for state "spec" role "reviewer"
    Then no measurement is selected
