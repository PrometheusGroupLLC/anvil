Feature: CompleteEvent Enum Scaffolding
  Validates that the `CompleteEvent` enum exists and each variant's fields
  are addressable. The domain handler is not invoked; this feature purely
  validates the enum type and field round-trip via the step infrastructure.

  This is a Phase 1 scaffolding feature for the complete_handler_cqrs_conversion
  track. It does not test handler behavior — only that the enum is constructible
  and its field accessors are reachable.

  Scenario: TransitionRecorded variant is constructible and its fields are addressable
    Given a CompleteEvent::TransitionRecorded with artifact_path "tracks/my-track" to_state "spec_review" at "2026-04-20T00:00:00Z" actor_name "Test-000000" role "spec"
    Then the CompleteEvent is a TransitionRecorded variant
    And the CompleteEvent TransitionRecorded artifact_path is "tracks/my-track"
    And the CompleteEvent TransitionRecorded to_state is "spec_review"
    And the CompleteEvent TransitionRecorded role is "spec"

  Scenario: ActorUpserted variant is constructible and its fields are addressable
    Given a CompleteEvent::ActorUpserted with artifact_path "tracks/my-track" actor_name "Test-000000" actor_type "agent"
    Then the CompleteEvent is an ActorUpserted variant
    And the CompleteEvent ActorUpserted artifact_path is "tracks/my-track"
    And the CompleteEvent ActorUpserted identity actor_name is "Test-000000"

  Scenario: ReflectionWritten variant is constructible and its fields are addressable
    Given a CompleteEvent::ReflectionWritten with artifact_path "tracks/my-track" source_state "spec" filename "20260420T000000Z-Test-000000.md" body "# Notes\n\nSome content."
    Then the CompleteEvent is a ReflectionWritten variant
    And the CompleteEvent ReflectionWritten artifact_path is "tracks/my-track"
    And the CompleteEvent ReflectionWritten source_state is "spec"
    And the CompleteEvent ReflectionWritten filename is "20260420T000000Z-Test-000000.md"

  Scenario: TransitionRecorded with reviewer path has approver None when not set
    Given a CompleteEvent::TransitionRecorded with artifact_path "tracks/my-track" to_state "plan" at "2026-04-20T00:00:00Z" actor_name "Reviewer-000000" role "review"
    Then the CompleteEvent TransitionRecorded approver is absent
    And the CompleteEvent TransitionRecorded note is absent

  Scenario: CompleteOutcome type exists and wraps a CompleteResult with events
    Given a CompleteOutcome with new_state "spec_review" and zero events
    Then the CompleteOutcome new_state is "spec_review"
    And the CompleteOutcome has 0 events
