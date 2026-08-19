Feature: A run's record is shaped once, and carries no cost

  The engine reads a run's artifacts off disk; this fold decides what the record
  LOOKS like. Both surfaces that serve it — the gRPC RunDetail RPC and the /ws
  run_detail method — go through this one function, so the shape pinned here is
  the shape both of them return.

  THE FOLD RE-DERIVES NOTHING. The ordered history is already resolved, once, by
  the transition-log seam over the per-file event store. This fold receives that
  history and only shapes it. Every scenario below therefore states its input as
  an ALREADY-FOLDED list of steps: if this feature seeded event files and folded
  them a second time, it would be asserting against the second opinion rather
  than against the first.

  WHAT A STEP COST IS RECORDED NOWHERE IN THIS ENGINE. Not on the artifact, not
  on a transition event, not in the measurement sink. The last scenario is
  structural rather than prose: it serializes a whole folded record and fails on
  any cost-shaped key anywhere in it. A `cost: 0.0` is not a missing number —
  it is a fabricated one, and it reads to a person as a step that was free.

  # ── ordering ──────────────────────────────────────────────────────────────
  # The record's order is the order the work was STARTED in, which is the first
  # step each nested run took — NOT the ids, which are minted at creation and
  # can be minted in a different order than the work begins in.
  #
  # THE TWO IDS BELOW SORT THE OPPOSITE WAY TO THEIR FIRST STEPS, DELIBERATELY.
  # The first version of this scenario named them `..1200_early` (12:00) and
  # `..2000_late` (20:00), so id order and time order agreed and an
  # implementation that sorted nested runs by INSTANCE ID passed it. Measured:
  # replacing the by-time key with `sort_by_key(instance_id)` left this scenario
  # GREEN — the ordering rule it is named for, unfalsifiable in the scenario that
  # exists to pin it. It reds now.
  Scenario: The runs started from inside a run come back oldest-first
    Given a run "20260101T0900_root" that has taken steps:
      | to_state     | at                   | actor | role   | approver | note        |
      | spec         | 2026-01-01T09:00:00Z | fable | doer   |          | opened      |
      | spec_review  | 2026-01-01T10:00:00Z | opus  | doer   |          |             |
      | implementing | 2026-01-01T11:00:00Z | opus  | review | nick     | approved it |
    And a run "20260101T1200_zulu" was started from inside it with steps:
      | to_state | at                   | actor | role | approver | note |
      | spec     | 2026-01-01T20:00:00Z | opus  | doer |          |      |
    And a run "20260101T2000_alpha" was started from inside it with steps:
      | to_state | at                   | actor | role | approver | note |
      | spec     | 2026-01-01T12:00:00Z | opus  | doer |          |      |
    When the run's record is folded
    Then the record lists the runs in the order "20260101T0900_root,20260101T2000_alpha,20260101T1200_zulu"

  # Two runs started in the same recorded moment is ordinary, not exotic: the
  # timestamps are second-granular. Without the id tiebreak the record reorders
  # itself between two reads of an unchanged hearth.
  Scenario: Two nested runs started in the same moment are ordered by name
    Given a run "20260101T0900_root" that has taken steps:
      | to_state | at                   | actor | role | approver | note |
      | spec     | 2026-01-01T09:00:00Z | fable | doer |          |      |
    And a run "20260101T1200_zulu" was started from inside it with steps:
      | to_state | at                   | actor | role | approver | note |
      | spec     | 2026-01-01T12:00:00Z | opus  | doer |          |      |
    And a run "20260101T1200_alpha" was started from inside it with steps:
      | to_state | at                   | actor | role | approver | note |
      | spec     | 2026-01-01T12:00:00Z | opus  | doer |          |      |
    When the run's record is folded
    Then the record lists the runs in the order "20260101T0900_root,20260101T1200_alpha,20260101T1200_zulu"

  # ── nesting ───────────────────────────────────────────────────────────────
  Scenario: The run that was asked for is first and is nested under nothing
    Given a run "20260101T0900_root" that has taken steps:
      | to_state | at                   | actor | role | approver | note |
      | spec     | 2026-01-01T09:00:00Z | fable | doer |          |      |
    And a run "20260101T1200_child" was started from inside it with steps:
      | to_state | at                   | actor | role | approver | note |
      | spec     | 2026-01-01T12:00:00Z | opus  | doer |          |      |
    When the run's record is folded
    Then the run "20260101T0900_root" is at depth 0 and is nested under nothing
    And the run "20260101T1200_child" is at depth 1 nested under "20260101T0900_root"
    And the run "20260101T1200_child" has steps to states "spec"

  Scenario: A run nothing was started from inside comes back alone
    Given a run "20260101T0900_root" that has taken steps:
      | to_state | at                   | actor | role | approver | note |
      | spec     | 2026-01-01T09:00:00Z | fable | doer |          |      |
    When the run's record is folded
    Then the record lists the runs in the order "20260101T0900_root"

  # ── the steps themselves ──────────────────────────────────────────────────
  Scenario: The steps stay in the order they happened, and say who took them
    Given a run "20260101T0900_root" that has taken steps:
      | to_state     | at                   | actor | role   | approver | note        |
      | spec         | 2026-01-01T09:00:00Z | fable | doer   |          | opened      |
      | spec_review  | 2026-01-01T10:00:00Z | opus  | doer   |          |             |
      | implementing | 2026-01-01T11:00:00Z | opus  | review | nick     | approved it |
    When the run's record is folded
    Then the run "20260101T0900_root" has steps to states "spec,spec_review,implementing"
    And the step to "implementing" in run "20260101T0900_root" was taken by "opus" playing "review" at "2026-01-01T11:00:00Z"
    And the step to "implementing" in run "20260101T0900_root" names approver "nick"

  # A step nobody had to approve must SAY nothing, not say something empty-ish.
  # A placeholder here ("n/a", "system", "unknown") is a claim about who signed
  # off, rendered in the same place a real approver's name is rendered.
  Scenario: A step that needed no approval names nobody rather than something
    Given a run "20260101T0900_root" that has taken steps:
      | to_state | at                   | actor | role | approver | note |
      | spec     | 2026-01-01T09:00:00Z | fable | doer |          |      |
    When the run's record is folded
    Then the step to "spec" in run "20260101T0900_root" carries no approver
    And the step to "spec" in run "20260101T0900_root" carries no note

  # A step the record carries no time or actor for is the shape a hand-created
  # or legacy artifact takes. It must come back empty in those places rather
  # than inventing a time, which would put a fabricated moment on a timeline.
  Scenario: A step the record carries no time or actor for says so
    Given a run "20260101T0900_root" that has taken steps:
      | to_state | at | actor | role | approver | note |
      | spec     |    |       |      |          |      |
    When the run's record is folded
    Then the step to "spec" in run "20260101T0900_root" was taken by "" playing "" at ""

  # ── actors ────────────────────────────────────────────────────────────────
  # The table APPENDS a configuration whenever an agent returns under a
  # different one, so the current model is the last entry — reading the first
  # would report the model the run started on as the model it is on now.
  Scenario: An actor's model and provider come from its most recent configuration
    Given a run "20260101T0900_root" that has taken steps:
      | to_state | at                   | actor | role | approver | note |
      | spec     | 2026-01-01T09:00:00Z | fable | doer |          |      |
    And the run "20260101T0900_root" carries an agent actor "fable" configured:
      | at                   | model          | provider  |
      | 2026-01-01T09:00:00Z | claude-sonnet  | anthropic |
      | 2026-01-01T11:00:00Z | claude-opus-5  | anthropic |
    When the run's record is folded
    Then the actor "fable" in run "20260101T0900_root" is an "agent" on model "claude-opus-5" from "anthropic"

  Scenario: An actor the table carries no configuration for reports no model
    Given a run "20260101T0900_root" that has taken steps:
      | to_state | at                   | actor | role | approver | note |
      | spec     | 2026-01-01T09:00:00Z | nick  | doer |          |      |
    And the run "20260101T0900_root" carries a human actor "nick"
    When the run's record is folded
    Then the actor "nick" in run "20260101T0900_root" is a "human" on model "" from ""

  # The `actors:` table is a MAP. Iterating it directly gives a different order
  # on every read, so an unchanged hearth would appear to reshuffle its people.
  Scenario: A run's actors come back in a stable order
    Given a run "20260101T0900_root" that has taken steps:
      | to_state | at                   | actor | role | approver | note |
      | spec     | 2026-01-01T09:00:00Z | fable | doer |          |      |
    And the run "20260101T0900_root" carries a human actor "nick"
    And the run "20260101T0900_root" carries a human actor "amelia"
    And the run "20260101T0900_root" carries a human actor "zeke"
    When the run's record is folded
    Then the run "20260101T0900_root" lists actors "amelia,nick,zeke"

  # ── the reviewer's verdict, on the record every surface reads ─────────────
  #
  # "What it got wrong, and WHO caught it" needs the verdict and the actor on the
  # same row. The event store has carried both since the carry-forward slice and
  # the fold dropped the verdict, so this record — the one every surface reads a
  # run's history from — could name the actor and never say what they decided.
  Scenario: A step's reviewer verdict is on the run's record beside the actor who recorded it
    Given a run "20260101T0900_root" that has taken steps with verdicts:
      | to_state      | at                   | actor | role     | approver | note | satisfaction  |
      | spec          | 2026-01-01T09:00:00Z | fable | doer     |          |      |               |
      | spec_revision | 2026-01-01T11:00:00Z | nick  | reviewer |          |      | full_revision |
    When the run's record is folded
    Then the step to "spec_revision" in run "20260101T0900_root" carries the verdict "full_revision"

  # EMPTY, not "none" and not "satisfied". Most steps carry no verdict, and an
  # invented one would read to a person as an approval nobody rendered.
  Scenario: A step nobody reviewed carries an empty verdict
    Given a run "20260101T0900_root" that has taken steps with verdicts:
      | to_state | at                   | actor | role | approver | note | satisfaction |
      | spec     | 2026-01-01T09:00:00Z | fable | doer |          |      |              |
    When the run's record is folded
    Then the step to "spec" in run "20260101T0900_root" carries the verdict ""

  # ── the absence that this track exists to protect ─────────────────────────
  Scenario: No part of the record carries a cost figure
    Given a run "20260101T0900_root" that has taken steps:
      | to_state     | at                   | actor | role   | approver | note        |
      | spec         | 2026-01-01T09:00:00Z | fable | doer   |          | opened      |
      | implementing | 2026-01-01T11:00:00Z | opus  | review | nick     | approved it |
    And a run "20260101T1200_child" was started from inside it with steps:
      | to_state | at                   | actor | role | approver | note |
      | spec     | 2026-01-01T12:00:00Z | opus  | doer |          |      |
    And the run "20260101T0900_root" carries an agent actor "fable" configured:
      | at                   | model         | provider  |
      | 2026-01-01T09:00:00Z | claude-opus-5 | anthropic |
    When the run's record is folded
    Then no part of the record carries a cost figure
    And the record was not empty
