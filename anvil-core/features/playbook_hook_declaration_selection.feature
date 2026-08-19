Feature: Playbook hook declaration and pure selection
  Phase 1 proves playbook-package hook metadata can be loaded, validated,
  and selected by pure interpreter helpers without filesystem, global state,
  registry writes, or Anvil-side hook execution.

  Scenario: valid state hook declaration on spec loads when the hook file exists
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: Track lifecycle with hook.
      required_fields: []
      roles: [doer]
      states:
        - name: spec
          role_filters: []
          registry_section: spec
          projection_targets: []
          is_review_gate: false
          is_terminal: false
          hook: spec-entry.md
      transitions: []
      """
    When the playbook loader parses the file with artifact id "hook-valid" and hook files "[\"spec-entry.md\"]"
    Then the parse succeeds
    And state 0 has hook "spec-entry.md"

  Scenario: missing state hook file is rejected with playbook_unknown_hook_reference
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: Track lifecycle with missing hook.
      required_fields: []
      roles: [doer]
      states:
        - name: spec
          role_filters: []
          registry_section: spec
          projection_targets: []
          is_review_gate: false
          is_terminal: false
          hook: missing-entry.md
      transitions: []
      """
    When the playbook loader parses the file with artifact id "hook-missing" and no hook files
    Then the parse fails with error code "playbook_unknown_hook_reference"
    And the error carries artifact_id "hook-missing"
    And the error carries context "state:spec"
    And the error carries filename "missing-entry.md"

  Scenario: unsafe state hook path is rejected with hook_path_invalid
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: Track lifecycle with unsafe hook path.
      required_fields: []
      roles: [doer]
      states:
        - name: spec
          role_filters: []
          registry_section: spec
          projection_targets: []
          is_review_gate: false
          is_terminal: false
          hook: ../secret.md
      transitions: []
      """
    When the playbook loader parses the file with artifact id "hook-unsafe" and hook files "[\"../secret.md\"]"
    Then the parse fails with error code "hook_path_invalid"
    And the error carries artifact_id "hook-unsafe"
    And the error carries context "state:spec"
    And the error carries filename "../secret.md"

  Scenario: spec state hook is selected by pure interpreter helper
    Given a fixture playbook machine with state hooks:
      | state | hook          |
      | spec  | spec-entry.md |
    When state_hook is called for state "spec"
    Then the selected hook has scope "state" state "spec" and filename "spec-entry.md"

  Scenario: non-selected state produces no hook selection
    Given a fixture playbook machine with state hooks:
      | state | hook          |
      | spec  | spec-entry.md |
    When state_hook is called for state "plan"
    Then no hook is selected

  # ---- P1 role-aware selector scenarios ----

  Scenario: state_role_hook selects doer hook when state declares hooks_by_role
    Given a fixture playbook machine with state hooks:
      | state | hook | doer_hook       | reviewer_hook    |
      | spec  |      | spec-writing.md | spec-review.md   |
      | plan  |      |                 |                  |
    When state_role_hook is called for state "spec" role "doer"
    Then the selected hook has scope "state_role" state "spec" and filename "spec-writing.md"

  Scenario: state_role_hook selects reviewer hook when state declares hooks_by_role
    Given a fixture playbook machine with state hooks:
      | state | hook | doer_hook       | reviewer_hook    |
      | spec  |      | spec-writing.md | spec-review.md   |
    When state_role_hook is called for state "spec" role "reviewer"
    Then the selected hook has scope "state_role" state "spec" and filename "spec-review.md"

  Scenario: state_role_hook falls back to role-agnostic hook when role key absent
    Given a fixture playbook machine with state hooks:
      | state | hook          | doer_hook | reviewer_hook |
      | spec  | spec-entry.md |           |               |
    When state_role_hook is called for state "spec" role "doer"
    Then the selected hook has scope "state" state "spec" and filename "spec-entry.md"

  Scenario: state_role_hook returns none when state has no hook at all
    Given a fixture playbook machine with state hooks:
      | state | hook | doer_hook | reviewer_hook |
      | spec  |      |           |               |
      | plan  |      |           |               |
    When state_role_hook is called for state "plan" role "doer"
    Then no hook is selected

  Scenario: state_hook still returns role-agnostic hook unchanged (regression)
    Given a fixture playbook machine with state hooks:
      | state | hook          | doer_hook       | reviewer_hook  |
      | spec  | spec-entry.md | spec-writing.md | spec-review.md |
    When state_hook is called for state "spec"
    Then the selected hook has scope "state" state "spec" and filename "spec-entry.md"

  # ---- P1 loader scenarios for hooks_by_role ----

  Scenario: hooks_by_role value pointing at missing file returns playbook_unknown_hook_reference
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: Track with missing role hook.
      required_fields: []
      roles: [doer, reviewer]
      states:
        - name: spec
          role_filters: []
          registry_section: spec
          projection_targets: []
          is_review_gate: false
          is_terminal: false
          hooks_by_role:
            doer: missing-role-hook.md
      transitions: []
      """
    When the playbook loader parses the file with artifact id "role-hook-missing" and no hook files
    Then the parse fails with error code "playbook_unknown_hook_reference"
    And the error carries artifact_id "role-hook-missing"
    And the error carries context "state:spec:role:doer"
    And the error carries filename "missing-role-hook.md"

  Scenario: hooks_by_role value with unsafe path returns hook_path_invalid
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: Track with unsafe role hook path.
      required_fields: []
      roles: [doer, reviewer]
      states:
        - name: spec
          role_filters: []
          registry_section: spec
          projection_targets: []
          is_review_gate: false
          is_terminal: false
          hooks_by_role:
            doer: ../secret.md
      transitions: []
      """
    When the playbook loader parses the file with artifact id "role-hook-unsafe" and hook files "[\"../secret.md\"]"
    Then the parse fails with error code "hook_path_invalid"
    And the error carries artifact_id "role-hook-unsafe"
    And the error carries context "state:spec:role:doer"
    And the error carries filename "../secret.md"

  Scenario: hooks_by_role key not in machine roles list returns playbook_unknown_role_key
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: Track with unknown role key.
      required_fields: []
      roles: [doer, reviewer]
      states:
        - name: spec
          role_filters: []
          registry_section: spec
          projection_targets: []
          is_review_gate: false
          is_terminal: false
          hooks_by_role:
            unknown_role: spec-writing.md
      transitions: []
      """
    When the playbook loader parses the file with artifact id "role-key-invalid" and hook files "[\"spec-writing.md\"]"
    Then the parse fails with error code "playbook_unknown_role_key"
    And the error carries artifact_id "role-key-invalid"
    And the error carries context "state:spec:role:unknown_role"

  # ---- P1 workflow_seed_yaml_equivalence regression with doer in roles ----

  Scenario: track seed and machine.yaml still agree after adding doer to roles
    Given the track lifecycle machine.yaml is loaded from the fixture
    And the track seed is loaded from the compiled-in seed
    Then the two PlaybookMachine structs are structurally equal
