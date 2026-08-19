Feature: Change record ref compare-and-swap

  The in-process hearth mutex does not span processes, and the second leg of a
  two-repository transaction holds no guard at all. So serialization of the
  change-record lineage comes from git's OWN ref update: compare-and-swap
  against the tip that was observed when the parent was chosen. A writer that
  loses the race retries against the new tip. It never forces.

  The race here is DETERMINISTIC, not hoped for. A test-only rendezvous barrier
  — armed by exactly the crash point's three conditions — holds the first
  writer to reach the ref update between reading the tip and moving it, so the
  other writer's commit is guaranteed to land underneath it and the held
  writer's compare-and-swap is guaranteed to be stale. Nothing here orders the
  race with a sleep.

  Drop the observed-old argument from the ref update and the held writer
  overwrites the ref with a commit whose parent is the stale tip: the other
  writer's commit becomes unreachable and the first two scenarios go red. That
  is the mutation this file exists to catch.

  The house split is the one hearth locking already uses: the deterministic
  no-lost-update proof lives at the core seam, and the engine seam proves that
  both RPCs succeed.

  The seam is anvil-core and the user is the engine.

  Scenario: Two writers racing on one hearth's ref both land, and the second is a child of the first
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-record | spec  |
    And a file exists at hearth path "tracks/t-record/spec.md" with content "# Spec, revision 2"
    And a change-record transaction with:
      | key           | value                   |
      | operation_id  | cr-op-held              |
      | command       | complete                |
      | artifact_kind | track                   |
      | event_kinds   | StateChanged            |
      | path          | tracks/t-record/spec.md |
    And a second change-record transaction with:
      | key           | value                       |
      | operation_id  | cr-op-free                  |
      | command       | amend                       |
      | artifact_kind | track                       |
      | event_kinds   | Amended                     |
      | path          | tracks/t-record/status.yaml |
    When both change-record writers race for the ref
    Then both recorded commits are reachable from the change-record tip
    And the change-record ref has exactly 3 commit

  Scenario: A writer that loses the ref race retries against the new tip rather than forcing
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-record | spec  |
    And a file exists at hearth path "tracks/t-record/spec.md" with content "# Spec, revision 2"
    And a change-record transaction with:
      | key           | value                   |
      | operation_id  | cr-op-held              |
      | command       | complete                |
      | artifact_kind | track                   |
      | event_kinds   | StateChanged            |
      | path          | tracks/t-record/spec.md |
    And a second change-record transaction with:
      | key           | value                       |
      | operation_id  | cr-op-free                  |
      | command       | amend                       |
      | artifact_kind | track                       |
      | event_kinds   | Amended                     |
      | path          | tracks/t-record/status.yaml |
    When both change-record writers race for the ref
    Then the held writer's commit names the winning commit as its parent
    And the held writer's commit does not name the tip observed before the race as its parent

  # The retry rebuilds a tree and commits again, so without the idempotency key
  # the recovery path already has, one transaction would end up recorded twice.
  Scenario: A retry does not duplicate a commit for the same operation id
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-record | spec  |
    And a file exists at hearth path "tracks/t-record/spec.md" with content "# Spec, revision 2"
    And a change-record transaction with:
      | key           | value                   |
      | operation_id  | cr-op-shared            |
      | command       | complete                |
      | artifact_kind | track                   |
      | event_kinds   | StateChanged            |
      | path          | tracks/t-record/spec.md |
    And a second change-record transaction with:
      | key           | value                       |
      | operation_id  | cr-op-shared                |
      | command       | complete                    |
      | artifact_kind | track                       |
      | event_kinds   | StateChanged                |
      | path          | tracks/t-record/status.yaml |
    When both change-record writers race for the ref
    Then the change-record ref has exactly 2 commit
    And exactly 1 commit carries that operation id
    And the change-record journal holds 0 leg

  Scenario: Writers on two different hearths do not contend
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-record | spec  |
    And a file exists at hearth path "tracks/t-record/spec.md" with content "# Spec, revision 2"
    And a change-record transaction with:
      | key           | value                   |
      | operation_id  | cr-op-held              |
      | command       | complete                |
      | artifact_kind | track                   |
      | event_kinds   | StateChanged            |
      | path          | tracks/t-record/spec.md |
    And a second change-record hearth with its own recorded governance file
    When a writer held at the barrier on the first hearth races a writer on the second hearth
    Then the second hearth's writer landed while the first was still held
    And each hearth's change-record ref has exactly 2 commit
