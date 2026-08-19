Feature: Catalog playbook kind
  Playbook artifacts surface in catalog responses when they exist under
  forge/playbooks/. A draft-state playbook with a valid machine.yaml appears
  in active_artifacts. Pre-rename artifacts are read as playbooks, while
  their existing discriminator remains the fallback review path unless they are
  a built-in engine-served lifecycle.

  Scenario: Draft playbook with valid machine.yaml appears in active artifacts
    Given a hearth directory with playbook files:
      | path                                                  | content                                                                                                                                                                                                                                                                                                                                           |
      | playbooks/20260420T1000_test_workflow/status.yaml     | version: 1\nkind: playbook\nstate: draft\ntrack: 20260419T1336_workflow_artifact_kind\n                                                                                                                                                                                                                                                          |
      | playbooks/20260420T1000_test_workflow/machine.yaml    | kind: test_playbook\ndirectory: test_playbooks\nregistry: test_playbooks.md\ndescription: A test playbook\nroles:\n  - doer\n  - reviewer\nrequired_fields: []\nstates:\n  - name: draft\n    registry_section: draft\n    is_review_gate: false\n    is_terminal: false\ntransitions: []\n |
    And the engine is started with that hearth
    When the catalog RPC is called
    Then the catalog response contains 1 active artifacts
    And the catalog response includes artifact "20260420T1000_test_workflow" with type "playbook"
    And the active artifact "20260420T1000_test_workflow" has execution_route "fallback:forge:review"
    And the catalog response does not have invalid artifacts

  @registration
  Scenario: Catalog playbook kind is included in available types
    Given a hearth directory with the following structure:
      | path                                              | state |
      | proposals/20260411T2021_example_proposal/         | active |
    And the engine is started with that hearth
    When the catalog RPC is called
    Then the catalog response contains 7 available artifact types
    And the available types include "playbook" with description containing "lifecycle"
    And the available types include "playbook" requiring parent "track"

  Scenario: Catalog reports playbook kind routability coverage
    Given a hearth directory with the following structure:
      | path                                              | state |
      | proposals/20260411T2021_example_proposal/         | active |
    And the engine is started with that hearth
    When the catalog RPC is called
    Then the catalog available playbook kind "track" reports described "true" and triggers "true"
    And the catalog available playbook kind "playbook" reports described "true" and triggers "true"
    And the catalog available playbook kind "initiative" reports described "true" and triggers "false"
