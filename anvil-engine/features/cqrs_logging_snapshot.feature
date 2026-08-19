Feature: Snapshot command RPC emits a structured CQRS log record
  When a snapshot transition runs through the engine, the engine emits one
  JSON log record to stderr carrying the actor, the resolved hearth, the
  command name, the write results, and an ok outcome — matched by named
  fields, not position.

  Background:
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260417T1000_rpc_track/                   | spec    |
    And the track "20260417T1000_rpc_track" has spec.md with content "# Rpc Track\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Rpc Track](tracks/20260417T1000_rpc_track/) — rpc track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## spec_review
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-04-16T00:00:00Z
      last_updated: 2026-04-16T00:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Spec (0)

      ## Spec Review (0)
      """

  Scenario: Snapshot RPC logs a JSON command-outcome record
    Given the engine is started with that hearth
    When the snapshot RPC is called with:
      | artifact_path        | tracks/20260417T1000_rpc_track |
      | to_state             | spec_review                    |
      | actor_name           | Rpc-Test-111111                |
      | actor_role           | review                         |
      | actor_type           | agent                          |
      | actor_model          | claude-opus-4-7                |
      | actor_provider       | anthropic                      |
      | actor_context_window | 1000000                        |
      | actor_sdk_version    | 0.2.111                        |
      | actor_entrypoint     | claude-desktop                 |
    Then the snapshot RPC response success is "true"
    And the engine stderr contains a JSON log record with fields:
      | command | snapshot        |
      | actor   | Rpc-Test-111111 |
      | hearth  | <non-empty>     |
      | outcome | ok              |
      | events  | status_updated  |

  Scenario: ANVIL_LOG controls verbosity of detail records
    Given the engine is started with that hearth and ANVIL_LOG "anvil_engine=debug"
    When the snapshot RPC is called with:
      | artifact_path        | tracks/20260417T1000_rpc_track |
      | to_state             | spec_review                    |
      | actor_name           | Rpc-Test-111111                |
      | actor_role           | review                         |
      | actor_type           | agent                          |
      | actor_model          | claude-opus-4-7                |
      | actor_provider       | anthropic                      |
      | actor_context_window | 1000000                        |
      | actor_sdk_version    | 0.2.111                        |
      | actor_entrypoint     | claude-desktop                 |
    Then the snapshot RPC response success is "true"
    And the engine stderr contains a JSON log record with fields:
      | command | snapshot |
      | detail  | snapshot_dispatch |
