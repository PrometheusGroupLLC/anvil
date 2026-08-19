Feature: Outcome-predicate fold evaluates each machine's DECLARED terminal_state
  A machine's `outcome_predicate.terminal_state` names the checkable "did the
  world-change happen" fact — distinct from the registry's generic per-state
  `is_terminal` flag, which any of several states may carry. This fold
  resolves each begun instance's kind to its machine and checks reach against
  the DECLARED predicate state specifically, not any terminal state. A kind
  whose machine declares no predicate at all is ungradeable by this fold, not
  a failure — its row is flagged `predicate_declared: false` rather than
  blended into a false "0% satisfied" rate.

  Scenario: A kind whose machine declares no outcome_predicate is ungradeable, not a failure
    # The playbook seed ships outcome_predicate: None (the generator has never
    # authored one). Both instances here reach "resolved" — a generic
    # terminal-shaped state — but with no declared predicate this fold cannot
    # evaluate an outcome fact at all.
    Given an outcome-predicate activity stream:
      | command  | from_state   | to_state     | at                   | artifact_kind | playbook_run_id |
      | begin    |              | draft        | 2026-06-15T09:00:00Z | playbook      | inst_a               |
      | complete | triaged      | resolved     | 2026-06-15T10:00:00Z | playbook      | inst_a               |
      | begin    |              | draft        | 2026-06-15T09:00:00Z | playbook      | inst_b               |
      | complete | draft        | triaged      | 2026-06-15T10:00:00Z | playbook      | inst_b               |
    When the outcome predicate is folded against the seed registry
    Then the outcome predicate outcome for "playbook" has begun 2
    And the outcome predicate outcome for "playbook" has no declared predicate
    And the outcome predicate outcome for "playbook" has predicate_satisfied 0

  Scenario: Each spark capture folds under its own unique identity
    # The spark seed now declares outcome_predicate.terminal_state = "captured".
    # Every capture appends to the shared sparks.md, but the engine emits a
    # UNIQUE playbook_run_id per capture, so three captures fold as three
    # begun instances that each reached "captured" — begun 3, satisfied 3.
    Given an outcome-predicate activity stream:
      | command | from_state | to_state | at                   | artifact_kind | playbook_run_id |
      | begin   |            | captured | 2026-06-15T09:00:00Z | spark         | spark-aaaaaaaaaaaaaaaa |
      | begin   |            | captured | 2026-06-15T09:01:00Z | spark         | spark-bbbbbbbbbbbbbbbb |
      | begin   |            | captured | 2026-06-15T09:02:00Z | spark         | spark-cccccccccccccccc |
    When the outcome predicate is folded against the seed registry
    Then the outcome predicate outcome for "spark" has begun 3
    And the outcome predicate outcome for "spark" has a declared predicate
    And the outcome predicate outcome for "spark" has predicate_satisfied 3

  Scenario: Spark captures sharing one fold identity collapse into a single instance
    # The regression this guards: before the unique-id fix every capture carried
    # the SAME instance id ("sparks.md"), so three captures folded as ONE begun
    # instance and the fold could never count captures per-spark.
    Given an outcome-predicate activity stream:
      | command | from_state | to_state | at                   | artifact_kind | playbook_run_id |
      | begin   |            | captured | 2026-06-15T09:00:00Z | spark         | sparks.md            |
      | begin   |            | captured | 2026-06-15T09:01:00Z | spark         | sparks.md            |
      | begin   |            | captured | 2026-06-15T09:02:00Z | spark         | sparks.md            |
    When the outcome predicate is folded against the seed registry
    Then the outcome predicate outcome for "spark" has begun 1
    And the outcome predicate outcome for "spark" has predicate_satisfied 1

  Scenario: Instances that reach the machine's DECLARED terminal_state are predicate-satisfied
    # inst_a reaches "archived" (the DECLARED terminal_state) -> satisfied.
    # inst_b reaches "completed" -- a different state, even though it would be
    # a plausible generic terminal elsewhere -- so it is NOT satisfied: this
    # fold checks the declared predicate, not any terminal-shaped state.
    # inst_c never advances past "draft" -- not satisfied. Rate = 1/3.
    Given an outcome-predicate activity stream:
      | command  | from_state | to_state  | at                   | artifact_kind | playbook_run_id |
      | begin    |            | draft     | 2026-06-15T09:00:00Z | widget        | inst_a               |
      | complete | draft      | archived  | 2026-06-15T10:00:00Z | widget        | inst_a               |
      | begin    |            | draft     | 2026-06-15T09:00:00Z | widget        | inst_b               |
      | complete | draft      | completed | 2026-06-15T10:00:00Z | widget        | inst_b               |
      | begin    |            | draft     | 2026-06-15T09:00:00Z | widget        | inst_c               |
    And an outcome-predicate registry:
      | kind   | predicate_terminal_state |
      | widget | archived                 |
    When the outcome predicate is folded against the outcome-predicate registry
    Then the outcome predicate outcome for "widget" has begun 3
    And the outcome predicate outcome for "widget" has a declared predicate
    And the outcome predicate outcome for "widget" has predicate_satisfied 1
    And the outcome predicate outcome for "widget" has predicate rate permille 333

  Scenario: an empty activity stream folds to an empty result
    Given an empty outcome-predicate activity stream
    When the outcome predicate is folded against the seed registry
    Then the outcome predicate result has 0 rows
