Feature: Begin Playbook Parent Validation
  playbook can be started from no parent or from any existing artifact
  recorded as provenance. Required-parent playbooks keep the proposal/active gate.

  Scenario: workflow_generation starts standalone when no provenance is supplied
    Given an in-memory query adapter with no parents
    When begin is called via query adapter with artifact_type "playbook" and parent "" and playbook_name "my-playbook"
    Then the begin outcome is successful
    And the begin outcome emits a PlaybookCreation event
    And the begin outcome PlaybookCreation has parent_id ""

  Scenario: workflow_generation records any existing artifact as provenance without state-gating it
    Given an in-memory query adapter with parent "20260411T2021_anvil_workflow_engine" in state "draft"
    When begin is called via query adapter with artifact_type "playbook" and parent "20260411T2021_anvil_workflow_engine" and playbook_name "my-playbook"
    Then the begin outcome is successful
    And the begin outcome emits a PlaybookCreation event
    And the begin outcome PlaybookCreation has parent_id "20260411T2021_anvil_workflow_engine"

  Scenario: workflow_generation rejects a provenance id that does not exist
    Given an in-memory query adapter with no parents
    When begin is called via query adapter with artifact_type "playbook" and parent "nonexistent_source" and playbook_name "my-playbook"
    Then the begin outcome is a ParentNotFound error for "nonexistent_source"
    And the handler emitted no events

  Scenario: track creation still rejects a non-active proposal parent
    Given an in-memory query adapter with parent "20260411T2021_anvil_workflow_engine" in state "draft"
    When begin is called via query adapter with artifact_type "track" and parent "20260411T2021_anvil_workflow_engine"
    Then the begin outcome is a ParentNotActive error for "20260411T2021_anvil_workflow_engine"
    And the handler emitted no events
