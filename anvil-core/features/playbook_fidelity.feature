Feature: WorkflowFidelity folds the activity log into a fidelity measure
  The WorkflowFidelity read-side folds the durable, redacted activity-log record
  stream (one record per command turn the engine serves) into the Layer-3
  fidelity measure: per-kind completion (begun, terminal, rate), dangling
  instances (begun + transitioned but never terminal), revision-cycle depth
  (transitions into a *_revision state), and review authenticity (per review-gate
  exit, whether the entering actor differs from the exiting actor, plus the
  elapsed time in the review state). It is purely derived — no new
  instrumentation. Records lacking a playbook_run_id are skipped from the
  per-instance measures. The terminal predicate is per-kind (from the registry):
  for the track seed `completed` is NON-terminal; `abandoned`/`superseded` are
  terminal. An empty stream folds to a zeroed result (no error).

  Scenario: completion rate is terminal-reaching instances over begun instances
    # Three track instances begun; two reach a terminal transition (abandoned /
    # superseded), one stalls at implementing → completion rate 2/3.
    Given a playbook fidelity record stream:
      | command  | from_state   | to_state     | actor_hash | at                   | artifact_kind | playbook_run_id |
      | begin    |              | spec         | aaaa1111   | 2026-06-15T09:00:00Z | track         | inst_a               |
      | complete | implementing | abandoned    | aaaa1111   | 2026-06-15T10:00:00Z | track         | inst_a               |
      | begin    |              | spec         | bbbb2222   | 2026-06-15T09:00:00Z | track         | inst_b               |
      | complete | implementing | superseded   | bbbb2222   | 2026-06-15T10:00:00Z | track         | inst_b               |
      | begin    |              | spec         | cccc3333   | 2026-06-15T09:00:00Z | track         | inst_c               |
      | complete | spec         | implementing | cccc3333   | 2026-06-15T10:00:00Z | track         | inst_c               |
    When the playbook fidelity is folded
    Then the playbook fidelity completion for "track" has begun 3
    And the playbook fidelity completion for "track" has terminal 2

  Scenario: an instance begun and transitioned but never terminal is dangling
    # `completed` is NON-terminal for the track seed, so an instance that only
    # reaches `completed` is still dangling — its loop was never closed.
    Given a playbook fidelity record stream:
      | command  | from_state   | to_state     | actor_hash | at                   | artifact_kind | playbook_run_id |
      | begin    |              | spec         | aaaa1111   | 2026-06-15T09:00:00Z | track         | inst_a               |
      | complete | implementing | superseded   | aaaa1111   | 2026-06-15T10:00:00Z | track         | inst_a               |
      | begin    |              | spec         | bbbb2222   | 2026-06-15T09:00:00Z | track         | inst_b               |
      | complete | implementing | completed    | bbbb2222   | 2026-06-15T10:00:00Z | track         | inst_b               |
      | begin    |              | spec         | cccc3333   | 2026-06-15T09:00:00Z | track         | inst_c               |
      | complete | spec         | implementing | cccc3333   | 2026-06-15T10:00:00Z | track         | inst_c               |
    When the playbook fidelity is folded
    Then the playbook fidelity has 2 dangling instances
    And the playbook fidelity dangling for "track" has count 2

  Scenario: revision-cycle depth counts transitions into a *_revision state
    # Two transitions into spec_revision/impl_revision for one instance → depth 2.
    Given a playbook fidelity record stream:
      | command  | from_state  | to_state      | actor_hash | at                   | artifact_kind | playbook_run_id |
      | begin    |             | spec          | aaaa1111   | 2026-06-15T09:00:00Z | track         | inst_a               |
      | complete | spec_review | spec_revision | aaaa1111   | 2026-06-15T10:00:00Z | track         | inst_a               |
      | complete | spec        | implementing  | aaaa1111   | 2026-06-15T11:00:00Z | track         | inst_a               |
      | complete | impl_review | impl_revision | aaaa1111   | 2026-06-15T12:00:00Z | track         | inst_a               |
      | complete | impl_review | superseded    | aaaa1111   | 2026-06-15T13:00:00Z | track         | inst_a               |
    When the playbook fidelity is folded
    Then the playbook fidelity has 2 revision cycles total
    And the playbook fidelity revision cycles for "track" has count 2

  Scenario: a review entered and exited by different actors is delegated
    # Author A enters spec_review (the spec→spec_review transition is by A); a
    # different actor B exits it (spec_review→plan). A != B → delegated/authentic.
    Given a playbook fidelity record stream:
      | command  | from_state  | to_state    | actor_hash | at                   | artifact_kind | playbook_run_id |
      | complete | spec        | spec_review | aaaa1111   | 2026-06-15T09:00:00Z | track         | inst_a               |
      | complete | spec_review | plan        | bbbb2222   | 2026-06-15T09:30:00Z | track         | inst_a               |
    When the playbook fidelity is folded
    Then the playbook fidelity has 1 review exits
    And the playbook fidelity has 1 delegated exits
    And the playbook fidelity has 0 self review exits

  Scenario: a review entered and exited by the same actor is a self-review
    # Author A enters spec_review and A also exits it → self-review (the
    # rubber-stamp signature).
    Given a playbook fidelity record stream:
      | command  | from_state  | to_state    | actor_hash | at                   | artifact_kind | playbook_run_id |
      | complete | spec        | spec_review | aaaa1111   | 2026-06-15T09:00:00Z | track         | inst_a               |
      | complete | spec_review | plan        | aaaa1111   | 2026-06-15T09:00:30Z | track         | inst_a               |
    When the playbook fidelity is folded
    Then the playbook fidelity has 1 review exits
    And the playbook fidelity has 0 delegated exits
    And the playbook fidelity has 1 self review exits

  Scenario: elapsed review time is computed from the enter and exit timestamps
    # spec_review entered at 09:00:00 and exited at 09:30:00 → 1800 seconds.
    Given a playbook fidelity record stream:
      | command  | from_state  | to_state    | actor_hash | at                   | artifact_kind | playbook_run_id |
      | complete | spec        | spec_review | aaaa1111   | 2026-06-15T09:00:00Z | track         | inst_a               |
      | complete | spec_review | plan        | bbbb2222   | 2026-06-15T09:30:00Z | track         | inst_a               |
    When the playbook fidelity is folded
    Then the playbook fidelity has a review elapsed sample of 1800 seconds

  Scenario: records lacking a playbook_run_id are skipped
    # A route turn with no instance id contributes nothing to the per-instance
    # measures; only the one instance with an id is begun.
    Given a playbook fidelity record stream:
      | command  | from_state | to_state  | actor_hash | at                   | artifact_kind | playbook_run_id |
      | route    |            |           | aaaa1111   | 2026-06-15T08:00:00Z | track         |                      |
      | begin    |            | spec      | aaaa1111   | 2026-06-15T09:00:00Z | track         | inst_a               |
      | complete | spec       | abandoned | aaaa1111   | 2026-06-15T10:00:00Z | track         | inst_a               |
    When the playbook fidelity is folded
    Then the playbook fidelity completion for "track" has begun 1
    And the playbook fidelity completion for "track" has terminal 1
    And the playbook fidelity has 0 dangling instances

  Scenario: an empty stream folds to a zeroed result
    Given an empty playbook fidelity record stream
    When the playbook fidelity is folded
    Then the playbook fidelity has 0 dangling instances
    And the playbook fidelity has 0 revision cycles total
    And the playbook fidelity has 0 review exits
    And the playbook fidelity completion has 0 entries

  Scenario: cross-hearth fold merges instances from both hearths
    Given playbook fidelity hearth A stream:
      | command  | from_state   | to_state   | actor_hash | at                   | artifact_kind | playbook_run_id |
      | begin    |              | spec       | aaaa1111   | 2026-06-15T09:00:00Z | track         | inst_a               |
      | complete | implementing | superseded | aaaa1111   | 2026-06-15T10:00:00Z | track         | inst_a               |
    And playbook fidelity hearth B stream:
      | command  | from_state   | to_state     | actor_hash | at                   | artifact_kind | playbook_run_id |
      | begin    |              | spec         | bbbb2222   | 2026-06-15T09:00:00Z | track         | inst_b               |
      | complete | spec         | implementing | bbbb2222   | 2026-06-15T10:00:00Z | track         | inst_b               |
    When the playbook fidelity is folded across both hearths
    Then the playbook fidelity completion for "track" has begun 2
    And the playbook fidelity completion for "track" has terminal 1
    And the playbook fidelity has 1 dangling instances

  Scenario: per-instance fidelity reports terminal dangling and revision evidence
    Given a playbook fidelity record stream:
      | command  | from_state      | to_state        | actor_hash | at                   | artifact_kind | playbook_run_id |
      | begin    |                 | vision          | aaaa1111   | 2026-06-15T09:00:00Z | proposal      | inst_a               |
      | complete | active          | completed       | aaaa1111   | 2026-06-15T10:00:00Z | proposal      | inst_a               |
      | begin    |                 | spec            | bbbb2222   | 2026-06-15T09:00:00Z | track         | inst_b               |
      | complete | spec_review     | spec_revision   | bbbb2222   | 2026-06-15T10:00:00Z | track         | inst_b               |
      | complete | spec_revision   | implementing    | bbbb2222   | 2026-06-15T11:00:00Z | track         | inst_b               |
      | begin    |                 | spec            | cccc3333   | 2026-06-15T09:00:00Z | track         | inst_c               |
      | complete | implementation | abandoned       | cccc3333   | 2026-06-15T10:00:00Z | track         | inst_c               |
    When the playbook fidelity is folded
    Then playbook fidelity instance "inst_a" has kind "proposal" folded state "completed" transition count 2 reached terminal "true" dangling "false" and revision cycles 0
    And playbook fidelity instance "inst_b" has kind "track" folded state "implementing" transition count 3 reached terminal "false" dangling "true" and revision cycles 1
    And playbook fidelity instance "inst_c" has kind "track" folded state "abandoned" transition count 2 reached terminal "true" dangling "false" and revision cycles 0
    And playbook fidelity instance rows are ordered by instance id

  Scenario: per-instance folded state comes from transitions rather than stale cache
    Given a playbook fidelity record stream:
      | command  | from_state | to_state     | actor_hash | at                   | artifact_kind | playbook_run_id |
      | begin    |            | spec         | aaaa1111   | 2026-06-15T09:00:00Z | track         | inst_stale_cache     |
      | complete | spec       | implementing | aaaa1111   | 2026-06-15T10:00:00Z | track         | inst_stale_cache     |
    When the playbook fidelity is folded
    Then playbook fidelity instance "inst_stale_cache" has folded state "implementing"
