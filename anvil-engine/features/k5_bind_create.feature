Feature: K5 supervision bind — bind creates exactly one drillable instance (R1/R4/A1/A4)
  A workflow-creation begin under the K5 capability binds the fired run to exactly
  one fresh playbook-driven instance and records a drillable bind carrier: the
  BeginResponse carries the instance's playbook_id, and the persisted instance
  records its machine kind, so a run can be resolved back to its instance. This is
  the exactly-one-instance guarantee (A1) plus the drillable carrier (A4) the
  downstream run->instance->correlation join relies on.

  Scenario: a bindable begin creates exactly one instance carrying a drillable bind id
    Given a hearth seeded with the K5 "bindable" machine
    And the engine is started with that hearth and K5 supervision bind on
    When the begin RPC is called to create a "k5_probe" artifact named "carrier probe" with no parent
    Then the begin RPC response track_path contains "k5_probes"
    And the begin RPC response has non-empty "playbook_id"
    And the begin RPC response track_path file "status.yaml" contains "k5_probe"
    And the hearth contains exactly 1 artifact directories under "k5_probes"

  Scenario: the bind is drillable run->instance->correlation — playbook id on the response, run correlation on the instance (A4/R6)
    Given a hearth seeded with the K5 "bindable" machine
    And the engine is started with that hearth and K5 supervision bind on
    When the begin RPC is called to create a "k5_probe" artifact named "carrier round trip" with no parent for conversation "run-corr-A4" and project root ""
    Then the begin RPC response track_path contains "k5_probes"
    And the begin RPC response has non-empty "playbook_id"
    And the begin RPC response track_path file "status.yaml" contains "k5_probe"
    And the begin RPC response track_path file "status.yaml" contains "run-corr-A4"
