Feature: anvil-hooks artifact-of-record — the Rust/Python boundary for the sweep
  The open-step sweep is Python, living with the reliability watchers; the
  authority for "which file is this state's artifact of record, and what does an
  untouched one look like" is Rust, in anvil-core. Rather than reimplement that
  in Python — which is exactly how a placeholder identity drifts from the writer
  that produces it — the sweep shells out to this subcommand.

  It emits a SHA-256, not the bytes, so the placeholder content is never
  duplicated on the Python side.

  Every scenario runs the REAL BINARY. A unit call would prove the function; only
  running the binary proves the subcommand the sweep will actually invoke.

  Scenario: a track's initial state reports its artifact and placeholder digest
    When anvil-hooks artifact-of-record runs for kind "track" state "spec" name "Alpha Track"
    Then the artifact-of-record output contains "spec.md"
    And the artifact-of-record output contains "placeholder_sha256"

  # Not an error. "This kind has no single artifact of record" is a legitimate,
  # common answer, and making it an error would push the sweep into treating an
  # expected case as a failure.
  Scenario: a multi-file kind reports not applicable
    When anvil-hooks artifact-of-record runs for kind "playbook_generation" state "gathering" name "X"
    Then the artifact-of-record output contains "\"applicable\":false"

  Scenario: a non-initial state reports not applicable
    When anvil-hooks artifact-of-record runs for kind "track" state "spec_review" name "X"
    Then the artifact-of-record output contains "\"applicable\":false"

  # The property the sweep depends on: the digest is per-ARTIFACT, not per-kind.
  # A stubbed implementation returning a constant would pass every scenario above
  # and fail this one.
  Scenario: the placeholder digest tracks the display name
    Then the artifact-of-record hash differs for name "One" versus "Two"
