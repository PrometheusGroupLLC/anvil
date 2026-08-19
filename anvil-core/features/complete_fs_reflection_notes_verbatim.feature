Feature: Complete filesystem — reflection_notes verbatim preservation
  Covers spec R10.1(f): reflection_notes content is preserved verbatim after
  outer whitespace trim. Internal formatting, Markdown, Unicode, and trailing
  whitespace inside the body are all preserved character-for-character.

  Scenario: reflection_notes with trailing whitespace — outer trim only, inner preserved
    Given a complete fs hearth with:
      | path                                                                    | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
      | tracks/20260420T1000_verbatim_trim_fs/status.yaml                       | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-1000001:\n    type: agent\n    configurations:\n      - at: "2026-04-20T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-20T00:00:00Z\n    actor: Author-1000001\n    role: spec\n    approver: mark\n                         |
      | tracks/20260420T1000_verbatim_trim_fs/spec.md                           | # Verbatim Trim FS\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
      | tracks.md                                                               | # Tracks\n\n## spec\n\n- [Verbatim Trim FS](tracks/20260420T1000_verbatim_trim_fs/) — verbatim trim fs — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                                         |
      | projections/execution.md                                                | ---\nincremental_count: 0\nbase_snapshot: 2026-04-20T00:00:00Z\nlast_updated: 2026-04-20T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                                                                                                                   |
    When complete fs is executed with:
      | artifact_path         | tracks/20260420T1000_verbatim_trim_fs                 |
      | actor_name            | Doer-1000001                                          |
      | actor_type            | agent                                                 |
      | actor_model           | claude-opus-4-7                                       |
      | actor_provider        | anthropic                                             |
      | actor_context_window  | 200000                                                |
      | actor_entrypoint      | claude-code                                           |
      | reflection_notes      |   Line with internal trailing whitespace   \nSecond line   |
      | at                    | 2026-04-20T10:00:00Z                                  |
    Then the complete result is successful
    And the complete result new_state is "spec_review"
    And the file "tracks/20260420T1000_verbatim_trim_fs/spec_reflection/20260420T100000Z-Doer-1000001.md" contains "Line with internal trailing whitespace"
    And the file "tracks/20260420T1000_verbatim_trim_fs/spec_reflection/20260420T100000Z-Doer-1000001.md" contains "Second line"

  Scenario: reflection_notes ending without newline — engine adds exactly one trailing newline
    Given a complete fs hearth with:
      | path                                                                    | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
      | tracks/20260420T1001_verbatim_newline_fs/status.yaml                    | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-1001001:\n    type: agent\n    configurations:\n      - at: "2026-04-20T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-20T00:00:00Z\n    actor: Author-1001001\n    role: spec\n    approver: mark\n                         |
      | tracks/20260420T1001_verbatim_newline_fs/spec.md                        | # Verbatim Newline FS\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
      | tracks.md                                                               | # Tracks\n\n## spec\n\n- [Verbatim Newline FS](tracks/20260420T1001_verbatim_newline_fs/) — verbatim newline fs — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                                |
      | projections/execution.md                                                | ---\nincremental_count: 0\nbase_snapshot: 2026-04-20T00:00:00Z\nlast_updated: 2026-04-20T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                                                                                                                   |
    When complete fs is executed with:
      | artifact_path         | tracks/20260420T1001_verbatim_newline_fs |
      | actor_name            | Doer-1001001                             |
      | actor_type            | agent                                    |
      | actor_model           | claude-opus-4-7                          |
      | actor_provider        | anthropic                                |
      | actor_context_window  | 200000                                   |
      | actor_entrypoint      | claude-code                              |
      | reflection_notes      | Notes without trailing newline           |
      | at                    | 2026-04-20T10:01:00Z                     |
    Then the complete result is successful
    And the reflection file "tracks/20260420T1001_verbatim_newline_fs/spec_reflection/20260420T100100Z-Doer-1001001.md" ends with a newline

  Scenario: reflection_notes containing Markdown and a triple-dash separator — preserved verbatim
    Given a complete fs hearth with:
      | path                                                                    | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
      | tracks/20260420T1002_verbatim_markdown_fs/status.yaml                   | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-1002001:\n    type: agent\n    configurations:\n      - at: "2026-04-20T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-20T00:00:00Z\n    actor: Author-1002001\n    role: spec\n    approver: mark\n                         |
      | tracks/20260420T1002_verbatim_markdown_fs/spec.md                       | # Verbatim Markdown FS\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
      | tracks.md                                                               | # Tracks\n\n## spec\n\n- [Verbatim Markdown FS](tracks/20260420T1002_verbatim_markdown_fs/) — verbatim markdown fs — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                             |
      | projections/execution.md                                                | ---\nincremental_count: 0\nbase_snapshot: 2026-04-20T00:00:00Z\nlast_updated: 2026-04-20T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                                                                                                                   |
    When complete fs is executed with:
      | artifact_path         | tracks/20260420T1002_verbatim_markdown_fs |
      | actor_name            | Doer-1002001                              |
      | actor_type            | agent                                     |
      | actor_model           | claude-opus-4-7                           |
      | actor_provider        | anthropic                                 |
      | actor_context_window  | 200000                                    |
      | actor_entrypoint      | claude-code                               |
      | reflection_notes      | **Bold text** and `code`.\n\n---\n\nParagraph after separator. |
      | at                    | 2026-04-20T10:02:00Z                      |
    Then the complete result is successful
    And the file "tracks/20260420T1002_verbatim_markdown_fs/spec_reflection/20260420T100200Z-Doer-1002001.md" contains "**Bold text**"
    And the file "tracks/20260420T1002_verbatim_markdown_fs/spec_reflection/20260420T100200Z-Doer-1002001.md" contains "Paragraph after separator"

  Scenario: reflection_notes containing Unicode — emoji, non-ASCII, and CJK preserved verbatim
    Given a complete fs hearth with:
      | path                                                                    | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
      | tracks/20260420T1003_verbatim_unicode_fs/status.yaml                    | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-1003001:\n    type: agent\n    configurations:\n      - at: "2026-04-20T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-20T00:00:00Z\n    actor: Author-1003001\n    role: spec\n    approver: mark\n                         |
      | tracks/20260420T1003_verbatim_unicode_fs/spec.md                        | # Verbatim Unicode FS\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
      | tracks.md                                                               | # Tracks\n\n## spec\n\n- [Verbatim Unicode FS](tracks/20260420T1003_verbatim_unicode_fs/) — verbatim unicode fs — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                              |
      | projections/execution.md                                                | ---\nincremental_count: 0\nbase_snapshot: 2026-04-20T00:00:00Z\nlast_updated: 2026-04-20T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                                                                                                                   |
    When complete fs is executed with:
      | artifact_path         | tracks/20260420T1003_verbatim_unicode_fs   |
      | actor_name            | Doer-1003001                               |
      | actor_type            | agent                                      |
      | actor_model           | claude-opus-4-7                            |
      | actor_provider        | anthropic                                  |
      | actor_context_window  | 200000                                     |
      | actor_entrypoint      | claude-code                                |
      | reflection_notes      | 🚀 shipped. Résumé: 完成. naïve façade.    |
      | at                    | 2026-04-20T10:03:00Z                       |
    Then the complete result is successful
    And the file "tracks/20260420T1003_verbatim_unicode_fs/spec_reflection/20260420T100300Z-Doer-1003001.md" contains "🚀 shipped"
    And the file "tracks/20260420T1003_verbatim_unicode_fs/spec_reflection/20260420T100300Z-Doer-1003001.md" contains "Résumé"
    And the file "tracks/20260420T1003_verbatim_unicode_fs/spec_reflection/20260420T100300Z-Doer-1003001.md" contains "完成"
