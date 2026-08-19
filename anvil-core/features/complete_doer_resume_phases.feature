Feature: Complete drives track doer-resume phases
  Doer completion advances each doer-resume phase to its review gate. The
  optional implementation phase review remains snapshot-only because it keeps
  reviewer role and therefore is not a doer-complete candidate.

  Scenario: doer complete advances plan to plan_review
    Given an in-memory query adapter seeded with artifact "tracks/20260614T0610_plan_complete" kind "track" state "plan"
    When complete is called via query adapter with:
      | artifact_path  | tracks/20260614T0610_plan_complete |
      | actor_name     | Doer-200001                         |
      | actor_type     | agent                               |
      | actor_model    | test-model                          |
      | actor_provider | test                                |
      | at             | 2026-06-14T06:10:00Z                |
    Then the complete outcome is successful
    And the complete outcome new_state is "plan_review"
    And the complete outcome TransitionRecorded to_state is "plan_review"
    And the complete outcome TransitionRecorded role is "plan"

  Scenario: doer complete advances implementing to impl_review
    Given an in-memory query adapter seeded with artifact "tracks/20260614T0611_impl_complete" kind "track" state "implementing"
    When complete is called via query adapter with:
      | artifact_path  | tracks/20260614T0611_impl_complete |
      | actor_name     | Doer-200001                        |
      | actor_type     | agent                              |
      | actor_model    | test-model                         |
      | actor_provider | test                               |
      | at             | 2026-06-14T06:11:00Z               |
    Then the complete outcome is successful
    And the complete outcome new_state is "impl_review"
    And the complete outcome TransitionRecorded to_state is "impl_review"
    And the complete outcome TransitionRecorded role is "implement"

  Scenario: doer complete advances reflecting to reflection_review
    Given an in-memory query adapter seeded with artifact "tracks/20260614T0612_reflect_complete" kind "track" state "reflecting"
    When complete is called via query adapter with:
      | artifact_path  | tracks/20260614T0612_reflect_complete |
      | actor_name     | Doer-200001                           |
      | actor_type     | agent                                 |
      | actor_model    | test-model                            |
      | actor_provider | test                                  |
      | at             | 2026-06-14T06:12:00Z                  |
    Then the complete outcome is successful
    And the complete outcome new_state is "reflection_review"
    And the complete outcome TransitionRecorded to_state is "reflection_review"
    And the complete outcome TransitionRecorded role is "reflect"

  Scenario: impl_phase_review remains reachable by snapshot from implementing
    Given a snapshot adapter
    And the snapshot adapter has artifact "tracks/impl-phase" of kind "track" in state "implementing"
    And the snapshot adapter has existing registry entry for "impl-phase" in "tracks.md"
    When snapshot is executed with:
      | artifact_path | tracks/impl-phase |
      | to_state      | impl_phase_review |
      | actor_name    | Reviewer-200002   |
      | actor_role    | reviewer          |
      | actor_type     | agent             |
      | actor_model    | test-model        |
      | actor_provider | test              |
    Then the snapshot result is successful
    And the snapshot adapter moved registry entry "impl-phase" in "tracks.md" to section "implementing"

  Scenario: implementing complete selects impl_review, not impl_phase_review
    Given an in-memory query adapter seeded with artifact "tracks/20260614T0613_impl_unique" kind "track" state "implementing"
    When complete is called via query adapter with:
      | artifact_path  | tracks/20260614T0613_impl_unique |
      | actor_name     | Doer-200003                      |
      | actor_type     | agent                            |
      | actor_model    | test-model                       |
      | actor_provider | test                             |
      | at             | 2026-06-14T06:13:00Z             |
    Then the complete outcome is successful
    And the complete outcome new_state is "impl_review"
    And the complete outcome TransitionRecorded role is "implement"
