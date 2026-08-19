Feature: PlaybookFidelity RPC folds the activity log into a fidelity measure
  The engine's PlaybookFidelity read folds the durable, redacted activity-log
  sink into the Layer-3 fidelity measure: per-kind completion (begun, terminal,
  rate), dangling instances (begun + transitioned but never terminal), revision
  cycles, and review authenticity (delegated vs self-review exits). The gRPC RPC
  and the loopback `/ws` JSON-RPC method fold the SAME core path, so the two
  surfaces can never diverge; counts are JSON numbers, never strings. The panel
  in the anvil-kit dashboard is the WS method's user.

  Scenario: the gRPC PlaybookFidelity folds the seeded instances
    Given an activity log engine hearth seeded with turns:
      | command  | outcome | artifact_kind | from_state   | to_state     | actor_hash | at                   | playbook_run_id |
      | begin    | ok      | track         |              | spec         | aaaa1111   | 2026-06-15T09:00:00Z | inst_a               |
      | complete | ok      | track         | implementing | abandoned    | aaaa1111   | 2026-06-15T10:00:00Z | inst_a               |
      | begin    | ok      | track         |              | spec         | bbbb2222   | 2026-06-15T09:00:00Z | inst_b               |
      | complete | ok      | track         | spec         | implementing | bbbb2222   | 2026-06-15T10:00:00Z | inst_b               |
    And the engine is started with that hearth
    When the PlaybookFidelity RPC is called
    Then the fidelity RPC completion for "track" has begun 2
    And the fidelity RPC completion for "track" has terminal 1
    And the fidelity RPC has 1 dangling instances
    And the fidelity RPC instance "inst_a" has folded state "abandoned" reached terminal "true" and dangling "false"
    And the fidelity RPC instance "inst_b" has folded state "implementing" reached terminal "false" and dangling "true"

  Scenario: the /ws playbook_fidelity folds identically to the gRPC RPC
    Given an activity log engine hearth seeded with turns:
      | command  | outcome | artifact_kind | from_state   | to_state    | actor_hash | at                   | playbook_run_id |
      | begin    | ok      | track         |              | spec        | aaaa1111   | 2026-06-15T09:00:00Z | inst_a               |
      | complete | ok      | track         | implementing | superseded  | aaaa1111   | 2026-06-15T10:00:00Z | inst_a               |
      | begin    | ok      | track         |              | spec        | bbbb2222   | 2026-06-15T09:00:00Z | inst_b               |
      | complete | ok      | track         | spec         | spec_review | bbbb2222   | 2026-06-15T10:00:00Z | inst_b               |
      | complete | ok      | track         | spec_review  | plan        | cccc3333   | 2026-06-15T10:30:00Z | inst_b               |
    And the engine is started with that hearth
    When a playbook_fidelity JSON-RPC request is sent over /ws
    Then the /ws fidelity completion for "track" has begun 2
    And the /ws fidelity completion for "track" has terminal 1
    And the /ws fidelity dangling_instances is 1
    And the /ws fidelity dangling_instances is a JSON number
    And the /ws fidelity review_exits is 1
    And the /ws fidelity delegated_exits is 1
    And the /ws fidelity instance "inst_b" has folded state "plan" reached terminal "false" and dangling "true"

  Scenario: an empty sink folds to a zeroed fidelity over /ws
    Given an activity log engine hearth seeded with turns:
      | command | outcome | artifact_kind | actor_hash | at |
    And the engine is started with that hearth
    When a playbook_fidelity JSON-RPC request is sent over /ws
    Then the /ws fidelity dangling_instances is 0
    And the /ws fidelity completion has 0 entries
    And the /ws fidelity instances has 0 entries
</content>
