Feature: Playbook measurement records are emitted by default
  When a playbook reaches a terminal state, the engine emits one redacted
  playbook_measurement record derived from the playbook machine, even when every
  state's measurement_by_role map is empty.

  Scenario: an unmeasured playbook emits exactly one playbook record on terminal entry
    Given a hearth seeded with the transition_probe playbook with empty measurements
    And the engine is started with that hearth
    When the begin RPC is called to create a "transition_probe" artifact named "phase two probe" with no parent for conversation "surface-session-playbook-001" and project root "/tmp/anvil-playbook-project"
    Then the begin RPC response state is "active"
    And the hearth playbook-measurement sink has exactly 0 records
    When the complete RPC is called on the begin RPC response artifact with satisfaction "" and project root "/tmp/anvil-playbook-project"
    Then the complete RPC response new_state is "completed"
    And the hearth playbook-measurement sink has exactly 1 records
    And the hearth playbook-measurement sink has exactly 1 record for artifact_kind "transition_probe" terminal_state "completed" outcome "terminal_reached" success "true"
    And the hearth playbook-measurement sink has a terminal_state "completed" carrying correlation keys for project root "/tmp/anvil-playbook-project"
    And the hearth playbook-measurement sink record for terminal_state "completed" carries a non-empty playbook_version
    And the "playbook-measurement.jsonl" sink does not contain raw text "surface-session-playbook-001"
    And the "playbook-measurement.jsonl" sink does not contain raw text "/tmp/anvil-playbook-project"

  Scenario: the terminal playbook record carries the quality-vector fields distinct from success
    Given a hearth seeded with the transition_probe playbook with empty measurements
    And the engine is started with that hearth
    When the begin RPC is called to create a "transition_probe" artifact named "quality vector probe" with no parent for conversation "surface-session-playbook-003" and project root "/tmp/anvil-playbook-project"
    And the complete RPC is called on the begin RPC response artifact with satisfaction "" and project root "/tmp/anvil-playbook-project"
    Then the complete RPC response new_state is "completed"
    And the hearth playbook-measurement sink has exactly 1 records
    And the hearth playbook-measurement sink has a terminal_state "completed" carrying quality-vector fields with success "true"

  Scenario: playbook measurement append failure does not block terminal completion
    Given a hearth seeded with the transition_probe playbook with empty measurements
    And the engine is started with that hearth
    And the playbook-measurement sink path is blocked by a directory
    When the begin RPC is called to create a "transition_probe" artifact named "blocked playbook sink probe" with no parent for conversation "surface-session-playbook-002" and project root "/tmp/anvil-playbook-project"
    And the complete RPC is called on the begin RPC response artifact with satisfaction "" and project root "/tmp/anvil-playbook-project"
    Then the complete RPC response new_state is "completed"
    And the activity log sink has a record command "complete" outcome "ok" artifact_kind "transition_probe"

  Scenario: an unmeasured playbook emits joinable structural records at all three granularities
    Given a hearth seeded with the transition_probe playbook with empty measurements
    And the engine is started with that hearth
    When the begin RPC is called to create a "transition_probe" artifact named "structural measurement probe" with no parent for conversation "surface-session-playbook-004" and project root "/tmp/anvil-playbook-project"
    And the complete RPC is called on the begin RPC response artifact with satisfaction "" and project root "/tmp/anvil-playbook-project"
    Then the hearth step-measurement sink has exactly 2 structural records for playbook "transition_probe"
    And the hearth transition-measurement sink has exactly 2 records
    And the hearth playbook-measurement sink has exactly 1 records
    And the three measurement sinks join for the completed playbook run on conversation hash, project label, and playbook run id

  Scenario: an unresolved playbook machine emits a durable coverage gap
    Given a terminal playbook event whose machine cannot be resolved
    When playbook measurement is emitted for the unresolved event
    Then the hearth playbook-measurement sink has exactly 1 records
    And the playbook-measurement record reports terminal reached "false" outcome "measurement_unknown" success "false"
    And the playbook-measurement record carries explicit correlation keys for the unresolved event
