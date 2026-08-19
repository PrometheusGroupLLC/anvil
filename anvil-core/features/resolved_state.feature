Feature: FullStatusYaml resolved_state accessor
  The shared accessor on the canonical status type resolves an artifact's
  state as: the top-level `state` field if present, otherwise the last
  transition's `to`, otherwise None (unresolvable). All filesystem read
  sites route through this single definition.

  Scenario: Top-level state present is returned without consulting transitions
    Given a status with top-level state "spec" and last transition to "plan"
    When resolved_state is computed
    Then the resolved state is "spec"

  Scenario: No top-level state falls back to the last transition's to
    Given a status with no top-level state and last transition to "decided"
    When resolved_state is computed
    Then the resolved state is "decided"

  Scenario: No state and no transitions is unresolvable
    Given a status with no top-level state and no transitions
    When resolved_state is computed
    Then the resolved state is unresolvable
