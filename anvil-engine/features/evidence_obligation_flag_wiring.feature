Feature: Evidence-obligation dark-gate wires the flag into every engine seam (T-EEC-1 P4)
  ANVIL_ENFORCE_EVIDENCE_OBLIGATION is the engine's evidence-obligation dark-gate,
  separate from ANVIL_ENFORCE_MEASUREMENT_DEFINITION. When it is on, the
  obligation leg fires at all three seams the measurement gate rides — the
  loader/registry construction (catalog), the persist WRITE boundary, and
  candidate intake — so a DRIVEN measured (state, role) lacking an
  evidence_obligation is refused with playbook_evidence_obligation_missing. When
  it is off (the default), every seam behaves exactly as today.
  #
  # T-EEC-2 P3 closes the intake cases deferred by T-EEC-1 Amendment 1. Candidate
  # register and per-step obligations now survive generation, so the shared
  # evidence-obligation validator can distinguish a forbidden FREE declaration
  # from a compliant DRIVEN declaration before an intake instance is opened.

  Scenario: With the flag on, a FREE candidate declaring an obligation is refused at intake
    Given a hearth seeded with an evidence-compliant builder machine and an active parent track
    And the engine is started with that hearth and evidence obligation enforcement on
    And a separate temp owner-home directory
    When the engine intake of a FREE anchored candidate declaring evidence obligations is attempted
    Then the candidate intake is refused with gRPC status "INVALID_ARGUMENT"
    And the candidate intake error message contains "playbook_evidence_obligation_on_free_register"
    And the candidate intake error message contains "evidence_triage"
    And no machine.yaml exists at "playbooks/evidence_triage/machine.yaml" under the persist owner-home
    And no intake instance is written

  Scenario: With the flag on, a compliant DRIVEN candidate is accepted at intake
    Given a hearth seeded with an evidence-compliant builder machine and an active parent track
    And the engine is started with that hearth and evidence obligation enforcement on
    And a separate temp owner-home directory
    When the engine intakes a DRIVEN anchored candidate declaring evidence obligations and drives it to completed
    Then a machine.yaml exists at "playbooks/evidence_triage/machine.yaml" under the persist owner-home
    And the persisted generated machine carries evidence obligations:
      | state                     | role     | evidence_classes                                    |
      | triage                    | doer     | verifiable_citation,artifact_of_consequence         |
      | triage_review             | reviewer | verifiable_citation,artifact_of_consequence         |
      | triage_revision           | doer     | verifiable_citation,artifact_of_consequence         |
      | outcome_reflection_review | reviewer | verifiable_citation,artifact_of_consequence         |

  # C12 loader/registry seam — the flag drives the registry-construction load
  # path. A hearth machine that fails the obligation gate is dropped from the
  # registry, so it disappears from the catalog's available_artifact_kinds. The
  # flag-off control below proves the flag is what drives the drop (before/after).
  Scenario: With the flag on, a DRIVEN measured machine lacking an obligation is dropped from the registry
    Given a hearth directory with playbook files:
      | path                                                       | content                                                                                                                                                                                                                                                                                                                                                                                                                          |
      | playbooks/20260718T0000_obligation_missing/status.yaml     | version: 1\nkind: playbook\nstate: active\n                                                                                                                                                                                                                                                                                                                                                                                       |
      | playbooks/20260718T0000_obligation_missing/machine.yaml    | kind: obligation_missing_kind\ndirectory: obligation_missing_kinds\nregistry: obligation_missing_kinds.md\ndescription: A driven measured machine lacking an obligation\nroles:\n  - doer\nrequired_fields: []\nstates:\n  - name: spec\n    registry_section: active\n    is_review_gate: false\n    is_terminal: false\n    measurement_by_role:\n      doer:\n        intent: Write the spec.\n        expected_output: A spec.md.\n  - name: completed\n    registry_section: completed\n    is_review_gate: false\n    is_terminal: true\ntransitions:\n  - from_state: spec\n    to_state: completed\n    required_role: doer\n    requires_approver: false\n |
    And the engine is started with that hearth and evidence obligation enforcement on
    When the catalog RPC is called
    Then the catalog available playbook kinds do not include "obligation_missing_kind"

  # C10 loader/registry seam control — flag OFF ⇒ the same machine registers.
  Scenario: With the flag off, the same machine registers and is available
    Given a hearth directory with playbook files:
      | path                                                       | content                                                                                                                                                                                                                                                                                                                                                                                                                          |
      | playbooks/20260718T0000_obligation_missing/status.yaml     | version: 1\nkind: playbook\nstate: active\n                                                                                                                                                                                                                                                                                                                                                                                       |
      | playbooks/20260718T0000_obligation_missing/machine.yaml    | kind: obligation_missing_kind\ndirectory: obligation_missing_kinds\nregistry: obligation_missing_kinds.md\ndescription: A driven measured machine lacking an obligation\nroles:\n  - doer\nrequired_fields: []\nstates:\n  - name: spec\n    registry_section: active\n    is_review_gate: false\n    is_terminal: false\n    measurement_by_role:\n      doer:\n        intent: Write the spec.\n        expected_output: A spec.md.\n  - name: completed\n    registry_section: completed\n    is_review_gate: false\n    is_terminal: true\ntransitions:\n  - from_state: spec\n    to_state: completed\n    required_role: doer\n    requires_approver: false\n |
    And the engine is started with that hearth
    When the catalog RPC is called
    Then the catalog available playbook kinds include "obligation_missing_kind"

  # C12 persist seam — the flag drives the persist WRITE boundary.
  Scenario: With the flag on, the persist RPC refuses a DRIVEN measured machine lacking an obligation
    Given a route hearth seeded with the builder machine
    And the engine is started with that hearth and evidence obligation enforcement on
    And a separate temp owner-home directory
    When the PersistPlaybook RPC is called for kind "obligation_missing_kind" under that owner-home with:
      | machine        | driven_measured_no_obligation |
      | actor_name     | Persist-Doer-800001           |
      | actor_type     | agent                         |
      | actor_model    | claude-opus-4-8               |
      | actor_provider | anthropic                     |
    Then the PersistPlaybook RPC returns gRPC status "INVALID_ARGUMENT"
    And the PersistPlaybook RPC error message contains "playbook_evidence_obligation_missing"
    And no machine.yaml exists at "playbooks/obligation_missing_kind/machine.yaml" under the persist owner-home

  # C12 intake seam — the flag drives candidate intake (the F1 vacuous-negative).
  # The anchored candidate generates a measured (triage, doer) pair, so the
  # refusal is the SPECIFIC obligation code on a genuinely measured step — not a
  # vacuous pass on a step-free machine.
  Scenario: With the flag on, intake of a candidate generating a measured step is refused with the obligation code
    Given a route hearth seeded with the builder machine
    And the engine is started with that hearth and evidence obligation enforcement on
    And a separate temp owner-home directory
    When the engine intake of an anchored candidate playbook is attempted
    Then the candidate intake is refused with gRPC status "INVALID_ARGUMENT"
    And the candidate intake error message contains "playbook_evidence_obligation_missing"

  # C10 end-to-end — flag OFF ⇒ zero behavior change (the same machine persists).
  Scenario: With the flag off, the persist RPC accepts a DRIVEN measured machine lacking an obligation
    Given a route hearth seeded with the builder machine
    And the engine is started with that hearth
    And a separate temp owner-home directory
    When the PersistPlaybook RPC is called for kind "obligation_missing_kind" under that owner-home with:
      | machine        | driven_measured_no_obligation |
      | actor_name     | Persist-Doer-800002           |
      | actor_type     | agent                         |
      | actor_model    | claude-opus-4-8               |
      | actor_provider | anthropic                     |
    Then the PersistPlaybook RPC response kind is "obligation_missing_kind"
    And a machine.yaml exists at "playbooks/obligation_missing_kind/machine.yaml" under the persist owner-home
