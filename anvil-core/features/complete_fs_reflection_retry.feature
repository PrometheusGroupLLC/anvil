Feature: Complete filesystem — retry after reflection write failure uses fresh timestamp (spec R2.5)
  When the reflection write fails on the first attempt, no state is committed.
  A subsequent retry (fresh call) samples a new `at` timestamp, producing a distinct
  filename, and succeeds if the fault has been resolved. The engine does not attempt
  to clean up any file from the first attempt (spec R4.3).

  Scenario: Retry after reflection_write_failed succeeds with a fresh timestamp
    Given a complete fs hearth with:
      | path                                                                      | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
      | tracks/20260420T0820_retry_after_fail/status.yaml                         | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-820001:\n    type: agent\n    configurations:\n      - at: "2026-04-20T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-20T00:00:00Z\n    actor: Author-820001\n    role: spec\n    approver: mark\n                              |
      | tracks/20260420T0820_retry_after_fail/spec.md                             | # Retry After Fail\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
      | tracks.md                                                                 | # Tracks\n\n## spec\n\n- [Retry After Fail](tracks/20260420T0820_retry_after_fail/) — retry after fail — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                                          |
      | projections/execution.md                                                  | ---\nincremental_count: 0\nbase_snapshot: 2026-04-20T00:00:00Z\nlast_updated: 2026-04-20T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                                                                                                                      |
    And the reflection write adapter will fail on the next call with "transient disk error"
    When complete fs is executed with:
      | artifact_path        | tracks/20260420T0820_retry_after_fail |
      | actor_name           | Doer-820001                           |
      | actor_type           | agent                                 |
      | actor_model          | claude-opus-4-7                       |
      | actor_provider       | anthropic                             |
      | actor_context_window | 200000                                |
      | actor_entrypoint     | claude-code                           |
      | reflection_notes     | first try                             |
      | at                   | 2026-04-20T08:20:00Z                  |
    Then the complete result is a CompleteError containing "reflection_write_failed"
    And the complete result is a CompleteError containing "transient disk error"
    And the complete result is a CompleteError containing "20260420T082000Z-Doer-820001.md"
    And the file "tracks/20260420T0820_retry_after_fail/status.yaml" contains "state: spec"
    # Second attempt — fault cleared, adapter not in context, filesystem adapter used.
    # The engine samples `at` anew (represented here by a distinct fixture timestamp).
    # The second attempt's filename will differ from the first attempt's path.
    When complete fs is executed with:
      | artifact_path        | tracks/20260420T0820_retry_after_fail |
      | actor_name           | Doer-820001                           |
      | actor_type           | agent                                 |
      | actor_model          | claude-opus-4-7                       |
      | actor_provider       | anthropic                             |
      | actor_context_window | 200000                                |
      | actor_entrypoint     | claude-code                           |
      | reflection_notes     | first try                             |
      | at                   | 2026-04-20T08:20:01Z                  |
    Then the complete result is successful
    And the complete result new_state is "spec_review"
    And the complete result reflection_path ends with "spec_reflection/20260420T082001Z-Doer-820001.md"
    And the resolved state of "tracks/20260420T0820_retry_after_fail" is "spec_review"
    # The second attempt wrote its file. Verify the retry file content.
    # The first attempt never wrote bytes (the FaultyReflectionWriteAdapter returned the
    # error before writing), so no cleanup was needed — spec R4.3 holds trivially.
    And the file "tracks/20260420T0820_retry_after_fail/spec_reflection/20260420T082001Z-Doer-820001.md" contains "source_state: spec"
    And the file "tracks/20260420T0820_retry_after_fail/spec_reflection/20260420T082001Z-Doer-820001.md" contains "actor: Doer-820001"
