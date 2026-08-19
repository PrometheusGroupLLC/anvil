Feature: Complete filesystem — rejection paths short-circuit before reflection write
  Covers spec R10.1(i) and (j).
  When the complete call is rejected (wrong state, satisfaction out-of-scope, missing
  actor), the reflection write adapter is never called (call count == 0) and no
  reflection file or subdirectory is created.

  Scenario: Wrong state — doer-style call on spec_review short-circuits before reflection write (R10.1(i))
    Given a complete fs hearth with:
      | path                                                                     | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
      | tracks/20260420T0810_short_circuit_state/status.yaml                     | version: 1\nkind: track\nstate: spec_review\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-810001:\n    type: agent\n    configurations:\n      - at: "2026-04-20T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec_review\n    at: 2026-04-20T00:00:00Z\n    actor: Author-810001\n    role: spec\n    approver: mark\n                       |
      | tracks/20260420T0810_short_circuit_state/spec.md                         | # Short Circuit State\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
      | tracks.md                                                                | # Tracks\n\n## spec\n\n## spec_review\n\n- [Short Circuit State](tracks/20260420T0810_short_circuit_state/) — short circuit state — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                                          |
      | projections/execution.md                                                 | ---\nincremental_count: 0\nbase_snapshot: 2026-04-20T00:00:00Z\nlast_updated: 2026-04-20T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                                                                                                                               |
    And the reflection write adapter will fail on the next call with "should not be reached"
    When complete fs is executed with:
      | artifact_path        | tracks/20260420T0810_short_circuit_state |
      | actor_name           | Doer-810001                              |
      | actor_type           | agent                                    |
      | actor_model          | claude-opus-4-7                          |
      | actor_provider       | anthropic                                |
      | actor_context_window | 200000                                   |
      | actor_entrypoint     | claude-code                              |
      | reflection_notes     | irrelevant notes                         |
      | at                   | 2026-04-20T08:10:00Z                     |
    Then the complete result is a CompleteError containing "wrong_state_for_complete"
    And the reflection write adapter call count is 0
    And the directory "tracks/20260420T0810_short_circuit_state/spec_reflection" does not exist

  Scenario: Findings required — address_in_next_step without findings short-circuits before reflection write (R10.1(i))
    # Slice C: address_in_next_step is accepted, but absent findings the engine
    # rejects with findings_required_for_address_in_next_step BEFORE any reflection
    # emit. This is the mechanical guard for spec R6.2: reflection writes are never
    # attempted on rejected calls.
    Given a complete fs hearth with:
      | path                                                                     | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
      | tracks/20260420T0811_short_circuit_scope/status.yaml                     | version: 1\nkind: track\nstate: spec_review\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-811001:\n    type: agent\n    configurations:\n      - at: "2026-04-20T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec_review\n    at: 2026-04-20T00:00:00Z\n    actor: Author-811001\n    role: spec\n    approver: mark\n                               |
      | tracks/20260420T0811_short_circuit_scope/spec.md                         | # Short Circuit Scope\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
      | tracks.md                                                                | # Tracks\n\n## spec\n\n## spec_review\n\n- [Short Circuit Scope](tracks/20260420T0811_short_circuit_scope/) — short circuit scope — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                                                    |
      | projections/execution.md                                                 | ---\nincremental_count: 0\nbase_snapshot: 2026-04-20T00:00:00Z\nlast_updated: 2026-04-20T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                                                                                                                                       |
    And the reflection write adapter will fail on the next call with "should not be reached"
    When complete fs is executed with:
      | artifact_path        | tracks/20260420T0811_short_circuit_scope |
      | actor_name           | Reviewer-811001                          |
      | actor_type           | agent                                    |
      | actor_model          | claude-opus-4-7                          |
      | actor_provider       | anthropic                                |
      | actor_context_window | 200000                                   |
      | actor_entrypoint     | claude-code                              |
      | satisfaction         | address_in_next_step                     |
      | reflection_notes     | irrelevant notes                         |
      | at                   | 2026-04-20T08:11:00Z                     |
    Then the complete result is a CompleteError containing "findings_required_for_address_in_next_step"
    And the reflection write adapter call count is 0
    And the directory "tracks/20260420T0811_short_circuit_scope/spec_review_reflection" does not exist

  Scenario: Missing actor_name — short-circuits before reflection write (R10.1(j))
    Given a complete fs hearth with:
      | path                                                                     | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
      | tracks/20260420T0812_short_circuit_actor/status.yaml                     | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-812001:\n    type: agent\n    configurations:\n      - at: "2026-04-20T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-20T00:00:00Z\n    actor: Author-812001\n    role: spec\n    approver: mark\n                                    |
      | tracks/20260420T0812_short_circuit_actor/spec.md                         | # Short Circuit Actor\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
      | tracks.md                                                                | # Tracks\n\n## spec\n\n- [Short Circuit Actor](tracks/20260420T0812_short_circuit_actor/) — short circuit actor — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                                           |
      | projections/execution.md                                                 | ---\nincremental_count: 0\nbase_snapshot: 2026-04-20T00:00:00Z\nlast_updated: 2026-04-20T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                                                                                                                              |
    And the reflection write adapter will fail on the next call with "should not be reached"
    When complete fs is executed with:
      | artifact_path        | tracks/20260420T0812_short_circuit_actor |
      | actor_name           |                                          |
      | actor_type           | agent                                    |
      | actor_model          | claude-opus-4-7                          |
      | actor_provider       | anthropic                                |
      | actor_context_window | 200000                                   |
      | actor_entrypoint     | claude-code                              |
      | reflection_notes     | irrelevant notes                         |
      | at                   | 2026-04-20T08:12:00Z                     |
    Then the complete result is a CompleteError containing "actor_name_required"
    And the reflection write adapter call count is 0
    And the directory "tracks/20260420T0812_short_circuit_actor/spec_reflection" does not exist
