Feature: Transition measurement records are emitted by default
  Every lifecycle transition emits one redacted transition_measurement record
  derived from the playbook machine, even when every state's measurement_by_role
  map is empty. The record carries public playbook/state/role labels, review
  satisfaction only when applicable, outcome/success, and hashed join keys.

  Scenario: an unmeasured playbook emits exactly one transition record per transition
    Given a hearth seeded with the transition_probe playbook with empty measurements
    And the engine is started with that hearth
    When the begin RPC is called to create a "transition_probe" artifact named "phase one probe" with no parent for conversation "surface-session-transition-001" and project root "/tmp/anvil-transition-project"
    And the complete RPC is called on the begin RPC response artifact with satisfaction "" and project root "/tmp/anvil-transition-project"
    Then the complete RPC response new_state is "completed"
    And the hearth transition-measurement sink has exactly 2 records
    And the hearth transition-measurement sink has exactly 1 record from "" to "active" role "doer" outcome "ok" success "true"
    And the hearth transition-measurement sink has exactly 1 record from "active" to "completed" role "doer" outcome "ok" success "true"
    And the hearth transition-measurement sink has a transition to "completed" carrying correlation keys for project root "/tmp/anvil-transition-project"
    And the "transition-measurement.jsonl" sink does not contain raw text "surface-session-transition-001"
    And the "transition-measurement.jsonl" sink does not contain raw text "/tmp/anvil-transition-project"
    # transition_carries_step_evidence_status phase 3: the two fields reach the
    # durable record. A begin is `pending` — the actor is entering the state, not
    # leaving it — and `transition_probe` declares no artifact of record, so the
    # artifact reads `not_applicable` rather than being guessed at.
    And the transition to "active" records claim "pending" and artifact "not_applicable"
    And the transition to "completed" records claim "not_applicable" and artifact "not_applicable"

  Scenario: transition measurement append failure does not block the transition
    Given a hearth seeded with the transition_probe playbook with empty measurements
    And the engine is started with that hearth
    And the transition-measurement sink path is blocked by a directory
    When the begin RPC is called to create a "transition_probe" artifact named "blocked sink probe" with no parent for conversation "surface-session-transition-002" and project root "/tmp/anvil-transition-project"
    Then the begin RPC response state is "active"
    And the activity log sink has a record command "begin" outcome "ok" artifact_kind "transition_probe"

  # transition_carries_step_evidence_status phase 4 — the WARNING.
  #
  # NOT COVERED AT THIS SEAM, and saying so rather than shipping a scenario that
  # cannot fail. I wrote one here first: it drove a `transition_probe` through
  # begin+complete and asserted only that the new state was "completed". Two
  # things were wrong with it. It never asserted the warning, and
  # `transition_probe` declares no artifact of record — so the assessment is
  # `not_applicable`, the gate cannot fire, and the scenario would have passed
  # forever while testing nothing.
  #
  # The gate itself IS covered, exhaustively and mutation-checked, in
  # anvil-core/features/transition_evidence_fields.feature: dropping the artifact
  # half of the conjunction fails exactly the two false-positive cases. What is
  # missing is proof that a fired warning SURFACES on the complete response, and
  # that needs a fixture whose kind has an artifact of record. Tracked as the
  # remaining work of phase 4 rather than papered over.
