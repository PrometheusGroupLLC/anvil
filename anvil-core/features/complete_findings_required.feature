Feature: Complete — findings required for address_in_next_step (R1.2 / R5.2)
  Slice C: complete(satisfaction: "address_in_next_step") on a spec_review track
  requires a non-empty findings field. Empty or absent findings is rejected with
  findings_required_for_address_in_next_step BEFORE any state change. Supplied
  with non-empty findings, the call succeeds (positive control).

  Scenario: address_in_next_step with empty findings is rejected
    Given a complete fs hearth with:
      | path                                                              | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
      | tracks/20260419T0710_findings_empty/status.yaml                  | version: 1\nkind: track\nstate: spec_review\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-710001:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec_review\n    at: 2026-04-19T00:00:00Z\n    actor: Author-710001\n    role: spec\n    approver: mark\n |
    When complete fs is executed with:
      | artifact_path  | tracks/20260419T0710_findings_empty |
      | actor_name     | Reviewer-710001                     |
      | actor_type     | agent                               |
      | actor_model    | claude-opus-4-7                     |
      | actor_provider | anthropic                           |
      | satisfaction   | address_in_next_step                |
      | findings       |                                     |
      | at             | 2026-04-19T07:10:00Z                |
    Then the complete result is a CompleteError containing "findings_required_for_address_in_next_step"
    And the complete result is a CompleteError containing "findings"
    And the resolved state of "tracks/20260419T0710_findings_empty" is "spec_review"

  Scenario: address_in_next_step with findings field absent is rejected
    Given a complete fs hearth with:
      | path                                                              | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
      | tracks/20260419T0711_findings_absent/status.yaml                 | version: 1\nkind: track\nstate: spec_review\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-711001:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec_review\n    at: 2026-04-19T00:00:00Z\n    actor: Author-711001\n    role: spec\n    approver: mark\n |
    When complete fs is executed with:
      | artifact_path  | tracks/20260419T0711_findings_absent |
      | actor_name     | Reviewer-711001                      |
      | actor_type     | agent                                |
      | actor_model    | claude-opus-4-7                      |
      | actor_provider | anthropic                            |
      | satisfaction   | address_in_next_step                 |
      | at             | 2026-04-19T07:11:00Z                 |
    Then the complete result is a CompleteError containing "findings_required_for_address_in_next_step"
    And the resolved state of "tracks/20260419T0711_findings_absent" is "spec_review"

  Scenario: address_in_next_step with non-empty findings succeeds (positive control)
    Given a complete fs hearth with:
      | path                                                              | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
      | tracks/20260419T0712_findings_present/status.yaml                | version: 1\nkind: track\nstate: spec_review\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-712001:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec_review\n    at: 2026-04-19T00:00:00Z\n    actor: Author-712001\n    role: spec\n    approver: mark\n |
      | tracks/20260419T0712_findings_present/spec.md                    | # Findings Present\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
      | tracks.md                                                        | # Tracks\n\n## spec\n\n## spec_review\n\n- [Findings Present](tracks/20260419T0712_findings_present/) — findings present — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                                |
      | projections/execution.md                                         | ---\nincremental_count: 0\nbase_snapshot: 2026-04-19T00:00:00Z\nlast_updated: 2026-04-19T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (1)\n\n- [Findings Present](tracks/20260419T0712_findings_present/)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                                            |
    When complete fs is executed with:
      | artifact_path  | tracks/20260419T0712_findings_present |
      | actor_name     | Reviewer-712001                       |
      | actor_type     | agent                                 |
      | actor_model    | claude-opus-4-7                       |
      | actor_provider | anthropic                             |
      | satisfaction   | address_in_next_step                  |
      | findings       | Please verify the migration ordering. |
      | at             | 2026-04-19T07:12:00Z                  |
    Then the complete result is successful
    And the complete result new_state is "plan"
    And the complete result carry_forward_path is non-empty
