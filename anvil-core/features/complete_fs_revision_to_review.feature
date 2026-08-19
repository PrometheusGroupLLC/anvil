Feature: Complete filesystem — doer revision done (spec_revision → spec_review)
  The CompleteCommandHandler exercises the doer revision-done path (Slice B):
  a doer's `complete` with no satisfaction on a spec_revision track advances it
  back to spec_review with role "spec" and no approver. Per spec R3.

  Scenario: Doer complete with no satisfaction on spec_revision track — advances to spec_review
    Given a complete fs hearth with:
      | path                                                              | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
      | tracks/20260419T0700_revision_done_track/status.yaml             | version: 1\nkind: track\nstate: spec_revision\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-700001:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec_revision\n    at: 2026-04-19T00:00:00Z\n    actor: Reviewer-700000\n    role: review\n    approver: mark\n |
      | tracks/20260419T0700_revision_done_track/spec.md                 | # Revision Done Track\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
      | tracks.md                                                         | # Tracks\n\n## spec\n\n- [Revision Done Track](tracks/20260419T0700_revision_done_track/) — revision done track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                                          |
      | projections/execution.md                                          | ---\nincremental_count: 0\nbase_snapshot: 2026-04-19T00:00:00Z\nlast_updated: 2026-04-19T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec Review (1)\n\n- [Revision Done Track](tracks/20260419T0700_revision_done_track/)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                            |
    When complete fs is executed with:
      | artifact_path         | tracks/20260419T0700_revision_done_track |
      | actor_name            | Doer-700001                              |
      | actor_type            | agent                                    |
      | actor_model           | claude-opus-4-7                          |
      | actor_provider        | anthropic                                |
      | actor_context_window  | 200000                                   |
      | actor_entrypoint      | claude-code                              |
      | at                    | 2026-04-19T07:00:00Z                     |
    Then the complete result is successful
    And the complete result new_state is "spec_review"
    And the complete result transition_at is "2026-04-19T07:00:00Z"
    And the complete result artifact_path is "tracks/20260419T0700_revision_done_track"
    And the resolved state of "tracks/20260419T0700_revision_done_track" is "spec_review"
    And a transition event for "tracks/20260419T0700_revision_done_track" contains "to: spec_review"
    And a transition event for "tracks/20260419T0700_revision_done_track" contains "role: spec"
    And a transition event for "tracks/20260419T0700_revision_done_track" contains "actor: Doer-700001"
    And the file "tracks/20260419T0700_revision_done_track/status.yaml" contains "Doer-700001:"
    # spec_review IS a projected state; the row appears under Spec Review.
    And the file "projections/execution.md" contains "## Spec Review"
    And the file "projections/execution.md" contains "Revision Done Track"
