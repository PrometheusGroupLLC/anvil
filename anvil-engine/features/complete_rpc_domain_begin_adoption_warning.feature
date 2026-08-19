Feature: Complete RPC soft-warn covers domain machines (BP8)
  A domain machine kind (knowledge_lifecycle) counts as DRIVEN for the
  begin-adoption soft-warn now that the engine drives it. An actor completing
  a knowledge artifact with no matching open begin-marker (and who is not the
  creating actor) still records the transition AND surfaces the begin-adoption
  warning — the same contract as the track soft-warn.

  Scenario: Domain doer complete with no begin-marker warns and still transitions
    Given a hearth seeded with the knowledge_lifecycle machine and a knowledge artifact "20260601T0000_warn" in state "ingesting"
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path        | knowledge/20260601T0000_warn |
      | actor_name           | Rpc-Ingestor-700001          |
      | actor_type           | agent                        |
      | actor_model          | claude-opus-4-8              |
      | actor_provider       | anthropic                    |
      | actor_context_window | 200000                       |
      | actor_entrypoint     | claude-code                  |
    Then the complete RPC response new_state is "ingest_review"
    And the complete RPC response warnings contain "begin_adoption: actor Rpc-Ingestor-700001 transitioned knowledge/20260601T0000_warn in state ingesting without a prior begin"
