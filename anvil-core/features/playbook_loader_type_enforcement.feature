Feature: Playbook loader enforces types and rejects unknown keys (H4 final bullet, L1 AC)

  # Scope: type enforcement for bool/list fields, plus unknown-key rejection
  # via #[serde(deny_unknown_fields)]. Enforces R7.3's sequential-phase-only
  # guarantee — unknown keys (e.g., event-trigger fields) are rejected, not silently ignored.

  Scenario: non-boolean is_review_gate produces a parse error
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
          is_review_gate: "yes"
          is_terminal: false
      transitions: []
      """
    When the playbook loader parses the file with artifact id "bad-bool" and no hook files
    Then the parse fails with error code "playbook_yaml_parse_error"

  Scenario: non-list roles produces a parse error
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: A track lifecycle.
      required_fields: []
      roles: single_role_not_list
      states:
        - name: spec
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
      transitions: []
      """
    When the playbook loader parses the file with artifact id "bad-roles" and no hook files
    Then the parse fails with error code "playbook_yaml_parse_error"

  Scenario: unknown top-level key is rejected via deny_unknown_fields
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: A track lifecycle.
      required_fields: []
      roles: [doer]
      timeout_advance: 30m
      states:
        - name: spec
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
      transitions: []
      """
    When the playbook loader parses the file with artifact id "unknown-top-key" and no hook files
    Then the parse fails with error code "playbook_yaml_parse_error"

  Scenario: unknown nested key in a state is rejected via deny_unknown_fields
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
          event_trigger: on_pr_merge
      transitions: []
      """
    When the playbook loader parses the file with artifact id "unknown-state-key" and no hook files
    Then the parse fails with error code "playbook_yaml_parse_error"

  Scenario: required_satisfaction with a scalar value produces a parse error
    # M2: required_satisfaction must be a list or null; a plain scalar string
    # is not a valid value for Option<Vec<String>> and must be rejected.
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
          required_satisfaction: definitely_not_a_list
          requires_approver: false
      """
    When the playbook loader parses the file with artifact id "bad-satisfaction" and no hook files
    Then the parse fails with error code "playbook_yaml_parse_error"
