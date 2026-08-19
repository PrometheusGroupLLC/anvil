Feature: Playbook generation persists anchored playbooks
  Completing a seeded playbook_generation artifact persists the generated
  playbook machine and its exemplar bundle into the resolved owner-home.

  Scenario: candidate with rubric anchors and exemplars persists machine and exemplar files
    Given a route hearth seeded with the builder machine
    And the engine is started with that hearth
    And a separate temp owner-home directory
    When the engine intakes an anchored candidate playbook and drives it to completed
    Then a machine.yaml exists at "playbooks/evidence_triage/machine.yaml" under the persist owner-home
    And the persisted generated machine has success rubric anchors:
      | instance    | band |
      | triage-good | good |
      | triage-trap | trap |
    And an exemplar markdown exists at "playbooks/evidence_triage/exemplars/triage-good.md" under the persist owner-home
    And an exemplar markdown exists at "playbooks/evidence_triage/exemplars/triage-trap.md" under the persist owner-home
