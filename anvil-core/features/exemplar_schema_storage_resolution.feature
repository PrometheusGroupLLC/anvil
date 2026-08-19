Feature: Exemplar schema storage and resolution
  Playbook exemplars are redacted Markdown artifacts stored beside a playbook
  kind. Their frontmatter uses a closed band vocabulary, carries a protected
  outcome pointer when claiming consequence-grade evidence, and resolves
  rubric anchors lazily by kind-scoped exemplar id.

  Scenario Outline: valid exemplars load for every canonical band
    Given an exemplar Markdown file with band "<band>"
    When the exemplar is loaded
    Then the exemplar load succeeds
    And the loaded exemplar band is "<band>"
    And the loaded exemplar body contains "distilled pattern"

    Examples:
      | band          |
      | good          |
      | bad           |
      | trap          |
      | hidden_virtue |

  Scenario: an exemplar with an out-of-vocabulary band is rejected
    Given an exemplar Markdown file with band "mediocre"
    When the exemplar is loaded
    Then the exemplar load fails with code "exemplar_invalid_band"

  Scenario: consequence-grade evidence requires an outcome link
    Given an exemplar Markdown file with artifact_of_consequence evidence and no outcome_link
    When the exemplar is loaded
    Then the exemplar load fails with code "exemplar_outcome_link_required"

  Scenario Outline: raw artifact markers are rejected
    Given an exemplar Markdown file containing raw marker "<marker>"
    When the exemplar is loaded
    Then the exemplar load fails with code "<code>"

    Examples:
      | marker           | code                         |
      | raw              | exemplar_raw_frontmatter_key |
      | raw_artifact     | exemplar_raw_frontmatter_key |
      | raw_artifact_ref | exemplar_raw_frontmatter_key |
      | body_fence       | exemplar_raw_artifact_body   |

  Scenario: a rubric anchor resolves to kind-scoped exemplar content
    Given a temporary hearth playbook kind "track" with exemplar "good-spec"
    And a success rubric anchor referencing "good-spec" with band "good" and playbook_version "v1"
    When the rubric anchors are resolved for kind "track" and playbook_version "v1"
    Then anchor resolution succeeds
    And the resolved exemplar body contains "distilled pattern"
    And anchor resolution has no warnings

  Scenario: an anchor with an out-of-vocabulary band is rejected during resolution
    Given a temporary hearth playbook kind "track" with exemplar "good-spec"
    And a success rubric anchor referencing "good-spec" with band "mediocre" and playbook_version "v1"
    When the rubric anchors are resolved for kind "track" and playbook_version "v1"
    Then anchor resolution fails with code "exemplar_invalid_anchor_band"

  Scenario: a missing anchor file is a resolution error
    Given a temporary hearth playbook kind "track" with no exemplars
    And a success rubric anchor referencing "missing-spec" with band "good" and playbook_version "v1"
    When the rubric anchors are resolved for kind "track" and playbook_version "v1"
    Then anchor resolution fails with code "exemplar_anchor_missing"

  Scenario: duplicate exemplar ids are a resolution error
    Given a temporary hearth playbook kind "track" with exemplar file "alpha.md" id "shared-id"
    And the hearth playbook kind "track" has exemplar file "beta.md" id "shared-id"
    And a success rubric anchor referencing "shared-id" with band "good" and playbook_version "v1"
    When the rubric anchors are resolved for kind "track" and playbook_version "v1"
    Then anchor resolution fails with code "exemplar_duplicate_id"

  Scenario: a stale exemplar version is a warning, not a hard error
    Given a temporary hearth playbook kind "track" with exemplar "good-spec" playbook_version "old-v1"
    And a success rubric anchor referencing "good-spec" with band "good" and playbook_version "new-v2"
    When the rubric anchors are resolved for kind "track" and playbook_version "new-v2"
    Then anchor resolution succeeds
    And anchor resolution has a stale warning for exemplar "good-spec" expected "new-v2" actual "old-v1"

  Scenario: coverage reports every scored dimension covered
    Given a success rubric scoring dimensions "correctness,security"
    And resolved exemplars covering dimensions "correctness,security"
    When exemplar coverage is calculated
    Then exemplar coverage is complete

  Scenario: coverage reports uncovered scored dimensions
    Given a success rubric scoring dimensions "correctness,security"
    And resolved exemplars covering dimensions "correctness"
    When exemplar coverage is calculated
    Then exemplar coverage is incomplete with uncovered dimensions "security"

  Scenario: anchor coverage validation passes when every scored dimension is covered
    Given a success rubric scoring dimensions "correctness,security"
    And resolved exemplars covering dimensions "correctness,security"
    When anchor coverage is validated
    Then anchor coverage validation passes

  Scenario: anchor coverage validation fails when a scored dimension lacks an anchor
    Given a success rubric scoring dimensions "correctness,security"
    And resolved exemplars covering dimensions "correctness"
    When anchor coverage is validated
    Then anchor coverage validation fails with code "anchor_coverage_uncovered_dimensions"
    And anchor coverage validation failure mentions "security"

  Scenario: none_yet classification can justify uncovered scored dimensions
    Given a success rubric scoring dimensions "correctness,security"
    And resolved exemplars covering dimensions "correctness"
    And ledger classification "none_yet" with complete none_yet justification
    When anchor coverage is validated
    Then anchor coverage validation passes

  Scenario: none_yet classification still fails with an incomplete justification
    Given a success rubric scoring dimensions "correctness,security"
    And resolved exemplars covering dimensions "correctness"
    And ledger classification "none_yet" with missing none_yet justification field "followup_condition"
    When anchor coverage is validated
    Then anchor coverage validation fails with code "anchor_coverage_incomplete_none_yet_justification"
    And anchor coverage validation failure mentions "followup_condition"

  Scenario: a machine with success_rubric and anchors parses additively
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: A track lifecycle with anchored rubric.
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
        anchors:
          - instance: good-spec
            band: good
      """
    When the playbook loader parses the file with artifact id "anchored-rubric" and no hook files
    Then the parse succeeds
    And the loaded playbook success_rubric anchor 0 is instance "good-spec" band "good"

  Scenario: every real on-disk machine.yaml still loads with strict fields
    Then every on-disk machine.yaml still parses through the loader
