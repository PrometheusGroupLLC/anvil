Feature: Complete filesystem status, registry, and execution projection
  The CompleteCommandHandler exercises the doer-complete happy path:
  spec → spec_review transition via FileSystemSnapshotAdapter and
  FileSystemActorWriteAdapter. Verifies status.yaml, tracks.md, and
  execution.md side effects on a seeded temp hearth.

  Scenario: Doer complete on spec track — updates status, registry, and execution projection
    Given a complete fs hearth with:
      | path                                                              | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
      | tracks/20260419T0100_complete_track/status.yaml                   | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-100001:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-19T00:00:00Z\n    actor: Author-100001\n    role: spec\n    approver: mark\n              |
      | tracks/20260419T0100_complete_track/spec.md                       | # Complete Track\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
      | tracks.md                                                         | # Tracks\n\n## spec\n\n- [Complete Track](tracks/20260419T0100_complete_track/) — complete track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                                   |
      | projections/execution.md                                          | ---\nincremental_count: 0\nbase_snapshot: 2026-04-19T00:00:00Z\nlast_updated: 2026-04-19T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                                                                                                        |
    When complete fs is executed with:
      | artifact_path         | tracks/20260419T0100_complete_track |
      | actor_name            | Doer-200001                         |
      | actor_type            | agent                               |
      | actor_model           | claude-opus-4-7                     |
      | actor_provider        | anthropic                           |
      | actor_context_window  | 200000                              |
      | actor_entrypoint      | claude-code                         |
      | at                    | 2026-04-19T01:00:00Z                |
    Then the complete result is successful
    And the complete result new_state is "spec_review"
    And the complete result transition_at is "2026-04-19T01:00:00Z"
    And the complete result artifact_path is "tracks/20260419T0100_complete_track"
    And the resolved state of "tracks/20260419T0100_complete_track" is "spec_review"
    And a transition event for "tracks/20260419T0100_complete_track" contains "to: spec_review"
    And the file "tracks/20260419T0100_complete_track/status.yaml" contains "role: spec"
    And a transition event for "tracks/20260419T0100_complete_track" contains "actor: Doer-200001"
    And the file "tracks/20260419T0100_complete_track/status.yaml" contains "Doer-200001:"
    And the file "tracks.md" contains "## spec_review"
    And the file "tracks.md" does not contain "20260419T0100_complete_track" under section "## spec"
    And the file "projections/execution.md" contains "## Spec Review (1)"
    And the file "projections/execution.md" contains "Complete Track"

  Scenario: Doer complete on review spec strand places registry entry under spec_review
    Given a complete fs hearth with:
      | path                                                              | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
      | tracks/20260414T0405_review_spec_strand/status.yaml               | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-100001:\n    type: agent\n    configurations:\n      - at: "2026-04-14T04:05:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-14T04:05:00Z\n    actor: Author-100001\n    role: spec\n    approver: mark\n              |
      | tracks/20260414T0405_review_spec_strand/spec.md                   | # Review Spec Strand\n\nExample spec body for E2E.                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
      | tracks.md                                                         | # Tracks\n\n## spec\n\n- [review spec strand](tracks/20260414T0405_review_spec_strand/) — review spec strand — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n\n## plan\n\n## completed\n                                                                                                                                                                                                                                                                        |
      | projections/execution.md                                          | ---\nincremental_count: 0\nbase_snapshot: 2026-04-14T04:05:00Z\nlast_updated: 2026-04-14T04:05:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Planned (0)\n\n## Completed (0)\n                                                                                                                                                                                                                                                                          |
    When complete fs is executed with:
      | artifact_path         | tracks/20260414T0405_review_spec_strand |
      | actor_name            | Doer-200001                             |
      | actor_type            | agent                                   |
      | actor_model           | claude-opus-4-7                         |
      | actor_provider        | anthropic                               |
      | actor_context_window  | 200000                                  |
      | actor_entrypoint      | claude-code                             |
      | at                    | 2026-04-14T04:06:00Z                    |
    Then the complete result is successful
    And the complete result new_state is "spec_review"
    And the file "tracks.md" contains "20260414T0405_review_spec_strand" under section "## spec_review"
    And the file "tracks.md" does not contain "20260414T0405_review_spec_strand" under section "## spec"
    And the file "projections/execution.md" contains "## Spec Review (1)"

  Scenario: Doer complete on a stateless-but-transitioned track resolves state via the fallback
    Given a complete fs hearth with:
      | path                                                              | content                                                                                                                                                                                                                                                                                                                                                                                                          |
      | tracks/20260419T0200_stateless_track/status.yaml                  | version: 1\nkind: track\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-100001:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-19T00:00:00Z\n    actor: Author-100001\n    role: spec\n    approver: mark\n |
      | tracks/20260419T0200_stateless_track/spec.md                      | # Stateless Track\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                   |
      | tracks.md                                                         | # Tracks\n\n## spec\n\n- [Stateless Track](tracks/20260419T0200_stateless_track/) — stateless track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n\n## plan\n\n## implementing\n                                                                                                                                                                                       |
      | projections/execution.md                                          | ---\nincremental_count: 0\nbase_snapshot: 2026-04-19T00:00:00Z\nlast_updated: 2026-04-19T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                              |
    When complete fs is executed with:
      | artifact_path         | tracks/20260419T0200_stateless_track |
      | actor_name            | Doer-200001                          |
      | actor_type            | agent                                |
      | actor_model           | claude-opus-4-7                      |
      | actor_provider        | anthropic                            |
      | actor_context_window  | 200000                               |
      | actor_entrypoint      | claude-code                          |
      | at                    | 2026-04-19T01:00:00Z                 |
    Then the complete result is successful
    And the complete result new_state is "spec_review"
