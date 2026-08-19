Feature: Begin RPC records target_owner and enforces the machine-driven required check
  Over the real engine: begin-create a playbook_generation (whose machine declares
  target_owner required) WITH target_owner records it on status.yaml; WITHOUT it
  the engine returns a FAILED_PRECONDITION carrying the `missing_required_field`
  substring. A track create (whose machine does NOT declare target_owner) succeeds
  with no target_owner and writes NO target_owner line (behavior preserved).
  (Anvil-lane 1b; A1/A2/A3, engine e2e over the real machine.yaml.)

  Scenario: playbook_generation create with target_owner records it on status.yaml
    Given a hearth seeded with the builder machine and an active parent track
    And the engine is started with that hearth
    When the begin RPC creates a playbook_generation with target_owner "kit:test-owner"
    Then the e2e artifact status.yaml contains "target_owner: kit:test-owner"
    And the e2e artifact path starts with "workflow_generations/"

  Scenario: playbook_generation create without target_owner is rejected machine-driven
    Given a hearth seeded with the builder machine and an active parent track
    And the engine is started with that hearth
    When the begin RPC creates a playbook_generation with target_owner ""
    Then the begin RPC returns gRPC status "FAILED_PRECONDITION"
    And the begin RPC error message contains "missing_required_field"

  Scenario: track create needs no target_owner and writes no target_owner line (preserved)
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
    And a playbook hook body for the spec doer hook "spec-writing.md" with content "Spec writing guidance."
    And the engine is started with that hearth
    When the begin RPC is called to create a track named "no owner track" under parent "20260411T2021_anvil_workflow_engine"
    Then the begin RPC response has non-empty "track_path"
    And the begin RPC response track_path file "status.yaml" contains "state: spec"
    And the begin RPC response track_path file "status.yaml" does not contain "target_owner:"

  # resume_aware_routing M3: a successful create begin RELIABLY writes the durable
  # resumable open-begin marker (the marker write is load-bearing, not advisory) —
  # so the freshly-scaffolded artifact carries the begin marker stamped with the
  # originating conversation_id, and a later continuation message can resume it.
  Scenario: a create begin reliably writes the resumable marker with the conversation_id
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
    And a playbook hook body for the spec doer hook "spec-writing.md" with content "Spec writing guidance."
    And the engine is started with that hearth
    When the begin RPC is called to create a track named "resumable track" under parent "20260411T2021_anvil_workflow_engine" for conversation "Conv-Create-9"
    Then the begin RPC response has non-empty "track_path"
    And the begin RPC response track_path file "status.yaml" contains "kind: begin"
    And the begin RPC response track_path file "status.yaml" contains "conversation_id: \"Conv-Create-9\""
