Feature: Playbook seed validity
  Both compiled-in seeds pass the Phase 1 loader's cross-reference validation.
  Passing validate() confirms:
  - every transition's from_state and to_state are declared states
  - every transition's required_role is a declared role
  - every review-gate state's outgoing transitions carry required_satisfaction

  The track seed carries the migrated (spec, doer) and (spec_review, reviewer)
  hook references (hook_content_serving P5), so it is validated against the
  matching hook filenames present in its on-disk hooks/ directory. The playbook
  seed carries no hook references, so it passes an empty hook_file_names slice.

  Scenario: track seed passes cross-reference validation
    Given the track seed
    When validate is called on the track seed with its hook files
    Then the validation result is Ok

  Scenario: playbook seed passes cross-reference validation
    Given the playbook seed
    When validate is called on the playbook seed with empty hook files
    Then the validation result is Ok
