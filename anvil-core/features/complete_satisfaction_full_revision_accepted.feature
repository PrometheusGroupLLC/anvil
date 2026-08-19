Feature: Complete — full_revision is accepted, address_in_next_step is no longer out of scope
  Slice B no-regression guard (R8.1(h) / R6.4 / R6.5): once Slice B ships, the
  Slice A `satisfaction_out_of_scope` guard for `full_revision` is removed — the
  value is accepted and advances spec_review → spec_revision. Slice C lifts the
  guard for `address_in_next_step` too — it no longer returns
  `satisfaction_out_of_scope`; supplied without `findings` it returns
  `findings_required_for_address_in_next_step` instead.

  Scenario: full_revision on a spec_review track does NOT return satisfaction_out_of_scope
    Given a complete fs hearth with:
      | path                                                              | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
      | tracks/20260419T0600_fr_accepted/status.yaml                     | version: 1\nkind: track\nstate: spec_review\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-600001:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec_review\n    at: 2026-04-19T00:00:00Z\n    actor: Author-600001\n    role: spec\n    approver: mark\n |
      | tracks/20260419T0600_fr_accepted/spec.md                         | # FR Accepted\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
      | tracks.md                                                         | # Tracks\n\n## spec\n\n- [FR Accepted](tracks/20260419T0600_fr_accepted/) — fr accepted — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                                                              |
      | projections/execution.md                                          | ---\nincremental_count: 0\nbase_snapshot: 2026-04-19T00:00:00Z\nlast_updated: 2026-04-19T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec Review (1)\n\n- [FR Accepted](tracks/20260419T0600_fr_accepted/)\n\n## Spec Revision (0)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                                          |
    When complete fs is executed with:
      | artifact_path  | tracks/20260419T0600_fr_accepted |
      | actor_name     | Reviewer-600001                  |
      | actor_type     | agent                            |
      | actor_model    | claude-opus-4-7                  |
      | actor_provider | anthropic                        |
      | satisfaction   | full_revision                    |
      | at             | 2026-04-19T06:00:00Z             |
    Then the complete result is successful
    And the complete result new_state is "spec_revision"

  Scenario: address_in_next_step is no longer satisfaction_out_of_scope (Slice C shipped)
    Given a complete fs hearth with:
      | path                                                              | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
      | tracks/20260419T0601_ains_rejected/status.yaml                   | version: 1\nkind: track\nstate: spec_review\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-600002:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec_review\n    at: 2026-04-19T00:00:00Z\n    actor: Author-600002\n    role: spec\n    approver: mark\n |
    When complete fs is executed with:
      | artifact_path  | tracks/20260419T0601_ains_rejected |
      | actor_name     | Reviewer-600002                    |
      | actor_type     | agent                              |
      | actor_model    | claude-opus-4-7                    |
      | actor_provider | anthropic                          |
      | satisfaction   | address_in_next_step               |
      | at             | 2026-04-19T06:01:00Z               |
    # Slice C: the value is accepted; absent findings it now demands findings
    # rather than rejecting the value as out of scope.
    Then the complete result is a CompleteError containing "findings_required_for_address_in_next_step"
