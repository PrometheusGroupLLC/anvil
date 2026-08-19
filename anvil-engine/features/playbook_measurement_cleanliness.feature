Feature: A finished run is graded, not just counted as finished

  The playbook-measurement record has always carried `success` — the completion
  FLOOR, "it reached the end" — and beside it an EMPTY quality vector with no
  grader. Everything that wanted to know whether a run finished WELL had only the
  floor to read, so a run a reviewer sent back three times presented exactly like
  one nobody ever touched. That is the fact the autonomy ladder's first tile
  asks for and it had no recorded meaning.

  The grader is `run_cleanliness/v1`: reached a terminal state AND was never sent
  back. It invents nothing — it is the predicate the fidelity fold already
  computes and the two-by-two's one-shot axis already uses, over the one shared
  revision-state rule.

  A SCORE AND NOTHING ELSE REACHES THIS SINK. The attribution — which step, which
  moment, who caught it — belongs to the read fold. This sink states that it
  never carries raw actor identity, and the last scenario holds it to that with
  the catcher's own name.

  THE GRADER'S NAME IS THE DISCRIMINATOR, NOT THE SCORE. An unclean run scores a
  real zero, so a reader keying on "non-zero" cannot tell it from a record
  nothing ever graded. Both cases appear below, and they are asserted differently.

  Scenario: A run that reached the end without being sent back is graded clean
    Given a hearth seeded with the revision_probe playbook
    And the engine is started with that hearth
    When the begin RPC is called to create a "revision_probe" artifact named "clean run probe" with no parent for conversation "surface-session-clean-001" and project root "/tmp/anvil-clean-project"
    Then the begin RPC response state is "active"
    When the complete RPC is called on the begin RPC response artifact with satisfaction "" and project root "/tmp/anvil-clean-project"
    Then the complete RPC response new_state is "active_review"
    When the complete RPC is called on the begin RPC response artifact with satisfaction "satisfied" and project root "/tmp/anvil-clean-project"
    Then the complete RPC response new_state is "completed"
    And the hearth playbook-measurement sink has exactly 1 records
    And the hearth playbook-measurement sink record for terminal_state "completed" is graded by "run_cleanliness/v1" scoring "1"

  # THE TAUTOLOGY, BROKEN WHERE IT CAN BE SEEN. This run reached the end, so the
  # floor says `success: true` — and it was sent back once, so the grade says
  # zero. The two fields disagree about the same run, which is the whole point:
  # before this they could not, because one of them was the other's name.
  Scenario: A run that was sent back is graded zero while the completion floor still says it finished
    Given a hearth seeded with the revision_probe playbook
    And the engine is started with that hearth
    When the begin RPC is called to create a "revision_probe" artifact named "sent back probe" with no parent for conversation "surface-session-unclean-001" and project root "/tmp/anvil-unclean-project"
    And the complete RPC is called on the begin RPC response artifact with satisfaction "" and project root "/tmp/anvil-unclean-project"
    Then the complete RPC response new_state is "active_review"
    When the complete RPC is called on the begin RPC response artifact with satisfaction "full_revision" and project root "/tmp/anvil-unclean-project"
    Then the complete RPC response new_state is "active_revision"
    When the complete RPC is called on the begin RPC response artifact with satisfaction "" and project root "/tmp/anvil-unclean-project"
    Then the complete RPC response new_state is "active_review"
    When the complete RPC is called on the begin RPC response artifact with satisfaction "satisfied" and project root "/tmp/anvil-unclean-project"
    Then the complete RPC response new_state is "completed"
    And the hearth playbook-measurement sink has exactly 1 records
    And the hearth playbook-measurement sink has exactly 1 record for artifact_kind "revision_probe" terminal_state "completed" outcome "terminal_reached" success "true"
    And the hearth playbook-measurement sink record for terminal_state "completed" is graded by "run_cleanliness/v1" scoring "0"

  # A run whose machine could not be resolved still writes a durable coverage-gap
  # record, and that record is UNGRADED. It is not scored zero: "I could not
  # look" and "this run was not clean" are different facts, and a zero here would
  # be a grade nobody produced. The two are told apart by the grader's presence,
  # which is why this scenario and the one above it read differently.
  #
  # THE SEEDED RUN'S HISTORY READS CLEANLY AND CONTAINS A CORRECTION. Only its
  # MACHINE is missing. An earlier seed left the history unreadable too, so the
  # grade came back absent for the wrong reason and removing the resolution-gap
  # gate entirely left this scenario green — measured, by the mutant that was
  # supposed to kill it. A grader that ignored the gap now scores this run a
  # definite zero and goes red.
  Scenario: A record nothing graded carries no grade, rather than a zero
    Given a terminal playbook event whose machine cannot be resolved
    When playbook measurement is emitted for the unresolved event
    Then the hearth playbook-measurement sink has exactly 1 records
    And the playbook-measurement record reports terminal reached "false" outcome "measurement_unknown" success "false"
    And the hearth playbook-measurement sink record for terminal_state "unknown_terminal" carries no grade at all

  # THE CATCHER'S NAME STOPS AT THE READ FOLD. The sink's own contract is that it
  # carries public labels and hashed join keys and never raw actor identity, and
  # the evidence record this grade is derived from names the person who caught
  # the mistake. The actor who records the `full_revision` here IS that catcher,
  # so its absence from the sink is checked by name rather than assumed.
  Scenario: The grade reaches the sink and the catcher's name does not
    Given a hearth seeded with the revision_probe playbook
    And the engine is started with that hearth
    When the begin RPC is called to create a "revision_probe" artifact named "redaction probe" with no parent for conversation "surface-session-redaction-001" and project root "/tmp/anvil-redaction-project"
    And the complete RPC is called on the begin RPC response artifact with satisfaction "" and project root "/tmp/anvil-redaction-project"
    And the complete RPC is called on the begin RPC response artifact with satisfaction "full_revision" and project root "/tmp/anvil-redaction-project"
    And the complete RPC is called on the begin RPC response artifact with satisfaction "" and project root "/tmp/anvil-redaction-project"
    And the complete RPC is called on the begin RPC response artifact with satisfaction "satisfied" and project root "/tmp/anvil-redaction-project"
    Then the complete RPC response new_state is "completed"
    And the hearth playbook-measurement sink record for terminal_state "completed" is graded by "run_cleanliness/v1" scoring "0"
    And the "playbook-measurement.jsonl" sink does not contain raw text "Rpc-Test-000000"
    And the "playbook-measurement.jsonl" sink does not contain raw text "surface-session-redaction-001"
    And the "playbook-measurement.jsonl" sink does not contain raw text "/tmp/anvil-redaction-project"

  # TWO-SIDED ON PURPOSE. "The sink gained no line" is green on an engine that
  # deleted the append from every path, so the same scenario goes on to make a
  # surfaced call and require the line to appear. The refusal must come BEFORE
  # the write, not instead of it.
  Scenario: A completion naming no surface is refused before the measurement is written, and a surfaced one writes it
    Given a hearth seeded with the transition_probe playbook with empty measurements
    And the engine is started with that hearth
    When the begin RPC is called to create a "transition_probe" artifact named "seam probe" with no parent for conversation "surface-session-seam-001" and project root "/tmp/anvil-seam-project"
    Then the hearth playbook-measurement sink has exactly 0 records
    When a complete that names no surface is sent for the open run
    Then the gRPC call is refused with "surface_required"
    And the hearth playbook-measurement sink has exactly 0 records
    When the complete RPC is called on the begin RPC response artifact with satisfaction "" and project root "/tmp/anvil-seam-project"
    Then the complete RPC response new_state is "completed"
    And the hearth playbook-measurement sink has exactly 1 records

  # ── the case, asked for over the bridge ───────────────────────────────────
  #
  # A fold with no caller answers nothing. These scenarios are what make the
  # substrate ASKABLE: a screen names itself, names a playbook, and gets the case
  # — how many of its runs were clean out of how many were attempted, and what it
  # got wrong with the run, the moment and the catcher on the same row.

  Scenario: A screen can ask a playbook for its case
    Given a hearth seeded with the revision_probe playbook
    And the engine is started with that hearth
    When the begin RPC is called to create a "revision_probe" artifact named "case run one" with no parent for conversation "surface-session-case-001" and project root "/tmp/anvil-case-project"
    And the complete RPC is called on the begin RPC response artifact with satisfaction "" and project root "/tmp/anvil-case-project"
    And the complete RPC is called on the begin RPC response artifact with satisfaction "satisfied" and project root "/tmp/anvil-case-project"
    Then the complete RPC response new_state is "completed"
    When the begin RPC is called to create a "revision_probe" artifact named "case run two" with no parent for conversation "surface-session-case-002" and project root "/tmp/anvil-case-project"
    And the complete RPC is called on the begin RPC response artifact with satisfaction "" and project root "/tmp/anvil-case-project"
    And the complete RPC is called on the begin RPC response artifact with satisfaction "full_revision" and project root "/tmp/anvil-case-project"
    And the complete RPC is called on the begin RPC response artifact with satisfaction "" and project root "/tmp/anvil-case-project"
    And the complete RPC is called on the begin RPC response artifact with satisfaction "satisfied" and project root "/tmp/anvil-case-project"
    Then the complete RPC response new_state is "completed"
    When the case for playbook "revision_probe" is asked for over /ws naming the surface "playbooks-screen"
    Then the /ws JSON-RPC response carries a result
    And the /ws case reports 1 clean of 2 attempted
    And no part of the /ws case carries a cost figure, and the case is not empty

  # A PLAYBOOK NOBODY HAS RUN HAS NO CASE. Not an empty one: "0 clean of 0
  # attempted" reads as a measured perfect failure, and the ladder's first beat is
  # that the evidence exists BEFORE the act.
  Scenario: A playbook with no runs is told it has no case, not an empty one
    Given a hearth seeded with the revision_probe playbook
    And the engine is started with that hearth
    When the case for playbook "revision_probe" is asked for over /ws naming the surface "playbooks-screen"
    Then the /ws JSON-RPC response carries a result
    And the /ws response says the playbook has no case

  # THE NEW METHOD IS NOT EXEMPT FROM THE SEAM. Every read this engine serves a
  # screen names the screen that asked; a default surface is how an unattributable
  # read comes to look attributed.
  Scenario: Asking for a case without naming the screen is refused
    Given a hearth seeded with the revision_probe playbook
    And the engine is started with that hearth
    When a "autonomy_evidence" request is sent over /ws naming no surface
    Then the /ws JSON-RPC response carries no result
    And the /ws JSON-RPC error.data.code is "surface_required"
