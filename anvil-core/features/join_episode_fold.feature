Feature: The joined-episode fold — one join rule, causes not one bucket, per hearth
  `fold_join_episodes` pairs a delivery row that carried a guidance kind with the
  begin of that kind in the same conversation and the same hearth. The seam is
  anvil-core and the user is the engine: it reads both sinks through ports and
  hands the vectors in. The fold is PURE — no filesystem, no I/O, no write port,
  and a read-only `PlaybookRegistry` for one reason only, stated below.

  THE MATCHING IS PROVED OVER SYNTHESIZED VECTORS, deliberately. Driving it
  through a live engine would make the interesting cases — a conversation absent
  from the delivery side, two deliveries against one begin, a kind whose
  `completed` is NOT terminal — expensive or unreachable.

  TERMINALITY IS PER KIND AND THE REGISTRY IS WHY THE FOLD TAKES ONE. The flat
  `TERMINAL_STATES` list contains "completed", while the live `track` machine
  declares `completed` with `is_terminal: false`. A flat-list implementation
  folds green on invented vectors and mislabels every track run on the real
  hearth. Two scenarios below fix that from both sides: a kind whose `completed`
  is non-terminal, and a kind whose terminal state is not on the flat list at
  all.

  UNJOINABLE ROWS ARE COUNTED, NEVER DROPPED. Every bucket carrying "no",
  "absent", "pre" or "superseded" in its name is a reported count INSIDE its
  denominator. A report that improves its coverage number by excluding rows is
  the failure this fold exists to make impossible, so the sentinel row and the
  pre-migration row are episodes with a reason and not absences.

  NOTHING POOLS. `per_hearth` is a list in INPUT order and there is no fleet
  total. The two hearths measured during planning sit at 32.9% and 64.4%
  begin-side coverage under different salts; a pooled number would be dominated
  by whichever is better instrumented and would be reported to two decimals.

  Background:
    Given join fixture playbooks:
      | kind     | state        | is_terminal |
      | track    | implementing | false       |
      | track    | reviewing    | false       |
      | track    | completed    | false       |
      | proposal | drafting     | false       |
      | proposal | reviewing    | false       |
      | proposal | accepted     | true        |
    And join options window "" to "" key epoch "e0e0e0e0e0e0"

  Scenario: A delivered kind followed by a begin of that kind in the same conversation and hearth joins
    Given a join hearth "anvil"
    And delivery rows:
      | id | at                   | conversation_hash | guidance_kind |
      | d1 | 2026-08-10T09:00:00Z | c1                | track         |
      | d2 | 2026-08-10T09:01:00Z | c2                | track         |
    And activity rows:
      | command | at                   | artifact_kind | to_state     | conversation_hash | playbook_run_id |
      | begin   | 2026-08-10T09:05:00Z | track         | implementing | c1                | run-a           |
      | begin   | 2026-08-10T09:06:00Z | track         | implementing | c2                | run-b           |
    When the join episodes are folded
    Then the join set holds 2 episodes and 0 unmatched begins
    And the episode "d1" joins to begin "run-a"
    And the episode "d2" joins to begin "run-b"

  Scenario: A delivered kind followed by a begin of a DIFFERENT kind does not join
    Given a join hearth "anvil"
    And delivery rows:
      | id | at                   | conversation_hash | guidance_kind |
      | d1 | 2026-08-10T09:00:00Z | c1                | track         |
    And activity rows:
      | command | at                   | artifact_kind | to_state | conversation_hash | playbook_run_id |
      | begin   | 2026-08-10T09:05:00Z | proposal      | drafting | c1                | run-a           |
    When the join episodes are folded
    Then the episode "d1" is unjoined with reason "NoBeginOfKindInConversation"
    And the begin "run-a" is unjoined with reason "NoPriorDeliveryOfKind"

  Scenario: A begin matches the most recent PRIOR delivery, never a later one
    Given a join hearth "anvil"
    And delivery rows:
      | id | at                   | conversation_hash | guidance_kind |
      | d1 | 2026-08-10T08:00:00Z | c1                | track         |
      | d2 | 2026-08-10T10:00:00Z | c1                | track         |
    And activity rows:
      | command | at                   | artifact_kind | to_state     | conversation_hash | playbook_run_id |
      | begin   | 2026-08-10T09:00:00Z | track         | implementing | c1                | run-a           |
    When the join episodes are folded
    Then the episode "d1" joins to begin "run-a"
    And the episode "d2" is unjoined with reason "NoBeginOfKindInConversation"

  Scenario: Two deliveries of one kind and one begin, the later joined and the earlier superseded
    Given a join hearth "anvil"
    And delivery rows:
      | id | at                   | conversation_hash | guidance_kind |
      | d1 | 2026-08-10T08:00:00Z | c1                | track         |
      | d2 | 2026-08-10T08:30:00Z | c1                | track         |
    And activity rows:
      | command | at                   | artifact_kind | to_state     | conversation_hash | playbook_run_id |
      | begin   | 2026-08-10T09:00:00Z | track         | implementing | c1                | run-a           |
    When the join episodes are folded
    Then the episode "d2" joins to begin "run-a"
    And the episode "d1" is unjoined with reason "SupersededByLaterDeliveryOfKind"
    And the join set holds 2 episodes and 0 unmatched begins

  Scenario: Two begins of one kind and one delivery, the second begin has no unconsumed prior delivery
    Given a join hearth "anvil"
    And delivery rows:
      | id | at                   | conversation_hash | guidance_kind |
      | d1 | 2026-08-10T08:00:00Z | c1                | track         |
    And activity rows:
      | command | at                   | artifact_kind | to_state     | conversation_hash | playbook_run_id |
      | begin   | 2026-08-10T09:00:00Z | track         | implementing | c1                | run-a           |
      | begin   | 2026-08-10T09:30:00Z | track         | implementing | c1                | run-b           |
    When the join episodes are folded
    Then the episode "d1" joins to begin "run-a"
    And the begin "run-b" is unjoined with reason "NoUnconsumedPriorDeliveryOfKind"

  Scenario: Episodes never match a begin in a different hearth
    Given a join hearth "anvil"
    And delivery rows:
      | id | at                   | conversation_hash | guidance_kind |
      | d1 | 2026-08-10T08:00:00Z | c1                | track         |
    And a join hearth "foundry"
    And activity rows:
      | command | at                   | artifact_kind | to_state     | conversation_hash | playbook_run_id |
      | begin   | 2026-08-10T09:00:00Z | track         | implementing | c1                | run-a           |
    When the join episodes are folded
    Then the episode "d1" is unjoined with reason "ConversationAbsentFromBeginSide"
    And the begin "run-a" is unjoined with reason "ConversationAbsentFromDeliverySide"

  Scenario: A row whose conversation key is the sentinel is an episode with no conversation key, not a dropped row
    Given a join hearth "anvil"
    And delivery rows:
      | id | at                   | conversation_hash | guidance_kind |
      | d1 | 2026-08-10T08:00:00Z | -                 | track         |
    When the join episodes are folded
    Then the episode "d1" is unjoined with reason "NoConversationKey"
    And the coverage for hearth "anvil" reports "episode_denominator" as 1
    And the coverage for hearth "anvil" reports "unjoin.no_conversation_key" as 1

  Scenario: A pre-migration row is an episode with its own reason, inside the denominator
    Given a join hearth "anvil"
    And delivery rows:
      | id | at                   | conversation_hash | guidance_kind | pre_migration |
      | d1 | 2026-08-10T08:00:00Z | -                 | track         | true          |
    When the join episodes are folded
    Then the episode "d1" is unjoined with reason "PreMigrationRow"
    And the coverage for hearth "anvil" reports "episode_denominator" as 1
    And the coverage for hearth "anvil" reports "unjoin.pre_migration_row" as 1
    And the coverage for hearth "anvil" reports "unjoin.no_conversation_key" as 0

  Scenario: A begin whose conversation never appears on the delivery side is bucketed apart from a begin with no key at all
    Given a join hearth "anvil"
    And delivery rows:
      | id | at                   | conversation_hash | guidance_kind |
      | d1 | 2026-08-10T08:00:00Z | c1                | track         |
    And activity rows:
      | command | at                   | artifact_kind | to_state     | conversation_hash | playbook_run_id |
      | begin   | 2026-08-10T09:00:00Z | track         | implementing | c1                | run-a           |
      | begin   | 2026-08-10T09:10:00Z | track         | implementing | c9                | run-b           |
      | begin   | 2026-08-10T09:20:00Z | track         | implementing | -                 | run-c           |
    When the join episodes are folded
    Then the episode "d1" joins to begin "run-a"
    And the begin "run-b" is unjoined with reason "ConversationAbsentFromDeliverySide"
    And the begin "run-c" is unjoined with reason "NoConversationKey"
    And the coverage for hearth "anvil" reports "begin_denominator" as 3

  Scenario: A joined run whose fixture kind declares completed NON-terminal is not yet terminal
    Given a join hearth "anvil"
    And delivery rows:
      | id | at                   | conversation_hash | guidance_kind |
      | d1 | 2026-08-10T08:00:00Z | c1                | track         |
    And activity rows:
      | command  | at                   | artifact_kind | from_state   | to_state     | conversation_hash | playbook_run_id |
      | begin    | 2026-08-10T09:00:00Z | track         | -            | implementing | c1                | run-a           |
      | complete | 2026-08-10T10:00:00Z | track         | implementing | completed    | -                 | run-a           |
    When the join episodes are folded
    Then the episode "d1" joins to begin "run-a"
    And the episode "d1" terminal status is "NotYetTerminal"

  Scenario: A joined run whose fixture kind declares a custom terminal state has reached terminal
    Given a join hearth "anvil"
    And delivery rows:
      | id | at                   | conversation_hash | guidance_kind |
      | d1 | 2026-08-10T08:00:00Z | c1                | proposal      |
    And activity rows:
      | command  | at                   | artifact_kind | from_state | to_state | conversation_hash | playbook_run_id |
      | begin    | 2026-08-10T09:00:00Z | proposal      | -          | drafting | c1                | run-a           |
      | complete | 2026-08-10T10:00:00Z | proposal      | drafting   | accepted | -                 | run-a           |
    When the join episodes are folded
    Then the episode "d1" joins to begin "run-a"
    And the episode "d1" terminal status is "ReachedTerminal"

  Scenario: A joined run with no transition record has an unknown run state
    Given a join hearth "anvil"
    And delivery rows:
      | id | at                   | conversation_hash | guidance_kind |
      | d1 | 2026-08-10T08:00:00Z | c1                | track         |
    And activity rows:
      | command | at                   | artifact_kind | to_state | conversation_hash | playbook_run_id |
      | begin   | 2026-08-10T09:00:00Z | track         | -        | c1                | run-a           |
    When the join episodes are folded
    Then the episode "d1" joins to begin "run-a"
    And the episode "d1" terminal status is "UnknownRunState"
    And the episode "d1" has no last transition at

  Scenario: A joined run that began and never moved reports the begin as its last transition
    Given a join hearth "anvil"
    And delivery rows:
      | id | at                   | conversation_hash | guidance_kind |
      | d1 | 2026-08-10T08:00:00Z | c1                | track         |
    And activity rows:
      | command | at                   | artifact_kind | to_state     | conversation_hash | playbook_run_id |
      | begin   | 2026-08-10T09:00:00Z | track         | implementing | c1                | run-a           |
    When the join episodes are folded
    Then the episode "d1" last transition at is "2026-08-10T09:00:00Z"
    And the episode "d1" terminal status is "NotYetTerminal"

  Scenario: A joined run with two later transitions reports the latest verbatim from the record terminality was read from
    Given a join hearth "anvil"
    And delivery rows:
      | id | at                   | conversation_hash | guidance_kind |
      | d1 | 2026-08-10T08:00:00Z | c1                | proposal      |
    And activity rows:
      | command  | at                   | artifact_kind | from_state | to_state  | conversation_hash | playbook_run_id |
      | begin    | 2026-08-10T09:00:00Z | proposal      | -          | drafting  | c1                | run-a           |
      | complete | 2026-08-10T10:00:00Z | proposal      | drafting   | reviewing | -                 | run-a           |
      | complete | 2026-08-10T11:00:00Z | proposal      | reviewing  | accepted  | -                 | run-a           |
    When the join episodes are folded
    Then the episode "d1" last transition at is "2026-08-10T11:00:00Z"
    And the episode "d1" terminal status is "ReachedTerminal"

  Scenario: The last transition is absent if and only if the run state is unknown
    Given a join hearth "anvil"
    And delivery rows:
      | id | at                   | conversation_hash | guidance_kind |
      | d1 | 2026-08-10T08:00:00Z | c1                | track         |
      | d2 | 2026-08-10T08:10:00Z | c2                | track         |
    And activity rows:
      | command | at                   | artifact_kind | to_state     | conversation_hash | playbook_run_id |
      | begin   | 2026-08-10T09:00:00Z | track         | -            | c1                | run-a           |
      | begin   | 2026-08-10T09:10:00Z | track         | implementing | c2                | run-b           |
    When the join episodes are folded
    Then the episode "d1" has no last transition at
    And the episode "d1" terminal status is "UnknownRunState"
    And the episode "d2" last transition at is "2026-08-10T09:10:00Z"
    And the episode "d2" terminal status is "NotYetTerminal"
    And the last transition is present exactly when the run state is known

  Scenario: A transition in another hearth carrying the same playbook run id does not supply the last transition
    Given a join hearth "anvil"
    And delivery rows:
      | id | at                   | conversation_hash | guidance_kind |
      | d1 | 2026-08-10T08:00:00Z | c1                | track         |
    And activity rows:
      | command | at                   | artifact_kind | to_state     | conversation_hash | playbook_run_id |
      | begin   | 2026-08-10T09:00:00Z | track         | implementing | c1                | run-a           |
    And a join hearth "foundry"
    And activity rows:
      | command  | at                   | artifact_kind | from_state   | to_state  | conversation_hash | playbook_run_id |
      | complete | 2026-08-10T11:00:00Z | track         | implementing | completed | -                 | run-a           |
    When the join episodes are folded
    Then the episode "d1" last transition at is "2026-08-10T09:00:00Z"

  Scenario: Every episode carries exactly one of a matched begin or an unjoin reason
    Given a join hearth "anvil"
    And delivery rows:
      | id | at                   | conversation_hash | guidance_kind |
      | d1 | 2026-08-10T08:00:00Z | c1                | track         |
      | d2 | 2026-08-10T08:10:00Z | c2                | track         |
      | d3 | 2026-08-10T08:20:00Z | -                 | proposal      |
    And activity rows:
      | command | at                   | artifact_kind | to_state     | conversation_hash | playbook_run_id |
      | begin   | 2026-08-10T09:00:00Z | track         | implementing | c1                | run-a           |
    When the join episodes are folded
    Then the join set holds 3 episodes and 0 unmatched begins
    And every episode carries exactly one of matched begin or unjoin reason

  Scenario: Per-kind and per-week groups each carry their own n and a group of one is returned
    Given a join hearth "anvil"
    And delivery rows:
      | id | at                   | conversation_hash | guidance_kind |
      | d1 | 2026-08-10T09:00:00Z | c1                | track         |
      | d2 | 2026-08-10T09:10:00Z | c2                | track         |
      | d3 | 2026-08-11T09:20:00Z | c3                | proposal      |
    When the join episodes are folded
    And the episodes are grouped by "hearth_kind"
    Then the grouping yields 2 groups
    And the group "anvil/track" carries n 2
    And the group "anvil/proposal" carries n 1
    And every reported group carries its own n
    When the episodes are grouped by "hearth_period_week"
    Then the grouping yields 1 groups
    And the group "anvil/2026-08-10" carries n 3
    And every reported group carries its own n

  Scenario: Weekly grouping puts Sunday and the following Monday in different buckets
    Given a join hearth "anvil"
    And delivery rows:
      | id | at                   | conversation_hash | guidance_kind |
      | d1 | 2026-08-09T23:59:59Z | c1                | track         |
      | d2 | 2026-08-10T00:00:00Z | c2                | track         |
    When the join episodes are folded
    And the episodes are grouped by "hearth_period_week"
    Then the grouping yields 2 groups
    And the group "anvil/2026-08-03" carries n 1
    And the group "anvil/2026-08-10" carries n 1

  Scenario: The join relevance predicate retains begins and run-bearing transitions and drops route rows
    Given a join hearth "anvil"
    And activity rows:
      | command  | at                   | artifact_kind | from_state   | to_state     | conversation_hash | playbook_run_id |
      | route    | 2026-08-10T08:00:00Z | track         | -            | -            | c1                | -               |
      | begin    | 2026-08-10T09:00:00Z | track         | -            | implementing | c1                | run-a           |
      | route    | 2026-08-10T09:30:00Z | track         | -            | -            | c1                | run-a           |
      | complete | 2026-08-10T10:00:00Z | track         | implementing | completed    | -                 | run-a           |
    When the activity rows are filtered by is_join_relevant
    Then the retained activity rows carry commands "begin,complete" in order

  Scenario: An empty input folds to an empty set and a zeroed report with no error
    When the join episodes are folded
    Then the join set holds 0 episodes and 0 unmatched begins
    And the report exposes 0 per hearth entries
    And the report filter version is the "JOIN_FILTER_VERSION" constant

  Scenario: The same frozen input folded twice produces an identical vector and report
    Given a join hearth "anvil"
    And delivery rows:
      | id | at                   | conversation_hash | guidance_kind |
      | d1 | 2026-08-10T08:00:00Z | c1                | track         |
      | d2 | 2026-08-10T08:10:00Z | c2                | proposal      |
      | d3 | 2026-08-10T08:20:00Z | -                 | track         |
    And activity rows:
      | command | at                   | artifact_kind | to_state     | conversation_hash | playbook_run_id |
      | begin   | 2026-08-10T09:00:00Z | track         | implementing | c1                | run-a           |
      | begin   | 2026-08-10T09:10:00Z | proposal      | drafting     | c9                | run-b           |
    When the join episodes are folded
    And the join episodes are folded again
    Then the second fold produces an identical set and report

  Scenario: The report declares its window, its rows read, its rows scanned and retained, both denominators and the filter version
    Given join options window "2026-08-01T00:00:00Z" to "2026-09-01T00:00:00Z" key epoch "e0e0e0e0e0e0"
    And a join hearth "anvil"
    And the hearth read metadata is 2 read defects and 900 activity rows scanned
    And delivery rows:
      | id | at                   | conversation_hash | guidance_kind | outcome            | guidance_produced |
      | d1 | 2026-08-10T08:00:00Z | c1                | track         | guidance_produced  | true              |
      | d2 | 2026-08-10T08:10:00Z | c2                | -             | guidance_produced  | true              |
      | d3 | 2026-08-10T08:20:00Z | c3                | -             | engine_unreachable | false             |
      | d4 | 2026-08-10T08:30:00Z | c4                | -             | no_candidate       | false             |
    And activity rows:
      | command  | at                   | artifact_kind | from_state   | to_state     | conversation_hash | playbook_run_id |
      | begin    | 2026-08-10T09:00:00Z | track         | -            | implementing | c1                | run-a           |
      | complete | 2026-08-10T10:00:00Z | track         | implementing | completed    | -                 | run-a           |
    When the join episodes are folded
    Then the coverage for hearth "anvil" reports window "2026-08-01T00:00:00Z" to "2026-09-01T00:00:00Z"
    And the coverage for hearth "anvil" reports "delivery_rows_read" as 4
    And the coverage for hearth "anvil" reports "read_defects" as 2
    And the coverage for hearth "anvil" reports "activity_rows_scanned" as 900
    And the coverage for hearth "anvil" reports "activity_rows_retained" as 2
    And the coverage for hearth "anvil" reports "begin_rows_read" as 1
    And the coverage for hearth "anvil" reports "episode_denominator" as 1
    And the coverage for hearth "anvil" reports "begin_denominator" as 1
    And the coverage for hearth "anvil" reports "menu_delivered" as 1
    And the coverage for hearth "anvil" reports "no_engine_answer" as 1
    And the coverage for hearth "anvil" reports "nothing_delivered" as 1
    And the coverage for hearth "anvil" reports "joined" as 1
    And the report filter version is the "JOIN_FILTER_VERSION" constant

  Scenario: The serialized report exposes a per-hearth entry per input hearth and no pooled coverage key
    Given a join hearth "anvil"
    And delivery rows:
      | id | at                   | conversation_hash | guidance_kind |
      | d1 | 2026-08-10T08:00:00Z | c1                | track         |
    And a join hearth "foundry"
    And delivery rows:
      | id | at                   | conversation_hash | guidance_kind |
      | d2 | 2026-08-10T08:10:00Z | c2                | proposal      |
    When the join episodes are folded
    Then the report exposes 2 per hearth entries
    And the serialized report exposes no pooled coverage key

  Scenario: The per-hearth entries follow input order and the fold re-sorts nothing
    Given a join hearth "zulu"
    And delivery rows:
      | id | at                   | conversation_hash | guidance_kind |
      | d1 | 2026-08-10T09:00:00Z | c1                | track         |
    And a join hearth "alpha"
    And delivery rows:
      | id | at                   | conversation_hash | guidance_kind |
      | d2 | 2026-08-10T08:00:00Z | c2                | track         |
    When the join episodes are folded
    Then the per hearth entries are labelled "zulu,alpha"
    And the episodes are ordered "d1,d2"

  Scenario: The four key-epoch reconciliation verdicts are each produced by their own input
    Given join options window "" to "" key epoch "aaaaaaaaaaaa"
    And a join hearth "no-file"
    And a join hearth "matching" with salt file epoch "aaaaaaaaaaaa"
    And a join hearth "differing" with salt file epoch "bbbbbbbbbbbb"
    When the join episodes are folded
    Then the coverage for hearth "matching" reports "key_epoch" as text "aaaaaaaaaaaa"
    And the coverage for hearth "no-file" reports "key_epoch_reconciliation" as text "NoLocalEvidence"
    And the coverage for hearth "matching" reports "key_epoch_reconciliation" as text "MatchesEffective"
    And the coverage for hearth "differing" reports "key_epoch_reconciliation" as text "DiffersFromEffective"
    When the join episodes are folded with an unknown key epoch
    Then the coverage for hearth "no-file" reports "key_epoch_reconciliation" as text "NoEffectiveSalt"
    And the coverage for hearth "matching" reports "key_epoch_reconciliation" as text "NoEffectiveSalt"
    And the coverage for hearth "differing" reports "key_epoch_reconciliation" as text "NoEffectiveSalt"
