Feature: Snapshot criticality ordering
  Per spec §2 the handler writes in criticality order: status.yaml first
  (fail-fast), then registry (warn-and-continue on failure), then
  projection (warn-and-continue on failure). A status-append failure
  short-circuits — no registry or projection call is made.

  Scenario: Status-append failure short-circuits
    Given a snapshot adapter
    And the snapshot adapter has artifact "tracks/t1" of kind "track" in state "spec"
    And the snapshot adapter has existing registry entry for "t1" in "tracks.md"
    And the snapshot adapter will fail status append
    When snapshot is executed with:
      | artifact_path | tracks/t1   |
      | to_state      | plan        |
      | actor_name    | Actor-123   |
      | actor_role    | plan        |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot result is an IoError
    And the snapshot adapter recorded no registry moves
    And the snapshot adapter recorded no registry creates

  Scenario: Registry-move failure adds warning but continues
    Given a snapshot adapter
    And the snapshot adapter has artifact "tracks/t1" of kind "track" in state "spec"
    And the snapshot adapter has existing registry entry for "t1" in "tracks.md"
    And the snapshot adapter will fail registry move
    When snapshot is executed with:
      | artifact_path | tracks/t1   |
      | to_state      | spec_review |
      | actor_name    | Actor-123   |
      | actor_role    | review      |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot result is successful
    And the snapshot result registry_updated is "false"
    And the snapshot result warnings contain "registry update failed"
    And the snapshot result projections_updated contains "execution.md"

  Scenario: Projection-update failure adds warning but returns success
    Given a snapshot adapter
    And the snapshot adapter has artifact "tracks/t1" of kind "track" in state "spec"
    And the snapshot adapter has existing registry entry for "t1" in "tracks.md"
    And the snapshot adapter will fail projection update
    When snapshot is executed with:
      | artifact_path | tracks/t1   |
      | to_state      | spec_review |
      | actor_name    | Actor-123   |
      | actor_role    | review      |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot result is successful
    And the snapshot result registry_updated is "true"
    And the snapshot result warnings contain "projection update failed"
