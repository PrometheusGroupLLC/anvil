Feature: A completion that cites code must cite code that is on origin/main
  `completed` is the engine's assertion that work shipped. Until this gate the
  engine never looked: on the live re-theme program a track was recorded
  `completed` against an implementation that exists only on an unmerged branch,
  and three siblings were in the same position. A completion whose claimed
  evidence names a commit, or a path, is now resolved against the real
  repository before the first write — and refused when the claim is false.

  The fixture is two real git repositories in a temp root: the hearth itself,
  and a sibling code repository carrying an `origin/main` plus a branch commit
  that never reached it. `MERGED_SHA` and `UNMERGED_SHA` stand for those two
  commits.

  # The defect, reproduced: the T-RETHEME-LORE shape.
  Scenario: A completion citing a commit that never reached origin/main is refused
    Given a completion merge-check hearth with the probe in state "spec_review"
    And the engine is started with that hearth
    When the reviewer completes the probe satisfied claiming "artifact_of_consequence:commit:codeapp@UNMERGED_SHA"
    Then the completion is refused
    And the refusal names the unmerged commit, the repository, and the branch it is on
    And the probe artifact rests in state "spec_review"
    And the probe artifact directory is byte-for-byte unchanged

  # The other direction: the gate must let true claims through, or it is a wall.
  Scenario: A completion citing a commit that is an ancestor of origin/main is accepted
    Given a completion merge-check hearth with the probe in state "spec_review"
    And the engine is started with that hearth
    When the reviewer completes the probe satisfied claiming "artifact_of_consequence:commit:codeapp@MERGED_SHA"
    Then the completion succeeds
    And the probe artifact rests in state "completed"

  # The other half of the same defect.
  Scenario: A completion citing a path that does not exist is refused
    Given a completion merge-check hearth with the probe in state "spec_review"
    And the engine is started with that hearth
    When the reviewer completes the probe satisfied claiming "verifiable_citation:codeapp@src/never_written.rs"
    Then the completion is refused
    And the refusal names the path it could not find
    And the probe artifact rests in state "spec_review"

  Scenario: A completion citing a path that exists is accepted
    Given a completion merge-check hearth with the probe in state "spec_review"
    And the engine is started with that hearth
    When the reviewer completes the probe satisfied claiming "verifiable_citation:codeapp@src/shipped.rs:12"
    Then the completion succeeds
    And the probe artifact rests in state "completed"

  # An unresolvable claim is not a passing claim.
  Scenario: A completion citing a repository the engine cannot locate is refused
    Given a completion merge-check hearth with the probe in state "spec_review"
    And the engine is started with that hearth
    When the reviewer completes the probe satisfied claiming "artifact_of_consequence:commit:nosuchrepo@MERGED_SHA"
    Then the completion is refused
    And the refusal names the repository it could not locate
    And the probe artifact rests in state "spec_review"

  # An honest case, stated out loud: a document-only track cites no code.
  Scenario: A completion presenting no claims at all is accepted
    Given a completion merge-check hearth with the probe in state "spec_review"
    And the engine is started with that hearth
    When the reviewer completes the probe satisfied presenting no claims
    Then the completion succeeds
    And the probe artifact rests in state "completed"

  # An honest case, stated out loud: prose is not a code citation.
  Scenario: A completion whose only claim is prose is accepted
    Given a completion merge-check hearth with the probe in state "spec_review"
    And the engine is started with that hearth
    When the reviewer completes the probe satisfied claiming "self_description:read the whole diff by hand"
    Then the completion succeeds
    And the probe artifact rests in state "completed"

  # An honest case, stated out loud: a URL is an external citation with no local referent.
  Scenario: A completion citing a URL is accepted
    Given a completion merge-check hearth with the probe in state "spec_review"
    And the engine is started with that hearth
    When the reviewer completes the probe satisfied claiming "verifiable_citation:https://example.invalid/pull/7/files"
    Then the completion succeeds
    And the probe artifact rests in state "completed"

  # The exclusion that keeps the gate off honest in-flight work: mid-lifecycle
  # code IS supposed to be on a branch. Only entering `completed` arms the check.
  Scenario: A transition that is not a completion is never gated
    Given a completion merge-check hearth with the probe in state "spec"
    And the engine is started with that hearth
    When the doer completes the probe claiming "artifact_of_consequence:commit:codeapp@UNMERGED_SHA"
    Then the completion succeeds
    And the probe artifact rests in state "spec_review"
