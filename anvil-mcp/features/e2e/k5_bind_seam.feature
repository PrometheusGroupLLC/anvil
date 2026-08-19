Feature: K5 supervision bind through the MCP shim seam (A1/A8 end-to-end)
  The MCP shim's user is Claude Code / the kiln fire path. This proves the K5
  bind vertical end-to-end THROUGH the shim: the shim forwards a workflow-creation
  begin to a running engine that has the K5 capability on (ANVIL_K5_BIND), the
  engine binds (or fails closed on an unbindable machine), and the result surfaces
  back through the shim. The unbindable-rejection scenario is the load-bearing
  proof that the capability is actually ON via the shim: an unbindable machine
  could only bind if the seam were dark, so its create-or-nothing rejection
  (machine_not_bindable) confirms the shim -> K5-engine vertical is wired.

  Scenario: a bindable begin through the shim binds exactly one instance
    Given a hearth seeded with the K5 "bindable" machine
    And the engine is started with that hearth and K5 supervision bind on
    And a .hearth file pointing to that directory while the running engine remains active
    And the MCP shim is started in that working directory with the running engine endpoint
    And the MCP session is initialized while the running engine remains active
    And a checkin tools/call is sent with:
      | field          | value    |
      | role           | creator  |
      | actor_type     | agent    |
      | actor_model    | test     |
      | actor_provider | test     |
    When a begin tools/call is sent with:
      | field          | value          |
      | artifact_type  | k5_probe       |
      | track_name     | k5 shim bind   |
      | actor_name     | Shim-K5-000001 |
      | actor_type     | agent          |
      | actor_model    | test-model     |
      | actor_provider | test           |
    Then the begin response has a track path
    And the hearth status.yaml under "k5_probes" contains "k5_probe"

  Scenario: an unbindable begin through the shim is rejected create-or-nothing
    Given a hearth seeded with the K5 "missing-abandon-edge" machine
    And the engine is started with that hearth and K5 supervision bind on
    And a .hearth file pointing to that directory while the running engine remains active
    And the MCP shim is started in that working directory with the running engine endpoint
    And the MCP session is initialized while the running engine remains active
    And a checkin tools/call is sent with:
      | field          | value    |
      | role           | creator  |
      | actor_type     | agent    |
      | actor_model    | test     |
      | actor_provider | test     |
    When a begin tools/call is sent with:
      | field          | value          |
      | artifact_type  | k5_probe       |
      | track_name     | k5 shim reject |
      | actor_name     | Shim-K5-000002 |
      | actor_type     | agent          |
      | actor_model    | test-model     |
      | actor_provider | test           |
    Then the MCP response is a tool error containing "machine_not_bindable"
    And the hearth contains no artifact directories under "k5_probes"
