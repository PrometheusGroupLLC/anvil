Feature: Filesystem Hearth Reader
  The filesystem hearth reader scans a hearth directory for artifacts
  by reading status.yaml files from proposals/, tracks/, milestones/,
  initiatives/, decisions/, learnings/, playbooks/, and legacy workflows/ subdirectories.

  Scenario: Artifacts are read from a hearth directory
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260403T1500_forge_lifecycle/           | active       |
      | proposals/20260404T2116_forge_initiatives/         | completed    |
      | tracks/20260411T2345_mcp_server_foundation/        | implementing |
      | milestones/20260404T0700_brine_rollout_readiness/  | active       |
      | initiatives/follow-forge-lifecycle/                | promoted     |
      | decisions/event-format-choice/                     | decided      |
    When the filesystem hearth reader lists artifacts
    Then 6 artifacts are returned
    And the returned artifacts include "20260403T1500_forge_lifecycle" with type "proposal" and state "active"
    And the returned artifacts include "20260404T2116_forge_initiatives" with type "proposal" and state "completed"
    And the returned artifacts include "20260411T2345_mcp_server_foundation" with type "track" and state "implementing"
    And the returned artifacts include "20260404T0700_brine_rollout_readiness" with type "milestone" and state "active"
    And the returned artifacts include "follow-forge-lifecycle" with type "initiative" and state "promoted"
    And the returned artifacts include "event-format-choice" with type "decision" and state "decided"

  Scenario: Playbook artifact under playbooks/ is returned with type playbook
    Given a hearth directory with the following structure:
      | path                                              | state  |
      | playbooks/20260420T0000_track_lifecycle/           | draft  |
    When the filesystem hearth reader lists artifacts
    Then 1 artifacts are returned
    And the returned artifacts include "20260420T0000_track_lifecycle" with type "playbook" and state "draft"

  Scenario: The removed legacy workflows/ dir is not scanned by the reader
    Given a hearth directory with the following structure:
      | path                                              | state  |
      | workflows/20260420T0000_track_lifecycle/           | draft  |
    When the filesystem hearth reader lists artifacts
    Then 0 artifacts are returned

  Scenario: Missing hearth directory returns structured error
    Given a hearth path that does not exist
    When the filesystem hearth reader lists artifacts
    Then a hearth-not-found error is returned with the path

  Scenario: Malformed status.yaml returns structured error
    Given a hearth directory with a malformed status.yaml:
      | path                                  | content          |
      | proposals/20260403T1500_bad_proposal/  | not: valid: yaml: [ |
    When the filesystem hearth reader lists artifacts
    Then a malformed-status error is returned for "20260403T1500_bad_proposal"

  Scenario: Stateless-but-transitioned artifact resolves via its last transition without aborting the catalog
    Given a hearth directory with a malformed status.yaml:
      | path                                          | content                                                                              |
      | proposals/20260403T1500_well_formed/           | version: 1\nstate: active                                                            |
      | decisions/event-format-choice/                 | version: 1\nkind: decision\ntransitions:\n  - to: tension\n  - to: decided           |
    When the filesystem hearth reader lists artifacts
    Then 2 artifacts are returned
    And the returned artifacts include "20260403T1500_well_formed" with type "proposal" and state "active"
    And the returned artifacts include "event-format-choice" with type "decision" and state "decided"

  Scenario: Unresolvable artifact is surfaced as a degraded unknown entry without aborting the catalog
    Given a hearth directory with a malformed status.yaml:
      | path                                       | content        |
      | proposals/20260403T1500_well_formed/        | version: 1\nstate: active |
      | decisions/event-format-choice/              | version: 1     |
    When the filesystem hearth reader lists artifacts
    Then 2 artifacts are returned
    And the returned artifacts include "20260403T1500_well_formed" with type "proposal" and state "active"
    And the returned artifacts include "event-format-choice" with type "decision" and state "unknown"

  Scenario: Status.yaml missing state field is surfaced as a degraded unknown entry
    Given a hearth directory with a malformed status.yaml:
      | path                                      | content              |
      | proposals/20260403T1500_missing_state/     | version: 1           |
    When the filesystem hearth reader lists artifacts
    Then 1 artifacts are returned
    And the returned artifacts include "20260403T1500_missing_state" with type "proposal" and state "unknown"

  Scenario: A hand-authored activity entry missing state: does not sink the scan — the artifact is still served
    # FIELD-VERIFIED (foundry-hearth): hand-authored status.yaml activity
    # entries carry kind/actor/at/note but NO `state:`, so serde rejected the
    # WHOLE file with "missing field `state`". Because the catalog scans ALL
    # artifacts, that one file sank the entire scan and every governed operation
    # died engine-wide — fail-closed on a shared path. The malformed activity
    # entry is now dropped per-entry (with a stderr warning naming the field +
    # parse error); the artifact's real top-level state still resolves and it
    # stays visible in the catalog, alongside every healthy artifact.
    Given a hearth directory with a malformed status.yaml:
      | path                                          | content                                                                                                                                    |
      | tracks/20260403T1500_healthy_track/            | version: 1\nkind: track\nstate: spec                                                                                                       |
      | tracks/20260403T1500_missing_activity_state/   | version: 1\nkind: track\nstate: implementing\nactivity:\n  - kind: begin\n    actor: nick\n    at: "2026-01-01T00:00:00Z"\n    note: hand authored |
    When the filesystem hearth reader lists artifacts
    Then 2 artifacts are returned
    And the returned artifacts include "20260403T1500_healthy_track" with type "track" and state "spec"
    And the returned artifacts include "20260403T1500_missing_activity_state" with type "track" and state "implementing"
    # The malformed marker is ABSENT from the parsed log (not coerced to a
    # bogus empty-state entry) and the degradation is surfaced as exactly one
    # dropped entry with a diagnostic — a "truly empty log" would report zero.
    When the activity log for "20260403T1500_missing_activity_state" is read
    Then the activity log retains 0 entries
    And the activity log has no entry by actor "nick"
    And the activity log reports 1 dropped entries

  Scenario: A file mixing a healthy and a malformed activity entry degrades per-entry, not per-file
    # Per-entry degradation (the codebase's grain — activity: is a soft,
    # append-only, back-compat field): a status.yaml with one well-formed
    # begin-marker AND one hand-authored marker missing `state:` still parses.
    # The bad marker is dropped; the artifact keeps its resolved top-level state
    # and stays in the catalog. Only genuinely top-level-corrupt status.yaml
    # (unparseable YAML) remains a hard per-file error (see the scenario above).
    Given a hearth directory with a malformed status.yaml:
      | path                                    | content                                                                                                                                                                                                 |
      | tracks/20260403T1500_mixed_activity/     | version: 1\nkind: track\nstate: implementing\nactivity:\n  - kind: begin\n    actor: healthy-actor\n    state: implementing\n    at: "2026-01-02T00:00:00Z"\n  - kind: begin\n    actor: nick\n    at: "2026-01-01T00:00:00Z"\n    note: no state |
    When the filesystem hearth reader lists artifacts
    Then 1 artifacts are returned
    And the returned artifacts include "20260403T1500_mixed_activity" with type "track" and state "implementing"
    # The healthy marker is RETAINED (its content is visible via the reader's
    # activity log), the malformed one is ABSENT, and exactly one drop is
    # surfaced. A whole-field-discarding implementation would drop the healthy
    # marker too — retaining 0 entries — and FAIL this scenario.
    When the activity log for "20260403T1500_mixed_activity" is read
    Then the activity log retains 1 entries
    And the activity log retains a begin marker by "healthy-actor" in state "implementing"
    And the activity log has no entry by actor "nick"
    And the activity log reports 1 dropped entries

  Scenario: An activity entry with a wrong-typed field is dropped while its healthy sibling is retained
    # Boundary: a per-entry TYPE error (here `at: 5` where `at` is a string), not
    # just a missing field, must also degrade per-entry — the bad entry drops,
    # the well-formed one (with a unicode actor + note) survives intact.
    Given a hearth directory with a malformed status.yaml:
      | path                                  | content                                                                                                                                                                                            |
      | tracks/20260403T1500_wrong_type/       | version: 1\nkind: track\nstate: implementing\nactivity:\n  - kind: begin\n    actor: "café-señor-λ"\n    state: implementing\n    at: "2026-01-02T00:00:00Z"\n    note: "über naïve — 日本語"\n  - kind: begin\n    actor: bad\n    state: implementing\n    at: 5 |
    When the filesystem hearth reader lists artifacts
    Then 1 artifacts are returned
    And the returned artifacts include "20260403T1500_wrong_type" with type "track" and state "implementing"
    When the activity log for "20260403T1500_wrong_type" is read
    Then the activity log retains 1 entries
    And the activity log retains a begin marker by "café-señor-λ" in state "implementing"
    And the activity log has no entry by actor "bad"
    And the activity log reports 1 dropped entries

  Scenario: A null activity: field is a clean, non-degraded empty log
    # Boundary: `activity: null` (and an absent key) is "no markers", NOT a
    # degraded log — zero entries AND zero drops, so begin-adoption reads it as
    # a truly-empty log rather than treating it conservatively.
    Given a hearth directory with a malformed status.yaml:
      | path                              | content                                            |
      | tracks/20260403T1500_null_activity/ | version: 1\nkind: track\nstate: spec\nactivity: null |
    When the filesystem hearth reader lists artifacts
    Then 1 artifacts are returned
    And the returned artifacts include "20260403T1500_null_activity" with type "track" and state "spec"
    When the activity log for "20260403T1500_null_activity" is read
    Then the activity log retains 0 entries
    And the activity log reports 0 dropped entries

  Scenario: A scalar activity: field degrades to an empty log instead of sinking the scan
    # Boundary: `activity:` present but structurally wrong (a scalar, not a
    # list). A hard parse error here would re-sink the whole file — the exact
    # failure this fix prevents — so it degrades to an empty, DEGRADED log
    # (zero entries, one drop) and the artifact stays visible.
    Given a hearth directory with a malformed status.yaml:
      | path                                | content                                        |
      | tracks/20260403T1500_scalar_activity/ | version: 1\nkind: track\nstate: spec\nactivity: 5 |
    When the filesystem hearth reader lists artifacts
    Then 1 artifacts are returned
    And the returned artifacts include "20260403T1500_scalar_activity" with type "track" and state "spec"
    When the activity log for "20260403T1500_scalar_activity" is read
    Then the activity log retains 0 entries
    And the activity log reports 1 dropped entries

  Scenario: A status.yaml carrying both a self-id track: and a proposal: parent is read without choking
    # Regression: other hearths (e.g. temper) use a top-level `track:` self-id
    # alongside a `proposal:` parent. parent_id must NOT alias `track:`, else
    # serde rejects the file as a duplicate field and the whole scan aborts.
    Given a hearth directory with a malformed status.yaml:
      | path                              | content                                                                                  |
      | tracks/20260428T0433_dual_key/     | state: spec\ntrack: 20260428T0433_dual_key\nproposal: proposals/20260426T1800_parent\ntitle: Dual |
    When the filesystem hearth reader lists artifacts
    Then 1 artifacts are returned
    And the returned artifacts include "20260428T0433_dual_key" with type "track" and state "spec"
