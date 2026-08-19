Feature: K8 required-by-state matrix and source-dependent binding tensions
  validate_required_by_state checks state-local inline fields per the printed
  K8 matrix (§1). The logically-always-required history is enforced by the
  strict loader, not this validator. Parked R* bindings are tolerated; the two
  frozen source-dependent binding tensions (superseded, aged-out) are handled
  by the edge, not the state-local validator.

  Background:
    Given a backlog fixture

  @schema
  Scenario Outline: A target missing a state-local required field rejects
    Given a source backlog item in state "candidate" from provenance "bare"
    When required-by-state validation is run for target "<target>"
    Then the backlog operation is rejected because "<missing>"

    Examples:
      | target      | missing              |
      | ready       | rank                 |
      | ready       | dependency_ready     |
      | in_flight   | execution_binding    |
      | in_flight   | success_measure_id   |
      | superseded  | superseded_by        |
      | parked      | wake_condition       |

  @schema
  Scenario Outline: A complete target satisfies the state-local validator
    Given a candidate backlog item shaped for row "<row>"
    When required-by-state validation is run for target "<target>"
    Then the backlog operation succeeds

    Examples:
      | row | target      |
      | 1   | ready       |
      | 5   | in_flight   |
      | 10  | done        |

  @schema
  Scenario Outline: An outcome binding that declares no success measure is not a declared binding
    Given a source backlog item in state "in_flight" from provenance "bound_without_declared_measure"
    When required-by-state validation is run for target "<target>"
    Then the backlog operation is rejected because "success_measure_id"

    Examples:
      | target    |
      | in_flight |
      | done      |

  @schema
  Scenario: A parked item may retain rank while staying off-view
    Given a candidate backlog item shaped for row "7"
    When required-by-state validation is run for target "parked"
    Then the backlog operation succeeds

  @schema
  Scenario: Superseded tolerates absent bindings at the state-local validator
    Given a source backlog item in state "ready" from provenance "ready_shaped_no_binding"
    When required-by-state validation is run for target "superseded"
    Then the backlog operation succeeds

  @schema
  Scenario: The always-required history is enforced by the loader not the state validator
    Given a source backlog item in state "candidate" from provenance "empty_history"
    When required-by-state validation is run for target "candidate"
    Then the backlog operation is rejected because "history"
