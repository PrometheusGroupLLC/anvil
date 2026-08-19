Feature: K5 supervision bind — run-cancel sync is idempotent (R9)
  The run-cancel resolve calls Snapshot(to_state:"abandoned") on the bound
  instance. A Kiln retry on an already-abandoned instance must be an idempotent
  no-op: it returns success and writes NO second transition to the instance
  ledger, so a repeated cancel never double-writes (A6). The guard is
  dark-by-default behind ANVIL_K5_BIND.

  Scenario: a repeat abandon Snapshot on a resolved instance is an idempotent no-op
    Given a hearth seeded with the K5 "bindable" machine
    And the engine is started with that hearth and K5 supervision bind on
    When the begin RPC is called to create a "k5_probe" artifact named "cancel probe" with no parent
    Then the begin RPC response track_path contains "k5_probes"
    When the snapshot RPC is called on the begin RPC response artifact to state "abandoned" with role "doer"
    Then the snapshot RPC response success is "true"
    When a repeat abandon Snapshot on the bound instance is a byte-identical no-op
    Then the snapshot RPC response success is "true"
