Feature: Playbook access schema

  BP1 (workflow_access_scoping): a machine declares access tags in machine.yaml.
  This is schema-only; it does not grant, filter, route, or begin playbooks.

  Scenario: loader reads a full access block
    Given a playbook machine.yaml with content:
      """
      kind: restricted_track
      directory: tracks
      registry: tracks.md
      description: A restricted track lifecycle.
      access:
        org: Acme
        min_role: write
        sensitivity: phi
        space: secure_lab
      required_fields: []
      roles: [doer]
      states:
        - name: active
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
      transitions: []
      """
    When the playbook loader parses the file with artifact id "restricted-track" and no hook files
    Then the parse succeeds
    And the loaded playbook access is org "Acme" min_role "write" sensitivity "phi" space "secure_lab"

  Scenario: loader defaults missing access tags to Foundation read internal with no space
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: A track lifecycle.
      required_fields: []
      roles: [doer]
      states:
        - name: active
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
      transitions: []
      """
    When the playbook loader parses the file with artifact id "track" and no hook files
    Then the parse succeeds
    And the loaded playbook access is org "Foundation" min_role "read" sensitivity "internal" with no space
