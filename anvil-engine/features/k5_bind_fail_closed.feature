Feature: K5 supervision bind — fail-closed error taxonomy at both seams (R3/R7/A3, §4.3)
  Every reason a bind or a resolve can fail returns a pinned non-OK gRPC status
  with a stable machine-readable reason and leaves NO partial or orphaned
  instance (create-or-nothing): there is no silent-success or silent-fallback
  path. The bind-time classes (§4.3a) fail at `Begin`; the resolve-time classes
  (§4.3b) fail at `Complete`/`Snapshot` on an instance that does not exist. The
  asserted reason strings are the engine's ACTUAL emitted messages; the
  `k5-bind/v1` §4.3 reason labels are the caller-facing vocabulary the fire path
  maps these onto ("unbindable -> do not run"). `machine_not_bindable` (§4.3a) is
  proven in k5_bind_bindability.feature; the byte-identical repeat-resolve no-op
  (an already-terminal instance is NOT a failure) in k5_bind_complete_sync /
  k5_bind_cancel_sync.

  # §4.3a bind-time — playbook_unknown: an unregistered kind for the owner-home.
  Scenario: an unregistered artifact type is rejected create-or-nothing
    Given a hearth seeded with the K5 "bindable" machine
    And the engine is started with that hearth and K5 supervision bind on
    When the begin RPC is called to create a "not_a_registered_kind" artifact named "ghost bind" with no parent
    Then the begin RPC returns gRPC status "INVALID_ARGUMENT"
    And the begin RPC error message contains "Unsupported artifact type"
    And the hearth contains no artifact directories under "k5_probes"

  # §4.3a bind-time — required_field_missing: a machine-declared create field absent.
  Scenario: a missing machine-declared required field is rejected create-or-nothing
    Given a hearth seeded with the K5 "requires-field" machine
    And the engine is started with that hearth and K5 supervision bind on
    When the begin RPC is called to create a "k5_probe" artifact named "field gap bind" with no parent
    Then the begin RPC returns gRPC status "FAILED_PRECONDITION"
    And the begin RPC error message contains "missing_required_field"
    And the hearth contains no artifact directories under "k5_probes"

  # §4.3a bind-time — actor_params_required: the firing actor identity is empty.
  Scenario: an empty firing-actor identity is rejected create-or-nothing
    Given a hearth seeded with the K5 "bindable" machine
    And the engine is started with that hearth and K5 supervision bind on
    When the begin RPC is called to create a "k5_probe" artifact named "no actor bind" with no parent and empty "actor_type"
    Then the begin RPC returns gRPC status "INVALID_ARGUMENT"
    And the begin RPC error message contains "actor_type"
    And the hearth contains no artifact directories under "k5_probes"

  # §4.3b resolve-time — instance_unknown: a run-complete resolve on an unknown path.
  Scenario: a run-complete resolve on an unknown instance path is NOT_FOUND with no state change
    Given a hearth seeded with the K5 "bindable" machine
    And the engine is started with that hearth and K5 supervision bind on
    When the complete RPC is called with:
      | artifact_path  | k5_probes/20990101T0000_no_such_instance |
      | actor_name     | Doer-000001                              |
      | actor_type     | agent                                    |
      | actor_model    | claude-opus-4-8                          |
      | actor_provider | anthropic                                |
    Then the complete RPC returns gRPC status "NOT_FOUND"
    And the hearth contains no artifact directories under "k5_probes"

  # §4.3b resolve-time — instance_unknown: a run-cancel abandon Snapshot on an unknown path.
  Scenario: a run-cancel resolve on an unknown instance path is NOT_FOUND with no state change
    Given a hearth seeded with the K5 "bindable" machine
    And the engine is started with that hearth and K5 supervision bind on
    When the snapshot RPC is called with:
      | artifact_path  | k5_probes/20990101T0000_no_such_instance |
      | to_state       | abandoned                                |
      | actor_name     | Doer-000001                              |
      | actor_role     | doer                                     |
      | actor_type     | agent                                    |
      | actor_model    | claude-opus-4-8                          |
      | actor_provider | anthropic                                |
    Then the snapshot RPC returns gRPC status "NOT_FOUND"
    And the hearth contains no artifact directories under "k5_probes"
