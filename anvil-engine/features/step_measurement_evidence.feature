Feature: Lifecycle transitions emit stamped evidence assessments
  A DRIVEN playbook's durable step measurement records what the caller claimed
  against the obligation of the exact step that transitioned.

  Background:
    Given a P2a evidence measurement hearth
    And the engine is started with that hearth

  Scenario: A complete transition records a stamped and redacted present claim
    Given a complete transition claiming "artifact_of_consequence" as "artifact:test-run:sha-9173"
    And the transition request contains recognizable raw note, project root, and artifact path text
    When the evidence lifecycle transition is sent
    Then the evidence lifecycle transition succeeds
    And exactly one evidence assessment row records status "present-as-claimed"
    And the evidence row carries the exact registry machine version
    And the evidence row contains only the opaque claim reference, not the raw request text

  Scenario: A partial complete transition records the missing class
    Given a complete transition with obligation "artifact_of_consequence, verifiable_citation" claiming "verifiable_citation"
    When the evidence lifecycle transition is sent
    Then the evidence lifecycle transition succeeds
    And exactly one evidence assessment row records status "incomplete"
    And the evidence row names missing classes "artifact_of_consequence"

  Scenario: A complete transition with no claims records absent and still succeeds
    Given a complete transition with obligation "artifact_of_consequence" and no claims
    When the evidence lifecycle transition is sent
    Then the evidence lifecycle transition succeeds
    And exactly one evidence assessment row records status "absent"
    And the evidence row names missing classes "artifact_of_consequence"

  Scenario: Complete assesses the machine-selected complete role, not the satisfaction shape
    Given a satisfied complete transition whose selected role "complete" requires "verifiable_citation" while reviewer requires "artifact_of_consequence"
    And that transition claims "verifiable_citation"
    When the evidence lifecycle transition is sent
    Then the evidence lifecycle transition succeeds
    And exactly one evidence assessment row records status "present-as-claimed"
    And the evidence row resolved the "closure_review" state and "complete" role

  Scenario: Complete disambiguates parallel same-destination edges by satisfaction and role
    Given parallel complete edges share a destination, with reviewer "artifact_of_consequence" for full_revision declared before complete "verifiable_citation" for satisfied
    And that transition claims "verifiable_citation"
    When the evidence lifecycle transition is sent
    Then the evidence lifecycle transition succeeds
    And exactly one evidence assessment row records status "present-as-claimed"
    And the evidence row resolved the "closure_review" state and "complete" role

  Scenario: A stronger claim satisfies a weaker obligation through the lifecycle RPC
    Given a complete transition with obligation "verifiable_citation" claiming "artifact_of_consequence"
    When the evidence lifecycle transition is sent
    Then the evidence lifecycle transition succeeds
    And exactly one evidence assessment row records status "present-as-claimed"
    And the evidence row names missing classes ""

  Scenario: An opaque reference containing JSON C0 controls round-trips unchanged
    Given a complete transition with an opaque evidence reference containing JSON C0 controls
    When the evidence lifecycle transition is sent
    Then the evidence lifecycle transition succeeds
    And exactly one evidence assessment row records status "present-as-claimed"
    And the evidence row round-trips the exact opaque JSON C0 reference

  Scenario Outline: Each lifecycle leg assesses the step that actually transitioned
    Given a "<leg>" transition whose assessed step requires "<required>"
    And that transition claims "<claimed>"
    When the evidence lifecycle transition is sent
    Then the evidence lifecycle transition succeeds
    And exactly one evidence assessment row records status "present-as-claimed"
    And the evidence row resolved the "<state>" state and "<role>" role

    Examples:
      | leg      | required                | claimed                 | state       | role     |
      | begin    | artifact_of_consequence | artifact_of_consequence | spec        | doer     |
      | snapshot | verifiable_citation     | verifiable_citation     | spec_review | reviewer |
      | complete | artifact_of_consequence | artifact_of_consequence | spec        | doer     |

  Scenario: A DRIVEN step without an obligation preserves the legacy row shape
    Given a complete transition whose step has no evidence obligation
    When the evidence lifecycle transition is sent
    Then the evidence lifecycle transition succeeds
    And the durable step measurement bytes match the legacy row shape
    And the durable row contains none of the evidence keys

  Scenario: A FREE playbook never emits an evidence assessment
    Given a FREE complete transition whose parsed step declares an evidence obligation
    And that transition claims "artifact_of_consequence"
    When the evidence lifecycle transition is sent
    Then the evidence lifecycle transition succeeds
    And the durable row contains none of the evidence keys
