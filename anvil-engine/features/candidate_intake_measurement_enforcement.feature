Feature: Candidate intake honors measurement enforcement (dark-gated)
  The candidate intake RPC generates the machine to derive a name before it
  begins the generation track. That generation must honor the same measurement
  dark-gate as the terminal-generation persist: when
  ANVIL_ENFORCE_MEASUREMENT_DEFINITION is on, the enforcing generator refuses a
  candidate that lacks a falsifiable per-step success_criteria or an
  outcome_predicate, so a predicate-less candidate can never open a generation
  track under enforcement. Default OFF, so the ordinary intake path is unchanged.

  Scenario: under enforcement, a candidate lacking success_criteria is refused at intake
    Given a route hearth seeded with the builder machine
    And the engine is started with that hearth and measurement enforcement on
    And a separate temp owner-home directory
    When the engine intake of an anchored candidate playbook is attempted
    Then the candidate intake is refused with gRPC status "INVALID_ARGUMENT"
    And the candidate intake error message contains "invalid_candidate"
    And the candidate intake error message contains "VacuousSuccessCriteria"
