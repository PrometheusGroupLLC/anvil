Feature: Snapshot filesystem status, registry, and execution projection
  The FileSystemSnapshotAdapter writes real status.yaml, registry, and
  projection files on disk. This feature exercises the happy path for a
  track `spec → spec_review` transition end-to-end against a seeded
  temp hearth and verifies the side effects by reading the files back.

  Scenario: Track spec → spec_review updates status, registry, and execution projection
    Given a snapshot fs hearth with:
      | path                                                              | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
      | tracks/20260417T0100_sample_track/status.yaml                     | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Aprilis-379075:\n    type: agent\n    configurations:\n      - at: "2026-04-17T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-17T00:00:00Z\n    actor: Aprilis-379075\n    role: spec\n    approver: mark\n              |
      | tracks/20260417T0100_sample_track/spec.md                         | # Sample Track\n\nExample body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
      | tracks.md                                                         | # Tracks\n\n## spec\n\n- [Sample Track](tracks/20260417T0100_sample_track/) — sample track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                                         |
      | projections/execution.md                                          | ---\nincremental_count: 0\nbase_snapshot: 2026-04-16T00:00:00Z\nlast_updated: 2026-04-16T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Plan (0)\n                                                                                                                                                                                                                                                                                                 |
    When snapshot fs is executed with:
      | artifact_path         | tracks/20260417T0100_sample_track |
      | to_state              | spec_review                       |
      | actor_name            | Reviewer-111111                   |
      | actor_role            | review                            |
      | actor_type            | agent                             |
      | actor_model           | claude-opus-4-7                   |
      | actor_provider        | anthropic                         |
      | actor_context_window  | 1000000                           |
      | actor_sdk_version     | 0.2.111                           |
      | actor_entrypoint      | claude-desktop                    |
      | at                    | 2026-04-17T01:00:00Z              |
    Then the snapshot result is successful
    And the snapshot result status_updated is "true"
    And the snapshot result registry_updated is "true"
    And the snapshot result projections_updated contains "execution.md"
    And the resolved state of "tracks/20260417T0100_sample_track" is "spec_review"
    And a transition event for "tracks/20260417T0100_sample_track" contains "actor: Reviewer-111111"
    And the file "tracks/20260417T0100_sample_track/status.yaml" contains "Reviewer-111111:"
    And the file "tracks.md" contains "## spec"
    And the file "projections/execution.md" contains "## Spec Review (1)"
    And the file "projections/execution.md" contains "Sample Track"

  Scenario: read_artifact_state resolves a stateless-but-transitioned artifact via the fallback
    Given a snapshot fs hearth with:
      | path                                        | content                                                                    |
      | decisions/event-format-choice/status.yaml    | version: 1\nkind: decision\ntransitions:\n  - to: tension\n  - to: decided |
    When snapshot fs read_artifact_state is called for "decisions/event-format-choice"
    Then the snapshot read state is "decided"

  Scenario: A transition records a per-file event and re-projects the top-level state line
    Given a snapshot fs hearth with:
      | path                                       | content                                                       |
      | decisions/event-format-choice/status.yaml   | version: 1\nkind: decision\ntransitions:\n  - to: tension     |
      | decisions/event-format-choice/definition.md | # Event Format Choice\n\nBody.                                 |
    When snapshot fs is executed with:
      | artifact_path         | decisions/event-format-choice |
      | to_state              | decided                       |
      | actor_name            | Reviewer-222222               |
      | actor_role            | decide                        |
      | actor_type            | agent                         |
      | actor_model           | claude-opus-4-7               |
      | actor_provider        | anthropic                     |
      | actor_context_window  | 1000000                       |
      | actor_sdk_version     | 0.2.111                       |
      | actor_entrypoint      | claude-desktop                |
      | at                    | 2026-04-17T01:00:00Z          |
    Then the snapshot result is successful
    And the file "decisions/event-format-choice/status.yaml" contains "state: decided"
    And a transition event for "decisions/event-format-choice" contains "to: decided"
    And the resolved state of "decisions/event-format-choice" is "decided"

  Scenario: read_artifact_state folds the event/transition tail over a stale top-level state
    Given a snapshot fs hearth with:
      | path                                       | content                                                                 |
      | tracks/20260417T0200_topstate/status.yaml   | version: 1\nkind: track\nstate: spec\ntransitions:\n  - to: spec\n  - to: plan |
    When snapshot fs read_artifact_state is called for "tracks/20260417T0200_topstate"
    Then the snapshot read state is "plan"
