Feature: Evidence obligation schema (parser-first, byte-identical when absent)
  T-EEC-1 Phase 0. `MeasurementSpec` gains an optional `evidence_obligation`
  field: a set of required `EvidenceClass` values naming the evidence classes a
  measured `(state, role)` step must claim. The field is additive and
  parser-first — it parses and round-trips before anything enforces it. Under
  `#[serde(default, skip_serializing_if = "Vec::is_empty")]`, an absent/empty
  obligation is omitted from serialization, so an undeclared machine serializes
  byte-for-byte identically to its pre-field form and its content-hash
  `playbook_version` is unchanged (no spurious variant-event wave over the
  fleet). A machine that DECLARES an obligation re-hashes (the intended variant
  event). The field reuses the existing closed `EvidenceClass` vocabulary; an
  unrecognized class value is rejected at parse time.

  # C1 — a declared obligation loads and the loaded spec carries it.
  Scenario: A measured step declaring an evidence obligation loads and carries it
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
              evidence_obligation: [artifact_of_consequence]
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
    When the playbook loader parses the file with artifact id "obligation-track" and no hook files
    Then the parse succeeds
    And the loaded spec for state "spec" role "doer" requires evidence classes "artifact_of_consequence"

  # C2 — a declared obligation round-trips through serde_yaml.
  Scenario: A declared obligation survives a serde_yaml round-trip
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
              evidence_obligation: [artifact_of_consequence, verifiable_citation]
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
    When the playbook loader parses the file with artifact id "roundtrip-track" and no hook files
    And the loaded machine is re-serialized to YAML and reloaded with artifact id "roundtrip-track"
    Then the re-serialized YAML contains an evidence_obligation key
    And the reloaded spec for state "spec" role "doer" requires evidence classes "artifact_of_consequence, verifiable_citation"

  # C3a — version-identity: an absent obligation and an explicit empty obligation
  # hash to the SAME playbook_version (empty is omitted from canonical JSON).
  Scenario: An absent obligation and an empty obligation yield the same playbook_version
    Given the playbook version for slot A is computed from machine content:
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
    And the playbook version for slot B is computed from machine content:
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
              evidence_obligation: []
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
    Then the playbook version for slot A equals slot B

  # C3b — byte-identity when absent: the serde_yaml output omits the key entirely
  # for a machine that declares no obligation.
  Scenario: A machine with no obligation omits the evidence_obligation key on re-serialization
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
    When the playbook loader parses the file with artifact id "no-obligation-track" and no hook files
    And the loaded machine is re-serialized to YAML and reloaded with artifact id "no-obligation-track"
    Then the re-serialized YAML omits an evidence_obligation key

  # C4 — a machine WITH an obligation on a measured step produces a DIFFERENT
  # playbook_version than the same machine without it (the intended variant event).
  Scenario: Adding an obligation to a measured step changes the playbook_version
    Given the playbook version for slot A is computed from machine content:
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
    And the playbook version for slot B is computed from machine content:
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
              evidence_obligation: [artifact_of_consequence]
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
    Then the playbook version for slot A differs from slot B

  # C5 — an unrecognized class value is rejected at parse time.
  Scenario: An unrecognized evidence class is rejected as a parse error
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
              evidence_obligation: [not_a_real_class]
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
    When the playbook loader parses the file with artifact id "bad-class-track" and no hook files
    Then the parse fails with error code "playbook_yaml_parse_error"
