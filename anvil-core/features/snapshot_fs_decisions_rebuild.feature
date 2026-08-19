Feature: Snapshot filesystem decisions projection rebuild
  Decision transitions rebuild the decisions projection: count line is
  regenerated from the registry's four state sections; Active tensions
  and Recently resolved lists are rebuilt; empty sections use `_None_`.
  Existing `base_snapshot` is preserved and `incremental_count` is
  incremented, per the checkpoint/compaction truth invariant — the
  engine never claims human-verified-base status.

  Scenario: Decision transition rebuilds counts, active tensions, recently resolved
    Given a snapshot fs hearth with:
      | path                                         | content                                                                                                                                                                                                                                                                                                                                                                                                        |
      | decisions/example-dec/status.yaml            | version: 1\nkind: decision\nstate: tension_review\nactors: {}\ntransitions: []\n                                                                                                                                                                                                                                                                                                                                 |
      | decisions/example-dec/definition.md          | # Example Decision\n\nBody.                                                                                                                                                                                                                                                                                                                                                                                      |
      | decisions.md                                 | # Decisions\n\n## tension\n\n- [Example Decision](decisions/example-dec/) — example decision — domain: x\n\n## investigating\n\n## decided\n\n- [Other Decision](decisions/other-dec/) — other decision — domain: y\n\n## retired\n                                                                                                                                                                            |
      | projections/decisions.md                     | ---\nincremental_count: 7\nbase_snapshot: 2026-04-10T00:00:00Z\nlast_updated: 2026-04-10T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — Decisions\n\nTension: 0 | Investigating: 0 | Decided: 0 | Retired: 0\n\n## Active tensions\n\n_None_\n\n## Recently resolved\n\n_None_\n                                                                                                                                    |
    When snapshot fs is executed with:
      | artifact_path  | decisions/example-dec |
      | to_state       | investigating         |
      | actor_name     | Decider-777777        |
      | actor_role     | decide                |
      | actor_type     | agent                 |
      | actor_model    | claude-opus-4-7       |
      | actor_provider | anthropic             |
      | at             | 2026-04-17T09:00:00Z  |
    Then the snapshot result is successful
    And the snapshot result projections_updated contains "decisions.md"
    And the file "projections/decisions.md" contains "Tension: 0 | Investigating: 1 | Decided: 1 | Retired: 0"
    And the file "projections/decisions.md" contains "- [Other Decision](decisions/other-dec/)"
    And the file "projections/decisions.md" contains "base_snapshot: 2026-04-10T00:00:00Z"
    And the file "projections/decisions.md" contains "incremental_count: 8"
    And the file "projections/decisions.md" does not contain "incremental_count: 0"
    And the file "projections/decisions.md" contains "_None_"
