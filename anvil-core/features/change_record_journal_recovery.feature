Feature: Change record journal and recovery

  The atomic unit is the pair (filesystem effects, commit). The engine can die
  between them, so every leg is journalled `prepared -> applying -> committed`
  before the ref moves, and a recovery pass rolls the unfinished ones forward.

  Recovery is EXACTLY-ONCE, and it is bought with an explicit operation id: a
  rolled-forward commit is byte-identical to the one an uninterrupted run would
  have written, so "exactly one commit for this transaction" is checkable by
  reading the lineage rather than by trusting a flag.

  Recovery NEVER rewinds disk. In shadow the on-disk state is the authority and
  git catches up to it; a divergence recovery cannot reconcile is REPORTED, in
  the refuse-rather-than-guess posture of the backlog journal's conflict.

  Reaching any of this needs an injected crash point, so the crash point is a
  requirement of the mechanism rather than a test convenience — and the last
  three scenarios are what make it unreachable in a shipped engine.

  The seam is anvil-core and the user is the engine.

  Scenario: A journal entry whose commit never landed is rolled forward on recovery
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-record | spec  |
    And a file exists at hearth path "tracks/t-record/spec.md" with content "# Spec, revision 2"
    And a change-record transaction with:
      | key           | value                    |
      | command       | complete                 |
      | artifact_kind | track                    |
      | event_kinds   | StateChanged             |
      | path          | tracks/t-record/spec.md  |
    When a change-record transaction is interrupted after "before_ref_update"
    Then the change-record ref has exactly 1 commit
    And the change-record journal holds 1 leg
    When change-record recovery is run for that hearth
    Then the change-record ref has exactly 2 commit
    And exactly 1 commit carries that operation id
    And the change-record journal holds 0 leg

  Scenario: The rolled-forward commit's tree matches the on-disk content that survived
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-record | spec  |
    And a file exists at hearth path "tracks/t-record/spec.md" with content "# Spec, revision 2"
    And a change-record transaction with:
      | key           | value                       |
      | command       | complete                    |
      | artifact_kind | track                       |
      | event_kinds   | StateChanged                |
      | path          | tracks/t-record/spec.md     |
      | path          | tracks/t-record/status.yaml |
    When a change-record transaction is interrupted after "before_ref_update"
    Then the change-record ref has exactly 1 commit
    When change-record recovery is run for that hearth
    Then the recorded commit tree matches the hearth content byte-for-byte

  Scenario: Recovery run twice produces exactly one commit for the interrupted transaction
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-record | spec  |
    And a file exists at hearth path "tracks/t-record/spec.md" with content "# Spec, revision 2"
    And a change-record transaction with:
      | key           | value                   |
      | command       | complete                |
      | artifact_kind | track                   |
      | event_kinds   | StateChanged            |
      | path          | tracks/t-record/spec.md |
    When a change-record transaction is interrupted after "before_ref_update"
    Then the change-record ref has exactly 1 commit
    When change-record recovery is run for that hearth
    And change-record recovery is run for that hearth
    Then the change-record ref has exactly 2 commit
    And exactly 1 commit carries that operation id

  Scenario: A journal entry whose commit is already present is cleared without a second commit
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-record | spec  |
    And a file exists at hearth path "tracks/t-record/spec.md" with content "# Spec, revision 2"
    And a change-record transaction with:
      | key           | value                   |
      | command       | complete                |
      | artifact_kind | track                   |
      | event_kinds   | StateChanged            |
      | path          | tracks/t-record/spec.md |
    When a change-record transaction is interrupted after "before_row"
    Then the change-record ref has exactly 2 commit
    And the change-record journal holds 1 leg
    When change-record recovery is run for that hearth
    Then the change-record ref has exactly 2 commit
    And exactly 1 commit carries that operation id
    And the change-record journal holds 0 leg

  # The one place the operation id is the SOLE guard. A crash between the ref
  # update and the committed phase leaves a leg still reading `applying` whose
  # commit has already landed, so "is it already there?" cannot be answered
  # from the phase file — only by reading the id back off the lineage. Clearing
  # the journal, which is what makes a second recovery pass harmless in every
  # other scenario, has not happened here.
  Scenario: A leg interrupted after its ref update is verified by operation id rather than committed twice
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-record | spec  |
    And a file exists at hearth path "tracks/t-record/spec.md" with content "# Spec, revision 2"
    And a change-record transaction with:
      | key           | value                   |
      | command       | complete                |
      | artifact_kind | track                   |
      | event_kinds   | StateChanged            |
      | path          | tracks/t-record/spec.md |
    When a change-record transaction is interrupted after "after_ref_update"
    Then the change-record ref has exactly 2 commit
    And the change-record journal holds 1 leg
    When change-record recovery is run for that hearth
    Then the change-record ref tip is unchanged
    And exactly 1 commit carries that operation id
    And the change-record journal holds 0 leg

  Scenario: Recovery never overwrites an on-disk file that differs from the journal's expectation, it reports
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-record | spec  |
    And a file exists at hearth path "tracks/t-record/spec.md" with content "# Spec, revision 2"
    And a change-record transaction with:
      | key           | value                   |
      | command       | complete                |
      | artifact_kind | track                   |
      | event_kinds   | StateChanged            |
      | path          | tracks/t-record/spec.md |
    When a change-record transaction is interrupted after "before_ref_update"
    And the hearth file "tracks/t-record/spec.md" is changed to "# Spec, revision 3, written by a human"
    And change-record recovery is run for that hearth
    Then change-record recovery reports a conflict naming hearth path "tracks/t-record/spec.md"
    And the file at hearth path "tracks/t-record/spec.md" still has content "# Spec, revision 3, written by a human"
    And the change-record ref has exactly 1 commit

  Scenario: Recovery on a hearth with no journal entries leaves the ref tip unchanged
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-record | spec  |
    And a file exists at hearth path "tracks/t-record/spec.md" with content "# Spec, revision 2"
    And a change-record transaction with:
      | key           | value                   |
      | command       | complete                |
      | artifact_kind | track                   |
      | event_kinds   | StateChanged            |
      | path          | tracks/t-record/spec.md |
    When a change-record transaction is recorded for that hearth
    Then the change-record journal holds 0 leg
    When change-record recovery is run for that hearth
    Then the change-record ref tip is unchanged
    And the change-record ref has exactly 2 commit

  Scenario: The injected crash point is inert unless test mode is explicitly set
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-record | spec  |
    And a file exists at hearth path "tracks/t-record/spec.md" with content "# Spec, revision 2"
    And a change-record transaction with:
      | key           | value                   |
      | command       | complete                |
      | artifact_kind | track                   |
      | event_kinds   | StateChanged            |
      | path          | tracks/t-record/spec.md |
    When a change-record transaction is interrupted after "before_ref_update" with test mode unset
    Then the change-record transaction completed without interruption
    And the change-record ref has exactly 2 commit
    And the change-record journal holds 0 leg

  Scenario: The injected crash point refuses a hearth outside the platform temporary directory
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-record | spec  |
    And a file exists at hearth path "tracks/t-record/spec.md" with content "# Spec, revision 2"
    And a change-record transaction with:
      | key           | value                   |
      | command       | complete                |
      | artifact_kind | track                   |
      | event_kinds   | StateChanged            |
      | path          | tracks/t-record/spec.md |
    And the hearth is copied outside the platform temporary directory
    When a change-record transaction is interrupted after "before_ref_update"
    Then the change-record transaction refuses naming "only honored for a temporary hearth"
    And the change-record ref has exactly 1 commit

  Scenario: A crash token outside the closed grammar is refused
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-record | spec  |
    And a file exists at hearth path "tracks/t-record/spec.md" with content "# Spec, revision 2"
    And a change-record transaction with:
      | key           | value                   |
      | command       | complete                |
      | artifact_kind | track                   |
      | event_kinds   | StateChanged            |
      | path          | tracks/t-record/spec.md |
    When a change-record transaction is interrupted after "after_the_row_was_appended"
    Then the change-record transaction refuses naming "after_the_row_was_appended"
    And the change-record ref has exactly 1 commit
