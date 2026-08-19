Feature: Multi-actor accumulation — multiple reflection files at the same source state
  Per spec R10.1(e): multiple complete calls on the same artifact at the
  same source state each produce a distinct reflection file. Files accumulate
  rather than overwrite.
  Slice B has not shipped, so the cross-artifact variant is used for
  two-reviewer accumulation at spec_review.
  The @slice_b_shipped scenario (e2, spec_revision round-trip) is tagged
  @pending and will be enabled when Slice B lands.

  Scenario: Two actors complete separate spec_review tracks — two distinct reflection files in separate artifacts
    # Narrowed per plan: Slice B not shipped → accumulation tested cross-artifact
    # (two distinct spec_review tracks each receiving one reviewer-complete with reflection_notes).
    # When Slice B ships, a full in-track accumulation scenario becomes possible.
    Given a complete fs hearth with:
      | path                                                                                        | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
      | tracks/20260420T0210_multi_accum_a/status.yaml                                              | version: 1\nkind: track\nstate: spec_review\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-accumA01:\n    type: agent\n    configurations:\n      - at: "2026-04-20T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec_review\n    at: 2026-04-20T00:00:00Z\n    actor: Author-accumA01\n    role: spec\n    approver: mark\n |
      | tracks/20260420T0210_multi_accum_a/spec.md                                                  | # Multi Accum A\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
      | tracks/20260420T0210_multi_accum_b/status.yaml                                              | version: 1\nkind: track\nstate: spec_review\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-accumB01:\n    type: agent\n    configurations:\n      - at: "2026-04-20T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec_review\n    at: 2026-04-20T00:00:00Z\n    actor: Author-accumB01\n    role: spec\n    approver: mark\n |
      | tracks/20260420T0210_multi_accum_b/spec.md                                                  | # Multi Accum B\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
      | tracks.md                                                                                   | # Tracks\n\n## spec\n\n## spec_review\n\n- [Multi Accum A](tracks/20260420T0210_multi_accum_a/) — multi accum a — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n- [Multi Accum B](tracks/20260420T0210_multi_accum_b/) — multi accum b — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## plan\n\n## implementing\n                                                                                                                                                      |
      | projections/execution.md                                                                    | ---\nincremental_count: 0\nbase_snapshot: 2026-04-20T00:00:00Z\nlast_updated: 2026-04-20T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (2)\n\n- [Multi Accum A](tracks/20260420T0210_multi_accum_a/)\n- [Multi Accum B](tracks/20260420T0210_multi_accum_b/)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                     |
    # Reviewer A completes track A
    When complete fs is executed with:
      | artifact_path         | tracks/20260420T0210_multi_accum_a |
      | actor_name            | ReviewerA-accum01                  |
      | actor_type            | agent                              |
      | actor_model           | claude-opus-4-7                    |
      | actor_provider        | anthropic                          |
      | actor_context_window  | 200000                             |
      | actor_entrypoint      | claude-code                        |
      | satisfaction          | satisfied                          |
      | reflection_notes      | Reviewer A: track A was clean.     |
      | at                    | 2026-04-20T10:10:00Z               |
    Then the complete result is successful
    And the complete result reflection_path ends with "spec_review_reflection/20260420T101000Z-ReviewerA-accum01.md"
    And the file "tracks/20260420T0210_multi_accum_a/spec_review_reflection/20260420T101000Z-ReviewerA-accum01.md" contains "actor: ReviewerA-accum01"
    # Reviewer B completes track B — distinct file, distinct artifact
    When complete fs is executed with:
      | artifact_path         | tracks/20260420T0210_multi_accum_b |
      | actor_name            | ReviewerB-accum01                  |
      | actor_type            | agent                              |
      | actor_model           | claude-opus-4-7                    |
      | actor_provider        | anthropic                          |
      | actor_context_window  | 200000                             |
      | actor_entrypoint      | claude-code                        |
      | satisfaction          | satisfied                          |
      | reflection_notes      | Reviewer B: track B was thorough.  |
      | at                    | 2026-04-20T10:11:00Z               |
    Then the complete result is successful
    And the complete result reflection_path ends with "spec_review_reflection/20260420T101100Z-ReviewerB-accum01.md"
    And the file "tracks/20260420T0210_multi_accum_b/spec_review_reflection/20260420T101100Z-ReviewerB-accum01.md" contains "actor: ReviewerB-accum01"
    # Both files are distinct — different timestamps, different actors
    And the file "tracks/20260420T0210_multi_accum_a/spec_review_reflection/20260420T101000Z-ReviewerA-accum01.md" contains "at: 2026-04-20T10:10:00Z"
    And the file "tracks/20260420T0210_multi_accum_b/spec_review_reflection/20260420T101100Z-ReviewerB-accum01.md" contains "at: 2026-04-20T10:11:00Z"
    And the directory "tracks/20260420T0210_multi_accum_a/spec_review_reflection" contains exactly 1 file
    And the directory "tracks/20260420T0210_multi_accum_b/spec_review_reflection" contains exactly 1 file

  # @pending Scenario: Two doer-completes on spec_revision accumulate two files
  # (spec R10.1(e2)) — conditional on Slice B (full_revision path) shipping.
  # Enable by adding the scenario back when Slice B is on main.
