Feature: K5 supervision bind — one instance per run (idempotent bind, R2/A2)
  A fire-path begin correlated to a run that already has an open (begun,
  non-terminal) instance must resolve and return THAT existing instance rather
  than mint a second — the exactly-one-instance-per-run guarantee the downstream
  run->instance->correlation join relies on. The correlation is the run token
  carried on `BeginRequest.conversation_id`; a Kiln retry of the same run is an
  idempotent no-op that returns the same `playbook_run_id`. The dedup is
  dark-by-default behind ANVIL_K5_BIND: with the capability unset the bind path
  is byte-identically the pre-K5 behaviour (each begin mints a fresh instance).

  Scenario: a repeat begin on the same run correlation returns the same bound instance
    Given a hearth seeded with the K5 "bindable" machine
    And the engine is started with that hearth and K5 supervision bind on
    When the begin RPC is called to create a "k5_probe" artifact named "first bind" with no parent for conversation "run-corr-101" and project root ""
    Then the begin RPC response track_path contains "k5_probes"
    When a repeat begin on conversation "run-corr-101" to create a "k5_probe" artifact resolves to the same bound instance
    Then the hearth contains exactly 1 artifact directories under "k5_probes"

  Scenario: the idempotent bind is dark by default — two begins on one correlation mint two instances with the capability unset
    Given a hearth seeded with the K5 "bindable" machine
    And the engine is started with that hearth
    When the begin RPC is called to create a "k5_probe" artifact named "dark first" with no parent for conversation "run-corr-dark" and project root ""
    Then the begin RPC response track_path contains "k5_probes"
    When the begin RPC is called to create a "k5_probe" artifact named "dark second" with no parent for conversation "run-corr-dark" and project root ""
    Then the begin RPC response track_path contains "k5_probes"
    And the hearth contains exactly 2 artifact directories under "k5_probes"
