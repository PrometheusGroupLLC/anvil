Feature: Complete command appends a redacted step_measurement to the durable sink
  In addition to the stderr `step_measurement` tracing event, each complete
  appends exactly one redacted record to the durable, append-only per-hearth
  sink `<hearth>/step-measurement.jsonl`. The record carries ONLY the Part-3
  allowlisted fields — record kind, the real from_state/to_state transition, the
  role, two BOOLEANS (intent_present / expected_output_present) capturing whether
  a measurement was declared for the step, and an ISO-8601 timestamp. It NEVER
  carries the intent/expected_output PROSE, the surface message text, paths,
  identities, or token counts. Temper folds this sink into per-(state,role)
  step-coverage aggregates.

  Background:
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260419T1100_rpc_complete_track/           | spec    |
    And the track "20260419T1100_rpc_complete_track" has spec.md with content "# RPC Complete Track\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [RPC Complete Track](tracks/20260419T1100_rpc_complete_track/) — rpc complete track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## spec_review

      ## plan

      ## implementing
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-04-19T00:00:00Z
      last_updated: 2026-04-19T00:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Spec (0)

      ## Spec Review (0)

      ## Planned (0)

      ## Implementing (0)
      """
    And the engine is started with that hearth

  Scenario: Complete appends one redacted step_measurement record with boolean coverage, no prose
    When the complete RPC is called with:
      | artifact_path        | tracks/20260419T1100_rpc_complete_track |
      | actor_name           | Rpc-Doer-111111                         |
      | actor_type           | agent                                   |
      | actor_model          | claude-opus-4-7                         |
      | actor_provider       | anthropic                               |
      | actor_context_window | 200000                                  |
      | actor_entrypoint     | claude-code                             |
    Then the complete RPC response new_state is "spec_review"
    And the hearth step-measurement sink has exactly 1 records
    And the hearth step-measurement sink contains "\"kind\":\"step_measurement\""
    And the hearth step-measurement sink contains "\"from_state\":\"spec\""
    And the hearth step-measurement sink contains "\"to_state\":\"spec_review\""
    And the hearth step-measurement sink contains "\"role\":\"doer\""
    And the hearth step-measurement sink contains "\"intent_present\":true"
    And the hearth step-measurement sink contains "\"expected_output_present\":true"

  Scenario: The durable step_measurement record leaks no prose, identities, paths, or tokens
    When the complete RPC is called with:
      | artifact_path        | tracks/20260419T1100_rpc_complete_track |
      | actor_name           | Rpc-Doer-111111                         |
      | actor_type           | agent                                   |
      | actor_model          | claude-opus-4-7                         |
      | actor_provider       | anthropic                               |
      | actor_context_window | 200000                                  |
      | actor_entrypoint     | claude-code                             |
    Then the complete RPC response new_state is "spec_review"
    And the hearth step-measurement sink does not contain "Rpc-Doer-111111"
    And the hearth step-measurement sink does not contain "actor_name"
    And the hearth step-measurement sink does not contain "tokens"
    And the hearth step-measurement sink does not contain "duration_ms"
    And the hearth step-measurement sink does not contain "tracks/"
    And the hearth step-measurement sink does not contain "intent\":\""
    And the hearth step-measurement sink does not contain "expected_output\":\""
    # The salted, non-reversible actor_hash is allowlisted (it is what makes
    # distinct-actor counting possible) — the RAW actor name above never appears.
    And the hearth step-measurement sink contains "\"actor_hash\":\""
    # The real public playbook kind rides along (same kind the routing sink records).
    And the hearth step-measurement sink contains "\"artifact_kind\":\"track\""
