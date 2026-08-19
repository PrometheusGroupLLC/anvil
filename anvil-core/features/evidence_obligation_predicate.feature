Feature: Evidence obligation satisfaction predicate (execution semantics)
  T-EEC-1 Phase 1. The strength ordering
  `artifact_of_consequence > verifiable_citation > self_description` is encoded
  as an obligation-satisfaction predicate: a claimed item of a
  stronger-or-equal class satisfies an obligation for a weaker class, never the
  reverse. An obligation of N distinct required classes is satisfied only when
  each required class has ≥1 claimed item of that class OR stronger (min-count
  1). A single claimed item may satisfy multiple distinct required classes
  simultaneously (item reuse allowed — decision M2).

  # C6 — same-class satisfaction, each of the three classes.
  Scenario: A claimed artifact_of_consequence satisfies an artifact_of_consequence obligation
    Given an evidence obligation requires "artifact_of_consequence"
    When the actor claims "artifact_of_consequence"
    Then the obligation is satisfied

  Scenario: A claimed verifiable_citation satisfies a verifiable_citation obligation
    Given an evidence obligation requires "verifiable_citation"
    When the actor claims "verifiable_citation"
    Then the obligation is satisfied

  Scenario: A claimed self_description satisfies a self_description obligation
    Given an evidence obligation requires "self_description"
    When the actor claims "self_description"
    Then the obligation is satisfied

  # C7 — upward substitution: stronger claimed satisfies weaker required.
  Scenario: A claimed artifact_of_consequence satisfies a verifiable_citation obligation
    Given an evidence obligation requires "verifiable_citation"
    When the actor claims "artifact_of_consequence"
    Then the obligation is satisfied

  Scenario: A claimed artifact_of_consequence satisfies a self_description obligation
    Given an evidence obligation requires "self_description"
    When the actor claims "artifact_of_consequence"
    Then the obligation is satisfied

  Scenario: A claimed verifiable_citation satisfies a self_description obligation
    Given an evidence obligation requires "self_description"
    When the actor claims "verifiable_citation"
    Then the obligation is satisfied

  # C8 — no downward substitution: weaker claimed never satisfies stronger required.
  Scenario: A claimed self_description does not satisfy a verifiable_citation obligation
    Given an evidence obligation requires "verifiable_citation"
    When the actor claims "self_description"
    Then the obligation is not satisfied

  Scenario: A claimed self_description does not satisfy an artifact_of_consequence obligation
    Given an evidence obligation requires "artifact_of_consequence"
    When the actor claims "self_description"
    Then the obligation is not satisfied

  Scenario: A claimed verifiable_citation does not satisfy an artifact_of_consequence obligation
    Given an evidence obligation requires "artifact_of_consequence"
    When the actor claims "verifiable_citation"
    Then the obligation is not satisfied

  # C9 — set / min-count-1 cardinality with item reuse (the executable form of M2).
  Scenario: A single artifact_of_consequence satisfies an obligation requiring artifact and citation
    Given an evidence obligation requires "artifact_of_consequence, verifiable_citation"
    When the actor claims "artifact_of_consequence"
    Then the obligation is satisfied

  Scenario: A single verifiable_citation does not satisfy an obligation requiring artifact and citation
    Given an evidence obligation requires "artifact_of_consequence, verifiable_citation"
    When the actor claims "verifiable_citation"
    Then the obligation is not satisfied
