Feature: Complete filesystem — reflection write failure is surfaced and write-ordering is preserved
  Covers spec R10.1(h) and (l).
  When the reflection-file write fails, the error is returned with the correct code and
  payload, and no transition is recorded (status.yaml, registry, projection are all
  byte-identical to pre-call state).

  Scenario: Write-ordering guarantee — status.yaml unchanged when reflection write fails (R10.1(h))
    Given a complete fs hearth with:
      | path                                                                     | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
      | tracks/20260420T0800_write_fail_order/status.yaml                        | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-800001:\n    type: agent\n    configurations:\n      - at: "2026-04-20T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-20T00:00:00Z\n    actor: Author-800001\n    role: spec\n    approver: mark\n                             |
      | tracks/20260420T0800_write_fail_order/spec.md                            | # Write Fail Order\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
      | tracks.md                                                                | # Tracks\n\n## spec\n\n- [Write Fail Order](tracks/20260420T0800_write_fail_order/) — write fail order — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                                         |
      | projections/execution.md                                                 | ---\nincremental_count: 0\nbase_snapshot: 2026-04-20T00:00:00Z\nlast_updated: 2026-04-20T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                                                                                                                     |
    And the reflection write adapter will fail on the next call with "simulated disk full"
    When complete fs is executed with:
      | artifact_path        | tracks/20260420T0800_write_fail_order |
      | actor_name           | Doer-800001                           |
      | actor_type           | agent                                 |
      | actor_model          | claude-opus-4-7                       |
      | actor_provider       | anthropic                             |
      | actor_context_window | 200000                                |
      | actor_entrypoint     | claude-code                           |
      | reflection_notes     | Noting for posterity.                 |
      | at                   | 2026-04-20T08:00:00Z                  |
    Then the complete result is a CompleteError containing "reflection_write_failed"
    And the complete result is a CompleteError containing "simulated disk full"
    And the complete result is a CompleteError containing "spec_reflection/"
    And the file "tracks/20260420T0800_write_fail_order/status.yaml" contains "state: spec"
    And the file "tracks/20260420T0800_write_fail_order/status.yaml" does not contain "state: spec_review"
    And the file "tracks.md" contains "20260420T0800_write_fail_order"
    And the directory "tracks/20260420T0800_write_fail_order/spec_reflection" is empty or absent

  Scenario: Write-failed error payload shape — error carries attempted path and I/O error (R10.1(l))
    Given a complete fs hearth with:
      | path                                                                     | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
      | tracks/20260420T0801_write_fail_payload/status.yaml                      | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-800002:\n    type: agent\n    configurations:\n      - at: "2026-04-20T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-20T00:00:00Z\n    actor: Author-800002\n    role: spec\n    approver: mark\n                                |
      | tracks/20260420T0801_write_fail_payload/spec.md                          | # Write Fail Payload\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
      | tracks.md                                                                | # Tracks\n\n## spec\n\n- [Write Fail Payload](tracks/20260420T0801_write_fail_payload/) — write fail payload — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                                        |
      | projections/execution.md                                                 | ---\nincremental_count: 0\nbase_snapshot: 2026-04-20T00:00:00Z\nlast_updated: 2026-04-20T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                                                                                                                        |
    And the reflection write adapter will fail on the next call with "simulated disk full"
    When complete fs is executed with:
      | artifact_path        | tracks/20260420T0801_write_fail_payload |
      | actor_name           | Doer-800002                             |
      | actor_type           | agent                                   |
      | actor_model          | claude-opus-4-7                         |
      | actor_provider       | anthropic                               |
      | actor_context_window | 200000                                  |
      | actor_entrypoint     | claude-code                             |
      | reflection_notes     | Notes for the failed write test.        |
      | at                   | 2026-04-20T08:01:00Z                    |
    Then the complete result is a CompleteError containing "reflection_write_failed"
    And the complete result is a CompleteError containing "spec_reflection/20260420T080100Z-Doer-800002.md"
    And the complete result is a CompleteError containing "simulated disk full"
