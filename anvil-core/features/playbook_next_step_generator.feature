Feature: Machine-derived next_step is generic across every playbook
  next_step_for(machine, state) synthesizes engine-native guidance from the
  OUTGOING transitions of a state, for ANY playbook — not a hardcoded
  track-only string table. At a non-review-gate doer state it names `complete`
  and the single `to_state` the artifact advances to; at a review-gate state it
  enumerates the satisfaction options and their target states; at a terminal
  state it reports completion. Every non-terminal state of every playbook gets
  a NON-EMPTY, skill-free next_step. This is the proof the generator is generic:
  it is exercised against the registered proposal, decision, and lore_query-like
  doer states, plus a review gate — not against track_lifecycle.

  Scenario: proposal vision (doer state) names complete and the correct to_state
    Given the proposal seed machine
    When next_step_for is computed for state "vision"
    Then the next_step is non-empty
    And the next_step contains "complete"
    And the next_step contains "vision_review"
    And the next_step does not contain "forge:"

  Scenario: decision tension (doer state) names complete and a reachable to_state
    Given the decision seed machine
    When next_step_for is computed for state "tension"
    Then the next_step is non-empty
    And the next_step contains "complete"
    And the next_step contains "tension_review"
    And the next_step does not contain "forge:"

  Scenario: a lore_query-style loading doer state names complete and persisting
    Given a fixture machine "lore_query" with doer flow "loading" to "persisting" and terminal "persisting"
    When next_step_for is computed for state "loading"
    Then the next_step is non-empty
    And the next_step contains "complete"
    And the next_step contains "persisting"
    And the next_step does not contain "forge:"

  Scenario: a review-gate state enumerates its satisfaction options
    Given the proposal seed machine
    When next_step_for is computed for state "vision_review"
    Then the next_step is non-empty
    And the next_step contains "satisfaction"
    And the next_step contains "satisfied"
    And the next_step does not contain "forge:"

  Scenario: a terminal state reports the artifact is complete
    Given the proposal seed machine
    When next_step_for is computed for state "completed"
    Then the next_step is non-empty
    And the next_step contains "complete"
