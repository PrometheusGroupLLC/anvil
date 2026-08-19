Feature: Route RPC — resume-aware routing (Phase 3)
  resume_aware_routing: the route handler runs a resume PRE-CHECK before normal
  resolution. When the message is a continuation token AND the conversation has
  an open (begun, non-terminal) playbook, route returns a `resume` outcome
  naming the open artifact (id + kind + current state) plus the supported
  advance action for that state — NOT a fresh single/candidates set. Any other
  message routes normally (selection scoring untouched); a continuation token
  with no open playbook falls through to normal routing.

  Scenario: a continuation token with an open playbook resumes it (id + kind + state + advance action)
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth has an open "track" artifact "20260601T0001_alpha" in state "spec" begun for conversation "Conv-1" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    When the route RPC is called with message "go", signal "", and conversation_id "Conv-1"
    Then the route resolution outcome is "resume"
    And the route resume artifact id is "20260601T0001_alpha"
    And the route resume source is "continuation_token"
    And the route resume kind is "track"
    And the route resume state is "spec"
    And the route resume advance action is "complete → spec_review (role: spec)"

  Scenario: a continuation token with NO open playbook for the conversation routes normally (no_match)
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth has an open "track" artifact "20260601T0001_alpha" in state "spec" begun for conversation "Conv-1" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    When the route RPC is called with message "go", signal "", and conversation_id "Conv-OTHER"
    Then the route resolution outcome is "no_match"
    And the route resume artifact id is ""

  Scenario: a completed (terminal) playbook is not resumed by a continuation token
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth has an open "track" artifact "20260601T0009_done" in state "abandoned" begun for conversation "Conv-2" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    When the route RPC is called with message "go", signal "", and conversation_id "Conv-2"
    Then the route resolution outcome is "no_match"
    And the route resume artifact id is ""

  # A non-token new-intent message does NOT trigger the continuation-token
  # pre-check (selection scoring untouched, req #7). With NO open playbook for the
  # conversation it resolves straight to the matched kind. (When the conversation
  # DOES have an open playbook, the matched turn instead surfaces the MID_PLAYBOOK_RUN
  # check-in nudge — covered in route_rpc_mid_run_nudge.feature.)
  Scenario: a new-intent message matching a different trigger routes to that kind (pre-check does not fire)
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth also has a driven "estimator" machine with trigger "estimate the project"
    And the route hearth has an open "track" artifact "20260601T0001_alpha" in state "spec" begun for conversation "Conv-1" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    When the route RPC is called with message "estimate the project", signal "", and conversation_id "Conv-OTHER"
    Then the route resolution outcome is "single"
    And the route selected kind is "estimator"
    And the route resume artifact id is ""

  Scenario: two open playbooks for one conversation resume the most-recently-begun
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth has an open "track" artifact "20260601T0010_early" in state "spec" begun for conversation "Conv-3" at "2026-06-01T10:00:00Z"
    And the route hearth has an open "track" artifact "20260601T0011_late" in state "plan" begun for conversation "Conv-3" at "2026-06-01T12:00:00Z"
    And the engine is started with that hearth
    When the route RPC is called with message "go", signal "", and conversation_id "Conv-3"
    Then the route resolution outcome is "resume"
    And the route resume artifact id is "20260601T0011_late"
    And the route resume state is "plan"

  # Selection unchanged (spec req #7) — a NAMED fixed set of three messages, each
  # asserted individually, with the resume pre-check active (a conversation_id is
  # supplied) but NO open playbook for that conversation. Each resolves to the
  # SAME (outcome + kind) it would without the pre-check: one no_match, one
  # single, one candidates. The triggers: "do alpha" → single (alpha only);
  # "shared task" → candidates (beta + gamma both match); "zzz nothing" → no_match.
  Scenario: selection unchanged — a no_match message routes normally with the pre-check active
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth also has a driven "alpha" machine with trigger "do alpha"
    And the route hearth also has a driven "beta" machine with trigger "shared task"
    And the route hearth also has a driven "gamma" machine with trigger "shared task"
    And the engine is started with that hearth
    When the route RPC is called with message "zzz nothing here", signal "", and conversation_id "Conv-NONE"
    Then the route resolution outcome is "no_match"
    And the route resume artifact id is ""

  Scenario: selection unchanged — a single-match message routes normally with the pre-check active
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth also has a driven "alpha" machine with trigger "do alpha"
    And the route hearth also has a driven "beta" machine with trigger "shared task"
    And the route hearth also has a driven "gamma" machine with trigger "shared task"
    And the engine is started with that hearth
    When the route RPC is called with message "do alpha", signal "", and conversation_id "Conv-NONE"
    Then the route resolution outcome is "single"
    And the route selected kind is "alpha"
    And the route resume artifact id is ""

  Scenario: selection unchanged — a candidates message routes normally with the pre-check active
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth also has a driven "alpha" machine with trigger "do alpha"
    And the route hearth also has a driven "beta" machine with trigger "shared task"
    And the route hearth also has a driven "gamma" machine with trigger "shared task"
    And the engine is started with that hearth
    When the route RPC is called with message "shared task", signal "", and conversation_id "Conv-NONE"
    Then the route resolution outcome is "candidates"
    And the route resume artifact id is ""

  # continuation_recognition (plan phase 4, engine-side). The procedure is proven
  # HERE, at the RPC, so the engine's own decision is under test without a
  # transcript fixture in the loop. Each scenario names the step it exercises.
  Scenario: an affirmative non-token message with context resumes the open playbook (step 7)
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth has an open "track" artifact "20260601T0001_alpha" in state "spec" begun for conversation "Conv-W1" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    When the route RPC is called with message "Great do it", conversation_id "Conv-W1", context "assistant: working on it", and proposal ""
    Then the route resolution outcome is "resume"
    And the route resume artifact id is "20260601T0001_alpha"
    And the route resume source is "widened_no_proposal"

  # Asserted on resume_source, because with a run open several paths can
  # legitimately return a resume: the continuation procedure and two mid-run
  # check-in nudges. Neither the artifact id nor the outcome label separates
  # them — which is exactly why phase 1's hard-coded "continuation_token" in the
  # shared builder was mislabelling both nudge paths as token resumes.
  #
  # The SAME message with no transcript must behave exactly as it does today.
  # `recent_context` being present is a different fact from there being no tail
  # at all, and conflating them made transcript-less surfaces resume where they
  # do not today.
  Scenario: the same message without context does NOT resume (step 8)
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth has an open "track" artifact "20260601T0001_alpha" in state "spec" begun for conversation "Conv-W2" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    When the route RPC is called with message "Great do it", conversation_id "Conv-W2", context "", and proposal ""
    Then the route resume source is "mid_run_nudge"

  # A rejection never substitutes prior context, however affirmative it looks.
  # It reports `rejected` rather than `mid_run_nudge`: the nudge is an affordance
  # about an open run, while `rejected` is the fact about THIS turn's
  # continuation decision — and it is the one the accepted-loss count depends on.
  Scenario: a rejection with an open playbook does not resume (step 4)
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth has an open "track" artifact "20260601T0001_alpha" in state "spec" begun for conversation "Conv-W3" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    When the route RPC is called with message "no, stop", conversation_id "Conv-W3", context "assistant: working on it", and proposal ""
    Then the route resume source is "rejected"
