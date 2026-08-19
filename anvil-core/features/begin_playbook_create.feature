Feature: Begin Playbook Create
  begin(artifact_type: "playbook", parent_id: <track-id>, playbook_name: ..., approver: ...)
  creates a new playbook artifact under forge/playbooks/{id}/ in state "draft".

  # Plan Phase 2: happy-path creation asserts event emission and playbook state.
  # At the domain seam, assertions are on emitted events (not filesystem).
  # Filesystem assertions are an engine-seam concern (Phase 3 catalog_workflow_kind.feature).

  Scenario: Happy path: begin playbook emits PlaybookCreation event with correct fields
    Given an in-memory query adapter with track parent "20260419T1336_my_track" in state "active"
    When begin is called via query adapter with artifact_type "playbook" and parent "20260419T1336_my_track" and playbook_name "my-playbook"
    Then the begin outcome is successful
    And the begin outcome emits a PlaybookCreation event
    And the begin outcome PlaybookCreation has playbook_name "my-playbook"
    And the begin outcome PlaybookCreation has parent_id "20260419T1336_my_track"
    And the begin outcome result state is "draft"

  Scenario: Playbook creation initial state is draft
    Given an in-memory query adapter with track parent "20260419T1336_my_track" in state "active"
    When begin is called via query adapter with artifact_type "playbook" and parent "20260419T1336_my_track" and playbook_name "test-playbook"
    Then the begin outcome is successful
    And the begin outcome result state is "draft"
