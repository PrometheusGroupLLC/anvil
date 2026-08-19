Feature: Route RPC — no_match-while-open reminder, sticky moved-on suppression, and park hint
  resume-signal context-awareness (P2/P3). A `no_match` turn whose conversation
  has an open non-terminal playbook surfaces a light resume REMINDER for that open
  playbook (tagged `no_match_open_reminder`) — the dangling-instance catch on a
  turn that matched nothing. A `no_match` turn with NO open playbook is unchanged
  (the candidate_playbook_intake handoff).

  The re-nudge dedup and the sticky moved-on suppression are keyed by the open
  ARTIFACT id (hearth, conversation_id, artifact_id): a repeat reminder within the
  dedup window with nothing changed is suppressed; and after N consecutive
  UNRELATED route turns (matched-other or idle-no_match, N =
  UNRELATED_QUIET_THRESHOLD = 2) with no intervening begin/snapshot/complete/
  continuation rearm, the resume for that artifact goes QUIET and a structured
  PARK HINT is surfaced ONCE so the agent can abandon the dangling track instead
  of being nagged. A later RELATED turn or any lifecycle rearm resets the counter.

  # ---- no_match-while-open reminder ----

  Scenario: a no_match turn with an open non-terminal playbook surfaces a reminder
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth has an open "track" artifact "20260601T0001_alpha" in state "spec" begun for conversation "Conv-1" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    When the route RPC is called with message "zzz nothing matches here", signal "", and conversation_id "Conv-1"
    Then the route resolution outcome is "resume"
    And the route resume artifact id is "20260601T0001_alpha"
    And the route resume kind is "track"
    And the route resume state is "spec"
    And the route resume advance action is "complete → spec_review (role: spec)"
    And the engine stderr contains a JSON log record with fields:
      | event_kind         | routing_decision        |
      | phase              | route                   |
      | turn_id            | Conv-1                  |
      | resolution_outcome | resume                  |
      | confidence         | no_match_open_reminder  |

  Scenario: a no_match turn with NO open playbook is unchanged (handoff, no reminder)
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth has an open "track" artifact "20260601T0001_alpha" in state "spec" begun for conversation "Conv-1" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    When the route RPC is called with message "zzz nothing matches here", signal "", and conversation_id "Conv-OTHER"
    Then the route resolution outcome is "no_match"
    And the route resume artifact id is ""

  # ---- time-window re-nudge dedup (artifact-keyed) ----

  Scenario: a second no_match reminder with no intervening change is suppressed
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth has an open "track" artifact "20260601T0001_alpha" in state "spec" begun for conversation "Conv-1" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    When the route RPC is called with message "zzz nothing matches here", signal "", and conversation_id "Conv-1"
    Then the route resolution outcome is "resume"
    And the route resume artifact id is "20260601T0001_alpha"
    When the route RPC is called with message "zzz nothing matches here", signal "", and conversation_id "Conv-1"
    Then the route resolution outcome is "no_match"
    And the route resume artifact id is ""
    And the engine stderr contains a JSON log record with fields:
      | event_kind  | routing_decision                  |
      | phase       | route                             |
      | turn_id     | Conv-1                            |
      | confidence  | no_match_open_reminder_suppressed |

  Scenario: an intervening snapshot re-arms the reminder
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth has an open "track" artifact "20260601T0001_alpha" in state "spec" begun for conversation "Conv-1" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    When the route RPC is called with message "zzz nothing matches here", signal "", and conversation_id "Conv-1"
    Then the route resolution outcome is "resume"
    And the route resume artifact id is "20260601T0001_alpha"
    When the snapshot RPC is called with:
      | artifact_path        | tracks/20260601T0001_alpha |
      | to_state             | spec_review                |
      | actor_name           | Reviewer-222222            |
      | actor_role           | review                     |
      | actor_type           | agent                      |
      | actor_model          | claude-opus-4-7            |
      | actor_provider       | anthropic                  |
      | actor_context_window | 1000000                    |
      | actor_sdk_version    | 0.2.111                    |
      | actor_entrypoint     | claude-desktop             |
      | conversation_id      | Conv-1                     |
    Then the snapshot RPC response success is "true"
    When the route RPC is called with message "zzz nothing matches here", signal "", and conversation_id "Conv-1"
    Then the route resolution outcome is "resume"
    And the route resume artifact id is "20260601T0001_alpha"
    And the route resume state is "spec_review"

  Scenario: a continuation token between reminders re-arms the reminder
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth has an open "track" artifact "20260601T0001_alpha" in state "spec" begun for conversation "Conv-1" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    When the route RPC is called with message "zzz nothing matches here", signal "", and conversation_id "Conv-1"
    Then the route resolution outcome is "resume"
    And the route resume artifact id is "20260601T0001_alpha"
    When the route RPC is called with message "go", signal "", and conversation_id "Conv-1"
    Then the route resolution outcome is "resume"
    And the route resume artifact id is "20260601T0001_alpha"
    When the route RPC is called with message "zzz nothing matches here", signal "", and conversation_id "Conv-1"
    Then the route resolution outcome is "resume"
    And the route resume artifact id is "20260601T0001_alpha"

  Scenario: the first reminder always fires (dedup never suppresses the first)
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth has an open "track" artifact "20260601T0001_alpha" in state "spec" begun for conversation "Conv-FIRST" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    When the route RPC is called with message "zzz nothing matches here", signal "", and conversation_id "Conv-FIRST"
    Then the route resolution outcome is "resume"
    And the route resume artifact id is "20260601T0001_alpha"
    And the engine stderr contains a JSON log record with fields:
      | event_kind         | routing_decision       |
      | phase              | route                  |
      | turn_id            | Conv-FIRST             |
      | resolution_outcome | resume                 |
      | confidence         | no_match_open_reminder |

  # ---- sticky moved-on suppression + park hint (N = 2) ----

  Scenario: N consecutive unrelated turns quiet the resume and surface the park hint once
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth also has a driven "estimator" machine with trigger "estimate the project"
    And the route hearth has an open "track" artifact "20260601T0001_alpha" in state "spec" begun for conversation "Conv-P" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    # turn 1 (unrelated: matches estimator, not the open track) — suppressed resume, no park hint yet
    When the route RPC is called with message "estimate the project", signal "", and conversation_id "Conv-P"
    Then the route resolution outcome is "single"
    And the route selected kind is "estimator"
    And the route resume artifact id is ""
    And no route park hint is set
    # turn 2 (unrelated) — still no park hint (counter = 2, not yet over threshold)
    When the route RPC is called with message "estimate the project", signal "", and conversation_id "Conv-P"
    Then the route resolution outcome is "single"
    And no route park hint is set
    # turn 3 (unrelated, N+1) — quiet: park hint surfaces ONCE for the open track
    When the route RPC is called with message "estimate the project", signal "", and conversation_id "Conv-P"
    Then the route resolution outcome is "single"
    And the route selected kind is "estimator"
    And the route resume artifact id is ""
    And the route park hint artifact id is "20260601T0001_alpha"
    And the route park hint kind is "track"
    And the route park hint state is "spec"
    And the route park hint park action is "snapshot → abandoned (role: doer)"
    # turn 4 (unrelated) — surface-once: the park hint is now absent
    When the route RPC is called with message "estimate the project", signal "", and conversation_id "Conv-P"
    Then the route resolution outcome is "single"
    And no route park hint is set

  Scenario: a RELATED turn after suppression still resumes and resets the counter
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth also has a driven "estimator" machine with trigger "estimate the project"
    And the route hearth also has a driven "planner" machine with trigger "plan the sprint"
    And the route hearth has an open "estimator" artifact "20260601T0002_est" in state "active" begun for conversation "Conv-R" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    # three unrelated (planner) turns push the open estimator past the quiet threshold
    When the route RPC is called with message "plan the sprint", signal "", and conversation_id "Conv-R"
    Then the route selected kind is "planner"
    When the route RPC is called with message "plan the sprint", signal "", and conversation_id "Conv-R"
    Then the route selected kind is "planner"
    When the route RPC is called with message "plan the sprint", signal "", and conversation_id "Conv-R"
    Then the route selected kind is "planner"
    And the route park hint artifact id is "20260601T0002_est"
    # a RELATED turn still resumes the open estimator (never quieted) and resets the counter
    When the route RPC is called with message "estimate the project", signal "", and conversation_id "Conv-R"
    Then the route resolution outcome is "resume"
    And the route resume artifact id is "20260601T0002_est"
    # after the reset, the next unrelated turn is back below the threshold — no park hint
    When the route RPC is called with message "plan the sprint", signal "", and conversation_id "Conv-R"
    Then the route selected kind is "planner"
    And no route park hint is set

  Scenario: two open tracks of the same kind keep independent moved-on state (artifact-keyed)
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth also has a driven "estimator" machine with trigger "estimate the project"
    And the route hearth has an open "track" artifact "20260601T0010_early" in state "spec" begun for conversation "Conv-2A" at "2026-06-01T10:00:00Z"
    And the route hearth has an open "track" artifact "20260601T0011_late" in state "spec" begun for conversation "Conv-2A" at "2026-06-01T12:00:00Z"
    And the engine is started with that hearth
    # the most-recently-begun track (late) is the open one; unrelated turns quiet IT
    When the route RPC is called with message "estimate the project", signal "", and conversation_id "Conv-2A"
    Then the route selected kind is "estimator"
    When the route RPC is called with message "estimate the project", signal "", and conversation_id "Conv-2A"
    Then the route selected kind is "estimator"
    When the route RPC is called with message "estimate the project", signal "", and conversation_id "Conv-2A"
    Then the route park hint artifact id is "20260601T0011_late"
    # park the late track → the early track becomes the open one, with FRESH state
    When the snapshot RPC is called with:
      | artifact_path        | tracks/20260601T0011_late |
      | to_state             | abandoned                 |
      | actor_name           | Doer-333333               |
      | actor_role           | doer                      |
      | actor_type           | agent                     |
      | actor_model          | claude-opus-4-7           |
      | actor_provider       | anthropic                 |
      | actor_context_window | 1000000                   |
      | actor_sdk_version    | 0.2.111                   |
      | actor_entrypoint     | claude-desktop            |
      | conversation_id      | Conv-2A                   |
    Then the snapshot RPC response success is "true"
    # a no_match turn now surfaces the EARLY track's reminder fresh (not inheriting late's quiet)
    When the route RPC is called with message "zzz nothing matches here", signal "", and conversation_id "Conv-2A"
    Then the route resolution outcome is "resume"
    And the route resume artifact id is "20260601T0010_early"

  # ---- park transition parks the track (AC6) ----

  Scenario: snapshotting a track along the park edge to abandoned stops it surfacing
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth has an open "track" artifact "20260601T0001_alpha" in state "spec" begun for conversation "Conv-K" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    When the route RPC is called with message "zzz nothing matches here", signal "", and conversation_id "Conv-K"
    Then the route resolution outcome is "resume"
    And the route resume artifact id is "20260601T0001_alpha"
    When the snapshot RPC is called with:
      | artifact_path        | tracks/20260601T0001_alpha |
      | to_state             | abandoned                  |
      | actor_name           | Doer-444444                |
      | actor_role           | doer                       |
      | actor_type           | agent                      |
      | actor_model          | claude-opus-4-7            |
      | actor_provider       | anthropic                  |
      | actor_context_window | 1000000                    |
      | actor_sdk_version    | 0.2.111                    |
      | actor_entrypoint     | claude-desktop             |
      | conversation_id      | Conv-K                     |
    Then the snapshot RPC response success is "true"
    When the route RPC is called with message "zzz nothing matches here", signal "", and conversation_id "Conv-K"
    Then the route resolution outcome is "no_match"
    And the route resume artifact id is ""
