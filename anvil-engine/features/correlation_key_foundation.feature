Feature: Correlation keys are stamped on durable join-participant records
  Durable route, lifecycle, and step records carry hashed conversation identity,
  a non-path project label, and playbook instance identity where applicable. Raw
  conversation ids and project roots never land on disk. Checkin is excluded
  from the join contract.

  Scenario: Route durable record carries hashed conversation and project label
    Given a route hearth seeded with consulting recap fixtures plus seed playbooks
    And the engine is started with that hearth
    When the route RPC is called with message "daily recap", signal "", conversation_id "surface-session-route-001", and project root "/tmp/anvil-correlation-project"
    Then the route outcome is "candidates"
    And the route resolution outcome is "single"
    And the routing activity sink carries correlation keys for project root "/tmp/anvil-correlation-project"
    And the "routing-activity.jsonl" sink does not contain raw text "surface-session-route-001"
    And the "routing-activity.jsonl" sink does not contain raw text "/tmp/anvil-correlation-project"

  Scenario: Begin and complete activity and lean step records carry inherited correlation keys
    Given a hearth seeded with the lore_query run-backed machine
    And the engine is started with that hearth
    When the begin RPC is called to create a "lore_query" artifact named "ask about plans" with no parent for conversation "surface-session-lifecycle-001" and project root "/tmp/anvil-correlation-project"
    And the complete RPC is called on the begin RPC response artifact with satisfaction "" and project root "/tmp/anvil-correlation-project"
    Then the complete RPC response new_state is "completed"
    And the activity log command "begin" carries correlation keys for project root "/tmp/anvil-correlation-project"
    And the activity log command "complete" carries correlation keys for project root "/tmp/anvil-correlation-project"
    And the activity log command "complete" has the same conversation_hash as command "begin"
    And the hearth step-measurement sink has a transition to "answering" carrying correlation keys for project root "/tmp/anvil-correlation-project"
    And the hearth step-measurement sink has a transition to "completed" carrying correlation keys for project root "/tmp/anvil-correlation-project"
    And the "activity-log.jsonl" sink does not contain raw text "surface-session-lifecycle-001"
    And the "step-measurement.jsonl" sink does not contain raw text "surface-session-lifecycle-001"
    And the "activity-log.jsonl" sink does not contain raw text "/tmp/anvil-correlation-project"
    And the "step-measurement.jsonl" sink does not contain raw text "/tmp/anvil-correlation-project"

  Scenario: Snapshot activity and lean step records inherit the open begin conversation
    Given a hearth seeded with the lore_query run-backed machine
    And the engine is started with that hearth
    When the begin RPC is called to create a "lore_query" artifact named "ask about plans" with no parent for conversation "surface-session-snapshot-001" and project root "/tmp/anvil-correlation-project"
    And the snapshot RPC is called on the begin RPC response artifact to state "completed" with role "doer" and project root "/tmp/anvil-correlation-project"
    Then the snapshot RPC response success is "true"
    And the activity log command "snapshot" carries correlation keys for project root "/tmp/anvil-correlation-project"
    And the activity log command "snapshot" has the same conversation_hash as command "begin"
    And the hearth step-measurement sink has a transition to "completed" carrying correlation keys for project root "/tmp/anvil-correlation-project"
    And the "activity-log.jsonl" sink does not contain raw text "surface-session-snapshot-001"
    And the "step-measurement.jsonl" sink does not contain raw text "surface-session-snapshot-001"

  # T1 — the delivery row is the one join participant this contract was missing,
  # precisely because a DIFFERENT binary writes it. Adding it here rather than in
  # a file of its own is deliberate: two homes for the same contract can
  # disagree, and a keyspace split between two writers is invisible except as a
  # low coverage number with no visible cause.
  Scenario: The delivery row carries hashed conversation identity and a project label
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the kiln router HTTP stub returns verdict kind "daily_recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with stdin UserPromptSubmit session_id "surface-session-delivery-001" prompt "daily recap" against that engine
    Then the anvil-hooks route-turn command exits 0
    And the delivery log records a hashed conversation identity
    And the delivery log records a non-path project label

  # THE load-bearing scenario of this track. The engine resolves the salt once
  # per process and is the only hasher; the hook persists that answer verbatim.
  # If the hook ever computed its own, these two rows would land in disjoint
  # keyspaces and every join would silently return nothing.
  #
  # SETUP IS LOAD-BEARING TOO. Both durable rows are conditional: the engine is
  # started WITH the hearth so `attribution_target` is `Some`, and the turn
  # resolves to a single kind so the routing-activity row is written as well.
  # Without that seeding this scenario asserts against a row that was never
  # written and passes vacuously.
  Scenario: The delivery row's conversation hash equals the activity-log route row's for the same turn
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the kiln router HTTP stub returns verdict kind "daily_recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with stdin UserPromptSubmit session_id "surface-session-delivery-002" prompt "daily recap" against that engine
    Then the anvil-hooks route-turn command exits 0
    And the delivery log records guidance_kind "daily_recap"
    And the delivery log conversation_hash equals the activity log "route" row conversation_hash

  # The value-level proof. Named to this sink explicitly: `delivery-log.jsonl`
  # is deliberately not in WRITTEN_SINKS, so the all-sinks step would be
  # vacuously green over it. The positive assertion comes first for the same
  # reason — an absent row proves an absence trivially.
  Scenario: The delivery log sink does not contain the raw conversation id
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the kiln router HTTP stub returns verdict kind "daily_recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with stdin UserPromptSubmit session_id "surface-session-delivery-003" prompt "daily recap" against that engine
    Then the anvil-hooks route-turn command exits 0
    And the delivery log records a hashed conversation identity
    And the "delivery-log.jsonl" sink does not contain raw text "surface-session-delivery-003"

  Scenario: Checkin activity record does not carry conversation hash
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
    And the engine is started with that hearth
    When the checkin RPC is called with role "creator" and actor_name "Checkin-Actor-000042"
    Then the checkin RPC response actor_name is "Checkin-Actor-000042"
    And the activity log command "checkin" has no conversation_hash field
