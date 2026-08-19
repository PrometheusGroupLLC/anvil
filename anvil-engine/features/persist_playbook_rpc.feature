Feature: PersistPlaybook RPC writes a validated machine to a given owner-home (track 1a, BP3, A3/A6)
  The standalone PersistPlaybook RPC authorizes, validates the generated machine
  via the pure handler, writes it to the GIVEN owner-home through the port, and is
  registry-resolvable from there. Engine-facing only. A loader-invalid machine
  fails INVALID_ARGUMENT; retrying the same content for an already-persisted kind
  is an idempotent success; a same-kind/different-content collision at the
  owner-home fails ALREADY_EXISTS.

  Scenario: RPC persists a valid machine to a given owner-home
    Given a hearth directory with the following structure:
      | path                                          | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And the engine is started with that hearth
    And a separate temp owner-home directory
    When the PersistPlaybook RPC is called for kind "throwaway_kind" under that owner-home with:
      | actor_name     | Persist-Doer-700001 |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-8 |
      | actor_provider | anthropic       |
    Then the PersistPlaybook RPC response kind is "throwaway_kind"
    And the PersistPlaybook RPC written_path is under the owner-home
    And a machine.yaml exists at "playbooks/throwaway_kind/machine.yaml" under the persist owner-home

  Scenario: persisted machine is registry-resolvable from the owner-home
    Given a hearth directory with the following structure:
      | path                                          | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And the engine is started with that hearth
    And a separate temp owner-home directory
    When the PersistPlaybook RPC is called for kind "throwaway_kind" under that owner-home with:
      | actor_name     | Persist-Doer-700002 |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-8 |
      | actor_provider | anthropic       |
    Then the PersistPlaybook RPC response kind is "throwaway_kind"
    And a fresh registry from the persist owner-home resolves kind "throwaway_kind"

  Scenario: loader-invalid machine returns INVALID_ARGUMENT
    Given a hearth directory with the following structure:
      | path                                          | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And the engine is started with that hearth
    And a separate temp owner-home directory
    When the PersistPlaybook RPC is called for kind "throwaway_kind" under that owner-home with:
      | machine        | loader_invalid  |
      | actor_name     | Persist-Doer-700003 |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-8 |
      | actor_provider | anthropic       |
    Then the PersistPlaybook RPC returns gRPC status "INVALID_ARGUMENT"
    And the PersistPlaybook RPC error message contains "playbook_unknown_role_reference"

  Scenario: retrying identical content for an already-persisted kind succeeds
    Given a hearth directory with the following structure:
      | path                                          | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And the engine is started with that hearth
    And a separate temp owner-home directory
    When the PersistPlaybook RPC is called for kind "throwaway_kind" under that owner-home with:
      | actor_name     | Persist-Doer-700004 |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-8 |
      | actor_provider | anthropic       |
    And the PersistPlaybook RPC is called for kind "throwaway_kind" under that owner-home with:
      | actor_name     | Persist-Doer-700004 |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-8 |
      | actor_provider | anthropic       |
    Then the PersistPlaybook RPC response kind is "throwaway_kind"
    And the PersistPlaybook RPC written_path is under the owner-home
    And a machine.yaml exists at "playbooks/throwaway_kind/machine.yaml" under the persist owner-home

  Scenario: same kind with different content at the owner-home returns ALREADY_EXISTS
    Given a hearth directory with the following structure:
      | path                                          | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And the engine is started with that hearth
    And a separate temp owner-home directory
    When the PersistPlaybook RPC is called for kind "throwaway_kind" under that owner-home with:
      | actor_name     | Persist-Doer-700005 |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-8 |
      | actor_provider | anthropic       |
    And the PersistPlaybook RPC is called for kind "throwaway_kind" under that owner-home with:
      | machine        | different_conformant |
      | actor_name     | Persist-Doer-700005 |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-8 |
      | actor_provider | anthropic       |
    Then the PersistPlaybook RPC returns gRPC status "ALREADY_EXISTS"
    And the PersistPlaybook RPC error message contains "playbook_duplicate_kind_registration"

  Scenario: RPC persists a hook-bearing machine and writes the hook file
    Given a hearth directory with the following structure:
      | path                                          | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And the engine is started with that hearth
    And a separate temp owner-home directory
    When the PersistPlaybook RPC is called for kind "throwaway_kind" under that owner-home with:
      | machine        | hook_bearing        |
      | hook           | intent.md           |
      | actor_name     | Persist-Doer-700009 |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-8 |
      | actor_provider | anthropic       |
    Then the PersistPlaybook RPC response kind is "throwaway_kind"
    And a machine.yaml exists at "playbooks/throwaway_kind/machine.yaml" under the persist owner-home
    And a hook file exists at "playbooks/throwaway_kind/hooks/intent.md" under the persist owner-home containing "Hook body for intent.md"

  Scenario: machine referencing a missing hook is rejected with unknown-hook
    Given a hearth directory with the following structure:
      | path                                          | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And the engine is started with that hearth
    And a separate temp owner-home directory
    When the PersistPlaybook RPC is called for kind "throwaway_kind" under that owner-home with:
      | machine        | hook_missing        |
      | actor_name     | Persist-Doer-700010 |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-8 |
      | actor_provider | anthropic       |
    Then the PersistPlaybook RPC returns gRPC status "INVALID_ARGUMENT"
    And the PersistPlaybook RPC error message contains "playbook_unknown_hook_reference"
    And no machine.yaml exists at "playbooks/throwaway_kind/machine.yaml" under the persist owner-home

  Scenario: whitespace-padded owner_home is rejected before direct persist writes
    Given a hearth directory with the following structure:
      | path                                          | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And the engine is started with that hearth
    When the PersistPlaybook RPC is called for kind "throwaway_kind" with raw owner_home " /tmp/anvil-direct-owner" and:
      | actor_name     | Persist-Doer-700006 |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-8 |
      | actor_provider | anthropic       |
    Then the PersistPlaybook RPC returns gRPC status "INVALID_ARGUMENT"
    And the PersistPlaybook RPC error message contains "unresolved_target_owner"
    And no relative playbook directory " /tmp/anvil-direct-owner/playbooks/throwaway_kind" exists under the engine working directory

  Scenario: descriptor owner_home is rejected before direct persist writes
    Given a hearth directory with the following structure:
      | path                                          | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And the engine is started with that hearth
    When the PersistPlaybook RPC is called for kind "throwaway_kind" with raw owner_home "kit:foo" and:
      | actor_name     | Persist-Doer-700007 |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-8 |
      | actor_provider | anthropic       |
    Then the PersistPlaybook RPC returns gRPC status "INVALID_ARGUMENT"
    And the PersistPlaybook RPC error message contains "unresolved_target_owner"
    And no relative playbook directory "kit:foo/playbooks/throwaway_kind" exists under the engine working directory

  Scenario: relative owner_home is rejected before direct persist writes
    Given a hearth directory with the following structure:
      | path                                          | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And the engine is started with that hearth
    When the PersistPlaybook RPC is called for kind "throwaway_kind" with raw owner_home "relative-owner" and:
      | actor_name     | Persist-Doer-700008 |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-8 |
      | actor_provider | anthropic       |
    Then the PersistPlaybook RPC returns gRPC status "INVALID_ARGUMENT"
    And the PersistPlaybook RPC error message contains "unresolved_target_owner"
    And no relative playbook directory "relative-owner/playbooks/throwaway_kind" exists under the engine working directory
