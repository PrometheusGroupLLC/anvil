Feature: Machine YAML Live Reload End-to-End
  Proof of Criterion 1 of milestone orchestrator_driven_track_lifecycle:
  editing machine.yaml between two describe calls produces different
  available_actions — no engine restart, no rebuild between calls.

  The engine constructs a fresh HearthPlaybookRegistry on every describe
  RPC call, so on-disk machine.yaml changes are always reflected on the
  next call.

  Scenario: Editing machine.yaml between describe calls changes available_actions without restart
    Given a scratch hearth with a track artifact in state "spec" and the real playbook files
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a describe tools/call is sent with id "20260422T0001_live_reload_track"
    Then the describe response includes action "spec_review"
    # Additively splice a new spec→plan transition into machine.yaml between two
    # describe calls. (An additive edit, rather than removing spec→spec_review,
    # keeps the tightly-chained track machine contiguous so it still registers —
    # the registration-time contiguity gate rejects a machine whose downstream
    # states would be orphaned by an edge removal.) The second describe reflects
    # the on-disk edit with no engine restart: spec now offers a "plan" action.
    When the scratch hearth machine.yaml has the spec-to-spec_review transition removed
    And a describe tools/call is sent with id "20260422T0001_live_reload_track"
    Then the describe response includes action "plan"
