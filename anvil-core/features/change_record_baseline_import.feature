Feature: Change record baseline import

  Replay from empty is unattainable for governance state that predates the
  mechanism, so the honest starting point is a named baseline whose tree equals
  the hearth's recorded state at import time and which replays byte-for-byte.

  The seam is anvil-core and the user is the engine: import is a library call
  against a real temporary git repository, with no subprocess, no port and no
  tonic.

  Scenario: Baseline import initializes a hearth that is not a git repository
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-import | spec  |
    When baseline import is run for that hearth
    Then the repository has exactly 1 baseline ref
    And the baseline tree contains hearth path "tracks/t-import/status.yaml"

  Scenario: Baseline import records every governance path in the recorded set
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-import | spec  |
    And a file exists at hearth path "tracks/t-import/transitions/20260814T1200_spec.yaml" with content "event_type: spec"
    And a file exists at hearth path "tracks/t-import/spec.md" with content "# Spec"
    And a file exists at hearth path "tracks/t-import/spec_reflection/notes.md" with content "reflection"
    And a file exists at hearth path "tracks/t-import/carry-forward.md" with content "findings"
    And a file exists at hearth path "projections/execution.md" with content "# Execution"
    And a file exists at hearth path "backlog_items/bi-1/.transactions/prepared.yaml" with content "phase: prepared"
    And the tracks registry contains:
      """
      # Tracks

      - t-import
      """
    When baseline import is run for that hearth
    Then the baseline tree contains hearth path "tracks/t-import/status.yaml"
    And the baseline tree contains hearth path "tracks/t-import/transitions/20260814T1200_spec.yaml"
    And the baseline tree contains hearth path "tracks/t-import/spec.md"
    And the baseline tree contains hearth path "tracks/t-import/spec_reflection/notes.md"
    And the baseline tree contains hearth path "tracks/t-import/carry-forward.md"
    And the baseline tree contains hearth path "projections/execution.md"
    And the baseline tree contains hearth path "backlog_items/bi-1/.transactions/prepared.yaml"
    And the baseline tree contains hearth path "tracks.md"

  Scenario: Baseline import records no excluded sink path
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-import | spec  |
    And a file exists at hearth path "activity-log.jsonl" with content "{}"
    And a file exists at hearth path "change-record.jsonl" with content "{}"
    And a file exists at hearth path "abstentions/ledger.jsonl" with content "{}"
    And a file exists at hearth path ".telemetry-salt" with content "deployment-salt"
    And a file exists at hearth path "engine-flags.env" with content "ANVIL_CHANGE_RECORD_SHADOW=on"
    And a file exists at hearth path "research/notes.md" with content "human notes"
    When baseline import is run for that hearth
    Then the baseline tree contains hearth path "tracks/t-import/status.yaml"
    And the baseline tree does not contain hearth path "activity-log.jsonl"
    And the baseline tree does not contain hearth path "change-record.jsonl"
    And the baseline tree does not contain hearth path "abstentions/ledger.jsonl"
    And the baseline tree does not contain hearth path ".telemetry-salt"
    And the baseline tree does not contain hearth path "engine-flags.env"
    And the baseline tree does not contain hearth path "research/notes.md"

  Scenario: Replay from baseline reproduces the governance tree byte-for-byte
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-import | spec  |
    And a file exists at hearth path "tracks/t-import/spec.md" with content "# Spec, no trailing newline"
    And a file exists at hearth path "tracks/t-import/notes.md" with content "  leading and trailing spaces  "
    And a file exists at hearth path "projections/execution.md" with content "b: 2"
    When baseline import is run for that hearth
    Then replay from the baseline reproduces every recorded path byte-for-byte

  Scenario: Re-running baseline import establishes no second baseline
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-import | spec  |
    When baseline import is run for that hearth
    And baseline import is run for that hearth
    Then the repository has exactly 1 baseline ref
    And the change-record ref has exactly 1 commit
    And the baseline ref names the commit the first import established

  Scenario: Baseline import leaves HEAD, the index and a dirty working tree untouched
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-import | spec  |
    And the hearth is an existing git repository with a committed file and two uncommitted modifications
    When baseline import is run for that hearth
    Then HEAD, the index and the working tree are unchanged
    And the repository has exactly 1 baseline ref

  Scenario: Baseline import refuses a hearth nested inside a foreign repository
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-import | spec  |
    And the hearth is a subdirectory of an outer git repository
    When baseline import is run for that hearth
    Then baseline import refuses naming the enclosing repository
