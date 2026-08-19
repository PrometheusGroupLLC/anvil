Feature: Playbook interpreter — outgoing transitions query

  # R12.1: pure-function bridge between seeded/loaded playbooks and consumers.
  # outgoing_transitions(machine, from_state) returns Vec<OutgoingTransition>
  # containing {to_state, required_role} for every transition whose from_state
  # matches. No I/O. No global state. Stable list order (preserves machine.transitions order).
  #
  # Scenarios use a fixture PlaybookMachine built by step helpers — NOT the
  # seeded track playbook. The seed-driven scenarios live in Phase 5 against
  # describe::available_actions (the consumer), per R12.3.

  Scenario: state with two outgoing transitions returns both in declaration order
    Given a fixture playbook machine with transitions:
      | from_state | to_state       | required_role |
      | drafting   | review         | doer          |
      | drafting   | abandoned      | doer          |
      | review     | active         | reviewer      |
    When outgoing_transitions is called for state "drafting"
    Then the result contains 2 transitions
    And result transition 0 has to_state "review" and required_role "doer"
    And result transition 1 has to_state "abandoned" and required_role "doer"

  Scenario: state with no outgoing transitions returns an empty vec
    Given a fixture playbook machine with transitions:
      | from_state | to_state | required_role |
      | drafting   | review   | doer          |
    When outgoing_transitions is called for state "review"
    Then the result contains 0 transitions

  Scenario: state not declared in machine.states returns an empty vec (defensive)
    Given a fixture playbook machine with transitions:
      | from_state | to_state | required_role |
      | drafting   | review   | doer          |
    When outgoing_transitions is called for state "nonexistent_state"
    Then the result contains 0 transitions

  Scenario: required_role is returned verbatim from the transition definition
    Given a fixture playbook machine with transitions:
      | from_state | to_state | required_role              |
      | spec       | spec_review | reviewer-with-special-chars |
    When outgoing_transitions is called for state "spec"
    Then the result contains 1 transitions
    And result transition 0 has to_state "spec_review" and required_role "reviewer-with-special-chars"
