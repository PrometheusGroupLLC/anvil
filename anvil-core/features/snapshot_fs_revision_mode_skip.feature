Feature: Snapshot filesystem revision-mode skips projection
  Revision-mode transitions (e.g., spec → spec_revision) update status
  and registry but leave projection files untouched — the projection
  already reflects the under-review view.

  Scenario: spec → spec_revision updates status + registry, leaves execution.md unchanged
    Given a snapshot fs hearth with:
      | path                                                      | content                                                                                                                                                                                                                                                                                                                                                                                                         |
      | tracks/20260417T0400_rev_track/status.yaml                | version: 1\nkind: track\nstate: spec_review\nproposal: 20260411T2021_anvil_workflow_engine\nactors: {}\ntransitions: []\n                                                                                                                                                                                                                                                                                         |
      | tracks/20260417T0400_rev_track/spec.md                    | # Rev Track                                                                                                                                                                                                                                                                                                                                                                                                     |
      | tracks.md                                                 | # Tracks\n\n## spec\n\n- [Rev Track](tracks/20260417T0400_rev_track/) — rev track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n                                                                                                                                                                                                                                                   |
      | projections/execution.md                                  | ---\nincremental_count: 42\nbase_snapshot: 2026-04-16T00:00:00Z\nlast_updated: 2026-04-16T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n                                                                                                                                                                                                                                                      |
    When snapshot fs is executed with:
      | artifact_path        | tracks/20260417T0400_rev_track |
      | to_state             | spec_revision                  |
      | actor_name           | Author-222222                  |
      | actor_role           | spec                           |
      | actor_type           | agent                          |
      | actor_model          | claude-opus-4-7                |
      | actor_provider       | anthropic                      |
      | actor_context_window | 1000000                        |
      | actor_sdk_version    | 0.2.111                        |
      | actor_entrypoint     | claude-desktop                 |
      | at                   | 2026-04-17T04:00:00Z           |
    Then the snapshot result is successful
    And the snapshot result status_updated is "true"
    And the snapshot result projections_updated is empty
    And the resolved state of "tracks/20260417T0400_rev_track" is "spec_revision"
    And the file "projections/execution.md" contains "incremental_count: 42"
    And the file "projections/execution.md" does not contain "spec-revision"
