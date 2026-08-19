Feature: Snapshot projection-only mode
  Projection-only mode is used by spark capture / annotation flows. The
  snapshot handler skips all status.yaml and registry writes and only
  rebuilds the sparks projection.

  Scenario: Projection-only spark event updates only sparks.md
    Given a snapshot adapter
    When snapshot is executed with:
      | artifact_path   | sparks/sparks.md |
      | projection_only | true             |
      | event_type      | spark            |
    Then the snapshot result is successful
    And the snapshot result status_updated is "false"
    And the snapshot result registry_updated is "false"
    And the snapshot result projections_updated contains "sparks.md"
    And the snapshot adapter recorded no transitions
    And the snapshot adapter recorded no registry moves
    And the snapshot adapter recorded no registry creates
    And the snapshot adapter recorded no seeded actors
    And the snapshot adapter recorded 1 sparks rebuilds

  Scenario: Projection-only annotation event updates only sparks.md
    Given a snapshot adapter
    When snapshot is executed with:
      | artifact_path   | sparks/sparks.md |
      | projection_only | true             |
      | event_type      | annotation       |
    Then the snapshot result is successful
    And the snapshot result projections_updated contains "sparks.md"
    And the snapshot adapter recorded no transitions
    And the snapshot adapter recorded 1 sparks rebuilds
