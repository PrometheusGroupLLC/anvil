Feature: Complete — wrong state rejections for the revision path
  CompleteCommandHandler returns WrongStateForComplete when the call shape does
  not match the artifact's current state on the spec_revision/spec_review states.
  Per spec R6.1, R6.3, R8.1(e). Two sub-cases:
  (e1) reviewer full_revision on spec_revision — wrong state.
  (e2) doer complete (no satisfaction) on spec_review — wrong role call shape.

  Scenario: (e1) Reviewer full_revision on spec_revision track returns wrong_state_for_complete
    Given a complete fs hearth with:
      | path                                                              | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
      | tracks/20260419T0800_ws_fr_on_revision/status.yaml               | version: 1\nkind: track\nstate: spec_revision\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-800001:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec_revision\n    at: 2026-04-19T00:00:00Z\n    actor: Reviewer-800000\n    role: review\n    approver: mark\n |
    When complete fs is executed with:
      | artifact_path  | tracks/20260419T0800_ws_fr_on_revision |
      | actor_name     | Reviewer-800001                        |
      | actor_type     | agent                                  |
      | actor_model    | claude-opus-4-7                        |
      | actor_provider | anthropic                              |
      | satisfaction   | full_revision                          |
      | at             | 2026-04-19T08:00:00Z                   |
    Then the complete result is a CompleteError containing "wrong_state_for_complete"
    And the complete result is a CompleteError containing "spec_revision"
    And the complete result is a CompleteError containing "no satisfaction"

  Scenario: (e2) Doer complete with no satisfaction on spec_review track returns wrong_state_for_complete
    Given a complete fs hearth with:
      | path                                                              | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
      | tracks/20260419T0801_ws_doer_on_review/status.yaml               | version: 1\nkind: track\nstate: spec_review\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-800002:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec_review\n    at: 2026-04-19T00:00:00Z\n    actor: Author-800002\n    role: spec\n    approver: mark\n |
    When complete fs is executed with:
      | artifact_path  | tracks/20260419T0801_ws_doer_on_review |
      | actor_name     | Doer-800002                            |
      | actor_type     | agent                                  |
      | actor_model    | claude-opus-4-7                        |
      | actor_provider | anthropic                              |
      | at             | 2026-04-19T08:01:00Z                   |
    Then the complete result is a CompleteError containing "wrong_state_for_complete"
    And the complete result is a CompleteError containing "spec_review"
    And the complete result is a CompleteError containing "satisfied"
    And the complete result is a CompleteError containing "full_revision"
