Feature: Route RPC — relevance-gated MID_PLAYBOOK_RUN resume nudge on matched turns
  resume-signal context-awareness (P1). When normal resolution MATCHES a playbook
  (Single or Candidates) AND the conversation already has an open (begun,
  non-terminal) playbook, the route handler surfaces a resume nudge for the OPEN
  playbook — the same `resume` response shape the continuation-token pre-check
  builds — ONLY when the turn RELATES to that open playbook (its kind is in the
  turn's `matching_candidates`). A matched turn that is UNRELATED to the open
  playbook (the agent moved to different work) SUPPRESSES the resume and returns
  the NORMAL route response for the matched kind, candidates/matching_candidates/
  selected_kind intact — no context-blind nagging. This feature covers the
  matched-turn relevance gate; the no_match-while-open reminder, the sticky
  moved-on suppression, and the park hint live in route_rpc_open_reminder_dedup.feature.

  Scenario: a matched turn whose matching set INCLUDES the open kind resumes it (related)
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth also has a driven "estimator" machine with trigger "estimate the project"
    And the route hearth has an open "estimator" artifact "20260601T0001_est" in state "active" begun for conversation "Conv-1" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    When the route RPC is called with message "estimate the project", signal "", and conversation_id "Conv-1"
    Then the route resolution outcome is "resume"
    And the route resume artifact id is "20260601T0001_est"
    And the route resume kind is "estimator"
    And the route resume state is "active"
    And the route resume advance action is "complete → completed (role: doer)"
    And no route selected kind is set

  Scenario: a matched turn whose matching set EXCLUDES the open kind suppresses the resume and returns the normal response
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth also has a driven "estimator" machine with trigger "estimate the project"
    And the route hearth has an open "track" artifact "20260601T0001_alpha" in state "spec" begun for conversation "Conv-1" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    When the route RPC is called with message "estimate the project", signal "", and conversation_id "Conv-1"
    Then the route resolution outcome is "single"
    And the route selected kind is "estimator"
    And the route matching candidates are exactly "estimator"
    And the route resume artifact id is ""
    And no route park hint is set

  Scenario: a matched turn with NO open playbook for the conversation gets the unchanged begin nudge
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth also has a driven "estimator" machine with trigger "estimate the project"
    And the route hearth has an open "track" artifact "20260601T0001_alpha" in state "spec" begun for conversation "Conv-1" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    When the route RPC is called with message "estimate the project", signal "", and conversation_id "Conv-OTHER"
    Then the route resolution outcome is "single"
    And the route selected kind is "estimator"
    And the route resume artifact id is ""

  Scenario: a matched turn whose only open playbook is terminal gets the begin nudge (no false check-in)
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth also has a driven "estimator" machine with trigger "estimate the project"
    And the route hearth has an open "track" artifact "20260601T0009_done" in state "abandoned" begun for conversation "Conv-2" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    When the route RPC is called with message "estimate the project", signal "", and conversation_id "Conv-2"
    Then the route resolution outcome is "single"
    And the route selected kind is "estimator"
    And the route resume artifact id is ""

  Scenario: a no_match turn with no open playbook for the conversation never nudges
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth also has a driven "estimator" machine with trigger "estimate the project"
    And the route hearth has an open "track" artifact "20260601T0001_alpha" in state "spec" begun for conversation "Conv-1" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    When the route RPC is called with message "zzz nothing here", signal "", and conversation_id "Conv-NONE"
    Then the route resolution outcome is "no_match"
    And the route resume artifact id is ""

  Scenario: a matched turn with an empty conversation id never runs the lookup (begin nudge)
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth also has a driven "estimator" machine with trigger "estimate the project"
    And the route hearth has an open "estimator" artifact "20260601T0001_est" in state "active" begun for conversation "Conv-1" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    When the route RPC is called with message "estimate the project", signal "", and conversation_id ""
    Then the route resolution outcome is "single"
    And the route selected kind is "estimator"
    And the route resume artifact id is ""

  Scenario: the continuation-token resume pre-check still fires for a continuation token (unchanged, not overlap-gated)
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth has an open "track" artifact "20260601T0001_alpha" in state "spec" begun for conversation "Conv-1" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    When the route RPC is called with message "go", signal "", and conversation_id "Conv-1"
    Then the route resolution outcome is "resume"
    And the route resume artifact id is "20260601T0001_alpha"
    And the route resume kind is "track"
    And the route resume state is "spec"
    And the route resume advance action is "complete → spec_review (role: spec)"
