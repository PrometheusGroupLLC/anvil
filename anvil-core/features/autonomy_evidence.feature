Feature: The case for a rung, folded from what was actually recorded

  A playbook's autonomy is supposed to be earned in public: runs clean out of
  runs attempted, the human-touch delta, and what it got wrong including which
  run, which week, and who caught it.

  THIS FOLD DOES NOT DEFINE CLEAN. Three folds in this crate already answer that
  question — the fidelity fold counts a run's revision cycles, the two-by-two's
  one-shot axis asks whether the run ever bounced back, and the survivor fold
  takes begun as the denominator. A fourth opinion is how two surfaces come to
  disagree about the same run while both look right. Clean is taken here, not
  invented: reached the end, and was never sent back.

  WHAT IS ADDED IS ATTRIBUTION. A count of revision cycles never says which
  moment, which step or who caught it, and the ladder's fourth tile asks for all
  three. When the count knows about more corrections than the history can name,
  the remainder is REPORTED — a fold that returned the shorter list quietly would
  make a missing history read as a clean stretch.

  THE HANDS ARE COUNTED OFF THE APPROVER, because that is the only human
  participation this engine records. Measured in the business hearth: the
  approver appears on 80 of the 249 transition events written since 2026-06-15,
  against 0 of 50 tracks carrying a `type: human` actor at all. A fold resting on
  the actors table alone would report no human touches on every run that exists.

  COST IS ABSENT AND STAYS ABSENT. The last scenario is structural and two-sided:
  a record that serialized nothing at all would pass a check that only looks for
  a forbidden key.

  # ── clean, taken from the count the fidelity fold already produced ────────

  Scenario: A run that was sent back is not clean
    Given a completed run "20260101T0900_a" sent back 1 time, with the steps:
      | to_state      | at                   | actor | role     | satisfaction  |
      | spec          | 2026-01-01T09:00:00Z | fable | doer     |               |
      | spec_review   | 2026-01-01T10:00:00Z | fable | doer     |               |
      | spec_revision | 2026-01-05T11:00:00Z | nick  | reviewer | full_revision |
    When the case is folded
    Then the run "20260101T0900_a" is not clean

  Scenario: The wrong action names the run it happened in
    Given a completed run "20260101T0900_a" sent back 1 time, with the steps:
      | to_state      | at                   | actor | role     | satisfaction  |
      | spec          | 2026-01-01T09:00:00Z | fable | doer     |               |
      | spec_review   | 2026-01-01T10:00:00Z | fable | doer     |               |
      | spec_revision | 2026-01-05T11:00:00Z | nick  | reviewer | full_revision |
    When the case is folded
    Then the case names 1 wrong action, and it happened in run "20260101T0900_a"

  Scenario: The wrong action names the week it happened in
    Given a completed run "20260101T0900_a" sent back 1 time, with the steps:
      | to_state      | at                   | actor | role     | satisfaction  |
      | spec          | 2026-01-01T09:00:00Z | fable | doer     |               |
      | spec_review   | 2026-01-01T10:00:00Z | fable | doer     |               |
      | spec_revision | 2026-01-05T11:00:00Z | nick  | reviewer | full_revision |
    When the case is folded
    Then the wrong action was caught at "2026-01-05T11:00:00Z"

  Scenario: The wrong action names who caught it and what they caught
    Given a completed run "20260101T0900_a" sent back 1 time, with the steps:
      | to_state      | at                   | actor | role     | satisfaction  |
      | spec          | 2026-01-01T09:00:00Z | fable | doer     |               |
      | spec_review   | 2026-01-01T10:00:00Z | fable | doer     |               |
      | spec_revision | 2026-01-05T11:00:00Z | nick  | reviewer | full_revision |
    When the case is folded
    Then the wrong action was caught by "nick" at the step "spec_review"

  Scenario: A run that reached the end and was never sent back is clean
    Given a completed run "20260101T0900_a" sent back 0 times, with the steps:
      | to_state    | at                   | actor | role     | satisfaction |
      | spec        | 2026-01-01T09:00:00Z | fable | doer     |              |
      | spec_review | 2026-01-01T10:00:00Z | fable | doer     |              |
      | completed   | 2026-01-01T11:00:00Z | nick  | reviewer | satisfied    |
    When the case is folded
    Then the run "20260101T0900_a" is clean
    And the case names 0 wrong actions

  # THE SEED CARRIES SOMETHING CORRECTION-SHAPED ON PURPOSE. A run still in
  # flight that had nothing verdict-like in it would leave this scenario green
  # against an implementation that never emits a correction at all. This one
  # carries a recorded `satisfied` verdict and a reviewer, so the arm bites: an
  # implementation that read any verdict as a correction goes red here.
  Scenario: A run still in flight is not clean, and its reviewed step is not a wrong action
    Given an unfinished run "20260101T0900_a" sent back 0 times, with the steps:
      | to_state    | at                   | actor | role     | satisfaction |
      | spec        | 2026-01-01T09:00:00Z | fable | doer     |              |
      | spec_review | 2026-01-01T10:00:00Z | nick  | reviewer | satisfied    |
    When the case is folded
    Then the run "20260101T0900_a" is not clean
    And the case names 0 wrong actions

  # THE COUNT AND THE HISTORY CAN DISAGREE, AND THE DIFFERENCE IS REPORTED. The
  # fidelity fold reads the activity log; the attribution reads the artifact's
  # own transitions. A hearth whose activity log outlived a pruned transition
  # store gives exactly this shape, and the run must not read as having been
  # sent back once when it was sent back twice.
  Scenario: Corrections the history cannot name are reported, not dropped
    Given a completed run "20260101T0900_a" sent back 2 times, with the steps:
      | to_state      | at                   | actor | role     | satisfaction  |
      | spec          | 2026-01-01T09:00:00Z | fable | doer     |               |
      | spec_revision | 2026-01-05T11:00:00Z | nick  | reviewer | full_revision |
    When the case is folded
    Then the run "20260101T0900_a" names 1 wrong action and 1 it could not name

  # ── the hands ─────────────────────────────────────────────────────────────

  Scenario: A step a person approved is a human touch
    Given a completed run "20260101T0900_a" sent back 0 times, with the steps:
      | to_state  | at                   | actor | role | approver | satisfaction |
      | spec      | 2026-01-01T09:00:00Z | fable | doer |          |              |
      | completed | 2026-01-01T10:00:00Z | fable | doer | nick     |              |
    When the case is folded
    Then the run "20260101T0900_a" records 1 human touch

  # The dormant route, kept because its dormancy is a defect to fix rather than a
  # shape to design around: the engine's own begin/snapshot/complete all demand a
  # model and a provider a person does not have, so no track in the business
  # hearth carries a `type: human` actor today. If that is fixed, this arm starts
  # contributing without anything else changing.
  Scenario: A step taken by an actor the run types as a person is a human touch
    Given a completed run "20260101T0900_a" sent back 0 times, with the steps:
      | to_state  | at                   | actor | role | satisfaction |
      | spec      | 2026-01-01T09:00:00Z | fable | doer |              |
      | completed | 2026-01-01T10:00:00Z | nick  | doer |              |
    And the run "20260101T0900_a" lists the actors:
      | name  | type  |
      | fable | agent |
      | nick  | human |
    When the case is folded
    Then the run "20260101T0900_a" records 1 human touch

  # SUM, NOT UNION, AND THE UNIT IS A PERSON'S INVOLVEMENT RATHER THAN A STEP.
  # One step where a person did the work AND a person allowed it involved two
  # people, and this counts two. Nothing pinned that before, so collapsing the
  # two contributions into an `||` left the suite green. It is the contestable
  # choice: a reader who thinks the unit is the step should change this scenario,
  # because under that reading the answer is 1.
  Scenario: A step a person took and a person approved counts twice
    Given a completed run "20260101T0900_a" sent back 0 times, with the steps:
      | to_state  | at                   | actor | role | approver | satisfaction |
      | completed | 2026-01-01T10:00:00Z | nick  | doer | maya     |              |
    And the run "20260101T0900_a" lists the actors:
      | name | type  |
      | nick | human |
      | maya | human |
    When the case is folded
    Then the run "20260101T0900_a" records 2 human touches

  # ZERO IS A REAL ANSWER AND IT IS NOT ABSENCE. This run's steps were examined
  # and none was a person's; the next scenario's run had no steps to examine at
  # all. A check written against zero would collapse the two.
  Scenario: A run nobody touched records zero touches, which is a count
    Given a completed run "20260101T0900_a" sent back 0 times, with the steps:
      | to_state  | at                   | actor | role | satisfaction |
      | spec      | 2026-01-01T09:00:00Z | fable | doer |              |
      | completed | 2026-01-01T10:00:00Z | fable | doer |              |
    When the case is folded
    Then the run "20260101T0900_a" records 0 human touches

  Scenario: A run that has taken no step records no touch count at all
    Given a completed run "20260101T0900_a" sent back 0 times, with no steps
    When the case is folded
    Then the run "20260101T0900_a" records no human touch count

  # THE DELTA IS THE FACT, NOT THE LEVEL. The card says "2 min, down from 14".
  # This engine records no minutes, so the delta is in touches — and it is
  # SIGNED, so a playbook that got worse says so rather than reporting a
  # magnitude that reads like an improvement either way.
  Scenario: The human-touch delta is the newest run against the oldest, signed
    Given a completed run "20260101T0900_a" sent back 0 times, with the steps:
      | to_state  | at                   | actor | role | approver | satisfaction |
      | spec      | 2026-01-01T09:00:00Z | fable | doer | nick     |              |
      | plan      | 2026-01-01T10:00:00Z | fable | doer | nick     |              |
      | completed | 2026-01-01T11:00:00Z | fable | doer | nick     |              |
    And a completed run "20260201T0900_b" sent back 0 times, with the steps:
      | to_state  | at                   | actor | role | approver | satisfaction |
      | spec      | 2026-02-01T09:00:00Z | fable | doer |          |              |
      | completed | 2026-02-01T10:00:00Z | fable | doer | nick     |              |
    When the case is folded
    Then the case reports a human-touch delta of -2

  Scenario: A delta needs both ends and is absent when either is missing
    Given a completed run "20260101T0900_a" sent back 0 times, with no steps
    And a completed run "20260201T0900_b" sent back 0 times, with the steps:
      | to_state  | at                   | actor | role | approver | satisfaction |
      | completed | 2026-02-01T10:00:00Z | fable | doer | nick     |              |
    When the case is folded
    Then the case reports no human-touch delta

  # ── the two populations ───────────────────────────────────────────────────

  # BOTH SIDES PRINTED, BOTH REQUIRED NON-ZERO. A one-sided check ("clean is at
  # least 1") passes on a playbook with one run, and the ratio is the fact. The
  # step refuses a zero on either side, so a fold that reported nothing clean on
  # a seed containing clean runs goes red instead of quietly passing.
  Scenario: The case reports both how many ran and how many were clean, both non-zero
    Given a completed run "20260101T0900_a" sent back 0 times, with the steps:
      | to_state  | at                   | actor | role | satisfaction |
      | completed | 2026-01-01T09:00:00Z | fable | doer |              |
    And a completed run "20260102T0900_b" sent back 1 time, with the steps:
      | to_state      | at                   | actor | role     | satisfaction  |
      | spec          | 2026-01-02T09:00:00Z | fable | doer     |               |
      | spec_revision | 2026-01-02T10:00:00Z | nick  | reviewer | full_revision |
    And a completed run "20260103T0900_c" sent back 0 times, with the steps:
      | to_state  | at                   | actor | role | satisfaction |
      | completed | 2026-01-03T09:00:00Z | fable | doer |              |
    When the case is folded
    Then the case reports 2 clean of 3 attempted

  # A playbook nobody has run has no case. Reporting "0 of 0" would render as a
  # measured perfect failure rather than as the absence of any evidence, and the
  # ladder's whole first beat is that the evidence must exist BEFORE the act.
  Scenario: A playbook with no runs at all has no case, not an empty one
    Given no runs at all
    When the case is folded
    Then there is no case to make

  # ── the count a demotion threshold would be applied to ────────────────────

  # THE NUMBER IS NOT CHOSEN HERE. How many bad runs cost a playbook its rung is
  # the owner's ruling and appears in no decision record; this fold measures the
  # quantity that ruling would be applied to and nothing more.
  Scenario: The run of bad runs is counted back from the newest and stops at the first clean one
    Given a completed run "20260101T0900_a" sent back 1 time, with the steps:
      | to_state      | at                   | actor | role     | satisfaction  |
      | spec_revision | 2026-01-01T09:00:00Z | nick  | reviewer | full_revision |
    And a completed run "20260102T0900_b" sent back 0 times, with the steps:
      | to_state  | at                   | actor | role | satisfaction |
      | completed | 2026-01-02T09:00:00Z | fable | doer |              |
    And a completed run "20260103T0900_c" sent back 1 time, with the steps:
      | to_state      | at                   | actor | role     | satisfaction  |
      | spec_revision | 2026-01-03T09:00:00Z | nick  | reviewer | full_revision |
    And a completed run "20260104T0900_d" sent back 1 time, with the steps:
      | to_state      | at                   | actor | role     | satisfaction  |
      | spec_revision | 2026-01-04T09:00:00Z | nick  | reviewer | full_revision |
    When the case is folded
    Then the case reports 2 bad runs in a row at the newest end

  # A REVIEWER CANNOT HAVE CAUGHT THEIR OWN STEP. R4's stated core rule, and
  # until this scenario nothing exercised it: every earlier seed already had a
  # different actor on the preceding step, so dropping the rule entirely left the
  # suite green. Here the catcher took the step immediately before as well, so
  # the fold has to walk PAST it to the last step somebody else took.
  Scenario: The wrong action skips past the catcher's own earlier step
    Given a completed run "20260101T0900_a" sent back 1 time, with the steps:
      | to_state      | at                   | actor | role     | satisfaction  |
      | spec          | 2026-01-01T09:00:00Z | fable | doer     |               |
      | plan          | 2026-01-01T10:00:00Z | nick  | doer     |               |
      | spec_revision | 2026-01-01T11:00:00Z | nick  | reviewer | full_revision |
    When the case is folded
    Then the wrong action was caught by "nick" at the step "spec"

  # THE CORRECTION'S OTHER FIELDS, ASSERTED. The revision state it landed in, the
  # verdict recorded alongside it, and who took the step it overturned were all
  # populated and read by nothing, so setting any of them to the empty string
  # left the suite green.
  Scenario: The wrong action names the state it landed in, the verdict, and whose step it was
    Given a completed run "20260101T0900_a" sent back 1 time, with the steps:
      | to_state      | at                   | actor | role     | satisfaction  |
      | spec          | 2026-01-01T09:00:00Z | fable | doer     |               |
      | spec_revision | 2026-01-01T11:00:00Z | nick  | reviewer | full_revision |
    When the case is folded
    Then the wrong action landed in "spec_revision" with the verdict "full_revision", overturning a step by "fable"

  # A machine can route work back without a reviewer recording a verdict on the
  # same step. The correction is still real; the verdict is EMPTY, and empty is
  # a surface's cue to render nothing rather than a claim that somebody said
  # nothing.
  Scenario: A correction with no recorded verdict carries no verdict
    Given a completed run "20260101T0900_a" sent back 1 time, with the steps:
      | to_state      | at                   | actor | role     | satisfaction |
      | spec          | 2026-01-01T09:00:00Z | fable | doer     |              |
      | spec_revision | 2026-01-01T11:00:00Z | nick  | reviewer |              |
    When the case is folded
    Then the wrong action landed in "spec_revision" with the verdict "", overturning a step by "fable"

  # TWO RUNS, TWO CORRECTIONS, EACH NAMING ITS OWN. With one correction in the
  # whole case, "names a specific run" is pinned against emptiness and not
  # against mis-attribution: writing every correction's run id from the first run
  # in the list passed.
  Scenario: Each wrong action names the run it happened in, not the first one
    Given a completed run "20260101T0900_a" sent back 1 time, with the steps:
      | to_state      | at                   | actor | role     | satisfaction  |
      | spec_revision | 2026-01-01T09:00:00Z | nick  | reviewer | full_revision |
    And a completed run "20260202T0900_b" sent back 1 time, with the steps:
      | to_state      | at                   | actor | role     | satisfaction  |
      | plan_revision | 2026-02-02T09:00:00Z | maya  | reviewer | full_revision |
    When the case is folded
    Then the wrong action caught by "maya" happened in run "20260202T0900_b"

  # THE ORDER IS WHEN THE WORK BEGAN, NOT WHAT THE ID SORTS AS. Ids are minted at
  # creation and can be minted in a different order than the work starts in. The
  # two runs below sort the OPPOSITE way to their first steps, deliberately:
  # every other seed had id order and time order agreeing, so deleting the sort
  # entirely left the suite green — and the newest-end count and both delta
  # endpoints depend on it completely.
  Scenario: The newest run is the one whose work began last, not the one whose id sorts last
    Given a completed run "20260301T0900_zulu" sent back 0 times, with the steps:
      | to_state  | at                   | actor | role | approver | satisfaction |
      | completed | 2026-01-01T09:00:00Z | fable | doer | nick     |              |
    And a completed run "20260401T0900_alpha" sent back 1 time, with the steps:
      | to_state      | at                   | actor | role     | approver | satisfaction  |
      | spec_revision | 2025-12-01T09:00:00Z | nick  | reviewer |          | full_revision |
    When the case is folded
    Then the case reports 0 bad runs in a row at the newest end

  # THE MIRROR IMAGE, WHICH A SATURATING SUBTRACTION SWALLOWS. The declared count
  # and the attributed history come from DIFFERENT stores — the activity log and
  # the artifact's own event files — so either can be the shorter one. Reporting
  # only the count-knows-more direction leaves a run reading `clean` while
  # carrying a non-empty list of what went wrong.
  Scenario: Corrections the count does not know about are reported too
    Given a completed run "20260101T0900_a" sent back 0 times, with the steps:
      | to_state      | at                   | actor | role     | satisfaction  |
      | spec          | 2026-01-01T09:00:00Z | fable | doer     |               |
      | spec_revision | 2026-01-05T11:00:00Z | nick  | reviewer | full_revision |
    When the case is folded
    Then the run "20260101T0900_a" names 1 wrong action the count does not know about, and the two counts disagree

  # ── the score the redacted sink carries ───────────────────────────────────

  # THE SCORE IS A NUMBER AND NOTHING ELSE. The measurement sink states it never
  # carries raw actor identity, so the catcher's name stops at the read fold. A
  # grader that returned a constant would pass a check that only pins the unclean
  # value, so both values are pinned.
  Scenario: A clean run scores one
    Given a run that reached the end and was sent back 0 times
    Then its cleanliness score is 1

  Scenario: A run that was sent back scores zero
    Given a run that reached the end and was sent back 1 time
    Then its cleanliness score is 0

  Scenario: A run that never reached the end scores zero
    Given a run that did not reach the end and was sent back 0 times
    Then its cleanliness score is 0

  # ── the refusal ───────────────────────────────────────────────────────────

  # TWO-SIDED ON PURPOSE. A record that serialized nothing at all carries no
  # cost-shaped key either, and the one-sided version of this check is green on
  # an empty record — the exact vacuity a sibling guard already learned to close.
  Scenario: No part of the case carries a cost figure, and the case is not empty
    Given a completed run "20260101T0900_a" sent back 1 time, with the steps:
      | to_state      | at                   | actor | role     | approver | satisfaction  |
      | spec          | 2026-01-01T09:00:00Z | fable | doer     |          |               |
      | spec_revision | 2026-01-05T10:00:00Z | nick  | reviewer | nick     | full_revision |
    And the run "20260101T0900_a" lists the actors:
      | name  | type  |
      | fable | agent |
    When the case is folded
    Then no part of the case carries a cost figure, and the case is not empty
