Feature: K8 backlog_item is a first-class artifact type
  Registration is all-or-nothing (Task 3): the compiled Free machine, catalog
  type, describe type, checkin creation routing, bare-ID directory lookup, and
  kind-aware terminal filtering must all resolve, and the Free kind must stay
  out of driven candidates.

  Background:
    Given a backlog fixture

  @registration
  Scenario Outline: Every first-class registration surface resolves
    When the backlog registration surfaces are inspected
    Then the backlog registration property "<property>" is satisfied

    Examples:
      | property                          |
      | first_class_artifact_type         |
      | serde_name_backlog_item           |
      | directory_backlog_items           |
      | registry_backlog_items_md         |
      | catalog_type_present              |
      | describe_type_present             |
      | describe_required_field_item      |
      | checkin_creation_supported        |
      | bare_id_directory_lookup          |
      | register_free                     |
      | excluded_from_driven_candidates   |
      | seven_type_workflow_baseline      |

  @registration
  Scenario Outline: The three K8 terminals are terminal and non-terminals are active
    When the backlog registration surfaces are inspected
    Then the backlog registration property "<property>" is satisfied

    Examples:
      | property                  |
      | terminal_done             |
      | terminal_superseded       |
      | terminal_aged_out         |
      | nonterminal_candidate     |
      | nonterminal_ready         |
      | nonterminal_in_flight     |
      | nonterminal_parked        |

  @registration
  Scenario Outline: A real hearth scan hides the K8 terminals and keeps the four live states
    Given a real hearth holding one backlog item in every K8 state and two tracks
    When the backlog registration surfaces are inspected
    Then the backlog registration property "<property>" is satisfied

    Examples:
      | property                                |
      | catalog_hides_done                      |
      | catalog_hides_superseded                |
      | catalog_hides_aged_out                  |
      | catalog_lists_candidate                 |
      | catalog_lists_ready                     |
      | catalog_lists_in_flight                 |
      | catalog_lists_parked                    |
      | catalog_unrelated_kind_terminals_intact |

  @registration
  Scenario Outline: A bare backlog id resolves through every hardcoded directory lookup
    Given a real hearth holding one backlog item in every K8 state and two tracks
    When the backlog registration surfaces are inspected
    Then the backlog registration property "<property>" is satisfied

    Examples:
      | property                               |
      | snapshot_adapter_resolves_bare_id      |
      | query_adapter_resolves_bare_id         |
      | transition_adapter_resolves_bare_id    |

  @registration
  Scenario: Every K8 state maps to a registry section in backlog_items.md
    When the backlog registration surfaces are inspected
    Then the backlog registration property "registry_sections_all_seven" is satisfied
    And the backlog registration property "registry_file_backlog_items_md" is satisfied

  @registration
  Scenario: The seed compiles with exactly twenty-nine transition definitions
    When the backlog registration surfaces are inspected
    Then the backlog registration property "twenty_nine_seed_transitions" is satisfied
    And the backlog registration property "no_genesis_seed_row" is satisfied
    And the backlog registration property "contiguity_all_states_reach_terminal" is satisfied
    And the backlog registration property "seed_matches_legal_transition_table" is satisfied
