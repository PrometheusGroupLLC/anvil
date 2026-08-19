Feature: FileSystemQueryAdapter check_projection_row_unique fidelity
  Verifies that check_projection_row_unique in FileSystemQueryAdapter
  produces byte-identical error messages to the original
  fs_begin_adapter.rs:437-449 implementation. This catches verbatim-lift
  drift at Phase 1 rather than as a Phase 3 feature regression.

  The InMemoryQueryAdapter mirrors the same message format — these
  scenarios exercise the FS adapter against a real fixture file to
  confirm the messages are byte-identical to the source.

  Scenario: 0-row case produces "No row matching" message
    Given a hearth fixture with a projection file containing 0 rows for track "my-track" in section "Spec"
    When fs_query_adapter.check_projection_row_unique is called for file "projections/execution.md" track "my-track" section "Spec"
    Then the fs query adapter returns an error with message "No row matching 'my-track' in 'Spec' section of projections/execution.md"

  Scenario: 2-row case produces "duplicate-name collision" message
    Given a hearth fixture with a projection file containing 2 rows for track "my-track" in section "Spec"
    When fs_query_adapter.check_projection_row_unique is called for file "projections/execution.md" track "my-track" section "Spec"
    Then the fs query adapter returns an error with message "2 rows matching 'my-track' in 'Spec' section of projections/execution.md — projection has a duplicate-name collision; dedupe or use distinct track names before retrying"
