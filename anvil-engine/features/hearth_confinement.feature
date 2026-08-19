Feature: Non-hearth and traversal paths are rejected, never scaffolded
  A client-supplied hearth path is canonicalized (resolving ../symlinks) and
  validated against the hearth predicate — a directory containing tracks/ and a
  tracks.md registry — before use (spec Req 4, AC "Confinement"). A path that is
  not an actual hearth, or a traversal path, is rejected with a clear error and
  never scaffolded.

  Scenario: A non-hearth directory is rejected
    Given two hearth directories X and Y each with the standard structure
    When the catalog RPC is called for hearth "/tmp"
    Then the catalog RPC returns gRPC error containing "hearth"

  Scenario: A non-existent / traversal path is rejected
    Given two hearth directories X and Y each with the standard structure
    When the catalog RPC is called for hearth "/tmp/../tmp/anvil-definitely-not-a-hearth-xyz"
    Then the catalog RPC returns gRPC error containing "hearth"

  Scenario: Empty request hearth without a default is rejected with self-heal guidance
    Given a standard hearth directory X and a hearth-less engine
    When the catalog RPC is called with no hearth_path
    Then the catalog RPC returns gRPC status "FAILED_PRECONDITION"
    And the catalog RPC error message contains "pass hearth_path"

  Scenario: Empty request hearth uses the default hearth when present
    Given a standard hearth directory X and an engine started with it as the default hearth
    When the catalog RPC is called
    Then the catalog RPC response resolved_hearth is the hearth "<hearth_x>"
    And the catalog response includes artifact "20260419T1100_track_x" with type "track"

  Scenario: A hearth under a permitted root is accepted
    Given a permitted root containing standard hearth X and outside standard hearth Y
    When the catalog RPC is called for hearth "X"
    Then the catalog RPC response resolved_hearth is the hearth "<hearth_x>"
    And the catalog response includes artifact "20260419T1100_track_x" with type "track"

  Scenario: A hearth outside the permitted root is denied
    Given a permitted root containing standard hearth X and outside standard hearth Y
    When the catalog RPC is called for hearth "Y"
    Then the catalog RPC returns gRPC error containing "hearth_not_permitted"

  Scenario: A parent-directory escape outside the permitted root is denied
    Given a permitted root containing standard hearth X and outside standard hearth Y
    When the catalog RPC is called for the parent escape to hearth Y
    Then the catalog RPC returns gRPC error containing "hearth_not_permitted"

  Scenario: A symlink escape outside the permitted root is denied
    Given a permitted root containing standard hearth X and a symlink inside it to outside hearth Y
    When the catalog RPC is called for hearth symlink "Y"
    Then the catalog RPC returns gRPC error containing "hearth_not_permitted"

  Scenario: The default hearth is implicitly permitted
    Given standard hearth X is the default and outside any configured permitted root
    When the catalog RPC is called
    Then the catalog RPC response resolved_hearth is the hearth "<hearth_x>"
    And the catalog response includes artifact "20260419T1100_track_x" with type "track"
