Feature: Complete — address_in_next_step rejected from non-spec_review states (R5.1)
  Slice C: the carry-forward path has no valid entry point outside spec_review.
  complete(satisfaction: "address_in_next_step", findings: ...) on a track in any
  other state returns wrong_state_for_complete. Findings are supplied so the
  findings guard does not fire first — the state guard is what rejects.

  Scenario: address_in_next_step on a spec track returns wrong_state_for_complete
    Given a complete fs hearth with:
      | path                                                            | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
      | tracks/20260419T0720_ains_on_spec/status.yaml                  | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-720001:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-19T00:00:00Z\n    actor: Author-720001\n    role: spec\n    approver: mark\n |
    When complete fs is executed with:
      | artifact_path  | tracks/20260419T0720_ains_on_spec |
      | actor_name     | Reviewer-720001                   |
      | actor_type     | agent                             |
      | actor_model    | claude-opus-4-7                   |
      | actor_provider | anthropic                         |
      | satisfaction   | address_in_next_step              |
      | findings       | premature carry-forward           |
      | at             | 2026-04-19T07:20:00Z              |
    Then the complete result is a CompleteError containing "wrong_state_for_complete"
    And the resolved state of "tracks/20260419T0720_ains_on_spec" is "spec"

  Scenario: address_in_next_step on a plan track returns wrong_state_for_complete
    Given a complete fs hearth with:
      | path                                                            | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
      | tracks/20260419T0721_ains_on_plan/status.yaml                  | version: 1\nkind: track\nstate: plan\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-721001:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: plan\n    at: 2026-04-19T00:00:00Z\n    actor: Author-721001\n    role: reviewer\n    approver: mark\n |
    When complete fs is executed with:
      | artifact_path  | tracks/20260419T0721_ains_on_plan |
      | actor_name     | Reviewer-721001                   |
      | actor_type     | agent                             |
      | actor_model    | claude-opus-4-7                   |
      | actor_provider | anthropic                         |
      | satisfaction   | address_in_next_step              |
      | findings       | premature carry-forward           |
      | at             | 2026-04-19T07:21:00Z              |
    Then the complete result is a CompleteError containing "wrong_state_for_complete"
    And the resolved state of "tracks/20260419T0721_ains_on_plan" is "plan"

  Scenario: address_in_next_step on an implementing track returns wrong_state_for_complete
    Given a complete fs hearth with:
      | path                                                                | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
      | tracks/20260419T0722_ains_on_impl/status.yaml                      | version: 1\nkind: track\nstate: implementing\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-722001:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: implementing\n    at: 2026-04-19T00:00:00Z\n    actor: Author-722001\n    role: implement\n    approver: mark\n |
    When complete fs is executed with:
      | artifact_path  | tracks/20260419T0722_ains_on_impl |
      | actor_name     | Reviewer-722001                   |
      | actor_type     | agent                             |
      | actor_model    | claude-opus-4-7                   |
      | actor_provider | anthropic                         |
      | satisfaction   | address_in_next_step              |
      | findings       | premature carry-forward           |
      | at             | 2026-04-19T07:22:00Z              |
    Then the complete result is a CompleteError containing "wrong_state_for_complete"
    And the resolved state of "tracks/20260419T0722_ains_on_impl" is "implementing"
