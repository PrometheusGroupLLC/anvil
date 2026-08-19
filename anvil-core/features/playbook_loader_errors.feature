Feature: Playbook loader returns named error codes (H3 AC)

  # Scope: one scenario per R5.2 error code. Each scenario provides a
  # specific malformed input and asserts the named error variant with the
  # documented parameters.

  Scenario: invalid YAML returns playbook_yaml_parse_error with line and column
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: [unclosed
      """
    When the playbook loader parses the file with artifact id "bad-yaml" and no hook files
    Then the parse fails with error code "playbook_yaml_parse_error"
    And the error carries artifact_id "bad-yaml"
    And the error carries a line number
    And the error carries a column number

  Scenario: missing required top-level key returns playbook_missing_required_key
    Given a playbook machine.yaml with content:
      """
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
    When the playbook loader parses the file with artifact id "missing-kind" and no hook files
    Then the parse fails with error code "playbook_missing_required_key"
    And the error carries artifact_id "missing-kind"
    And the error carries key_path "kind"

  Scenario: transition referencing undeclared role returns playbook_unknown_role_reference
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
          required_role: nonexistent_role
          required_satisfaction: ~
          requires_approver: false
      """
    When the playbook loader parses the file with artifact id "bad-role" and no hook files
    Then the parse fails with error code "playbook_unknown_role_reference"
    And the error carries artifact_id "bad-role"
    And the error carries from_state "spec"
    And the error carries to_state "spec_review"
    And the error carries role "nonexistent_role"

  Scenario: transition referencing undeclared from_state returns playbook_unknown_state_reference
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
      transitions:
        - from_state: nonexistent_state
          to_state: spec
          required_role: doer
          required_satisfaction: ~
          requires_approver: false
      """
    When the playbook loader parses the file with artifact id "bad-from-state" and no hook files
    Then the parse fails with error code "playbook_unknown_state_reference"
    And the error carries artifact_id "bad-from-state"
    And the error carries unknown_state "nonexistent_state"

  Scenario: transition referencing undeclared to_state returns playbook_unknown_state_reference
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
      transitions:
        - from_state: spec
          to_state: nonexistent_target
          required_role: doer
          required_satisfaction: ~
          requires_approver: false
      """
    When the playbook loader parses the file with artifact id "bad-to-state" and no hook files
    Then the parse fails with error code "playbook_unknown_state_reference"
    And the error carries artifact_id "bad-to-state"
    And the error carries unknown_state "nonexistent_target"

  Scenario: review gate with no required_satisfaction on outgoing transition returns playbook_review_gate_missing_satisfaction
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
          projection_targets: []
          is_review_gate: true
          is_terminal: false
        - name: plan
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
      transitions:
        - from_state: spec_review
          to_state: plan
          required_role: doer
          required_satisfaction: ~
          requires_approver: false
      """
    When the playbook loader parses the file with artifact id "bad-review-gate" and no hook files
    Then the parse fails with error code "playbook_review_gate_missing_satisfaction"
    And the error carries artifact_id "bad-review-gate"
    And the error carries state "spec_review"

  Scenario: state hook referencing absent hook file returns playbook_unknown_hook_reference
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
          hook: missing_hook.md
      transitions: []
      """
    When the playbook loader parses the file with artifact id "bad-hook" and no hook files
    Then the parse fails with error code "playbook_unknown_hook_reference"
    And the error carries artifact_id "bad-hook"
    And the error carries context "state:spec"
    And the error carries filename "missing_hook.md"

  Scenario: transition hook referencing absent hook file returns playbook_unknown_hook_reference
    # M3: the transition-hook error path (context "transition:X→Y") was not
    # covered in the original Phase 1 scenarios. This scenario exercises the
    # code path in validate_with_id that emits context in "transition:X→Y" format.
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
          hook: missing_transition_hook.md
      """
    When the playbook loader parses the file with artifact id "bad-transition-hook" and no hook files
    Then the parse fails with error code "playbook_unknown_hook_reference"
    And the error carries artifact_id "bad-transition-hook"
    And the error carries context "transition:spec→spec_review"
    And the error carries filename "missing_transition_hook.md"
