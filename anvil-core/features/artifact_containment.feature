Feature: Artifact-path containment at the engine filesystem boundary
  The engine must not trust that the MCP shim already validated `artifact_path`.
  A path is checked again at the fs adapters that join it under the hearth, so a
  `..` traversal, a drive-relative / rooted form, or an in-hearth symlink that
  points OUT of the hearth can never write outside the hearth subtree — even if
  the shim's normalization was bypassed. Path-independent syntax is refused on
  its own, before any hearth is resolved.

  Scenario: A parent-traversal path is syntactically invalid
    Then the artifact_path "../alpha/tracks/x" is syntactically invalid

  Scenario: A Windows drive-relative path is syntactically invalid
    Then the artifact_path "C:tracks\evil" is syntactically invalid

  Scenario: A rooted-relative path is syntactically invalid
    Then the artifact_path "\evil" is syntactically invalid

  Scenario: A legitimate hearth-relative path is syntactically valid
    Then the artifact_path "tracks/20260420T0100_contain_track" is syntactically valid

  Scenario: A contained path normalizes to its hearth-relative form
    Given a containment hearth with a track at "spec"
    Then the artifact_path "tracks/20260420T0100_contain_track" is contained and normalizes to "tracks/20260420T0100_contain_track"

  Scenario: An in-hearth symlink pointing outside the hearth escapes containment
    Given a containment hearth with a track at "spec"
    And a symlink "escape-link" inside the hearth pointing outside it
    Then the artifact_path "escape-link/tracks/20260420T0100_contain_track" escapes the hearth

  Scenario: The engine fs write refuses a symlink-out-of-hearth artifact_path
    Given a containment hearth with a track at "spec"
    And a symlink "escape-link" inside the hearth pointing outside it
    When a transition to "spec_review" is recorded for artifact_path "escape-link/tracks/20260420T0100_contain_track"
    Then the transition write is refused

  Scenario: The engine fs write records a contained artifact_path
    Given a containment hearth with a track at "spec"
    When a transition to "spec_review" is recorded for artifact_path "tracks/20260420T0100_contain_track"
    Then the transition write succeeds
