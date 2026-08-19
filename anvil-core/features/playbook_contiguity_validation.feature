Feature: Playbook contiguity validation rejects non-flowing machines
  validate_contiguity enforces three graph invariants over a machine's states
  and transitions so a machine that can't flow never registers:
  - Reachability: every NON-terminal declared state is reachable from the
    initial (first declared) state. (Terminal escape-hatch states may be
    out-of-band and are exempt.)
  - Forward-progress: every non-terminal state has at least one outgoing
    transition — a non-terminal dead-end is rejected.
  - Terminal-reachability: from every state the playbook can settle (reach a
    terminal state or a stable sink loop).
  A well-formed machine passes; the compiled-in seeds all pass.

  Event/step/queue-driven machines (e.g. import_transaction_history) declare
  states + per-state event maps but NO standard `transitions:` graph. Such a
  machine has an EMPTY transitions list, so the standard begin/complete
  lifecycle never drives it — it is driven by its own event mechanism. These
  machines are EXEMPT from contiguity validation entirely: they must load as
  valid registry entries, never rejected as dead-ends.

  Scenario: an event-driven machine with no standard transitions is exempt
    Given a contiguity fixture machine with states:
      | name        | is_terminal |
      | pending     | false       |
      | reading_csv | false       |
      | completed   | true        |
      | failed      | true        |
    When validate_contiguity is called on the fixture
    Then the contiguity result is Ok

  Scenario: a machine with a non-terminal dead-end fails validation
    Given a contiguity fixture machine with states:
      | name  | is_terminal |
      | start | false       |
      | stuck | false       |
      | done  | true        |
    And contiguity fixture transitions:
      | from_state | to_state |
      | start      | stuck    |
      | start      | done     |
    When validate_contiguity is called on the fixture
    Then the contiguity result is an error with code "playbook_dead_end_state"

  Scenario: a machine with an unreachable non-terminal state fails validation
    Given a contiguity fixture machine with states:
      | name      | is_terminal |
      | start     | false       |
      | orphan    | false       |
      | done      | true        |
    And contiguity fixture transitions:
      | from_state | to_state |
      | start      | done     |
      | orphan     | done     |
    When validate_contiguity is called on the fixture
    Then the contiguity result is an error with code "playbook_unreachable_state"

  Scenario: a well-formed machine passes validation
    Given a contiguity fixture machine with states:
      | name  | is_terminal |
      | start | false       |
      | mid   | false       |
      | done  | true        |
    And contiguity fixture transitions:
      | from_state | to_state |
      | start      | mid      |
      | mid        | done     |
    When validate_contiguity is called on the fixture
    Then the contiguity result is Ok

  Scenario: the track seed passes contiguity validation
    Given the contiguity track seed
    When validate_contiguity is called on the seed
    Then the contiguity result is Ok

  Scenario: the proposal seed passes contiguity validation
    Given the contiguity proposal seed
    When validate_contiguity is called on the seed
    Then the contiguity result is Ok
