Feature: Tool RPCs reject + guide on an unresolvable hearth
  When an LLM-facing tool call (catalog, begin, checkin, snapshot, complete, …)
  supplies NO hearth_path and the engine has no default hearth to charge, the
  engine returns a TYPED precondition error whose message tells the model how to
  resolve the hearth — instead of silently defaulting attribution into the global
  anvil-hearth. This lets the model retry correctly and self-heals undocumented
  projects. Every tool RPC shares the same `resolve_hearth` gate, so catalog
  stands in for the whole family.

  Scenario: catalog with no hearth_path against a hearth-less engine returns self-heal guidance
    Given a standard hearth directory X and a hearth-less engine
    When the catalog RPC is called with no hearth_path
    Then the catalog RPC returns gRPC status "FAILED_PRECONDITION"
    And the catalog RPC error message contains "pass hearth_path"
    And the catalog RPC error message contains "create `.hearth`"
