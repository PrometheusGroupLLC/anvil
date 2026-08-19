Feature: Command RPC error outcome is logged with its error code
  When a snapshot command fails, the engine logs a JSON record carrying the
  command, an error outcome, and the error code — at snapshot's single error
  funnel (the .map_err on the domain handler).

  Scenario: Snapshot on a non-existent artifact logs an error outcome with code
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
    And the engine is started with that hearth
    When the snapshot RPC is called with:
      | artifact_path  | tracks/no_such_track |
      | to_state       | spec_review          |
      | actor_name     | Rpc-Test-1           |
      | actor_role     | review               |
      | actor_type     | agent                |
      | actor_model    | claude-opus-4-7      |
      | actor_provider | anthropic            |
    Then the snapshot RPC returns gRPC status "NOT_FOUND"
    And the engine stderr contains a JSON log record with fields:
      | command    | snapshot |
      | outcome    | error    |
      | error_code | NotFound |
