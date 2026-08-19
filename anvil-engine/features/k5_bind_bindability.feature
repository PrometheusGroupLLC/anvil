Feature: K5 supervision bind — bind-time bindability precondition (R10)
  When the K5 supervision-bind capability is enabled, a workflow-creation begin
  binds a fired run to a fresh instance ONLY when the target machine is
  K5-bindable: from its initial state a single unattended doer Complete resolves
  straight to terminal `completed`, and it declares a `-> abandoned` park edge for
  the Snapshot cancel leg. A machine that is not bindable is rejected
  create-or-nothing with FAILED_PRECONDITION / machine_not_bindable, leaving no
  orphan instance behind. The whole precondition is dark-by-default (R8/A7): with
  the capability unset it never fires.

  Scenario: the canonical bindable machine binds to exactly one instance
    Given a hearth seeded with the K5 "bindable" machine
    And the engine is started with that hearth and K5 supervision bind on
    When the begin RPC is called to create a "k5_probe" artifact named "probe one" with no parent
    Then the begin RPC response track_path contains "k5_probes"
    And the begin RPC response track_path file "status.yaml" contains "k5_probe"

  Scenario: a machine whose completed sits behind a review gate is rejected create-or-nothing
    Given a hearth seeded with the K5 "review-gated-completed" machine
    And the engine is started with that hearth and K5 supervision bind on
    When the begin RPC is called to create a "k5_probe" artifact named "probe gated" with no parent
    Then the begin RPC returns gRPC status "FAILED_PRECONDITION"
    And the begin RPC error message contains "machine_not_bindable"
    And the hearth contains no artifact directories under "k5_probes"

  Scenario: a machine needing more than one doer step is rejected create-or-nothing
    Given a hearth seeded with the K5 "two-doer-step" machine
    And the engine is started with that hearth and K5 supervision bind on
    When the begin RPC is called to create a "k5_probe" artifact named "probe two step" with no parent
    Then the begin RPC returns gRPC status "FAILED_PRECONDITION"
    And the begin RPC error message contains "machine_not_bindable"
    And the hearth contains no artifact directories under "k5_probes"

  Scenario: a machine with a sibling forward edge is rejected create-or-nothing
    Given a hearth seeded with the K5 "sibling-forward-edge" machine
    And the engine is started with that hearth and K5 supervision bind on
    When the begin RPC is called to create a "k5_probe" artifact named "probe sibling" with no parent
    Then the begin RPC returns gRPC status "FAILED_PRECONDITION"
    And the begin RPC error message contains "machine_not_bindable"
    And the hearth contains no artifact directories under "k5_probes"

  Scenario: a machine with a null-satisfaction abandon edge is rejected create-or-nothing
    Given a hearth seeded with the K5 "null-sat-abandon" machine
    And the engine is started with that hearth and K5 supervision bind on
    When the begin RPC is called to create a "k5_probe" artifact named "probe abandon collision" with no parent
    Then the begin RPC returns gRPC status "FAILED_PRECONDITION"
    And the begin RPC error message contains "machine_not_bindable"
    And the hearth contains no artifact directories under "k5_probes"

  Scenario: a machine with no abandon park edge is rejected create-or-nothing
    Given a hearth seeded with the K5 "missing-abandon-edge" machine
    And the engine is started with that hearth and K5 supervision bind on
    When the begin RPC is called to create a "k5_probe" artifact named "probe no abandon" with no parent
    Then the begin RPC returns gRPC status "FAILED_PRECONDITION"
    And the begin RPC error message contains "machine_not_bindable"
    And the hearth contains no artifact directories under "k5_probes"

  Scenario: the precondition is dark by default — an unbindable machine binds with the capability unset
    Given a hearth seeded with the K5 "missing-abandon-edge" machine
    And the engine is started with that hearth
    When the begin RPC is called to create a "k5_probe" artifact named "probe dark" with no parent
    Then the begin RPC response track_path contains "k5_probes"
