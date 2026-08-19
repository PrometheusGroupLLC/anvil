Feature: Claimed evidence is assessed against a step obligation
  The engine can distinguish complete, partial, and absent evidence claims
  without duplicating the playbook's evidence-strength rules.

  Scenario: Every obligated class is present as claimed
    Given an evidence obligation of "artifact_of_consequence, verifiable_citation"
    And claimed evidence classes of "artifact_of_consequence, verifiable_citation"
    When the evidence obligation is assessed
    Then the evidence assessment status is "present-as-claimed"
    And the evidence assessment names no missing classes

  Scenario: A partial claim names each missing class once in declaration order
    Given an evidence obligation of "artifact_of_consequence, verifiable_citation, artifact_of_consequence"
    And claimed evidence classes of "verifiable_citation"
    When the evidence obligation is assessed
    Then the evidence assessment status is "incomplete"
    And the missing evidence classes are "artifact_of_consequence"

  Scenario: An obligated step with no claims is absent
    Given an evidence obligation of "verifiable_citation, artifact_of_consequence"
    And no evidence classes are claimed
    When the evidence obligation is assessed
    Then the evidence assessment status is "absent"
    And the missing evidence classes are "verifiable_citation, artifact_of_consequence"

  Scenario: A stronger claim satisfies a weaker obligation through the shared predicate
    Given an evidence obligation of "verifiable_citation"
    And claimed evidence classes of "artifact_of_consequence"
    When the evidence obligation is assessed
    Then the evidence assessment agrees with the shared obligation predicate
    And the evidence assessment status is "present-as-claimed"

  Scenario: A step without an obligation has nothing to assess
    Given no evidence obligation
    And claimed evidence classes of "artifact_of_consequence"
    When the evidence obligation is assessed
    Then no evidence assessment is produced
