Feature: Survivor-corrected outcome rates
  The survivor-corrected outcome fold joins the activity log (begun + terminal per
  kind, reusing the same per-kind terminal predicate the fidelity fold uses) with
  the playbook-measurement sink (outcome-satisfied = a begun instance whose
  terminal measurement recorded success). The headline outcome_rate is over BEGUN
  instances, NOT completed ones: a dangling instance (begun but never terminal)
  emits no terminal measurement and so is counted as an outcome-FAILURE. Judging
  only terminals would give the router survivor-biased fitness. For the track seed
  `superseded` is terminal and `implementing`/`completed` are not.

  Scenario: the rate is over begun, so dangling drags it below the survivor rate
    # 3 begun; 2 reach superseded (terminal) with a success measurement; 1 stalls
    # at implementing (dangling). Corrected rate = 2/3, NOT the survivor-biased 2/2.
    Given a survivor activity stream:
      | command  | from_state   | to_state     | at                   | artifact_kind | playbook_run_id |
      | begin    |              | spec         | 2026-06-15T09:00:00Z | track         | inst_a               |
      | complete | implementing | superseded   | 2026-06-15T10:00:00Z | track         | inst_a               |
      | begin    |              | spec         | 2026-06-15T09:00:00Z | track         | inst_b               |
      | complete | implementing | superseded   | 2026-06-15T10:00:00Z | track         | inst_b               |
      | begin    |              | spec         | 2026-06-15T09:00:00Z | track         | inst_c               |
      | complete | spec         | implementing | 2026-06-15T10:00:00Z | track         | inst_c               |
    And a survivor measurement stream:
      | playbook_run_id | success |
      | inst_a               | true    |
      | inst_b               | true    |
    When the survivor outcome is folded
    Then the survivor outcome for "track" has begun 3
    And the survivor outcome for "track" has terminal 2
    And the survivor outcome for "track" has outcome_satisfied 2
    And the survivor outcome for "track" has 1 dangling
    And the survivor outcome for "track" has outcome rate permille 667

  Scenario: a terminal instance whose measurement failed is not outcome-satisfied
    # Both instances reach superseded, but inst_b's outcome predicate failed
    # (success=false). outcome_satisfied 1 < terminal 2 → rate 1/2.
    Given a survivor activity stream:
      | command  | from_state   | to_state   | at                   | artifact_kind | playbook_run_id |
      | begin    |              | spec       | 2026-06-15T09:00:00Z | track         | inst_a               |
      | complete | implementing | superseded | 2026-06-15T10:00:00Z | track         | inst_a               |
      | begin    |              | spec       | 2026-06-15T09:00:00Z | track         | inst_b               |
      | complete | implementing | superseded | 2026-06-15T10:00:00Z | track         | inst_b               |
    And a survivor measurement stream:
      | playbook_run_id | success |
      | inst_a               | true    |
      | inst_b               | false   |
    When the survivor outcome is folded
    Then the survivor outcome for "track" has begun 2
    And the survivor outcome for "track" has terminal 2
    And the survivor outcome for "track" has outcome_satisfied 1
    And the survivor outcome for "track" has outcome rate permille 500

  Scenario: empty streams fold to an empty result
    Given an empty survivor activity stream
    And an empty survivor measurement stream
    When the survivor outcome is folded
    Then the survivor outcome has 0 rows
