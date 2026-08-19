Feature: Amend RPC proposal machine path
  An Amend call on proposal now resolves the proposal_lifecycle machine. The op
  is recorded and the machine-declared active to amend transition is driven.

  Scenario: Amend on a proposal records the op and drives the amend transition
    Given a hearth directory with the following structure:
      | path                                              | state  |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
    And the engine is started with that hearth
    When the amend RPC is called with:
      | artifact_path   | proposals/20260411T2021_anvil_workflow_engine |
      | kind            | proposal                                       |
      | target_document | proposal                                       |
      | target_id       | slice-5                                         |
      | op_kind         | add                                            |
      | new_kind        | slice                                          |
      | body            | a new delivery slice                           |
      | actor_name      | Rpc-Doer-300030                                |
      | actor_type      | agent                                          |
      | actor_model     | claude-opus-4-7                                |
      | actor_provider  | anthropic                                      |
    Then the amend RPC response op_id matches "^op-\d{8}T\d{6}Z-0$"
    And the amend RPC response new_state is "amend"
    And the hearth file "proposals/20260411T2021_anvil_workflow_engine/proposal.amendments.yaml" contains "target_id: slice-5"
    And a hearth transition event for "proposals/20260411T2021_anvil_workflow_engine" contains "to: amend"
