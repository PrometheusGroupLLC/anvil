Feature: Route call-state classification labels every route turn
  Every route turn is classified into exactly one call-state so the adoption
  denominator is honest: NO_PLAYBOOK_RUN (the router found nothing relevant),
  START_OPPORTUNITY (a playbook is relevant and the conversation has no open,
  non-terminal playbook — the agent should begin), or MID_PLAYBOOK_RUN (the
  conversation already has an open, non-terminal playbook — the call should
  continue it, not begin a new one). The classification is a pure function of the
  route outcome and the conversation's open-playbook lookup: a terminal-only
  conversation is NOT mid-playbook (terminal ≠ open), and an empty conversation
  id can never be mid-playbook (it falls back to outcome-only). The track seed
  treats `completed` as NON-terminal and `abandoned` as terminal.

  Scenario: a no-match outcome classifies as no_playbook_run
    Given a call-state route outcome "no_match"
    When the call-state is classified for conversation "C1"
    Then the call-state is "no_playbook_run"

  Scenario: a match with no open playbook is a start opportunity
    Given a call-state route outcome "matched"
    When the call-state is classified for conversation "C1"
    Then the call-state is "start_opportunity"

  Scenario: a match with an open non-terminal RELEVANT playbook is mid playbook
    Given a call-state route outcome "matched"
    And a call-state open playbook "20260601T0001_alpha" kind "track" state "spec" for conversation "C1" begun at "2026-06-01T10:00:00Z"
    When the call-state is classified for conversation "C1" matching "track"
    Then the call-state is "mid_playbook_run"

  # resume-signal context-awareness: a matched turn whose open playbook is NOT in
  # the turn's matching set (the agent moved to different work) is NORMAL routing
  # — a start opportunity, never mid_playbook_run. This is the telemetry half of the
  # P1 relevance gate: an unrelated matched turn is not counted as a mid-playbook
  # continuation.
  Scenario: a match with an open non-terminal UNRELATED playbook is a start opportunity
    Given a call-state route outcome "matched"
    And a call-state open playbook "20260601T0001_alpha" kind "track" state "spec" for conversation "C1" begun at "2026-06-01T10:00:00Z"
    When the call-state is classified for conversation "C1" matching "estimator"
    Then the call-state is "start_opportunity"

  Scenario: a match whose only playbook is terminal is a start opportunity
    Given a call-state route outcome "matched"
    And a call-state open playbook "20260601T0002_done" kind "track" state "abandoned" for conversation "C1" begun at "2026-06-01T10:00:00Z"
    When the call-state is classified for conversation "C1"
    Then the call-state is "start_opportunity"

  Scenario: a match with an empty conversation id is a start opportunity, never mid
    Given a call-state route outcome "matched"
    And a call-state open playbook "20260601T0001_alpha" kind "track" state "spec" for conversation "C1" begun at "2026-06-01T10:00:00Z"
    When the call-state is classified for conversation ""
    Then the call-state is "start_opportunity"
