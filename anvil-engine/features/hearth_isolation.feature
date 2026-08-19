Feature: A transition on one hearth leaves another byte-unchanged
  A call bound to hearth X reads/writes only hearth X's files; never another
  hearth's (spec Req 2, AC "Isolation"). A transition on X must leave hearth Y's
  files byte-unchanged.

  Scenario: Completing a track on hearth X does not touch hearth Y
    Given two hearth directories X and Y each with the standard structure
    And hearth Y's file contents are recorded
    When the complete RPC is called with:
      | artifact_path        | tracks/20260419T1100_track_x |
      | actor_name           | Rpc-Doer-111111              |
      | actor_type           | agent                        |
      | actor_model          | claude-opus-4-7              |
      | actor_provider       | anthropic                    |
      | actor_context_window | 200000                       |
      | actor_entrypoint     | claude-code                  |
      | hearth_path          | <hearth_x>                   |
    Then the complete RPC response new_state is "spec_review"
    And hearth Y's files are byte-unchanged
