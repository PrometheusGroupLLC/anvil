Feature: Playbook seed track describe identity
  # Scope: This feature asserts identity ONLY along the interpreter-visible axis —
  # the three transition fields the R12.1 interpreter reads:
  #   from_state, to_state, required_role
  # Other seed fields (role_filters, registry_section, projection_targets,
  # is_review_gate, is_terminal, hook) are intentionally NOT asserted here
  # per R12.4's partial-population permission.
  #
  # Purpose: Phase 5 replaces the hardcoded match in describe::available_actions
  # for the track kind. Any divergence between the track seed's transitions and
  # the current hardcoded match output would be a Phase-5 regression waiting
  # to happen. This canary must be green before the Phase 5 cutover.

  # Each scenario asserts that the track seed contains a transition matching
  # the corresponding action() call in describe::available_actions("track", _).

  Scenario: spec state outgoing transition matches describe
    Given the track seed
    Then the track seed has transition from "spec" to "spec_review" with role "spec"

  Scenario: spec_review state has two outgoing transitions matching describe
    Given the track seed
    Then the track seed has transition from "spec_review" to "spec_revision" with role "reviewer"
    And the track seed has transition from "spec_review" to "plan" with role "reviewer"

  Scenario: spec_revision state outgoing transition matches describe
    Given the track seed
    Then the track seed has transition from "spec_revision" to "spec_review" with role "spec"

  Scenario: plan state outgoing transition matches describe
    Given the track seed
    Then the track seed has transition from "plan" to "plan_review" with role "plan"

  Scenario: plan_review state has two outgoing transitions matching describe
    Given the track seed
    Then the track seed has transition from "plan_review" to "plan_revision" with role "plan"
    And the track seed has transition from "plan_review" to "implementing" with role "implement"

  Scenario: plan_revision state outgoing transition matches describe
    Given the track seed
    Then the track seed has transition from "plan_revision" to "plan_review" with role "reviewer"

  Scenario: implementing state has two outgoing transitions matching describe
    Given the track seed
    Then the track seed has transition from "implementing" to "impl_phase_review" with role "reviewer"
    And the track seed has transition from "implementing" to "impl_review" with role "implement"

  Scenario: impl_phase_review state outgoing transition matches describe
    Given the track seed
    Then the track seed has transition from "impl_phase_review" to "implementing" with role "implement"

  Scenario: impl_review state has two outgoing transitions matching describe
    Given the track seed
    Then the track seed has transition from "impl_review" to "impl_revision" with role "implement"
    And the track seed has transition from "impl_review" to "reflecting" with role "reflect"

  Scenario: impl_revision state outgoing transition matches describe
    Given the track seed
    Then the track seed has transition from "impl_revision" to "impl_review" with role "reviewer"

  Scenario: reflecting state outgoing transition matches describe
    Given the track seed
    Then the track seed has transition from "reflecting" to "reflection_review" with role "reflect"

  Scenario: reflection_review state has two outgoing transitions matching describe
    Given the track seed
    Then the track seed has transition from "reflection_review" to "reflection_revision" with role "reflect"
    And the track seed has transition from "reflection_review" to "completed" with role "complete"

  Scenario: reflection_revision state outgoing transition matches describe
    Given the track seed
    Then the track seed has transition from "reflection_revision" to "reflection_review" with role "reviewer"

  # B5b BP4: `completed` is now non-terminal — it gains the amend-loop edges.
  Scenario: completed enters the amend loop in track seed
    Given the track seed
    Then the track seed has transition from "completed" to "amend" with role "doer"
    And the track seed has transition from "amend" to "amend_review" with role "doer"
    And the track seed has transition from "amend_review" to "completed" with role "reviewer"
    And the track seed has transition from "amend_review" to "amend_revision" with role "doer"
    And the track seed has transition from "amend_revision" to "amend_review" with role "doer"

  Scenario: terminal states have no outgoing transitions in track seed
    Given the track seed
    And the track seed has no transitions from "abandoned"
    And the track seed has no transitions from "superseded"
