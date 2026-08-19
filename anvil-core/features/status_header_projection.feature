Feature: The status.yaml state header is a projection of the transition events
  An artifact's `status.yaml` carries a top-level `state:` header. Everything
  that reads the FILE rather than calling the engine trusts it: the tracker's
  drift panel, the playbook atlas (`domain::playbook::status_read`), a human
  opening the file, `grep`.

  Before this contract existed, `snapshot`/`complete` wrote a new file into
  `<artifact>/transitions/` and never touched the header, so the header kept its
  creation-time value forever — measured on the live hearths, one proposal was
  14 transitions past its header and still declared `state: vision`.

  The header is therefore a DERIVED PROJECTION of the event store, rewritten on
  every transition. It is never a source: the fold over `transitions/` stays
  authoritative, so a header knocked out of agreement by a hand-edit or a git
  merge is DETECTED and REPAIRED rather than believed.

  # ── The header tracks the transitions ────────────────────────────────────

  # HEADLINE. Fails on the pre-fix engine: the header stays at "spec".
  Scenario: One transition moves the header with it
    Given a status-header hearth with a track created at "spec"
    When the artifact transitions to "spec_review" at "2026-08-06T01:00:00Z"
    Then the status.yaml state header is "spec_review"
    And the state resolved through the engine seam is "spec_review"

  # The drift Nick saw was cumulative, not a single missed write.
  Scenario: A chain of transitions leaves the header on the newest, losing no history
    Given a status-header hearth with a track created at "vision"
    When the artifact transitions to "shaping" at "2026-08-06T01:00:00Z"
    And the artifact transitions to "drafting" at "2026-08-06T02:00:00Z"
    And the artifact transitions to "review" at "2026-08-06T03:00:00Z"
    And the artifact transitions to "active" at "2026-08-06T04:00:00Z"
    Then the status.yaml state header is "active"
    And the state resolved through the engine seam is "active"
    And the transitions event directory holds exactly 5 events

  Scenario: The header is inserted when the artifact never had one
    Given a status-header hearth with a track created at "spec" and no state header
    When the artifact transitions to "plan" at "2026-08-06T01:00:00Z"
    Then the status.yaml state header is "plan"
    And the status.yaml keeps its other keys unchanged

  # ── The negative case: a header that disagrees is not trusted ────────────

  # "spec" is the exact staleness the defect produced: a real earlier state of
  # this artifact, frozen while the events moved on.
  Scenario: A stale header does not change what the engine resolves
    Given a status-header hearth with a track created at "spec"
    When the artifact transitions to "plan" at "2026-08-06T01:00:00Z"
    And the status.yaml state header is hand-edited to "spec"
    Then the state resolved through the engine seam is "plan"

  Scenario: The reconciler reports a stale header and writes nothing
    Given a status-header hearth with a track created at "spec"
    When the artifact transitions to "plan" at "2026-08-06T01:00:00Z"
    And the status.yaml state header is hand-edited to "spec"
    And the header reconciler runs in report mode
    Then the reconciler reports drift from "spec" to "plan"
    And the reconciler wrote nothing
    And the status.yaml state header is "spec"

  Scenario: The reconciler repairs a stale header in apply mode
    Given a status-header hearth with a track created at "spec"
    When the artifact transitions to "plan" at "2026-08-06T01:00:00Z"
    And the status.yaml state header is hand-edited to "spec"
    And the header reconciler runs in apply mode
    Then the reconciler reports drift from "spec" to "plan"
    And the reconciler wrote the repair
    And the status.yaml state header is "plan"

  # The write path is AUTHORITATIVE — it just made the newest event — so it
  # re-projects unconditionally, including over a header the offline auditor
  # would refuse to touch.
  Scenario: The next transition self-heals a header no transition ever set
    Given a status-header hearth with a track created at "spec"
    When the artifact transitions to "plan" at "2026-08-06T01:00:00Z"
    And the status.yaml state header is hand-edited to "abandoned"
    And the artifact transitions to "implementing" at "2026-08-06T02:00:00Z"
    Then the status.yaml state header is "implementing"

  # The backfill's own hazard, found by running it against the live hearths:
  # three foundry-hearth tracks declare `state: complete` while their
  # hand-authored `transitions:` array stops at `build` (the final state change
  # was recorded only as an `activity:` marker) and they have NO event store.
  # A reconciler that trusted the array would "repair" a finished track
  # BACKWARDS. It must report and leave it alone instead.
  Scenario: A legacy-array-only header that is ahead of the array is reported, never overwritten
    Given a status-header hearth with a legacy-only track whose header "complete" is ahead of its array tail "build"
    When the header reconciler runs in apply mode
    Then the reconciler reports the header unverifiable, showing "complete" against "build"
    And the reconciler wrote nothing
    And the status.yaml state header is "complete"

  # The second hazard, same backfill run: foundry-hearth's
  # tracks/20260627T1607_connections_into_foundry_mcp_cli declares
  # `state: abandoned` and holds exactly ONE event — the creation seed to
  # `spec`. `abandoned` was never transitioned to, so it is not this log's
  # stale value; a reconciler that only checked "are there events?" would have
  # reverted a deliberately-abandoned track to `spec`.
  Scenario: A header naming a state no transition ever set is reported, never overwritten
    Given a status-header hearth with a track created at "spec"
    When the status.yaml state header is hand-edited to "abandoned"
    And the header reconciler runs in apply mode
    Then the reconciler reports the header unverifiable, showing "abandoned" against "spec"
    And the reconciler wrote nothing
    And the status.yaml state header is "abandoned"

  # A reconciler that rewrote every file it touched would be unauditable.
  Scenario: A header already in agreement is reported clean and left alone
    Given a status-header hearth with a track created at "spec"
    When the artifact transitions to "plan" at "2026-08-06T01:00:00Z"
    And the header reconciler runs in apply mode
    Then the reconciler reports no drift
    And the reconciler wrote nothing

  # ── Refusals: never project a header from evidence it could not read ─────

  Scenario: The reconciler refuses an artifact whose state cannot be resolved
    Given a status-header hearth with a track that has no state header and no transitions
    When the header reconciler runs in apply mode and is expected to refuse
    Then the reconciler refuses with "state_unresolvable"

  Scenario: The reconciler refuses an artifact with no status.yaml
    Given a status-header hearth with an artifact directory that has no status.yaml
    When the header reconciler runs in apply mode and is expected to refuse
    Then the reconciler refuses with "status_missing"
