Feature: Standalone mode is unchanged when no Foundry session token is present (spec Req 3)
  With no FOUNDRY_SESSION_TOKEN in the environment the full MCP shim + engine
  subprocess stack must behave exactly as it did before the Foundry auth track:
  self-asserted actor_name is accepted verbatim, no bearer metadata is attached,
  no refusal occurs.  This is the standing no-regression proof for spec Req 3.

  Background:
    Given a hearth directory with the following structure:
      | path                                              | state  |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |

  # ── catalog: returns active artifacts, no auth error ─────────────────────────

  Scenario: Standalone catalog succeeds with no session token
    Given the engine is started with that hearth
    And a .hearth file pointing to that directory while the running engine remains active
    And the MCP shim is started in that working directory with the running engine endpoint
    And the MCP session is initialized while the running engine remains active
    When a tools/call request is sent for "catalog"
    Then the MCP response contains active artifacts
    And the MCP catalog result includes artifact "20260411T2021_anvil_workflow_engine"

  # ── begin: caller-supplied actor_name is persisted verbatim ───────────────────
  # In Foundry mode the engine overrides actor_name with foundry:<sub>.
  # In standalone the self-asserted name must land in status.yaml unchanged.
  # The full checkin → begin chain exercises the create playbook exactly as today.

  Scenario: Standalone begin persists caller-supplied actor_name verbatim
    Given a playbook hook body for the spec doer hook "spec-writing.md" with content "Standalone spec writing."
    And the engine is started with that hearth
    And a .hearth file pointing to that directory while the running engine remains active
    And the MCP shim is started in that working directory with the running engine endpoint
    And the MCP session is initialized while the running engine remains active
    When a checkin tools/call is sent with role "creator" and:
      | field          | value               |
      | actor_name     | StandaloneActor-000001 |
      | actor_type     | agent               |
      | actor_model    | claude-test         |
      | actor_provider | anthropic           |
    Then the checkin response has a generated actor name
    When a begin tools/call is sent with:
      | field         | value                               |
      | artifact_type | track                               |
      | parent_id     | 20260411T2021_anvil_workflow_engine  |
      | track_name    | standalone-proof-track              |
      | approver      | mark                                |
      | actor_name    | StandaloneActor-000001              |
      | actor_type    | agent                               |
      | actor_model   | claude-test                         |
      | actor_provider| anthropic                           |
    Then the begin response has state "spec"
    And the begin response has a track path
    And the hearth has a new track directory with status.yaml
    And the hearth's new track status.yaml contains "StandaloneActor-000001"
