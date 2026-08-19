Feature: ReadInstanceArtifact RPC lets the Atlas live-agent artifact explorer read a file's full content
  `ListInstanceArtifacts` lists a live instance's files with a short (~500
  char) preview each — enough to show file chips, not enough to actually read
  an in-progress agent's real work. `ReadInstanceArtifact(instance_dir, name)`
  reads ONE named file's FULL content, so the Atlas can render it (as
  markdown, or monospace) and poll it to show the file updating live as the
  agent writes.

  Capped at 2 MiB: anything larger is truncated to exactly that many bytes and
  `truncated=true` is set, but `size_bytes` always reports the file's real
  on-disk size.

  FAIL-CLOSED on path escape: `instance_dir` is validated the SAME way
  `ListInstanceArtifacts` validates it (must resolve under a permitted hearth
  root), and `name` must be a single flat path component naming a file
  directly inside `instance_dir` — no `..`, no path separators, no absolute
  path. Either violation is refused (PermissionDenied). FAIL-OPEN on ordinary
  read trouble (missing file, unreadable): the response carries empty content
  and a non-empty `error_message`, never a hard error.

  Scenario: the gRPC ReadInstanceArtifact RPC reads a track file's full content
    Given a hearth with a track directory "20260710T0500_atlas_reader" containing files:
      | name    | content                                          |
      | spec.md | # Spec\n\nWhat and why, in full, not a preview. |
    And the engine is started with that hearth
    When the ReadInstanceArtifact RPC is called for that track directory and file "spec.md"
    Then the read_instance_artifact result has content "# Spec\n\nWhat and why, in full, not a preview."
    And the read_instance_artifact result is not truncated

  Scenario: a file over 2 MiB is truncated, honestly reporting its real size
    Given a hearth with a track directory "20260710T0600_atlas_reader" containing an oversized file "big.md" of 2200000 bytes
    And the engine is started with that hearth
    When the ReadInstanceArtifact RPC is called for that track directory and file "big.md"
    Then the read_instance_artifact result is truncated
    And the read_instance_artifact result content has length 2097152
    And the read_instance_artifact result size_bytes is 2200000

  Scenario: a name that escapes the instance directory is refused, fail-closed
    Given a hearth with a track directory "20260710T0700_atlas_reader" containing files:
      | name    | content |
      | spec.md | # Spec |
    And the engine is started with that hearth
    When the ReadInstanceArtifact RPC is called for that track directory and file "../secret.txt"
    Then the ReadInstanceArtifact RPC fails with permission_denied

  Scenario: a missing file in an otherwise permitted directory fails open with an honest error_message
    Given a hearth with a track directory "20260710T0800_atlas_reader" containing files:
      | name    | content |
      | spec.md | # Spec |
    And the engine is started with that hearth
    When the ReadInstanceArtifact RPC is called for that track directory and file "does_not_exist.md"
    Then the read_instance_artifact result has content ""
    And the read_instance_artifact result has a non-empty error_message

  Scenario: the /ws read_instance_artifact method mirrors the gRPC RPC, carrying every field
    Given a hearth with a track directory "20260710T0900_atlas_reader" containing files:
      | name    | content              |
      | plan.md | # Plan\n\nFull text. |
    And the engine is started with that hearth
    When a read_instance_artifact JSON-RPC request is sent over /ws for that track directory and file "plan.md"
    Then the /ws read_instance_artifact result has content "# Plan\n\nFull text."
    And the /ws read_instance_artifact result is not truncated
    And the /ws read_instance_artifact result carries name, size_bytes, modified_at, truncated, and error_message fields
