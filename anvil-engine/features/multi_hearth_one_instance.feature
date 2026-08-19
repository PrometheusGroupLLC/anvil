Feature: One engine serves multiple distinct hearths by request
  The engine resolves the target hearth from each request's hearth_path rather
  than a single spawn-time binding (spec Req 1, AC "Multi-hearth from one
  instance"). One engine instance serves >=2 distinct hearths across the RPC
  surface — catalog (read) and complete (write, via the new hearth_path field) —
  each returning/writing the requested hearth.

  Scenario: One engine reads and writes two different hearths by request
    Given two hearth directories X and Y each with the standard structure
    When the catalog RPC is called for hearth "X"
    Then the catalog RPC response resolved_hearth is the hearth "<hearth_x>"
    And the catalog response includes artifact "20260419T1100_track_x" with type "track"
    And the catalog response does not include "20260419T1100_track_y"
    When the catalog RPC is called for hearth "Y"
    Then the catalog RPC response resolved_hearth is the hearth "<hearth_y>"
    And the catalog response includes artifact "20260419T1100_track_y" with type "track"
    And the catalog response does not include "20260419T1100_track_x"
    When the complete RPC is called with:
      | artifact_path        | tracks/20260419T1100_track_y |
      | actor_name           | Rpc-Doer-222222              |
      | actor_type           | agent                        |
      | actor_model          | claude-opus-4-7              |
      | actor_provider       | anthropic                    |
      | actor_context_window | 200000                       |
      | actor_entrypoint     | claude-code                  |
      | hearth_path          | <hearth_y>                   |
    Then the complete RPC response new_state is "spec_review"
    And the complete RPC response resolved_hearth is the hearth "<hearth_y>"

  Scenario: One hearth-less engine resolves two request-supplied hearths
    Given two hearth directories X and Y each with the standard structure and a hearth-less engine
    When the catalog RPC is called for hearth "X"
    Then the catalog RPC response resolved_hearth is the hearth "<hearth_x>"
    And the catalog response includes artifact "20260419T1100_track_x" with type "track"
    And the catalog response does not include "20260419T1100_track_y"
    When the checkin RPC is called for hearth "Y" with role "creator"
    Then the checkin RPC response has a non-empty actor name
    And the checkin RPC filtered artifacts include "20260411T2021_anvil_workflow_engine_y"
    And the checkin RPC filtered artifacts do not include "20260411T2021_anvil_workflow_engine_x"
