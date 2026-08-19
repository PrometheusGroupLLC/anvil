Feature: Global playbook hearth supplies playbook kinds to request hearths
  A hearth-less engine may be started with a global playbooks hearth. Request
  hearths keep ownership of artifacts, while playbook kind resolution searches
  request playbooks first, then global playbooks, then compiled seeds.

  Scenario: Global playbook hook body is served when the request hearth lacks the kind
    Given a request hearth without knowledge_lifecycle and a global playbooks hearth with knowledge_lifecycle body "GLOBAL INGEST BODY"
    And the hearth-less engine is started with the global playbooks hearth
    When the begin RPC is called for the request hearth to create a "knowledge_lifecycle" artifact named "global knowledge" with no parent
    Then the begin RPC response state is "ingesting"
    And the begin RPC response "context_text" is exactly "GLOBAL INGEST BODY"

  Scenario: Request playbook kind shadows the global playbook kind
    Given a request hearth with knowledge_lifecycle body "REQUEST INGEST BODY" and a global playbooks hearth with knowledge_lifecycle body "GLOBAL INGEST BODY"
    And the hearth-less engine is started with the global playbooks hearth
    When the begin RPC is called for the request hearth to create a "knowledge_lifecycle" artifact named "shadow knowledge" with no parent
    Then the begin RPC response state is "ingesting"
    And the begin RPC response "context_text" is exactly "REQUEST INGEST BODY"

  Scenario: Global-only playbook kind is unresolved without global configuration
    Given a request hearth without knowledge_lifecycle and a global playbooks hearth with knowledge_lifecycle body "GLOBAL INGEST BODY"
    And a hearth-less engine is started without the global playbooks hearth
    When the begin RPC is called for the request hearth to create a "knowledge_lifecycle" artifact named "missing global" with no parent
    Then the begin RPC returns gRPC status "INVALID_ARGUMENT"

  Scenario: Complete advances a global-kind artifact using the global machine
    Given a request hearth without knowledge_lifecycle and a global playbooks hearth with knowledge_lifecycle body "GLOBAL INGEST BODY"
    And the request hearth has a knowledge_lifecycle artifact "20260611T2100_global_complete" in state "ingesting"
    And the hearth-less engine is started with the global playbooks hearth
    When the complete RPC is called with:
      | artifact_path        | knowledge/20260611T2100_global_complete |
      | actor_name           | Rpc-Doer-420000                         |
      | actor_type           | agent                                   |
      | actor_model          | claude-opus-4-7                         |
      | actor_provider       | anthropic                               |
      | actor_context_window | 200000                                  |
      | actor_entrypoint     | claude-code                             |
      | hearth_path          | <hearth>                                |
    Then the complete RPC response new_state is "ingest_review"

  Scenario: Amend drives a global-kind artifact only when the global machine declares the amend edge
    Given a request hearth without knowledge_lifecycle and a global playbooks hearth with knowledge_lifecycle body "GLOBAL INGEST BODY" and a "published" to amend edge
    And the request hearth has a knowledge_lifecycle artifact "20260611T2101_global_amend" in state "published"
    And the hearth-less engine is started with the global playbooks hearth
    When the amend RPC is called with:
      | artifact_path        | knowledge/20260611T2101_global_amend |
      | kind                 | learning                             |
      | target_document      | definition                           |
      | target_id            | evidence-1                           |
      | op_kind              | add                                  |
      | body                 | Global amend                         |
      | new_kind             | evidence                             |
      | actor_name           | Rpc-Doer-420001                      |
      | actor_type           | agent                                |
      | actor_model          | claude-opus-4-7                      |
      | actor_provider       | anthropic                            |
      | actor_context_window | 200000                               |
      | actor_entrypoint     | claude-code                          |
      | hearth_path          | <hearth>                             |
    Then the amend RPC response new_state is "amend"

  Scenario: Catalog from a foreign request hearth lists the global playbook kind
    Given a request hearth without knowledge_lifecycle and a global playbooks hearth with knowledge_lifecycle body "GLOBAL INGEST BODY"
    And the hearth-less engine is started with the global playbooks hearth
    When the catalog RPC is called for the request hearth
    Then the catalog available playbook kinds include "knowledge_lifecycle"

  Scenario: Seed playbook hook body is still served from the request hearth
    Given a hearth directory with the following structure:
      | path                                          | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a playbook hook body for the spec doer hook "seed-spec.md" with content "SEED TRACK BODY"
    And the engine is started with that hearth
    When the begin RPC is called to create a track named "seed regression" under parent "20260411T2021_anvil_workflow_engine"
    Then the begin RPC response "context_text" is exactly "SEED TRACK BODY"
