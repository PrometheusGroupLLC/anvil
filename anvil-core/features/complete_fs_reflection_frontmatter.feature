Feature: Complete filesystem — reflection file frontmatter correctness
  Covers spec R10.1(g): the frontmatter block contains exactly the right keys.
  Doer-complete: source_state, actor, at (NO satisfaction).
  Reviewer-complete with satisfied: all four keys including satisfaction.

  Scenario: Doer-complete frontmatter — source_state actor at present, satisfaction absent
    Given a complete fs hearth with:
      | path                                                                      | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
      | tracks/20260420T1100_frontmatter_doer_fs/status.yaml                      | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-1100001:\n    type: agent\n    configurations:\n      - at: "2026-04-20T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-20T00:00:00Z\n    actor: Author-1100001\n    role: spec\n    approver: mark\n                             |
      | tracks/20260420T1100_frontmatter_doer_fs/spec.md                          | # Frontmatter Doer FS\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
      | tracks.md                                                                 | # Tracks\n\n## spec\n\n- [Frontmatter Doer FS](tracks/20260420T1100_frontmatter_doer_fs/) — frontmatter doer fs — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                                 |
      | projections/execution.md                                                  | ---\nincremental_count: 0\nbase_snapshot: 2026-04-20T00:00:00Z\nlast_updated: 2026-04-20T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                                                                                                                     |
    When complete fs is executed with:
      | artifact_path         | tracks/20260420T1100_frontmatter_doer_fs |
      | actor_name            | Doer-1100001                             |
      | actor_type            | agent                                    |
      | actor_model           | claude-opus-4-7                          |
      | actor_provider        | anthropic                                |
      | actor_context_window  | 200000                                   |
      | actor_entrypoint      | claude-code                              |
      | reflection_notes      | Doer observation.                        |
      | at                    | 2026-04-20T11:00:00Z                     |
    Then the complete result is successful
    And the file "tracks/20260420T1100_frontmatter_doer_fs/spec_reflection/20260420T110000Z-Doer-1100001.md" contains "source_state: spec"
    And the file "tracks/20260420T1100_frontmatter_doer_fs/spec_reflection/20260420T110000Z-Doer-1100001.md" contains "actor: Doer-1100001"
    And the file "tracks/20260420T1100_frontmatter_doer_fs/spec_reflection/20260420T110000Z-Doer-1100001.md" contains "at: 2026-04-20T11:00:00Z"
    And the file "tracks/20260420T1100_frontmatter_doer_fs/spec_reflection/20260420T110000Z-Doer-1100001.md" does not contain "satisfaction:"
    And the reflection file frontmatter "tracks/20260420T1100_frontmatter_doer_fs/spec_reflection/20260420T110000Z-Doer-1100001.md" starts with "---"
    And the reflection file frontmatter "tracks/20260420T1100_frontmatter_doer_fs/spec_reflection/20260420T110000Z-Doer-1100001.md" ends frontmatter before the body

  Scenario: Reviewer-complete with satisfied — frontmatter includes satisfaction: satisfied
    Given a complete fs hearth with:
      | path                                                                      | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
      | tracks/20260420T1101_frontmatter_reviewer_fs/status.yaml                  | version: 1\nkind: track\nstate: spec_review\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-1101001:\n    type: agent\n    configurations:\n      - at: "2026-04-20T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec_review\n    at: 2026-04-20T00:00:00Z\n    actor: Author-1101001\n    role: spec\n    approver: mark\n             |
      | tracks/20260420T1101_frontmatter_reviewer_fs/spec.md                      | # Frontmatter Reviewer FS\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
      | tracks.md                                                                 | # Tracks\n\n## spec\n\n## spec_review\n\n- [Frontmatter Reviewer FS](tracks/20260420T1101_frontmatter_reviewer_fs/) — frontmatter reviewer fs — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                    |
      | projections/execution.md                                                  | ---\nincremental_count: 0\nbase_snapshot: 2026-04-20T00:00:00Z\nlast_updated: 2026-04-20T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (1)\n\n- [Frontmatter Reviewer FS](tracks/20260420T1101_frontmatter_reviewer_fs/)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                                         |
    When complete fs is executed with:
      | artifact_path         | tracks/20260420T1101_frontmatter_reviewer_fs |
      | actor_name            | Reviewer-1101001                             |
      | actor_type            | agent                                        |
      | actor_model           | claude-opus-4-7                              |
      | actor_provider        | anthropic                                    |
      | actor_context_window  | 200000                                       |
      | actor_entrypoint      | claude-code                                  |
      | satisfaction          | satisfied                                    |
      | reflection_notes      | Reviewer observation.                        |
      | at                    | 2026-04-20T11:01:00Z                         |
    Then the complete result is successful
    And the file "tracks/20260420T1101_frontmatter_reviewer_fs/spec_review_reflection/20260420T110100Z-Reviewer-1101001.md" contains "source_state: spec_review"
    And the file "tracks/20260420T1101_frontmatter_reviewer_fs/spec_review_reflection/20260420T110100Z-Reviewer-1101001.md" contains "actor: Reviewer-1101001"
    And the file "tracks/20260420T1101_frontmatter_reviewer_fs/spec_review_reflection/20260420T110100Z-Reviewer-1101001.md" contains "at: 2026-04-20T11:01:00Z"
    And the file "tracks/20260420T1101_frontmatter_reviewer_fs/spec_review_reflection/20260420T110100Z-Reviewer-1101001.md" contains "satisfaction: satisfied"
