Feature: Complete — actor write three-leg rule
  CompleteCommandHandler applies the uniform actor-write rule via
  FileSystemActorWriteAdapter on a seeded temp hearth. Three sub-scenarios
  per R8.1(f):
    1. add-if-absent: actor not yet in actors table → new entry created
    2. match-no-op: actor present with identical params → no extra entry
    3. mismatch-append: actor present with different model → configuration appended

  Scenario: add-if-absent — fresh actor written to status.yaml on doer complete
    Given a complete fs hearth with:
      | path                                                                     | content                                                                                                                                                                                                                                                                                                                                                           |
      | tracks/20260419T0450_three_leg_add_absent/status.yaml                   | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors: {}\ntransitions:\n  - to: spec\n    at: 2026-04-19T00:00:00Z\n    actor: Author-450001\n    role: spec\n    approver: mark\n                                                                                                                                            |
      | tracks.md                                                                | # Tracks\n\n## spec\n\n- [Three Leg Add Track](tracks/20260419T0450_three_leg_add_absent/) — three leg — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n\n## plan\n\n## implementing\n                                                                                                                                 |
      | projections/execution.md                                                 | ---\nincremental_count: 0\nbase_snapshot: 2026-04-19T00:00:00Z\nlast_updated: 2026-04-19T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                              |
    When complete fs is executed with:
      | artifact_path  | tracks/20260419T0450_three_leg_add_absent |
      | actor_name     | Doer-450001                               |
      | actor_type     | agent                                     |
      | actor_model    | claude-opus-4-7                           |
      | actor_provider | anthropic                                 |
      | at             | 2026-04-19T04:50:00Z                      |
    Then the complete result is successful
    And the file "tracks/20260419T0450_three_leg_add_absent/status.yaml" contains "Doer-450001:"
    And the file "tracks/20260419T0450_three_leg_add_absent/status.yaml" contains "model: claude-opus-4-7"

  Scenario: match-no-op — re-complete with same actor params leaves configurations unchanged
    Given a complete fs hearth with:
      | path                                                                     | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
      | tracks/20260419T0451_three_leg_match_noop/status.yaml                   | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Doer-450002:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-7\n        provider: anthropic\n        details:\n          context_window: 200000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-19T00:00:00Z\n    actor: Doer-450002\n    role: spec\n    approver: mark\n                          |
      | tracks.md                                                                | # Tracks\n\n## spec\n\n- [Three Leg Noop Track](tracks/20260419T0451_three_leg_match_noop/) — three leg — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n\n## plan\n\n## implementing\n                                                                                                                                                                                 |
      | projections/execution.md                                                 | ---\nincremental_count: 0\nbase_snapshot: 2026-04-19T00:00:00Z\nlast_updated: 2026-04-19T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                              |
    When complete fs is executed with:
      | artifact_path        | tracks/20260419T0451_three_leg_match_noop |
      | actor_name           | Doer-450002                               |
      | actor_type           | agent                                     |
      | actor_model          | claude-opus-4-7                           |
      | actor_provider       | anthropic                                 |
      | actor_context_window | 200000                                    |
      | actor_entrypoint     | claude-code                               |
      | at                   | 2026-04-19T04:51:00Z                      |
    Then the complete result is successful
    And the file "tracks/20260419T0451_three_leg_match_noop/status.yaml" contains "Doer-450002:"
    And the file "tracks/20260419T0451_three_leg_match_noop/status.yaml" contains "model: claude-opus-4-7"

  Scenario: mismatch-append — actor with different model gets a new configuration entry appended
    Given a complete fs hearth with:
      | path                                                                      | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
      | tracks/20260419T0452_three_leg_mismatch_append/status.yaml               | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Doer-450003:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 200000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-19T00:00:00Z\n    actor: Doer-450003\n    role: spec\n    approver: mark\n                          |
      | tracks.md                                                                 | # Tracks\n\n## spec\n\n- [Three Leg Append Track](tracks/20260419T0452_three_leg_mismatch_append/) — three leg — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n\n## plan\n\n## implementing\n                                                                                                                                                                        |
      | projections/execution.md                                                  | ---\nincremental_count: 0\nbase_snapshot: 2026-04-19T00:00:00Z\nlast_updated: 2026-04-19T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                              |
    When complete fs is executed with:
      | artifact_path  | tracks/20260419T0452_three_leg_mismatch_append |
      | actor_name     | Doer-450003                                    |
      | actor_type     | agent                                          |
      | actor_model    | claude-opus-4-7                                |
      | actor_provider | anthropic                                      |
      | at             | 2026-04-19T04:52:00Z                           |
    Then the complete result is successful
    And the file "tracks/20260419T0452_three_leg_mismatch_append/status.yaml" contains "model: claude-opus-4-6"
    And the file "tracks/20260419T0452_three_leg_mismatch_append/status.yaml" contains "model: claude-opus-4-7"
