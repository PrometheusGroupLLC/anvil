Feature: Complete — address_in_next_step is no longer satisfaction out of scope
  Slice C ships the carry-forward path, so the Slice A scope-naming rejection
  (`satisfaction_out_of_scope`) is no longer returned for `address_in_next_step`
  (R1.4 / R7.1(f)). Supplied WITH findings the value succeeds; supplied WITHOUT
  findings it returns `findings_required_for_address_in_next_step`. Neither path
  returns `satisfaction_out_of_scope`. This feature is the regression guard
  preventing the Slice A rejection block from being resurrected.

  Scenario: address_in_next_step with findings succeeds and does NOT return satisfaction_out_of_scope
    Given a complete fs hearth with:
      | path                                                                     | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
      | tracks/20260419T0411_no_longer_out_of_scope/status.yaml                  | version: 1\nkind: track\nstate: spec_review\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-410002:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec_review\n    at: 2026-04-19T00:00:00Z\n    actor: Author-410002\n    role: spec\n    approver: mark\n |
      | tracks/20260419T0411_no_longer_out_of_scope/spec.md                      | # No Longer Out Of Scope\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
      | tracks.md                                                                | # Tracks\n\n## spec\n\n## spec_review\n\n- [No Longer Out Of Scope](tracks/20260419T0411_no_longer_out_of_scope/) — no longer out of scope — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                          |
      | projections/execution.md                                                 | ---\nincremental_count: 0\nbase_snapshot: 2026-04-19T00:00:00Z\nlast_updated: 2026-04-19T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (1)\n\n- [No Longer Out Of Scope](tracks/20260419T0411_no_longer_out_of_scope/)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                              |
    When complete fs is executed with:
      | artifact_path  | tracks/20260419T0411_no_longer_out_of_scope |
      | actor_name     | Reviewer-410002                             |
      | actor_type     | agent                                       |
      | actor_model    | claude-opus-4-7                             |
      | actor_provider | anthropic                                   |
      | satisfaction   | address_in_next_step                        |
      | findings       | Confirm X and Y during implementation.      |
      | at             | 2026-04-19T04:11:00Z                        |
    Then the complete result is successful
    And the complete result new_state is "plan"

  Scenario: address_in_next_step without findings returns findings_required, not satisfaction_out_of_scope
    Given a complete fs hearth with:
      | path                                                                     | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
      | tracks/20260419T0412_no_out_of_scope_no_findings/status.yaml             | version: 1\nkind: track\nstate: spec_review\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-410003:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec_review\n    at: 2026-04-19T00:00:00Z\n    actor: Author-410003\n    role: spec\n    approver: mark\n |
    When complete fs is executed with:
      | artifact_path  | tracks/20260419T0412_no_out_of_scope_no_findings |
      | actor_name     | Reviewer-410003                                  |
      | actor_type     | agent                                            |
      | actor_model    | claude-opus-4-7                                  |
      | actor_provider | anthropic                                        |
      | satisfaction   | address_in_next_step                             |
      | at             | 2026-04-19T04:12:00Z                             |
    Then the complete result is a CompleteError containing "findings_required_for_address_in_next_step"
    And the complete result is a CompleteError containing "address_in_next_step"
