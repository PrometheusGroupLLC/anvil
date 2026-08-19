Feature: Complete domain — reviewer path emits correct events (spec_review → plan)
  After Phase 2 CQRS conversion: CompleteCommandHandler::execute reads via
  InMemoryQueryAdapter (QueryPort) and emits Vec<CompleteEvent>. The reviewer
  path (satisfaction: "satisfied") on a track in state "spec_review" must emit
  exactly two events in order:
    1. ActorUpserted { artifact_path, identity }
    2. TransitionRecorded { to_state: "plan", role: "review" }
  (No ReflectionWritten when reflection_notes is empty.)

  Scenario: Reviewer complete on spec_review track — emits ActorUpserted then TransitionRecorded
    Given an in-memory query adapter seeded with artifact "tracks/20260420T1922_reviewer_events" kind "track" state "spec_review"
    When complete is called via query adapter with:
      | artifact_path  | tracks/20260420T1922_reviewer_events |
      | actor_name     | Reviewer-200001                      |
      | actor_type     | agent                                |
      | actor_model    | claude-opus-4-7                      |
      | actor_provider | anthropic                            |
      | satisfaction   | satisfied                            |
      | at             | 2026-04-20T19:22:10Z                 |
    Then the complete outcome is successful
    And the complete outcome new_state is "plan"
    And the complete outcome has 2 events
    And the complete outcome event 0 is ActorUpserted
    And the complete outcome event 1 is TransitionRecorded
    And the complete outcome TransitionRecorded to_state is "plan"
    And the complete outcome TransitionRecorded role is "review"
    And the complete outcome TransitionRecorded actor_name is "Reviewer-200001"
    And the complete outcome TransitionRecorded approver is absent
    And the complete outcome ActorUpserted actor_name is "Reviewer-200001"
    And the complete outcome ActorUpserted artifact_path is "tracks/20260420T1922_reviewer_events"

  Scenario: Reviewer complete with approver — TransitionRecorded carries approver
    Given an in-memory query adapter seeded with artifact "tracks/20260420T1922_reviewer_approver" kind "track" state "spec_review"
    When complete is called via query adapter with:
      | artifact_path  | tracks/20260420T1922_reviewer_approver |
      | actor_name     | Reviewer-200002                        |
      | actor_type     | agent                                  |
      | actor_model    | claude-opus-4-7                        |
      | actor_provider | anthropic                              |
      | satisfaction   | satisfied                              |
      | approver       | mark                                   |
      | at             | 2026-04-20T19:22:11Z                   |
    Then the complete outcome is successful
    And the complete outcome has 2 events
    And the complete outcome TransitionRecorded approver is "mark"
    And the complete outcome TransitionRecorded to_state is "plan"

  Scenario: Reviewer complete with reflection_notes — emits three events (ActorUpserted, ReflectionWritten, TransitionRecorded)
    Given an in-memory query adapter seeded with artifact "tracks/20260420T1922_reviewer_reflection" kind "track" state "spec_review"
    When complete is called via query adapter with:
      | artifact_path    | tracks/20260420T1922_reviewer_reflection |
      | actor_name       | Reviewer-200003                          |
      | actor_type       | agent                                    |
      | actor_model      | claude-opus-4-7                          |
      | actor_provider   | anthropic                                |
      | satisfaction     | satisfied                                |
      | reflection_notes | Found the spec clear and well-structured. |
      | at               | 2026-04-20T19:22:12Z                     |
    Then the complete outcome is successful
    And the complete outcome has 3 events
    And the complete outcome event 0 is ActorUpserted
    And the complete outcome event 1 is ReflectionWritten
    And the complete outcome event 2 is TransitionRecorded
    And the complete outcome ReflectionWritten source_state is "spec_review"
    And the complete outcome ReflectionWritten artifact_path is "tracks/20260420T1922_reviewer_reflection"
    And the complete outcome ReflectionWritten filename is "20260420T192212Z-Reviewer-200003.md"

  Scenario: Reviewer complete on wrong state (spec) returns WrongStateForComplete
    Given an in-memory query adapter seeded with artifact "tracks/20260420T1922_reviewer_wrong_state" kind "track" state "spec"
    When complete is called via query adapter with:
      | artifact_path  | tracks/20260420T1922_reviewer_wrong_state |
      | actor_name     | Reviewer-200004                           |
      | actor_type     | agent                                     |
      | actor_model    | claude-opus-4-7                           |
      | actor_provider | anthropic                                 |
      | satisfaction   | satisfied                                 |
      | at             | 2026-04-20T19:22:13Z                      |
    Then the complete outcome is a CompleteError containing "wrong_state_for_complete"
    And the complete outcome is a CompleteError containing "spec"
    And the complete outcome is a CompleteError containing "doer"

  Scenario: Complete on unsupported state returns WrongStateForComplete
    Given an in-memory query adapter seeded with artifact "tracks/20260420T1922_plan_review_state" kind "track" state "plan_review"
    When complete is called via query adapter with:
      | artifact_path  | tracks/20260420T1922_plan_review_state |
      | actor_name     | Actor-200005                    |
      | actor_type     | agent                           |
      | actor_model    | claude-opus-4-7                 |
      | actor_provider | anthropic                       |
      | at             | 2026-04-20T19:22:14Z            |
    Then the complete outcome is a CompleteError containing "wrong_state_for_complete"
    And the complete outcome is a CompleteError containing "plan_review"

  Scenario: Complete with unknown artifact returns NotFound
    Given an in-memory query adapter with no artifacts seeded
    When complete is called via query adapter with:
      | artifact_path  | tracks/20260420T1922_nonexistent |
      | actor_name     | Actor-200006                     |
      | actor_type     | agent                            |
      | actor_model    | claude-opus-4-7                  |
      | actor_provider | anthropic                        |
      | at             | 2026-04-20T19:22:15Z             |
    Then the complete outcome is a CompleteError containing "not found"
