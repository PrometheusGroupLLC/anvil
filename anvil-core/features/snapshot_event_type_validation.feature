Feature: Snapshot event_type validation
  In projection-only mode, `event_type` must be one of the known values
  (`spark`, `annotation`). An empty or unknown value is rejected as
  `InvalidArgument`. Outside projection-only mode an ordinary `event_type`
  (e.g. `spark`) is carried through unvalidated — EXCEPT that the RESERVED
  value `adoption` is refused on a public snapshot. `adoption` is the internal
  governance-adoption discriminator; only the engine's internal adoption route
  (which sets `allow_reserved_event_type`) may stamp it. This stops a public
  snapshot forging an adoption transition (which would skip begin-marker
  closure and diverge the telemetry stream).

  Scenario: Projection-only with event_type="spark" accepted
    Given a snapshot adapter
    When snapshot is executed with:
      | artifact_path   | sparks/sparks.md |
      | projection_only | true             |
      | event_type      | spark            |
    Then the snapshot result is successful

  Scenario: Projection-only with empty event_type rejected
    Given a snapshot adapter
    When snapshot is executed with:
      | artifact_path   | sparks/sparks.md |
      | projection_only | true             |
      | event_type      |                  |
    Then the snapshot result is an InvalidArgument error containing "event_type"

  Scenario: Projection-only with unknown event_type rejected
    Given a snapshot adapter
    When snapshot is executed with:
      | artifact_path   | sparks/sparks.md |
      | projection_only | true             |
      | event_type      | bogus            |
    Then the snapshot result is an InvalidArgument error containing "unknown event_type"

  Scenario: Non-projection-only with event_type set is accepted
    Given a snapshot adapter
    And the snapshot adapter has artifact "tracks/t1" of kind "track" in state "spec"
    And the snapshot adapter has existing registry entry for "t1" in "tracks.md"
    When snapshot is executed with:
      | artifact_path | tracks/t1       |
      | to_state      | spec_review     |
      | actor_name    | Actor-123       |
      | actor_role    | review          |
      | event_type    | spark           |
      | actor_type    | agent           |
      | actor_model   | claude-opus-4-7 |
      | actor_provider| anthropic       |
    Then the snapshot result is successful

  Scenario: Non-projection-only with reserved event_type "adoption" is rejected
    Given a snapshot adapter
    And the snapshot adapter has artifact "tracks/t1" of kind "track" in state "spec"
    And the snapshot adapter has existing registry entry for "t1" in "tracks.md"
    When snapshot is executed with:
      | artifact_path | tracks/t1       |
      | to_state      | spec_review     |
      | actor_name    | Actor-123       |
      | actor_role    | review          |
      | event_type    | adoption        |
      | actor_type    | agent           |
      | actor_model   | claude-opus-4-7 |
      | actor_provider| anthropic       |
    Then the snapshot result is an InvalidArgument error containing "reserved"
    And the snapshot adapter recorded no transitions

  Scenario: The internal adoption route may set event_type "adoption"
    Given a snapshot adapter
    And the snapshot adapter has artifact "tracks/t1" of kind "track" in state "spec"
    And the snapshot adapter has existing registry entry for "t1" in "tracks.md"
    When snapshot is executed with:
      | artifact_path             | tracks/t1       |
      | to_state                  | spec_review     |
      | actor_name                | Actor-123       |
      | actor_role                | review          |
      | event_type                | adoption        |
      | allow_reserved_event_type | true            |
      | actor_type                | agent           |
      | actor_model               | claude-opus-4-7 |
      | actor_provider            | anthropic       |
    Then the snapshot result is successful

  Scenario: Empty actor_type rejected as ActorParamsRequired
    Given a snapshot adapter
    And the snapshot adapter has artifact "tracks/t1" of kind "track" in state "spec"
    When snapshot is executed with:
      | artifact_path | tracks/t1       |
      | to_state      | spec_review     |
      | actor_name    | Actor-123       |
      | actor_role    | review          |
      | actor_type    |                 |
      | actor_model   | claude-opus-4-7 |
      | actor_provider| anthropic       |
    Then the snapshot result is an ActorParamsRequired error for "actor_type"
    And the snapshot adapter recorded no transitions

  Scenario: Empty actor_model rejected as ActorParamsRequired
    Given a snapshot adapter
    And the snapshot adapter has artifact "tracks/t1" of kind "track" in state "spec"
    When snapshot is executed with:
      | artifact_path | tracks/t1   |
      | to_state      | spec_review |
      | actor_name    | Actor-123   |
      | actor_role    | review      |
      | actor_type    | agent       |
      | actor_model   |             |
      | actor_provider| anthropic   |
    Then the snapshot result is an ActorParamsRequired error for "actor_model"
    And the snapshot adapter recorded no transitions

  Scenario: Empty actor_provider rejected as ActorParamsRequired
    Given a snapshot adapter
    And the snapshot adapter has artifact "tracks/t1" of kind "track" in state "spec"
    When snapshot is executed with:
      | artifact_path | tracks/t1       |
      | to_state      | spec_review     |
      | actor_name    | Actor-123       |
      | actor_role    | review          |
      | actor_type    | agent           |
      | actor_model   | claude-opus-4-7 |
      | actor_provider|                 |
    Then the snapshot result is an ActorParamsRequired error for "actor_provider"
    And the snapshot adapter recorded no transitions

  Scenario: Projection-only mode does not require identity fields
    Given a snapshot adapter
    When snapshot is executed with:
      | artifact_path   | sparks/sparks.md |
      | projection_only | true             |
      | event_type      | spark            |
    Then the snapshot result is successful
