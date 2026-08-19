Feature: Snapshot filesystem projection-only sparks update
  Projection-only mode rebuilds `projections/sparks.md` from the sparks
  source, updating the count line and frontmatter while preserving the
  narrative lines (including "Last reflection:").

  Scenario: Projection-only spark event rebuilds sparks.md counts and preserves narrative
    Given a snapshot fs hearth with:
      | path                              | content                                                                                                                                                                                                                                                                                                                        |
      | sparks/sparks.md                  | ## spark: one\nid: spark-001\n\n## spark: two\nid: spark-002\n\n## spark: three\nid: spark-003\n\n## disposition: one\ntarget: spark-001\n\n## annotation: foo\n                                                                                                                                                                 |
      | projections/sparks.md             | ---\nincremental_count: 4\nbase_snapshot: 2026-04-10T00:00:00Z\nlast_updated: 2026-04-10T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — Sparks\n\nUntriaged sparks: 3\nAnnotations: 0\n\nLast reflection: 2026-04-10T22:00:00Z                                                                                                    |
    When snapshot fs is executed with:
      | artifact_path   | sparks/sparks.md     |
      | projection_only | true                 |
      | event_type      | spark                |
      | at              | 2026-04-17T07:00:00Z |
    Then the snapshot result is successful
    And the snapshot result projections_updated contains "sparks.md"
    And the file "projections/sparks.md" contains "Untriaged sparks: 2"
    And the file "projections/sparks.md" contains "Annotations: 1"
    And the file "projections/sparks.md" contains "Last reflection: 2026-04-10T22:00:00Z"
    And the file "projections/sparks.md" contains "incremental_count: 5"
