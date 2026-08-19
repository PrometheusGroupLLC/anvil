Feature: The gate-check decision is a pure function over the artifact + begin state
  At runtime the harness's pre-tool hook calls `anvil-hooks gate-check`, which must
  decide ALLOW or BLOCK before a mutation lands. The decision logic is pure and
  separate from the stdin/exit plumbing: given the edited file's enclosing forge
  artifact (state + playbook kind), the hard_enforce policy, and whether the actor
  has an open begin session, it returns a verdict.

  BLOCK iff the kind is HARD-enforced AND there is no open begin session.
  ALLOW when the path is not a forge artifact, when the kind is soft, when there is
  an open begin, and (fail-open) on any resolution error.

  Scenario: a hard-enforced kind with no open begin is blocked
    Given a gate-check for an artifact of kind "track" in state "spec"
    And the hard_enforce policy includes "track"
    And there is no open begin session
    When the gate-check decision is computed
    Then the gate-check verdict is "block"

  Scenario: a hard-enforced kind with an open begin is allowed
    Given a gate-check for an artifact of kind "track" in state "spec"
    And the hard_enforce policy includes "track"
    And there is an open begin session
    When the gate-check decision is computed
    Then the gate-check verdict is "allow"

  Scenario: a soft (non-hard-enforced) kind is always allowed
    Given a gate-check for an artifact of kind "decision" in state "investigating"
    And the hard_enforce policy includes "track"
    And there is no open begin session
    When the gate-check decision is computed
    Then the gate-check verdict is "allow"

  Scenario: a path with no enclosing forge artifact is allowed
    Given a gate-check for a path with no enclosing forge artifact
    And the hard_enforce policy includes "track"
    When the gate-check decision is computed
    Then the gate-check verdict is "allow"

  Scenario: a resolution error fails open (allow)
    Given a gate-check that errored resolving the begin status
    And the hard_enforce policy includes "track"
    And the artifact is of kind "track" in state "spec"
    When the gate-check decision is computed
    Then the gate-check verdict is "allow"

  Scenario: a Codex hard-lane resolution error fails closed
    Given a gate-check that errored resolving the begin status
    And the artifact is of kind "track" in state "spec"
    And the hard_enforce policy includes "track"
    When the Codex gate-check decision is computed
    Then the gate-check verdict is "block"

  Scenario: a Codex artifact kind excluded from hard enforcement remains allowed
    Given a gate-check for an artifact of kind "decision" in state "investigating"
    And the hard_enforce policy includes "track"
    And there is no open begin session
    When the Codex gate-check decision is computed
    Then the gate-check verdict is "allow"

  Scenario: a malformed Codex governed mutation target fails closed
    Given a Codex gate-check with a malformed governed mutation target
    When the Codex gate-check decision is computed
    Then the gate-check verdict is "block"

  Scenario: a Codex gate-check timeout fails closed
    Given a Codex gate-check that timed out resolving a governed mutation
    When the Codex gate-check decision is computed
    Then the gate-check verdict is "block"

  Scenario: the Codex process blocks before its outer hook timeout
    Given a Codex governed mutation with an open begin whose resolver exceeds the internal deadline
    When the Codex gate-check process is executed
    Then the gate-check process exits with the blocking status and a reason

  Scenario: a real Codex resolver error fails closed with a reason
    Given a Codex governed mutation with malformed artifact state
    When the Codex gate-check process is executed
    Then the gate-check process exits with the blocking status and a reason

  Scenario: a Codex apply_patch edit without an open begin is blocked
    Given a Codex apply_patch mutation inside an Anvil-governed track with no open begin
    When the Codex gate-check process is executed
    Then the gate-check process exits with the blocking status

  Scenario: a Codex apply_patch edit with an open begin is allowed
    Given a Codex apply_patch mutation inside an Anvil-governed track with an open begin
    When the Codex gate-check process is executed
    Then the gate-check process exits successfully

  Scenario: a Codex Bash command resolves its working artifact context and blocks
    Given a Codex Bash mutation inside an Anvil-governed track with no open begin
    When the Codex gate-check process is executed
    Then the gate-check process exits with the blocking status

  Scenario: a Codex Bash command launched from the hearth root resolves its governed target
    Given a Codex Bash mutation launched from the hearth root targeting a governed track with no open begin
    When the Codex gate-check process is executed
    Then the gate-check process exits with the blocking status

  Scenario: a Codex Bash command launched from the hearth root allows an adopted target
    Given a Codex Bash mutation launched from the hearth root targeting a governed track with an open begin
    When the Codex gate-check process is executed
    Then the gate-check process exits successfully

  Scenario: an ambiguous Codex Bash mutation target inside a resolved hearth blocks
    Given a Codex Bash mutation launched from the hearth root without a trustworthy target
    When the Codex gate-check process is executed
    Then the gate-check process exits with the blocking status

  Scenario: a Codex Bash command outside an Anvil-governed artifact is allowed
    Given a Codex Bash mutation outside an Anvil-governed artifact
    When the Codex gate-check process is executed
    Then the gate-check process exits successfully

  Scenario: a Codex mutation outside every Anvil hearth is allowed
    Given a Codex Bash mutation in a directory with no Anvil hearth
    When the Codex gate-check process is executed
    Then the gate-check process exits successfully

  Scenario: an outside-first multi-file Codex patch cannot bypass a governed target
    Given an outside-first Codex apply_patch mutation also targeting a governed track
    When the Codex gate-check process is executed
    Then the gate-check process exits with the blocking status naming the governed target

  Scenario: an outside-first multi-file Codex Bash command cannot bypass a governed target
    Given an outside-first Codex Bash mutation also targeting a governed track
    When the Codex gate-check process is executed
    Then the gate-check process exits with the blocking status naming the governed target

  Scenario: a Codex patch move into a governed track cannot bypass the gate
    Given a Codex apply_patch move from an outside source into a governed track
    When the Codex gate-check process is executed
    Then the gate-check process exits with the blocking status naming the governed target

  Scenario: gate-check resolves the folded state after a transition event
    Given a filesystem gate-check track begun in "spec_review" after seed state "spec"
    And the filesystem track has a transition event to "spec_review"
    And the hard_enforce policy includes "track"
    When the filesystem gate-check decision is computed for actor "Reviewer-500010"
    Then the filesystem gate-check resolved state is "spec_review"
    And the gate-check verdict is "allow"

  Scenario: a DEGRADED activity log fails OPEN — never nudges a duplicate begin
    # A dropped (malformed) activity entry may have been the actor's OPEN begin
    # marker. The surviving entries show no open begin, but because the log is
    # degraded the gate must NOT return a confident "closed" verdict that BLOCKs
    # a hard-enforced edit and pushes the actor into a duplicate begin. It
    # conservatively fails OPEN (ALLOW), matching the read-error posture.
    Given a filesystem gate-check hard-enforced track in state "spec" whose only activity entry is malformed
    And the hard_enforce policy includes "track"
    When the filesystem gate-check decision is computed for actor "Reviewer-500010"
    Then the gate-check verdict is "allow"

  Scenario: a CLEAN empty activity log still blocks — degradation, not absence, softens the gate
    # The contrast to the scenario above: with a clean, truly-empty activity log
    # (no markers, nothing dropped) the gate is confident there is no open begin
    # and BLOCKs the hard-enforced edit. This proves the ALLOW above is caused
    # specifically by the log being DEGRADED, not merely by having no marker.
    Given a filesystem gate-check hard-enforced track in state "spec" with a clean empty activity log
    And the hard_enforce policy includes "track"
    When the filesystem gate-check decision is computed for actor "Reviewer-500010"
    Then the gate-check verdict is "block"
