Feature: available_artifact_types returns backlog_item as the 7th creation-surface kind
  R11.3 + K8: available_artifact_types() returns 7 entries (up from 6). The
  playbook entry has requires_parent "track" and a description that names it as
  the definition of another artifact kind's lifecycle. The new backlog_item
  entry is parent-less and is the engine-supported K8 genesis surface.

  @registration
  Scenario: available_artifact_types returns 7 entries including playbook and backlog_item
    Given a describe handler with type schemas
    When available_artifact_types is called
    Then 7 artifact types are returned
    And the artifact type "playbook" is present
    And the artifact type "playbook" has requires_parent "track"
    And the artifact type "playbook" description contains "lifecycle"
    And the artifact type "backlog_item" is present
    And the artifact type "backlog_item" has requires_parent ""
    And the artifact type "backlog_item" description contains "backlog"
