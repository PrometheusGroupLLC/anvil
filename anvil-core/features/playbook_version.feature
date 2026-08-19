Feature: Playbook version is a content hash of the playbook machine
  temper treats "playbook version = experiment unit" (the generator's evolve
  loop and the router's v1-vs-v2 experiments compare versions of a playbook), so
  every measurement/verdict record must carry WHICH version of a playbook
  produced it. The version is a CONTENT HASH of the loaded PlaybookMachine — not
  an author-declared field — so it changes exactly when the machine changes and
  can never be forgotten.

  Scenario: a valid machine yields a non-empty content version
    When the playbook version for slot A is computed from machine content:
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
    Then the playbook version for slot A is non-empty

  Scenario: identical machine content yields an identical version
    When the playbook version for slot A is computed from machine content:
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
      transitions: []
      """
    Then the playbook version for slot A equals slot B

  Scenario: different machine content yields a different version
    When the playbook version for slot A is computed from machine content:
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
    And the playbook version for slot B is computed from machine content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: A DIFFERENT track lifecycle description.
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
    Then the playbook version for slot A differs from slot B

  # ===== Backward compatibility: the additive field round-trips =====

  Scenario: an older playbook-measurement line without playbook_version still parses
    When a playbook-measurement.jsonl line without a playbook_version field is read back
    Then the read-back playbook-measurement playbook_version is absent

  Scenario: a playbook-measurement line carrying playbook_version round-trips
    When a playbook-measurement.jsonl line with playbook_version "abc123def4567890" is read back
    Then the read-back playbook-measurement playbook_version is "abc123def4567890"

  Scenario: an older review-verdict line without playbook_version still parses
    When a review-verdict.jsonl line without a playbook_version field is read back
    Then the read-back review-verdict playbook_version is absent

  Scenario: a review-verdict line carrying playbook_version round-trips
    When a review-verdict.jsonl line with playbook_version "abc123def4567890" is read back
    Then the read-back review-verdict playbook_version is "abc123def4567890"
