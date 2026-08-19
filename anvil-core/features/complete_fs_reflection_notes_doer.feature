Feature: Complete filesystem — doer with reflection_notes writes reflection file
  Covers spec R10.1(a): doer-complete on `spec` with non-empty reflection_notes.
  Verifies the reflection file is created, its content, frontmatter, and that
  Slice A bookkeeping is unchanged.

  Scenario: Doer complete with reflection_notes — reflection file written, reflection_path populated
    Given a complete fs hearth with:
      | path                                                                 | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
      | tracks/20260420T0700_reflection_doer_fs/status.yaml                  | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-700001:\n    type: agent\n    configurations:\n      - at: "2026-04-20T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-20T00:00:00Z\n    actor: Author-700001\n    role: spec\n    approver: mark\n                             |
      | tracks/20260420T0700_reflection_doer_fs/spec.md                      | # Reflection Doer FS\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
      | tracks.md                                                            | # Tracks\n\n## spec\n\n- [Reflection Doer FS](tracks/20260420T0700_reflection_doer_fs/) — reflection doer fs — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                                     |
      | projections/execution.md                                             | ---\nincremental_count: 0\nbase_snapshot: 2026-04-20T00:00:00Z\nlast_updated: 2026-04-20T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                                                                                                                     |
    When complete fs is executed with:
      | artifact_path         | tracks/20260420T0700_reflection_doer_fs |
      | actor_name            | Doer-700001                             |
      | actor_type            | agent                                   |
      | actor_model           | claude-opus-4-7                         |
      | actor_provider        | anthropic                               |
      | actor_context_window  | 200000                                  |
      | actor_entrypoint      | claude-code                             |
      | reflection_notes      | Surprised that the registry move landed before projection; noting for next pass. |
      | at                    | 2026-04-20T07:00:00Z                    |
    Then the complete result is successful
    And the complete result new_state is "spec_review"
    And the complete result transition_at is "2026-04-20T07:00:00Z"
    And the complete result reflection_path ends with "spec_reflection/20260420T070000Z-Doer-700001.md"
    And the complete result reflection_path is non-empty
    And the file "tracks/20260420T0700_reflection_doer_fs/spec_reflection/20260420T070000Z-Doer-700001.md" contains "source_state: spec"
    And the file "tracks/20260420T0700_reflection_doer_fs/spec_reflection/20260420T070000Z-Doer-700001.md" contains "actor: Doer-700001"
    And the file "tracks/20260420T0700_reflection_doer_fs/spec_reflection/20260420T070000Z-Doer-700001.md" contains "at: 2026-04-20T07:00:00Z"
    And the file "tracks/20260420T0700_reflection_doer_fs/spec_reflection/20260420T070000Z-Doer-700001.md" does not contain "satisfaction:"
    And the file "tracks/20260420T0700_reflection_doer_fs/spec_reflection/20260420T070000Z-Doer-700001.md" contains "Surprised that the registry move landed before projection"
    And the resolved state of "tracks/20260420T0700_reflection_doer_fs" is "spec_review"
    And a transition event for "tracks/20260420T0700_reflection_doer_fs" contains "to: spec_review"
    And the file "tracks.md" contains "## spec_review"
    And the file "tracks.md" does not contain "20260420T0700_reflection_doer_fs" under section "## spec"
    And the file "projections/execution.md" contains "## Spec Review (1)"
