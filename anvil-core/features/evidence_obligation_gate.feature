Feature: Loader-side evidence-obligation enforcement (dark-gated)
  Evidence obligations are authored on measured playbook steps. While the gate
  is off, existing DRIVEN playbooks without obligations and parser-first FREE
  playbooks with obligations continue to load. When the gate is on, every
  measured state-and-role pair in a DRIVEN playbook must declare at least one
  evidence class, while FREE playbooks must not declare an obligation.

  States with no measurement entry remain exempt. Event-driven playbooks obey
  the same rules as transition-driven playbooks, including preserving a
  declared obligation through loading.

  # C10 — gate off preserves today's DRIVEN loader behavior.
  Scenario: A DRIVEN measured step without an obligation loads while the gate is off
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: A track lifecycle.
      required_fields: []
      roles: [doer]
      states:
        - name: spec
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
          measurement_by_role:
            doer:
              intent: Write the spec.
              expected_output: A spec.md.
        - name: completed
          role_filters: []
          registry_section: completed
          projection_targets: []
          is_review_gate: false
          is_terminal: true
      transitions:
        - from_state: spec
          to_state: completed
          required_role: doer
          required_satisfaction: ~
          requires_approver: false
      """
    When the playbook loader parses the file with artifact id "legacy-track" and no hook files under evidence obligation enforcement "off"
    Then the parse succeeds

  # C11 — every measured pair is checked, and the refusal is diagnosable.
  Scenario: A DRIVEN measured pair without an obligation is refused while the gate is on
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: A track lifecycle.
      required_fields: []
      roles: [doer, reviewer]
      states:
        - name: spec
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
          measurement_by_role:
            doer:
              intent: Write the spec.
              expected_output: A spec.md.
              evidence_obligation: [artifact_of_consequence]
            reviewer:
              intent: Review the spec.
              expected_output: A review decision.
        - name: completed
          role_filters: []
          registry_section: completed
          projection_targets: []
          is_review_gate: false
          is_terminal: true
      transitions:
        - from_state: spec
          to_state: completed
          required_role: doer
          required_satisfaction: ~
          requires_approver: false
      """
    When the playbook loader parses the file with artifact id "missing-obligation-artifact" and no hook files under evidence obligation enforcement "on"
    Then the parse fails with error code "playbook_evidence_obligation_missing"
    And the error carries artifact_id "missing-obligation-artifact"
    And the evidence obligation error carries detail containing "track"
    And the evidence obligation error carries detail containing "spec"
    And the evidence obligation error carries detail containing "reviewer"

  # C13 — all measured pairs carrying obligations clears the gate.
  Scenario: A DRIVEN playbook whose measured pairs all carry obligations loads while the gate is on
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: A track lifecycle.
      required_fields: []
      roles: [doer, reviewer]
      states:
        - name: spec
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
          measurement_by_role:
            doer:
              intent: Write the spec.
              expected_output: A spec.md.
              evidence_obligation: [artifact_of_consequence]
            reviewer:
              intent: Review the spec.
              expected_output: A review decision.
              evidence_obligation: [verifiable_citation]
        - name: completed
          role_filters: []
          registry_section: completed
          projection_targets: []
          is_review_gate: false
          is_terminal: true
      transitions:
        - from_state: spec
          to_state: completed
          required_role: doer
          required_satisfaction: ~
          requires_approver: false
      """
    When the playbook loader parses the file with artifact id "obligation-complete-track" and no hook files under evidence obligation enforcement "on"
    Then the parse succeeds

  # C14 — an unmeasured state has no obligation to declare.
  Scenario: A DRIVEN state without a measurement entry is exempt while the gate is on
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: A track lifecycle with an unmeasured routing state.
      required_fields: []
      roles: [doer]
      states:
        - name: spec
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
          measurement_by_role:
            doer:
              intent: Write the spec.
              expected_output: A spec.md.
              evidence_obligation: [artifact_of_consequence]
        - name: routing
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
        - name: completed
          role_filters: []
          registry_section: completed
          projection_targets: []
          is_review_gate: false
          is_terminal: true
      transitions:
        - from_state: spec
          to_state: routing
          required_role: doer
          required_satisfaction: ~
          requires_approver: false
        - from_state: routing
          to_state: completed
          required_role: doer
          required_satisfaction: ~
          requires_approver: false
      """
    When the playbook loader parses the file with artifact id "routing-state-track" and no hook files under evidence obligation enforcement "on"
    Then the parse succeeds

  # C15 — event-driven measured pairs reach the same gate.
  Scenario: An event-driven DRIVEN measured step without an obligation is refused
    Given a playbook machine.yaml with content:
      """
      name: event_task
      version: "0.1.0"
      anvil_kind: event_task
      description: An event-driven playbook.
      trigger:
        kind: pending_queue
        poll_tool: list_pending
      steps:
        - id: list_pending
          kind: mcp_call
          tool: list_pending
          terminal: true
      states:
        - name: pending
          is_terminal: false
          on:
            PlaybookStarted: completed
          measurement_by_role:
            doer:
              intent: Process the pending item.
              expected_output: A processed item.
        - name: completed
          is_terminal: true
      """
    When the playbook loader parses the file with artifact id "event-missing-obligation" and no hook files under evidence obligation enforcement "on"
    Then the parse fails with error code "playbook_evidence_obligation_missing"
    And the evidence obligation error carries detail containing "event_task"
    And the evidence obligation error carries detail containing "pending"
    And the evidence obligation error carries detail containing "doer"

  Scenario: An event-driven DRIVEN measured step carrying an obligation loads and preserves it
    Given a playbook machine.yaml with content:
      """
      name: event_task
      version: "0.1.0"
      anvil_kind: event_task
      description: An event-driven playbook.
      trigger:
        kind: pending_queue
        poll_tool: list_pending
      steps:
        - id: list_pending
          kind: mcp_call
          tool: list_pending
          terminal: true
      states:
        - name: pending
          is_terminal: false
          on:
            PlaybookStarted: completed
          measurement_by_role:
            doer:
              intent: Process the pending item.
              expected_output: A processed item.
              evidence_obligation: [verifiable_citation]
        - name: completed
          is_terminal: true
      """
    When the playbook loader parses the file with artifact id "event-with-obligation" and no hook files under evidence obligation enforcement "on"
    Then the parse succeeds
    And the loaded spec for state "pending" role "doer" requires evidence classes "verifiable_citation"

  # C16 — FREE declarations remain parser-first and inert while the gate is off.
  Scenario: A FREE measured step carrying an obligation loads and round-trips while the gate is off
    Given a playbook machine.yaml with content:
      """
      kind: spark
      directory: sparks
      registry: sparks.md
      description: A free spark playbook.
      required_fields: []
      roles: [doer]
      register: free
      states:
        - name: capture
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
          measurement_by_role:
            doer:
              intent: Capture the spark.
              expected_output: A spark entry.
              evidence_obligation: [self_description]
        - name: completed
          role_filters: []
          registry_section: completed
          projection_targets: []
          is_review_gate: false
          is_terminal: true
      transitions:
        - from_state: capture
          to_state: completed
          required_role: doer
          required_satisfaction: ~
          requires_approver: false
      """
    When the playbook loader parses the file with artifact id "free-obligation-parser-first" and no hook files under evidence obligation enforcement "off"
    Then the parse succeeds
    And the loaded spec for state "capture" role "doer" requires evidence classes "self_description"
    When the loaded machine is re-serialized to YAML and reloaded with artifact id "free-obligation-parser-first"
    Then the re-serialized YAML contains an evidence_obligation key
    And the reloaded spec for state "capture" role "doer" requires evidence classes "self_description"

  # C17 loader leg — FREE kinds may not declare obligations when gated.
  Scenario: A FREE measured step carrying an obligation is refused while the gate is on
    Given a playbook machine.yaml with content:
      """
      kind: spark
      directory: sparks
      registry: sparks.md
      description: A free spark playbook.
      required_fields: []
      roles: [doer]
      register: free
      states:
        - name: capture
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
          measurement_by_role:
            doer:
              intent: Capture the spark.
              expected_output: A spark entry.
              evidence_obligation: [self_description]
        - name: completed
          role_filters: []
          registry_section: completed
          projection_targets: []
          is_review_gate: false
          is_terminal: true
      transitions:
        - from_state: capture
          to_state: completed
          required_role: doer
          required_satisfaction: ~
          requires_approver: false
      """
    When the playbook loader parses the file with artifact id "free-obligation-artifact" and no hook files under evidence obligation enforcement "on"
    Then the parse fails with error code "playbook_evidence_obligation_on_free_register"
    And the error carries artifact_id "free-obligation-artifact"
    And the evidence obligation error carries detail containing "spark"

  # C18 — FREE kinds with no declared obligation always load.
  Scenario Outline: A FREE measured step without an obligation loads regardless of the gate
    Given a playbook machine.yaml with content:
      """
      kind: spark
      directory: sparks
      registry: sparks.md
      description: A free spark playbook.
      required_fields: []
      roles: [doer]
      register: free
      states:
        - name: capture
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
          measurement_by_role:
            doer:
              intent: Capture the spark.
              expected_output: A spark entry.
        - name: completed
          role_filters: []
          registry_section: completed
          projection_targets: []
          is_review_gate: false
          is_terminal: true
      transitions:
        - from_state: capture
          to_state: completed
          required_role: doer
          required_satisfaction: ~
          requires_approver: false
      """
    When the playbook loader parses the file with artifact id "free-without-obligation" and no hook files under evidence obligation enforcement "<gate>"
    Then the parse succeeds

    Examples:
      | gate |
      | off  |
      | on   |
