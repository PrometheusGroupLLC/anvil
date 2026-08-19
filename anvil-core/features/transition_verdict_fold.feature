Feature: A reviewer's verdict survives the fold

  A reviewer who sends work back records that on the transition. The event file
  has carried the verdict since the carry-forward slice shipped — and the fold
  that every reader goes through DROPPED it, so no surface downstream could tell
  a run that was sent back from a run that was waved through. Nothing that reads
  an artifact's history could see who caught anything.

  This is the seam that makes "what it got wrong, and who caught it" a recorded
  fact rather than a drawing. The verdict and the actor who recorded it live on
  the SAME event; folding one without the other is what left the catch invisible.

  A transition nobody reviewed carries NO verdict. Not the empty string, not
  "none", not "satisfied" — a run whose author simply moved it forward was not
  approved by anybody, and saying so is different from inventing an approval.

  Scenario: A reviewer verdict written to the event store reaches the folded history
    Given an artifact directory with the transition events:
      | to          | at                   | actor | role     | satisfaction  |
      | spec        | 2026-01-01T09:00:00Z | fable | doer     |               |
      | spec_review | 2026-01-01T10:00:00Z | fable | doer     |               |
      | spec_revision | 2026-01-01T11:00:00Z | nick | reviewer | full_revision |
    When the artifact's history is folded from the event store
    Then the folded step to "spec_revision" carries the verdict "full_revision"
    And the folded step to "spec_revision" was recorded by "nick"

  # The empty-string placeholder is the failure this pins. A fold that wrote
  # Some("") for an unreviewed step would satisfy a check that only asks "is the
  # field there", and every unreviewed step in the product would then read as a
  # verdict nobody rendered.
  Scenario: A transition nobody reviewed carries no verdict at all
    Given an artifact directory with the transition events:
      | to          | at                   | actor | role | satisfaction |
      | spec        | 2026-01-01T09:00:00Z | fable | doer |              |
    When the artifact's history is folded from the event store
    Then the folded step to "spec" carries no verdict

  # The legacy status.yaml transitions array predates reviewer verdicts, so in
  # practice it has no such column and a row from it folds to no verdict.
  Scenario: A legacy status.yaml row with no verdict column carries no verdict
    Given an artifact whose legacy status.yaml lists the transitions:
      | to   | at                   | actor | role |
      | spec | 2026-01-01T09:00:00Z | fable | doer |
    When the artifact's history is folded from the event store
    Then the folded step to "spec" carries no verdict

  # AND WHEN A LEGACY ROW DOES CARRY ONE, IT IS CARRIED, NOT DROPPED. The
  # scenario above alone proves only that the deserializer defaults an absent key
  # — it is green against a fold that discards the legacy column outright. A fold
  # that dropped a recorded verdict to keep the shape tidy would be deleting
  # evidence to preserve a claim about the data.
  Scenario: A legacy status.yaml row that does carry a verdict keeps it
    Given an artifact whose legacy status.yaml lists the transitions with verdicts:
      | to            | at                   | actor | role     | satisfaction  |
      | spec_revision | 2026-01-01T09:00:00Z | nick  | reviewer | full_revision |
    When the artifact's history is folded from the event store
    Then the folded step to "spec_revision" carries the verdict "full_revision"
