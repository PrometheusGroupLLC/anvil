Feature: An explicitly-bad hearth errors and writes nothing
  When a request explicitly targets a (bad) hearth, the engine rejects it with a
  clear error and writes nothing — it does NOT fall back to the --hearth default
  (spec Req 3, AC "Explicit binding"). (An empty hearth_path is the distinct
  no-regression path that uses the --hearth default; see resolve_hearth C1.)

  Scenario: complete with an explicitly unresolvable hearth errors and writes nothing
    Given two hearth directories X and Y each with the standard structure
    And hearth Y's file contents are recorded
    When the complete RPC is called with:
      | artifact_path        | tracks/20260419T1100_track_y                          |
      | actor_name           | Rpc-Doer-333333                                       |
      | actor_type           | agent                                                 |
      | actor_model          | claude-opus-4-7                                       |
      | actor_provider       | anthropic                                             |
      | actor_context_window | 200000                                                |
      | actor_entrypoint     | claude-code                                           |
      | hearth_path          | /tmp/anvil-explicitly-bad-hearth-does-not-exist-xyz   |
    Then the complete RPC returns gRPC status "INVALID_ARGUMENT"
    And hearth Y's files are byte-unchanged
