Feature: Playbook hooks/ directory is discoverable and load-validated
  R10.1(h): The forge hearth reader exposes a list_playbook_hooks method so
  playbook hooks files are discoverable. The serving half — begin injecting a
  declared (state, role) hook body into context_text / review_context_text —
  landed in the hook_content_serving track and is proven by the positive
  scenarios in begin_create_serves_hook.feature, begin_review_serves_hook.feature,
  begin_create_context_text_sections.feature, begin_spec_review_happy_path.feature,
  and begin_hook_hearth_first.feature. This feature now covers only the
  discoverability listing and the load-time hook-reference validation boundary.

  # Scope: per R5.6, hook files under forge/workflows/{id}/hooks/ are
  # enumerable. Per R5.2 / Phase 1, a machine.yaml referencing a nonexistent
  # hook file surfaces playbook_unknown_hook_reference (also covered by
  # playbook_loader_errors.feature; this feature asserts that boundary together
  # with the discoverability path). The former scenario (b) — which asserted an
  # empty-context negative on the begin(artifact_type:"workflow") creation path —
  # was retired in P5 (hook_content_serving): hooks ARE served now, and the
  # negative premise is misleading. The positive serving scenarios listed above
  # are its replacement.

  Scenario: (a) list_playbook_hooks returns filenames under hooks/ directory
    Given a hearth directory containing playbook "20260420T1000_test_workflow" with hook files:
      | filename            |
      | spec_entry.md       |
      | amend_guidance.md   |
    When list_playbook_hooks is called for playbook "20260420T1000_test_workflow"
    Then the hook filenames include "spec_entry.md"
    And the hook filenames include "amend_guidance.md"
    And exactly 2 hook filenames are returned

  # Scenario (b) retired in P5 (hook_content_serving): it asserted an
  # empty-context negative on the begin(artifact_type:"workflow") creation path,
  # but hooks are now served. See the positive serving scenarios named in the
  # feature header for the replacement assertions.

  Scenario: (c) machine.yaml referencing a nonexistent hook file returns playbook_unknown_hook_reference
    Given a playbook machine.yaml with content:
      """
      kind: my_workflow
      directory: workflows
      registry: workflows.md
      description: A test workflow.
      required_fields: []
      roles: [doer]
      states:
        - name: draft
          role_filters: []
          registry_section: draft
          projection_targets: []
          is_review_gate: false
          is_terminal: false
          hook: nonexistent_hook.md
      transitions: []
      """
    When the playbook loader parses the file with artifact id "hooks-test" and no hook files
    Then the parse fails with error code "playbook_unknown_hook_reference"
    And the error carries artifact_id "hooks-test"
    And the error carries context "state:draft"
    And the error carries filename "nonexistent_hook.md"
