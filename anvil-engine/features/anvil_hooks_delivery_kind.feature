Feature: the delivery log records WHICH kind the hook delivered
  The left-hand leg of the delivered-kind→begin join. Where the activity log
  records what the ENGINE served, `delivery-log.jsonl` records what the HOOK
  wrote to stdout — and until now it could not say for which playbook kind, so
  a suggestion and the begin that followed it could not be paired.

  What is proven here is that the kind and the conversation key survive the
  WHOLE chain: engine decision → RouteResponse → hook → delivery-log.jsonl.
  That is six links, and a field that stops at link four is invisible in exactly
  the way the 1500ms delivery outage was invisible. So every scenario asserts
  the LOG, never the response. Only the end of the chain proves the chain.

  The field is `guidance_kind`, not `delivered_kind`: this row is written BEFORE
  `println!` and long before the harness consumes stdout, so it cannot witness
  delivery. The producer side is all this binary can honestly see.

  Scenario: a single-resolution turn records the resolved kind in the delivery log
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the kiln router HTTP stub returns verdict kind "daily_recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" against that engine
    Then the anvil-hooks route-turn command exits 0
    And the anvil-hooks route-turn output contains "daily_recap"
    And the delivery log records guidance_kind "daily_recap"
    And the delivery log records outcome "guidance_produced"

  # A menu is not a kind. The hook was handed two granted candidates and
  # produced nothing, so the row must say "two candidates, no kind" — never
  # fabricate a single kind out of a set the selector declined to choose from.
  Scenario: a candidates turn records an empty guidance kind and a candidate count
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the route hearth also has a driven "track" machine with trigger "start a track"
    And the kiln router HTTP stub returns verdict abstain
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" against that engine
    Then the anvil-hooks route-turn command exits 0
    And the anvil-hooks route-turn output is empty
    And the delivery log records guidance_kind ""
    And the delivery log records more than 1 engine_candidates

  # A resume delivers the OPEN run's kind, not a fresh selection. `resume_source`
  # rides alongside so the row proves the resume branch was taken and not a
  # coincidental single.
  Scenario: a resume turn records the open playbook's kind and its resume source
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the route hearth has an open "daily_recap" artifact "20260601T0004_recap" in state "active" begun for conversation "sess-delivery-resume-1" at "2026-06-01T10:00:00Z"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with stdin UserPromptSubmit session_id "sess-delivery-resume-1" prompt "go" against that engine
    Then the anvil-hooks route-turn command exits 0
    And the delivery log records guidance_kind "daily_recap"
    And the delivery log records resume_source "continuation_token"

  # Silence is recorded as silence. An abstention that wrote a kind would make
  # the suggestion denominator larger than the set of turns anything was
  # suggested on.
  Scenario: an abstaining turn records an empty guidance kind and its outcome reason
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the kiln router HTTP stub returns verdict kind "daily_recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "<system-reminder>daily recap reminder: settle up</system-reminder>" against that engine
    Then the anvil-hooks route-turn command exits 0
    And the anvil-hooks route-turn output is empty
    And the delivery log records guidance_kind ""
    And the delivery log records outcome "no_candidate"

  # THE OUTAGE GUARD. This is the early-return branch where the 1500ms delivery
  # outage lived: every turn timed out into UNLOGGED silence, three confident
  # wrong diagnoses were made, and an empty log was indistinguishable from "no
  # turns happened". A row the hook does not write is an outage that reads as
  # nothing, so the branch must still produce one — with the SENTINEL, because
  # the hook has no engine answer and must never compute a hash of its own.
  Scenario: a turn against an unreachable engine still writes a row, with the sentinel and no guidance kind
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" against a closed port
    Then the anvil-hooks route-turn command exits 0
    And the delivery log records conversation_hash "unknown_conversation_hash"
    And the delivery log records guidance_kind ""
    And the delivery log records outcome "engine_unreachable"

  # One turn, one line. The row count is also a glued-JSON guard: the assertion
  # parses every line, and the defect this sink already shipped once (live line
  # 110) puts two objects on one of them.
  Scenario: the delivery log holds one parseable row per turn after several turns
    Given a route hearth with one unrestricted driven machine kind "daily_recap" trigger "daily recap"
    And the kiln router HTTP stub returns verdict kind "daily_recap"
    And the engine is started with that hearth
    When anvil-hooks route-turn runs with message "daily recap" against that engine
    And anvil-hooks route-turn runs with message "daily recap" against that engine
    And anvil-hooks route-turn runs with message "daily recap" against that engine
    Then the anvil-hooks route-turn command exits 0
    And the delivery log holds 3 parseable rows
