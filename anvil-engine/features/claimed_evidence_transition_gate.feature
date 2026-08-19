Feature: Unsatisfied claimed evidence can dark-gate a transition per request lane
  A DRIVEN Tier-A transition may become fail-closed only for a lane that explicitly
  opts in. The opt-in ships OFF and MUST NOT be activated until D-privacy decision
  20260622T1940_step_measurement_emit_privacy reaches decided. The fixture seeds
  that decision as decided only where needed to exercise the otherwise-dark
  mechanism, and resets it to tension to prove an unresolved decision stays inert.

  Background:
    Given a P4 claimed-evidence gate hearth with D-privacy decided
    And the engine is started with that hearth

  Scenario: An opted-in lane refuses an unsatisfied DRIVEN transition before mutation
    Given the claimed-evidence transition gate is enabled for this request lane
    And a complete transition with obligation "artifact_of_consequence" and no claims
    When the evidence lifecycle transition is sent
    Then the transition is refused with failed precondition "playbook_evidence_obligation_unsatisfied"
    And the gated artifact remains in state "spec"
    And the gated transition artifacts remain byte-for-byte unchanged
    And no evidence assessment row is emitted

  Scenario Outline: Every lifecycle leg refuses before its first write
    Given the claimed-evidence transition gate is enabled for this request lane
    And a "<leg>" transition whose assessed step requires "artifact_of_consequence"
    When the evidence lifecycle transition is sent
    Then the transition is refused with failed precondition "playbook_evidence_obligation_unsatisfied"
    And the gated transition artifacts remain byte-for-byte unchanged
    And no evidence assessment row is emitted

    Examples:
      | leg      |
      | begin    |
      | snapshot |

  Scenario: A lane defaults to recording an absent claim without refusing the transition
    Given the claimed-evidence transition gate is not configured for this request lane
    And a complete transition with obligation "artifact_of_consequence" and no claims
    When the evidence lifecycle transition is sent
    Then the evidence lifecycle transition succeeds
    And exactly one evidence assessment row records status "absent"

  Scenario: An opted-in lane remains fail-open while D-privacy is unresolved
    Given the claimed-evidence transition gate is enabled for this request lane
    And the D-privacy decision is unresolved for this request lane
    And a complete transition with obligation "artifact_of_consequence" and no claims
    When the evidence lifecycle transition is sent
    Then the evidence lifecycle transition succeeds
    And exactly one evidence assessment row records status "absent"

  Scenario: An opted-in lane accepts a stronger claim through the canonical predicate
    Given the claimed-evidence transition gate is enabled for this request lane
    And a complete transition with obligation "verifiable_citation" claiming "artifact_of_consequence"
    When the evidence lifecycle transition is sent
    Then the evidence lifecycle transition succeeds
    And exactly one evidence assessment row records status "present-as-claimed"

  Scenario: A FREE transition is never gated even when its lane opts in
    Given the claimed-evidence transition gate is enabled for this request lane
    And a FREE complete transition whose parsed step declares an evidence obligation
    When the evidence lifecycle transition is sent
    Then the evidence lifecycle transition succeeds
    And the durable row contains none of the evidence keys

  Scenario: A DRIVEN step without an obligation is never gated
    Given the claimed-evidence transition gate is enabled for this request lane
    And a complete transition whose step has no evidence obligation
    When the evidence lifecycle transition is sent
    Then the evidence lifecycle transition succeeds
    And the durable row contains none of the evidence keys
