Feature: Enforcement-bundle conformance check
  The engine registers playbooks through the ENFORCING loader when
  ANVIL_ENFORCE_MEASUREMENT_DEFINITION=1 (which the shipped kit sets). A machine
  whose measured states lack `success_criteria`, or that declares no
  `outcome_predicate`, silently DROPS OUT of the registry under enforcement —
  its begin()/catalog calls then fail with no trace. That half-migration broke
  the proposal/milestone/spark lifecycles live.

  `evaluate_enforcement_bundle` is the checker's library core (the
  `enforcement_bundle_check` example is a thin wrapper around it, and the
  `build-kit.sh` conformance gate runs it over the STAGED kit). Its three-way
  outcome maps to three STABLE exit statuses: pass=0, genuine loader-drop=1,
  setup-failure=2. The build gate maps ONLY a genuine loader-drop (1) to its own
  publish-blocking exit 3, so a half-migrated kit can never be published.

  Scenario: A fully backfilled bundle registers every staged machine under enforcement
    Given a temp playbook bundle
    And the bundle stages a backfilled "milestone" machine
    And the bundle stages a backfilled "spark" machine
    When the enforcement bundle check is evaluated
    Then the bundle check outcome is "pass"
    And the bundle check exit code is "0"
    And the enforcing registry over the bundle drops no artifacts
    And the enforcing registry over the bundle registers kind "milestone"
    And the enforcing registry over the bundle registers kind "spark"

  Scenario: An unbackfilled machine drops under enforcement and is diagnosable
    Given a temp playbook bundle
    And the bundle stages a backfilled "milestone" machine
    And the bundle stages an unbackfilled "proposal" machine
    When the enforcement bundle check is evaluated
    Then the bundle check outcome is "drop"
    And the bundle check exit code is "1"
    And the bundle check drops artifact "proposal_lifecycle"
    And the enforcing registry over the bundle reports measurement-dropped artifact "proposal_lifecycle"
    And the enforcing registry over the bundle registers kind "milestone"

  Scenario: A bundle with no playbooks directory is a setup failure, not a drop
    Given a temp playbook bundle
    When the enforcement bundle check is evaluated
    Then the bundle check outcome is "setup"
    And the bundle check exit code is "2"
