Feature: Candidate intake evidence wire extensions
  Candidate authors can send evidence declarations through
  the engine without losing meaning, while older candidate payloads retain
  their established defaults.

  Background:
    Given a hearth seeded with the builder machine and an active parent track
    And the engine is started with that hearth

  Scenario: Candidate extensions survive protobuf intake
    When a free candidate carrying evidence obligations is protobuf round-tripped and submitted
    Then the candidate wire intake succeeds
    And the mapped candidate register is "free"
    And mapped proposed state "triage" has ordered evidence obligations:
      | evidence_class          |
      | verifiable_citation     |
      | artifact_of_consequence |

  Scenario: Omitted candidate extensions retain legacy defaults
    When a legacy candidate payload is protobuf round-tripped and submitted
    Then the candidate wire intake succeeds
    And the legacy protobuf payload bytes are unchanged
    And the mapped candidate register is "driven"
    And mapped proposed state "triage" has no evidence obligations
    And the serialized legacy candidate omits extension defaults

  Scenario: Unknown candidate evidence classes are rejected
    When a candidate carrying unknown evidence class "fabricated_proof" is protobuf round-tripped and submitted
    Then the candidate wire intake is refused with gRPC status "INVALID_ARGUMENT"
    And the candidate wire intake error names "fabricated_proof"

  Scenario: Unknown candidate registers are rejected
    When a candidate carrying unknown register "archived" is protobuf round-tripped and submitted
    Then the candidate wire intake is refused with gRPC status "INVALID_ARGUMENT"
    And the candidate wire intake error names "archived"
