Feature: A claimed-evidence reference is classified before it is resolved
  The completion merge check can only judge a reference it recognises. What it
  recognises — and, just as load-bearing, what it deliberately does NOT — is the
  difference between a gate and a wall. Over-recognising refuses honest
  completions; under-recognising is a check that cannot fail. These scenarios
  pin both edges of that line as a pure domain seam, with no git and no
  filesystem.

  Scenario Outline: A reference is classified as a commit, a path, or not code
    When the claimed-evidence reference "<reference>" is classified
    Then the reference is classified as "<kind>"
    And the classified repository is "<repo>"
    And the classified target is "<target>"

    Examples: commits — bare, qualified, and symbolic
      | reference                | kind   | repo    | target   |
      | commit:af0bf296          | commit |         | af0bf296 |
      | commit:lore@af0bf296     | commit | lore    | af0bf296 |
      | commit:HEAD              | commit |         | HEAD     |

    Examples: paths — plain, line-suffixed, file-prefixed, and qualified
      | reference                              | kind | repo | target                  |
      | anvil-engine/src/main.rs               | path |      | anvil-engine/src/main.rs |
      | anvil-engine/src/main.rs:467           | path |      | anvil-engine/src/main.rs |
      | file:tracks/20260722T2029_c2/spec.md   | path |      | tracks/20260722T2029_c2/spec.md |
      | lore@packages/ui/button.tsx:12         | path | lore | packages/ui/button.tsx  |
      | Cargo.toml                             | path |      | Cargo.toml              |

    Examples: not code — the deliberate exclusions
      | reference                              | kind     | repo | target |
      | read the whole diff by hand            | not-code |      |        |
      | https://example.invalid/pull/7/files   | not-code |      |        |
      | v0.4.3                                 | not-code |      |        |
      | foundry-app                            | not-code |      |        |
      |                                        | not-code |      |        |

  # The version-literal exclusion is not a nicety: published vendored-package versions
  # (0.3.3, 0.4.3) are quoted as evidence all over this program, and reading
  # `.3` as a file extension would refuse every one of them as a missing path.
  Scenario: A version literal is not mistaken for a filename
    When the claimed-evidence reference "0.1.324" is classified
    Then the reference is classified as "not-code"

  # An `@` inside a path is not a repository qualifier.
  Scenario: An at-sign after the first slash is part of the path
    When the claimed-evidence reference "node_modules/@example-org/design-system/index.css" is classified
    Then the reference is classified as "path"
    And the classified repository is ""
    And the classified target is "node_modules/@example-org/design-system/index.css"

  Scenario: A completion presenting no claims at all passes
    Given no claimed evidence is presented
    When the merge check verdict is taken
    Then the merge check passes

  Scenario: A completion whose claims are all non-code passes
    Given a claim "read the diff" resolved as "not-applicable"
    When the merge check verdict is taken
    Then the merge check passes

  Scenario: One unmerged commit refuses the whole completion
    Given a claim "commit:lore@deadbeef" resolved as "not-merged"
    And a claim "commit:lore@cafef00d" resolved as "merged"
    When the merge check verdict is taken
    Then the merge check refuses
    And the refusal counts 2 code claims examined and 1 refused
    And the refusal message names "deadbeef"
    And the refusal message names "example-org/stranded"

  Scenario: An unresolvable repository refuses rather than passes
    Given a claim "commit:nosuchrepo@deadbeef" resolved as "repo-unresolved"
    When the merge check verdict is taken
    Then the merge check refuses
    And the refusal message names "could not be located"

  Scenario: A repository with no origin/main refuses rather than passes
    Given a claim "commit:lore@deadbeef" resolved as "no-origin-main"
    When the merge check verdict is taken
    Then the merge check refuses
    And the refusal message names "has no origin/main"

  Scenario: A missing path refuses
    Given a claim "lore@src/never.rs" resolved as "path-missing"
    When the merge check verdict is taken
    Then the merge check refuses
    And the refusal message names "does not exist"

  Scenario: A present path passes
    Given a claim "lore@src/real.rs" resolved as "path-present"
    When the merge check verdict is taken
    Then the merge check passes

  Scenario Outline: Only entering completed arms the check
    When the destination state "<state>" is offered to the merge check
    Then the merge check is "<armed>"

    Examples:
      | state         | armed    |
      | completed     | armed    |
      | spec_review   | disarmed |
      | impl_revision | disarmed |
      | abandoned     | disarmed |
      | superseded    | disarmed |
      | resolved      | disarmed |
      | established   | disarmed |
