Feature: Logs never emit payload contents or secrets
  A sentinel seeded into a note payload must not appear in any log record,
  and the FOUNDRY_SESSION_TOKEN value must never appear. Logs carry
  identifiers, variant names, and outcomes — never content (Req 6).

  Scenario: A note sentinel never reaches the logs
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
    And the engine is started with that hearth
    When the snapshot RPC is called with:
      | artifact_path        | tracks/20260417T1000_rpc_track          |
      | to_state             | spec_review                             |
      | actor_name           | Rpc-Test-111111                         |
      | actor_role           | review                                  |
      | note                 | SENTINEL-DO-NOT-LOG-7f3a confidential   |
      | actor_type           | agent                                   |
      | actor_model          | claude-opus-4-7                         |
      | actor_provider       | anthropic                               |
      | actor_context_window | 1000000                                 |
      | actor_sdk_version    | 0.2.111                                 |
      | actor_entrypoint     | claude-desktop                          |
    Then the snapshot RPC response success is "true"
    And the engine stderr contains a JSON log record with fields:
      | command | snapshot |
      | outcome | ok       |
    And the engine stderr contains no log record with value "SENTINEL-DO-NOT-LOG-7f3a"
    And the engine stderr contains no log record with value "FOUNDRY_SESSION_TOKEN"
