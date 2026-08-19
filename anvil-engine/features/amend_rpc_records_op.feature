Feature: Amend RPC records a valid op end-to-end (BP3, AC-2)
  A caller submitting a valid structured op via the Amend RPC has it validated
  against the kind's content schema + the document's op log, appended to the
  per-document op-log file `<artifact>/<target_document>.amendments.yaml`, and
  accumulated across successive calls in ordered() order. The response carries
  op_id + resolved_hearth.

  Scenario: Amend on a track records the op to spec.amendments.yaml
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260411T2021_anvil_workflow_engine/     | active       |
      | tracks/20260604T2114_amend_rpc/                    | implementing |
    And the engine is started with that hearth
    When the amend RPC is called with:
      | artifact_path   | tracks/20260604T2114_amend_rpc |
      | kind            | track                          |
      | target_document | spec                           |
      | target_id       | goal-ship                      |
      | op_kind         | add                            |
      | new_kind        | goal                           |
      | body            | Ship the amend surface         |
      | actor_name      | Rpc-Doer-300001                |
      | actor_type      | agent                          |
      | actor_model     | claude-opus-4-7                |
      | actor_provider  | anthropic                      |
    Then the amend RPC response op_id matches "^op-\d{8}T\d{6}Z-0$"
    And the amend RPC response new_state is ""
    And the hearth file "tracks/20260604T2114_amend_rpc/spec.amendments.yaml" contains "target_id: goal-ship"
    And the hearth file "tracks/20260604T2114_amend_rpc/spec.amendments.yaml" contains "kind: add"
    And the hearth file "tracks/20260604T2114_amend_rpc/spec.amendments.yaml" contains "seq: 0"

  Scenario: a second Amend accumulates in the same op-log file
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260411T2021_anvil_workflow_engine/     | active       |
      | tracks/20260604T2114_amend_acc/                    | implementing |
    And the engine is started with that hearth
    When the amend RPC is called with:
      | artifact_path   | tracks/20260604T2114_amend_acc |
      | kind            | track                          |
      | target_document | spec                           |
      | target_id       | goal-first                     |
      | op_kind         | add                            |
      | new_kind        | goal                           |
      | body            | first goal                     |
      | actor_name      | Rpc-Doer-300002                |
      | actor_type      | agent                          |
      | actor_model     | claude-opus-4-7                |
      | actor_provider  | anthropic                      |
    And the amend RPC is called with:
      | artifact_path   | tracks/20260604T2114_amend_acc |
      | kind            | track                          |
      | target_document | spec                           |
      | target_id       | goal-second                    |
      | op_kind         | add                            |
      | new_kind        | goal                           |
      | body            | second goal                    |
      | actor_name      | Rpc-Doer-300002                |
      | actor_type      | agent                          |
      | actor_model     | claude-opus-4-7                |
      | actor_provider  | anthropic                      |
    Then the amend RPC response op_id matches "^op-\d{8}T\d{6}Z-1$"
    And the hearth file "tracks/20260604T2114_amend_acc/spec.amendments.yaml" contains "target_id: goal-first"
    And the hearth file "tracks/20260604T2114_amend_acc/spec.amendments.yaml" contains "target_id: goal-second"
    And the hearth file "tracks/20260604T2114_amend_acc/spec.amendments.yaml" contains "seq: 1"
