Feature: Complete domain — doer path emits correct events (spec → spec_review)
  After Phase 2 CQRS conversion: CompleteCommandHandler::execute reads via
  InMemoryQueryAdapter (QueryPort) and emits Vec<CompleteEvent> rather than
  calling mutation ports inline. The doer path (empty satisfaction) on a
  track in state "spec" must emit exactly three events in order:
    1. ActorUpserted { artifact_path, identity }
    2. TransitionRecorded { to_state: "spec_review", role: "spec" }
  (No ReflectionWritten when reflection_notes is empty.)

  Scenario: Doer complete on spec track — emits ActorUpserted then TransitionRecorded
    Given an in-memory query adapter seeded with artifact "tracks/20260420T1922_doer_events" kind "track" state "spec"
    When complete is called via query adapter with:
      | artifact_path  | tracks/20260420T1922_doer_events |
      | actor_name     | Doer-100001                      |
      | actor_type     | agent                            |
      | actor_model    | claude-opus-4-7                  |
      | actor_provider | anthropic                        |
      | at             | 2026-04-20T19:22:00Z             |
    Then the complete outcome is successful
    And the complete outcome new_state is "spec_review"
    And the complete outcome has 2 events
    And the complete outcome event 0 is ActorUpserted
    And the complete outcome event 1 is TransitionRecorded
    And the complete outcome TransitionRecorded to_state is "spec_review"
    And the complete outcome TransitionRecorded role is "spec"
    And the complete outcome TransitionRecorded actor_name is "Doer-100001"
    And the complete outcome TransitionRecorded approver is absent
    And the complete outcome ActorUpserted actor_name is "Doer-100001"
    And the complete outcome ActorUpserted artifact_path is "tracks/20260420T1922_doer_events"

  Scenario: Doer complete on spec track with note — TransitionRecorded carries note
    Given an in-memory query adapter seeded with artifact "tracks/20260420T1922_doer_note" kind "track" state "spec"
    When complete is called via query adapter with:
      | artifact_path  | tracks/20260420T1922_doer_note |
      | actor_name     | Doer-100002                    |
      | actor_type     | agent                          |
      | actor_model    | claude-opus-4-7                |
      | actor_provider | anthropic                      |
      | note           | Ready for review.              |
      | at             | 2026-04-20T19:22:01Z           |
    Then the complete outcome is successful
    And the complete outcome has 2 events
    And the complete outcome TransitionRecorded note is "Ready for review."

  Scenario: Doer complete with reflection_notes — emits three events (ActorUpserted, ReflectionWritten, TransitionRecorded)
    Given an in-memory query adapter seeded with artifact "tracks/20260420T1922_doer_reflection" kind "track" state "spec"
    When complete is called via query adapter with:
      | artifact_path    | tracks/20260420T1922_doer_reflection |
      | actor_name       | Doer-100003                          |
      | actor_type       | agent                                |
      | actor_model      | claude-opus-4-7                      |
      | actor_provider   | anthropic                            |
      | reflection_notes | Noted the approach worked cleanly.   |
      | at               | 2026-04-20T19:22:02Z                 |
    Then the complete outcome is successful
    And the complete outcome has 3 events
    And the complete outcome event 0 is ActorUpserted
    And the complete outcome event 1 is ReflectionWritten
    And the complete outcome event 2 is TransitionRecorded
    And the complete outcome ReflectionWritten source_state is "spec"
    And the complete outcome ReflectionWritten artifact_path is "tracks/20260420T1922_doer_reflection"
    And the complete outcome ReflectionWritten filename is "20260420T192202Z-Doer-100003.md"

  Scenario: Doer complete on wrong state returns WrongStateForComplete
    Given an in-memory query adapter seeded with artifact "tracks/20260420T1922_doer_wrong_state" kind "track" state "spec_review"
    When complete is called via query adapter with:
      | artifact_path  | tracks/20260420T1922_doer_wrong_state |
      | actor_name     | Doer-100004                           |
      | actor_type     | agent                                 |
      | actor_model    | claude-opus-4-7                       |
      | actor_provider | anthropic                             |
      | at             | 2026-04-20T19:22:03Z                  |
    Then the complete outcome is a CompleteError containing "wrong_state_for_complete"
    And the complete outcome is a CompleteError containing "spec_review"
