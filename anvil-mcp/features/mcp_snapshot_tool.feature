Feature: MCP snapshot tool
  The MCP shim exposes a `snapshot` tool whose schema marks
  `actor_name`, `actor_type`, `actor_model`, `actor_provider` as
  required — per spec R5 of the checkin_backfill_spec_context track.
  Non-identity optional fields round out the shape.

  Scenario: Tools list includes snapshot with the correct schema shape
    Given the MCP shim is started
    And the MCP session is initialized
    When a tools/list request is sent
    Then the response contains a tool named "snapshot"
    And the snapshot tool requires property "artifact_path"
    And the snapshot tool requires property "to_state"
    And the snapshot tool requires property "actor_role"
    And the snapshot tool requires property "actor_name"
    And the snapshot tool requires property "actor_type"
    And the snapshot tool requires property "actor_model"
    And the snapshot tool requires property "actor_provider"
    And the snapshot tool has optional property "actor_context_window"
    And the snapshot tool has optional property "actor_sdk_version"
    And the snapshot tool has optional property "actor_entrypoint"
    And the snapshot tool has optional property "approver"
    And the snapshot tool has optional property "note"
    And the snapshot tool has optional property "projection_only"
    And the snapshot tool has optional property "event_type"
