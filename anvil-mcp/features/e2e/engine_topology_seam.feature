Feature: Engine topology seam over one shared daemon
  The MCP shim must treat the engine as an already-running shared daemon. A
  single daemon resolves each request's project hearth, unions global playbook
  kinds, survives restarts, and does not create engines on dial-only
  paths.

  Scenario: Foreign project accepts a global playbook kind through the shared daemon
    Given a foreign project hearth without knowledge_lifecycle and a global playbooks hearth with measured knowledge_lifecycle intent "GLOBAL SEAM INTENT" expected_output "GLOBAL SEAM OUTPUT" body "GLOBAL SEAM BODY"
    And the canonical hearth-less daemon is started with the global playbooks hearth for the seam projects
    And a .hearth file points to the foreign project hearth while the canonical daemon remains active
    And the MCP shim is started in that working directory with canonical daemon discovery
    And the MCP session is initialized while the canonical engine remains active
    When a tools/call request is sent for "catalog" while the canonical engine remains active
    Then the MCP catalog result includes available playbook kind "knowledge_lifecycle"
    And the MCP shim spawned no engine of its own
    And a checkin tools/call is sent with:
      | field          | value   |
      | role           | creator |
      | actor_type     | agent   |
      | actor_model    | test    |
      | actor_provider | test    |
    When a begin tools/call is sent with:
      | field          | value                    |
      | artifact_type  | knowledge_lifecycle      |
      | track_name     | foreign acceptance proof  |
      | actor_type     | agent                    |
      | actor_model    | test-model               |
      | actor_provider | test                     |
    Then the begin response has state "ingesting"
    And the begin response field "playbook_id" is exactly "20260529T0409_knowledge_lifecycle"
    And the begin response field "intent" is exactly "GLOBAL SEAM INTENT"
    And the begin response field "expected_output" is exactly "GLOBAL SEAM OUTPUT"

  Scenario: One daemon serves two project hearths without crossing artifacts
    Given two seam project hearths under one permitted root and a global playbooks hearth with knowledge_lifecycle body "GLOBAL SEAM BODY"
    And the canonical hearth-less daemon is started with the global playbooks hearth for the seam projects
    When two MCP shims in different project hearths call catalog through the same canonical daemon
    Then the first seam project catalog includes artifact "20260419T1100_track_alpha" and not "20260419T1100_track_beta"
    And the second seam project catalog includes artifact "20260419T1100_track_beta" and not "20260419T1100_track_alpha"
    And both seam project shims spawned no fallback engine

  Scenario: Shim recovers when the daemon restarts on the same endpoint
    Given a foreign project hearth without knowledge_lifecycle and a global playbooks hearth with measured knowledge_lifecycle intent "RESTART INTENT" expected_output "RESTART OUTPUT" body "RESTART BODY"
    And the canonical hearth-less daemon is started with the global playbooks hearth for the seam projects
    And a .hearth file points to the foreign project hearth while the canonical daemon remains active
    And the MCP shim is started in that working directory with canonical daemon discovery
    And the MCP session is initialized while the canonical engine remains active
    When a tools/call request is sent for "catalog" while the canonical engine remains active
    Then the MCP catalog result includes available playbook kind "knowledge_lifecycle"
    When the canonical daemon is restarted on the same endpoint
    And a tools/call request is sent for "catalog" while the canonical engine remains active
    Then the MCP catalog result includes available playbook kind "knowledge_lifecycle"
    And the MCP shim spawned no engine of its own

  Scenario: Dial-only shared daemon calls create zero new orphan engines
    Given a foreign project hearth without knowledge_lifecycle and a global playbooks hearth with measured knowledge_lifecycle intent "NO ORPHAN INTENT" expected_output "NO ORPHAN OUTPUT" body "NO ORPHAN BODY"
    And the canonical hearth-less daemon is started with the global playbooks hearth for the seam projects
    And a .hearth file points to the foreign project hearth while the canonical daemon remains active
    And the MCP shim is started in that working directory with canonical daemon discovery
    And the MCP session is initialized while the canonical engine remains active
    When a tools/call request is sent for "catalog" while the canonical engine remains active
    Then the MCP catalog result includes available playbook kind "knowledge_lifecycle"
    And the MCP shim spawned no engine of its own
    And the shim and canonical daemon have no leaked child engines
