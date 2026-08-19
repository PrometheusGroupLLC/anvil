Feature: K5 supervision bind — run-complete sync is idempotent (R9)
  The run-complete resolve calls Complete (no satisfaction) on the bound instance,
  riding the single doer edge to terminal `completed`. A Kiln retry on an
  already-completed instance must be an idempotent no-op: it returns OK and writes
  NO second transition to the instance ledger, so the bound instance stays exactly
  once-resolved (A5). The guard is dark-by-default behind ANVIL_K5_BIND.

  Scenario: a repeat Complete on a resolved instance is an idempotent no-op
    Given a hearth seeded with the K5 "bindable" machine
    And the engine is started with that hearth and K5 supervision bind on
    When the begin RPC is called to create a "k5_probe" artifact named "complete probe" with no parent
    Then the begin RPC response track_path contains "k5_probes"
    When the complete RPC is called on the begin RPC response artifact with satisfaction ""
    Then the complete RPC response new_state is "completed"
    When a repeat Complete on the bound instance is a byte-identical no-op
    Then the complete RPC response new_state is "completed"
