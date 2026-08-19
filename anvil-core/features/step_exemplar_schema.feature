Feature: Step exemplar schema and validation
  Step exemplars are STEP-GRAIN artifact-quality calibration anchors — the
  per-step grader's reference frame (temper's step_quality_grader), distinct
  from whole-playbook rubric exemplars (see exemplar_schema_storage_resolution
  .feature). Their band vocabulary is {good, bad, ceiling, mediocre} — NOT the
  playbook-exemplar vocabulary {good, bad, trap, hidden_virtue} — they declare
  their grain and target lifecycle state explicitly, and only the synthetic
  "ceiling" band is exempt from naming a real source_instance.

  Scenario Outline: valid step exemplars load for every canonical band
    Given a step exemplar Markdown file with band "<band>"
    When the step exemplar is loaded
    Then the step exemplar load succeeds
    And the loaded step exemplar band is "<band>"
    And the loaded step exemplar body contains "distilled pattern"

    Examples:
      | band     |
      | good     |
      | bad      |
      | ceiling  |
      | mediocre |

  Scenario: an out-of-vocabulary band is rejected with a step-specific code
    Given a step exemplar Markdown file with band "trap"
    When the step exemplar is loaded
    Then the step exemplar load fails with code "step_exemplar_invalid_band"

  Scenario: a non-step grain is rejected
    Given a step exemplar Markdown file with grain "playbook"
    When the step exemplar is loaded
    Then the step exemplar load fails with code "step_exemplar_invalid_grain"

  Scenario Outline: good and bad step exemplars require a source_instance
    Given a step exemplar Markdown file with band "<band>" and no source_instance
    When the step exemplar is loaded
    Then the step exemplar load fails with code "step_exemplar_source_required"

    Examples:
      | band |
      | good |
      | bad  |

  Scenario: a ceiling step exemplar needs no source_instance
    Given a step exemplar Markdown file with band "ceiling" and no source_instance
    When the step exemplar is loaded
    Then the step exemplar load succeeds

  Scenario: a step exemplar file with missing frontmatter is rejected
    Given a step exemplar Markdown file with no frontmatter
    When the step exemplar is loaded
    Then the step exemplar load fails with code "step_exemplar_missing_frontmatter"

  Scenario Outline: raw artifact markers are rejected
    Given a step exemplar Markdown file containing raw marker "<marker>"
    When the step exemplar is loaded
    Then the step exemplar load fails with code "<code>"

    Examples:
      | marker           | code                         |
      | raw              | exemplar_raw_frontmatter_key |
      | raw_artifact     | exemplar_raw_frontmatter_key |
      | raw_artifact_ref | exemplar_raw_frontmatter_key |
      | body_fence       | exemplar_raw_artifact_body   |
