Feature: Three-pillar reflect-pillar enumerability — reflection files assembable from directory listings
  Per spec R10.1(k) and spec AC "three-pillar-verifiable": the files produced
  by this track are enumerable without cross-referencing status.yaml.
  Slice B has not shipped, so this feature covers the two-file shape
  (spec + spec_review subdirectories). The @slice_b_shipped scenario
  (spec_revision) is conditional — see multi_actor_accumulation for that.

  Scenario: Two distinct reflection files in two sibling subdirectories — enumerable from filenames
    Given a complete fs hearth with:
      | path                                                                                      | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
      | tracks/20260420T0210_three_pillar/status.yaml                                             | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-3pillar01:\n    type: agent\n    configurations:\n      - at: "2026-04-20T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-20T00:00:00Z\n    actor: Author-3pillar01\n    role: spec\n    approver: mark\n              |
      | tracks/20260420T0210_three_pillar/spec.md                                                 | # Three Pillar Track\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
      | tracks.md                                                                                 | # Tracks\n\n## spec\n\n- [Three Pillar](tracks/20260420T0210_three_pillar/) — three pillar — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                                                     |
      | projections/execution.md                                                                  | ---\nincremental_count: 0\nbase_snapshot: 2026-04-20T00:00:00Z\nlast_updated: 2026-04-20T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (1)\n\n- [Three Pillar](tracks/20260420T0210_three_pillar/)\n\n## Spec Review (0)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                                                            |
    # Step 1: Doer completes spec, writes reflection file to spec_reflection/
    When complete fs is executed with:
      | artifact_path         | tracks/20260420T0210_three_pillar |
      | actor_name            | Doer-3pillar01                    |
      | actor_type            | agent                             |
      | actor_model           | claude-opus-4-7                   |
      | actor_provider        | anthropic                         |
      | actor_context_window  | 200000                            |
      | actor_entrypoint      | claude-code                       |
      | reflection_notes      | Doer pass: noticed the routing table was easy to read. |
      | at                    | 2026-04-20T10:00:00Z              |
    Then the complete result is successful
    And the complete result new_state is "spec_review"
    And the complete result reflection_path ends with "spec_reflection/20260420T100000Z-Doer-3pillar01.md"
    # The doer reflection file contains the expected frontmatter (enumerable from filename + frontmatter)
    And the file "tracks/20260420T0210_three_pillar/spec_reflection/20260420T100000Z-Doer-3pillar01.md" contains "source_state: spec"
    And the file "tracks/20260420T0210_three_pillar/spec_reflection/20260420T100000Z-Doer-3pillar01.md" contains "actor: Doer-3pillar01"
    And the file "tracks/20260420T0210_three_pillar/spec_reflection/20260420T100000Z-Doer-3pillar01.md" contains "at: 2026-04-20T10:00:00Z"
    And the file "tracks/20260420T0210_three_pillar/spec_reflection/20260420T100000Z-Doer-3pillar01.md" does not contain "satisfaction:"
    # Step 2: Reviewer completes spec_review with satisfied, writes reflection file to spec_review_reflection/
    # (The hearth is now in spec_review state from step 1)
    When complete fs is executed with:
      | artifact_path         | tracks/20260420T0210_three_pillar |
      | actor_name            | Reviewer-3pillar01                |
      | actor_type            | agent                             |
      | actor_model           | claude-opus-4-7                   |
      | actor_provider        | anthropic                         |
      | actor_context_window  | 200000                            |
      | actor_entrypoint      | claude-code                       |
      | satisfaction          | satisfied                         |
      | reflection_notes      | Reviewer pass: spec was clear. No structural issues. |
      | at                    | 2026-04-20T10:01:00Z              |
    Then the complete result is successful
    And the complete result new_state is "plan"
    And the complete result reflection_path ends with "spec_review_reflection/20260420T100100Z-Reviewer-3pillar01.md"
    # The reviewer reflection file is in a sibling subdirectory
    And the file "tracks/20260420T0210_three_pillar/spec_review_reflection/20260420T100100Z-Reviewer-3pillar01.md" contains "source_state: spec_review"
    And the file "tracks/20260420T0210_three_pillar/spec_review_reflection/20260420T100100Z-Reviewer-3pillar01.md" contains "actor: Reviewer-3pillar01"
    And the file "tracks/20260420T0210_three_pillar/spec_review_reflection/20260420T100100Z-Reviewer-3pillar01.md" contains "satisfaction: satisfied"
    # Enumerability assertion: the two reflection subdirectories are distinct siblings;
    # no cross-referencing of status.yaml is needed to identify which file belongs to which pass.
    And the directory "tracks/20260420T0210_three_pillar/spec_reflection" contains exactly 1 file
    And the directory "tracks/20260420T0210_three_pillar/spec_review_reflection" contains exactly 1 file
