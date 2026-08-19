Feature: PersistPlaybook MCP tool writes generated playbooks to a resolved owner home
  A surface agent calls the `persist_playbook` MCP tool through the shim. The
  shim forwards the generated machine.yaml to the existing engine RPC, which
  writes it under the caller-supplied owner_home rather than under the engine
  hearth.

  Scenario: persist_playbook is advertised with its schema
    Given a hearth directory with the following structure:
      | path                                           | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a tools/list request is sent
    Then the tools list contains "persist_playbook"
    And the tools list contains "persist_playbook"
    And the "persist_playbook" tool inputSchema requires "owner_home"
    And the "persist_playbook" tool inputSchema requires "kind"
    And the "persist_playbook" tool inputSchema requires "machine_yaml"
    And the "persist_playbook" tool inputSchema requires "actor_name"
    And the "persist_playbook" tool inputSchema requires "actor_type"
    And the "persist_playbook" tool inputSchema requires "actor_model"
    And the "persist_playbook" tool inputSchema requires "actor_provider"
    And the "persist_playbook" tool inputSchema has property "hooks"
    And the "persist_playbook" tool inputSchema requires "owner_home"
    And the "persist_playbook" tool inputSchema requires "kind"
    And the "persist_playbook" tool inputSchema requires "machine_yaml"
    And the "persist_playbook" tool inputSchema requires "actor_name"
    And the "persist_playbook" tool inputSchema requires "actor_type"
    And the "persist_playbook" tool inputSchema requires "actor_model"
    And the "persist_playbook" tool inputSchema requires "actor_provider"
    And the "persist_playbook" tool inputSchema has property "hooks"

  Scenario: persist_playbook writes a valid generated machine to the owner home
    Given a hearth directory with the following structure:
      | path                                           | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    And a separate temp owner-home directory for MCP persist_playbook
    When a "persist_playbook" tools/call is sent for kind "throwaway_kind" with:
      | machine        | minimal          |
      | actor_name     | Persist-MCP-800001 |
      | actor_type     | agent            |
      | actor_model    | claude-opus-4-8  |
      | actor_provider | anthropic        |
    Then the persist_playbook response kind is "throwaway_kind"
    And the persist_playbook response written_path is under the owner-home
    And a machine.yaml exists at "playbooks/throwaway_kind/machine.yaml" under the MCP persist owner-home
    And a fresh registry from the MCP persist owner-home resolves kind "throwaway_kind"

  Scenario: persist_playbook writes a valid generated machine to the owner home
    Given a hearth directory with the following structure:
      | path                                           | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    And a separate temp owner-home directory for MCP persist_playbook
    When a "persist_playbook" tools/call is sent for kind "throwaway_playbook_kind" with:
      | machine        | minimal          |
      | actor_name     | Persist-MCP-800101 |
      | actor_type     | agent            |
      | actor_model    | claude-opus-4-8  |
      | actor_provider | anthropic        |
    Then the persist_playbook response kind is "throwaway_playbook_kind"
    And the persist_playbook response written_path is under the owner-home
    And a machine.yaml exists at "playbooks/throwaway_playbook_kind/machine.yaml" under the MCP persist owner-home
    And a fresh registry from the MCP persist owner-home resolves kind "throwaway_playbook_kind"

  Scenario: persist_playbook carries a role hook through the MCP surface into the persisted playbook
    Given a hearth directory with the following structure:
      | path                                           | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    And a separate temp owner-home directory for MCP persist_playbook
    When a "persist_playbook" tools/call is sent for kind "hooked_kind" with:
      | machine        | hook_bearing       |
      | hook           | intent.md          |
      | actor_name     | Persist-MCP-800201 |
      | actor_type     | agent              |
      | actor_model    | claude-opus-4-8    |
      | actor_provider | anthropic          |
    Then the persist_playbook response kind is "hooked_kind"
    And a machine.yaml exists at "playbooks/hooked_kind/machine.yaml" under the MCP persist owner-home
    And a machine.yaml exists at "playbooks/hooked_kind/hooks/intent.md" under the MCP persist owner-home
    And a fresh registry from the MCP persist owner-home resolves kind "hooked_kind"

  Scenario: a machine referencing a hook not carried through the MCP surface is rejected
    Given a hearth directory with the following structure:
      | path                                           | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    And a separate temp owner-home directory for MCP persist_playbook
    When a "persist_playbook" tools/call is sent for kind "hook_missing_kind" with:
      | machine        | hook_missing       |
      | actor_name     | Persist-MCP-800202 |
      | actor_type     | agent              |
      | actor_model    | claude-opus-4-8    |
      | actor_provider | anthropic          |
    Then the MCP response is a tool error containing "INVALID_ARGUMENT"
    And the MCP response is a tool error containing "playbook_unknown_hook_reference"
    And no machine.yaml exists at "playbooks/hook_missing_kind/machine.yaml" under the MCP persist owner-home

  Scenario: a predicate-less machine is refused at the enforcing write boundary and does not write
    Given a hearth directory with the following structure:
      | path                                           | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    And a separate temp owner-home directory for MCP persist_playbook
    When a "persist_playbook" tools/call is sent for kind "throwaway_kind" with:
      | machine        | no_predicate       |
      | actor_name     | Persist-MCP-800301 |
      | actor_type     | agent              |
      | actor_model    | claude-opus-4-8    |
      | actor_provider | anthropic          |
    Then the MCP response is a tool error containing "INVALID_ARGUMENT"
    And the MCP response is a tool error containing "playbook_measurement_definition_missing"
    And the MCP response is a tool error containing "throwaway_kind"
    And no machine.yaml exists at "playbooks/throwaway_kind/machine.yaml" under the MCP persist owner-home
    And no kind directory exists at "playbooks/throwaway_kind" under the MCP persist owner-home

  Scenario: a machine with a hookless non-terminal state is refused at the enforcing write boundary and does not write
    Given a hearth directory with the following structure:
      | path                                           | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    And a separate temp owner-home directory for MCP persist_playbook
    When a "persist_playbook" tools/call is sent for kind "throwaway_kind" with:
      | machine        | predicate_no_hook  |
      | actor_name     | Persist-MCP-800302 |
      | actor_type     | agent              |
      | actor_model    | claude-opus-4-8    |
      | actor_provider | anthropic          |
    Then the MCP response is a tool error containing "INVALID_ARGUMENT"
    And the MCP response is a tool error containing "playbook_non_terminal_state_hookless"
    And the MCP response is a tool error containing "throwaway_kind"
    And the MCP response is a tool error containing "active"
    And no machine.yaml exists at "playbooks/throwaway_kind/machine.yaml" under the MCP persist owner-home
    And no kind directory exists at "playbooks/throwaway_kind" under the MCP persist owner-home

  Scenario: loader-invalid machine returns an MCP tool error and does not write
    Given a hearth directory with the following structure:
      | path                                           | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    And a separate temp owner-home directory for MCP persist_playbook
    When a "persist_playbook" tools/call is sent for kind "throwaway_kind" with:
      | machine        | loader_invalid   |
      | actor_name     | Persist-MCP-800002 |
      | actor_type     | agent            |
      | actor_model    | claude-opus-4-8  |
      | actor_provider | anthropic        |
    Then the MCP response is a tool error containing "INVALID_ARGUMENT"
    And the MCP response is a tool error containing "playbook_unknown_role_reference"
    And no machine.yaml exists at "playbooks/throwaway_kind/machine.yaml" under the MCP persist owner-home

  Scenario: machine kind mismatch returns an MCP tool error and does not write
    Given a hearth directory with the following structure:
      | path                                           | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    And a separate temp owner-home directory for MCP persist_playbook
    When a "persist_playbook" tools/call is sent for kind "argument_kind" with:
      | machine_kind   | declared_kind      |
      | actor_name     | Persist-MCP-800005 |
      | actor_type     | agent              |
      | actor_model    | claude-opus-4-8    |
      | actor_provider | anthropic          |
    Then the MCP response is a tool error containing "INVALID_ARGUMENT"
    And the MCP response is a tool error containing "kind_mismatch"
    And no machine.yaml exists at "playbooks/argument_kind/machine.yaml" under the MCP persist owner-home

  Scenario: duplicate kind with different content returns an MCP tool error and does not add another entry
    Given a hearth directory with the following structure:
      | path                                           | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    And a separate temp owner-home directory for MCP persist_playbook
    When a "persist_playbook" tools/call is sent for kind "throwaway_kind" with:
      | machine        | minimal          |
      | actor_name     | Persist-MCP-800003 |
      | actor_type     | agent            |
      | actor_model    | claude-opus-4-8  |
      | actor_provider | anthropic        |
    And the MCP persist owner-home playbooks entry count is recorded
    And a "persist_playbook" tools/call is sent for kind "throwaway_kind" with:
      | machine        | different_minimal |
      | actor_name     | Persist-MCP-800003 |
      | actor_type     | agent            |
      | actor_model    | claude-opus-4-8  |
      | actor_provider | anthropic        |
    Then the MCP response is a tool error containing "ALREADY_EXISTS"
    And the MCP response is a tool error containing "playbook_duplicate_kind_registration"
    And the MCP persist owner-home playbooks entry count is unchanged

  Scenario: persist_playbook does not write to the engine hearth playbooks directory
    Given a hearth directory with the following structure:
      | path                                           | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    And a separate temp owner-home directory for MCP persist_playbook
    And the engine hearth playbooks entry count is recorded for MCP persist_playbook
    When a "persist_playbook" tools/call is sent for kind "throwaway_kind" with:
      | machine        | minimal          |
      | actor_name     | Persist-MCP-800004 |
      | actor_type     | agent            |
      | actor_model    | claude-opus-4-8  |
      | actor_provider | anthropic        |
    Then the engine hearth playbooks entry count is unchanged for MCP persist_playbook
    And a fresh registry from the engine hearth does not resolve kind "throwaway_kind"
