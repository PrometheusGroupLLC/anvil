Feature: Complete filesystem — reviewer-satisfied with reflection_notes
  Covers spec R10.1(b): reviewer-complete on `spec_review` with satisfaction
  "satisfied" and non-empty reflection_notes. Verifies the reflection file is
  created in spec_review_reflection/ with frontmatter including satisfaction.

  Scenario: Reviewer complete with satisfied and reflection_notes — reflection file in spec_review_reflection/
    Given a complete fs hearth with:
      | path                                                                      | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
      | tracks/20260420T0800_reflection_reviewer_fs/status.yaml                   | version: 1\nkind: track\nstate: spec_review\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-800001:\n    type: agent\n    configurations:\n      - at: "2026-04-20T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec_review\n    at: 2026-04-20T00:00:00Z\n    actor: Author-800001\n    role: spec\n    approver: mark\n     |
      | tracks/20260420T0800_reflection_reviewer_fs/spec.md                       | # Reflection Reviewer FS\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
      | tracks.md                                                                 | # Tracks\n\n## spec\n\n## spec_review\n\n- [Reflection Reviewer FS](tracks/20260420T0800_reflection_reviewer_fs/) — reflection reviewer fs — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                 |
      | projections/execution.md                                                  | ---\nincremental_count: 0\nbase_snapshot: 2026-04-20T00:00:00Z\nlast_updated: 2026-04-20T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (1)\n\n- [Reflection Reviewer FS](tracks/20260420T0800_reflection_reviewer_fs/)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                                     |
    When complete fs is executed with:
      | artifact_path         | tracks/20260420T0800_reflection_reviewer_fs |
      | actor_name            | Reviewer-800001                             |
      | actor_type            | agent                                       |
      | actor_model           | claude-opus-4-7                             |
      | actor_provider        | anthropic                                   |
      | actor_context_window  | 200000                                      |
      | actor_entrypoint      | claude-code                                 |
      | satisfaction          | satisfied                                   |
      | reflection_notes      | Review felt like a grammar pass — nothing structural caught me. |
      | at                    | 2026-04-20T08:00:00Z                        |
    Then the complete result is successful
    And the complete result new_state is "plan"
    And the complete result reflection_path ends with "spec_review_reflection/20260420T080000Z-Reviewer-800001.md"
    And the complete result reflection_path is non-empty
    And the file "tracks/20260420T0800_reflection_reviewer_fs/spec_review_reflection/20260420T080000Z-Reviewer-800001.md" contains "source_state: spec_review"
    And the file "tracks/20260420T0800_reflection_reviewer_fs/spec_review_reflection/20260420T080000Z-Reviewer-800001.md" contains "actor: Reviewer-800001"
    And the file "tracks/20260420T0800_reflection_reviewer_fs/spec_review_reflection/20260420T080000Z-Reviewer-800001.md" contains "at: 2026-04-20T08:00:00Z"
    And the file "tracks/20260420T0800_reflection_reviewer_fs/spec_review_reflection/20260420T080000Z-Reviewer-800001.md" contains "satisfaction: satisfied"
    And the file "tracks/20260420T0800_reflection_reviewer_fs/spec_review_reflection/20260420T080000Z-Reviewer-800001.md" contains "Review felt like a grammar pass"
    And the resolved state of "tracks/20260420T0800_reflection_reviewer_fs" is "plan"
    And the file "tracks.md" contains "## plan"
