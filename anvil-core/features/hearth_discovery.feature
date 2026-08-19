Feature: Hearth discovery beneath permitted roots
  An `all_hearths` read-fold must see every project hearth, not just the
  permitted roots verbatim. When a permitted root is a PARENT directory, the
  real activity sinks live in sub-hearths beneath it. Discovery scans each
  permitted root's immediate subdirectories, includes each one that looks like
  a hearth, always includes the explicit default hearth, dedups the result, and
  excludes ordinary (non-hearth) subdirectories.

  Scenario: sub-hearths beneath a permitted root are discovered and non-hearths excluded
    Given a permitted root containing sub-hearths "foundry-hearth,kiln-hearth,lore-hearth" and a non-hearth dir "not-a-hearth"
    And an explicit default hearth "anvil-hearth"
    When hearths are discovered for that root and explicit hearth
    Then the discovered hearths include "foundry-hearth"
    And the discovered hearths include "kiln-hearth"
    And the discovered hearths include "lore-hearth"
    And the discovered hearths include "anvil-hearth"
    And the discovered hearths exclude "not-a-hearth"
    And 4 hearths are discovered

  Scenario: the explicit hearth is not double-counted when it sits under the root
    Given a permitted root containing sub-hearths "proj-hearth" and a non-hearth dir "docs"
    And the explicit default hearth is the sub-hearth "proj-hearth" under that root
    When hearths are discovered for that root and explicit hearth
    Then the discovered hearths include "proj-hearth"
    And 1 hearths are discovered

  Scenario: only genuine hearths are discovered — code repos and bare sinks are excluded, no double-count
    Given a permitted root with a genuine hearth "foundry-hearth", a code-repo dir "foundry" pointing at it, and a bare-sink dir "logs-only"
    And no explicit default hearth
    When hearths are discovered for that root and explicit hearth
    Then the discovered hearths include "foundry-hearth"
    And the discovered hearths exclude "foundry"
    And the discovered hearths exclude "logs-only"
    And 1 hearths are discovered

  Scenario: a permitted root that is itself a hearth is included
    Given a permitted root that is itself a hearth with no sub-hearths
    And no explicit default hearth
    When hearths are discovered for that root and explicit hearth
    Then the root itself is among the discovered hearths
    And 1 hearths are discovered
