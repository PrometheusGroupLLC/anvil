Feature: Catalog invalid playbook artifacts
  Malformed playbook artifacts surface in catalog's invalid_artifacts list,
  not in active_artifacts. The engine does not crash; other valid artifacts
  and kinds continue to respond normally. One artifact's failure does not
  short-circuit other artifacts.

  Scenario: (a) unparseable YAML in machine.yaml surfaces as yaml_parse_error
    Given a hearth directory with playbook files:
      | path                                                       | content                                              |
      | playbooks/20260420T1000_bad_yaml/status.yaml              | version: 1\nkind: playbook\nstate: draft\n           |
      | playbooks/20260420T1000_bad_yaml/machine.yaml             | : this is not valid yaml: [\n                        |
    And the engine is started with that hearth
    When the catalog RPC is called
    Then the catalog response contains 0 active artifacts
    And the catalog response contains 1 invalid artifacts
    And the catalog invalid artifacts include an entry with id "20260420T1000_bad_yaml" and code "playbook_yaml_parse_error"

  Scenario: (b) unknown role reference in machine.yaml surfaces as unknown_role_reference
    Given a hearth directory with playbook files:
      | path                                                              | content                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
      | playbooks/20260420T1000_bad_role/status.yaml                     | version: 1\nkind: playbook\nstate: draft\n                                                                                                                                                                                                                                                                                                                                                                                                                    |
      | playbooks/20260420T1000_bad_role/machine.yaml                    | kind: bad_role_playbook\ndirectory: bad_role_playbooks\nregistry: bad_role_playbooks.md\ndescription: A playbook with bad role\nroles:\n  - doer\n  - reviewer\nrequired_fields: []\nstates:\n  - name: draft\n    registry_section: draft\n    is_review_gate: false\n    is_terminal: false\ntransitions:\n  - from_state: draft\n    to_state: draft\n    required_role: nonexistent_role\n    requires_approver: false\n |
    And the engine is started with that hearth
    When the catalog RPC is called
    Then the catalog response contains 0 active artifacts
    And the catalog response contains 1 invalid artifacts
    And the catalog invalid artifacts include an entry with id "20260420T1000_bad_role" and code "playbook_unknown_role_reference"

  Scenario: (c) one malformed playbook does not block a valid playbook artifact
    Given a hearth directory with playbook files:
      | path                                                        | content                                                                                                                                                                                                                                                                                                                                                          |
      | playbooks/20260420T1000_valid_workflow/status.yaml         | version: 1\nkind: playbook\nstate: draft\n                                                                                                                                                                                                                                                                                                                      |
      | playbooks/20260420T1000_valid_workflow/machine.yaml        | kind: valid_playbook\ndirectory: valid_playbooks\nregistry: valid_playbooks.md\ndescription: A valid playbook\nroles:\n  - doer\n  - reviewer\nrequired_fields: []\nstates:\n  - name: draft\n    registry_section: draft\n    is_review_gate: false\n    is_terminal: false\ntransitions: []\n |
      | playbooks/20260420T1001_malformed_workflow/status.yaml     | version: 1\nkind: playbook\nstate: draft\n                                                                                                                                                                                                                                                                                                                      |
      | playbooks/20260420T1001_malformed_workflow/machine.yaml    | : invalid yaml content [                                                                                                                                                                                                                                                                                                                                         |
    And the engine is started with that hearth
    When the catalog RPC is called
    Then the catalog response contains 1 active artifacts
    And the catalog response includes artifact "20260420T1000_valid_workflow" with type "playbook"
    And the catalog response contains 1 invalid artifacts
    And the catalog invalid artifacts include an entry with id "20260420T1001_malformed_workflow" and code "playbook_yaml_parse_error"

  Scenario: (d) duplicate kind registration surfaces playbook_duplicate_kind_registration
    Given a hearth directory with playbook files:
      | path                                                          | content                                                                                                                                                                                                                                                                                                                                                                     |
      | playbooks/20260420T1000_first_workflow/status.yaml           | version: 1\nkind: playbook\nstate: draft\n                                                                                                                                                                                                                                                                                                                               |
      | playbooks/20260420T1000_first_workflow/machine.yaml          | kind: my_shared_kind\ndirectory: my_shared_kinds\nregistry: my_shared_kinds.md\ndescription: First playbook\nroles:\n  - doer\n  - reviewer\nrequired_fields: []\nstates:\n  - name: draft\n    registry_section: draft\n    is_review_gate: false\n    is_terminal: false\ntransitions: []\n |
      | playbooks/20260420T1001_second_workflow/status.yaml          | version: 1\nkind: playbook\nstate: draft\n                                                                                                                                                                                                                                                                                                                               |
      | playbooks/20260420T1001_second_workflow/machine.yaml         | kind: my_shared_kind\ndirectory: my_shared_kinds\nregistry: my_shared_kinds.md\ndescription: Second playbook with same kind\nroles:\n  - doer\n  - reviewer\nrequired_fields: []\nstates:\n  - name: draft\n    registry_section: draft\n    is_review_gate: false\n    is_terminal: false\ntransitions: []\n |
    And the engine is started with that hearth
    When the catalog RPC is called
    Then the catalog response contains 0 active artifacts
    And the catalog response contains 2 invalid artifacts
    And the catalog invalid artifacts include an entry with id "20260420T1000_first_workflow" and code "playbook_duplicate_kind_registration"
    And the catalog invalid artifacts include an entry with id "20260420T1001_second_workflow" and code "playbook_duplicate_kind_registration"

  Scenario: (e) a playbook whose state declares a PRESENT hook is not flagged unknown_hook_reference
    Given a hearth directory with playbook files:
      | path                                                     | content                                                                                                                                                                                                                                                                                                                                                                              |
      | playbooks/20260420T1002_hooked_workflow/status.yaml     | version: 1\nkind: playbook\nstate: draft\n                                                                                                                                                                                                                                                                                                                                        |
      | playbooks/20260420T1002_hooked_workflow/machine.yaml    | kind: hooked_playbook\ndirectory: hooked_playbooks\nregistry: hooked_playbooks.md\ndescription: A playbook whose state declares a present hook\nroles:\n  - doer\nrequired_fields: []\nstates:\n  - name: draft\n    registry_section: draft\n    is_review_gate: false\n    is_terminal: false\n    hook: draft.md\ntransitions: []\n |
      | playbooks/20260420T1002_hooked_workflow/hooks/draft.md  | # Draft hook\nDo the draft.\n                                                                                                                                                                                                                                                                                                                                                      |
    And the engine is started with that hearth
    When the catalog RPC is called
    Then the catalog response contains 1 active artifacts
    And the catalog response includes artifact "20260420T1002_hooked_workflow" with type "playbook"
    And the catalog response contains 0 invalid artifacts

