Feature: PlaybookRegistry — seed adapter resolves kind names to machines

  # Amendment Gap 2: PlaybookRegistry trait at the consumer boundary.
  # SeedPlaybookRegistry resolves kind names to compiled-in seeds.
  # The interpreter signature is unchanged — outgoing_transitions takes
  # &PlaybookMachine, not the registry. The registry is used BY consumers
  # (Phase 5: describe::available_actions), not by the interpreter.
  #
  # This port is the CQRS seam for kind-name → machine resolution.
  # Track #14's hearth-loader adapter swaps in later without touching consumers.

  Scenario: machine_for "track" returns the track seed
    When the seed registry resolves kind "track"
    Then the resolved machine has kind "track"

  Scenario: machine_for "playbook" returns the playbook seed
    When the seed registry resolves kind "playbook"
    Then the resolved machine has kind "playbook"

  Scenario: machine_for "workflow" no longer aliases the playbook seed
    When the seed registry resolves kind "workflow"
    Then the resolved machine is absent

  Scenario: machine_for "proposal" returns the proposal seed
    When the seed registry resolves kind "proposal"
    Then the resolved machine has kind "proposal"

  @registration
  Scenario: machine_for "backlog_item" returns the K8 backlog_item seed
    When the seed registry resolves kind "backlog_item"
    Then the resolved machine has kind "backlog_item"

  Scenario: machine_for an unknown kind returns None
    When the seed registry resolves kind "unknown_kind"
    Then the resolved machine is absent

  # M2 (hook_content_serving P5): workflow_id_for must agree with machine_for —
  # every kind the seed resolves to a machine also resolves to a playbook id,
  # so hook-body reads have a workflow_id for any seed-served (state, role) hook.
  Scenario: workflow_id_for "track" returns the track lifecycle id
    When the seed registry resolves playbook id for kind "track"
    Then the resolved playbook id is "20260422T0000_track_lifecycle"

  Scenario: workflow_id_for "playbook" returns the playbook lifecycle id
    When the seed registry resolves playbook id for kind "playbook"
    Then the resolved playbook id is "playbook_lifecycle"

  Scenario: workflow_id_for "workflow" no longer aliases the playbook lifecycle id
    When the seed registry resolves playbook id for kind "workflow"
    Then the resolved playbook id is absent

  Scenario: workflow_id_for "proposal" returns the proposal lifecycle id
    When the seed registry resolves playbook id for kind "proposal"
    Then the resolved playbook id is "proposal_lifecycle"

  @registration
  Scenario: workflow_id_for "backlog_item" returns the backlog_item lifecycle id
    When the seed registry resolves playbook id for kind "backlog_item"
    Then the resolved playbook id is "backlog_item_lifecycle"

  Scenario: workflow_id_for an unknown kind returns None
    When the seed registry resolves playbook id for kind "unknown_kind"
    Then the resolved playbook id is absent
