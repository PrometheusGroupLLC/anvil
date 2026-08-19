Feature: Complete filesystem — reviewer satisfied (spec_review → plan)
  The CompleteCommandHandler exercises the reviewer-satisfied happy path:
  spec_review → plan transition via FileSystemSnapshotAdapter and
  FileSystemActorWriteAdapter. Verifies status.yaml, tracks.md, and
  execution.md side effects on a seeded temp hearth.

  Scenario: Reviewer complete with satisfied on spec_review track — updates status, registry, and execution projection
    Given a complete fs hearth with:
      | path                                                              | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
      | tracks/20260419T0200_reviewer_satisfied_track/status.yaml         | version: 1\nkind: track\nstate: spec_review\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-300001:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec_review\n    at: 2026-04-19T00:00:00Z\n    actor: Author-300001\n    role: spec\n    approver: mark\n |
      | tracks/20260419T0200_reviewer_satisfied_track/spec.md             | # Reviewer Satisfied Track\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
      | tracks.md                                                         | # Tracks\n\n## spec\n\n## spec_review\n\n- [Reviewer Satisfied Track](tracks/20260419T0200_reviewer_satisfied_track/) — reviewer satisfied track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                      |
      | projections/execution.md                                          | ---\nincremental_count: 0\nbase_snapshot: 2026-04-19T00:00:00Z\nlast_updated: 2026-04-19T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (1)\n\n- [Reviewer Satisfied Track](tracks/20260419T0200_reviewer_satisfied_track/)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                         |
    When complete fs is executed with:
      | artifact_path         | tracks/20260419T0200_reviewer_satisfied_track |
      | actor_name            | Reviewer-300001                               |
      | actor_type            | agent                                         |
      | actor_model           | claude-opus-4-7                               |
      | actor_provider        | anthropic                                     |
      | actor_context_window  | 200000                                        |
      | actor_entrypoint      | claude-code                                   |
      | satisfaction          | satisfied                                     |
      | at                    | 2026-04-19T02:00:00Z                          |
    Then the complete result is successful
    And the complete result new_state is "plan"
    And the complete result transition_at is "2026-04-19T02:00:00Z"
    And the complete result artifact_path is "tracks/20260419T0200_reviewer_satisfied_track"
    And the resolved state of "tracks/20260419T0200_reviewer_satisfied_track" is "plan"
    And a transition event for "tracks/20260419T0200_reviewer_satisfied_track" contains "to: plan"
    And a transition event for "tracks/20260419T0200_reviewer_satisfied_track" contains "role: review"
    And a transition event for "tracks/20260419T0200_reviewer_satisfied_track" contains "actor: Reviewer-300001"
    And the file "tracks/20260419T0200_reviewer_satisfied_track/status.yaml" contains "Reviewer-300001:"
    And the file "tracks.md" contains "## plan"
    And the file "tracks.md" does not contain "20260419T0200_reviewer_satisfied_track" under section "## spec_review"
    And the file "projections/execution.md" contains "## Planned (1)"
    And the file "projections/execution.md" contains "Reviewer Satisfied Track"

  Scenario: Reviewer complete with approver supplied — approver appears in status.yaml transition row
    Given a complete fs hearth with:
      | path                                                                   | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
      | tracks/20260419T0201_reviewer_with_approver_track/status.yaml          | version: 1\nkind: track\nstate: spec_review\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-300002:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec_review\n    at: 2026-04-19T00:00:00Z\n    actor: Author-300002\n    role: spec\n    approver: mark\n |
      | tracks/20260419T0201_reviewer_with_approver_track/spec.md              | # Reviewer With Approver Track\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
      | tracks.md                                                              | # Tracks\n\n## spec\n\n## spec_review\n\n- [Reviewer With Approver Track](tracks/20260419T0201_reviewer_with_approver_track/) — reviewer with approver track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                         |
      | projections/execution.md                                               | ---\nincremental_count: 0\nbase_snapshot: 2026-04-19T00:00:00Z\nlast_updated: 2026-04-19T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (1)\n\n- [Reviewer With Approver Track](tracks/20260419T0201_reviewer_with_approver_track/)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                 |
    When complete fs is executed with:
      | artifact_path         | tracks/20260419T0201_reviewer_with_approver_track |
      | actor_name            | Reviewer-300002                                   |
      | actor_type            | agent                                             |
      | actor_model           | claude-opus-4-7                                   |
      | actor_provider        | anthropic                                         |
      | actor_context_window  | 200000                                            |
      | actor_entrypoint      | claude-code                                       |
      | satisfaction          | satisfied                                         |
      | approver              | mark                                              |
      | at                    | 2026-04-19T02:01:00Z                              |
    Then the complete result is successful
    And the complete result new_state is "plan"
    And a transition event for "tracks/20260419T0201_reviewer_with_approver_track" contains "to: plan"
    And a transition event for "tracks/20260419T0201_reviewer_with_approver_track" contains "role: review"
    And a transition event for "tracks/20260419T0201_reviewer_with_approver_track" contains "approver: mark"
