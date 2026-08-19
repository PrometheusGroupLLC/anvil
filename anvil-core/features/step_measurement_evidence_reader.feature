Feature: Evidence-bearing step measurements survive durable storage
  Evidence assessments are readable as the same typed values that were written,
  while pre-evidence rows remain readable and partial extensions are rejected.

  Background:
    Given a step measurement evidence-reader hearth

  Scenario: An opaque evidence reference preserves every JSON C0 control character
    Given an incomplete evidence record with one missing artifact class and one citation claim containing every JSON C0 control character
    When the evidence-bearing step measurement is appended and read
    Then the typed step measurement read succeeds
    And the evidence extension round-trips exactly
    And the opaque reference preserves every JSON C0 control character

  Scenario: A legacy row without evidence remains readable
    Given a legacy step measurement row without evidence and with a form-feed in its project label
    When the durable step measurements are read
    Then the typed step measurement read succeeds
    And the legacy record has no evidence extension
    And the legacy project label is preserved exactly

  Scenario: An evidence-neutral base field preserves every JSON C0 control character
    Given an evidence-neutral step measurement whose project label contains every JSON C0 control character
    When the evidence-neutral step measurement is appended and read
    Then the typed step measurement read succeeds
    And the evidence-neutral record has no evidence extension
    And the project label preserves every JSON C0 control character

  Scenario: A partial evidence extension is rejected
    Given a step measurement row carrying only an evidence status
    When the durable step measurements are read
    Then the typed step measurement read is rejected as a partial evidence extension

  Scenario: A first-position partial evidence extension is rejected
    Given a step measurement row whose sole evidence key is first in the object
    When the durable step measurements are read
    Then the typed step measurement read is rejected as a partial evidence extension

  Scenario: A whitespace-formatted partial evidence extension is rejected
    Given a whitespace-formatted step measurement row carrying only an evidence status
    When the durable step measurements are read
    Then the typed step measurement read is rejected as a partial evidence extension
