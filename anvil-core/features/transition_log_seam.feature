Feature: Transition-log resolution seam
  The single chokepoint every consumer routes through to resolve an
  artifact's current state and its ordered transition history from the
  parsed legacy status.yaml. `resolve_state` returns the top-level `state`
  if present, otherwise the last transition's `to`. `declared_state` returns
  the raw top-level `state:` field only, with no transition fallback.
  `resolve_transitions` returns the history oldest → newest.

  Scenario: Current state prefers the top-level field over the transition log
    Given a legacy status with top-level state "spec" and transitions "spec,plan"
    When the transition-log seam resolves the status
    Then the seam current state is "spec"
    And the seam declared state is "spec"
    And the seam history in order is "spec,plan"

  Scenario: Current state falls back to the last transition when no top-level state
    Given a legacy status with no top-level state and transitions "tension,investigating,decided"
    When the transition-log seam resolves the status
    Then the seam current state is "decided"
    And the seam declared state is empty
    And the seam history in order is "tension,investigating,decided"

  Scenario: No top-level state and no transitions yields an empty history
    Given a legacy status with no top-level state and transitions ""
    When the transition-log seam resolves the status
    Then the seam declared state is empty
    And the seam history is empty
