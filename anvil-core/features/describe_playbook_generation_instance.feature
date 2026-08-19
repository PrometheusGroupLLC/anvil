Feature: Describe resolves a workflow_generation instance
  FileSystemDescribeAdapter::read_instance must find playbook_generation
  (legacy `workflow_generation`) instances, which live under
  `workflow_generations/<id>/` — a directory that is NOT one of the seven
  ALL_ARTIFACT_TYPES directories. A consumer verifying a generated-playbook
  registration (the kiln RWG.1 admission round-trip) calls
  describe(<generation-id>); before the scanned set included this directory,
  every such call returned UnknownIdentifier. Drives the REAL
  FileSystemDescribeAdapter, not the synthetic TestDescribeAdapter.

  Scenario: read_instance finds a freshly-intaken workflow_generation instance
    Given a describe fs hearth with:
      | path                                                             | content                                                                              |
      | workflow_generations/20260716T2037_generate_playbook/status.yaml | version: 1\nkind: workflow_generation\ntransitions:\n  - to: intake\n  - to: drafting |
    When describe fs read_instance is called for "20260716T2037_generate_playbook"
    Then the describe fs instance state is "drafting"
    And the describe fs instance kind is "workflow_generation"
    And the describe fs instance last_transition to is "drafting"

  Scenario: read_instance returns UnknownIdentifier for a nonexistent id
    Given a describe fs hearth with:
      | path                                                             | content                                                        |
      | workflow_generations/20260716T2037_generate_playbook/status.yaml | version: 1\nkind: workflow_generation\ntransitions:\n  - to: intake |
    When describe fs read_instance is called for "no_such_generation"
    Then the describe fs read is an UnknownIdentifier error
