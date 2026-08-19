Feature: Amend RPC rejects invalid ops with the B5a typed code (BP3, AC-1)
  An op outside the kind's schema, or one targeting an element never introduced
  by a prior Add in the document's op log, is rejected. The B5a typed code
  surfaces in the gRPC error as INVALID_ARGUMENT.

  Scenario: an Add whose new_kind only allows Revise is rejected
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260411T2021_anvil_workflow_engine/     | active       |
      | tracks/20260604T2114_amend_rej/                    | implementing |
    And the engine is started with that hearth
    When the amend RPC is called with:
      | artifact_path   | tracks/20260604T2114_amend_rej |
      | kind            | track                          |
      | target_document | spec                           |
      | target_id       | overview                       |
      | op_kind         | add                            |
      | new_kind        | overview                       |
      | body            | a second overview              |
      | actor_name      | Rpc-Doer-300010                |
      | actor_type      | agent                          |
      | actor_model     | claude-opus-4-7                |
      | actor_provider  | anthropic                      |
    Then the amend RPC returns gRPC status "INVALID_ARGUMENT"
    And the amend RPC error message contains "amendment_op_not_in_schema"

  Scenario: a Revise targeting an element never Added is rejected unknown element
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260411T2021_anvil_workflow_engine/     | active       |
      | tracks/20260604T2114_amend_unk/                    | implementing |
    And the engine is started with that hearth
    When the amend RPC is called with:
      | artifact_path   | tracks/20260604T2114_amend_unk |
      | kind            | track                          |
      | target_document | spec                           |
      | target_id       | never-added                    |
      | op_kind         | revise                         |
      | body            | revised body                   |
      | actor_name      | Rpc-Doer-300011                |
      | actor_type      | agent                          |
      | actor_model     | claude-opus-4-7                |
      | actor_provider  | anthropic                      |
    Then the amend RPC returns gRPC status "INVALID_ARGUMENT"
    And the amend RPC error message contains "amendment_unknown_element"
