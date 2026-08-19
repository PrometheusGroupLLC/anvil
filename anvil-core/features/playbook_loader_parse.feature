Feature: Playbook loader parses machine.yaml per-key (H4 AC)

  # Scope: one scenario per R2.4 schema key, asserting the loader reads each
  # key from machine.yaml into the in-memory PlaybookMachine representation.
  # ~15 scenarios covering every field from the R2.4 bullet list.

  Scenario: loader reads top-level kind field
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
      transitions: []
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse succeeds
    And the loaded playbook kind is "track"

  Scenario: loader reads top-level directory field
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
      transitions: []
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse succeeds
    And the loaded playbook directory is "tracks"

  Scenario: loader reads top-level registry field
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
      transitions: []
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse succeeds
    And the loaded playbook registry is "tracks.md"

  Scenario: loader reads top-level description field
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
      transitions: []
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse succeeds
    And the loaded playbook description is "A track lifecycle."

  Scenario: loader reads parent_kind when present
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      parent_kind: proposal
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
      transitions: []
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse succeeds
    And the loaded playbook parent_kind is "proposal"

  Scenario: loader accepts absent parent_kind as None
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
      transitions: []
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse succeeds
    And the loaded playbook parent_kind is absent

  Scenario: loader reads route trigger phrases
    Given a playbook machine.yaml with content:
      """
      kind: daily_recap
      directory: daily_recaps
      registry: daily_recaps.md
      description: A daily recap lifecycle.
      route:
        triggers: ["daily recap", "end of day recap", "today's recap", "recap"]
      required_fields: []
      roles: [doer]
      states:
        - name: gathering
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
      transitions: []
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse succeeds
    And the loaded playbook route triggers are:
      | trigger          |
      | daily recap      |
      | end of day recap |
      | today's recap    |
      | recap            |

  Scenario: loader reads route description for LLM routing
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: Concrete implementation of a proposal slice.
      route:
        description: Route here when the user asks to IMPLEMENT a feature. NOT for authoring a new workflow definition.
        triggers: ["implement a feature"]
      required_fields: []
      roles: [doer]
      states:
        - name: spec
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
      transitions: []
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse succeeds
    And the loaded playbook route description is "Route here when the user asks to IMPLEMENT a feature. NOT for authoring a new workflow definition."
    And the loaded playbook route triggers are:
      | trigger             |
      | implement a feature |

  Scenario: loader defaults absent route block to empty triggers
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
      transitions: []
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse succeeds
    And the loaded playbook route triggers are empty

  Scenario: loader reads required_fields list with all sub-fields
    Given a playbook machine.yaml with content:
      """
      kind: workflow
      directory: workflows
      registry: workflows.md
      description: Workflow kind.
      required_fields:
        - name: workflow_name
          field_type: string
          description: The name of the workflow.
        - name: parent_id
          field_type: artifact_id
          description: Parent track id.
      roles: [doer]
      states:
        - name: draft
          role_filters: []
          registry_section: draft
          projection_targets: []
          is_review_gate: false
          is_terminal: false
      transitions: []
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse succeeds
    And the loaded playbook has 2 required fields
    And required field 0 has name "workflow_name" type "string" description "The name of the workflow."
    And required field 1 has name "parent_id" type "artifact_id" description "Parent track id."

  Scenario: loader reads roles list
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
      transitions: []
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse succeeds
    And the loaded playbook roles are "[\"doer\", \"reviewer\"]"

  Scenario: loader reads state fields — name, registry_section, projection_targets, is_review_gate, is_terminal
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: A track lifecycle.
      required_fields: []
      roles: [doer]
      states:
        - name: spec_review
          role_filters: [review_pending]
          registry_section: active
          projection_targets: [execution.md]
          is_review_gate: true
          is_terminal: false
        - name: completed
          role_filters: [terminal]
          registry_section: completed
          projection_targets: []
          is_review_gate: false
          is_terminal: true
      transitions: []
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse succeeds
    And the loaded playbook has 2 states
    And state 0 has name "spec_review" registry_section "active" is_review_gate "true" is_terminal "false"
    And state 0 has role_filters "[\"review_pending\"]"
    And state 0 has projection_targets "[\"execution.md\"]"
    And state 1 has name "completed" registry_section "completed" is_review_gate "false" is_terminal "true"

  Scenario: loader reads optional hook on a state
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
          hook: spec_entry.md
      transitions: []
      """
    When the playbook loader parses the file with artifact id "test-artifact" and hook files "[\"spec_entry.md\"]"
    Then the parse succeeds
    And state 0 has hook "spec_entry.md"

  Scenario: loader reads state with absent hook as None
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
      transitions: []
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse succeeds
    And state 0 has no hook

  Scenario: loader reads transition fields — from_state, to_state, required_role, requires_approver
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
        - name: spec_review
          role_filters: [review_pending]
          registry_section: active
          projection_targets: []
          is_review_gate: true
          is_terminal: false
      transitions:
        - from_state: spec
          to_state: spec_review
          required_role: doer
          required_satisfaction: ~
          requires_approver: false
        - from_state: spec_review
          to_state: spec
          required_role: reviewer
          required_satisfaction: [satisfied, rejected]
          requires_approver: true
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse succeeds
    And the loaded playbook has 2 transitions
    And transition 0 has from_state "spec" to_state "spec_review" required_role "doer" requires_approver "false"
    And transition 0 has no required_satisfaction
    And transition 1 has from_state "spec_review" to_state "spec" required_role "reviewer" requires_approver "true"
    And transition 1 has required_satisfaction "[\"satisfied\", \"rejected\"]"

  Scenario: loader reads optional hook on a transition
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
        - name: spec_review
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
      transitions:
        - from_state: spec
          to_state: spec_review
          required_role: doer
          required_satisfaction: ~
          requires_approver: false
          hook: transition_spec_to_review.md
      """
    When the playbook loader parses the file with artifact id "test-artifact" and hook files "[\"transition_spec_to_review.md\"]"
    Then the parse succeeds
    And transition 0 has hook "transition_spec_to_review.md"

  Scenario: loader reads multiple role_filters on a state
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
          role_filters: [doer_actionable, creator_parent]
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
      transitions: []
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse succeeds
    And state 0 has role_filters "[\"doer_actionable\", \"creator_parent\"]"

  Scenario: loader reads empty required_fields list
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
      transitions: []
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse succeeds
    And the loaded playbook has 0 required fields
