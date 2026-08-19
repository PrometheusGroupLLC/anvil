Feature: The full §0 step-measurement stream is emitted through the privacy gate
  Anvil writes the temper-consumed full §0 StepMeasurementEvent stream to a NEW
  append-only file per step transition at
  `~/.temper/step-measurements/<artifact_kind>/events.jsonl` — one JSON object per
  line, partitioned by artifact_kind. This is SEPARATE from the lean booleans-only
  hearth audit sink. The rich record ships ONLY when the
  `step-measurement-emit-privacy` decision is DECIDED; until then the emit path
  falls back to the lean/redacted behavior and no rich prose/identities leave the
  process. The redaction policy omits `tokens`. Emit covers begin, snapshot, AND
  complete, for EVERY playbook kind. track_id equals the artifact_kind (not the
  run id). Replayed transitions do not duplicate. A sink error never fails the
  originating transition.

  Scenario: a decided gate ships the full §0 field set with track_id equal to the kind
    Given a hearth seeded with the lore_query run-backed machine
    And the hearth has a decided "step-measurement-emit-privacy" decision
    And the engine is started with that hearth
    When the begin RPC is called to create a "lore_query" artifact named "ask about plans" with no parent
    Then the begin RPC response has non-empty "track_path"
    And the temper step0 stream for kind "lore_query" has 1 events
    And the temper step0 stream for kind "lore_query" has an event with fields:
      | track_id        | lore_query                                          |
      | from_state      |                                                     |
      | to_state        | answering                                           |
      | role            | doer                                                |
      | actor           | Rpc-Test-000000                                      |
      | intent          | <non-empty>                                         |
      | expected_output | <non-empty>                                         |
      | at              | <non-empty>                                         |
      | workflow_id     | <non-empty>                                         |
      | model           | <non-empty>                                         |
    And the temper step0 stream for kind "lore_query" event has no "tokens" field
    And the temper step0 stream for kind "lore_query" event has "workflow_id" not equal to "track_id"

  Scenario: snapshot and complete also emit §0 events in lifecycle order for the kind
    Given a hearth seeded with the lore_query run-backed machine
    And the hearth has a decided "step-measurement-emit-privacy" decision
    And the engine is started with that hearth
    When the begin RPC is called to create a "lore_query" artifact named "ask about plans" with no parent
    And the complete RPC is called on the begin RPC response artifact with satisfaction ""
    Then the complete RPC response new_state is "completed"
    And the temper step0 stream for kind "lore_query" has 2 events
    And the temper step0 stream for kind "lore_query" events are in lifecycle order by to_state "answering,completed"
    And the temper step0 stream for kind "lore_query" has an event with fields:
      | track_id   | lore_query |
      | from_state | answering  |
      | to_state   | completed  |
      | role       | doer       |

  Scenario: each logical transition emits exactly one §0 event (no double-count)
    Given a hearth seeded with the lore_query run-backed machine
    And the hearth has a decided "step-measurement-emit-privacy" decision
    And the engine is started with that hearth
    When the begin RPC is called to create a "lore_query" artifact named "ask about plans" with no parent
    And the snapshot RPC is called on the begin RPC response artifact to state "completed" with role "doer"
    Then the temper step0 stream for kind "lore_query" has exactly 1 events with to_state "completed"
    And the temper step0 stream for kind "lore_query" has 2 events

  # M5 (idempotency model): the engine emits EXACTLY ONE §0 event per LOGICAL
  # transition across a full begin -> complete lifecycle — there is no engine
  # path that re-emits the same persisted transition. begin is guarded by
  # `origin_turn_hit` (re-begin short-circuits before the emit), complete is
  # guarded by `select_edge` (a re-complete from a terminal state errors before
  # the emit), and each snapshot appends a NEW distinct transition the fold
  # preserves. So no logical transition is ever double-counted. (True at-least-
  # once / crash redelivery idempotency is temper's per-file read cursor, not a
  # write-side concern; the adapter `event_id` dedupe stays as defense-in-depth.)
  Scenario: no logical transition is double-counted across a begin -> complete lifecycle
    Given a hearth seeded with the lore_query run-backed machine
    And the hearth has a decided "step-measurement-emit-privacy" decision
    And the engine is started with that hearth
    When the begin RPC is called to create a "lore_query" artifact named "ask about plans" with no parent
    And the complete RPC is called on the begin RPC response artifact with satisfaction ""
    Then the complete RPC response new_state is "completed"
    And the temper step0 stream for kind "lore_query" has exactly 1 events with to_state "answering"
    And the temper step0 stream for kind "lore_query" has exactly 1 events with to_state "completed"
    And the temper step0 stream for kind "lore_query" has 2 events

  Scenario: an undecided gate withholds the rich §0 temper stream and keeps the lean sink
    Given a hearth seeded with the lore_query run-backed machine
    And the engine is started with that hearth
    When the begin RPC is called to create a "lore_query" artifact named "ask about plans" with no parent
    And the complete RPC is called on the begin RPC response artifact with satisfaction ""
    Then the complete RPC response new_state is "completed"
    And the temper step0 stream for kind "lore_query" has 0 events
    And the lean step-measurement sink has at least 1 records

  # H2: the engine's OWN stderr step_measurement record is a local diagnostic
  # log, explicitly EXEMPT from the temper-egress gate (documented in the
  # decision's data-flow inventory). It therefore still carries actor/intent
  # prose even while the gate is undecided — that is intended local behavior,
  # NOT a leak across the temper privacy boundary (the temper stream is empty,
  # asserted above and re-asserted here).
  Scenario: while the gate is undecided the temper stream stays empty but local stderr still records
    Given a hearth seeded with the lore_query run-backed machine
    And the engine is started with that hearth
    When the begin RPC is called to create a "lore_query" artifact named "ask about plans" with no parent
    Then the begin RPC response has non-empty "track_path"
    And the temper step0 stream for kind "lore_query" has 0 events
    And the engine stderr contains a JSON log record with fields:
      | event_kind      | step_measurement |
      | track_id        | lore_query       |
      | to_state        | answering        |
      | role            | doer             |
      | actor           | Rpc-Test-000000  |
      | intent          | <non-empty>      |
      | expected_output | <non-empty>      |

  Scenario: a §0 stream write error does not fail the originating begin transition
    Given a hearth seeded with the lore_query run-backed machine
    And the hearth has a decided "step-measurement-emit-privacy" decision
    And the temper step0 stream path for kind "lore_query" is blocked by a file
    And the engine is started with that hearth
    When the begin RPC is called to create a "lore_query" artifact named "ask about plans" with no parent
    Then the begin RPC response has non-empty "track_path"

  # MUTATION: make `emit_step0_stream` propagate/panic on its append error. This
  # scenario must then fail at the snapshot RPC instead of returning success.
  Scenario: a §0 stream write error does not fail the originating snapshot transition
    Given a hearth seeded with the lore_query run-backed machine
    And the hearth has a decided "step-measurement-emit-privacy" decision
    And the engine is started with that hearth
    When the begin RPC is called to create a "lore_query" artifact named "ask about plans" with no parent
    And the temper step0 stream path for kind "lore_query" becomes blocked
    And the snapshot RPC is called on the begin RPC response artifact to state "completed" with role "doer"
    Then the snapshot RPC response success is "true"

  # MUTATION: make `emit_step0_stream` propagate/panic on its append error. This
  # scenario must then fail at the complete RPC instead of reaching completed.
  Scenario: a §0 stream write error does not fail the originating complete transition
    Given a hearth seeded with the lore_query run-backed machine
    And the hearth has a decided "step-measurement-emit-privacy" decision
    And the engine is started with that hearth
    When the begin RPC is called to create a "lore_query" artifact named "ask about plans" with no parent
    And the temper step0 stream path for kind "lore_query" becomes blocked
    And the complete RPC is called on the begin RPC response artifact with satisfaction ""
    Then the complete RPC response new_state is "completed"
