Feature: Amend RPC drives the track completed → amend transition (BP4, AC-5)
  After the track machine gains the amend loop anchored on `completed`, an Amend
  call on a track in `completed` drives the interpreter-validated
  `completed → amend` transition (SnapshotPort.append_transition direct), records
  the op, and returns new_state "amend". Idempotent: a track already in `amend`
  records the op only. A track NOT in `completed` is record-only.

  Scenario: Amend on a track in completed drives completed → amend
    Given a hearth directory with the following structure:
      | path                                              | state     |
      | proposals/20260411T2021_anvil_workflow_engine/     | active    |
      | tracks/20260604T2114_amend_done/                   | completed |
    And the engine is started with that hearth
    When the amend RPC is called with:
      | artifact_path   | tracks/20260604T2114_amend_done |
      | kind            | track                           |
      | target_document | spec                            |
      | target_id       | goal-extra                      |
      | op_kind         | add                             |
      | new_kind        | goal                            |
      | body            | An amended goal                 |
      | actor_name      | Rpc-Doer-400001                 |
      | actor_type      | agent                           |
      | actor_model     | claude-opus-4-7                 |
      | actor_provider  | anthropic                       |
    Then the amend RPC response new_state is "amend"
    And the hearth file "tracks/20260604T2114_amend_done/spec.amendments.yaml" contains "target_id: goal-extra"
    And a hearth transition event for "tracks/20260604T2114_amend_done" contains "to: amend"
    And a hearth transition event for "tracks/20260604T2114_amend_done" contains "role: doer"
    And a hearth transition event for "tracks/20260604T2114_amend_done" contains "actor: Rpc-Doer-400001"

  Scenario: Amend on a track already in amend records the op only — no duplicate transition
    Given a hearth directory with the following structure:
      | path                                              | state |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
      | tracks/20260604T2114_amend_already/                | amend  |
    And the engine is started with that hearth
    When the amend RPC is called with:
      | artifact_path   | tracks/20260604T2114_amend_already |
      | kind            | track                              |
      | target_document | spec                               |
      | target_id       | goal-again                         |
      | op_kind         | add                                |
      | new_kind        | goal                               |
      | body            | Another amended goal               |
      | actor_name      | Rpc-Doer-400002                    |
      | actor_type      | agent                              |
      | actor_model     | claude-opus-4-7                    |
      | actor_provider  | anthropic                          |
    Then the amend RPC response new_state is "amend"
    And the hearth file "tracks/20260604T2114_amend_already/spec.amendments.yaml" contains "target_id: goal-again"
    And the hearth file "tracks/20260604T2114_amend_already/status.yaml" does not contain "to: amend"

  Scenario: Amend on a track NOT in completed is record-only
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260411T2021_anvil_workflow_engine/     | active       |
      | tracks/20260604T2114_amend_impl/                   | implementing |
    And the engine is started with that hearth
    When the amend RPC is called with:
      | artifact_path   | tracks/20260604T2114_amend_impl |
      | kind            | track                           |
      | target_document | spec                            |
      | target_id       | goal-impl                       |
      | op_kind         | add                             |
      | new_kind        | goal                            |
      | body            | A goal during implementing      |
      | actor_name      | Rpc-Doer-400003                 |
      | actor_type      | agent                           |
      | actor_model     | claude-opus-4-7                 |
      | actor_provider  | anthropic                       |
    Then the amend RPC response new_state is ""
    And the hearth file "tracks/20260604T2114_amend_impl/spec.amendments.yaml" contains "target_id: goal-impl"
    And the hearth file "tracks/20260604T2114_amend_impl/status.yaml" does not contain "to: amend"
