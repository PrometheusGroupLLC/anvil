Feature: Seed / machine.yaml outcome-predicate parity
  Every free lifecycle kind is defined in two source tiers — the hearth
  `playbooks/<kind>_lifecycle/machine.yaml` and the compiled-in
  `seeds::<kind>_seed()` fallback. The outcome-predicate fold reads the
  predicate from whichever tier resolves the kind, so the two MUST agree or
  enforcement is not universal across tiers. This feature is the
  generate-don't-duplicate parity guard for the backfilled `outcome_predicate`:
  if a machine.yaml predicate changes without the seed (or vice versa), it fails.

  The three seeds carrying measured states (milestone, proposal, spark) must
  additionally pass the SAME measurement-enforcing validation the yaml loader
  applies, so a seed-resolved instance is never a silent drop.

  Scenario: decision seed matches its machine.yaml outcome predicate
    Then the "decision" seed and its lifecycle machine.yaml declare the same outcome_predicate

  Scenario: initiative seed matches its machine.yaml outcome predicate
    Then the "initiative" seed and its lifecycle machine.yaml declare the same outcome_predicate

  Scenario: learning seed matches its machine.yaml outcome predicate
    Then the "learning" seed and its lifecycle machine.yaml declare the same outcome_predicate

  Scenario: milestone seed matches its machine.yaml outcome predicate and enforces clean
    Then the "milestone" seed and its lifecycle machine.yaml declare the same outcome_predicate
    And the "milestone" seed passes measurement enforcement

  Scenario: proposal seed matches its machine.yaml outcome predicate and enforces clean
    Then the "proposal" seed and its lifecycle machine.yaml declare the same outcome_predicate
    And the "proposal" seed passes measurement enforcement

  Scenario: spark seed matches its machine.yaml outcome predicate and enforces clean
    Then the "spark" seed and its lifecycle machine.yaml declare the same outcome_predicate
    And the "spark" seed passes measurement enforcement
