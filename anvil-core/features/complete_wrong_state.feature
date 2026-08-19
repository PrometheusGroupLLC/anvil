Feature: Complete — wrong state rejection
  CompleteCommandHandler returns WrongStateForComplete when the call shape
  does not match the artifact's current state. Three sub-scenarios per R8.1(c):
  1. doer-style on spec_review (should use reviewer-style)
  2. reviewer-style on spec (should use doer-style first)
  3. doer-style on plan is supported and advances to plan_review

  Scenario: Doer-style complete on spec_review track returns wrong_state_for_complete
    Given a complete fs hearth with:
      | path                                                              | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
      | tracks/20260419T0400_wrong_state_doer_on_review/status.yaml      | version: 1\nkind: track\nstate: spec_review\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-400001:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec_review\n    at: 2026-04-19T00:00:00Z\n    actor: Author-400001\n    role: spec\n    approver: mark\n |
    When complete fs is executed with:
      | artifact_path  | tracks/20260419T0400_wrong_state_doer_on_review |
      | actor_name     | Doer-400001                                     |
      | actor_type     | agent                                           |
      | actor_model    | claude-opus-4-7                                 |
      | actor_provider | anthropic                                       |
      | at             | 2026-04-19T04:00:00Z                            |
    Then the complete result is a CompleteError containing "wrong_state_for_complete"
    And the complete result is a CompleteError containing "spec_review"
    And the complete result is a CompleteError containing "satisfied"

  Scenario: Reviewer-style complete on spec track returns wrong_state_for_complete
    Given a complete fs hearth with:
      | path                                                              | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
      | tracks/20260419T0401_wrong_state_reviewer_on_spec/status.yaml    | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-400002:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-19T00:00:00Z\n    actor: Author-400002\n    role: spec\n    approver: mark\n |
    When complete fs is executed with:
      | artifact_path  | tracks/20260419T0401_wrong_state_reviewer_on_spec |
      | actor_name     | Reviewer-400002                                   |
      | actor_type     | agent                                             |
      | actor_model    | claude-opus-4-7                                   |
      | actor_provider | anthropic                                         |
      | satisfaction   | satisfied                                         |
      | at             | 2026-04-19T04:01:00Z                              |
    Then the complete result is a CompleteError containing "wrong_state_for_complete"
    And the complete result is a CompleteError containing "spec"
    And the complete result is a CompleteError containing "doer"
    And the complete result is a CompleteError containing "no satisfaction"

  Scenario: Doer-style complete on plan track advances to plan_review
    Given a complete fs hearth with:
      | path                                                              | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
      | tracks/20260419T0402_wrong_state_plan_track/status.yaml          | version: 1\nkind: track\nstate: plan\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-400003:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: plan\n    at: 2026-04-19T00:00:00Z\n    actor: Author-400003\n    role: plan\n    approver: mark\n |
    When complete fs is executed with:
      | artifact_path  | tracks/20260419T0402_wrong_state_plan_track |
      | actor_name     | Actor-400003                                |
      | actor_type     | agent                                       |
      | actor_model    | claude-opus-4-7                             |
      | actor_provider | anthropic                                   |
      | at             | 2026-04-19T04:02:00Z                        |
    Then the complete result new_state is "plan_review"
    And the complete fs artifact "tracks/20260419T0402_wrong_state_plan_track" status.yaml contains "role: plan"
