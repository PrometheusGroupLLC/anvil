Feature: Complete filesystem — reviewer full_revision (spec_review → spec_revision)
  The CompleteCommandHandler exercises the reviewer full_revision path (Slice B):
  spec_review → spec_revision via FileSystemSnapshotAdapter and
  FileSystemActorWriteAdapter. Verifies status.yaml transition (role: review),
  actor seeding, and the consolidated registry section (spec_revision lives in
  the `## spec` registry section). Per the consolidated registry/projection
  model, transitions INTO a revision state skip the execution projection — the
  row stays in its under-review projection view. Per spec R1.

  Scenario: Reviewer complete with full_revision on spec_review track — records transition, seeds actor, moves registry and projection
    Given a complete fs hearth with:
      | path                                                              | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
      | tracks/20260419T0500_full_revision_track/status.yaml             | version: 1\nkind: track\nstate: spec_review\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-500001:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec_review\n    at: 2026-04-19T00:00:00Z\n    actor: Author-500001\n    role: spec\n    approver: mark\n |
      | tracks/20260419T0500_full_revision_track/spec.md                 | # Full Revision Track\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
      | tracks.md                                                         | # Tracks\n\n## spec\n\n- [Full Revision Track](tracks/20260419T0500_full_revision_track/) — full revision track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                                          |
      | projections/execution.md                                          | ---\nincremental_count: 0\nbase_snapshot: 2026-04-19T00:00:00Z\nlast_updated: 2026-04-19T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec Review (1)\n\n- [Full Revision Track](tracks/20260419T0500_full_revision_track/)\n\n## Spec Revision (0)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                          |
    When complete fs is executed with:
      | artifact_path         | tracks/20260419T0500_full_revision_track |
      | actor_name            | Reviewer-500001                          |
      | actor_type            | agent                                    |
      | actor_model           | claude-opus-4-7                          |
      | actor_provider        | anthropic                                |
      | actor_context_window  | 200000                                   |
      | actor_entrypoint      | claude-code                              |
      | satisfaction          | full_revision                            |
      | at                    | 2026-04-19T05:00:00Z                     |
    Then the complete result is successful
    And the complete result new_state is "spec_revision"
    And the complete result transition_at is "2026-04-19T05:00:00Z"
    And the complete result artifact_path is "tracks/20260419T0500_full_revision_track"
    And the resolved state of "tracks/20260419T0500_full_revision_track" is "spec_revision"
    And a transition event for "tracks/20260419T0500_full_revision_track" contains "to: spec_revision"
    And a transition event for "tracks/20260419T0500_full_revision_track" contains "role: review"
    And a transition event for "tracks/20260419T0500_full_revision_track" contains "actor: Reviewer-500001"
    And the file "tracks/20260419T0500_full_revision_track/status.yaml" contains "Reviewer-500001:"
    # Consolidated registry: spec_revision lives in the `## spec` section.
    And the file "tracks.md" contains "## spec"
    # Transitions into a revision state skip the execution projection (the row
    # stays in its prior under-review view).
    And the file "projections/execution.md" contains "## Spec Review (1)"

  Scenario: Reviewer full_revision with approver supplied — approver appears in the transition row
    Given a complete fs hearth with:
      | path                                                                  | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
      | tracks/20260419T0501_full_revision_approver/status.yaml              | version: 1\nkind: track\nstate: spec_review\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-500002:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec_review\n    at: 2026-04-19T00:00:00Z\n    actor: Author-500002\n    role: spec\n    approver: mark\n |
      | tracks/20260419T0501_full_revision_approver/spec.md                  | # Full Revision Approver\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
      | tracks.md                                                             | # Tracks\n\n## spec\n\n- [Full Revision Approver](tracks/20260419T0501_full_revision_approver/) — full revision approver — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                              |
      | projections/execution.md                                             | ---\nincremental_count: 0\nbase_snapshot: 2026-04-19T00:00:00Z\nlast_updated: 2026-04-19T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec Review (1)\n\n- [Full Revision Approver](tracks/20260419T0501_full_revision_approver/)\n\n## Spec Revision (0)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                      |
    When complete fs is executed with:
      | artifact_path         | tracks/20260419T0501_full_revision_approver |
      | actor_name            | Reviewer-500002                             |
      | actor_type            | agent                                       |
      | actor_model           | claude-opus-4-7                             |
      | actor_provider        | anthropic                                   |
      | satisfaction          | full_revision                               |
      | approver              | mark                                        |
      | at                    | 2026-04-19T05:01:00Z                        |
    Then the complete result is successful
    And the complete result new_state is "spec_revision"
    And a transition event for "tracks/20260419T0501_full_revision_approver" contains "to: spec_revision"
    And a transition event for "tracks/20260419T0501_full_revision_approver" contains "role: review"
    And a transition event for "tracks/20260419T0501_full_revision_approver" contains "approver: mark"
