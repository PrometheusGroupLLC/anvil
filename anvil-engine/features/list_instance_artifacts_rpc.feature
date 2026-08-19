Feature: ListInstanceArtifacts RPC lets the Atlas live panel explore a live instance's on-disk artifacts
  The Atlas live panel already shows each live instance's `artifact_dir` (the
  on-disk track directory) but gives no way to see the actual work in
  progress. `ListInstanceArtifacts(instance_dir)` reads that directory
  read-only and returns each file's name, size, and a short preview, so the
  user can explore an in-progress agent's real artifacts (spec.md, plan.md,
  reflection.md, *.review.md, status.yaml) without leaving the Atlas.

  FAIL-CLOSED on path escape: `instance_dir` must resolve under one of the
  engine's permitted hearth roots (the SAME `HearthPolicy` every other query
  enforces), or the request is refused — this RPC must never become a way to
  read arbitrary filesystem paths. FAIL-OPEN on ordinary read trouble: an
  unreadable (but permitted) directory yields an empty list and an honest
  `error_message`, never a hard error.

  Scenario: the gRPC ListInstanceArtifacts RPC lists a track's real files with size and preview
    Given a hearth with a track directory "20260710T0100_atlas_explorer" containing files:
      | name           | content                                  |
      | spec.md        | # Spec\n\nWhat and why.                  |
      | plan.md        | # Plan\n\nPhased tasks.                  |
      | status.yaml    | version: 1\nkind: track\nstate: implementing\n |
    And the engine is started with that hearth
    When the ListInstanceArtifacts RPC is called for that track directory
    Then the list_instance_artifacts result has 3 artifacts
    And the list_instance_artifacts result artifact "spec.md" has preview containing "What and why"
    And the list_instance_artifacts result artifact "plan.md" has size greater than 0

  Scenario: a directory outside every permitted root is refused, fail-closed
    Given a hearth with a track directory "20260710T0200_atlas_explorer" containing files:
      | name    | content       |
      | spec.md | # Spec       |
    And the engine is started with that hearth
    And an unrelated directory outside any permitted root containing a file "secret.txt"
    When the ListInstanceArtifacts RPC is called for the unrelated directory
    Then the ListInstanceArtifacts RPC fails with permission_denied

  Scenario: a missing directory is refused with an honest invalid_argument, not a crash
    Given a hearth with a track directory "20260710T0300_atlas_explorer" containing files:
      | name    | content |
      | spec.md | # Spec |
    And the engine is started with that hearth
    When the ListInstanceArtifacts RPC is called for instance_dir "<hearth>/tracks/does_not_exist"
    Then the ListInstanceArtifacts RPC fails with invalid_argument

  Scenario: the /ws list_instance_artifacts method mirrors the gRPC RPC
    Given a hearth with a track directory "20260710T0400_atlas_explorer" containing files:
      | name    | content        |
      | spec.md | # Spec content |
    And the engine is started with that hearth
    When a list_instance_artifacts JSON-RPC request is sent over /ws for that track directory
    Then the /ws list_instance_artifacts result has 1 artifacts
    And the /ws list_instance_artifacts result artifact "spec.md" has preview containing "Spec content"
