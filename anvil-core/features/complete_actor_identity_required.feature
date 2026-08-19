Feature: Complete — actor identity required
  CompleteCommandHandler validates identity fields before any state read.
  Empty artifact_path, actor_name, or any actor_* runtime param each returns
  the corresponding snake_case error code. Pattern from begin_error_taxonomy.

  Scenario: Empty artifact_path returns artifact_path_required
    Given a complete fs hearth with:
      | path | content |
    When complete fs is executed with:
      | artifact_path  |                 |
      | actor_name     | Doer-430001     |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
      | at             | 2026-04-19T04:30:00Z |
    Then the complete result is a CompleteError containing "artifact_path_required"

  Scenario: Empty actor_name returns actor_name_required
    Given a complete fs hearth with:
      | path                                                               | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
      | tracks/20260419T0431_identity_required/status.yaml                | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-430002:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-19T00:00:00Z\n    actor: Author-430002\n    role: spec\n    approver: mark\n |
    When complete fs is executed with:
      | artifact_path  | tracks/20260419T0431_identity_required |
      | actor_name     |                                        |
      | actor_type     | agent                                  |
      | actor_model    | claude-opus-4-7                        |
      | actor_provider | anthropic                              |
      | at             | 2026-04-19T04:31:00Z                   |
    Then the complete result is a CompleteError containing "actor_name_required"

  Scenario: Empty actor_type returns actor_params_required
    Given a complete fs hearth with:
      | path                                                               | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
      | tracks/20260419T0432_identity_required_type/status.yaml           | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-430003:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-19T00:00:00Z\n    actor: Author-430003\n    role: spec\n    approver: mark\n |
    When complete fs is executed with:
      | artifact_path  | tracks/20260419T0432_identity_required_type |
      | actor_name     | Doer-430003                                 |
      | actor_type     |                                             |
      | actor_model    | claude-opus-4-7                             |
      | actor_provider | anthropic                                   |
      | at             | 2026-04-19T04:32:00Z                        |
    Then the complete result is a CompleteError containing "actor_params_required"
    And the complete result is a CompleteError containing "actor_type"

  Scenario: Empty actor_model returns actor_params_required
    Given a complete fs hearth with:
      | path                                                               | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
      | tracks/20260419T0433_identity_required_model/status.yaml          | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-430004:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-19T00:00:00Z\n    actor: Author-430004\n    role: spec\n    approver: mark\n |
    When complete fs is executed with:
      | artifact_path  | tracks/20260419T0433_identity_required_model |
      | actor_name     | Doer-430004                                  |
      | actor_type     | agent                                        |
      | actor_model    |                                              |
      | actor_provider | anthropic                                    |
      | at             | 2026-04-19T04:33:00Z                         |
    Then the complete result is a CompleteError containing "actor_params_required"
    And the complete result is a CompleteError containing "actor_model"

  Scenario: Empty actor_provider returns actor_params_required
    Given a complete fs hearth with:
      | path                                                               | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
      | tracks/20260419T0434_identity_required_provider/status.yaml       | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-430005:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-19T00:00:00Z\n    actor: Author-430005\n    role: spec\n    approver: mark\n |
    When complete fs is executed with:
      | artifact_path  | tracks/20260419T0434_identity_required_provider |
      | actor_name     | Doer-430005                                     |
      | actor_type     | agent                                           |
      | actor_model    | claude-opus-4-7                                 |
      | actor_provider |                                                 |
      | at             | 2026-04-19T04:34:00Z                            |
    Then the complete result is a CompleteError containing "actor_params_required"
    And the complete result is a CompleteError containing "actor_provider"
