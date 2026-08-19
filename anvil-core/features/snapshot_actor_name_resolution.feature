Feature: Snapshot actor-name resolution
  Per spec R3 of the checkin_backfill_spec_context track, the snapshot
  handler requires the caller to supply a non-empty `actor_name`. The
  engine no longer generates names; an empty `actor_name` returns
  `ActorNameRequired` and no filesystem state mutates.

  Scenario: Supplied actor_name is used verbatim
    Given a snapshot adapter
    And the snapshot adapter has artifact "tracks/t1" of kind "track" in state "spec"
    And the snapshot adapter has existing registry entry for "t1" in "tracks.md"
    When snapshot is executed with:
      | artifact_path | tracks/t1      |
      | to_state      | spec_review    |
      | actor_name    | Gliridae-12345 |
      | actor_role    | review         |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot result is successful
    And the snapshot result actor_name is "Gliridae-12345"
    And the snapshot adapter transition actor is "Gliridae-12345"

  Scenario: Empty actor_name returns ActorNameRequired
    Given a snapshot adapter
    And the snapshot adapter has artifact "tracks/t1" of kind "track" in state "spec"
    When snapshot is executed with:
      | artifact_path | tracks/t1   |
      | to_state      | spec_review |
      | actor_name    |             |
      | actor_role    | review      |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot result is an ActorNameRequired error
    And the snapshot adapter recorded no transitions
