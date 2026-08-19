Feature: Loader-side measurement enforcement (dark-gated)
  The loader that parses every hearth `machine.yaml` into the registry does
  not, by default, require per-step measurement — this keeps today's
  unbackfilled fleet loadable. An ENFORCING variant of the same loader
  (`load_from_yaml_enforcing` / `validate_with_id_enforcing`) additionally
  requires: (a) every state carrying a `measurement_by_role` entry has a
  non-empty `success_criteria` on that entry (a state with NO
  `measurement_by_role` at all is exempt — automated/routing machines
  legitimately have no per-step measurement), and (b) the machine declares a
  non-null `outcome_predicate` with a non-blank `terminal_state`.

  This is the loader-side twin of the generator's DEFINE block
  (`generator_defines_measurement.feature`) and is dark-gated the same way:
  the engine flips ANVIL_ENFORCE_MEASUREMENT_DEFINITION on only after the
  hearth fleet is backfilled with per-step success_criteria + outcome
  predicates. The default (non-enforcing) loader path is untouched by this
  feature — existing hearth machines keep loading exactly as before.

  Scenario: A measured state missing success_criteria is rejected under enforcement
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
          required_satisfaction: ~
          requires_approver: false
      outcome_predicate:
        terminal_state: completed
      """
    When the playbook loader parses the file with artifact id "unbackfilled-track" and no hook files under measurement enforcement
    Then the parse fails with error code "playbook_measurement_definition_missing"
    And the error carries artifact_id "unbackfilled-track"
    And the error carries detail containing "spec"
    And the error carries detail containing "doer"

  Scenario: A machine with per-step criteria and an outcome predicate loads under enforcement
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
              success_criteria: The spec.md names at least one falsifiable acceptance criterion.
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
          required_satisfaction: ~
          requires_approver: false
      outcome_predicate:
        terminal_state: completed
      """
    When the playbook loader parses the file with artifact id "backfilled-track" and no hook files under measurement enforcement
    Then the parse succeeds

  Scenario: A machine with no measured states at all is exempt under enforcement
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
          required_satisfaction: ~
          requires_approver: false
      outcome_predicate:
        terminal_state: completed
      """
    When the playbook loader parses the file with artifact id "unmeasured-track" and no hook files under measurement enforcement
    Then the parse succeeds

  Scenario: The default (non-enforcing) loader accepts a machine missing measurement
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
          required_satisfaction: ~
          requires_approver: false
      """
    When the playbook loader parses the file with artifact id "unbackfilled-track" and no hook files
    Then the parse succeeds
