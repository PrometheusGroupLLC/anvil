Feature: Residual playbook-token classification is fail-loud

  The residual-token classifier is the gate that keeps a leftover `playbook`
  token from being accepted merely because nobody looked at it. A checker that
  cannot go red is not a check, so every scenario below is a distinct way it
  MUST go red, plus the one way it may go green.

  Each scenario builds a throwaway fixture repository carrying its own
  vocabulary matrix and exactly one user-visible token, then runs the real
  `scripts/residual-workflow-tokens.py` against it.

  Two of the reds below are about BREADTH rather than correctness. An exemption
  written broadly enough — a whole file, a bare stem — covers not only the
  residues someone reviewed but every one anybody writes into that file
  afterwards. So an exemption must say how many tokens it covers, and it goes
  red when that number stops being true.

  Scenario: An unclassified hit fails
    Given a fixture repository whose only user-visible playbook token is in a document
    And the allowlist classifies nothing
    When the residual-token classifier runs
    Then the classifier fails reporting "unclassified_hit"

  Scenario: A category outside the five fails
    Given a fixture repository whose only user-visible playbook token is in a document
    And the allowlist classifies the token with category "vestigial" and referent "definition_artifact" and gate "NG-PROTO-VNEXT"
    When the residual-token classifier runs
    Then the classifier fails reporting "unknown_category"

  Scenario: A referent absent from the vocabulary matrix fails
    Given a fixture repository whose only user-visible playbook token is in a document
    And the allowlist classifies the token with category "legacy_adapter" and referent "invented_referent" and gate "NG-PROTO-VNEXT"
    When the residual-token classifier runs
    Then the classifier fails reporting "unknown_referent"

  Scenario: The category internal is rejected for rendered text
    Given a fixture repository whose only user-visible playbook token is in a document
    And the allowlist classifies the token with category "internal" and referent "definition_artifact" and gate "NG-PROTO-VNEXT"
    When the residual-token classifier runs
    Then the classifier fails reporting "rejected_category"

  Scenario: An ordinary-process noun with no context proof fails
    Given a fixture repository whose only user-visible playbook token is in a document
    And the allowlist classifies the token with category "ordinary_process_noun" and referent "none" and gate "not_applicable"
    When the residual-token classifier runs
    Then the classifier fails reporting "missing_context_proof"

  Scenario: A stale entry that classifies a token nobody wrote fails
    Given a fixture repository whose only user-visible playbook token is in a document
    And the allowlist classifies the token with category "legacy_adapter" and referent "definition_artifact" and gate "NG-PROTO-VNEXT"
    And the allowlist also classifies a token in a file that has none
    When the residual-token classifier runs
    Then the classifier fails reporting "stale_allowlist_entry"

  Scenario: An exemption that does not say how many tokens it covers fails
    Given a fixture repository whose only user-visible playbook token is in a document
    And the allowlist exempts the whole document without declaring how many tokens it covers
    When the residual-token classifier runs
    Then the classifier fails reporting "missing_hit_count"

  Scenario: A whole-file exemption does not silently absorb a token written later
    Given a fixture repository whose only user-visible playbook token is in a document
    And the allowlist exempts the whole document
    And someone later writes another retired token into that same document
    When the residual-token classifier runs
    Then the classifier fails reporting "hit_count_drift"

  Scenario: A fully classified set passes
    Given a fixture repository whose only user-visible playbook token is in a document
    And the allowlist classifies the token with category "legacy_adapter" and referent "definition_artifact" and gate "NG-PROTO-VNEXT"
    When the residual-token classifier runs
    Then the classifier passes
