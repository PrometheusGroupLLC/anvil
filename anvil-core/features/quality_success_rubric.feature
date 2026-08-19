Feature: Quality dimension vocabulary + success rubric schema
  A shared, canonical quality-dimension vocabulary makes cross-playbook quality
  scores comparable. A PlaybookMachine may declare an OPTIONAL success_rubric
  selecting + weighting dimensions from that vocabulary, naming a grader
  reference, and declaring lagging-signal identifiers. A MeasurementSpec may
  declare an OPTIONAL per-step success_criteria. All fields are additive: a
  machine without a rubric, and a measurement without success_criteria, still
  parse (backward compatible with deny_unknown_fields).

  # ===== Backward compatibility: every on-disk machine still parses =====

  Scenario: every real on-disk machine.yaml still loads after the additive fields
    Then every on-disk machine.yaml still parses through the loader

  # ===== Phase A: shared dimension vocabulary =====

  Scenario Outline: canonical quality dimensions are in the shared vocabulary
    Then the quality dimension "<dimension>" is in the shared vocabulary

    Examples:
      | dimension                    |
      | correctness                  |
      | clarity_structure            |
      | pattern_alignment            |
      | research_rigor               |
      | forethought                  |
      | extensibility                |
      | security                     |
      | faithfulness_to_real_process |
      | parent_alignment             |

  Scenario: an unknown dimension is rejected by the shared vocabulary
    Then the quality dimension "vibes" is not in the shared vocabulary

  # ===== Phase B: success_rubric on PlaybookMachine =====

  Scenario: a machine declaring a valid success_rubric parses
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: A track lifecycle.
      required_fields: []
      roles: [doer]
      states:
        - name: spec
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
      transitions: []
      success_rubric:
        dimensions:
          - dimension: correctness
            weight: 3
          - dimension: security
            weight: 1
        grader: llm_rubric_judge_v1
        lagging_signals: ["churn", "revert_rate"]
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse succeeds
    And the loaded playbook success_rubric has 2 dimensions
    And the loaded playbook success_rubric dimension 0 is "correctness" weight 3
    And the loaded playbook success_rubric dimension 1 is "security" weight 1
    And the loaded playbook success_rubric grader is "llm_rubric_judge_v1"
    And the loaded playbook success_rubric lagging_signals are "[\"churn\", \"revert_rate\"]"

  Scenario: a machine omitting success_rubric parses (backward compatible)
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: A track lifecycle.
      required_fields: []
      roles: [doer]
      states:
        - name: spec
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
      transitions: []
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse succeeds
    And the loaded playbook has no success_rubric

  Scenario: a success_rubric referencing an unknown dimension is rejected
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: A track lifecycle.
      required_fields: []
      roles: [doer]
      states:
        - name: spec
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
      transitions: []
      success_rubric:
        dimensions:
          - dimension: vibes
            weight: 1
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse fails with error code "playbook_unknown_quality_dimension"

  Scenario: a success_rubric dimension with a zero weight is rejected
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: A track lifecycle.
      required_fields: []
      roles: [doer]
      states:
        - name: spec
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
      transitions: []
      success_rubric:
        dimensions:
          - dimension: correctness
            weight: 0
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse fails with error code "playbook_invalid_rubric_weight"

  # ===== Phase B: success_criteria on MeasurementSpec =====

  Scenario: a MeasurementSpec declaring success_criteria parses
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: A track lifecycle.
      required_fields: []
      roles: [doer]
      states:
        - name: spec
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
          measurement_by_role:
            doer:
              intent: Write the spec.
              expected_output: A spec.md.
              success_criteria: Acceptance criteria are brine-checkable and unambiguous.
      transitions: []
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse succeeds
    And the loaded measurement for state "spec" role "doer" has success_criteria "Acceptance criteria are brine-checkable and unambiguous."

  Scenario: a MeasurementSpec without success_criteria parses (backward compatible)
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: A track lifecycle.
      required_fields: []
      roles: [doer]
      states:
        - name: spec
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
          measurement_by_role:
            doer:
              intent: Write the spec.
              expected_output: A spec.md.
      transitions: []
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse succeeds
    And the loaded measurement for state "spec" role "doer" has no success_criteria

  # ===== Phase 3a: evidence_class on a rubric dimension =====

  Scenario: a rubric dimension declaring an evidence_class parses
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: A track lifecycle.
      required_fields: []
      roles: [doer]
      states:
        - name: spec
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
      transitions: []
      success_rubric:
        dimensions:
          - dimension: correctness
            weight: 3
            evidence_class: artifact_of_consequence
          - dimension: research_rigor
            weight: 1
            evidence_class: verifiable_citation
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse succeeds
    And the loaded playbook success_rubric dimension 0 evidence_class is "artifact_of_consequence"
    And the loaded playbook success_rubric dimension 1 evidence_class is "verifiable_citation"

  Scenario: a rubric dimension omitting evidence_class defaults to self_description
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: A track lifecycle.
      required_fields: []
      roles: [doer]
      states:
        - name: spec
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
      transitions: []
      success_rubric:
        dimensions:
          - dimension: correctness
            weight: 3
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse succeeds
    And the loaded playbook success_rubric dimension 0 evidence_class is "self_description"

  Scenario: an unknown evidence_class is rejected
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: A track lifecycle.
      required_fields: []
      roles: [doer]
      states:
        - name: spec
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
      transitions: []
      success_rubric:
        dimensions:
          - dimension: correctness
            weight: 3
            evidence_class: vibes
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse fails with error code "playbook_yaml_parse_error"

  # ===== Phase 3a: anchors on a success_rubric =====

  Scenario: a success_rubric declaring anchors parses
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: A track lifecycle.
      required_fields: []
      roles: [doer]
      states:
        - name: spec
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
      transitions: []
      success_rubric:
        dimensions:
          - dimension: correctness
            weight: 3
        anchors:
          - instance: exemplar_good_001
            band: good
          - instance: exemplar_mediocre_002
            band: mediocre
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse succeeds
    And the loaded playbook success_rubric has 2 anchors
    And the loaded playbook success_rubric anchor 0 is instance "exemplar_good_001" band "good"
    And the loaded playbook success_rubric anchor 1 is instance "exemplar_mediocre_002" band "mediocre"

  Scenario: a success_rubric omitting anchors parses with an empty anchor list
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: A track lifecycle.
      required_fields: []
      roles: [doer]
      states:
        - name: spec
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
      transitions: []
      success_rubric:
        dimensions:
          - dimension: correctness
            weight: 3
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse succeeds
    And the loaded playbook success_rubric has 0 anchors

  # ===== Phase 3a: outcome_predicate on a PlaybookMachine =====

  Scenario: a machine declaring an outcome_predicate parses
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: A track lifecycle.
      required_fields: []
      roles: [doer]
      states:
        - name: spec
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
        - name: completed
          role_filters: []
          registry_section: completed
          projection_targets: []
          is_review_gate: false
          is_terminal: true
      transitions:
        - from_state: spec
          to_state: completed
          required_role: doer
          requires_approver: false
      outcome_predicate:
        terminal_state: completed
        check: behavior on main + brine-green
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse succeeds
    And the loaded playbook outcome_predicate terminal_state is "completed"
    And the loaded playbook outcome_predicate check is "behavior on main + brine-green"

  Scenario: a machine omitting outcome_predicate parses (backward compatible)
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: A track lifecycle.
      required_fields: []
      roles: [doer]
      states:
        - name: spec
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
      transitions: []
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse succeeds
    And the loaded playbook has no outcome_predicate

  Scenario: an outcome_predicate naming a non-existent terminal_state is rejected
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: A track lifecycle.
      required_fields: []
      roles: [doer]
      states:
        - name: spec
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
      transitions: []
      outcome_predicate:
        terminal_state: nonexistent
      """
    When the playbook loader parses the file with artifact id "test-artifact" and no hook files
    Then the parse fails with error code "playbook_outcome_predicate_unknown_state"
