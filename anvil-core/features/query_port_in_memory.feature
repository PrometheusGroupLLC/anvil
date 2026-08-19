Feature: InMemoryQueryAdapter scaffolding
  InMemoryQueryAdapter implements QueryPort using HashMap-backed storage.
  Builder methods seed the adapter; read methods return seeded data or
  appropriate errors. This proves the port compiles and the adapter works
  without filesystem interactions.

  Scenario: read artifact kind and state from seeded adapter
    Given an in-memory query adapter seeded with artifact "track-001" kind "track" state "spec"
    When query_port.read_artifact_kind is called for "track-001"
    Then the query port returns kind "track"
    When query_port.read_artifact_state is called for "track-001"
    Then the query port returns state "spec"

  Scenario: read artifact text from seeded adapter
    Given an in-memory query adapter seeded with artifact "track-001" kind "track" state "spec"
    And the in-memory query adapter has artifact text for "tracks/track-001" filename "spec.md" content "# Spec content"
    When query_port.read_artifact_text is called for track "tracks/track-001" filename "spec.md"
    Then the query port returns artifact text containing "Spec content"

  Scenario: read context file from seeded adapter
    Given an in-memory query adapter seeded with context file "context/spec-writing.md" content "# Spec writing guide"
    When query_port.read_context_file is called for "context/spec-writing.md"
    Then the query port returns context text containing "Spec writing guide"

  Scenario: NotFound error for missing artifact
    Given an in-memory query adapter with no artifacts seeded
    When query_port.read_artifact_kind is called for "missing-artifact"
    Then the query port returns a NotFound error for "missing-artifact"

  Scenario: check_projection_row_unique returns Ok for default (unseeded) triple
    Given an in-memory query adapter seeded with artifact "track-001" kind "track" state "spec"
    When query_port.check_projection_row_unique is called for file "projections/execution.md" track "my-track" section "Spec"
    Then the query port projection check returns Ok

  Scenario: check_projection_row_unique returns error for 0 rows
    Given an in-memory query adapter seeded with projection row count for file "projections/execution.md" section "Spec" track "my-track" count 0
    When query_port.check_projection_row_unique is called for file "projections/execution.md" track "my-track" section "Spec"
    Then the query port projection check returns an error containing "No row matching"

  Scenario: check_projection_row_unique returns error for 2 rows
    Given an in-memory query adapter seeded with projection row count for file "projections/execution.md" section "Spec" track "my-track" count 2
    When query_port.check_projection_row_unique is called for file "projections/execution.md" track "my-track" section "Spec"
    Then the query port projection check returns an error containing "duplicate-name collision"
