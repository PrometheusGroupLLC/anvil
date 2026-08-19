Feature: Complete filesystem — reflection_notes omitted or empty is a no-op
  Covers spec R10.1(c) and (d): when reflection_notes is absent, empty, or
  whitespace-only, no reflection file is written and reflection_path is empty.

  Scenario: reflection_notes omitted — no reflection file, reflection_path empty
    Given a complete fs hearth with:
      | path                                                                 | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
      | tracks/20260420T0900_noop_omit_fs/status.yaml                        | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-900001:\n    type: agent\n    configurations:\n      - at: "2026-04-20T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-20T00:00:00Z\n    actor: Author-900001\n    role: spec\n    approver: mark\n                             |
      | tracks/20260420T0900_noop_omit_fs/spec.md                            | # Noop Omit FS\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
      | tracks.md                                                            | # Tracks\n\n## spec\n\n- [Noop Omit FS](tracks/20260420T0900_noop_omit_fs/) — noop omit fs — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                                                       |
      | projections/execution.md                                             | ---\nincremental_count: 0\nbase_snapshot: 2026-04-20T00:00:00Z\nlast_updated: 2026-04-20T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                                                                                                                     |
    When complete fs is executed with:
      | artifact_path         | tracks/20260420T0900_noop_omit_fs |
      | actor_name            | Doer-900001                       |
      | actor_type            | agent                             |
      | actor_model           | claude-opus-4-7                   |
      | actor_provider        | anthropic                         |
      | actor_context_window  | 200000                            |
      | actor_entrypoint      | claude-code                       |
      | at                    | 2026-04-20T09:00:00Z              |
    Then the complete result is successful
    And the complete result new_state is "spec_review"
    And the complete result reflection_path is empty
    And the directory "tracks/20260420T0900_noop_omit_fs/spec_reflection" does not exist

  Scenario: reflection_notes is empty string — no reflection file, reflection_path empty
    Given a complete fs hearth with:
      | path                                                                 | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
      | tracks/20260420T0901_noop_empty_fs/status.yaml                       | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-900002:\n    type: agent\n    configurations:\n      - at: "2026-04-20T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-20T00:00:00Z\n    actor: Author-900002\n    role: spec\n    approver: mark\n                             |
      | tracks/20260420T0901_noop_empty_fs/spec.md                           | # Noop Empty FS\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
      | tracks.md                                                            | # Tracks\n\n## spec\n\n- [Noop Empty FS](tracks/20260420T0901_noop_empty_fs/) — noop empty fs — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                                                    |
      | projections/execution.md                                             | ---\nincremental_count: 0\nbase_snapshot: 2026-04-20T00:00:00Z\nlast_updated: 2026-04-20T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                                                                                                                     |
    When complete fs is executed with:
      | artifact_path         | tracks/20260420T0901_noop_empty_fs |
      | actor_name            | Doer-900002                        |
      | actor_type            | agent                              |
      | actor_model           | claude-opus-4-7                    |
      | actor_provider        | anthropic                          |
      | actor_context_window  | 200000                             |
      | actor_entrypoint      | claude-code                        |
      | reflection_notes      |                                    |
      | at                    | 2026-04-20T09:01:00Z               |
    Then the complete result is successful
    And the complete result new_state is "spec_review"
    And the complete result reflection_path is empty
    And the directory "tracks/20260420T0901_noop_empty_fs/spec_reflection" does not exist

  Scenario: reflection_notes is whitespace-only — treated as empty, no reflection file
    # Gherkin table cells trim whitespace, so reflection_notes is injected via docstring
    # to deliver an actual tab-and-newline-only value that is distinct from empty string.
    Given a complete fs hearth with:
      | path                                                                 | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
      | tracks/20260420T0902_noop_whitespace_fs/status.yaml                  | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-900003:\n    type: agent\n    configurations:\n      - at: "2026-04-20T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-20T00:00:00Z\n    actor: Author-900003\n    role: spec\n    approver: mark\n                             |
      | tracks/20260420T0902_noop_whitespace_fs/spec.md                      | # Noop Whitespace FS\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
      | tracks.md                                                            | # Tracks\n\n## spec\n\n- [Noop Whitespace FS](tracks/20260420T0902_noop_whitespace_fs/) — noop whitespace fs — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                                     |
      | projections/execution.md                                             | ---\nincremental_count: 0\nbase_snapshot: 2026-04-20T00:00:00Z\nlast_updated: 2026-04-20T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                                                                                                                     |
    And the complete request has reflection_notes set to:
      """
        
      """
    When complete fs is executed with:
      | artifact_path         | tracks/20260420T0902_noop_whitespace_fs |
      | actor_name            | Doer-900003                             |
      | actor_type            | agent                                   |
      | actor_model           | claude-opus-4-7                         |
      | actor_provider        | anthropic                               |
      | actor_context_window  | 200000                                  |
      | actor_entrypoint      | claude-code                             |
      | at                    | 2026-04-20T09:02:00Z                    |
    Then the complete result is successful
    And the complete result new_state is "spec_review"
    And the complete result reflection_path is empty
    And the directory "tracks/20260420T0902_noop_whitespace_fs/spec_reflection" does not exist
