Feature: /health stays responsive while heavy whole-hearth folds run
  Foundry's runtime watchdog SIGTERM-kills a kit engine after 6 consecutive
  /health probe failures. The engine multiplexes /health, the /ws dashboard, and
  the gRPC RPCs onto ONE tokio runtime and ONE listener, and every /ws dashboard
  method rebuilds the playbook registry and folds the whole hearth off disk
  synchronously. If such a fold runs inline on an async worker, a big or
  concurrent fold starves the /health accept loop and the watchdog false-kills a
  perfectly healthy engine — the observed crash-loop, which worsened as the
  hearth grew. Whole-hearth folds MUST run off the async workers (tokio blocking
  pool) so /health always answers well within the watchdog window.

  Scenario: /health answers under concurrent heavy folds on a single-worker runtime
    Given a playbook activity engine hearth with playbooks:
      | kind       | owner    | description       |
      | lore_query | lore-kit | Answer a question |
    And the hearth activity log has 120000 route entries for kind "lore_query"
    And the engine is started with that hearth on a single worker thread
    When the engine is hammered with concurrent whole-hearth folds while /health is probed
    Then every /health probe returned 200 within the watchdog window
    And the max /health latency under load is under 1000 ms

  # The /ws bridge is not the only surface that folds the whole hearth inline —
  # the gRPC read/route RPCs (driven by the MCP shim, the CLI, and the Forge UI)
  # share the SAME runtime + listener as /health. A concurrent burst of gRPC
  # `route` (registry rebuild every turn) and `playbook_activity` (folds the whole
  # activity log) must not starve /health any more than the /ws folds do. Same
  # single-worker starvation proxy, same watchdog budget — but the load is driven
  # entirely over gRPC.
  Scenario: /health answers under concurrent gRPC route + playbook_activity folds on a single-worker runtime
    Given a playbook activity engine hearth with playbooks:
      | kind       | owner    | description       |
      | lore_query | lore-kit | Answer a question |
    And the hearth activity log has 120000 route entries for kind "lore_query"
    And the engine is started with that hearth on a single worker thread
    When the engine is hammered with concurrent gRPC route and playbook_activity calls while /health is probed
    Then every /health probe returned 200 within the watchdog window
    And the max /health latency under load is under 1000 ms
