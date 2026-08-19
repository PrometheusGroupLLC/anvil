Feature: Begin create flow returns spec-writing context with required sections
  Per spec R6 of the checkin_backfill_spec_context track, the
  `spec-writing.md` body carries a preamble + five body sections.
  `begin(artifact_type="track")` returns that body verbatim via
  `context_text`. As of the hook_content_serving track, the body is
  served as the playbook-declared `(spec, doer)` hook — resolved via the
  `PlaybookRegistry` and read via `QueryPort::read_playbook_hook_body` —
  not the static `read_context_file` path. The test seeds a facsimile of
  the real body under the hook path and asserts each section's distinctive
  heading reaches the caller.

  Scope: this scenario validates the port/adapter contract — whatever
  content is written as the `spec-writing.md` hook body flows through
  `begin`'s `context_text` verbatim. End-to-end verification that the real
  `anvil-hearth` ships the migrated body with the R6 headings is a later
  manual-check concern; this test deliberately uses a fixture so the
  core-level seam stays seam-appropriate and does not read files outside
  the test hearth.

  Scenario: begin create returns context_text with the spec-writing sections
    Given an in-memory query adapter with parent "20260411T2021_anvil_workflow_engine" in state "active"
    And the in-memory query adapter has spec-writing context with the full R6 body
    When begin is called via query adapter with parent "20260411T2021_anvil_workflow_engine" and a registry declaring spec doer hook "spec-writing.md"
    Then the begin outcome result context_text contains "Actor identity discipline"
    And the begin outcome result context_text contains "Pre-read list"
    And the begin outcome result context_text contains "Decisions-search step"
    And the begin outcome result context_text contains "After the spec is accepted"
    And the begin outcome result context_text contains "Commit convention"
    # Revision context now lives in spec-revision.md (Slice B), not the
    # spec-creation context. The create flow no longer carries "Revision mode".
    And the begin outcome result context_text does not contain "Revision mode"
